use std::net::IpAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use bytes::{Buf, BufMut, Bytes, BytesMut};
use chrono::{Local, NaiveTime};
use crossbeam_channel::{Receiver, Select, Sender, bounded};
use pnet::datalink::MacAddr;
use rand::random;

const SEC: Duration = Duration::from_secs(1);
// Preserve the old maximum idle sleep for worker liveness checks. Packet and
// control events do not wait for this timer; a panic may not send any signal.
const WORKER_CHECK_INTERVAL: Duration = Duration::from_millis(10);

pub struct IdleWait {
    tx: Sender<()>,
    rx: Receiver<()>,
}

impl IdleWait {
    pub fn new() -> Self {
        let (tx, rx) = bounded(1);
        Self { tx, rx }
    }

    pub fn notifier(&self) -> Sender<()> {
        self.tx.clone()
    }

    pub fn wait(&self, packets: &Receiver<Vec<u8>>, control: Option<&Receiver<ChannelData>>) {
        self.wait_for(packets, control, WORKER_CHECK_INTERVAL);
    }

    fn wait_for(
        &self,
        packets: &Receiver<Vec<u8>>,
        control: Option<&Receiver<ChannelData>>,
        timeout: Duration,
    ) -> bool {
        let mut ready = Select::new();
        ready.recv(&self.rx);
        ready.recv(packets);
        if let Some(control) = control {
            ready.recv(control);
        }
        // Wait only for readiness. Leave packet/control reads to the process
        // loop so health checks and EAP notifications keep their old priority.
        let signalled = ready.ready_timeout(timeout).is_ok();
        let _ = self.rx.try_recv();
        signalled
    }
}

pub fn set_and_wake(flag: &AtomicBool, wake: &Sender<()>) {
    flag.store(true, Ordering::Release);
    // One pending notification is enough: the owner rechecks every flag.
    let _ = wake.try_send(());
}

#[inline]
pub fn ip_to_vec(ip: &IpAddr) -> Vec<u8> {
    match ip {
        IpAddr::V4(ip) => ip.octets().to_vec(),
        IpAddr::V6(ip) => ip.octets().to_vec(),
    }
}

#[inline]
pub fn put_mac(data: &mut BytesMut, mac: &MacAddr) {
    data.put_u8(mac.0);
    data.put_u8(mac.1);
    data.put_u8(mac.2);
    data.put_u8(mac.3);
    data.put_u8(mac.4);
    data.put_u8(mac.5);
}

#[inline]
pub fn get_mac(data: &mut Bytes) -> MacAddr {
    let mut mac = MacAddr::zero();
    mac.0 = data.get_u8();
    mac.1 = data.get_u8();
    mac.2 = data.get_u8();
    mac.3 = data.get_u8();
    mac.4 = data.get_u8();
    mac.5 = data.get_u8();
    mac
}

#[inline]
pub fn sleep_at(time: NaiveTime) {
    let mut dt = Local::now().date_naive().and_time(time);
    while dt < Local::now().naive_local() {
        dt += chrono::Duration::days(1);
    }
    while dt > Local::now().naive_local() {
        std::thread::sleep(SEC);
    }
}

#[inline]
pub fn random_vec(n: usize) -> Vec<u8> {
    let mut v = Vec::with_capacity(n);
    for _ in 0..n {
        v.push(random::<u8>());
    }
    v
}

/// enum of ChannelData.state
///
/// # State
#[derive(Clone)]
pub enum State {
    Success,
    Stop,
    Sleep,
    Quit,
}

#[derive(Clone)]
pub struct ChannelData {
    pub state: State,
    pub data: Vec<u8>,
}

#[cfg(test)]
mod idle_wait_tests {
    use super::*;
    use crossbeam_channel::unbounded;

    #[test]
    fn idle_wait_times_out_and_wakes_for_a_packet_without_consuming_it() {
        let idle = IdleWait::new();
        let (tx, rx) = unbounded();
        assert!(!idle.wait_for(&rx, None, Duration::from_millis(2)));

        std::thread::scope(|scope| {
            let (waiting, start) = bounded(0);
            scope.spawn(move || {
                start.recv().unwrap();
                tx.send(vec![42]).unwrap();
            });
            waiting.send(()).unwrap();
            assert!(idle.wait_for(&rx, None, Duration::from_secs(2)));
            assert_eq!(rx.try_recv().unwrap(), vec![42]);
        });
    }

    #[test]
    fn idle_wait_detects_control_even_without_packets_and_does_not_dequeue_either() {
        let idle = IdleWait::new();
        let (packets, packet_rx) = unbounded();
        let (control, control_rx) = unbounded();
        control
            .send(ChannelData {
                state: State::Stop,
                data: vec![],
            })
            .unwrap();
        assert!(idle.wait_for(&packet_rx, Some(&control_rx), Duration::ZERO));
        packets.send(vec![1, 2]).unwrap();
        assert!(idle.wait_for(&packet_rx, Some(&control_rx), Duration::ZERO));
        assert!(matches!(control_rx.try_recv().unwrap().state, State::Stop));
        assert_eq!(packet_rx.try_recv().unwrap(), vec![1, 2]);
    }

    #[test]
    fn worker_flags_coalesce_yet_both_are_visible_and_next_signal_still_wakes() {
        let idle = IdleWait::new();
        let wake = idle.notifier();
        let (_packets, packet_rx) = unbounded();
        let stop = AtomicBool::new(false);
        let quit = AtomicBool::new(false);
        set_and_wake(&stop, &wake);
        set_and_wake(&quit, &wake);
        assert_eq!(idle.rx.len(), 1);
        assert!(idle.wait_for(&packet_rx, None, Duration::ZERO));
        assert!(stop.load(Ordering::Acquire));
        assert!(quit.load(Ordering::Acquire));
        assert!(!idle.wait_for(&packet_rx, None, Duration::ZERO));
        set_and_wake(&quit, &wake);
        assert!(idle.wait_for(&packet_rx, None, Duration::ZERO));
    }

    #[test]
    fn disconnected_channel_wakes_existing_error_handling() {
        let idle = IdleWait::new();
        let (packets, packet_rx) = unbounded();
        drop(packets);
        assert!(idle.wait_for(&packet_rx, None, Duration::ZERO));
        assert!(matches!(
            packet_rx.try_recv(),
            Err(crossbeam_channel::TryRecvError::Disconnected)
        ));
    }
}
