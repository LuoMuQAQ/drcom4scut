//! 按映像路径查找/等待本安装所属进程。禁止按进程名批量终止。

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, TerminateProcess, WaitForSingleObject,
    PROCESS_NAME_FORMAT, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE,
};

use crate::install::identity::normalize_for_compare;
use crate::platform;

pub fn pids_with_image(expected: &Path) -> Vec<u32> {
    let want = normalize_for_compare(expected);
    let mut out = Vec::new();
    unsafe {
        let snap = match CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) {
            Ok(h) => h,
            Err(_) => return out,
        };
        let mut entry = PROCESSENTRY32W::default();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        if Process32FirstW(snap, &mut entry).is_ok() {
            loop {
                if let Some(path) = image_path(entry.th32ProcessID) {
                    if normalize_for_compare(&path) == want {
                        out.push(entry.th32ProcessID);
                    }
                }
                if Process32NextW(snap, &mut entry).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snap);
    }
    out
}

fn image_path(pid: u32) -> Option<PathBuf> {
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 512];
        let mut size = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_FORMAT(0),
            windows::core::PWSTR(buf.as_mut_ptr()),
            &mut size,
        )
        .is_ok();
        let _ = CloseHandle(handle);
        if !ok {
            return None;
        }
        Some(PathBuf::from(String::from_utf16_lossy(
            &buf[..size as usize],
        )))
    }
}

pub fn wait_pids_exit(pids: &[u32], timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        let alive: Vec<u32> = pids.iter().copied().filter(|pid| pid_alive(*pid)).collect();
        if alive.is_empty() {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn pid_alive(pid: u32) -> bool {
    unsafe {
        let handle = match OpenProcess(PROCESS_SYNCHRONIZE, false, pid) {
            Ok(h) => h,
            Err(_) => return false,
        };
        let wait = WaitForSingleObject(handle, 0);
        let _ = CloseHandle(handle);
        wait != WAIT_OBJECT_0
    }
}

/// 请求本产品 GUI 退出（具名事件，由 GUI 转为完整退出流程）。
pub fn request_gui_exit() {
    let _ = platform::signal_exit();
}

/// 仅当映像路径仍等于 expected 时才终止，避免误杀同名进程。
pub fn terminate_if_image(pid: u32, expected: &Path) -> bool {
    let Some(path) = image_path(pid) else {
        return false;
    };
    if normalize_for_compare(&path) != normalize_for_compare(expected) {
        return false;
    }
    unsafe {
        let handle = match OpenProcess(
            PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION,
            false,
            pid,
        ) {
            Ok(h) => h,
            Err(_) => return false,
        };
        let ok = TerminateProcess(handle, 0).is_ok();
        let _ = CloseHandle(handle);
        ok
    }
}

pub fn stop_owned(install_dir: &Path, timeout: Duration) -> Result<(), String> {
    let gui = install_dir.join(super::GUI_EXE_NAME);
    let core = {
        // 任何 runtime\<sha>\drcom4scut.exe
        let mut list = Vec::new();
        let runtime = install_dir.join("runtime");
        if runtime.is_dir() {
            if let Ok(rd) = std::fs::read_dir(&runtime) {
                for e in rd.flatten() {
                    let exe = e.path().join("drcom4scut.exe");
                    if exe.is_file() {
                        list.push(exe);
                    }
                }
            }
        }
        list
    };
    let gui_pids = pids_with_image(&gui);
    if !gui_pids.is_empty() {
        request_gui_exit();
    }
    if !wait_pids_exit(&gui_pids, timeout) {
        for pid in &gui_pids {
            if !terminate_if_image(*pid, &gui) && pid_alive(*pid) {
                return Err("无法关闭正在运行的校园网客户端，请退出后重试。".into());
            }
        }
        if !wait_pids_exit(&gui_pids, Duration::from_secs(3)) {
            return Err("客户端尚未退出，请稍后重试。".into());
        }
    }
    for c in core {
        let pids = pids_with_image(&c);
        if !wait_pids_exit(&pids, Duration::from_secs(3)) {
            for pid in &pids {
                if !terminate_if_image(*pid, &c) && pid_alive(*pid) {
                    return Err("无法停止当前安装的认证核心，请退出客户端后重试。".into());
                }
            }
            if !wait_pids_exit(&pids, Duration::from_secs(3)) {
                return Err("认证核心尚未退出，请稍后重试。".into());
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_process_image_roundtrip() {
        let me = std::env::current_exe().unwrap();
        let pids = pids_with_image(&me);
        assert!(
            pids.contains(&std::process::id()),
            "应能按映像路径找到自身，pids={pids:?}"
        );
    }

    #[test]
    fn terminate_if_image_rejects_mismatch() {
        let ok = terminate_if_image(std::process::id(), Path::new(r"C:\not\this.exe"));
        assert!(!ok);
    }
}
