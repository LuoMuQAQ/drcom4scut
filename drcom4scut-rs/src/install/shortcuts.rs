//! 桌面与开始菜单快捷方式（IShellLink）。

use std::path::{Path, PathBuf};

use windows::core::{Interface, PCWSTR};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, IPersistFile, CLSCTX_INPROC_SERVER,
    COINIT_APARTMENTTHREADED,
};
use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};

use super::{APP_DISPLAY_NAME, GUI_EXE_NAME};

pub fn shortcut_name() -> String {
    format!("{APP_DISPLAY_NAME}.lnk")
}

pub fn uninstall_shortcut_name() -> String {
    "卸载 校园网认证客户端.lnk".into()
}

pub fn create_shortcut(
    link_path: &Path,
    target: &Path,
    workdir: &Path,
    args: &str,
) -> Result<(), String> {
    unsafe {
        // S_OK / S_FALSE both increment the COM refcount; RPC_E_CHANGED_MODE does not.
        let initialized = CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok();
        struct Com(bool);
        impl Drop for Com {
            fn drop(&mut self) {
                if self.0 {
                    unsafe {
                        CoUninitialize();
                    }
                }
            }
        }
        let _com = Com(initialized);
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)
            .map_err(|e| format!("创建快捷方式对象失败：{e}"))?;
        let target_w: Vec<u16> = os_wide(target);
        let dir_w: Vec<u16> = os_wide(workdir);
        link.SetPath(PCWSTR(target_w.as_ptr()))
            .map_err(|e| format!("SetPath：{e}"))?;
        link.SetWorkingDirectory(PCWSTR(dir_w.as_ptr()))
            .map_err(|e| format!("SetWorkingDirectory：{e}"))?;
        let _ = link.SetIconLocation(PCWSTR(target_w.as_ptr()), 0);
        if !args.is_empty() {
            let args_w: Vec<u16> = args.encode_utf16().chain(std::iter::once(0)).collect();
            let _ = link.SetArguments(PCWSTR(args_w.as_ptr()));
        }
        let desc: Vec<u16> = APP_DISPLAY_NAME
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let _ = link.SetDescription(PCWSTR(desc.as_ptr()));
        let persist: IPersistFile = link.cast().map_err(|e| e.to_string())?;
        if let Some(parent) = link_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let out_w: Vec<u16> = os_wide(link_path);
        persist
            .Save(PCWSTR(out_w.as_ptr()), true)
            .map_err(|e| format!("保存快捷方式失败：{e}"))?;
        use windows::Win32::UI::Shell::{SHChangeNotify, SHCNE_CREATE, SHCNF_PATHW};
        SHChangeNotify(SHCNE_CREATE, SHCNF_PATHW, Some(out_w.as_ptr().cast()), None);
    }
    Ok(())
}

pub fn notify_shell() {
    use windows::Win32::UI::Shell::{SHChangeNotify, SHCNE_ASSOCCHANGED, SHCNF_IDLIST};
    unsafe {
        SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, None, None);
    }
}

fn os_wide(p: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    p.as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

pub fn desktop_link_path(desktop: &Path) -> PathBuf {
    desktop.join(shortcut_name())
}

pub fn start_menu_dir(programs: &Path) -> PathBuf {
    programs.join("drcom4scutGUI")
}

pub fn start_menu_gui_link(programs: &Path) -> PathBuf {
    start_menu_dir(programs).join(shortcut_name())
}

pub fn start_menu_uninstall_link(programs: &Path) -> PathBuf {
    start_menu_dir(programs).join(uninstall_shortcut_name())
}

/// Direct Start Menu item, so Windows 11 “All apps” lists it without opening a folder.
pub fn programs_gui_link(programs: &Path) -> PathBuf {
    programs.join(shortcut_name())
}

pub fn programs_uninstall_link(programs: &Path) -> PathBuf {
    programs.join(uninstall_shortcut_name())
}

pub fn gui_target(install_dir: &Path) -> PathBuf {
    install_dir.join(GUI_EXE_NAME)
}

pub fn quoted_target(path: &Path) -> String {
    format!("\"{}\"", path.display())
}

pub fn remove_if_exists(path: &Path) {
    let _ = std::fs::remove_file(path);
}

/// Read the stored target without resolving, searching for, or launching it.
pub fn remove_if_target(path: &Path, expected: &Path) -> Result<(), String> {
    super::validate::reject_reparse_in_chain(path).map_err(|e| e.message())?;
    match std::fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.to_string()),
        Ok(_) => {}
    }
    unsafe {
        let initialized = CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok();
        let result = (|| -> Result<(), String> {
            let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)
                .map_err(|e| e.to_string())?;
            let persist: IPersistFile = link.cast().map_err(|e| e.to_string())?;
            persist
                .Load(
                    PCWSTR(os_wide(path).as_ptr()),
                    windows::Win32::System::Com::STGM_READ,
                )
                .map_err(|e| format!("无法核验快捷方式 {}：{e}", path.display()))?;
            let mut target = [0u16; 32768];
            link.GetPath(
                &mut target,
                std::ptr::null_mut(),
                windows::Win32::UI::Shell::SLGP_RAWPATH.0 as u32,
            )
            .map_err(|e| e.to_string())?;
            let end = target.iter().position(|&c| c == 0).unwrap_or(target.len());
            let target = PathBuf::from(String::from_utf16_lossy(&target[..end]));
            if super::identity::normalize_for_compare(&target)
                == super::identity::normalize_for_compare(expected)
            {
                std::fs::remove_file(path).map_err(|e| format!("删除快捷方式失败：{e}"))?;
            }
            Ok(())
        })();
        if initialized {
            CoUninitialize();
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_quoting() {
        assert!(shortcut_name().ends_with(".lnk"));
        assert!(quoted_target(Path::new(
            r"C:\Program Files\drcom4scutGUI\drcom4scutGUI.exe"
        ))
        .starts_with('\"'));
        let programs = Path::new(r"C:\ProgramData\Microsoft\Windows\Start Menu\Programs");
        assert_eq!(
            start_menu_gui_link(programs).parent().unwrap(),
            start_menu_dir(programs)
        );
        assert_eq!(programs_gui_link(programs).parent().unwrap(), programs);
        assert_eq!(
            programs_uninstall_link(programs).file_name().unwrap(),
            std::ffi::OsStr::new("卸载 校园网认证客户端.lnk")
        );
    }

    #[test]
    fn create_and_remove_shortcut_in_temp() {
        let dir = std::env::temp_dir().join(format!(
            "drcom-lnk-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("drcom4scutGUI.exe");
        std::fs::write(&target, b"mz").unwrap();
        let link = dir.join("校园网认证客户端.lnk");
        create_shortcut(&link, &target, &dir, "").expect("应能创建快捷方式");
        assert!(link.is_file());
        remove_if_target(&link, &dir.join("other.exe")).unwrap();
        assert!(
            link.exists(),
            "another install must not remove this shortcut"
        );
        std::fs::remove_file(&target).unwrap();
        remove_if_target(&link, &target).unwrap();
        assert!(
            !link.exists(),
            "stored target must work even after payload deletion"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
