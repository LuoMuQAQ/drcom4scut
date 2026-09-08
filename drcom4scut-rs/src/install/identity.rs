//! 安装身份文件：产品标记、版本、所有权清单。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::{APP_VERSION, GUI_EXE_NAME, UNINSTALL_EXE_NAME};
use crate::coreproc::CORE_SHA256;

pub const PRODUCT_ID: &str = "drcom4scutGUI";
pub const STATE_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InstallState {
    pub product_id: String,
    pub state_version: u32,
    pub install_id: String,
    pub version: String,
    pub install_dir: String,
    pub origin_sid: String,
    pub core_sha256: String,
    #[serde(default)]
    pub owned_files: Vec<String>,
    #[serde(default)]
    pub owned_dirs: Vec<String>,
    #[serde(default)]
    pub desktop_shortcut: bool,
    #[serde(default)]
    pub start_menu_shortcut: bool,
    #[serde(default)]
    pub driver_status: String,
}

impl InstallState {
    pub fn new(install_dir: &Path, origin_sid: &str, install_id: &str) -> Self {
        let core_rel = format!("runtime/{CORE_SHA256}/drcom4scut.exe");
        Self {
            product_id: PRODUCT_ID.to_string(),
            state_version: STATE_VERSION,
            install_id: install_id.to_string(),
            version: APP_VERSION.to_string(),
            install_dir: install_dir.to_string_lossy().into_owned(),
            origin_sid: origin_sid.to_string(),
            core_sha256: CORE_SHA256.to_string(),
            owned_files: vec![
                GUI_EXE_NAME.to_string(),
                UNINSTALL_EXE_NAME.to_string(),
                "install-state.json".to_string(),
                core_rel,
                "licenses/NOTICE.txt".to_string(),
                "licenses/GPL-3.0.txt".to_string(),
                "licenses/lucide-LICENSE".to_string(),
            ],
            owned_dirs: vec!["runtime".into(), "licenses".into(), "data".into()],
            desktop_shortcut: true,
            start_menu_shortcut: true,
            driver_status: String::new(),
        }
    }

    pub fn new_for_test(install_dir: &Path, origin_sid: &str) -> Self {
        Self::new(install_dir, origin_sid, "test-install-id")
    }
}

pub fn load_state(path: &Path) -> Result<InstallState, String> {
    let raw = std::fs::read(path).map_err(|e| format!("无法读取安装身份：{e}"))?;
    let state: InstallState =
        serde_json::from_slice(&raw).map_err(|e| format!("安装身份无效：{e}"))?;
    validate_state(&state)?;
    Ok(state)
}

/// JSON is an identity record, never an arbitrary filesystem deletion script.
pub fn validate_state(state: &InstallState) -> Result<(), String> {
    if state.product_id != PRODUCT_ID || state.state_version != STATE_VERSION {
        return Err("安装身份产品或版本不匹配。".into());
    }
    if !valid_install_id(&state.install_id) || state.install_dir.is_empty() {
        return Err("安装身份缺少或包含无效字段。".into());
    }
    if state.core_sha256.len() != 64 || !state.core_sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("安装身份核心哈希无效。".into());
    }
    let expected = InstallState::new(
        Path::new(&state.install_dir),
        &state.origin_sid,
        &state.install_id,
    );
    let mut files = expected.owned_files;
    files[3] = format!("runtime/{}/drcom4scut.exe", state.core_sha256);
    if state.owned_files.iter().any(|p| !files.contains(p))
        || state
            .owned_dirs
            .iter()
            .any(|p| !expected.owned_dirs.contains(p))
    {
        return Err("安装清单包含非本产品路径，已拒绝。".into());
    }
    super::validate::logical_normalize(&state.install_dir).map_err(|e| e.message())?;
    Ok(())
}

pub fn valid_install_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 80 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

pub fn save_state(path: &Path, state: &InstallState) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_vec_pretty(state).map_err(|e| e.to_string())?;
    std::fs::write(path, json).map_err(|e| e.to_string())
}

/// 身份文件声明的安装目录必须与 EXE 所在目录逻辑上为同一路径。
pub fn state_matches_dir(state: &InstallState, exe_dir: &Path) -> bool {
    let declared = PathBuf::from(&state.install_dir);
    let a = normalize_for_compare(&declared);
    let b = normalize_for_compare(exe_dir);
    a == b && state.product_id == PRODUCT_ID
}

pub fn normalize_for_compare(path: &Path) -> String {
    let s = path.to_string_lossy().replace('/', "\\");
    let trimmed = s.trim_end_matches('\\');
    trimmed.to_ascii_lowercase()
}

pub fn new_install_id() -> String {
    let mut bytes = [0u8; 16];
    fill_random(&mut bytes);
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15]
    )
}

fn fill_random(buf: &mut [u8]) {
    use windows::Win32::Security::Cryptography::{
        BCryptGenRandom, BCRYPT_USE_SYSTEM_PREFERRED_RNG,
    };
    // SAFETY: buf 为本函数可写缓冲。
    let status = unsafe { BCryptGenRandom(None, buf, BCRYPT_USE_SYSTEM_PREFERRED_RNG) };
    if status.is_err() {
        let t = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        for (i, b) in buf.iter_mut().enumerate() {
            *b ^= ((t >> ((i % 8) * 8)) as u8).wrapping_add(i as u8);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_state() {
        let dir = std::env::temp_dir().join(format!(
            "drcom-state-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("install-state.json");
        let state = InstallState::new_for_test(&dir, "S-1-5-21-1");
        save_state(&path, &state).unwrap();
        let loaded = load_state(&path).unwrap();
        assert_eq!(loaded.product_id, PRODUCT_ID);
        assert!(state_matches_dir(&loaded, &dir));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn foreign_product_rejected() {
        let dir = std::env::temp_dir().join(format!("drcom-state-bad-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("install-state.json");
        std::fs::write(&path, br#"{"productId":"other","stateVersion":1,"installId":"x","version":"1","installDir":"C:\\x","originSid":"S-1-5","coreSha256":"aa"}"#).unwrap();
        assert!(load_state(&path).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn compare_is_case_and_slash_insensitive() {
        let state = InstallState::new_for_test(Path::new(r"C:\Program Files\drcom4scutGUI"), "S-1");
        assert!(state_matches_dir(
            &state,
            Path::new(r"C:/PROGRAM FILES/drcom4scutGUI\")
        ));
        assert!(!state_matches_dir(
            &state,
            Path::new(r"C:\Program Files\other")
        ));
    }
}
