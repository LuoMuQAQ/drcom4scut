#![windows_subsystem = "windows"]

//! HeroUI v3 + WebView2 阶段 2 共享视觉系统与协议验证入口。
//! 支持通过命令行参数 --view=main/setup/uninstall 预览三个视图，
//! 严格使用版本化安全 IPC 协议（ui::protocol）。

use std::sync::Arc;

use drcom4scut_gui::ui::protocol::{self, FrontendCommand, HostEvent, InitSnapshotPayload, MainSnapshot, SafeSettingsSnapshot, SetupSnapshot, StatusInfo, ThemeConfig, UninstallSnapshot};
use drcom4scut_gui::ui::webview::{self, RuntimeStatus, WebView};
use drcom4scut_gui::ui::winutil::wide;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetClientRect, GetMessageW,
    MessageBoxW, PostQuitMessage, RegisterClassW, ShowWindow, TranslateMessage, CS_HREDRAW,
    CS_VREDRAW, MB_ICONERROR, MB_OK, MSG, SW_SHOW, WINDOW_EX_STYLE, WM_CLOSE, WM_DESTROY,
    WM_SIZE, WNDCLASSW, WS_OVERLAPPEDWINDOW,
};

const HTML_CONTENT: &str = include_str!("../../frontend/dist/index.html");

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let view_type = args
        .iter()
        .find_map(|a| a.strip_prefix("--view="))
        .unwrap_or("main");

    // 1. 检查 WebView2 运行时
    let runtime = webview::detect_runtime();
    match runtime {
        RuntimeStatus::Installed { version, path } => {
            println!("检测到 WebView2 运行时：版本 {version}，路径 {:?}", path);
        }
        RuntimeStatus::Missing => {
            unsafe {
                let msg = wide("系统中未检测到 Microsoft Edge WebView2 运行时。\n\n请先从微软官方下载安装 WebView2 Runtime 后再运行本程序。");
                let _ = MessageBoxW(
                    None,
                    PCWSTR(msg.as_ptr()),
                    w!("校园网认证客户端"),
                    MB_OK | MB_ICONERROR,
                );
            }
            return;
        }
    }

    // 2. 加载用户独立主题设置
    let theme_config = std::sync::Arc::new(std::sync::Mutex::new(ThemeConfig::load_for_current_user()));

    // 3. 创建宿主窗口
    let hinstance = unsafe { windows::Win32::System::LibraryLoader::GetModuleHandleW(None).unwrap() };
    let class_name = w!("DrcomHeroUIPreviewWnd");

    let wc = WNDCLASSW {
        lpfnWndProc: Some(wndproc),
        hInstance: hinstance.into(),
        lpszClassName: class_name,
        style: CS_HREDRAW | CS_VREDRAW,
        ..Default::default()
    };
    unsafe {
        RegisterClassW(&wc);
    }

    let (win_w, win_h, win_title) = match view_type {
        "setup" => (560, 608, "校园网客户端 · 安装向导"),
        "uninstall" => (480, 360, "校园网客户端 · 卸载"),
        _ => (420, 520, "校园网客户端 · HeroUI v3"),
    };

    let wide_title = wide(win_title);
    let hwnd = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            class_name,
            PCWSTR(wide_title.as_ptr()),
            WS_OVERLAPPEDWINDOW,
            120,
            120,
            win_w,
            win_h,
            None,
            None,
            Some(hinstance.into()),
            None,
        )
        .unwrap()
    };

    // 4. 创建 WebView2 实例并绑定受控白名单 IPC
    let webview_arc = Arc::new(std::sync::Mutex::new(None::<WebView>));
    let webview_clone = webview_arc.clone();
    let theme_clone = theme_config.clone();
    let current_view = view_type.to_string();

    let hwnd_addr = hwnd.0 as usize;
    let webview = match WebView::create(hwnd, HTML_CONTENT, move |json_msg| {
        let hwnd = HWND(hwnd_addr as *mut _);
        println!("[Rust IPC] 收到命令: {json_msg}");
        let cmd = match protocol::parse_command(json_msg) {
            Ok(c) => c,
            Err(e) => {
                println!("[Rust IPC 安全拦截] 拒绝执行未授权或非法命令: {e}");
                return;
            }
        };

        match cmd {
            FrontendCommand::Init { .. } => {
                let theme_cfg = theme_clone.lock().unwrap().clone();
                let actual = theme_cfg.resolve_actual();

                let snapshot = InitSnapshotPayload {
                    protocol_version: protocol::PROTOCOL_VERSION,
                    window_type: current_view.clone(),
                    theme_mode: theme_cfg.mode,
                    actual_theme: actual,
                    main: if current_view == "main" {
                        Some(MainSnapshot {
                            settings: SafeSettingsSnapshot {
                                username: "20240001".into(),
                                has_password: true,
                                auto_connect: true,
                                remember_password: true,
                                startup: true,
                                tray_keep: true,
                            },
                            adapters: vec![
                                protocol::AdapterInfo {
                                    id: "eth0".into(),
                                    name: "以太网 (Intel Gigabit Ethernet)".into(),
                                    description: "PCI\\VEN_8086".into(),
                                    mac: "00:1A:2B:3C:4D:5E".into(),
                                    is_up: true,
                                    has_gateway: true,
                                },
                                protocol::AdapterInfo {
                                    id: "wlan0".into(),
                                    name: "WLAN (Wi-Fi 6 AX200)".into(),
                                    description: "PCI\\VEN_8086".into(),
                                    mac: "00:1A:2B:3C:4D:5F".into(),
                                    is_up: true,
                                    has_gateway: false,
                                },
                            ],
                            selected_adapter_id: Some("eth0".into()),
                            status: StatusInfo {
                                state: "idle".into(),
                                title: "已就绪".into(),
                                detail: "网卡准备完成，点击连接认证".into(),
                                indicator: "idle".into(),
                            },
                        })
                    } else {
                        None
                    },
                    setup: if current_view == "setup" {
                        Some(SetupSnapshot {
                            default_dir: r"C:\Program Files\drcom4scut".into(),
                            desktop_shortcut: true,
                            start_menu_shortcut: true,
                            launch_after: true,
                            driver_status: "已就绪".into(),
                            driver_detail: "Npcap 1.80 (已安装并支持)".into(),
                        })
                    } else {
                        None
                    },
                    uninstall: if current_view == "uninstall" {
                        Some(UninstallSnapshot {
                            install_dir: r"C:\Program Files\drcom4scut".into(),
                            version: "0.3.6".into(),
                            is_elevated: true,
                        })
                    } else {
                        None
                    },
                };

                let event = HostEvent::InitSnapshot(Box::new(snapshot));
                if let Ok(guard) = webview_clone.lock() {
                    if let Some(wv) = guard.as_ref() {
                        let _ = wv.post_message(&serde_json::to_string(&event).unwrap());
                    }
                }
            }
            FrontendCommand::SetTheme { mode } => {
                let mut guard = theme_clone.lock().unwrap();
                guard.mode = mode.clone();
                let _ = guard.save_for_current_user();
                let actual = guard.resolve_actual();

                let event = HostEvent::ThemeChanged {
                    mode,
                    actual,
                };
                if let Ok(wv_guard) = webview_clone.lock() {
                    if let Some(wv) = wv_guard.as_ref() {
                        let _ = wv.post_message(&serde_json::to_string(&event).unwrap());
                    }
                }
            }
            FrontendCommand::WindowAction { action } => match action.as_str() {
                "minimize" => unsafe {
                    use windows::Win32::UI::WindowsAndMessaging::ShowWindow;
                    let _ = ShowWindow(hwnd, windows::Win32::UI::WindowsAndMessaging::SW_MINIMIZE);
                },
                "close" => unsafe {
                    let _ = DestroyWindow(hwnd);
                },
                _ => {}
            },
            FrontendCommand::Connect => {
                println!("[Rust] 收到连接请求，模拟认证流程…");
                let wv_ref = webview_clone.clone();
                std::thread::spawn(move || {
                    let update = |state: &str, title: &str, detail: &str, indicator: &str| {
                        let event = HostEvent::StatusChanged {
                            state: state.into(),
                            title: title.into(),
                            detail: detail.into(),
                            indicator: indicator.into(),
                        };
                        if let Ok(guard) = wv_ref.lock() {
                            if let Some(wv) = guard.as_ref() {
                                let _ = wv.post_message(&serde_json::to_string(&event).unwrap());
                            }
                        }
                    };

                    update("connecting", "正在连接…", "正在发送 EAPoL 认证报文", "progress");
                    std::thread::sleep(std::time::Duration::from_millis(800));
                    update("connecting", "正在登录…", "正在验证学号与密码凭据", "progress");
                    std::thread::sleep(std::time::Duration::from_millis(800));
                    update("connected", "已连接校园网", "已登录 · IP: 10.12.34.56 (正常心跳)", "success");
                });
            }
            FrontendCommand::Disconnect => {
                let event = HostEvent::StatusChanged {
                    state: "idle".into(),
                    title: "未连接".into(),
                    detail: "已断开连接".into(),
                    indicator: "idle".into(),
                };
                if let Ok(guard) = webview_clone.lock() {
                    if let Some(wv) = guard.as_ref() {
                        let _ = wv.post_message(&serde_json::to_string(&event).unwrap());
                    }
                }
            }
            FrontendCommand::StartInstall => {
                println!("[Rust] 开始安装向导测试流程…");
                let wv_ref = webview_clone.clone();
                std::thread::spawn(move || {
                    for (percent, msg) in [
                        (15, "正在解压并校验客户端核心…"),
                        (45, "正在释放应用文件及资源…"),
                        (75, "正在创建桌面与开始菜单快捷方式…"),
                        (90, "正在注册系统卸载信息…"),
                        (100, "安装步骤完成"),
                    ] {
                        let event = HostEvent::InstallProgress {
                            percent,
                            message: msg.into(),
                        };
                        if let Ok(guard) = wv_ref.lock() {
                            if let Some(wv) = guard.as_ref() {
                                let _ = wv.post_message(&serde_json::to_string(&event).unwrap());
                            }
                        }
                        std::thread::sleep(std::time::Duration::from_millis(400));
                    }
                    let finished = HostEvent::InstallFinished {
                        ok: true,
                        message: "校园网认证客户端已成功安装到目标路径。".into(),
                        need_reboot: false,
                    };
                    if let Ok(guard) = wv_ref.lock() {
                        if let Some(wv) = guard.as_ref() {
                            let _ = wv.post_message(&serde_json::to_string(&finished).unwrap());
                        }
                    }
                });
            }
            FrontendCommand::ConfirmUninstall => {
                println!("[Rust] 收到卸载确认，执行模拟卸载…");
                let wv_ref = webview_clone.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(600));
                    let finished = HostEvent::UninstallFinished {
                        ok: true,
                        message: "所有程序组件、配置文件及快捷方式已安全移除。".into(),
                    };
                    if let Ok(guard) = wv_ref.lock() {
                        if let Some(wv) = guard.as_ref() {
                            let _ = wv.post_message(&serde_json::to_string(&finished).unwrap());
                        }
                    }
                });
            }
            _ => {}
        }
    }) {
        Ok(wv) => wv,
        Err(e) => {
            unsafe {
                let err_text = wide(&format!("初始化 WebView2 失败：{e}"));
                let _ = MessageBoxW(
                    Some(hwnd),
                    PCWSTR(err_text.as_ptr()),
                    w!("错误"),
                    MB_OK | MB_ICONERROR,
                );
            }
            return;
        }
    };

    *webview_arc.lock().unwrap() = Some(webview);

    unsafe {
        windows::Win32::UI::WindowsAndMessaging::SetWindowLongPtrW(
            hwnd,
            windows::Win32::UI::WindowsAndMessaging::GWLP_USERDATA,
            Arc::into_raw(webview_arc.clone()) as isize,
        );
        let _ = ShowWindow(hwnd, SW_SHOW);
    }

    unsafe {
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_SIZE => {
            let ptr = windows::Win32::UI::WindowsAndMessaging::GetWindowLongPtrW(
                hwnd,
                windows::Win32::UI::WindowsAndMessaging::GWLP_USERDATA,
            );
            if ptr != 0 {
                let webview_arc = &*(ptr as *const std::sync::Mutex<Option<WebView>>);
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
