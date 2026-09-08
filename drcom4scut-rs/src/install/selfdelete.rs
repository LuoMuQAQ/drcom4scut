//! 卸载器自删除：复制到仅管理员可写的临时目录，校验后再清理安装目录。

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use super::acl::{create_new_secure_dir, data_container_sddl};

pub fn staging_dir(install_id: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "drcom4scut-uninst-{install_id}-{}",
        super::identity::new_install_id()
    ))
}

pub fn copy_self_to_secure_temp(self_exe: &Path, install_id: &str) -> Result<PathBuf, String> {
    if !super::sid::is_elevated() {
        return Err("卸载清理需要管理员权限。".into());
    }
    copy_with_acl(self_exe, install_id, &data_container_sddl())
}
fn copy_with_acl(self_exe: &Path, install_id: &str, sddl: &str) -> Result<PathBuf, String> {
    if !super::identity::valid_install_id(install_id) {
        return Err("无效安装 ID。".into());
    }
    let dir = staging_dir(install_id);
    create_new_secure_dir(&dir, sddl)?;
    let dest = dir.join("uninstall.exe");
    let result = (|| {
        std::fs::copy(self_exe, &dest).map_err(|e| format!("复制卸载器失败：{e}"))?;
        if hash_file(self_exe)? != hash_file(&dest)? {
            return Err("卸载器副本哈希不匹配。".into());
        }
        Ok(dest)
    })();
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&dir);
    }
    result
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupTarget {
    pub install_dir: String,
    pub install_id: String,
}

pub fn write_cleanup_target(
    copy_exe: &Path,
    install_dir: &Path,
    install_id: &str,
) -> Result<(), String> {
    let dir = copy_exe.parent().ok_or("卸载副本缺少目录")?;
    let target = CleanupTarget {
        install_dir: install_dir.to_string_lossy().into_owned(),
        install_id: install_id.to_string(),
    };
    std::fs::write(
        dir.join("target.json"),
        serde_json::to_vec_pretty(&target).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}

fn validate_copy_path(copy_exe: &Path) -> Result<&Path, String> {
    let dir = copy_exe.parent().ok_or("卸载副本缺少目录")?;
    let name = dir
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or("无效暂存目录")?;
    let suffix = name
        .strip_prefix("drcom4scut-uninst-")
        .ok_or("无效暂存目录")?;
    if !suffix
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        || suffix.is_empty()
        || copy_exe.file_name() != Some(std::ffi::OsStr::new("uninstall.exe"))
        || dir.parent().map(super::identity::normalize_for_compare)
            != Some(super::identity::normalize_for_compare(&std::env::temp_dir()))
    {
        return Err("卸载副本不在指定临时目录内。".into());
    }
    super::validate::reject_reparse_in_chain(copy_exe).map_err(|e| e.message())?;
    super::validate::reject_reparse_in_chain(&dir.join("target.json")).map_err(|e| e.message())?;
    Ok(dir)
}

pub fn read_cleanup_target(copy_exe: &Path) -> Result<CleanupTarget, String> {
    let dir = validate_copy_path(copy_exe)?;
    let raw = std::fs::read(dir.join("target.json")).map_err(|e| e.to_string())?;
    let target: CleanupTarget = serde_json::from_slice(&raw).map_err(|e| e.to_string())?;
    if !super::identity::valid_install_id(&target.install_id)
        || !dir
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with(&format!("drcom4scut-uninst-{}-", target.install_id))
    {
        return Err("卸载交接 ID 无效。".into());
    }
    super::validate::logical_normalize(&target.install_dir).map_err(|e| e.message())?;
    std::fs::remove_file(dir.join("target.json"))
        .map_err(|e| format!("无法删除卸载交接文件：{e}"))?;
    Ok(target)
}

pub fn cleanup_copy(copy_exe: &Path) -> Result<(), String> {
    cleanup_copy_with(copy_exe, pending_delete)
}
fn cleanup_copy_with(
    copy_exe: &Path,
    mut schedule: impl FnMut(&Path) -> bool,
) -> Result<(), String> {
    let dir = validate_copy_path(copy_exe)?;
    super::flow::leave_install_dir(dir);
    let target = dir.join("target.json");
    if target.exists() {
        std::fs::remove_file(&target).map_err(|e| format!("清理交接文件失败：{e}"))?;
    }
    if copy_exe.exists() && !try_delete_now(copy_exe) && !schedule(copy_exe) {
        return Err(format!(
            "无法登记重启后删除卸载副本：{}",
            copy_exe.display()
        ));
    }
    // Windows processes pending deletes in order: executable first, directory second.
    if dir.exists() && std::fs::remove_dir(dir).is_err() && !schedule(dir) {
        return Err(format!("无法登记重启后删除暂存目录：{}", dir.display()));
    }
    Ok(())
}

pub fn hash_file(path: &Path) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    Ok(format!("{:x}", Sha256::digest(&bytes)))
}

/// 登记重启后删除（文件仍被当前进程占用时）。
pub fn pending_delete(path: &Path) -> bool {
    use windows::Win32::Storage::FileSystem::{MoveFileExW, MOVEFILE_DELAY_UNTIL_REBOOT};
    let wide: Vec<u16> = {
        use std::os::windows::ffi::OsStrExt;
        path.as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    };
    unsafe {
        MoveFileExW(
            windows::core::PCWSTR(wide.as_ptr()),
            windows::core::PCWSTR::null(),
            MOVEFILE_DELAY_UNTIL_REBOOT,
        )
        .is_ok()
    }
}

pub fn try_delete_now(path: &Path) -> bool {
    std::fs::remove_file(path).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handoff_consumed_and_locked_copy_schedules_file_then_directory() {
        use std::os::windows::fs::OpenOptionsExt;
        let id = super::super::identity::new_install_id();
        let dir = staging_dir(&id);
        std::fs::create_dir(&dir).unwrap();
        let copy = dir.join("uninstall.exe");
        std::fs::write(&copy, b"fake-uninstaller").unwrap();
        write_cleanup_target(&copy, Path::new(r"C:\TestApp"), &id).unwrap();
        assert_eq!(read_cleanup_target(&copy).unwrap().install_id, id);
        assert!(!dir.join("target.json").exists());
        let lock = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&copy)
            .unwrap();
        let mut scheduled = Vec::new();
        cleanup_copy_with(&copy, |p| {
            scheduled.push(p.to_path_buf());
            true
        })
        .unwrap();
        assert_eq!(scheduled, vec![copy.clone(), dir.clone()]);
        assert!(cleanup_copy_with(&copy, |_| false).is_err());
        drop(lock);
        cleanup_copy_with(&copy, |_| panic!("unlocked fake needs no reboot")).unwrap();
        assert!(!dir.exists());
    }
    #[test]
    fn temp_paths_are_unique_and_invalid_ids_cannot_escape() {
        assert_ne!(staging_dir("same-install"), staging_dir("same-install"));
        assert!(copy_self_to_secure_temp(Path::new("unused.exe"), "../escape").is_err());
        assert!(validate_copy_path(Path::new(r"C:\Windows\uninstall.exe")).is_err());
    }

    #[test]
    fn copy_verifies_hash_and_rejects_mismatch() {
        let dir = std::env::temp_dir().join(format!(
            "drcom-selfdel-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("uninstall.exe");
        std::fs::write(&src, b"uninstaller-bytes").unwrap();
        let id = format!(
            "t{}{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let sid = super::super::sid::current_user_sid().unwrap();
        let sddl = super::super::acl::user_data_sddl(&sid).unwrap();
        let copy = copy_with_acl(&src, &id, &sddl).unwrap();
        assert!(copy.is_file());
        assert_eq!(hash_file(&src).unwrap(), hash_file(&copy).unwrap());
        std::fs::write(&copy, b"tampered").unwrap();
        assert_ne!(hash_file(&src).unwrap(), hash_file(&copy).unwrap());
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(copy.parent().unwrap());
    }
}
