//! Headless installer. Never opens a window or elevates interactively.
use std::path::{Path, PathBuf};

use super::{driver, flow, identity, knownfolder, maintenance, origin, registry, sid};
use flow::{InstallOptions, InstallResult};

pub const INVALID_ARGUMENTS: i32 = 2;
pub const ELEVATION_REQUIRED: i32 = 740;
pub const INSTALL_FAILED: i32 = 1603;
pub const INSTALL_BUSY: i32 = 1618;

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Options {
    pub install_dir: Option<PathBuf>,
    pub launch: bool,
    pub json: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Request {
    Gui,
    Help,
    Version,
    Install(Options),
}

pub fn parse(args: &[String]) -> Result<Request, String> {
    if args.is_empty() {
        return Ok(Request::Gui);
    }
    if args == ["--help"] || args == ["-h"] {
        return Ok(Request::Help);
    }
    if args == ["--version"] {
        return Ok(Request::Version);
    }
    let mut options = Options::default();
    let mut install = false;
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--install" | "--silent" => install = true,
            "--json" => options.json = true,
            "--launch" => options.launch = true,
            "--install-dir" => {
                set_directory(&mut options, args.next().ok_or("--install-dir 缺少目录。")?)?;
            }
            value if value.starts_with("--install-dir=") => {
                set_directory(&mut options, &value[14..])?;
            }
            other => return Err(format!("未知参数：{other}。使用 --help 查看用法。")),
        }
    }
    if !install {
        return Err("终端安装需要 --install 或 --silent。".into());
    }
    Ok(Request::Install(options))
}

fn set_directory(options: &mut Options, value: &str) -> Result<(), String> {
    if options.install_dir.is_some() || value.trim().is_empty() || value.starts_with("--") {
        return Err("--install-dir 必须且只能指定一个非空目录。".into());
    }
    options.install_dir = Some(PathBuf::from(value));
    Ok(())
}

fn select_directory(
    requested: Option<&Path>,
    registered: Option<&Path>,
    default: PathBuf,
) -> PathBuf {
    requested
        .or(registered)
        .map(Path::to_path_buf)
        .unwrap_or(default)
}

fn require_driver(status: driver::DriverStatus) -> Result<(), String> {
    if status == driver::DriverStatus::Available {
        Ok(())
    } else {
        Err("终端安装需要已安装兼容的 x64 Npcap / WinPcap。请先通过图形安装器安装驱动，再运行终端安装；现有客户端未被停止或修改。".into())
    }
}

#[derive(Debug)]
struct Failure {
    code: i32,
    message: String,
}

fn failure(message: impl Into<String>) -> Failure {
    Failure {
        code: INSTALL_FAILED,
        message: message.into(),
    }
}

fn install(options: &Options) -> Result<InstallResult, Failure> {
    if !sid::is_elevated() {
        return Err(Failure {
            code: ELEVATION_REQUIRED,
            message: "请在管理员终端中运行 --install。终端模式不会弹出 UAC 或安装窗口。".into(),
        });
    }
    let _guard = maintenance::acquire().map_err(|message| Failure {
        code: INSTALL_BUSY,
        message,
    })?;
    let registered = registry::registered_install_dir().map_err(failure)?;
    let default = knownfolder::program_files()
        .map_err(failure)?
        .join("drcom4scutGUI");
    let mut opts = InstallOptions {
        install_dir: select_directory(
            options.install_dir.as_deref(),
            registered.as_deref(),
            default,
        ),
        launch_after: options.launch,
        ..Default::default()
    };
    registry::enforce_single_install(&opts.install_dir, registered.as_deref()).map_err(failure)?;
    let system = std::env::var_os("SystemRoot")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
    opts.install_dir = flow::validate_options(
        &opts,
        &system,
        knownfolder::program_files_x86().ok().as_deref(),
    )
    .map_err(|e| failure(e.message()))?;
    if opts.install_dir.join("install-state.json").exists() {
        let state =
            identity::load_state(&opts.install_dir.join("install-state.json")).map_err(failure)?;
        opts.desktop_shortcut = state.desktop_shortcut;
        opts.start_menu_shortcut = state.start_menu_shortcut;
    }
    require_driver(driver::detect_with(&driver::RealDriverHost)).map_err(failure)?;
    let payload = flow::default_payload().map_err(failure)?;
    let source = origin::capture_current_origin(&origin::new_nonce()).map_err(failure)?;
    flow::install_package(&opts, &source, &payload, &mut |_, _| {}, &|| false).map_err(failure)?;
    if options.launch {
        std::process::Command::new(opts.install_dir.join(super::GUI_EXE_NAME))
            .arg(crate::platform::AUTOSTART_ARG)
            .current_dir(&opts.install_dir)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| failure(format!("客户端已安装，但后台启动失败：{e}")))?;
    }
    Ok(InstallResult {
        ok: true,
        install_dir: opts.install_dir.to_string_lossy().into_owned(),
        version: super::APP_VERSION.into(),
        driver: "available".into(),
        need_reboot: false,
        message: "安装完成。".into(),
        launch: options.launch,
    })
}

pub fn help() -> String {
    format!(
        "drcom4scut Setup {}\n\n\
Usage (administrator terminal):\n  drcom4scut-Setup.exe --install [options]\n\n\
  --install, --silent         Install/upgrade without installer windows\n\
  --install-dir PATH         Target directory (defaults to existing installation)\n\
  --launch                  Start the installed client in the background\n\
  --json                    Print one JSON result, including exitCode\n\
  --help, -h                Show this help\n\
  --version                 Show installer version\n\n\
Requires an existing compatible x64 Npcap/WinPcap driver.\n\
Exit codes: 0 success, 2 invalid arguments, 740 administrator required,\n\
1603 installation/prerequisite failure, 1618 another install/uninstall is active.\n\
PowerShell: use Start-Process -PassThru, then WaitForExit() and ExitCode.\n",
        super::APP_VERSION
    )
}

fn attach_console() {
    use windows::Win32::System::Console::{
        AttachConsole, GetStdHandle, ATTACH_PARENT_PROCESS, STD_OUTPUT_HANDLE,
    };
    unsafe {
        if GetStdHandle(STD_OUTPUT_HANDLE).map_or(true, |handle| handle.is_invalid()) {
            let _ = AttachConsole(ATTACH_PARENT_PROCESS);
        }
    }
}

fn emit(result: InstallResult, code: i32, json: bool) -> i32 {
    if json {
        let mut value = serde_json::to_value(&result).expect("InstallResult serialization");
        value["exitCode"] = code.into();
        println!("{value}");
    } else if result.ok {
        println!("{}\n{}", result.message, result.install_dir);
    } else {
        eprintln!("{} (exit code {code})", result.message);
    }
    code
}

pub fn run_args(args: &[String]) -> Option<i32> {
    let request = parse(args);
    if request == Ok(Request::Gui) {
        return None;
    }
    attach_console();
    let (outcome, json) = match request {
        Ok(Request::Help) => {
            print!("{}", help());
            return Some(0);
        }
        Ok(Request::Version) => {
            println!("{}", super::APP_VERSION);
            return Some(0);
        }
        Ok(Request::Install(options)) => (install(&options), options.json),
        Err(message) => (
            Err(Failure {
                code: INVALID_ARGUMENTS,
                message,
            }),
            args.iter().any(|a| a == "--json"),
        ),
        Ok(Request::Gui) => unreachable!(),
    };
    Some(match outcome {
        Ok(result) => emit(result, 0, json),
        Err(error) => emit(
            InstallResult {
                ok: false,
                install_dir: String::new(),
                version: super::APP_VERSION.into(),
                driver: String::new(),
                need_reboot: false,
                message: error.message,
                launch: false,
            },
            error.code,
            json,
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|v| v.to_string()).collect()
    }

    #[test]
    fn no_arguments_preserve_gui_and_information_commands_do_not_install() {
        assert_eq!(parse(&[]), Ok(Request::Gui));
        assert_eq!(parse(&args(&["--help"])), Ok(Request::Help));
        assert_eq!(parse(&args(&["--version"])), Ok(Request::Version));
    }

    #[test]
    fn headless_defaults_do_not_launch_and_silent_alias_works() {
        assert_eq!(
            parse(&args(&["--install"])),
            Ok(Request::Install(Options::default()))
        );
        assert_eq!(
            parse(&args(&["--silent", "--json"])),
            Ok(Request::Install(Options {
                json: true,
                ..Default::default()
            }))
        );
    }

    #[test]
    fn paths_with_spaces_and_unicode_are_preserved_in_both_forms() {
        let path = r"D:\校园网 Apps\Client";
        for values in [
            args(&["--install", "--install-dir", path]),
            args(&["--install", &format!("--install-dir={path}")]),
        ] {
            let Ok(Request::Install(options)) = parse(&values) else {
                panic!("valid install rejected")
            };
            assert_eq!(options.install_dir, Some(PathBuf::from(path)));
        }
    }

    #[test]
    fn invalid_arguments_never_fall_back_to_the_gui() {
        for values in [
            vec!["--unknown"],
            vec!["--json"],
            vec!["--install", "--install-dir"],
            vec!["--install", "--install-dir="],
            vec!["--install", "--install-dir", "--json"],
            vec!["--install", "--worker"],
            vec!["--install", "--install-dir=A", "--install-dir=B"],
        ] {
            assert!(parse(&args(&values)).is_err(), "{values:?}");
        }
    }

    #[test]
    fn background_launch_is_opt_in() {
        let Ok(Request::Install(options)) = parse(&args(&["--install", "--launch"])) else {
            panic!("valid switches rejected")
        };
        assert!(options.launch);
    }

    #[test]
    fn upgrades_use_the_registered_directory_unless_an_explicit_path_is_given() {
        let old = Path::new(r"D:\Campus Client");
        let default = PathBuf::from(r"C:\Program Files\drcom4scutGUI");
        assert_eq!(select_directory(None, Some(old), default.clone()), old);
        assert_eq!(select_directory(None, None, default.clone()), default);
        let explicit = Path::new(r"E:\Client");
        assert_eq!(
            select_directory(Some(explicit), Some(old), default),
            explicit
        );
    }

    #[test]
    fn missing_and_wrong_arch_drivers_fail_before_the_install_transaction() {
        assert!(require_driver(driver::DriverStatus::Available).is_ok());
        for status in [
            driver::DriverStatus::Missing,
            driver::DriverStatus::FoundWrongArch,
        ] {
            assert!(require_driver(status)
                .unwrap_err()
                .contains("现有客户端未被停止或修改"));
        }
    }
}
