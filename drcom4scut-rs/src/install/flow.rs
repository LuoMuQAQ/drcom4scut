//! 安装与卸载编排。文件事务可在临时目录实测；驱动/注册表走可注入接口。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::acl::{self, apply_install_tree, apply_user_data, create_dir_with_sddl};
use super::driver::DriverHost;
use super::identity::{self, new_install_id, InstallState};
use super::knownfolder;
use super::origin::Origin;
use super::registry;
use super::shortcuts;
use super::transaction::Transaction;
use super::validate::{self, PathReject};
use super::{GUI_EXE_NAME, UNINSTALL_EXE_NAME};
use crate::coreproc::{CORE_BYTES, CORE_SHA256};

#[derive(Debug, Clone)]
pub struct InstallOptions {
    pub install_dir: PathBuf,
    pub desktop_shortcut: bool,
    pub start_menu_shortcut: bool,
    pub install_npcap_if_missing: bool,
    pub launch_after: bool,
    pub oem_silent_npcap: Option<PathBuf>,
    pub oem_sha256: Option<String>,
}

impl Default for InstallOptions {
    fn default() -> Self {
        Self {
            install_dir: PathBuf::from(r"C:\Program Files\drcom4scutGUI"),
            desktop_shortcut: true,
            start_menu_shortcut: true,
            install_npcap_if_missing: true,
            launch_after: true,
            oem_silent_npcap: None,
            oem_sha256: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallResult {
    pub ok: bool,
    pub install_dir: String,
    pub version: String,
    pub driver: String,
    pub need_reboot: bool,
    pub message: String,
    pub launch: bool,
}

#[derive(Clone)]
pub struct Payload {
    pub gui: Vec<u8>,
    pub uninstall: Vec<u8>,
    pub notice: Vec<u8>,
    pub gpl: Vec<u8>,
    pub lucide: Vec<u8>,
}

pub fn default_payload() -> Result<Payload, String> {
    #[cfg(embed_payload)]
    {
        Ok(Payload {
            gui: include_bytes!(env!("DRCOM_GUI_EXE")).to_vec(),
            uninstall: include_bytes!(env!("DRCOM_UNINSTALL_EXE")).to_vec(),
            notice: include_bytes!("../../resources/licenses/NOTICE.txt").to_vec(),
            gpl: include_bytes!("../../resources/licenses/GPL-3.0.txt").to_vec(),
            lucide: include_bytes!("../../resources/lucide-LICENSE").to_vec(),
        })
    }
    #[cfg(not(embed_payload))]
    {
        Err("安装器未嵌入 GUI 负载。请使用 publish-setup.ps1 构建。".into())
    }
}

pub fn validate_options(
    opts: &InstallOptions,
    system_root: &Path,
    program_files_x86: Option<&Path>,
) -> Result<PathBuf, PathReject> {
    let dir = validate::validate_install_dir(
        &opts.install_dir.to_string_lossy(),
        system_root,
        program_files_x86,
    )?;
    validate::reject_foreign_occupied(&dir)?;
    Ok(dir)
}

/// Validate first; an unmarked nonempty directory is never cleaned automatically.
pub fn prepare_install_dir(dir: &Path) -> Result<(), String> {
    validate::reject_reparse_in_chain(dir).map_err(|e| e.message())?;
    validate::reject_foreign_occupied(dir).map_err(|e| e.message())?;
    if validate::is_our_install(dir) {
        let plan = plan_uninstall(&dir.join(UNINSTALL_EXE_NAME))?;
        preflight_owned(&plan)?;
        for entry in std::fs::read_dir(dir).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if validate::is_install_cache(&name) || validate::is_upgrade_backup(&name) {
                let p = entry.path();
                if p.is_dir() {
                    std::fs::remove_dir_all(&p)
                } else {
                    std::fs::remove_file(&p)
                }
                .map_err(|e| format!("无法清理 {}：{e}", p.display()))?;
            }
        }
    }
    Ok(())
}

pub fn install_files(
    opts: &InstallOptions,
    origin: &Origin,
    payload: &Payload,
    skip_acl: bool,
) -> Result<InstallState, String> {
    let dir = &opts.install_dir;
    prepare_install_dir(dir)?;
    std::fs::create_dir_all(dir).map_err(|e| format!("无法创建安装目录：{e}"))?;
    if !skip_acl {
        apply_install_tree(dir)?;
    }
    let mut tx = Transaction::begin(dir)?;
    tx.stage_file(GUI_EXE_NAME, &payload.gui)?;
    tx.stage_file(UNINSTALL_EXE_NAME, &payload.uninstall)?;
    tx.stage_file(&format!("runtime/{CORE_SHA256}/drcom4scut.exe"), CORE_BYTES)?;
    tx.stage_file("licenses/NOTICE.txt", &payload.notice)?;
    tx.stage_file("licenses/GPL-3.0.txt", &payload.gpl)?;
    tx.stage_file("licenses/lucide-LICENSE", &payload.lucide)?;
    if let Err(e) = tx.commit() {
        let _ = tx.rollback();
        return Err(e);
    }
    if !skip_acl {
        let _ = apply_install_tree(&dir.join(GUI_EXE_NAME));
        let _ = apply_install_tree(&dir.join(UNINSTALL_EXE_NAME));
        let runtime = dir.join("runtime");
        let _ = apply_install_tree(&runtime);
        let core_dir = runtime.join(CORE_SHA256);
        let _ = apply_install_tree(&core_dir);
        let _ = apply_install_tree(&core_dir.join("drcom4scut.exe"));
        let licenses = dir.join("licenses");
        let _ = apply_install_tree(&licenses);
        let data = dir.join("data");
        create_dir_with_sddl(&data, &acl::data_container_sddl())?;
        let users = data.join("users");
        create_dir_with_sddl(&users, &acl::data_container_sddl())?;
        let user_dir = users.join(&origin.sid);
        std::fs::create_dir_all(&user_dir).map_err(|e| e.to_string())?;
        apply_user_data(&user_dir, &origin.sid)?;
        crate::paths::mark_user_data_ready(&user_dir).map_err(|e| e.to_string())?;
    } else {
        let user_dir = dir.join("data").join("users").join(&origin.sid);
        let _ = crate::paths::mark_user_data_ready(&user_dir);
    }

    let mut state = InstallState::new(dir, &origin.sid, &new_install_id());
    state.desktop_shortcut = opts.desktop_shortcut;
    state.start_menu_shortcut = opts.start_menu_shortcut;
    identity::save_state(&dir.join("install-state.json"), &state)?;
    if !skip_acl {
        let _ = apply_install_tree(&dir.join("install-state.json"));
    }
    Ok(state)
}

pub fn create_shortcuts(opts: &InstallOptions) -> Result<(), String> {
    let gui = opts.install_dir.join(GUI_EXE_NAME);
    let un = opts.install_dir.join(UNINSTALL_EXE_NAME);
    if opts.desktop_shortcut {
        let desk = knownfolder::public_desktop()
            .or_else(|_| knownfolder::user_desktop())
            .map_err(|e| format!("无法定位桌面文件夹：{e}"))?;
        shortcuts::create_shortcut(
            &shortcuts::desktop_link_path(&desk),
            &gui,
            &opts.install_dir,
            "",
        )?;
    }
    if opts.start_menu_shortcut {
        let prog = knownfolder::common_programs()
            .or_else(|_| knownfolder::user_programs())
            .map_err(|e| format!("无法定位开始菜单文件夹：{e}"))?;
        shortcuts::create_shortcut(
            &shortcuts::programs_gui_link(&prog),
            &gui,
            &opts.install_dir,
            "",
        )?;
        shortcuts::create_shortcut(
            &shortcuts::programs_uninstall_link(&prog),
            &un,
            &opts.install_dir,
            "",
        )?;
    }
    shortcuts::notify_shell();
    Ok(())
}

pub fn register_uninstall(opts: &InstallOptions, payload: &Payload) -> Result<(), String> {
    let size_kb = ((payload.gui.len() + payload.uninstall.len() + CORE_BYTES.len()) / 1024) as u32;
    registry::write_uninstall_key(&opts.install_dir, size_kb.max(1), None)
}

pub fn run_driver_step<H: DriverHost>(
    host: &H,
    opts: &InstallOptions,
    cache: &Path,
) -> (String, bool, String) {
    super::driver_flow::ensure_driver(host, opts, cache, &mut |_| {}, &|| false)
}
pub fn scheduled_task_targets_us(install_dir: &Path) -> bool {
    let gui = install_dir.join(GUI_EXE_NAME);
    let needle = crate::install::identity::normalize_for_compare(&gui);
    let out = query_task_xml();
    crate::install::identity::normalize_for_compare(Path::new(&out)).contains(&needle)
        || out.to_ascii_lowercase().contains(&needle)
}

fn query_task_xml() -> String {
    use std::os::windows::process::CommandExt;
    use windows::Win32::System::Threading::CREATE_NO_WINDOW;
    let output = std::process::Command::new("schtasks.exe")
        .args([
            "/Query",
            "/TN",
            crate::platform::TASK_NAME,
            "/FO",
            "LIST",
            "/V",
        ])
        .creation_flags(CREATE_NO_WINDOW.0)
        .output();
    match output {
        Ok(o) => String::from_utf8_lossy(&o.stdout).into_owned(),
        Err(_) => String::new(),
    }
}

pub fn remove_startup_if_ours(install_dir: &Path) -> Result<(), String> {
    if scheduled_task_targets_us(install_dir) {
        crate::platform::remove()?;
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub struct UninstallPlan {
    pub install_dir: PathBuf,
    pub state: InstallState,
}

pub fn plan_uninstall(uninstall_exe: &Path) -> Result<UninstallPlan, String> {
    let dir = uninstall_exe
        .parent()
        .ok_or("无法确定安装目录。")?
        .to_path_buf();
    validate::reject_reparse_in_chain(&dir).map_err(|e| e.message())?;
    validate::reject_reparse_in_chain(&dir.join("install-state.json")).map_err(|e| e.message())?;
    let state = identity::load_state(&dir.join("install-state.json"))?;
    if !identity::state_matches_dir(&state, &dir) {
        return Err("安装身份与卸载器所在目录不一致，已停止。".into());
    }
    Ok(UninstallPlan {
        install_dir: dir,
        state,
    })
}

fn push_reparse(leftover: &mut Vec<String>, p: &Path) {
    leftover.push(format!("重解析点未删除：{}", p.display()));
}

fn remove_file_owned(p: PathBuf, leftover: &mut Vec<String>) {
    if crate::install::validate::is_reparse_point(&p) {
        push_reparse(leftover, &p);
        return;
    }
    if p.exists() && std::fs::remove_file(&p).is_err() {
        leftover.push(p.display().to_string());
    }
}

fn remove_tree_owned(p: PathBuf, leftover: &mut Vec<String>) {
    if !p.exists() {
        return;
    }
    if crate::install::validate::is_reparse_point(&p) {
        push_reparse(leftover, &p);
        return;
    }
    if std::fs::remove_dir_all(&p).is_err() {
        leftover.push(p.display().to_string());
    }
}

/// Revalidate even for plans assembled by a caller, and before the first deletion.
fn preflight_owned(plan: &UninstallPlan) -> Result<(), String> {
    identity::validate_state(&plan.state)?;
    if !identity::state_matches_dir(&plan.state, &plan.install_dir) {
        return Err("安装身份与目录不一致。".into());
    }
    let system = std::env::var_os("SystemRoot")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
    validate::validate_install_dir(&plan.install_dir.to_string_lossy(), &system, None)
        .map_err(|e| e.message())?;
    validate::reject_reparse_in_chain(&plan.install_dir.join("install-state.json"))
        .map_err(|e| e.message())?;
    let current = identity::load_state(&plan.install_dir.join("install-state.json"))?;
    if current.install_id != plan.state.install_id
        || !identity::state_matches_dir(&current, &plan.install_dir)
    {
        return Err("安装身份已经改变，请重新启动安装或卸载。".into());
    }
    for name in [
        GUI_EXE_NAME,
        UNINSTALL_EXE_NAME,
        "install-state.json",
        "runtime",
        "licenses",
        "data",
    ] {
        validate::reject_reparse_tree(&plan.install_dir.join(name))?;
    }
    for entry in std::fs::read_dir(&plan.install_dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if validate::is_install_cache(&name) || validate::is_upgrade_backup(&name) {
            validate::reject_reparse_tree(&entry.path())?;
        }
    }
    Ok(())
}

/// Payload only. Keep the uninstaller and marker until finalization succeeds.
pub fn delete_owned(plan: &UninstallPlan) -> Vec<String> {
    if let Err(e) = preflight_owned(plan) {
        return vec![e];
    }
    let mut leftover = Vec::new();
    let dir = &plan.install_dir;
    remove_file_owned(dir.join(GUI_EXE_NAME), &mut leftover);
    for name in ["runtime", "licenses", "data"] {
        remove_tree_owned(dir.join(name), &mut leftover);
    }
    match std::fs::read_dir(dir) {
        Ok(rd) => {
            for entry in rd {
                match entry {
                    Ok(e) => {
                        let name = e.file_name().to_string_lossy().into_owned();
                        if validate::is_install_cache(&name) {
                            remove_tree_owned(e.path(), &mut leftover);
                        } else if validate::is_upgrade_backup(&name) {
                            remove_file_owned(e.path(), &mut leftover);
                        }
                    }
                    Err(e) => leftover.push(e.to_string()),
                }
            }
        }
        Err(e) => leftover.push(e.to_string()),
    }
    leftover
}

pub fn remove_shortcuts(state: &InstallState) -> Result<(), String> {
    let dir = Path::new(&state.install_dir);
    let gui = dir.join(GUI_EXE_NAME);
    let un = dir.join(UNINSTALL_EXE_NAME);
    for desk in [knownfolder::public_desktop(), knownfolder::user_desktop()]
        .into_iter()
        .flatten()
    {
        shortcuts::remove_if_target(&shortcuts::desktop_link_path(&desk), &gui)?;
    }
    for prog in [knownfolder::common_programs(), knownfolder::user_programs()]
        .into_iter()
        .flatten()
    {
        for (link, target) in [
            (shortcuts::programs_gui_link(&prog), &gui),
            (shortcuts::programs_uninstall_link(&prog), &un),
            (shortcuts::start_menu_gui_link(&prog), &gui),
            (shortcuts::start_menu_uninstall_link(&prog), &un),
        ] {
            shortcuts::remove_if_target(&link, target)?;
        }
        let _ = std::fs::remove_dir(shortcuts::start_menu_dir(&prog));
    }
    shortcuts::notify_shell();
    Ok(())
}

/// Keep recovery bytes until the final (atomic) registration deletion succeeds.
fn finalize_with(
    plan: &UninstallPlan,
    cleanup_entries: impl FnOnce() -> Result<(), String>,
    delete_registration: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    preflight_owned(plan)?;
    let paths = [
        plan.install_dir.join(UNINSTALL_EXE_NAME),
        plan.install_dir.join("install-state.json"),
    ];
    let copies = paths
        .iter()
        .map(std::fs::read)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("无法保留卸载恢复文件：{e}"))?;
    cleanup_entries()?;
    let mut removed = Vec::new();
    let result = (|| {
        for (i, p) in paths.iter().enumerate() {
            std::fs::remove_file(p).map_err(|e| format!("无法删除 {}：{e}", p.display()))?;
            removed.push(i);
        }
        delete_registration()
    })();
    if let Err(mut error) = result {
        for i in removed {
            if let Err(e) = std::fs::write(&paths[i], &copies[i]) {
                error.push_str(&format!("；恢复 {} 失败：{e}", paths[i].display()));
            }
        }
        return Err(error);
    }
    let _ = remove_empty_install_dir(&plan.install_dir);
    Ok(())
}

pub fn finish_uninstall(plan: &UninstallPlan, leftover: &[String]) -> Result<(), String> {
    if !leftover.is_empty() {
        return Err("文件清理未完成，已保留卸载入口，请关闭占用后重试。".into());
    }
    finalize_with(
        plan,
        || {
            remove_shortcuts(&plan.state)?;
            remove_startup_if_ours(&plan.install_dir)?;
            Ok(())
        },
        || registry::delete_uninstall_key_if_ours(&plan.install_dir),
    )
}

pub fn run_uninstall(plan: &UninstallPlan) -> Vec<String> {
    run_uninstall_with(
        plan,
        || super::process::stop_owned(&plan.install_dir, std::time::Duration::from_secs(10)),
        || finish_uninstall(plan, &[]),
    )
}
fn run_uninstall_with(
    plan: &UninstallPlan,
    stop: impl FnOnce() -> Result<(), String>,
    finalize: impl FnOnce() -> Result<(), String>,
) -> Vec<String> {
    if let Err(e) = preflight_owned(plan) {
        return vec![e];
    }
    if let Err(e) = stop() {
        return vec![e];
    }
    let mut leftover = delete_owned(plan);
    if leftover.is_empty() {
        if let Err(e) = finalize() {
            leftover.push(e);
        }
    }
    leftover
}

fn cwd_is_inside(dir: &Path) -> bool {
    let Ok(cwd) = std::env::current_dir() else {
        return false;
    };
    validate::is_under(&cwd, dir)
}

/// 进程工作目录若还在安装路径内，Windows 删不掉该空目录。
pub fn leave_install_dir(dir: &Path) {
    if !cwd_is_inside(dir) {
        return;
    }
    let fallback = std::env::temp_dir();
    if std::env::set_current_dir(&fallback).is_err() || cwd_is_inside(dir) {
        let _ = std::env::set_current_dir("\\");
    }
}

pub fn remove_empty_install_dir(dir: &Path) -> bool {
    if !dir.exists() {
        return true;
    }
    leave_install_dir(dir);
    if std::fs::remove_dir(dir).is_ok() || !dir.exists() {
        return true;
    }
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let name = e.file_name();
            let name = name.to_string_lossy();
            if name.eq_ignore_ascii_case("desktop.ini") || name.eq_ignore_ascii_case("thumbs.db") {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
    leave_install_dir(dir);
    std::fs::remove_dir(dir).is_ok() || !dir.exists()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::install::origin::Origin;

    fn tmp(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "drcom-flow-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn payload() -> Payload {
        Payload {
            gui: b"gui-bytes".to_vec(),
            uninstall: b"uninst-bytes".to_vec(),
            notice: b"notice".to_vec(),
            gpl: b"gpl".to_vec(),
            lucide: b"lucide".to_vec(),
        }
    }

    fn origin(sid: &str) -> Origin {
        Origin {
            sid: sid.into(),
            nonce: "0123456789abcdef0123456789abcdef".into(),
            parent_pid: 1,
            user_name: "t".into(),
        }
    }

    fn fixture(tag: &str) -> UninstallPlan {
        let dest = tmp(tag);
        let opts = InstallOptions {
            install_dir: dest.clone(),
            ..Default::default()
        };
        let state = install_files(&opts, &origin("S-1-5-21-1-2-3-1001"), &payload(), true).unwrap();
        UninstallPlan {
            install_dir: dest,
            state,
        }
    }
    #[test]
    fn unmarked_data_and_invalid_marker_never_deleted() {
        let dest = tmp("foreign-data");
        std::fs::create_dir(dest.join("data")).unwrap();
        std::fs::write(dest.join("data/document.txt"), b"personal").unwrap();
        for extra in [None, Some("notes.txt"), Some("install-state.json")] {
            if let Some(name) = extra {
                std::fs::write(dest.join(name), b"invalid").unwrap();
            }
            assert!(prepare_install_dir(&dest).is_err());
            assert_eq!(
                std::fs::read(dest.join("data/document.txt")).unwrap(),
                b"personal"
            );
        }
        std::fs::remove_dir_all(dest).unwrap();
    }
    #[test]
    fn crafted_lists_cannot_escape_and_fail_before_deletion() {
        let mut plan = fixture("traversal");
        let outside = tmp("outside");
        let file = outside.join("keep.txt");
        std::fs::write(&file, b"keep").unwrap();
        for bad in [
            "../outside.txt".to_string(),
            file.to_string_lossy().into_owned(),
            "data/../../outside".into(),
            "data:stream".into(),
            "data\\..\\outside".into(),
            "data.".into(),
        ] {
            for directory in [false, true] {
                let mut state = plan.state.clone();
                if directory {
                    state.owned_dirs.push(bad.clone());
                } else {
                    state.owned_files.push(bad.clone());
                }
                identity::save_state(&plan.install_dir.join("install-state.json"), &state).unwrap();
                assert!(plan_uninstall(&plan.install_dir.join(UNINSTALL_EXE_NAME)).is_err());
                let original = std::mem::replace(&mut plan.state, state);
                assert!(!delete_owned(&plan).is_empty());
                plan.state = original;
                assert!(plan.install_dir.join(GUI_EXE_NAME).exists());
                assert_eq!(std::fs::read(&file).unwrap(), b"keep");
            }
        }
        std::fs::remove_dir_all(plan.install_dir).unwrap();
        std::fs::remove_dir_all(outside).unwrap();
    }
    #[test]
    fn locked_payload_and_stop_failure_keep_retry_identity() {
        use std::os::windows::fs::OpenOptionsExt;
        let plan = fixture("locked");
        assert!(!run_uninstall_with(
            &plan,
            || Err("stop failed".into()),
            || panic!("must not finalize")
        )
        .is_empty());
        assert!(plan.install_dir.join(GUI_EXE_NAME).exists());
        let locked = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(plan.install_dir.join(GUI_EXE_NAME))
            .unwrap();
        assert!(!run_uninstall_with(&plan, || Ok(()), || panic!("must not finalize")).is_empty());
        assert!(plan_uninstall(&plan.install_dir.join(UNINSTALL_EXE_NAME)).is_ok());
        assert!(plan.install_dir.join(UNINSTALL_EXE_NAME).is_file());
        drop(locked);
        assert!(delete_owned(&plan).is_empty());
        finalize_with(&plan, || Ok(()), || Ok(())).unwrap();
        assert!(!plan.install_dir.exists());
    }
    #[test]
    fn finalization_errors_preserve_recovery_and_registration_order() {
        use std::os::windows::fs::OpenOptionsExt;
        let plan = fixture("finalize");
        assert!(delete_owned(&plan).is_empty());
        assert!(finalize_with(
            &plan,
            || Err("entry failure".into()),
            || panic!("registration must remain")
        )
        .is_err());
        assert!(plan_uninstall(&plan.install_dir.join(UNINSTALL_EXE_NAME)).is_ok());
        let locked = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(plan.install_dir.join("install-state.json"))
            .unwrap();
        assert!(finalize_with(&plan, || Ok(()), || panic!("registration must remain")).is_err());
        assert!(plan.install_dir.join(UNINSTALL_EXE_NAME).is_file());
        drop(locked);
        assert!(finalize_with(&plan, || Ok(()), || Err("registry failure".into())).is_err());
        assert!(plan_uninstall(&plan.install_dir.join(UNINSTALL_EXE_NAME)).is_ok());
        assert_eq!(
            std::fs::read(plan.install_dir.join(UNINSTALL_EXE_NAME)).unwrap(),
            payload().uninstall
        );
        finalize_with(&plan, || Ok(()), || Ok(())).unwrap();
        assert!(!plan.install_dir.exists());
    }
    #[test]
    fn nested_junction_rejected_before_any_payload_deletion() {
        use std::os::windows::process::CommandExt;
        let plan = fixture("junction");
        let outside = tmp("junction-target");
        std::fs::write(outside.join("personal.txt"), b"keep").unwrap();
        let link = plan.install_dir.join("data").join("junction");
        let output = std::process::Command::new("cmd.exe")
            .args(["/d", "/c", "mklink", "/J"])
            .arg(&link)
            .arg(&outside)
            .creation_flags(0x08000000)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(!delete_owned(&plan).is_empty());
        assert!(plan.install_dir.join(GUI_EXE_NAME).is_file());
        assert!(outside.join("personal.txt").is_file());
        assert!(prepare_install_dir(&plan.install_dir).is_err());
        std::fs::remove_dir(&link).unwrap();
        std::fs::remove_dir_all(plan.install_dir).unwrap();
        std::fs::remove_dir_all(outside).unwrap();
    }

    #[test]
    fn install_then_uninstall_keeps_strangers() {
        let dest = tmp("iu");
        let mut opts = InstallOptions::default();
        opts.install_dir = dest.clone();
        let state = install_files(&opts, &origin("S-1-5-21-1-2-3-1001"), &payload(), true).unwrap();
        assert!(dest.join(GUI_EXE_NAME).is_file());
        assert!(dest
            .join("runtime")
            .join(CORE_SHA256)
            .join("drcom4scut.exe")
            .is_file());
        std::fs::write(dest.join("user-notes.txt"), b"keep me").unwrap();
        let plan = UninstallPlan {
            install_dir: dest.clone(),
            state,
        };
        let leftover = delete_owned(&plan);
        assert!(dest.join("user-notes.txt").is_file(), "陌生文件必须保留");
        assert!(!dest.join(GUI_EXE_NAME).exists());
        assert!(!dest.join("runtime").exists(), "runtime 树必须整棵删除");
        assert!(!dest.join("licenses").exists());
        assert!(!dest.join("data").exists());
        assert!(leftover.is_empty() || leftover.iter().all(|s| !s.contains("user-notes")));
        let _ = std::fs::remove_dir_all(&dest);
    }

    #[test]
    fn uninstall_removes_upgrade_backups_cache_and_nested_trees() {
        let dest = tmp("junk");
        let mut opts = InstallOptions::default();
        opts.install_dir = dest.clone();
        let o = origin("S-1-5-21-1-2-3-1001");
        let state = install_files(&opts, &o, &payload(), true).unwrap();
        std::fs::write(dest.join("drcom4scutGUI.bak-upgrade"), b"old-gui").unwrap();
        std::fs::write(dest.join("uninstall.bak-upgrade"), b"old-un").unwrap();
        let hash_dir = dest.join("runtime").join(CORE_SHA256);
        std::fs::write(hash_dir.join("drcom4scut.bak-upgrade"), b"old-core").unwrap();
        let cache = dest.join(".npcap-setup-deadbeef");
        std::fs::create_dir_all(&cache).unwrap();
        std::fs::write(cache.join("npcap-1.88.exe"), b"npcap").unwrap();
        std::fs::create_dir_all(dest.join(".install-staging")).unwrap();
        let logs = dest.join("data").join("users").join(&o.sid).join("logs");
        std::fs::create_dir_all(&logs).unwrap();
        std::fs::write(logs.join("latest.log"), b"log").unwrap();
        std::fs::write(dest.join("keep-me.txt"), b"stranger").unwrap();
        std::fs::write(dest.join("personal.bak-upgrade"), b"stranger").unwrap();
        let leftover = delete_owned(&UninstallPlan {
            install_dir: dest.clone(),
            state,
        });
        assert!(leftover.is_empty(), "{leftover:?}");
        assert!(!dest.join("runtime").exists());
        assert!(!dest.join("licenses").exists());
        assert!(!dest.join("data").exists());
        assert!(!dest.join("drcom4scutGUI.bak-upgrade").exists());
        assert!(!dest.join("uninstall.bak-upgrade").exists());
        assert!(!dest.join(".npcap-setup-deadbeef").exists());
        assert!(!dest.join(".install-staging").exists());
        assert!(dest.join("keep-me.txt").is_file());
        assert!(dest.join("personal.bak-upgrade").is_file());
        let _ = std::fs::remove_dir_all(&dest);
    }

    #[test]
    fn prepare_install_dir_preserves_unmarked_contents() {
        let dest = tmp("prep");
        std::fs::create_dir_all(dest.join("runtime").join("abc")).unwrap();
        std::fs::write(
            dest.join("runtime").join("abc").join("drcom4scut.exe"),
            b"core",
        )
        .unwrap();
        std::fs::write(dest.join("uninstall.exe"), b"un").unwrap();
        std::fs::write(dest.join("drcom4scutGUI.bak-upgrade"), b"old").unwrap();
        std::fs::create_dir_all(dest.join(".npcap-setup-x")).unwrap();
        std::fs::write(dest.join("notes.txt"), b"keep").unwrap();
        assert!(!validate::looks_like_our_residue(&dest));
        let err = prepare_install_dir(&dest).unwrap_err();
        assert!(err.contains("其他应用文件"), "{err}");
        assert!(dest.join("runtime").exists());
        assert!(dest.join("uninstall.exe").exists());
        assert!(dest.join("drcom4scutGUI.bak-upgrade").exists());
        assert!(dest.join(".npcap-setup-x").exists());
        assert_eq!(std::fs::read(dest.join("notes.txt")).unwrap(), b"keep");
        std::fs::remove_file(dest.join("notes.txt")).unwrap();
        assert!(prepare_install_dir(&dest).is_err());
        let _ = std::fs::remove_dir_all(&dest);
    }

    #[test]
    fn empty_install_dir_is_removed_even_if_cwd_is_inside() {
        let dest = tmp("cwdun");
        std::fs::write(dest.join("desktop.ini"), b"[.ShellClassInfo]").unwrap();
        let prev = std::env::current_dir().unwrap();
        struct Restore(PathBuf);
        impl Drop for Restore {
            fn drop(&mut self) {
                let _ = std::env::set_current_dir(&self.0);
            }
        }
        let _restore = Restore(prev);
        std::env::set_current_dir(&dest).unwrap();
        assert!(remove_empty_install_dir(&dest));
        assert!(!dest.exists());
    }

    #[test]
    fn upgrade_keeps_user_data() {
        let dest = tmp("up");
        let mut opts = InstallOptions::default();
        opts.install_dir = dest.clone();
        let o = origin("S-1-5-21-9");
        install_files(&opts, &o, &payload(), true).unwrap();
        let data = dest
            .join("data")
            .join("users")
            .join(&o.sid)
            .join("settings.v1.dat");
        std::fs::create_dir_all(data.parent().unwrap()).unwrap();
        std::fs::write(&data, b"user-secret").unwrap();
        let mut p2 = payload();
        p2.gui = b"gui-v2".to_vec();
        install_files(&opts, &o, &p2, true).unwrap();
        assert_eq!(std::fs::read(&data).unwrap(), b"user-secret");
        assert_eq!(std::fs::read(dest.join(GUI_EXE_NAME)).unwrap(), b"gui-v2");
        let _ = std::fs::remove_dir_all(&dest);
    }

    #[test]
    fn plan_uninstall_rejects_mismatched_state() {
        let dest = tmp("badplan");
        std::fs::write(
            dest.join("install-state.json"),
            br#"{"productId":"drcom4scutGUI","stateVersion":1,"installId":"x","version":"3.4.0","installDir":"C:\\Other","originSid":"S-1-5-18","coreSha256":"aa"}"#,
        )
        .unwrap();
        let exe = dest.join("uninstall.exe");
        std::fs::write(&exe, b"x").unwrap();
        assert!(plan_uninstall(&exe).is_err());
        let _ = std::fs::remove_dir_all(&dest);
    }

    #[test]
    fn driver_skip_when_available() {
        use crate::install::driver::{DllProbe, DriverHost};
        use crate::install::pe::IMAGE_FILE_MACHINE_AMD64;
        struct H;
        impl DriverHost for H {
            fn system_root(&self) -> PathBuf {
                PathBuf::from(r"C:\Windows")
            }
            fn registry_present(&self) -> bool {
                true
            }
            fn probe_dll(&self, path: &Path) -> DllProbe {
                if path.ends_with("Packet.dll") {
                    DllProbe {
                        path: path.to_path_buf(),
                        exists: true,
                        is_file: true,
                        under_system32: true,
                        machine: Some(IMAGE_FILE_MACHINE_AMD64),
                        loaded: true,
                        has_packet_exports: true,
                    }
                } else {
                    DllProbe {
                        path: path.to_path_buf(),
                        exists: false,
                        is_file: false,
                        under_system32: false,
                        machine: None,
                        loaded: false,
                        has_packet_exports: false,
                    }
                }
            }
            fn download(&self, _: &str, _: &Path) -> Result<(), String> {
                panic!("不应下载");
            }
            fn authenticode_publisher(&self, _: &Path) -> Result<String, String> {
                panic!("不应验签");
            }
            fn run_installer(&self, _: &Path, _: &[String]) -> Result<i32, String> {
                panic!("不应安装驱动");
            }
        }
        let opts = InstallOptions::default();
        let (k, reboot, _) = run_driver_step(&H, &opts, Path::new("."));
        assert_eq!(k, "available");
        assert!(!reboot);
    }
}
