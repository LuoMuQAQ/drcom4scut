//! WebView2 运行时探测与安全动态加载器。
//! 确保在无 WebView2 Runtime 时不发生 PE 导入加载失败（exit 127 / STATUS_DLL_NOT_FOUND）。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicPtr, Ordering};
use std::ffi::c_void;

use windows::core::{s, HRESULT, PCWSTR};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows::Win32::System::Registry::{
    RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE,
    KEY_READ, KEY_WOW64_32KEY, KEY_WOW64_64KEY, REG_SZ,
};

pub const CLIENT_GUID: &str = "{F3017226-FE2A-4295-80F6-670562D7184A}";
const LOADER_BIN: &[u8] = include_bytes!("../../../resources/WebView2Loader.dll");

pub type CreateEnvWithOptionsFn = unsafe extern "system" fn(
    browserexecutablefolder: PCWSTR,
    userdatafolder: PCWSTR,
    environmentoptions: *mut c_void,
    environmentcreatedhandler: *mut c_void,
) -> HRESULT;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeStatus {
    Installed { version: String, path: PathBuf },
    Missing,
}

/// 探测系统中是否已安装 Microsoft Edge WebView2 Runtime。
pub fn detect_runtime() -> RuntimeStatus {
    // 1. 检查注册表
    let subkeys = [
        format!(r"SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{CLIENT_GUID}"),
        format!(r"SOFTWARE\Microsoft\EdgeUpdate\Clients\{CLIENT_GUID}"),
    ];

    for root in [HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER] {
        for subkey in &subkeys {
            for flags in [KEY_WOW64_32KEY, KEY_WOW64_64KEY] {
                if let Some((ver, loc)) = read_reg_client(root, subkey, flags.0) {
                    if !ver.is_empty() && ver != "0.0.0.0" {
                        let path = if !loc.is_empty() {
                            PathBuf::from(loc).join(&ver)
                        } else {
                            find_candidate_dir(&ver).unwrap_or_else(|| PathBuf::from(&ver))
                        };
                        return RuntimeStatus::Installed { version: ver, path };
                    }
                }
            }
        }
    }

    // 2. 检查已知文件路径
    let program_files = [
        std::env::var_os("ProgramFiles(x86)").map(PathBuf::from),
        std::env::var_os("ProgramFiles").map(PathBuf::from),
        std::env::var_os("LOCALAPPDATA").map(|p| PathBuf::from(p).join("Microsoft")),
    ];

    for base in program_files.into_iter().flatten() {
        let app_dir = base.join("Microsoft").join("EdgeWebView").join("Application");
        if app_dir.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&app_dir) {
                for entry in entries.flatten() {
                    let p = entry.path();
                    if p.is_dir() {
                        let dll = p.join("EBWebView").join("x64").join("EmbeddedBrowserWebView.dll");
                        if dll.is_file() {
                            let ver = p.file_name().unwrap_or_default().to_string_lossy().to_string();
                            return RuntimeStatus::Installed {
                                version: ver,
                                path: p,
                            };
                        }
                    }
                }
            }
        }
    }

    RuntimeStatus::Missing
}

fn find_candidate_dir(ver: &str) -> Option<PathBuf> {
    let program_files = [
        std::env::var_os("ProgramFiles(x86)").map(PathBuf::from),
        std::env::var_os("ProgramFiles").map(PathBuf::from),
    ];
    for base in program_files.into_iter().flatten() {
        let candidate = base.join(r"Microsoft\EdgeWebView\Application").join(ver);
        if candidate.is_dir() {
            return Some(candidate);
        }
    }
    None
}

fn read_reg_client(root: HKEY, subkey: &str, extra_flags: u32) -> Option<(String, String)> {
    unsafe {
        let mut key = HKEY::default();
        let subkey_w: Vec<u16> = subkey.encode_utf16().chain(std::iter::once(0)).collect();
        let status = RegOpenKeyExW(
            root,
            PCWSTR(subkey_w.as_ptr()),
            Some(0),
            windows::Win32::System::Registry::REG_SAM_FLAGS(KEY_READ.0 | extra_flags),
            &mut key,
        );
        if status.is_err() {
            return None;
        }

        let ver = read_reg_string(key, "pv").unwrap_or_default();
        let loc = read_reg_string(key, "location").unwrap_or_default();
        let _ = RegCloseKey(key);
        Some((ver, loc))
    }
}

unsafe fn read_reg_string(key: HKEY, name: &str) -> Option<String> {
    let name_w: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
    let mut buf = [0u16; 512];
    let mut size = (buf.len() * 2) as u32;
    let mut kind = REG_SZ;
    let res = RegQueryValueExW(
        key,
        PCWSTR(name_w.as_ptr()),
        None,
        Some(&mut kind),
        Some(buf.as_mut_ptr() as *mut u8),
        Some(&mut size),
    );
    if res.is_ok() && kind == REG_SZ && size > 2 {
        let len = (size as usize / 2).saturating_sub(1);
        Some(String::from_utf16_lossy(&buf[..len]))
    } else {
        None
    }
}

/// 确保安全释放并动态加载嵌入的 WebView2Loader.dll。
pub fn ensure_loader() -> Result<CreateEnvWithOptionsFn, String> {
    static CACHED_FN: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());

    let existing = CACHED_FN.load(Ordering::Acquire);
    if !existing.is_null() {
        return Ok(unsafe { std::mem::transmute(existing) });
    }

    // 释放到 LocalAppData 安全子目录
    let local_app_data = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\ProgramData"));
    let loader_dir = local_app_data.join("drcom4scut").join("bin");
    let _ = std::fs::create_dir_all(&loader_dir);
    let loader_path = loader_dir.join("WebView2Loader.dll");

    // 校验已有文件哈希或重新写入
    let need_write = match std::fs::read(&loader_path) {
        Ok(bytes) => bytes != LOADER_BIN,
        Err(_) => true,
    };
    if need_write {
        std::fs::write(&loader_path, LOADER_BIN).map_err(|e| format!("无法释放 WebView2Loader: {e}"))?;
    }

    unsafe {
        let path_w: Vec<u16> = loader_path.to_string_lossy().encode_utf16().chain(std::iter::once(0)).collect();
        let hmodule = LoadLibraryW(PCWSTR(path_w.as_ptr()))
            .map_err(|e| format!("无法加载 WebView2Loader.dll: {e}"))?;
        let proc = GetProcAddress(hmodule, s!("CreateCoreWebView2EnvironmentWithOptions"))
            .ok_or_else(|| "WebView2Loader 中缺少 CreateCoreWebView2EnvironmentWithOptions 导出".to_string())?;

        let fn_ptr = proc as *mut c_void;
        CACHED_FN.store(fn_ptr, Ordering::Release);
        Ok(std::mem::transmute(fn_ptr))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_runtime_returns_status() {
        let status = detect_runtime();
        // 在安装了 WebView2 的机器上应该返回 Installed
        match status {
            RuntimeStatus::Installed { version, path } => {
                assert!(!version.is_empty());
                println!("Detected WebView2 Runtime version: {}, path: {:?}", version, path);
            }
            RuntimeStatus::Missing => {
                println!("WebView2 Runtime not installed on this system");
            }
        }
    }

    #[test]
    fn test_ensure_loader_loads_successfully() {
        let create_fn = ensure_loader();
        assert!(create_fn.is_ok(), "ensure_loader failed: {:?}", create_fn.err());
    }
}
