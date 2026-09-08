//! The actual elevated workers hold this cross-session lock for all mutations.
use windows::core::HSTRING;
use windows::Win32::Foundation::{
    CloseHandle, HANDLE, WAIT_ABANDONED, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows::Win32::System::Threading::{CreateMutexW, ReleaseMutex, WaitForSingleObject};
const NAME: &str = r"Global\drcom4scutGUI-Maintenance";
pub struct Guard(HANDLE);
impl Drop for Guard {
    fn drop(&mut self) {
        unsafe {
            let _ = ReleaseMutex(self.0);
            let _ = CloseHandle(self.0);
        }
    }
}
pub fn acquire() -> Result<Guard, String> {
    acquire_named(NAME)
}
fn acquire_named(name: &str) -> Result<Guard, String> {
    unsafe {
        let handle = CreateMutexW(None, false, &HSTRING::from(name))
            .map_err(|e| format!("无法取得安装维护锁：{e}。请等待其他安装/卸载结束后重试。"))?;
        let wait = WaitForSingleObject(handle, 0);
        if wait == WAIT_OBJECT_0 || wait == WAIT_ABANDONED {
            return Ok(Guard(handle));
        }
        let _ = CloseHandle(handle);
        if wait == WAIT_TIMEOUT {
            Err("另一安装或卸载正在进行，请等待完成后重试。".into())
        } else {
            Err("无法取得安装维护锁，已停止操作。".into())
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn worker_lock_excludes_competitor_and_releases() {
        let name = format!(
            "Local\\drcom-maintenance-test-{}",
            super::super::identity::new_install_id()
        );
        let guard = acquire_named(&name).unwrap();
        let other = name.clone();
        assert!(std::thread::spawn(move || acquire_named(&other).is_err())
            .join()
            .unwrap());
        drop(guard);
        assert!(std::thread::spawn(move || acquire_named(&name).is_ok())
            .join()
            .unwrap());
    }
}
