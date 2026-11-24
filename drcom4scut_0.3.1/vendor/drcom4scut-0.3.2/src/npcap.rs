//! Windows-only Npcap capture channel with kernel cBPF and event-driven reads.
//!
//! The production path keeps `pnet::datalink`'s public types but talks to the
//! loaded `Packet.dll` directly: an outer-EAPOL/destination-MAC cBPF runs in
//! the driver, the receive thread waits on the Npcap read event instead of
//! issuing empty reads, and a wake event releases the thread on stop/quit.
#![allow(non_camel_case_types, non_snake_case, dead_code)]

use std::collections::VecDeque;
use std::ffi::{CString, c_char, c_int, c_void};
use std::io;
use std::mem;
use std::ptr;
use std::slice;
use std::sync::{Arc, Mutex, OnceLock, Weak};

use log::{debug, warn};
use pnet::datalink::{Channel, DataLinkReceiver, DataLinkSender, MacAddr, NetworkInterface};

type LPADAPTER = *mut c_void;
type LPPACKET = *mut c_void;
type PVOID = *mut c_void;
type HANDLE = isize;

const NDIS_PACKET_TYPE_DIRECTED: u32 = 0x0000_0001;
const NDIS_PACKET_TYPE_MULTICAST: u32 = 0x0000_0002;
const NDIS_PACKET_TYPE_PROMISCUOUS: u32 = 0x0000_0020;
const NPF_DISABLE_LOOPBACK: u32 = 1;

const WAIT_OBJECT_0: u32 = 0;
const WAIT_TIMEOUT: u32 = 258;

const READ_BUFFER_SIZE: usize = 65_536;
const WRITE_BUFFER_SIZE: usize = 4_096;
const READ_POLL_MS: u32 = 250;
const EVENT_WAIT_MS: u32 = 500;

const BPF_LD: u16 = 0x00;
const BPF_JMP: u16 = 0x05;
const BPF_RET: u16 = 0x06;
const BPF_W: u16 = 0x00;
const BPF_H: u16 = 0x08;
const BPF_B: u16 = 0x10;
const BPF_ABS: u16 = 0x20;
const BPF_JA: u16 = 0x00;
const BPF_JEQ: u16 = 0x10;
const BPF_K: u16 = 0x00;

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BpfInsn {
    pub code: u16,
    pub jt: u8,
    pub jf: u8,
    pub k: u32,
}

#[repr(C)]
pub struct BpfProgram {
    pub bf_len: u32,
    pub bf_insns: *const BpfInsn,
}

#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
struct BpfHeader {
    tv_sec: u32,
    tv_usec: u32,
    caplen: u32,
    datalen: u32,
    hdrlen: u16,
    _padding: u16,
}

#[repr(C)]
struct Packet {
    h_event: HANDLE,
    overlapped: [u8; 32],
    buffer: PVOID,
    length: u32,
    bytes_received: u32,
    io_complete: u8,
}

type LoopbackFn = unsafe extern "C" fn(LPADAPTER, u32) -> u8;

#[link(name = "Packet")]
unsafe extern "C" {
    fn PacketOpenAdapter(adapter_name: *mut c_char) -> LPADAPTER;
    fn PacketCloseAdapter(adapter: LPADAPTER);
    fn PacketSetHwFilter(adapter: LPADAPTER, filter: u32) -> u8;
    fn PacketSetBuff(adapter: LPADAPTER, size: c_int) -> u8;
    fn PacketSetMinToCopy(adapter: LPADAPTER, size: c_int) -> u8;
    fn PacketSetReadTimeout(adapter: LPADAPTER, timeout: c_int) -> u8;
    fn PacketSetBpf(adapter: LPADAPTER, program: *const BpfProgram) -> u8;
    fn PacketGetReadEvent(adapter: LPADAPTER) -> HANDLE;
    fn PacketAllocatePacket() -> LPPACKET;
    fn PacketInitPacket(packet: LPPACKET, buffer: PVOID, length: u32);
    fn PacketFreePacket(packet: LPPACKET);
    fn PacketReceivePacket(adapter: LPADAPTER, packet: LPPACKET, sync: u8) -> u8;
    fn PacketSendPacket(adapter: LPADAPTER, packet: LPPACKET, sync: u8) -> u8;
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreateEventW(
        attributes: *const c_void,
        manual_reset: i32,
        initial_state: i32,
        name: *const u16,
    ) -> HANDLE;
    fn SetEvent(handle: HANDLE) -> i32;
    fn ResetEvent(handle: HANDLE) -> i32;
    fn WaitForMultipleObjects(count: u32, handles: *const HANDLE, wait_all: i32, ms: u32) -> u32;
    fn CloseHandle(handle: HANDLE) -> i32;
    fn GetModuleHandleA(name: *const u8) -> *mut c_void;
    fn GetProcAddress(module: *mut c_void, name: *const u8) -> *mut c_void;
}

struct Handle(HANDLE);

impl Handle {
    fn create(manual_reset: bool) -> io::Result<Self> {
        let raw = unsafe { CreateEventW(ptr::null(), i32::from(manual_reset), 0, ptr::null()) };
        if raw == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(Self(raw))
        }
    }
    fn raw(&self) -> HANDLE {
        self.0
    }
    fn set(&self) {
        unsafe {
            SetEvent(self.0);
        }
    }
    fn reset(&self) {
        unsafe {
            ResetEvent(self.0);
        }
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

/// Accept only outer EAPOL frames addressed to `mac`; reject VLAN and all
/// ordinary traffic before the driver copies the frame to user space.
pub fn eapol_for_mac_bpf(mac: [u8; 6]) -> [BpfInsn; 8] {
    [
        BpfInsn {
            code: BPF_LD | BPF_H | BPF_ABS,
            jt: 0,
            jf: 0,
            k: 12,
        },
        BpfInsn {
            code: BPF_JMP | BPF_JEQ | BPF_K,
            jt: 0,
            jf: 5,
            k: 0x0000_888e,
        },
        BpfInsn {
            code: BPF_LD | BPF_W | BPF_ABS,
            jt: 0,
            jf: 0,
            k: 0,
        },
        BpfInsn {
            code: BPF_JMP | BPF_JEQ | BPF_K,
            jt: 0,
            jf: 3,
            k: u32::from_be_bytes([mac[0], mac[1], mac[2], mac[3]]),
        },
        BpfInsn {
            code: BPF_LD | BPF_H | BPF_ABS,
            jt: 0,
            jf: 0,
            k: 4,
        },
        BpfInsn {
            code: BPF_JMP | BPF_JEQ | BPF_K,
            jt: 0,
            jf: 1,
            k: u32::from(u16::from_be_bytes([mac[4], mac[5]])),
        },
        BpfInsn {
            code: BPF_RET | BPF_K,
            jt: 0,
            jf: 0,
            k: 0x0000_ffff,
        },
        BpfInsn {
            code: BPF_RET | BPF_K,
            jt: 0,
            jf: 0,
            k: 0x0000_0000,
        },
    ]
}

/// Small classic-BPF interpreter used by unit tests and offline validation.
/// Only the instructions emitted by this module are accepted; anything else
/// drops the frame rather than guessing at the driver's semantics.
pub fn run_bpf(program: &[BpfInsn], frame: &[u8]) -> u32 {
    let mut accumulator: u32 = 0;
    let mut pc = 0usize;
    for _ in 0..=program.len() * 2 + 1 {
        let Some(insn) = program.get(pc) else {
            return 0;
        };
        match insn.code {
            code if code == BPF_LD | BPF_H | BPF_ABS => {
                let start = insn.k as usize;
                let Some(end) = start.checked_add(2) else {
                    return 0;
                };
                let Some(bytes) = frame.get(start..end) else {
                    return 0;
                };
                accumulator = u32::from(u16::from_be_bytes([bytes[0], bytes[1]]));
                pc += 1;
            }
            code if code == BPF_LD | BPF_W | BPF_ABS => {
                let start = insn.k as usize;
                let Some(end) = start.checked_add(4) else {
                    return 0;
                };
                let Some(bytes) = frame.get(start..end) else {
                    return 0;
                };
                accumulator = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
                pc += 1;
            }
            code if code == BPF_LD | BPF_B | BPF_ABS => {
                let Some(byte) = frame.get(insn.k as usize) else {
                    return 0;
                };
                accumulator = u32::from(*byte);
                pc += 1;
            }
            code if code == BPF_JMP | BPF_JEQ | BPF_K => {
                pc += if accumulator == insn.k {
                    1 + usize::from(insn.jt)
                } else {
                    1 + usize::from(insn.jf)
                };
            }
            code if code == BPF_JA => {
                pc += 1 + insn.k as usize;
            }
            code if code == BPF_RET | BPF_K => return insn.k,
            _ => return 0,
        }
    }
    0
}

#[inline]
fn word_align(value: usize) -> usize {
    value.wrapping_add(3) & !3
}

/// Decode the `bpf_hdr` records returned by one `PacketReceivePacket` call.
/// Any truncated or impossible record ends parsing; no record may point or
/// step outside the buffer the driver filled.
fn decode_records(buffer: &[u8], received: usize) -> Vec<(usize, usize)> {
    let mut frames = Vec::new();
    let received = received.min(buffer.len());
    let header_size = mem::size_of::<BpfHeader>();
    let mut offset = 0usize;
    while offset
        .checked_add(header_size)
        .is_some_and(|end| end <= received)
    {
        // The buffer comes from the driver; read only the fixed header first.
        let header =
            unsafe { ptr::read_unaligned(buffer.as_ptr().add(offset) as *const BpfHeader) };
        let hdrlen = header.hdrlen as usize;
        let caplen = header.caplen as usize;
        if hdrlen < header_size || offset + hdrlen > received {
            break;
        }
        if caplen > received - offset - hdrlen {
            break;
        }
        frames.push((offset + hdrlen, caplen));
        let step = word_align(hdrlen + caplen);
        if step == 0 || step > received - offset {
            break;
        }
        offset += step;
    }
    frames
}

fn loopback_function() -> Option<LoopbackFn> {
    static FUNCTION: OnceLock<Option<LoopbackFn>> = OnceLock::new();
    *FUNCTION.get_or_init(|| unsafe {
        let module = GetModuleHandleA(b"Packet.dll\0".as_ptr());
        if module.is_null() {
            return None;
        }
        let address = GetProcAddress(module, b"PacketSetLoopbackBehavior\0".as_ptr());
        if address.is_null() {
            None
        } else {
            Some(mem::transmute::<*mut c_void, LoopbackFn>(address))
        }
    })
}

fn disable_loopback(adapter: LPADAPTER) {
    match loopback_function() {
        Some(function) => unsafe {
            if function(adapter, NPF_DISABLE_LOOPBACK) == 0 {
                debug!("PacketSetLoopbackBehavior failed; keeping the driver default");
            }
        },
        None => debug!("PacketSetLoopbackBehavior is unavailable; keeping the driver default"),
    }
}

fn install_bpf(adapter: LPADAPTER, mac: [u8; 6]) -> io::Result<()> {
    let program = eapol_for_mac_bpf(mac);
    let block = BpfProgram {
        bf_len: program.len() as u32,
        bf_insns: program.as_ptr(),
    };
    if unsafe { PacketSetBpf(adapter, &block) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn set_read_timeout(adapter: LPADAPTER, timeout: c_int) -> io::Result<()> {
    if unsafe { PacketSetReadTimeout(adapter, timeout) } == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

struct Adapter {
    raw: LPADAPTER,
}

impl Drop for Adapter {
    fn drop(&mut self) {
        unsafe {
            PacketCloseAdapter(self.raw);
        }
    }
}

unsafe impl Send for Adapter {}
unsafe impl Sync for Adapter {}

struct PacketHandle {
    raw: LPPACKET,
}

impl Drop for PacketHandle {
    fn drop(&mut self) {
        unsafe {
            PacketFreePacket(self.raw);
        }
    }
}

unsafe impl Send for PacketHandle {}
unsafe impl Sync for PacketHandle {}

struct Sender {
    adapter: Arc<Adapter>,
    buffer: Vec<u8>,
    packet: PacketHandle,
}

impl DataLinkSender for Sender {
    fn build_and_send(
        &mut self,
        num_packets: usize,
        packet_size: usize,
        func: &mut dyn FnMut(&mut [u8]),
    ) -> Option<io::Result<()>> {
        if packet_size == 0 || num_packets == 0 {
            return Some(Ok(()));
        }
        let total = num_packets.checked_mul(packet_size)?;
        if total > self.buffer.len() {
            return None;
        }
        for packet in self.buffer[..total].chunks_mut(packet_size) {
            func(packet);
            unsafe {
                PacketInitPacket(
                    self.packet.raw,
                    packet.as_mut_ptr() as PVOID,
                    packet_size as u32,
                );
            }
            if unsafe { PacketSendPacket(self.adapter.raw, self.packet.raw, 0) } == 0 {
                return Some(Err(io::Error::last_os_error()));
            }
        }
        Some(Ok(()))
    }

    fn send_to(&mut self, packet: &[u8], _dst: Option<NetworkInterface>) -> Option<io::Result<()>> {
        self.build_and_send(1, packet.len(), &mut |buffer: &mut [u8]| {
            buffer.copy_from_slice(packet);
        })
    }
}

unsafe impl Send for Sender {}
unsafe impl Sync for Sender {}

struct WakeHandle {
    event: Handle,
}

impl WakeHandle {
    fn new() -> io::Result<Arc<Self>> {
        Ok(Arc::new(Self {
            event: Handle::create(true)?,
        }))
    }
    fn wake(&self) {
        self.event.set();
    }
}

static WAKE_HANDLES: Mutex<Vec<Weak<WakeHandle>>> = Mutex::new(Vec::new());

/// Wake the receive thread if it is waiting on the driver read event.
pub fn wake_receivers() {
    let handles = WAKE_HANDLES
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .filter_map(Weak::upgrade)
        .collect::<Vec<_>>();
    for handle in handles {
        handle.wake();
    }
}

struct Receiver {
    adapter: Arc<Adapter>,
    buffer: Vec<u8>,
    packet: PacketHandle,
    frames: VecDeque<(usize, usize)>,
    read_event: Option<HANDLE>,
    event_driven: bool,
    wake: Arc<WakeHandle>,
}

impl Receiver {
    /// Wait for data or the stop wake event. A timeout never calls
    /// `PacketReceivePacket`.
    fn wait(&mut self) -> io::Result<bool> {
        if !self.event_driven {
            return Ok(true);
        }
        let Some(read_event) = self.read_event else {
            return Ok(true);
        };
        let handles = [read_event, self.wake.event.raw()];
        match unsafe { WaitForMultipleObjects(2, handles.as_ptr(), 0, EVENT_WAIT_MS) } {
            WAIT_OBJECT_0 => Ok(true),
            value if value == WAIT_OBJECT_0 + 1 => {
                self.wake.event.reset();
                Err(io::Error::from(io::ErrorKind::Interrupted))
            }
            WAIT_TIMEOUT => Ok(false),
            _ => Err(io::Error::last_os_error()),
        }
    }

    fn read_once(&mut self) -> io::Result<bool> {
        let result = unsafe { PacketReceivePacket(self.adapter.raw, self.packet.raw, 0) };
        if result == 0 {
            return Err(io::Error::last_os_error());
        }
        let received = unsafe { (*(self.packet.raw as *const Packet)).bytes_received as usize };
        if received == 0 {
            if let Some(event) = self.read_event {
                unsafe {
                    ResetEvent(event);
                }
            }
            return Ok(false);
        }
        let decoded = decode_records(&self.buffer, received);
        self.frames.extend(decoded.iter().copied());
        Ok(!decoded.is_empty())
    }
}

impl DataLinkReceiver for Receiver {
    fn next(&mut self) -> io::Result<&[u8]> {
        loop {
            if let Some((start, len)) = self.frames.pop_front() {
                let data = unsafe { self.buffer.as_ptr().add(start) };
                return Ok(unsafe { slice::from_raw_parts(data, len) });
            }
            let should_read = self.wait()?;
            if should_read && self.read_once()? {
                continue;
            }
        }
    }
}

unsafe impl Send for Receiver {}
unsafe impl Sync for Receiver {}

/// Open the production capture channel: kernel EAPOL cBPF, no loopback, large
/// buffer, and event-driven reads. If the driver rejects the BPF program the
/// channel stays usable with the legacy 250 ms read and the user-space gate.
pub fn open_channel(interface: &NetworkInterface, mac: MacAddr) -> io::Result<Channel> {
    let name = CString::new(interface.name.as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "interface name has NUL"))?;
    let raw = unsafe { PacketOpenAdapter(name.as_ptr() as *mut c_char) };
    if raw.is_null() {
        return Err(io::Error::last_os_error());
    }
    let adapter = Arc::new(Adapter { raw });

    // Prefer hardware directed/multicast filtering; fall back to the old
    // promiscuous mode only when the adapter rejects the narrower filter.
    if unsafe {
        PacketSetHwFilter(
            adapter.raw,
            NDIS_PACKET_TYPE_DIRECTED | NDIS_PACKET_TYPE_MULTICAST,
        )
    } == 0
    {
        let error = io::Error::last_os_error();
        warn!(
            "PacketSetHwFilter directed|multicast failed on {}: {error}; using promiscuous mode",
            interface.name
        );
        if unsafe { PacketSetHwFilter(adapter.raw, NDIS_PACKET_TYPE_PROMISCUOUS) } == 0 {
            return Err(io::Error::last_os_error());
        }
    }
    if unsafe { PacketSetBuff(adapter.raw, READ_BUFFER_SIZE as c_int) } == 0 {
        return Err(io::Error::last_os_error());
    }
    if unsafe { PacketSetMinToCopy(adapter.raw, 1) } == 0 {
        return Err(io::Error::last_os_error());
    }

    let read_event = unsafe { PacketGetReadEvent(adapter.raw) };
    let read_event = (read_event != 0).then_some(read_event);
    let kernel_filter = match install_bpf(adapter.raw, [mac.0, mac.1, mac.2, mac.3, mac.4, mac.5]) {
        Ok(()) => {
            debug!("Installed outer-EAPOL kernel filter on {}", interface.name);
            disable_loopback(adapter.raw);
            true
        }
        Err(err) => {
            warn!(
                "PacketSetBpf was rejected on {}: {err}; falling back to the 250 ms read with user-space EAPOL filtering",
                interface.name
            );
            false
        }
    };

    let mut event_driven = false;
    if kernel_filter {
        if read_event.is_some() {
            match set_read_timeout(adapter.raw, -1) {
                Ok(()) => event_driven = true,
                Err(err) => warn!(
                    "PacketSetReadTimeout(-1) failed on {}: {err}; falling back to 250 ms reads",
                    interface.name
                ),
            }
        } else {
            warn!(
                "PacketGetReadEvent is unavailable on {}; falling back to 250 ms reads",
                interface.name
            );
        }
    }
    if !event_driven {
        set_read_timeout(adapter.raw, READ_POLL_MS as c_int).map_err(|err| {
            io::Error::other(format!(
                "PacketSetReadTimeout({READ_POLL_MS}) failed on {}: {err}",
                interface.name
            ))
        })?;
    }

    let mut read_buffer = vec![0u8; READ_BUFFER_SIZE];
    let read_packet = unsafe { PacketAllocatePacket() };
    if read_packet.is_null() {
        return Err(io::Error::last_os_error());
    }
    let read_packet = PacketHandle { raw: read_packet };
    unsafe {
        PacketInitPacket(
            read_packet.raw,
            read_buffer.as_mut_ptr() as PVOID,
            READ_BUFFER_SIZE as u32,
        );
    }

    let mut write_buffer = vec![0u8; WRITE_BUFFER_SIZE];
    let write_packet = unsafe { PacketAllocatePacket() };
    if write_packet.is_null() {
        return Err(io::Error::last_os_error());
    }
    let write_packet = PacketHandle { raw: write_packet };
    unsafe {
        PacketInitPacket(
            write_packet.raw,
            write_buffer.as_mut_ptr() as PVOID,
            WRITE_BUFFER_SIZE as u32,
        );
    }

    let wake = WakeHandle::new()?;
    WAKE_HANDLES
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .push(Arc::downgrade(&wake));

    let sender = Box::new(Sender {
        adapter: adapter.clone(),
        buffer: write_buffer,
        packet: write_packet,
    });
    let receiver = Box::new(Receiver {
        adapter,
        buffer: read_buffer,
        packet: read_packet,
        frames: VecDeque::new(),
        read_event,
        event_driven,
        wake,
    });
    Ok(Channel::Ethernet(sender, receiver))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mac() -> [u8; 6] {
        [0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff]
    }

    fn frame(destination: [u8; 6], ether_type: [u8; 2], payload: usize) -> Vec<u8> {
        let mut frame = vec![0u8; 14 + payload];
        frame[..6].copy_from_slice(&destination);
        frame[6..12].copy_from_slice(&[0x11, 0x22, 0x33, 0x44, 0x55, 0x66]);
        frame[12..14].copy_from_slice(&ether_type);
        frame
    }

    #[test]
    fn bpf_abi_is_windows_x64_layout() {
        assert_eq!(mem::size_of::<BpfInsn>(), 8);
        assert_eq!(mem::align_of::<BpfInsn>(), 4);
        assert_eq!(mem::size_of::<BpfProgram>(), 16);
        assert_eq!(mem::size_of::<BpfHeader>(), 20);
    }

    #[test]
    fn kernel_filter_rejects_short_ordinary_vlan_and_other_macs() {
        let program = eapol_for_mac_bpf(mac());
        for length in 0..13 {
            let short = vec![0u8; length];
            assert_eq!(run_bpf(&program, &short), 0, "length {length}");
        }
        let ordinary = frame(mac(), [0x08, 0x00], 32);
        assert_eq!(run_bpf(&program, &ordinary), 0);
        let mut vlan = frame([0x01, 0x80, 0xc2, 0x00, 0x00, 0x03], [0x81, 0x00], 32);
        vlan[16..18].copy_from_slice(&[0x88, 0x8e]);
        assert_eq!(run_bpf(&program, &vlan), 0);
        let multicast = frame([0x01, 0x80, 0xc2, 0x00, 0x00, 0x03], [0x88, 0x8e], 0);
        assert_eq!(run_bpf(&program, &multicast), 0);
        let other_unicast = frame([0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0x00], [0x88, 0x8e], 0);
        assert_eq!(run_bpf(&program, &other_unicast), 0);
    }

    #[test]
    fn kernel_filter_accepts_minimal_and_full_eapol_to_this_mac() {
        let program = eapol_for_mac_bpf(mac());
        let minimal = frame(mac(), [0x88, 0x8e], 0);
        assert_eq!(minimal.len(), 14);
        assert_eq!(run_bpf(&program, &minimal), 0x0000_ffff);
        let full = frame(mac(), [0x88, 0x8e], 82);
        assert_eq!(full.len(), 96);
        assert_eq!(run_bpf(&program, &full), 0x0000_ffff);
    }

    #[test]
    fn decode_records_walks_aligned_records_and_rejects_bad_tails() {
        let mut buffer = Vec::new();
        for (index, payload) in [14usize, 3, 0].into_iter().enumerate() {
            let header_size = mem::size_of::<BpfHeader>();
            let header = BpfHeader {
                caplen: payload as u32,
                datalen: payload as u32,
                hdrlen: header_size as u16,
                ..Default::default()
            };
            let start = buffer.len();
            buffer.resize(start + header_size + payload, 0);
            unsafe {
                ptr::write(buffer.as_mut_ptr().add(start) as *mut BpfHeader, header);
            }
            buffer[start + header_size..start + header_size + payload]
                .fill(u8::try_from(index).unwrap_or(0));
            let step = word_align(header_size + payload);
            buffer.resize(start + step, 0);
        }
        let received = buffer.len();
        let frames = decode_records(&buffer, received);
        assert_eq!(frames.len(), 3);
        assert_eq!(frames[0], (20, 14));
        assert_eq!(frames[1].1, 3);
        assert_eq!(frames[2].1, 0);

        let truncated = &buffer[..buffer.len() - 1];
        let _ = decode_records(truncated, truncated.len());

        let mut malformed = buffer.clone();
        unsafe {
            (*(malformed.as_mut_ptr() as *mut BpfHeader)).caplen = u32::MAX;
        }
        assert!(decode_records(&malformed, malformed.len()).is_empty());
    }

    #[test]
    fn timeout_and_wake_actions_do_not_read() {
        assert_eq!(wait_action(WAIT_OBJECT_0), ReadAction::Receive);
        assert_eq!(wait_action(WAIT_OBJECT_0 + 1), ReadAction::Interrupt);
        assert_eq!(wait_action(WAIT_TIMEOUT), ReadAction::PollOnly);
    }

    #[test]
    fn word_alignment_matches_bpf() {
        assert_eq!(word_align(0), 0);
        assert_eq!(word_align(1), 4);
        assert_eq!(word_align(20), 20);
        assert_eq!(word_align(21), 24);
        assert_eq!(word_align(34), 36);
    }
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ReadAction {
    Receive,
    PollOnly,
    Interrupt,
}

#[cfg(test)]
fn wait_action(result: u32) -> ReadAction {
    match result {
        WAIT_OBJECT_0 => ReadAction::Receive,
        value if value == WAIT_OBJECT_0 + 1 => ReadAction::Interrupt,
        _ => ReadAction::PollOnly,
    }
}
