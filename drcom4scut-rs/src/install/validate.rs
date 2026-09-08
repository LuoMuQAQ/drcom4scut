//! 安装路径规范化与拒绝策略。纯逻辑为主，便于在临时目录验证。

use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathReject {
    Empty,
    NotAbsolute,
    UncOrDevice,
    HasParentDir,
    DriveRoot,
    SystemFolder,
    InvalidChars,
    TooLong,
    ReservedName,
    ReparsePoint(String),
    NetworkDrive,
    OtherAppDirectory,
}

impl PathReject {
    pub fn message(&self) -> String {
        match self {
            PathReject::Empty => "请输入安装目录。".into(),
            PathReject::NotAbsolute => "安装目录必须是绝对路径。".into(),
            PathReject::UncOrDevice => "不支持网络路径或设备路径。".into(),
            PathReject::HasParentDir => "安装目录不能包含相对上级引用。".into(),
            PathReject::DriveRoot => "不能安装到磁盘根目录。".into(),
            PathReject::SystemFolder => "不能安装到 Windows 系统目录。".into(),
            PathReject::InvalidChars => "安装路径包含非法字符。".into(),
            PathReject::TooLong => "安装路径过长。".into(),
            PathReject::ReservedName => "安装路径包含 Windows 保留名。".into(),
            PathReject::ReparsePoint(p) => format!("路径包含重解析点，已拒绝：{p}"),
            PathReject::NetworkDrive => "不支持网络驱动器。".into(),
            PathReject::OtherAppDirectory => "目标目录已有其他应用文件，请另选目录。".into(),
        }
    }
}

const MAX_INSTALL_PATH: usize = 200;
const RESERVED: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// 逻辑规范化：统一分隔符、去掉尾部分隔符、折叠 `.`，拒绝 `..` 与 UNC。
pub fn logical_normalize(input: &str) -> Result<PathBuf, PathReject> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(PathReject::Empty);
    }
    if trimmed
        .chars()
        .any(|c| matches!(c, '<' | '>' | '"' | '|' | '?' | '*' | '\0'))
    {
        return Err(PathReject::InvalidChars);
    }
    let unified = trimmed.replace('/', "\\");
    if unified.starts_with("\\\\") || unified.starts_with("//") {
        return Err(PathReject::UncOrDevice);
    }
    if unified.starts_with("\\\\?\\") || unified.to_ascii_lowercase().starts_with("\\\\.\\") {
        return Err(PathReject::UncOrDevice);
    }
    let path = PathBuf::from(&unified);
    if !path.is_absolute() {
        return Err(PathReject::NotAbsolute);
    }
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::Prefix(p) => out.push(p.as_os_str()),
            Component::RootDir => out.push("\\"),
            Component::CurDir => {}
            Component::ParentDir => return Err(PathReject::HasParentDir),
            Component::Normal(s) => {
                let name = s.to_string_lossy();
                if name.ends_with(['.', ' ']) || name.chars().any(|c| matches!(c, ':' | '*')) {
                    return Err(PathReject::InvalidChars);
                }
                let stem = name.split('.').next().unwrap_or(&name);
                if RESERVED.iter().any(|r| stem.eq_ignore_ascii_case(r)) {
                    return Err(PathReject::ReservedName);
                }
                out.push(s);
            }
        }
    }
    let rendered = out.to_string_lossy();
    if rendered.len() > MAX_INSTALL_PATH {
        return Err(PathReject::TooLong);
    }
    Ok(out)
}

/// 盘符根，如 `C:\`。
pub fn is_drive_root(path: &Path) -> bool {
    let mut comps = path.components();
    matches!(comps.next(), Some(Component::Prefix(_)))
        && matches!(comps.next(), Some(Component::RootDir))
        && comps.next().is_none()
}

pub fn is_under(child: &Path, parent: &Path) -> bool {
    let c = crate::install::identity::normalize_for_compare(child);
    let p = crate::install::identity::normalize_for_compare(parent);
    c == p || c.starts_with(&(p.clone() + "\\"))
}

/// `system_root` 一般为 `C:\Windows`；`program_files_x86` 用于避免默认落到 32 位目录。
pub fn classify_system_folder(
    path: &Path,
    system_root: &Path,
    program_files_x86: Option<&Path>,
) -> Result<(), PathReject> {
    if is_drive_root(path) {
        return Err(PathReject::DriveRoot);
    }
    let sensitive = [
        system_root.to_path_buf(),
        system_root.join("System32"),
        system_root.join("SysWOW64"),
        system_root.join("WinSxS"),
        system_root.join("Sysnative"),
    ];
    for s in &sensitive {
        if is_under(path, s) || is_under(s, path) && path != s && is_drive_root(path) {
            return Err(PathReject::SystemFolder);
        }
        if identity_equal(path, s) {
            return Err(PathReject::SystemFolder);
        }
    }
    // 允许安装到 Program Files 的子目录，但拒绝系统目录本身以及 Windows 根。
    if identity_equal(path, system_root) || is_under(path, system_root) {
        return Err(PathReject::SystemFolder);
    }
    if let Some(x86) = program_files_x86 {
        // 用户仍可显式选择 x86 子目录；此处只拒绝把「系统目录本身」当安装点。
        if identity_equal(path, x86) {
            return Err(PathReject::SystemFolder);
        }
    }
    Ok(())
}

fn identity_equal(a: &Path, b: &Path) -> bool {
    crate::install::identity::normalize_for_compare(a)
        == crate::install::identity::normalize_for_compare(b)
}

/// 路径上任一已存在组件若为重解析点则拒绝。
pub fn reject_reparse_in_chain(path: &Path) -> Result<(), PathReject> {
    let mut cur = PathBuf::new();
    for comp in path.components() {
        cur.push(comp);
        if is_reparse_point(&cur) {
            return Err(PathReject::ReparsePoint(cur.display().to_string()));
        }
    }
    Ok(())
}

pub fn is_reparse_point(path: &Path) -> bool {
    use windows::Win32::Storage::FileSystem::{
        GetFileAttributesW, FILE_ATTRIBUTE_REPARSE_POINT, INVALID_FILE_ATTRIBUTES,
    };
    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    // SAFETY: wide 在调用期间有效。
    let attr = unsafe { GetFileAttributesW(windows::core::PCWSTR(wide.as_ptr())) };
    attr != INVALID_FILE_ATTRIBUTES && (attr & FILE_ATTRIBUTE_REPARSE_POINT.0) != 0
}

use std::os::windows::ffi::OsStrExt;

pub fn drive_is_remote(path: &Path) -> bool {
    use windows::Win32::Storage::FileSystem::GetDriveTypeW;
    use windows::Win32::System::WindowsProgramming::DRIVE_REMOTE;
    let Some(prefix) = path.components().next() else {
        return false;
    };
    let mut root = PathBuf::new();
    root.push(prefix);
    root.push("\\");
    let wide: Vec<u16> = root
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    unsafe { GetDriveTypeW(windows::core::PCWSTR(wide.as_ptr())) == DRIVE_REMOTE }
}

/// 完整校验：规范化 + 系统目录 + 重解析点 + 网络盘。
pub fn validate_install_dir(
    input: &str,
    system_root: &Path,
    program_files_x86: Option<&Path>,
) -> Result<PathBuf, PathReject> {
    let path = logical_normalize(input)?;
    classify_system_folder(&path, system_root, program_files_x86)?;
    if drive_is_remote(&path) {
        return Err(PathReject::NetworkDrive);
    }
    reject_reparse_in_chain(&path)?;
    Ok(path)
}

/// 升级识别：目录内存在本产品安装身份。
pub fn is_our_install(dir: &Path) -> bool {
    crate::install::identity::load_state(&dir.join("install-state.json"))
        .ok()
        .is_some_and(|s| crate::install::identity::state_matches_dir(&s, dir))
}

/// Generic names are never sufficient proof that an unmarked directory is ours.
pub fn looks_like_our_residue(_dir: &Path) -> bool {
    false
}

/// Only exact transaction backup names are owned, never arbitrary *.bak-upgrade.
pub fn is_upgrade_backup(name: &str) -> bool {
    ["drcom4scutGUI.bak-upgrade", "uninstall.bak-upgrade"]
        .iter()
        .any(|n| name.eq_ignore_ascii_case(n))
}
pub fn is_install_cache(name: &str) -> bool {
    name == ".install-staging"
        || name
            .strip_prefix(".npcap-setup-")
            .is_some_and(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// Preflight the entire owned tree, including dangling links, before mutating it.
pub fn reject_reparse_tree(path: &Path) -> Result<(), String> {
    reject_reparse_in_chain(path).map_err(|e| e.message())?;
    let meta = match std::fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(format!("无法检查 {}：{e}", path.display())),
    };
    if meta.is_dir() {
        for entry in std::fs::read_dir(path).map_err(|e| e.to_string())? {
            reject_reparse_tree(&entry.map_err(|e| e.to_string())?.path())?;
        }
    }
    Ok(())
}

/// 无有效安装身份的非空目录一律拒绝；通用目录名不构成所有权证据。
pub fn reject_foreign_occupied(dir: &Path) -> Result<(), PathReject> {
    if !dir.exists() {
        return Ok(());
    }
    if is_our_install(dir) {
        return Ok(());
    }
    let ok = match std::fs::read_dir(dir) {
        Ok(mut rd) => rd.next().is_none(),
        Err(_) => false,
    };
    if ok {
        Ok(())
    } else {
        Err(PathReject::OtherAppDirectory)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_relative_unc_and_dots() {
        assert_eq!(logical_normalize(""), Err(PathReject::Empty));
        assert_eq!(logical_normalize("foo\\bar"), Err(PathReject::NotAbsolute));
        assert_eq!(
            logical_normalize(r"\\server\share\app"),
            Err(PathReject::UncOrDevice)
        );
        assert_eq!(
            logical_normalize(r"C:\apps\..\Windows"),
            Err(PathReject::HasParentDir)
        );
        assert!(matches!(
            logical_normalize(r"C:\apps\foo*"),
            Err(PathReject::InvalidChars)
        ));
    }

    #[test]
    fn accepts_normal_absolute() {
        let p = logical_normalize(r"C:\Program Files\drcom4scutGUI").unwrap();
        assert_eq!(p, PathBuf::from(r"C:\Program Files\drcom4scutGUI"));
        let p = logical_normalize(r"D:/Apps/drcom4scutGUI/").unwrap();
        assert_eq!(p, PathBuf::from(r"D:\Apps\drcom4scutGUI"));
    }

    #[test]
    fn drive_root_and_windows_rejected() {
        let win = Path::new(r"C:\Windows");
        assert_eq!(
            classify_system_folder(Path::new(r"C:\"), win, None),
            Err(PathReject::DriveRoot)
        );
        assert_eq!(
            classify_system_folder(Path::new(r"C:\Windows"), win, None),
            Err(PathReject::SystemFolder)
        );
        assert_eq!(
            classify_system_folder(Path::new(r"C:\Windows\System32\foo"), win, None),
            Err(PathReject::SystemFolder)
        );
        assert!(classify_system_folder(
            Path::new(r"C:\Program Files\drcom4scutGUI"),
            win,
            Some(Path::new(r"C:\Program Files (x86)"))
        )
        .is_ok());
        assert_eq!(
            classify_system_folder(
                Path::new(r"C:\Program Files (x86)"),
                win,
                Some(Path::new(r"C:\Program Files (x86)"))
            ),
            Err(PathReject::SystemFolder)
        );
    }

    #[test]
    fn reserved_names_rejected() {
        assert_eq!(
            logical_normalize(r"C:\apps\CON"),
            Err(PathReject::ReservedName)
        );
    }

    #[test]
    fn foreign_dir_rejected_empty_ok() {
        let dir = std::env::temp_dir().join(format!(
            "drcom-val-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        assert!(reject_foreign_occupied(&dir).is_ok());
        std::fs::write(dir.join("readme.txt"), b"hi").unwrap();
        assert_eq!(
            reject_foreign_occupied(&dir),
            Err(PathReject::OtherAppDirectory)
        );
        let _ = std::fs::remove_dir_all(&dir);
        assert!(reject_foreign_occupied(&dir.join("missing")).is_ok());
    }

    #[test]
    fn unmarked_residue_is_rejected() {
        let dir = std::env::temp_dir().join(format!(
            "drcom-val-res-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join("runtime")).unwrap();
        std::fs::write(dir.join("drcom4scutGUI.bak-upgrade"), b"old").unwrap();
        assert!(!looks_like_our_residue(&dir));
        assert!(reject_foreign_occupied(&dir).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
