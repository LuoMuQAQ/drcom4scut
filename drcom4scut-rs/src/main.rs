#![windows_subsystem = "windows"]

//! 入口：单实例 → 主窗口 → 消息循环。
//! 业务初始化（设置迁移、Npcap、核心释放、托盘、自动连接）在窗口创建后完成。

use drcom4scut_gui::platform;
use drcom4scut_gui::ui;
use windows::core::w;
use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetMessageW, MessageBoxW, TranslateMessage, MB_ICONERROR, MB_OK, MSG,
};

fn main() {
    let guard = match platform::acquire(platform::MUTEX_NAME) {
        Ok(g) => g,
        Err(platform::AlreadyRunning::InstanceRunning) => {
            let _ = platform::Guard::wake_other(platform::WAKE_EVENT_NAME);
            return;
        }
        Err(platform::AlreadyRunning::Win32(e)) => {
            let text = format!("无法获取单实例锁：{e}");
            let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
            unsafe {
                let _ = MessageBoxW(
                    None,
                    windows::core::PCWSTR(wide.as_ptr()),
                    w!("校园网"),
                    MB_OK | MB_ICONERROR,
                );
            }
            return;
        }
    };

    let Some(hwnd) = ui::window::create_main_window() else {
        unsafe {
            let _ = MessageBoxW(
                None,
                w!("无法创建主窗口。"),
                w!("校园网"),
                MB_OK | MB_ICONERROR,
            );
        }
        return;
    };

    let hwnd_addr = hwnd.0 as usize;
    guard.on_wake(move || unsafe {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::UI::WindowsAndMessaging::PostMessageW;
        let hwnd = HWND(hwnd_addr as *mut _);
        let _ = PostMessageW(Some(hwnd), ui::window::WM_APP_OPEN, WPARAM(0), LPARAM(0));
    });
    let hwnd_exit = hwnd.0 as usize;
    guard.on_exit(move || unsafe {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::UI::WindowsAndMessaging::PostMessageW;
        let hwnd = HWND(hwnd_exit as *mut _);
        let _ = PostMessageW(Some(hwnd), ui::window::WM_APP_EXIT, WPARAM(0), LPARAM(0));
    });

    unsafe {
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            if msg.message == windows::Win32::UI::WindowsAndMessaging::WM_KEYDOWN
                && msg.wParam.0 == 9
                && windows::Win32::UI::WindowsAndMessaging::IsDialogMessageW(hwnd, &msg).as_bool()
            {
                continue;
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}
