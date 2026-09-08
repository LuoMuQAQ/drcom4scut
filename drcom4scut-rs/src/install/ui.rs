//! 安装器/卸载器共用的简洁原生窗口。

use std::path::PathBuf;

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, EndPaint, SetBkMode, SetTextColor, TextOutW, PAINTSTRUCT, TRANSPARENT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetClientRect, GetMessageW,
    GetWindowLongPtrW, GetWindowTextW, LoadCursorW, MessageBoxW, PostQuitMessage, RegisterClassW,
    SendMessageW, SetWindowLongPtrW, SetWindowPos, SetWindowTextW, ShowWindow, TranslateMessage,
    CS_HREDRAW, CS_VREDRAW, GWLP_USERDATA, HMENU, HWND_TOP, IDC_ARROW, MB_ICONWARNING, MB_OK,
    MB_OKCANCEL, MSG, SW_SHOW, WINDOW_EX_STYLE, WINDOW_STYLE, WM_CLOSE, WM_COMMAND, WM_CREATE,
    WM_CTLCOLORBTN, WM_CTLCOLOREDIT, WM_CTLCOLORSTATIC, WM_DESTROY, WM_ERASEBKGND, WM_LBUTTONDOWN,
    WM_PAINT, WM_SETFONT, WNDCLASSW, WS_CHILD, WS_CLIPCHILDREN, WS_EX_APPWINDOW, WS_OVERLAPPED,
    WS_POPUP, WS_TABSTOP, WS_VISIBLE,
};

use crate::install::{APP_DISPLAY_NAME, APP_VERSION};
use crate::ui::winutil::{
    self, create_font, delete_gdi, font_as_gdi, scale, solid_brush, wide, COLOR_ACCENT, COLOR_PAGE,
    COLOR_TEXT_PRIMARY, COLOR_TEXT_SECONDARY,
};

pub const ID_PATH: isize = 2001;
pub const ID_BROWSE: isize = 2002;
pub const ID_OK: isize = 2003;
pub const ID_CANCEL: isize = 2004;
pub const ID_DESKTOP: isize = 2005;
pub const ID_STARTMENU: isize = 2006;
pub const ID_NPCAP: isize = 2007;
pub const ID_LAUNCH: isize = 2008;

pub fn confirm(hwnd: HWND, title: &str, text: &str) -> bool {
    let t = wide(title);
    let b = wide(text);
    unsafe {
        MessageBoxW(
            Some(hwnd),
            PCWSTR(b.as_ptr()),
            PCWSTR(t.as_ptr()),
            MB_OKCANCEL | MB_ICONWARNING,
        )
        .0 == 1
    }
}

pub fn alert(hwnd: HWND, title: &str, text: &str) {
    let t = wide(title);
    let b = wide(text);
    unsafe {
        let _ = MessageBoxW(
            Some(hwnd),
            PCWSTR(b.as_ptr()),
            PCWSTR(t.as_ptr()),
            MB_OK | MB_ICONWARNING,
        );
    }
}

pub fn pick_directory(owner: HWND, current: &str) -> Option<PathBuf> {
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
        COINIT_APARTMENTTHREADED,
    };
    use windows::Win32::UI::Shell::{
        FileOpenDialog, IFileOpenDialog, IShellItem, SHCreateItemFromParsingName,
        FOS_FORCEFILESYSTEM, FOS_PICKFOLDERS, SIGDN_FILESYSPATH,
    };
    unsafe {
        struct ComGuard;
        impl Drop for ComGuard {
            fn drop(&mut self) {
                unsafe {
                    CoUninitialize();
                }
            }
        }
        CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok().ok()?;
        let _com = ComGuard;
        let dlg: IFileOpenDialog =
            CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        let _ = dlg.SetOptions(FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM);
        let current_path = PathBuf::from(current);
        if let Some(existing) = current_path.ancestors().find(|p| p.is_dir()) {
            let path = wide(&existing.to_string_lossy());
            if let Ok(item) =
                SHCreateItemFromParsingName::<_, _, IShellItem>(PCWSTR(path.as_ptr()), None)
            {
                let _ = dlg.SetFolder(&item);
            }
        }
        let _ = dlg.SetTitle(w!("选择校园网客户端安装文件夹"));
        if dlg.Show(Some(owner)).is_err() {
            return None;
        }
        let item = dlg.GetResult().ok()?;
        let psz = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let s = psz.to_string();
        windows::Win32::System::Com::CoTaskMemFree(Some(psz.0 as *const _));
        Some(PathBuf::from(s.ok()?))
    }
}

pub fn run_message_loop(hwnd: HWND) {
    unsafe {
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        let _ = hwnd;
    }
}

pub fn create_child_edit(
    parent: HWND,
    id: isize,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    text: &str,
) -> HWND {
    unsafe {
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE(0x200), // WS_EX_CLIENTEDGE
            w!("EDIT"),
            PCWSTR::null(),
            WINDOW_STYLE(WS_CHILD.0 | WS_VISIBLE.0 | WS_TABSTOP.0 | 0x80), // ES_AUTOHSCROLL
            x,
            y,
            w,
            h,
            Some(parent),
            Some(HMENU(id as *mut _)),
            None,
            None,
        )
        .unwrap_or_default();
        let t = wide(text);
        let _ = SetWindowTextW(hwnd, PCWSTR(t.as_ptr()));
        hwnd
    }
}

pub fn create_child_button(
    parent: HWND,
    id: isize,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    text: &str,
) -> HWND {
    unsafe {
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("BUTTON"),
            PCWSTR::null(),
            WINDOW_STYLE(WS_CHILD.0 | WS_VISIBLE.0 | WS_TABSTOP.0),
            x,
            y,
            w,
            h,
            Some(parent),
            Some(HMENU(id as *mut _)),
            None,
            None,
        )
        .unwrap_or_default();
        let t = wide(text);
        let _ = SetWindowTextW(hwnd, PCWSTR(t.as_ptr()));
        hwnd
    }
}

pub fn create_child_check(
    parent: HWND,
    id: isize,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    text: &str,
    on: bool,
) -> HWND {
    unsafe {
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("BUTTON"),
            PCWSTR::null(),
            WINDOW_STYLE(WS_CHILD.0 | WS_VISIBLE.0 | WS_TABSTOP.0 | 0x0002 | 0x0003), // BS_AUTOCHECKBOX
            x,
            y,
            w,
            h,
            Some(parent),
            Some(HMENU(id as *mut _)),
            None,
            None,
        )
        .unwrap_or_default();
        let t = wide(text);
        let _ = SetWindowTextW(hwnd, PCWSTR(t.as_ptr()));
        if on {
            let _ = SendMessageW(hwnd, 0x00F1, Some(WPARAM(1)), Some(LPARAM(0)));
            // BM_SETCHECK
        }
        hwnd
    }
}

pub fn is_checked(hwnd: HWND) -> bool {
    unsafe { SendMessageW(hwnd, 0x00F0, Some(WPARAM(0)), Some(LPARAM(0))).0 != 0 }
    // BM_GETCHECK
}

pub fn edit_text(hwnd: HWND) -> String {
    unsafe {
        let mut buf = [0u16; 1024];
        let n = GetWindowTextW(hwnd, &mut buf) as usize;
        String::from_utf16_lossy(&buf[..n])
    }
}

pub fn apply_font(hwnd: HWND, font: windows::Win32::Graphics::Gdi::HFONT) {
    unsafe {
        let _ = SendMessageW(
            hwnd,
            WM_SETFONT,
            Some(WPARAM(font.0 as usize)),
            Some(LPARAM(1)),
        );
    }
}

pub fn paint_header(
    hdc: windows::Win32::Graphics::Gdi::HDC,
    title: &str,
    subtitle: &str,
    dpi: u32,
) {
    unsafe {
        let _ = SetBkMode(hdc, TRANSPARENT);
        let _ = SetTextColor(hdc, COLOR_TEXT_PRIMARY);
        let t: Vec<u16> = title.encode_utf16().collect();
        let _ = TextOutW(hdc, scale(20, dpi), scale(16, dpi), &t);
        let _ = SetTextColor(hdc, COLOR_TEXT_SECONDARY);
        let s: Vec<u16> = subtitle.encode_utf16().collect();
        let _ = TextOutW(hdc, scale(20, dpi), scale(40, dpi), &s);
    }
}

pub fn window_title() -> String {
    format!("{APP_DISPLAY_NAME} {APP_VERSION}")
}

/// 居中弹出无边框窗口，配色与主界面一致。
pub fn register_class(
    name: PCWSTR,
    wndproc: windows::Win32::UI::WindowsAndMessaging::WNDPROC,
) -> bool {
    let wc = WNDCLASSW {
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: wndproc,
        hCursor: unsafe { LoadCursorW(None, IDC_ARROW).unwrap_or_default() },
        lpszClassName: name,
        ..Default::default()
    };
    unsafe { RegisterClassW(&wc) != 0 }
}

pub fn create_popup(
    class: PCWSTR,
    title: &str,
    w: i32,
    h: i32,
    wndproc: windows::Win32::UI::WindowsAndMessaging::WNDPROC,
) -> Option<HWND> {
    let _ = register_class(class, wndproc);
    let dpi = winutil::screen_dpi();
    let wa = winutil::work_area();
    let cw = scale(w, dpi);
    let ch = scale(h, dpi);
    let x = wa.left + (wa.right - wa.left - cw) / 2;
    let y = wa.top + (wa.bottom - wa.top - ch) / 2;
    let t = wide(title);
    unsafe {
        CreateWindowExW(
            WS_EX_APPWINDOW,
            class,
            PCWSTR(t.as_ptr()),
            WS_POPUP | WS_CLIPCHILDREN | WINDOW_STYLE(WS_VISIBLE.0),
            x,
            y,
            cw,
            ch,
            None,
            None,
            None,
            None,
        )
        .ok()
    }
}

pub fn fill_page(hdc: windows::Win32::Graphics::Gdi::HDC, hwnd: HWND) {
    unsafe {
        let mut rc = RECT::default();
        let _ = GetClientRect(hwnd, &mut rc);
        let brush = solid_brush(COLOR_PAGE);
        let _ = windows::Win32::Graphics::Gdi::FillRect(hdc, &rc, brush);
        delete_gdi(windows::Win32::Graphics::Gdi::HGDIOBJ(brush.0));
        let _ = COLOR_ACCENT;
        let _ = WS_OVERLAPPED;
    }
}

pub fn ctl_color_static(hdc: windows::Win32::Graphics::Gdi::HDC) -> LRESULT {
    unsafe {
        let _ = SetBkMode(hdc, TRANSPARENT);
        let _ = SetTextColor(hdc, COLOR_TEXT_PRIMARY);
    }
    LRESULT(solid_brush(COLOR_PAGE).0 as isize)
}

pub fn ctl_color_edit(hdc: windows::Win32::Graphics::Gdi::HDC) -> LRESULT {
    unsafe {
        let _ = SetBkMode(hdc, TRANSPARENT);
        let _ = SetTextColor(hdc, COLOR_TEXT_PRIMARY);
    }
    LRESULT(solid_brush(winutil::COLOR_CONTROL).0 as isize)
}

pub use winutil::screen_dpi;
