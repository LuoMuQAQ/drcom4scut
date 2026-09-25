//! 基于 HeroUI v3 + WebView2 的现代化安装向导宿主窗口。
//! 严格复用现有提权 worker、origin 交接、安全 ACL 与驱动安装逻辑。

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::install::driver::{self, DriverStatus};
use crate::install::flow::{self, InstallOptions, InstallResult};
use crate::install::knownfolder;
use crate::install::origin;
use crate::install::ui;
use crate::install::webview_bootstrapper;
use crate::ui::protocol::{self, FrontendCommand, HostEvent, InitSnapshotPayload, SetupSnapshot, ThemeConfig};
use crate::ui::webview::WebView;
use crate::ui::winutil::wide;

const HTML_CONTENT: &str = include_str!("../../frontend/dist/index.html");

struct SetupState {
    options: InstallOptions,
    driver_status: String,
    driver_detail: String,
    theme_config: ThemeConfig,
    origin_dir: Option<PathBuf>,
    is_installing: bool,
}

pub fn run_webview_setup() {
    unsafe {
        // 1. 确保 WebView2 运行时可用（若缺失则在用户同意后在线补装）
        if let Err(e) = webview_bootstrapper::ensure_runtime_or_install(HWND::default()) {
            let msg = wide(&e);
            let _ = MessageBoxW(
                None,
                PCWSTR(msg.as_ptr()),
                w!("校园网客户端安装"),
                MB_OK | MB_ICONWARNING,
            );
            return;
        }

        // 2. 准备默认安装参数
        let default_dir = knownfolder::program_files()
            .unwrap_or_else(|_| PathBuf::from(r"C:\Program Files"))
            .join("drcom4scut");

        let options = InstallOptions {
            install_dir: default_dir,
            desktop_shortcut: true,
            start_menu_shortcut: true,
            launch_after: true,
            ..Default::default()
        };

        // 检测驱动状态
        let drv_status = driver::detect_with(&driver::RealDriverHost);
        let (driver_status, driver_detail) = match drv_status {
            DriverStatus::Available => ("已就绪".into(), "Npcap / WinPcap 驱动正常".into()),
            DriverStatus::Missing => ("需要安装".into(), "将在安装过程中自动配置 Npcap".into()),
            DriverStatus::FoundWrongArch => ("需要更新".into(), "缺少 x64 驱动，将自动安装适配版本".into()),
        };

        let state = Arc::new(Mutex::new(SetupState {
            options,
            driver_status,
            driver_detail,
            theme_config: ThemeConfig::load_for_current_user(),
            origin_dir: None,
            is_installing: false,
        }));

        // 3. 注册并创建宿主窗口 (560x608)
        let hinstance = windows::Win32::System::LibraryLoader::GetModuleHandleW(None).unwrap();
        let class_name = w!("DrcomSetupWebViewWnd");

        let wc = WNDCLASSW {
            lpfnWndProc: Some(setup_wndproc),
            hInstance: hinstance.into(),
            lpszClassName: class_name,
            style: CS_HREDRAW | CS_VREDRAW,
            ..Default::default()
        };
        let _ = RegisterClassW(&wc);

        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            class_name,
            w!("校园网客户端安装"),
            WS_POPUP | WS_SYSMENU | WS_CLIPCHILDREN,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            560,
            608,
            None,
            None,
            Some(hinstance.into()),
            None,
        )
        .unwrap();

        // 居中窗口
        center_window(hwnd, 560, 608);

        // 4. 创建 WebView2 实例并配置 IPC
        let webview_arc = Arc::new(Mutex::new(None::<WebView>));
        let webview_clone = webview_arc.clone();
        let state_clone = state.clone();
        let hwnd_addr = hwnd.0 as usize;

        let webview = match WebView::create(hwnd, HTML_CONTENT, move |json_msg| {
            let hwnd = HWND(hwnd_addr as *mut _);
            handle_setup_command(hwnd, json_msg, &state_clone, &webview_clone);
        }) {
            Ok(wv) => wv,
            Err(e) => {
                let msg = wide(&format!("无法初始化安装向导界面：{e}"));
                let _ = MessageBoxW(Some(hwnd), PCWSTR(msg.as_ptr()), w!("安装失败"), MB_OK | MB_ICONERROR);
                let _ = DestroyWindow(hwnd);
                return;
            }
        };

        *webview_arc.lock().unwrap() = Some(webview);

        SetWindowLongPtrW(
            hwnd,
            GWLP_USERDATA,
            Arc::into_raw(webview_arc.clone()) as isize,
        );

        let _ = ShowWindow(hwnd, SW_SHOW);

        // 消息循环
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

unsafe fn center_window(hwnd: HWND, w: i32, h: i32) {
    use crate::ui::winutil::work_area;
    let wa = work_area();
    let x = wa.left + (wa.right - wa.left - w).max(0) / 2;
    let y = wa.top + (wa.bottom - wa.top - h).max(0) / 2;
    let _ = SetWindowPos(hwnd, None, x, y, w, h, SWP_NOZORDER | SWP_NOACTIVATE);
}

fn handle_setup_command(
    hwnd: HWND,
    json_msg: &str,
    state_arc: &Arc<Mutex<SetupState>>,
    webview_arc: &Arc<Mutex<Option<WebView>>>,
) {
    let Ok(cmd) = protocol::parse_command(json_msg) else {
        return;
    };

    match cmd {
        FrontendCommand::Init { .. } => {
            let state = state_arc.lock().unwrap();
            let actual = state.theme_config.resolve_actual();
            let snapshot = InitSnapshotPayload {
                protocol_version: protocol::PROTOCOL_VERSION,
                window_type: "setup".into(),
                theme_mode: state.theme_config.mode.clone(),
                actual_theme: actual,
                main: None,
                setup: Some(SetupSnapshot {
                    default_dir: state.options.install_dir.to_string_lossy().to_string(),
                    desktop_shortcut: state.options.desktop_shortcut,
                    start_menu_shortcut: state.options.start_menu_shortcut,
                    launch_after: state.options.launch_after,
                    driver_status: state.driver_status.clone(),
                    driver_detail: state.driver_detail.clone(),
                }),
                uninstall: None,
            };
            let event = HostEvent::InitSnapshot(Box::new(snapshot));
            if let Ok(guard) = webview_arc.lock() {
                if let Some(wv) = guard.as_ref() {
                    let _ = wv.post_message(&serde_json::to_string(&event).unwrap());
                }
            }
        }
        FrontendCommand::SetTheme { mode } => {
            let mut state = state_arc.lock().unwrap();
            state.theme_config.mode = mode.clone();
            let _ = state.theme_config.save_for_current_user();
            let actual = state.theme_config.resolve_actual();
            let event = HostEvent::ThemeChanged { mode, actual };
            if let Ok(guard) = webview_arc.lock() {
                if let Some(wv) = guard.as_ref() {
                    let _ = wv.post_message(&serde_json::to_string(&event).unwrap());
                }
            }
        }
        FrontendCommand::WindowAction { action } => match action.as_str() {
            "close" => unsafe {
                let _ = DestroyWindow(hwnd);
            },
            _ => {}
        },
        FrontendCommand::BrowseInstallDir => {
            let current = {
                let state = state_arc.lock().unwrap();
                state.options.install_dir.to_string_lossy().to_string()
            };
            if let Some(chosen) = ui::pick_directory(hwnd, &current) {
                let mut state = state_arc.lock().unwrap();
                state.options.install_dir = chosen.clone();
                let system_root = std::env::var_os("SystemRoot")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
                let valid = flow::validate_options(
                    &state.options,
                    &system_root,
                    knownfolder::program_files_x86().ok().as_deref(),
                )
                .is_ok();

                let event = HostEvent::InstallDirValidated {
                    path: chosen.to_string_lossy().to_string(),
                    valid,
                    error: if valid { None } else { Some("安装目录不合法或受限".into()) },
                };
                if let Ok(guard) = webview_arc.lock() {
                    if let Some(wv) = guard.as_ref() {
                        let _ = wv.post_message(&serde_json::to_string(&event).unwrap());
                    }
                }
            }
        }
        FrontendCommand::SetInstallDir { path } => {
            let mut state = state_arc.lock().unwrap();
            let pb = PathBuf::from(&path);
            state.options.install_dir = pb;
            let system_root = std::env::var_os("SystemRoot")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
            let res = flow::validate_options(
                &state.options,
                &system_root,
                knownfolder::program_files_x86().ok().as_deref(),
            );
            let (valid, error) = match res {
                Ok(_) => (true, None),
                Err(e) => (false, Some(e.message().to_string())),
            };

            let event = HostEvent::InstallDirValidated {
                path,
                valid,
                error,
            };
            if let Ok(guard) = webview_arc.lock() {
                if let Some(wv) = guard.as_ref() {
                    let _ = wv.post_message(&serde_json::to_string(&event).unwrap());
                }
            }
        }
        FrontendCommand::SetInstallOption { key, value } => {
            let mut state = state_arc.lock().unwrap();
            match key.as_str() {
                "desktopShortcut" => state.options.desktop_shortcut = value,
                "startMenuShortcut" => state.options.start_menu_shortcut = value,
                "launchAfter" => state.options.launch_after = value,
                _ => {}
            }
        }
        FrontendCommand::StartInstall => {
            let mut state = state_arc.lock().unwrap();
            if state.is_installing {
                return;
            }
            state.is_installing = true;

            // 写入安全 origin 握手信息
            let nonce = origin::new_nonce();
            let origin_obj = match origin::capture_current_origin(&nonce) {
                Ok(o) => o,
                Err(e) => {
                    let event = HostEvent::InstallFinished {
                        ok: false,
                        message: format!("无法生成安装安全凭证: {e}"),
                        need_reboot: false,
                    };
                    if let Ok(guard) = webview_arc.lock() {
                        if let Some(wv) = guard.as_ref() {
                            let _ = wv.post_message(&serde_json::to_string(&event).unwrap());
                        }
                    }
                    return;
                }
            };

            let origin_dir = match origin::write_origin(&origin_obj) {
                Ok(d) => d,
                Err(e) => {
                    let event = HostEvent::InstallFinished {
                        ok: false,
                        message: format!("无法创建安装交接目录: {e}"),
                        need_reboot: false,
                    };
                    if let Ok(guard) = webview_arc.lock() {
                        if let Some(wv) = guard.as_ref() {
                            let _ = wv.post_message(&serde_json::to_string(&event).unwrap());
                        }
                    }
                    return;
                }
            };

            state.origin_dir = Some(origin_dir.clone());

            // 写入 options.json
            let opts_value = serde_json::json!({
                "installDir": state.options.install_dir.to_string_lossy(),
                "desktopShortcut": state.options.desktop_shortcut,
                "startMenuShortcut": state.options.start_menu_shortcut,
                "launchAfter": state.options.launch_after,
                "driverOnly": false
            });
            let _ = std::fs::write(origin_dir.join("options.json"), opts_value.to_string());

            let wv_ref = webview_arc.clone();
            let install_dir = state.options.install_dir.clone();
            let launch_after = state.options.launch_after;

            // 启动后台线程执行提权 worker 并监控进度
            std::thread::spawn(move || {
                let worker_args = vec![
                    "--worker".to_string(),
                    format!("--origin={}", origin_dir.to_string_lossy()),
                    format!("--nonce={nonce}"),
                ];

                // 启动提权进程
                let relaunch_res = relaunch_worker_elevated(&worker_args);

                if let Err(e) = relaunch_res {
                    let event = HostEvent::InstallFinished {
                        ok: false,
                        message: e,
                        need_reboot: false,
                    };
                    if let Ok(guard) = wv_ref.lock() {
                        if let Some(wv) = guard.as_ref() {
                            let _ = wv.post_message(&serde_json::to_string(&event).unwrap());
                        }
                    }
                    return;
                }

                // 循环读取进度
                let deadline = std::time::Instant::now() + Duration::from_secs(180);
                while std::time::Instant::now() < deadline {
                    let status_file = origin_dir.join("status.json");
                    if let Ok(status_str) = std::fs::read_to_string(&status_file) {
                        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&status_str) {
                            let percent = v.get("percent").and_then(|p| p.as_u64()).unwrap_or(0) as u32;
                            let message = v.get("message").and_then(|m| m.as_str()).unwrap_or("").to_string();
                            let event = HostEvent::InstallProgress { percent, message };
                            if let Ok(guard) = wv_ref.lock() {
                                if let Some(wv) = guard.as_ref() {
                                    let _ = wv.post_message(&serde_json::to_string(&event).unwrap());
                                }
                            }
                        }
                    }

                    let result_file = origin_dir.join("result.json");
                    if let Ok(result_str) = std::fs::read_to_string(&result_file) {
                        if let Ok(res) = serde_json::from_str::<InstallResult>(&result_str) {
                            let event = HostEvent::InstallFinished {
                                ok: res.ok,
                                message: res.message.clone(),
                                need_reboot: res.need_reboot,
                            };
                            if let Ok(guard) = wv_ref.lock() {
                                if let Some(wv) = guard.as_ref() {
                                    let _ = wv.post_message(&serde_json::to_string(&event).unwrap());
                                }
                            }
                            if res.ok && launch_after && !res.need_reboot {
                                let exe = install_dir.join(crate::install::GUI_EXE_NAME);
                                if exe.is_file() {
                                    let exe_w = wide(&exe.to_string_lossy());
                                    unsafe {
                                        let _ = ShellExecuteW(None, w!("open"), PCWSTR(exe_w.as_ptr()), None, None, SW_SHOWNORMAL);
                                    }
                                }
                            }
                            break;
                        }
                    }
                    std::thread::sleep(Duration::from_millis(250));
                }
            });
        }
        FrontendCommand::CancelInstall => {
            let state = state_arc.lock().unwrap();
            if let Some(dir) = &state.origin_dir {
                origin::request_cancel(dir);
            }
        }
        _ => {}
    }
}

fn relaunch_worker_elevated(args: &[String]) -> Result<(), String> {
    use windows::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
    use windows::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject};
    use windows::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};

    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let executable = wide(&exe.to_string_lossy());
    let arguments = args
        .iter()
        .map(|a| format!("\"{a}\""))
        .collect::<Vec<_>>()
        .join(" ");
    let parameters = wide(&arguments);

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
                "已取消管理员授权，安装未完成。".into()
            } else {
                format!("无法启动提权安装进程：{e}")
            }
        })?;

        if info.hProcess.is_invalid() {
            return Err("安装进程没有返回句柄。".into());
        }

        let wait = WaitForSingleObject(info.hProcess, u32::MAX);
        let mut code = 0;
        let _ = GetExitCodeProcess(info.hProcess, &mut code);
        let _ = CloseHandle(info.hProcess);

        if wait != WAIT_OBJECT_0 || code != 0 {
            return Err(format!("安装未成功完成 (退出码: {code})。"));
        }
    }
    Ok(())
}

unsafe extern "system" fn setup_wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_SIZE => {
            let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA);
            if ptr != 0 {
                let webview_arc = &*(ptr as *const Mutex<Option<WebView>>);
                if let Ok(guard) = webview_arc.lock() {
                    if let Some(wv) = guard.as_ref() {
                        let mut rc = RECT::default();
                        let _ = GetClientRect(hwnd, &mut rc);
                        wv.resize(rc);
                    }
                }
            }
            LRESULT(0)
        }
        WM_CLOSE => {
            let _ = DestroyWindow(hwnd);
            LRESULT(0)
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}
