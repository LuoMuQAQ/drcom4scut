//! 安装器/卸载器共用的简洁原生窗口。

use std::path::PathBuf;

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DispatchMessageW, GetMessageW, GetWindowTextW, LoadCursorW, LoadIconW,
    MessageBoxW, RegisterClassW, SendMessageW, SetWindowTextW, TranslateMessage, CS_HREDRAW,
    CS_VREDRAW, HMENU, IDC_ARROW, MB_ICONWARNING, MB_OK, MB_OKCANCEL, MSG, WINDOW_EX_STYLE,
    WINDOW_STYLE, WM_SETFONT, WNDCLASSW, WS_CHILD, WS_CLIPCHILDREN, WS_EX_APPWINDOW, WS_POPUP,
    WS_SYSMENU, WS_TABSTOP, WS_VISIBLE,
};

use crate::ui::winutil::{self, scale, wide};

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

/// Keep the native automatic-checkbox state, keyboard behavior and accessibility,
/// while painting its presentation as a HeroUI v3 switch.
pub fn theme_checkbox(hwnd: HWND, dark: bool, dpi: u32) {
    unsafe {
        let _ = windows::Win32::UI::Shell::SetWindowSubclass(
            hwnd,
            Some(switch_proc),
            31,
            ((dpi as usize) << 1) | dark as usize,
        );
        let _ = windows::Win32::Graphics::Gdi::InvalidateRect(Some(hwnd), None, false);
    }
}

unsafe extern "system" fn switch_proc(
    hwnd: HWND,
    msg: u32,
    wp: WPARAM,
    lp: LPARAM,
    _id: usize,
    data: usize,
) -> windows::Win32::Foundation::LRESULT {
    use crate::ui::hero;
    use windows::Win32::Graphics::Gdi::*;
    use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass};
    use windows::Win32::UI::WindowsAndMessaging::*;
    if msg == WM_ERASEBKGND {
        return windows::Win32::Foundation::LRESULT(1);
    }
    if matches!(msg, WM_PAINT | WM_PRINT | WM_PRINTCLIENT) {
        let mut ps = PAINTSTRUCT::default();
        let dc = if msg == WM_PAINT {
            BeginPaint(hwnd, &mut ps)
        } else {
            HDC(wp.0 as *mut _)
        };
        let dpi = (data >> 1) as u32;
        let p = winutil::Palette::for_dark(data & 1 != 0);
        let mut r = windows::Win32::Foundation::RECT::default();
        let _ = GetClientRect(hwnd, &mut r);
        winutil::paint_buffered(dc, r, |dc| {
            hero::line(dc, r, p.page);
            let font = HFONT(SendMessageW(hwnd, WM_GETFONT, None, None).0 as *mut _);
            let mut label = r;
            label.right -= scale(60, dpi);
            hero::text(
                dc,
                font,
                p.text_primary,
                label,
                &edit_text(hwnd),
                DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS,
            );
            let x = (r.right * 96 / dpi as i32) - 46;
            let y = ((r.bottom * 96 / dpi as i32) - 20) / 2;
            hero::switch(dc, x, y, dpi, p, if is_checked(hwnd) { 1.0 } else { 0.0 });
        });
        if msg == WM_PAINT {
            let _ = EndPaint(hwnd, &ps);
        }
        return windows::Win32::Foundation::LRESULT(0);
    }
    if msg == WM_NCDESTROY {
        let _ = RemoveWindowSubclass(hwnd, Some(switch_proc), 31);
    }
    let result = DefSubclassProc(hwnd, msg, wp, lp);
    if matches!(
        msg,
        WM_SETFOCUS | WM_KILLFOCUS | WM_LBUTTONUP | WM_KEYUP | WM_ENABLE | 0x00f1 | 0x00f5
    ) {
        let _ = InvalidateRect(Some(hwnd), None, false);
    }
    result
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

/// 居中弹出无边框窗口，配色与主界面一致。
pub fn register_class(
    name: PCWSTR,
    wndproc: windows::Win32::UI::WindowsAndMessaging::WNDPROC,
) -> bool {
    // Icon resource ID 1 is embedded into every binary by build.rs (windres).
    let icon = unsafe {
        let instance = GetModuleHandleW(None).unwrap_or_default();
        LoadIconW(Some(instance.into()), PCWSTR(1 as *const u16)).unwrap_or_default()
    };
    let wc = WNDCLASSW {
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: wndproc,
        hCursor: unsafe { LoadCursorW(None, IDC_ARROW).unwrap_or_default() },
        hIcon: icon,
        lpszClassName: name,
        ..Default::default()
    };
    unsafe { RegisterClassW(&wc) != 0 }
}

/// Ask DWM for anti-aliased rounded corners (Windows 11+; silently ignored
/// elsewhere, falling back to square corners). Same approach as the main
/// window; replaces the jagged SetWindowRgn hard clipping.
pub fn round_corners(hwnd: HWND) {
    #[link(name = "dwmapi")]
    extern "system" {
        fn DwmSetWindowAttribute(
            hwnd: HWND,
            attr: u32,
            value: *const core::ffi::c_void,
            size: u32,
        ) -> i32;
    }
    let pref: i32 = 2; // DWMWCP_ROUND
    unsafe {
        let _ = DwmSetWindowAttribute(hwnd, 33, (&pref as *const i32).cast(), 4);
    }
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
            WS_POPUP | WS_SYSMENU | WS_CLIPCHILDREN | WINDOW_STYLE(WS_VISIBLE.0),
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

pub use winutil::screen_dpi;
