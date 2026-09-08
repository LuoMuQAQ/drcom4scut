#![windows_subsystem = "windows"]
//! Native installer: responsive unelevated UI, elevated file/driver worker.
use drcom4scut_gui::install::{
    acl, driver, driver_flow, flow, identity, knownfolder, origin, process, sid,
};
use flow::{InstallOptions, InstallResult};
use std::path::{Path, PathBuf};
use std::time::Duration;
use windows::core::{w, PCWSTR};

#[path = "../install/setup_window.rs"]
mod setup_window;

fn relaunch_elevated(args: &[String]) -> Result<(), String> {
    use windows::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
    use windows::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject};
    use windows::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let executable = drcom4scut_gui::ui::winutil::wide(&exe.to_string_lossy());
    let arguments = args
        .iter()
        .map(|a| quote_arg(a))
        .collect::<Vec<_>>()
        .join(" ");
    let parameters = drcom4scut_gui::ui::winutil::wide(&arguments);
    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS,
        lpVerb: w!("runas"),
        lpFile: PCWSTR(executable.as_ptr()),
        lpParameters: PCWSTR(parameters.as_ptr()),
        nShow: 1,
        ..Default::default()
    };
    unsafe {
        ShellExecuteExW(&mut info).map_err(|e| {
            if e.code().0 as u32 == 0x800704c7 {
                "已取消管理员授权，尚未开始安装。".into()
            } else {
                format!("无法启动安装：{e}")
            }
        })?;
        if info.hProcess.is_invalid() {
            return Err("安装进程没有返回句柄。".into());
        }
        let wait = WaitForSingleObject(info.hProcess, u32::MAX);
        let mut code = 0;
        let result = GetExitCodeProcess(info.hProcess, &mut code);
        let _ = CloseHandle(info.hProcess);
        if wait != WAIT_OBJECT_0 || result.is_err() {
            return Err("无法读取安装进程结果。".into());
        }
        if code != 0 {
            return Err(format!("安装进程未正常完成（{code}）。"));
        }
    }
    Ok(())
}

fn quote_arg(arg: &str) -> String {
    let mut out = String::from("\"");
    let mut slashes = 0;
    for c in arg.chars() {
        if c == '\\' {
            slashes += 1;
            continue;
        }
        out.extend(std::iter::repeat_n(
            '\\',
            if c == '"' { slashes * 2 + 1 } else { slashes },
        ));
        out.push(c);
        slashes = 0;
    }
    out.extend(std::iter::repeat_n('\\', slashes * 2));
    out.push('"');
    out
}

fn write_options(dir: &Path, opts: &InstallOptions, driver_only: bool) -> Result<(), String> {
    let value = serde_json::json!({ "installDir": opts.install_dir.to_string_lossy(),
        "desktopShortcut": opts.desktop_shortcut, "startMenuShortcut": opts.start_menu_shortcut,
        "launchAfter": opts.launch_after, "driverOnly": driver_only });
    std::fs::write(dir.join("options.json"), value.to_string()).map_err(|e| e.to_string())
}

fn read_options(dir: &Path) -> Result<(InstallOptions, bool), String> {
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Options {
        install_dir: PathBuf,
        desktop_shortcut: bool,
        start_menu_shortcut: bool,
        launch_after: bool,
        #[serde(default)]
        driver_only: bool,
    }
    let raw = std::fs::read(dir.join("options.json")).map_err(|e| e.to_string())?;
    let o: Options = serde_json::from_slice(&raw).map_err(|e| e.to_string())?;
    let mut options = InstallOptions {
        install_dir: o.install_dir,
        desktop_shortcut: o.desktop_shortcut,
        start_menu_shortcut: o.start_menu_shortcut,
        launch_after: o.launch_after,
        ..Default::default()
    };
    let system = std::env::var_os("SystemRoot")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
    options.install_dir = flow::validate_options(
        &options,
        &system,
        knownfolder::program_files_x86().ok().as_deref(),
    )
    .map_err(|e| e.message())?;
    Ok((options, o.driver_only))
}

fn progress(dir: &Path, percent: u32, message: &str) {
    origin::write_status(
        dir,
        &serde_json::json!({"percent":percent,"message":message}).to_string(),
    );
}

fn worker_main(dir: PathBuf, nonce: String) -> i32 {
    if !sid::is_elevated() {
        return 1;
    }
    // Never write a result to a directory that failed validation.
    let source = match origin::verify_origin(&dir, &nonce) {
        Ok(o) => o,
        Err(_) => return 1,
    };
    let action = || -> Result<InstallResult, String> {
        let _guard = drcom4scut_gui::install::maintenance::acquire()?;
        let (opts, driver_only) = read_options(&dir)?;
        let registered = drcom4scut_gui::install::registry::registered_install_dir()?;
        drcom4scut_gui::install::registry::enforce_single_install(
            &opts.install_dir,
            registered.as_deref(),
        )?;
        if origin::cancel_requested(&dir) {
            return Err("安装已取消。".into());
        }
        if driver_only {
            let state = identity::load_state(&opts.install_dir.join("install-state.json"))?;
            if !identity::state_matches_dir(&state, &opts.install_dir) {
                return Err("安装位置已改变，请重新运行安装器。".into());
            }
        } else {
            progress(&dir, 8, "正在准备安装文件…");
            let payload = flow::default_payload()?;
            process::stop_owned(&opts.install_dir, Duration::from_secs(8))?;
            if origin::cancel_requested(&dir) {
                return Err("安装已取消。".into());
            }
            progress(&dir, 16, "正在清理旧安装残留…");
            flow::prepare_install_dir(&opts.install_dir)?;
            progress(&dir, 25, "正在安装校园网客户端…");
            flow::install_files(&opts, &source, &payload, false)?;
            progress(&dir, 58, "正在创建快捷方式和卸载入口…");
            flow::create_shortcuts(&opts)?;
            flow::register_uninstall(&opts, &payload)?;
        }
        // Executed as administrator: cache must not be replaceable by normal users.
        let cache = opts.install_dir.join(format!(".npcap-setup-{nonce}"));
        acl::create_dir_with_sddl(&cache, &acl::data_container_sddl())?;
        let (driver, need_reboot, message) = driver_flow::ensure_driver(
            &driver::RealDriverHost,
            &opts,
            &cache,
            &mut |p| progress(&dir, p.percent, &p.message),
            &|| origin::cancel_requested(&dir),
        );
        let _ = std::fs::remove_dir_all(&cache);
        let launch = opts.launch_after
            && driver_flow::ready(&driver)
            && !need_reboot
            && !origin::cancel_requested(&dir);
        Ok(InstallResult {
            ok: true,
            install_dir: opts.install_dir.to_string_lossy().into_owned(),
            version: env!("CARGO_PKG_VERSION").into(),
            driver,
            need_reboot,
            message,
            launch,
        })
    };
    let result = action().unwrap_or_else(|message| InstallResult {
        ok: false,
        install_dir: String::new(),
        version: env!("CARGO_PKG_VERSION").into(),
        driver: String::new(),
        need_reboot: false,
        message,
        launch: false,
    });
    progress(
        &dir,
        100,
        if result.ok {
            "安装步骤已完成"
        } else {
            "安装未完成"
        },
    );
    origin::write_result(&dir, &serde_json::to_string(&result).unwrap());
    0
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--worker") {
        let dir = args
            .iter()
            .find_map(|a| a.strip_prefix("--origin="))
            .map(PathBuf::from);
        let nonce = args
            .iter()
            .find_map(|a| a.strip_prefix("--nonce="))
            .map(str::to_owned);
        if let (Some(dir), Some(nonce)) = (dir, nonce) {
            std::process::exit(worker_main(dir, nonce));
        }
        return;
    }
    setup_window::run();
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn worker_arguments_quote_spaces_and_trailing_slashes() {
        assert_eq!(
            quote_arg(r"--origin=C:\Users\Test User\Temp\setup"),
            r#""--origin=C:\Users\Test User\Temp\setup""#
        );
        assert_eq!(quote_arg("a\"b"), "\"a\\\"b\"");
        assert_eq!(quote_arg("C:\\folder\\"), "\"C:\\folder\\\\\"");
    }
}
