#![windows_subsystem = "windows"]

//! HeroUI v3 + WebView2 可行性验证原型入口。
//! 加载离线打包的 React + HeroUI 页面，验证浅/深色切换、双向受控 IPC 通信。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use drcom4scut_gui::ui::webview::{self, RuntimeStatus, WebView};
use drcom4scut_gui::ui::winutil::{self, wide};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetClientRect, GetMessageW,
    MessageBoxW, PostQuitMessage, RegisterClassW, SetWindowPos, ShowWindow, TranslateMessage,
    CS_HREDRAW, CS_VREDRAW, MB_ICONERROR, MB_OK, MSG, SWP_NOZORDER, SW_SHOW, WINDOW_EX_STYLE,
    WM_CLOSE, WM_CREATE, WM_DESTROY, WM_SIZE, WNDCLASSW, WS_OVERLAPPEDWINDOW,
};

const HTML_CONTENT: &str = include_str!("../../frontend/dist/index.html");

static RUNNING: AtomicBool = AtomicBool::new(true);

fn main() {
    let start_time = Instant::now();

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
                    w!("校园网认证客户端 · 原型"),
                    MB_OK | MB_ICONERROR,
                );
            }
            return;
        }
    }

    // 2. 创建宿主窗口
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

    // 440 x 540 窗口大小
    let hwnd = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            class_name,
            w!("校园网客户端 · HeroUI v3 预览"),
            WS_OVERLAPPEDWINDOW,
            150,
            150,
            440,
            540,
            None,
            None,
            Some(hinstance.into()),
            None,
        )
        .unwrap()
    };

    println!("宿主窗口创建耗时: {:?}", start_time.elapsed());

    // 3. 创建 WebView2 实例并绑定受控 IPC
    let webview_arc = Arc::new(std::sync::Mutex::new(None::<WebView>));
    let webview_clone = webview_arc.clone();

    let webview_init_start = Instant::now();
    let webview = match WebView::create(hwnd, HTML_CONTENT, move |json_msg| {
        println!("[Rust IPC 收到前端消息] {json_msg}");
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(json_msg) {
            let msg_type = v.get("type").and_then(|t| t.as_str()).unwrap_or_default();
            match msg_type {
                "READY" => {
                    let client = v.get("client").and_then(|c| c.as_str()).unwrap_or("未知");
                    println!("[Rust] 前端客户端已就绪: {client}");
                    if let Ok(guard) = webview_clone.lock() {
                        if let Some(wv) = guard.as_ref() {
                            let _ = wv.post_message(&serde_json::json!({
                                "type": "STATUS",
                                "status": "Rust 认证就绪（就绪信号握手成功）",
                                "time": chrono_now_str(),
                            }).to_string());
                        }
                    }
                }
                "PING" => {
                    let count = v.get("count").and_then(|c| c.as_u64()).unwrap_or(0);
                    let resp = serde_json::json!({
                        "type": "PONG",
                        "count": count,
                        "timestamp": chrono_now_str(),
                    });
                    if let Ok(guard) = webview_clone.lock() {
                        if let Some(wv) = guard.as_ref() {
                            let _ = wv.post_message(&resp.to_string());
                        }
                    }
                }
                "THEME_CHANGE" => {
                    let theme = v.get("theme").and_then(|t| t.as_str()).unwrap_or("dark");
                    println!("[Rust] 前端切换主题为: {theme}");
                }
                _ => {
                    println!("[Rust] 未知命令: {msg_type}");
                }
            }
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

    println!("WebView2 初始化耗时: {:?}", webview_init_start.elapsed());
    *webview_arc.lock().unwrap() = Some(webview);

    // 绑定 HWND USERDATA
    unsafe {
        windows::Win32::UI::WindowsAndMessaging::SetWindowLongPtrW(
            hwnd,
            windows::Win32::UI::WindowsAndMessaging::GWLP_USERDATA,
            Arc::into_raw(webview_arc.clone()) as isize,
        );
        let _ = ShowWindow(hwnd, SW_SHOW);
    }

    println!("总启动时间（到页面展示）: {:?}", start_time.elapsed());

    // Win32 消息循环
    unsafe {
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

fn chrono_now_str() -> String {
    use std::time::SystemTime;
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    format!("{}:{:02}:{:02}", (now / 3600 + 8) % 24, (now % 3600) / 60, now % 60)
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
