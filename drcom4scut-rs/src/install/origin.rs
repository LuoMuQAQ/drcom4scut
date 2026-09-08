//! 未提权入口与提权工人之间的可信交接。不信任裸命令行 SID。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::sid::{current_user_sid, is_plausible_sid};
use super::validate::reject_reparse_in_chain;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Origin {
    pub sid: String,
    pub nonce: String,
    pub parent_pid: u32,
    pub user_name: String,
}

pub fn origin_root(nonce: &str) -> PathBuf {
    std::env::temp_dir().join(format!("drcom4scut-setup-{nonce}"))
}

pub fn write_origin(origin: &Origin) -> Result<PathBuf, String> {
    if !is_plausible_sid(&origin.sid) {
        return Err("发起用户 SID 无效。".into());
    }
    if origin.nonce.len() < 16 || !origin.nonce.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err("交接随机数无效。".into());
    }
    let dir = origin_root(&origin.nonce);
    // Do not reuse an existing handshake directory.
    std::fs::create_dir(&dir).map_err(|e| e.to_string())?;
    let path = dir.join("origin.json");
    std::fs::write(
        &path,
        serde_json::to_vec_pretty(origin).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    Ok(dir)
}

pub fn capture_current_origin(nonce: &str) -> Result<Origin, String> {
    let sid = current_user_sid().ok_or("无法读取当前用户 SID")?;
    Ok(Origin {
        sid,
        nonce: nonce.to_string(),
        parent_pid: std::process::id(),
        user_name: std::env::var("USERNAME").unwrap_or_default(),
    })
}

/// 提权端校验：目录位于临时目录、无重解析点、JSON 与 nonce/SID 一致。
pub fn verify_origin(dir: &Path, nonce: &str) -> Result<Origin, String> {
    if nonce.len() < 16 || nonce.len() > 64 || !nonce.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err("交接随机数无效。".into());
    }
    if !dir.is_absolute()
        || dir
            .file_name()
            .is_none_or(|n| n != format!("drcom4scut-setup-{nonce}").as_str())
    {
        return Err("交接目录与随机数不匹配。".into());
    }
    reject_reparse_in_chain(&dir.join("origin.json")).map_err(|e| e.message())?;
    if std::fs::metadata(dir.join("origin.json"))
        .map_err(|e| e.to_string())?
        .len()
        > 16384
    {
        return Err("交接文件过大。".into());
    }
    let raw =
        std::fs::read(dir.join("origin.json")).map_err(|e| format!("无法读取交接文件：{e}"))?;
    let origin: Origin = serde_json::from_slice(&raw).map_err(|e| format!("交接文件无效：{e}"))?;
    if origin.nonce != nonce {
        return Err("交接随机数不匹配。".into());
    }
    if !is_plausible_sid(&origin.sid) {
        return Err("交接 SID 无效。".into());
    }
    verify_parent(&origin, dir)?;
    Ok(origin)
}

fn verify_parent(origin: &Origin, dir: &Path) -> Result<(), String> {
    use windows::Win32::{
        Foundation::CloseHandle,
        System::Threading::{
            OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT,
            PROCESS_QUERY_LIMITED_INFORMATION,
        },
    };
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, origin.parent_pid)
            .map_err(|_| "发起安装的窗口已退出，请重新打开安装器。")?;
        let sid = super::sid::sid_from_process(process);
        let mut buffer = [0u16; 32768];
        let mut size = buffer.len() as u32;
        let read = QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_FORMAT(0),
            windows::core::PWSTR(buffer.as_mut_ptr()),
            &mut size,
        );
        // UAC may use a different administrator account. Resolve the initiating
        // user's Temp folder from its live process token, not the worker's profile.
        let original_temp = parent_temp_dir(process);
        let _ = CloseHandle(process);
        if sid.as_deref() != Some(&origin.sid) {
            return Err("发起用户与交接信息不一致。".into());
        }
        read.map_err(|_| "无法核验发起安装的程序。")?;
        let parent = PathBuf::from(String::from_utf16_lossy(&buffer[..size as usize]));
        let current = std::env::current_exe().map_err(|e| e.to_string())?;
        if super::identity::normalize_for_compare(&parent)
            != super::identity::normalize_for_compare(&current)
        {
            return Err("发起程序与安装器不一致。".into());
        }
        let mut expected = original_temp
            .map(|p| p.join(format!("drcom4scut-setup-{}", origin.nonce)))
            .into_iter()
            .collect::<Vec<_>>();
        if current_user_sid().as_deref() == Some(&origin.sid) {
            expected.push(origin_root(&origin.nonce));
        }
        if !expected.iter().any(|p| {
            super::identity::normalize_for_compare(p) == super::identity::normalize_for_compare(dir)
        }) {
            return Err("交接目录不在发起用户的临时目录内。".into());
        }
    }
    Ok(())
}

fn parent_temp_dir(process: windows::Win32::Foundation::HANDLE) -> Option<PathBuf> {
    use windows::Win32::{
        Foundation::{CloseHandle, HANDLE},
        Security::TOKEN_QUERY,
        System::{Com::CoTaskMemFree, Threading::OpenProcessToken},
        UI::Shell::{FOLDERID_LocalAppData, SHGetKnownFolderPath, KF_FLAG_DEFAULT},
    };
    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(process, TOKEN_QUERY, &mut token).ok()?;
        let result = SHGetKnownFolderPath(&FOLDERID_LocalAppData, KF_FLAG_DEFAULT, Some(token));
        let _ = CloseHandle(token);
        let raw = result.ok()?;
        let path = raw.to_string();
        CoTaskMemFree(Some(raw.0.cast()));
        Some(PathBuf::from(path.ok()?).join("Temp"))
    }
}

pub fn write_status(dir: &Path, json: &str) {
    let _ = std::fs::write(dir.join("status.json"), json.as_bytes());
}

pub fn write_result(dir: &Path, json: &str) {
    let _ = std::fs::write(dir.join("result.json"), json.as_bytes());
}

pub fn cancel_requested(dir: &Path) -> bool {
    dir.join("cancel.flag").is_file()
}

pub fn request_cancel(dir: &Path) {
    let _ = std::fs::write(dir.join("cancel.flag"), b"1");
}

pub fn cleanup_origin(dir: &Path) {
    let _ = std::fs::remove_dir_all(dir);
}

pub fn new_nonce() -> String {
    crate::install::identity::new_install_id().replace('-', "")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handshake_accepts_matching_and_rejects_tamper() {
        let generated = new_nonce();
        let nonce = generated.as_str();
        let origin = capture_current_origin(nonce).unwrap();
        let dir = write_origin(&origin).unwrap();
        let loaded = verify_origin(&dir, nonce).unwrap();
        assert_eq!(loaded.sid, origin.sid);

        let mut tampered = origin.clone();
        tampered.sid = "S-1-5-21-999-888-777-1001".into();
        std::fs::write(
            dir.join("origin.json"),
            serde_json::to_vec(&tampered).unwrap(),
        )
        .unwrap();
        assert!(verify_origin(&dir, nonce).is_err());
        assert!(verify_origin(&dir, "ffffffffffffffffffffffffffffffff").is_err());
        cleanup_origin(&dir);
    }

    #[test]
    fn rejects_path_outside_temp() {
        let nonce = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let dir = PathBuf::from(r"C:\Windows\Temp\not-ours");
        assert!(verify_origin(&dir, nonce).is_err());
    }
}
