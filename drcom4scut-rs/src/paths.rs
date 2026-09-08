//! 运行路径：安装版与便携版分离。
//!
//! - **便携版**（EXE 旁没有可信 `install-state.json`）：数据根仍为
//!   `%LOCALAPPDATA%\drcom4scutGUI`，与历史契约一致。
//! - **安装版**（EXE 旁存在通过校验的安装身份文件）：数据根为
//!   `<InstallDir>\data\users\<SID>`，核心在 `<InstallDir>\runtime\<sha>\`。
//!   不得悄悄回落到 LocalAppData。
//!
//! 路径由 EXE 所在目录决定，不使用进程当前工作目录。

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::install::identity::{self, InstallState, PRODUCT_ID};
use crate::install::sid;

/// 产品在便携模式下的 LocalAppData 目录名。
pub const PORTABLE_DIR_NAME: &str = "drcom4scutGUI";

/// 安装身份文件名。
pub const INSTALL_STATE_FILE: &str = "install-state.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppModeKind {
    Portable,
    Installed,
}

#[derive(Debug, Clone)]
pub struct PathContext {
    pub kind: AppModeKind,
    pub exe_dir: PathBuf,
    pub install_dir: Option<PathBuf>,
    pub user_sid: String,
    /// 用户可写数据根（设置、配置、日志、图标释放）。
    pub data_root: PathBuf,
    /// 受保护核心根目录（其下再按 SHA 分子目录）。
    pub runtime_root: PathBuf,
}

static CONTEXT: OnceLock<PathContext> = OnceLock::new();

thread_local! {
    static TEST_OVERRIDE: RefCell<Option<PathContext>> = const { RefCell::new(None) };
}

fn current_context() -> PathContext {
    #[cfg(test)]
    {
        if let Some(ctx) = TEST_OVERRIDE.with(|slot| slot.borrow().clone()) {
            return ctx;
        }
    }
    CONTEXT.get_or_init(detect_context).clone()
}

/// 根据当前 EXE 目录检测模式。测试可通过 [`set_context_for_test`] 覆盖。
pub fn detect_context() -> PathContext {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."));
    detect_context_from(
        &exe_dir,
        sid::current_user_sid().as_deref().unwrap_or("unknown"),
    )
}

pub fn detect_context_from(exe_dir: &Path, user_sid: &str) -> PathContext {
    let state_path = exe_dir.join(INSTALL_STATE_FILE);
    if let Ok(state) = identity::load_state(&state_path) {
        if identity::state_matches_dir(&state, exe_dir) {
            return installed_context(exe_dir, user_sid, &state);
        }
    }
    portable_context(exe_dir, user_sid)
}

fn portable_data_root() -> PathBuf {
    let local = std::env::var_os("LOCALAPPDATA")
        .unwrap_or_else(|| std::env::temp_dir().join("LocalAppData").into_os_string());
    PathBuf::from(local).join(PORTABLE_DIR_NAME)
}

fn portable_context(exe_dir: &Path, user_sid: &str) -> PathContext {
    let data_root = portable_data_root();
    PathContext {
        kind: AppModeKind::Portable,
        exe_dir: exe_dir.to_path_buf(),
        install_dir: None,
        user_sid: user_sid.to_string(),
        runtime_root: data_root.join("runtime"),
        data_root,
    }
}

fn installed_context(exe_dir: &Path, user_sid: &str, _state: &InstallState) -> PathContext {
    PathContext {
        kind: AppModeKind::Installed,
        exe_dir: exe_dir.to_path_buf(),
        install_dir: Some(exe_dir.to_path_buf()),
        user_sid: user_sid.to_string(),
        data_root: exe_dir.join("data").join("users").join(user_sid),
        runtime_root: exe_dir.join("runtime"),
    }
}

/// 供测试注入路径上下文。
pub fn set_context_for_test(ctx: PathContext) {
    TEST_OVERRIDE.with(|slot| *slot.borrow_mut() = Some(ctx));
}

/// 清除测试覆盖，恢复自动检测。
pub fn clear_context_for_test() {
    TEST_OVERRIDE.with(|slot| *slot.borrow_mut() = None);
}

pub fn context() -> PathContext {
    current_context()
}

pub fn is_installed() -> bool {
    current_context().kind == AppModeKind::Installed
}

pub fn is_portable() -> bool {
    current_context().kind == AppModeKind::Portable
}

/// 用户数据根目录。
pub fn root() -> Option<PathBuf> {
    Some(current_context().data_root)
}

/// 加密设置文件。
pub fn settings_file() -> Option<PathBuf> {
    Some(root()?.join("settings.v1.dat"))
}

/// 交给核心的配置文件。
pub fn config_file() -> Option<PathBuf> {
    Some(root()?.join("config").join("config.yml"))
}

/// 核心日志（GUI 只读，不写）。
pub fn core_log_file() -> Option<PathBuf> {
    Some(root()?.join("logs").join("latest.log"))
}

/// 释放出来的核心可执行文件目录：按核心 SHA-256 分目录。
pub fn runtime_dir(core_sha256: &str) -> Option<PathBuf> {
    Some(current_context().runtime_root.join(core_sha256))
}

/// 已释放的核心可执行文件完整路径。
pub fn core_exe(core_sha256: &str) -> Option<PathBuf> {
    Some(runtime_dir(core_sha256)?.join("drcom4scut.exe"))
}

/// 旧版明文设置（迁移用，位于 exe 同目录）。
pub fn legacy_gui_json() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    Some(exe.parent()?.join("gui.json"))
}

/// 历史便携/旧 GUI 的 LocalAppData 根（只读迁移源，安装版不得删除）。
pub fn legacy_local_app_data_root() -> Option<PathBuf> {
    let dir = std::env::var_os("LOCALAPPDATA")?;
    Some(PathBuf::from(dir).join(PORTABLE_DIR_NAME))
}

/// 确保数据根、config、logs 目录存在。安装版若无权创建 SID 目录则返回错误。
pub fn ensure_dirs() -> std::io::Result<()> {
    if let Some(root) = root() {
        std::fs::create_dir_all(root.join("config"))?;
        std::fs::create_dir_all(root.join("logs"))?;
    }
    Ok(())
}

/// 首次运行时写入默认 YAML（已存在则不覆盖）。
pub fn ensure_default_config() -> std::io::Result<()> {
    ensure_dirs()?;
    let Some(path) = config_file() else {
        return Err(std::io::Error::other("无法确定配置文件路径"));
    };
    if path.exists() {
        return Ok(());
    }
    const DEFAULT_YAML: &[u8] = include_bytes!("../resources/default_config.yml");
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, DEFAULT_YAML)
}

/// 安装版：当前用户数据目录是否已存在且可写。
pub fn user_data_ready() -> bool {
    let ctx = current_context();
    if ctx.kind != AppModeKind::Installed {
        return true;
    }
    let marker = ctx.data_root.join(".ready");
    ctx.data_root.is_dir() && (marker.is_file() || ctx.data_root.join("config").is_dir())
}

pub fn mark_user_data_ready(data_root: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(data_root.join("config"))?;
    std::fs::create_dir_all(data_root.join("logs"))?;
    std::fs::write(data_root.join(".ready"), PRODUCT_ID.as_bytes())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unique_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "drcom4scut-paths-{}-{}-{}",
            tag,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn paths_are_rooted_in_local_app_data() {
        clear_context_for_test();
        let local = std::env::var("LOCALAPPDATA").unwrap();
        let root = portable_data_root();
        assert!(root.starts_with(&local), "root = {root:?}");
        assert_eq!(root.file_name().unwrap(), "drcom4scutGUI");
        let ctx = portable_context(Path::new(r"C:\tmp\app"), "S-1-5-21-1");
        set_context_for_test(ctx);
        assert_eq!(
            settings_file().unwrap().file_name().unwrap(),
            "settings.v1.dat"
        );
        assert_eq!(config_file().unwrap().file_name().unwrap(), "config.yml");
        assert!(is_portable());
        assert!(!is_installed());
        clear_context_for_test();
    }

    #[test]
    fn runtime_dir_is_keyed_by_sha() {
        let sha = "ce79e117d14d172cb172a7d2a8adb2c638eb32d952db28d8ce04602eb5445ec2";
        let ctx = portable_context(Path::new(r"C:\tmp\app"), "S-1-5-21-1");
        set_context_for_test(ctx);
        let exe = core_exe(sha).unwrap();
        assert!(exe.ends_with("drcom4scut.exe"));
        assert!(exe.parent().unwrap().ends_with(sha));
        clear_context_for_test();
    }

    #[test]
    fn installed_layout_uses_sid_and_exe_dir() {
        let dir = unique_dir("installed");
        let sid = "S-1-5-21-1000-2000-3000-1001";
        let state = InstallState::new_for_test(&dir, sid);
        identity::save_state(&dir.join(INSTALL_STATE_FILE), &state).unwrap();
        let ctx = detect_context_from(&dir, sid);
        assert_eq!(ctx.kind, AppModeKind::Installed);
        assert_eq!(ctx.data_root, dir.join("data").join("users").join(sid));
        assert_eq!(ctx.runtime_root, dir.join("runtime"));
        assert_eq!(
            ctx.data_root.file_name().unwrap(),
            std::ffi::OsStr::new(sid)
        );
        assert!(ctx
            .data_root
            .ends_with(Path::new("data").join("users").join(sid)));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn installed_mode_ignores_process_cwd() {
        let dir = unique_dir("cwd");
        let sid = "S-1-5-21-cwd";
        let state = InstallState::new_for_test(&dir, sid);
        identity::save_state(&dir.join(INSTALL_STATE_FILE), &state).unwrap();
        let cwd = std::env::current_dir().unwrap();
        let ctx = detect_context_from(&dir, sid);
        set_context_for_test(ctx);
        assert_eq!(root().unwrap(), dir.join("data").join("users").join(sid));
        assert_eq!(std::env::current_dir().unwrap(), cwd);
        clear_context_for_test();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_or_foreign_state_stays_portable() {
        let dir = unique_dir("foreign");
        std::fs::write(dir.join(INSTALL_STATE_FILE), b"{\"productId\":\"other\"}").unwrap();
        let ctx = detect_context_from(&dir, "S-1-5-21-x");
        assert_eq!(ctx.kind, AppModeKind::Portable);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sid_data_dirs_are_isolated() {
        let dir = unique_dir("sid");
        let a = installed_context(
            &dir,
            "S-1-5-21-A",
            &InstallState::new_for_test(&dir, "S-1-5-21-A"),
        );
        let b = installed_context(
            &dir,
            "S-1-5-21-B",
            &InstallState::new_for_test(&dir, "S-1-5-21-B"),
        );
        assert_ne!(a.data_root, b.data_root);
        assert_eq!(a.runtime_root, b.runtime_root);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
