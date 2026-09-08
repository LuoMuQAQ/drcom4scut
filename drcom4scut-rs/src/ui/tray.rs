//! 系统托盘：图标 + 悬停提示 + 右键菜单 + 双击唤起。
//!
//! 行为对齐 .NET 版 `MainWindow.xaml.cs:436-447`：双击显示主窗口；右键菜单为
//! 打开 / 连接 / 断开 / 退出；提示文本随状态更新为「校园网 · {标题}」。
//! 菜单动作通过自定义消息转发给主窗口处理，托盘模块自身不持有业务状态。

use std::sync::atomic::{AtomicIsize, Ordering};

use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY,
    NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyWindow, GetCursorPos,
    RegisterClassW, SetForegroundWindow, TrackPopupMenu, CS_HREDRAW, CS_VREDRAW, MF_SEPARATOR,
    MF_STRING, TPM_RETURNCMD, TPM_RIGHTBUTTON, WM_APP, WNDCLASSW, WS_OVERLAPPED,
};

/// 托盘回调消息（NOTIFYICONDATAW.uCallbackMessage）。
pub const WM_APP_TRAY: u32 = WM_APP + 1;

/// 菜单命令 ID。
const ID_OPEN: u32 = 1;
const ID_CONNECT: u32 = 2;
const ID_DISCONNECT: u32 = 3;
const ID_EXIT: u32 = 4;

static MAIN_HWND: AtomicIsize = AtomicIsize::new(0);

fn post_main(msg: u32) {
    let main = MAIN_HWND.load(Ordering::Relaxed);
    if main != 0 {
        unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::PostMessageW(
                Some(HWND(main as *mut _)),
                msg,
                WPARAM(0),
                LPARAM(0),
            );
        }
    }
}

/// 托盘句柄集。Drop 时移除图标并销毁隐藏窗口。
pub struct Tray {
    hwnd: HWND,
    /// 保留 NIM_ADD 时使用的 hIcon 字节数据归属；图标本体由调用方持有。
    _added: bool,
}

impl Tray {
    /// 创建托盘图标。`main` 是接收菜单动作消息的主窗口。
    pub fn create(main: HWND, icon: HIconOrFile) -> Option<Tray> {
        unsafe {
            let hinstance = windows::Win32::System::LibraryLoader::GetModuleHandleW(None).ok()?;
            let class_name = w!("DrcomTrayWnd");

            static REGISTERED: std::sync::Once = std::sync::Once::new();
            REGISTERED.call_once(|| {
                let wc = WNDCLASSW {
                    lpfnWndProc: Some(tray_wndproc),
                    hInstance: hinstance.into(),
                    lpszClassName: class_name,
                    style: CS_HREDRAW | CS_VREDRAW,
                    ..Default::default()
                };
                RegisterClassW(&wc);
            });

            let hwnd = CreateWindowExW(
                windows::Win32::UI::WindowsAndMessaging::WINDOW_EX_STYLE(0),
                class_name,
                w!("drcom4scut-tray"),
                WS_OVERLAPPED,
                0,
                0,
                0,
                0,
                None,
                None,
                Some(hinstance.into()),
                None,
            )
            .ok()?;

            // 消息专用窗口没有 user data 槽位可用性差异，直接存静态里。
            MAIN_HWND.store(main.0 as isize, Ordering::Relaxed);

            let mut data = NOTIFYICONDATAW {
                cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
                hWnd: hwnd,
                uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP,
                uCallbackMessage: WM_APP_TRAY,
                szTip: utf16_tip("校园网"),
                ..Default::default()
            };
            match icon {
                HIconOrFile::Handle(h) => data.hIcon = h,
                HIconOrFile::File(path, cx, cy) => {
                    if let Some(h) = super::winutil::load_icon(&path, cx, cy) {
                        data.hIcon = h;
                    }
                }
            }
            let added = Shell_NotifyIconW(NIM_ADD, &data).as_bool();
            if !added {
                let _ = DestroyWindow(hwnd);
                return None;
            }
            Some(Tray { hwnd, _added: true })
        }
    }

    /// 更新悬停提示：「校园网 · {状态标题}」。
    pub fn set_tooltip(&self, text: &str) {
        unsafe {
            let mut data = NOTIFYICONDATAW {
                cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
                hWnd: self.hwnd,
                uFlags: NIF_TIP,
                szTip: utf16_tip(text),
                ..Default::default()
            };
            let _ = Shell_NotifyIconW(NIM_MODIFY, &mut data);
        }
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        unsafe {
            let mut data = NOTIFYICONDATAW {
                cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
                hWnd: self.hwnd,
                ..Default::default()
            };
            let _ = Shell_NotifyIconW(NIM_DELETE, &mut data);
            let _ = DestroyWindow(self.hwnd);
            MAIN_HWND.store(0, Ordering::Relaxed);
        }
    }
}

/// 图标来源：现成句柄，或从文件加载指定尺寸。
pub enum HIconOrFile {
    Handle(windows::Win32::UI::WindowsAndMessaging::HICON),
    File(std::path::PathBuf, i32, i32),
}

/// szTip 容量 128 个 UTF-16 单元（含 NUL），超长截断。
fn utf16_tip(text: &str) -> [u16; 128] {
    let mut tip = [0u16; 128];
    for (i, unit) in text.encode_utf16().take(127).enumerate() {
        tip[i] = unit;
    }
    tip
}

extern "system" fn tray_wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        use windows::Win32::UI::WindowsAndMessaging::{
            SetWindowPos, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SW_HIDE, SW_SHOWNA,
            WM_CLOSE, WM_COMMAND, WM_LBUTTONDBLCLK, WM_NULL, WM_RBUTTONUP,
        };
        if msg == WM_APP_TRAY {
            let main = MAIN_HWND.load(Ordering::Relaxed);
            let main = HWND(main as *mut _);
            let mouse = lparam.0 as u32;
            match mouse {
                WM_LBUTTONDBLCLK => {
                    post_main(super::window::WM_APP_OPEN);
                }
                WM_RBUTTONUP => {
                    let mut point = Default::default();
                    let _ = GetCursorPos(&mut point);
                    let menu = CreatePopupMenu().unwrap_or_default();
                    if !menu.is_invalid() {
                        let _ = AppendMenuW(menu, MF_STRING, ID_OPEN as usize, w!("打开"));
                        let _ = AppendMenuW(menu, MF_STRING, ID_CONNECT as usize, w!("连接"));
                        let _ = AppendMenuW(menu, MF_STRING, ID_DISCONNECT as usize, w!("断开"));
                        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, w!(""));
                        let _ = AppendMenuW(menu, MF_STRING, ID_EXIT as usize, w!("退出"));
                        // 经典要求：先 foreground 再 TrackPopupMenu，否则菜单不消失。
                        let _ = SetForegroundWindow(hwnd);
                        let cmd = TrackPopupMenu(
                            menu,
                            TPM_RIGHTBUTTON | TPM_RETURNCMD,
                            point.x,
                            point.y,
                            Some(0),
                            hwnd,
                            None,
                        );
                        let _ = PostMessageW_compat(hwnd, WM_NULL);
                        if cmd.as_bool() {
                            let cmd_id = cmd.0 as u32;
                            match cmd_id {
                                ID_OPEN => post_main(super::window::WM_APP_OPEN),
                                ID_CONNECT => {
                                    // 对齐 .NET：菜单里的连接先显示窗口再连。
                                    post_main(super::window::WM_APP_OPEN);
                                    post_main(super::window::WM_APP_CONNECT);
                                }
                                ID_DISCONNECT => post_main(super::window::WM_APP_DISCONNECT),
                                ID_EXIT => post_main(super::window::WM_APP_EXIT),
                                _ => {}
                            }
                        }
                        let _ = windows::Win32::UI::WindowsAndMessaging::DestroyMenu(menu);
                    }
                }
                _ => {}
            }
            LRESULT(0)
        } else {
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
    }
}

/// PostMessageW 的窄封装（避免在闭包里重复 import）。
unsafe fn PostMessageW_compat(hwnd: HWND, msg: u32) {
    use windows::Win32::UI::WindowsAndMessaging::PostMessageW;
    let _ = PostMessageW(Some(hwnd), msg, WPARAM(0), LPARAM(0));
}
