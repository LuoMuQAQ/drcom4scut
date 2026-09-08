//! HKLM 卸载注册（64 位视图）。Publisher 使用产品显示名，不冒用其他厂商。

use std::path::{Path, PathBuf};

use windows::core::PCWSTR;
use windows::Win32::Foundation::{ERROR_SUCCESS, WIN32_ERROR};
use windows::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegDeleteKeyExW, RegGetValueW, RegSetValueExW, HKEY,
    HKEY_LOCAL_MACHINE, KEY_WOW64_64KEY, KEY_WRITE, REG_DWORD, REG_OPTION_NON_VOLATILE, REG_SZ,
    RRF_RT_REG_SZ, RRF_SUBKEY_WOW6464KEY,
};

use super::sid::wide;
use super::{APP_DISPLAY_NAME, APP_VERSION, GUI_EXE_NAME, UNINSTALL_EXE_NAME};

pub const UNINSTALL_SUBKEY: &str =
    r"Software\Microsoft\Windows\CurrentVersion\Uninstall\drcom4scutGUI";

pub fn uninstall_string(install_dir: &Path) -> String {
    format!("\"{}\"", install_dir.join(UNINSTALL_EXE_NAME).display())
}

pub fn display_icon(install_dir: &Path) -> String {
    install_dir
        .join(GUI_EXE_NAME)
        .to_string_lossy()
        .into_owned()
}

fn version_parts() -> (u32, u32) {
    let mut it = APP_VERSION.split('.');
    let major = it.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let minor = it.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    (major, minor)
}

fn install_location(install_dir: &Path) -> String {
    let mut s = install_dir.to_string_lossy().into_owned();
    if !s.ends_with('\\') {
        s.push('\\');
    }
    s
}

fn install_date() -> String {
    use windows::Win32::System::SystemInformation::GetLocalTime;
    let st = unsafe { GetLocalTime() };
    format!("{:04}{:02}{:02}", st.wYear, st.wMonth, st.wDay)
}

pub fn write_uninstall_key(
    install_dir: &Path,
    estimated_size_kb: u32,
    publisher: Option<&str>,
) -> Result<(), String> {
    let sub = wide(UNINSTALL_SUBKEY);
    unsafe {
        let mut key = HKEY::default();
        let err = RegCreateKeyExW(
            HKEY_LOCAL_MACHINE,
            PCWSTR(sub.as_ptr()),
            None,
            windows::core::PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_WRITE | KEY_WOW64_64KEY,
            None,
            &mut key,
            None,
        );
        if err != ERROR_SUCCESS {
            return Err(format!("无法写入卸载注册表（{}）。", err.0));
        }
        let set_sz = |name: &str, value: &str| -> Result<(), String> {
            let n = wide(name);
            let v: Vec<u8> = value
                .encode_utf16()
                .chain(std::iter::once(0))
                .flat_map(|c| c.to_le_bytes())
                .collect();
            let e = RegSetValueExW(key, PCWSTR(n.as_ptr()), None, REG_SZ, Some(&v));
            if e != ERROR_SUCCESS {
                Err(format!("无法写入卸载注册表项 {name}（{}）。", e.0))
            } else {
                Ok(())
            }
        };
        let set_dword = |name: &str, value: u32| -> Result<(), String> {
            let n = wide(name);
            let bytes = value.to_le_bytes();
            let e = RegSetValueExW(key, PCWSTR(n.as_ptr()), None, REG_DWORD, Some(&bytes));
            if e != ERROR_SUCCESS {
                Err(format!("无法写入卸载注册表项 {name}（{}）。", e.0))
            } else {
                Ok(())
            }
        };
        let publisher = publisher
            .filter(|s| !s.is_empty())
            .unwrap_or(APP_DISPLAY_NAME);
        let result: Result<(), String> = (|| {
            set_sz("DisplayName", APP_DISPLAY_NAME)?;
            set_sz("DisplayVersion", APP_VERSION)?;
            set_sz("Publisher", publisher)?;
            set_sz("InstallLocation", &install_location(install_dir))?;
            set_sz("InstallDate", &install_date())?;
            set_sz("DisplayIcon", &display_icon(install_dir))?;
            set_sz("UninstallString", &uninstall_string(install_dir))?;
            set_sz(
                "QuietUninstallString",
                &format!("{} --quiet", uninstall_string(install_dir)),
            )?;
            let (major, minor) = version_parts();
            set_dword("EstimatedSize", estimated_size_kb.max(1))?;
            set_dword("VersionMajor", major)?;
            set_dword("VersionMinor", minor)?;
            set_dword("NoModify", 1)?;
            set_dword("NoRepair", 1)?;
            Ok(())
        })();
        let _ = RegCloseKey(key);
        result?;
    }
    super::shortcuts::notify_shell();
    Ok(())
}

fn read_location(root: HKEY, subkey: &str) -> Result<Option<PathBuf>, String> {
    let sub = wide(subkey);
    let value = wide("InstallLocation");
    let mut buffer = vec![0u16; 32768];
    let mut bytes = (buffer.len() * 2) as u32;
    let err = unsafe {
        RegGetValueW(
            root,
            PCWSTR(sub.as_ptr()),
            PCWSTR(value.as_ptr()),
            RRF_RT_REG_SZ | RRF_SUBKEY_WOW6464KEY,
            None,
            Some(buffer.as_mut_ptr().cast()),
            Some(&mut bytes),
        )
    };
    if err == WIN32_ERROR(2) {
        return Ok(None);
    }
    if err != ERROR_SUCCESS {
        return Err(format!("读取既有安装位置失败（{}）。", err.0));
    }
    let end = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
    let text = String::from_utf16(&buffer[..end]).map_err(|e| e.to_string())?;
    if text.trim().is_empty() {
        return Err("已有卸载登记缺少安装位置，请先修复登记。".into());
    }
    Ok(Some(PathBuf::from(text)))
}

pub fn registered_install_dir() -> Result<Option<PathBuf>, String> {
    read_location(HKEY_LOCAL_MACHINE, UNINSTALL_SUBKEY)
}

pub fn enforce_single_install(target: &Path, registered: Option<&Path>) -> Result<(), String> {
    if let Some(existing) = registered {
        if super::identity::normalize_for_compare(existing)
            != super::identity::normalize_for_compare(target)
        {
            return Err(format!(
                "已登记安装在 {}。请在原目录升级或先完成卸载，再更换目录。",
                existing.display()
            ));
        }
    }
    Ok(())
}

fn delete_if_ours(root: HKEY, subkey: &str, install_dir: &Path) -> Result<(), String> {
    let existing = read_location(root, subkey)?;
    if !existing.as_deref().is_some_and(|p| {
        super::identity::normalize_for_compare(p)
            == super::identity::normalize_for_compare(install_dir)
    }) {
        return Ok(());
    }
    let sub = wide(subkey);
    // Delete one leaf atomically. Never partially erase a key with RegDeleteTree.
    let err = unsafe { RegDeleteKeyExW(root, PCWSTR(sub.as_ptr()), KEY_WOW64_64KEY.0, None) };
    if err != ERROR_SUCCESS && err != WIN32_ERROR(2) {
        return Err(format!("删除卸载注册表失败（{}）。", err.0));
    }
    Ok(())
}

pub fn delete_uninstall_key_if_ours(install_dir: &Path) -> Result<(), String> {
    delete_if_ours(HKEY_LOCAL_MACHINE, UNINSTALL_SUBKEY, install_dir)?;
    super::shortcuts::notify_shell();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn single_install_policy_and_conditional_registration_deletion() {
        use windows::Win32::System::Registry::HKEY_CURRENT_USER;
        let subkey = format!(
            r"Software\drcom4scut-test-{}",
            super::super::identity::new_install_id()
        );
        let a = Path::new(r"C:\Test Apps\A");
        let b = Path::new(r"C:\Test Apps\B");
        assert!(enforce_single_install(a, None).is_ok());
        assert!(enforce_single_install(a, Some(a)).is_ok());
        assert!(enforce_single_install(b, Some(a)).is_err());
        let sub = wide(&subkey);
        unsafe {
            let mut key = HKEY::default();
            assert_eq!(
                RegCreateKeyExW(
                    HKEY_CURRENT_USER,
                    PCWSTR(sub.as_ptr()),
                    None,
                    PCWSTR::null(),
                    REG_OPTION_NON_VOLATILE,
                    KEY_WRITE | KEY_WOW64_64KEY,
                    None,
                    &mut key,
                    None
                ),
                ERROR_SUCCESS
            );
            let name = wide("InstallLocation");
            let bytes: Vec<u8> = install_location(b)
                .encode_utf16()
                .chain(Some(0))
                .flat_map(u16::to_le_bytes)
                .collect();
            assert_eq!(
                RegSetValueExW(key, PCWSTR(name.as_ptr()), None, REG_SZ, Some(&bytes)),
                ERROR_SUCCESS
            );
            let _ = RegCloseKey(key);
        }
        delete_if_ours(HKEY_CURRENT_USER, &subkey, a).unwrap();
        assert_eq!(
            read_location(HKEY_CURRENT_USER, &subkey).unwrap(),
            Some(PathBuf::from(install_location(b)))
        );
        delete_if_ours(HKEY_CURRENT_USER, &subkey, b).unwrap();
        assert!(read_location(HKEY_CURRENT_USER, &subkey).unwrap().is_none());
    }

    #[test]
    fn uninstall_string_is_quoted() {
        let dir = PathBuf::from(r"C:\Program Files\drcom4scutGUI");
        let s = uninstall_string(&dir);
        assert_eq!(s, r#""C:\Program Files\drcom4scutGUI\uninstall.exe""#);
        assert_eq!(
            display_icon(&dir),
            r"C:\Program Files\drcom4scutGUI\drcom4scutGUI.exe"
        );
        assert_eq!(install_location(&dir), r"C:\Program Files\drcom4scutGUI\");
        assert_eq!(version_parts(), (0, 3));
    }
}
