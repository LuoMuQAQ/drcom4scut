//! GUI 绑定：父进程结束或停止事件触发时发送 EAPOL-Logoff，然后退出。
//! 没有 `DRCOM_PARENT_HANDLE` 时什么都不做，命令行直接运行核心保持原样。

use std::sync::Mutex;
use std::thread;
use std::time::Duration;

use log::info;
use pnet::datalink::{Channel, Config, MacAddr, channel, interfaces};

use crate::eap::logoff_frame;

const WAIT_OBJECT_0: u32 = 0;
const WAIT_TIMEOUT: u32 = 0x102;
const WAIT_FAILED: u32 = 0xFFFF_FFFF;
const INFINITE: u32 = 0xFFFF_FFFF;

static LOGOFF_MAC: Mutex<Option<MacAddr>> = Mutex::new(None);

#[link(name = "kernel32")]
unsafe extern "system" {
    fn WaitForSingleObject(handle: isize, millis: u32) -> u32;
    fn WaitForMultipleObjects(count: u32, handles: *const isize, wait_all: i32, millis: u32)
    -> u32;
}

pub fn set_logoff_mac(mac: MacAddr) {
    if let Ok(mut slot) = LOGOFF_MAC.lock() {
        *slot = Some(mac);
    }
}

/// 在打开网卡之前调用。监视线程进入等待后才返回，避免主线程先崩溃而没人收尾。
pub fn arm() {
    let Some(parent) = inherited_handle("DRCOM_PARENT_HANDLE") else {
        if std::env::var_os("DRCOM_PARENT_HANDLE").is_some() {
            std::process::exit(1);
        }
        return;
    };
    if !handle_usable(parent) {
        std::process::exit(1);
    }
    let shutdown = match inherited_handle("DRCOM_SHUTDOWN_EVENT") {
        Some(handle) if handle_usable(handle) => Some(handle),
        Some(_) => std::process::exit(1),
        None if std::env::var_os("DRCOM_SHUTDOWN_EVENT").is_some() => std::process::exit(1),
        None => None,
    };

    let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel(1);
    thread::Builder::new()
        .name("Parent-Watch".to_owned())
        .spawn(move || {
            let _ = ready_tx.send(());
            let handles = [parent, shutdown.unwrap_or(0)];
            let count = if shutdown.is_some() { 2 } else { 1 };
            let woke = unsafe { WaitForMultipleObjects(count, handles.as_ptr(), 0, INFINITE) };
            if woke == WAIT_FAILED {
                exit_process(1);
            }
            shutdown_from_binding();
        })
        .expect("Can't create parent watch thread!");
    let _ = ready_rx.recv();
}

fn inherited_handle(name: &str) -> Option<isize> {
    let text = std::env::var(name).ok()?;
    let value: usize = text.parse().ok()?;
    if value == 0 {
        None
    } else {
        Some(value as isize)
    }
}

fn handle_usable(handle: isize) -> bool {
    let state = unsafe { WaitForSingleObject(handle, 0) };
    state == WAIT_OBJECT_0 || state == WAIT_TIMEOUT
}

fn shutdown_from_binding() -> ! {
    let mac = LOGOFF_MAC.lock().ok().and_then(|mut slot| slot.take());
    thread::spawn(|| {
        thread::sleep(Duration::from_secs(1));
        exit_process(0);
    });
    if let Some(mac) = mac {
        info!("Send Logoff packet.");
        log::logger().flush();
        let _ = transmit_logoff(mac);
    }
    exit_process(0);
}

fn transmit_logoff(mac: MacAddr) -> std::io::Result<()> {
    let interface = interfaces()
        .into_iter()
        .find(|iface| iface.mac == Some(mac))
        .ok_or_else(|| std::io::Error::other("interface for logoff is gone"))?;
    let mut sender = match channel(&interface, Config::default()) {
        Ok(Channel::Ethernet(sender, _receiver)) => sender,
        _ => {
            return Err(std::io::Error::other("logoff channel is not ethernet"));
        }
    };
    let frame = logoff_frame(mac);
    match sender.send_to(&frame, None) {
        Some(result) => result,
        None => Err(std::io::Error::other("logoff send buffer was busy")),
    }
}

fn exit_process(code: i32) -> ! {
    log::logger().flush();
    std::process::exit(code);
}
