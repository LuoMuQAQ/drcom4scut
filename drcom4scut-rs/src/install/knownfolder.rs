//! Known Folder 路径。64 位进程取 FOLDERID_ProgramFiles → 通常为 C:\Program Files。

use std::path::PathBuf;

use windows::core::GUID;
use windows::Win32::Foundation::E_FAIL;
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::UI::Shell::{
    FOLDERID_CommonPrograms, FOLDERID_Desktop, FOLDERID_ProgramFiles, FOLDERID_ProgramFilesX86,
    FOLDERID_Programs, FOLDERID_PublicDesktop, SHGetKnownFolderPath, KF_FLAG_DEFAULT,
};

fn known_folder(id: &GUID) -> Result<PathBuf, String> {
    unsafe {
        let pw = SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None)
            .map_err(|e| format!("SHGetKnownFolderPath 失败：{e}"))?;
        if pw.is_null() {
            return Err(format!("SHGetKnownFolderPath 返回空：{E_FAIL:?}"));
        }
        let s = pw.to_string().map_err(|e| e.to_string())?;
        CoTaskMemFree(Some(pw.0 as *const _));
        Ok(PathBuf::from(s))
    }
}

/// 64 位 Program Files。常规 C 盘系统上为 `C:\Program Files`。
pub fn program_files() -> Result<PathBuf, String> {
    known_folder(&FOLDERID_ProgramFiles)
}

pub fn program_files_x86() -> Result<PathBuf, String> {
    known_folder(&FOLDERID_ProgramFilesX86)
}

pub fn default_install_dir() -> Result<PathBuf, String> {
    Ok(program_files()?.join("drcom4scutGUI"))
}

pub fn public_desktop() -> Result<PathBuf, String> {
    known_folder(&FOLDERID_PublicDesktop)
}

pub fn user_desktop() -> Result<PathBuf, String> {
    known_folder(&FOLDERID_Desktop)
}

pub fn common_programs() -> Result<PathBuf, String> {
    known_folder(&FOLDERID_CommonPrograms)
}

pub fn user_programs() -> Result<PathBuf, String> {
    known_folder(&FOLDERID_Programs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn program_files_is_64bit_not_x86() {
        let pf = program_files().expect("应能读取 Program Files");
        let name = pf.file_name().unwrap().to_string_lossy();
        assert!(
            !name.to_ascii_lowercase().contains("x86"),
            "64 位 Known Folder 不应落到 Program Files (x86)，实际 {pf:?}"
        );
        if let Ok(x86) = program_files_x86() {
            assert_ne!(pf, x86, "Program Files 与 (x86) 必须不同");
        }
        let def = default_install_dir().unwrap();
        assert_eq!(def.file_name().unwrap(), "drcom4scutGUI");
        assert_eq!(def.parent().unwrap(), pf.as_path());
    }
}
