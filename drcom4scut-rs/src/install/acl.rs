//! 安装目录 ACL。用 SDDL 描述，安装器提权后应用到真实目录。

use std::path::Path;

use windows::core::PCWSTR;
use windows::Win32::Foundation::{LocalFree, HLOCAL};
use windows::Win32::Security::Authorization::{
    ConvertStringSecurityDescriptorToSecurityDescriptorW, SetNamedSecurityInfoW, SDDL_REVISION,
    SE_FILE_OBJECT,
};
use windows::Win32::Security::{
    GetSecurityDescriptorDacl, ACL, DACL_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION,
    PSECURITY_DESCRIPTOR,
};

use super::sid::is_plausible_sid;

/// 安装根、runtime、licenses、uninstall/GUI：SYSTEM+Administrators 完全控制，Users 读执行。
pub fn install_tree_sddl() -> String {
    // FA = FILE_ALL_ACCESS；0x1200a9 = FILE_GENERIC_READ|FILE_GENERIC_EXECUTE
    "D:PAI(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;0x1200a9;;;BU)".to_string()
}

/// `data` 与 `data\users`：仅 SYSTEM 与 Administrators。
pub fn data_container_sddl() -> String {
    "D:PAI(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)".to_string()
}

/// 用户 SID 数据目录：SYSTEM+Administrators 完全控制，该用户修改。
pub fn user_data_sddl(sid: &str) -> Result<String, String> {
    if !is_plausible_sid(sid) {
        return Err("无效的用户 SID。".into());
    }
    // 0x1301bf ≈ MODIFY（含删除，不含改权限）
    Ok(format!(
        "D:PAI(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;0x1301bf;;;{sid})"
    ))
}

pub fn apply_sddl(path: &Path, sddl: &str) -> Result<(), String> {
    let path_w: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let sddl_w: Vec<u16> = sddl.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        let mut sd = PSECURITY_DESCRIPTOR::default();
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            PCWSTR(sddl_w.as_ptr()),
            SDDL_REVISION,
            &mut sd,
            None,
        )
        .map_err(|e| format!("SDDL 无效：{e}"))?;
        let mut present: i32 = 0;
        let mut defaulted: i32 = 0;
        let mut dacl: *mut ACL = std::ptr::null_mut();
        GetSecurityDescriptorDacl(
            sd,
            &mut present as *mut i32 as *mut _,
            &mut dacl,
            &mut defaulted as *mut i32 as *mut _,
        )
        .map_err(|e| format!("读取 DACL 失败：{e}"))?;
        let err = SetNamedSecurityInfoW(
            PCWSTR(path_w.as_ptr()),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            None,
            None,
            if dacl.is_null() {
                None
            } else {
                Some(dacl as *const ACL)
            },
            None,
        );
        let _ = LocalFree(Some(HLOCAL(sd.0 as *mut _)));
        if err != windows::Win32::Foundation::WIN32_ERROR(0) {
            return Err(format!("设置 ACL 失败（{}）：{}", err.0, path.display()));
        }
    }
    Ok(())
}

use std::os::windows::ffi::OsStrExt;

pub fn apply_install_tree(path: &Path) -> Result<(), String> {
    apply_sddl(path, &install_tree_sddl())
}

pub fn apply_data_container(path: &Path) -> Result<(), String> {
    apply_sddl(path, &data_container_sddl())
}

pub fn apply_user_data(path: &Path, sid: &str) -> Result<(), String> {
    apply_sddl(path, &user_data_sddl(sid)?)
}

/// 创建目录并立刻套上 ACL，避免继承到宽泛写入权限的窗口。
pub fn create_dir_with_sddl(path: &Path, sddl: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        if !parent.exists() {
            return Err(format!("父目录不存在：{}", parent.display()));
        }
    }
    if !path.exists() {
        std::fs::create_dir(path).map_err(|e| format!("创建目录失败：{e}"))?;
    }
    apply_sddl(path, sddl)
}

/// Create a NEW directory with its protected ACL applied atomically by Windows.
pub fn create_new_secure_dir(path: &Path, sddl: &str) -> Result<(), String> {
    use windows::Win32::Security::SECURITY_ATTRIBUTES;
    use windows::Win32::Storage::FileSystem::CreateDirectoryW;
    super::validate::reject_reparse_in_chain(path).map_err(|e| e.message())?;
    let path_w: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let sddl_w: Vec<u16> = sddl.encode_utf16().chain(Some(0)).collect();
    unsafe {
        let mut sd = PSECURITY_DESCRIPTOR::default();
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            PCWSTR(sddl_w.as_ptr()),
            SDDL_REVISION,
            &mut sd,
            None,
        )
        .map_err(|e| e.to_string())?;
        let sa = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: sd.0,
            bInheritHandle: false.into(),
        };
        let result = CreateDirectoryW(PCWSTR(path_w.as_ptr()), Some(&sa))
            .map_err(|e| format!("创建安全暂存目录失败：{e}"));
        let _ = LocalFree(Some(HLOCAL(sd.0)));
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sddl_contains_expected_trustees() {
        let s = install_tree_sddl();
        assert!(s.contains("PAI"), "应保护 DACL 禁止继承");
        assert!(s.contains(";;;SY"));
        assert!(s.contains(";;;BA"));
        assert!(s.contains(";;;BU"));
        assert!(!s.contains("WD")); // 不应给 Everyone
    }

    #[test]
    fn user_sddl_embeds_sid_and_rejects_garbage() {
        let sid = "S-1-5-21-1-2-3-1001";
        let s = user_data_sddl(sid).unwrap();
        assert!(s.contains(sid));
        assert!(s.contains("0x1301bf"));
        assert!(user_data_sddl("../S-1-5-18").is_err());
        assert!(user_data_sddl("S-1").is_err());
    }

    #[test]
    fn data_container_has_no_users_ace() {
        let s = data_container_sddl();
        assert!(!s.contains(";;;BU"));
        assert!(!s.contains(";;;AU"));
        assert!(s.contains(";;;SY") && s.contains(";;;BA"));
    }

    #[test]
    fn apply_sddl_on_temp_dir() {
        let dir = std::env::temp_dir().join(format!(
            "drcom-acl-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        apply_install_tree(&dir).expect("当前用户应能在临时目录设置 ACL");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
