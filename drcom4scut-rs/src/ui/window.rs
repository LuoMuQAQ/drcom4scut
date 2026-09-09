//! 主窗口：共享布局尺寸，无边框标题栏 + 账号区 + 连接编排。
//! 行为对齐 .NET 版 v3.3.0 `MainWindow.xaml` / `MainWindow.xaml.cs`。

use std::path::PathBuf;
use std::time::Instant;

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, ClientToScreen, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, Ellipse,
    EndPaint, FillRect, GetTextExtentPoint32W, LineTo, MoveToEx, SelectObject, SetBkColor,
    SetBkMode, SetTextColor, TextOutW, HDC, PAINTSTRUCT, SRCCOPY, TRANSPARENT,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, VK_ESCAPE};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetClientRect, GetWindowLongPtrW,
    GetWindowTextW, KillTimer, LoadCursorW, LoadImageW, MessageBoxW, PostQuitMessage,
    RegisterClassW, SendMessageW, SetCursor, SetForegroundWindow, SetTimer, SetWindowLongPtrW,
    SetWindowTextW, ShowWindow, CS_HREDRAW, CS_VREDRAW, GWLP_USERDATA, HMENU, IDC_ARROW, IDC_HAND,
    IDYES, IMAGE_ICON, LR_LOADFROMFILE, MB_ICONINFORMATION, MB_ICONWARNING, MB_OK, MB_YESNO,
    SW_HIDE, SW_MINIMIZE, SW_RESTORE, SW_SHOW, SW_SHOWNA, WINDOW_EX_STYLE, WINDOW_STYLE,
    WM_ACTIVATE, WM_CLOSE, WM_COMMAND, WM_CREATE, WM_CTLCOLOREDIT, WM_CTLCOLORLISTBOX,
    WM_CTLCOLORSTATIC, WM_DESTROY, WM_ERASEBKGND, WM_KEYDOWN, WM_LBUTTONDOWN, WM_MOUSEMOVE,
    WM_MOUSEWHEEL, WM_NCLBUTTONDOWN, WM_PAINT, WM_SETFONT, WM_TIMER, WNDCLASSW, WS_CHILD,
    WS_CLIPCHILDREN, WS_EX_APPWINDOW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_POPUP, WS_SYSMENU,
    WS_TABSTOP, WS_VISIBLE,
};

use crate::controller::ReconnectBackoff;
use crate::coreproc::{self, OwnedCore};
use crate::health::HealthMonitor;
use crate::logtail::LogTail;
use crate::model::{Adapter, LinkState, Settings};
use crate::platform::{self, NpcapStatus};
use crate::{adapters, paths, settings};

use super::anim::{bool01, lerp_color, Anim};
use super::layout;
#[cfg(test)]
#[path = "animation_render_tests.rs"]
mod animation_render_tests;
#[path = "display.rs"]
mod display;
use super::tray::{HIconOrFile, Tray};
use super::winutil::{
    self, brush_as_gdi, create_font, delete_gdi, destroy_icon, font_as_gdi, solid_brush, wide,
    COLOR_ACCENT, COLOR_CARD, COLOR_DANGER, COLOR_PAGE, COLOR_TEXT_PRIMARY, COLOR_TEXT_SECONDARY,
};

pub const WM_APP_OPEN: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 10;
pub const WM_APP_CONNECT: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 11;
pub const WM_APP_DISCONNECT: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 12;
pub const WM_APP_EXIT: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 13;
pub const WM_APP_FIELD_CLICK: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 14;

const TIMER_POLL: usize = 1;
const TIMER_MS: u32 = 2000;
const TIMER_ANIM: usize = 2;
// WM_TIMER is quantized to the system clock. 16 ms can become two 15.6 ms
// ticks; requesting its supported 10 ms minimum avoids an accidental 32 Hz cap.
const ANIM_MS: u32 = 10;
const TOGGLE_ANIM_MS: u32 = 200;
const COMBO_OPEN_MS: u32 = 180;
const COMBO_CLOSE_MS: u32 = 140;
const CLIENT_W: i32 = layout::WIDTH;
const CLIENT_H: i32 = layout::HEIGHT;
const TITLE_H: i32 = layout::TITLE_HEIGHT;

const ID_USER: isize = 1001;
const ID_PASS: isize = 1002;

const ES_AUTOHSCROLL: i32 = 0x0080;
const ES_PASSWORD: i32 = 0x0020;
const ID_EYE: isize = 1009;
const EM_SETPASSWORDCHAR: u32 = 0x00CC;
const HTCAPTION: usize = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Hit {
    None,
    Drag,
    Min,
    Close,
    Connect,
    Eye,
    Auto,
    Remember,
    Startup,
    TrayKeep,
    Combo,
}

struct App {
    preview: bool,
    settings: Settings,
    adapters: Vec<Adapter>,
    link_state: LinkState,
    status_title: String,
    status_detail: String,
    hwnd_user: HWND,
    hwnd_pass: HWND,
    hwnd_eye: HWND,
    hwnd_eye_tip: HWND,
    eye_rect: RECT,
    combo_sel: i32,
    combo_open: bool,
    combo_scroll: i32,
    combo_hot: i32,
    combo_visual: Anim,
    toggle_anim: [Anim; 4],
    anim_timer_running: bool,
    popup_surface: Option<PopupSurface>,
    hwnd_popup: HWND,
    core_path: Option<PathBuf>,
    core: Option<OwnedCore>,
    tray: Option<Tray>,
    backoff: ReconnectBackoff,
    desired_running: bool,
    manual_pause: bool,
    starting: bool,
    exiting: bool,
    suppress_exit_failure: bool,
    connect_enabled: bool,
    pass_revealed: bool,
    health: Option<HealthMonitor>,
    log_tail: LogTail,
    page_brush: windows::Win32::Graphics::Gdi::HBRUSH,
    card_brush: windows::Win32::Graphics::Gdi::HBRUSH,
    control_brush: windows::Win32::Graphics::Gdi::HBRUSH,
    font: windows::Win32::Graphics::Gdi::HFONT,
    font_title: windows::Win32::Graphics::Gdi::HFONT,
    font_label: windows::Win32::Graphics::Gdi::HFONT,
    font_btn: windows::Win32::Graphics::Gdi::HFONT,
    icon_small: windows::Win32::UI::WindowsAndMessaging::HICON,
    icon_title: windows::Win32::UI::WindowsAndMessaging::HICON,
    icon_status: windows::Win32::UI::WindowsAndMessaging::HICON,
    logo_title: Option<winutil::SvgBmp>,
    logo_status: Option<winutil::SvgBmp>,
    eye_on: Option<winutil::SvgBmp>,
    eye_off: Option<winutil::SvgBmp>,
    hover: Hit,
    dpi: u32,
    preview_dpi: Option<u32>,
    layout_pending: bool,
    layout_in_progress: bool,
}

impl App {
    fn s(&self, px: i32) -> i32 {
        winutil::scale(px, self.dpi.max(1))
    }

    fn new() -> Self {
        let null = HWND(std::ptr::null_mut());
        let icon0 = windows::Win32::UI::WindowsAndMessaging::HICON(std::ptr::null_mut());
        let preview = std::env::args().any(|a| a == "--ui-preview");
        let preview_dpi = if preview {
            std::env::args()
                .find_map(|a| {
                    a.strip_prefix("--ui-dpi=")
                        .and_then(|d| d.parse::<u32>().ok())
                })
                .filter(|d| (96..=288).contains(d))
        } else {
            None
        };
        let dpi = preview_dpi.unwrap_or_else(winutil::screen_dpi);
        Self {
            preview,
            settings: Settings::default(),
            adapters: Vec::new(),
            link_state: LinkState::Offline,
            status_title: "未连接".into(),
            status_detail: "填写账号后点击连接".into(),
            hwnd_user: null,
            hwnd_pass: null,
            hwnd_eye: null,
            hwnd_eye_tip: null,
            eye_rect: RECT::default(),
            combo_sel: 0,
            combo_open: false,
            combo_scroll: 0,
            combo_hot: -1,
            combo_visual: Anim::snap(0.0),
            toggle_anim: [Anim::snap(0.0); 4],
            anim_timer_running: false,
            popup_surface: None,
            hwnd_popup: null,
            core_path: None,
            core: None,
            tray: None,
            backoff: ReconnectBackoff::new(),
            desired_running: false,
            manual_pause: true,
            starting: false,
            exiting: false,
            suppress_exit_failure: false,
            connect_enabled: true,
            pass_revealed: false,
            health: None,
            log_tail: LogTail::default(),
            page_brush: solid_brush(COLOR_PAGE),
            card_brush: solid_brush(COLOR_CARD),
            control_brush: solid_brush(winutil::COLOR_CONTROL),
            font: create_font(14, dpi, false),
            font_title: create_font(20, dpi, true),
            font_label: create_font(12, dpi, false),
            font_btn: create_font(14, dpi, true),
            icon_small: icon0,
            icon_title: icon0,
            icon_status: icon0,
            logo_title: None,
            logo_status: None,
            eye_on: None,
            eye_off: None,
            hover: Hit::None,
            dpi,
            preview_dpi,
            layout_pending: false,
            layout_in_progress: false,
        }
    }
}

impl Drop for App {
    fn drop(&mut self) {
        self.core.take();
        self.tray.take();
        if !self.hwnd_popup.0.is_null() {
            unsafe {
                let _ = DestroyWindow(self.hwnd_popup);
            }
            self.hwnd_popup = HWND(std::ptr::null_mut());
        }
        delete_gdi(brush_as_gdi(self.page_brush));
        delete_gdi(brush_as_gdi(self.card_brush));
        delete_gdi(brush_as_gdi(self.control_brush));
        delete_gdi(font_as_gdi(self.font));
        delete_gdi(font_as_gdi(self.font_title));
        delete_gdi(font_as_gdi(self.font_label));
        delete_gdi(font_as_gdi(self.font_btn));
        if !self.icon_small.0.is_null() {
            destroy_icon(self.icon_small);
        }
        if !self.icon_title.0.is_null() {
            destroy_icon(self.icon_title);
        }
        if !self.icon_status.0.is_null() {
            destroy_icon(self.icon_status);
        }
    }
}

pub fn create_main_window() -> Option<HWND> {
    unsafe {
        let hinstance = windows::Win32::System::LibraryLoader::GetModuleHandleW(None).ok()?;
        let class_name = w!("DrcomMainWnd");
        static REGISTERED: std::sync::Once = std::sync::Once::new();
        REGISTERED.call_once(|| {
            let wc = WNDCLASSW {
                lpfnWndProc: Some(wndproc),
                hInstance: hinstance.into(),
                lpszClassName: class_name,
                style: CS_HREDRAW | CS_VREDRAW,
                hbrBackground: solid_brush(COLOR_PAGE),
                ..Default::default()
            };
            let _ = RegisterClassW(&wc);
        });

        let app_box = Box::new(App::new());
        let ww = app_box.s(CLIENT_W);
        let hh = app_box.s(CLIENT_H);
        let wa = winutil::work_area();
        let x = wa.left + (wa.right - wa.left - ww).max(0) / 2;
        let y = wa.top + (wa.bottom - wa.top - hh).max(0) / 2;
        let app = Box::into_raw(app_box);

        let hwnd = CreateWindowExW(
            WS_EX_APPWINDOW,
            class_name,
            w!("校园网"),
            WS_POPUP | WS_SYSMENU | WS_CLIPCHILDREN,
            x,
            y,
            ww,
            hh,
            None,
            None,
            Some(hinstance.into()),
            Some(app as *const core::ffi::c_void),
        );
        let hwnd = match hwnd {
            Ok(h) => h,
            Err(_) => {
                drop(Box::from_raw(app));
                return None;
            }
        };
        display::refresh(hwnd);
        initialize(hwnd);
        round_corners(hwnd);
        if !platform::is_autostart_launch() {
            let _ = ShowWindow(hwnd, SW_SHOW);
        }
        Some(hwnd)
    }
}

unsafe fn round_corners(hwnd: HWND) {
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
    let _ = DwmSetWindowAttribute(hwnd, 33, (&pref as *const i32).cast(), 4);
}

unsafe fn strip_theme(hwnd: HWND) {
    #[link(name = "uxtheme")]
    extern "system" {
        fn SetWindowTheme(hwnd: HWND, app: PCWSTR, idlist: PCWSTR) -> i32;
    }
    let empty = w!("");
    let _ = SetWindowTheme(hwnd, empty, empty);
}

unsafe fn app_mut(hwnd: HWND) -> Option<&'static mut App> {
    let p = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut App;
    if p.is_null() {
        None
    } else {
        Some(&mut *p)
    }
}

unsafe fn create_child(
    parent: HWND,
    class: PCWSTR,
    text: PCWSTR,
    style: WINDOW_STYLE,
    ex: WINDOW_EX_STYLE,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    id: isize,
) -> HWND {
    let inst = windows::Win32::System::LibraryLoader::GetModuleHandleW(None).ok();
    CreateWindowExW(
        ex,
        class,
        text,
        style,
        x,
        y,
        w,
        h,
        Some(parent),
        Some(HMENU(id as *mut core::ffi::c_void)),
        inst.map(|h| h.into()),
        None,
    )
    .unwrap_or(HWND(std::ptr::null_mut()))
}

fn child_style(extra: u32) -> WINDOW_STYLE {
    WINDOW_STYLE(WS_CHILD.0 | WS_VISIBLE.0 | WS_TABSTOP.0 | extra)
}

unsafe fn set_font(hwnd: HWND, font: windows::Win32::Graphics::Gdi::HFONT) {
    if hwnd.0.is_null() {
        return;
    }
    let _ = SendMessageW(
        hwnd,
        WM_SETFONT,
        Some(WPARAM(font.0 as usize)),
        Some(LPARAM(1)),
    );
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_CREATE => {
            let cs = &*(lparam.0 as *const windows::Win32::UI::WindowsAndMessaging::CREATESTRUCTW);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, cs.lpCreateParams as isize);
            create_controls(hwnd);
            let _ = SetTimer(Some(hwnd), TIMER_POLL, TIMER_MS, None);
            LRESULT(0)
        }
        WM_DESTROY => {
            let p = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut App;
            if !p.is_null() {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                drop(Box::from_raw(p));
            }
            PostQuitMessage(0);
            LRESULT(0)
        }
        windows::Win32::UI::WindowsAndMessaging::WM_DPICHANGED => {
            if lparam.0 != 0 {
                display::change(hwnd, (wparam.0 & 0xffff) as u32, *(lparam.0 as *const RECT));
            }
            LRESULT(0)
        }
        windows::Win32::UI::WindowsAndMessaging::WM_DISPLAYCHANGE
        | windows::Win32::UI::WindowsAndMessaging::WM_SETTINGCHANGE
        | windows::Win32::UI::WindowsAndMessaging::WM_EXITSIZEMOVE => {
            display::schedule(hwnd);
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        windows::Win32::UI::WindowsAndMessaging::WM_SIZE => {
            if wparam.0 != windows::Win32::UI::WindowsAndMessaging::SIZE_MINIMIZED as usize {
                display::schedule(hwnd);
            }
            LRESULT(0)
        }
        display::WM_APP_LAYOUT => {
            display::refresh(hwnd);
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        WM_APP_FIELD_CLICK => {
            close_combo(hwnd);
            LRESULT(0)
        }
        WM_COMMAND if wparam.0 == ID_EYE as usize => {
            close_combo(hwnd);
            toggle_password_reveal(hwnd);
            LRESULT(0)
        }
        WM_COMMAND if matches!(wparam.0 >> 16, 0x100 | 0x200) => {
            if wparam.0 >> 16 == 0x100 {
                close_combo(hwnd);
            }
            invalidate(hwnd);
            LRESULT(0)
        }
        windows::Win32::UI::WindowsAndMessaging::WM_DRAWITEM => {
            let di = &*(lparam.0 as *const windows::Win32::UI::Controls::DRAWITEMSTRUCT);
            if di.CtlID == ID_EYE as u32 {
                if let Some(app) = app_mut(hwnd) {
                    let _ = FillRect(di.hDC, &di.rcItem, app.control_brush);
                    if di.itemState.0 & 0x10 != 0 {
                        winutil::fill_component(
                            di.hDC,
                            di.rcItem,
                            app.dpi,
                            winutil::COLOR_CONTROL,
                            Some(COLOR_ACCENT),
                        );
                    }
                    let bmp = if app.pass_revealed {
                        &app.eye_off
                    } else {
                        &app.eye_on
                    };
                    if let Some(bmp) = bmp {
                        let size = app.s(20);
                        winutil::blit_svg(
                            di.hDC,
                            bmp,
                            (di.rcItem.right - size) / 2,
                            (di.rcItem.bottom - size) / 2,
                            size,
                            size,
                        );
                    }
                }
                return LRESULT(1);
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let hdc = BeginPaint(hwnd, &mut ps);
            paint(hwnd, hdc, ps.rcPaint);
            let _ = EndPaint(hwnd, &ps);
            LRESULT(0)
        }
        WM_CTLCOLOREDIT | WM_CTLCOLORLISTBOX | WM_CTLCOLORSTATIC => {
            if let Some(app) = app_mut(hwnd) {
                let hdc = HDC(wparam.0 as *mut _);
                let _ = SetBkColor(hdc, winutil::COLOR_CONTROL);
                let _ = SetTextColor(hdc, COLOR_TEXT_PRIMARY);
                let _ = SetBkMode(hdc, windows::Win32::Graphics::Gdi::OPAQUE);
                return LRESULT(app.control_brush.0 as isize);
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_ACTIVATE => {
            if (wparam.0 & 0xFFFF) == 0 {
                dismiss_combo(hwnd);
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_LBUTTONDOWN => {
            let y = ((lparam.0 as u32) >> 16) as i16 as i32;
            let x = (lparam.0 as u32 & 0xFFFF) as i16 as i32;
            let hit = app_mut(hwnd)
                .map(|a| hit_test(a, x, y))
                .unwrap_or(Hit::None);
            if !matches!(hit, Hit::Combo) {
                close_combo(hwnd);
            }
            match hit {
                Hit::Drag => {
                    let _ = ReleaseCapture();
                    let _ = SendMessageW(
                        hwnd,
                        WM_NCLBUTTONDOWN,
                        Some(WPARAM(HTCAPTION)),
                        Some(LPARAM(0)),
                    );
                }
                Hit::Min => {
                    dismiss_combo(hwnd);
                    let _ = ShowWindow(hwnd, SW_MINIMIZE);
                }
                Hit::Close => request_close(hwnd),
                Hit::Connect => on_action(hwnd),
                Hit::Eye => toggle_password_reveal(hwnd),
                Hit::Combo => toggle_combo(hwnd),

                Hit::Auto => toggle_flag(hwnd, 0, |s| {
                    s.auto_login = !s.auto_login;
                }),
                Hit::Remember => toggle_flag(hwnd, 1, |s| {
                    s.remember_password = !s.remember_password;
                }),
                Hit::Startup => toggle_flag(hwnd, 2, |s| {
                    s.start_with_windows = !s.start_with_windows;
                }),
                Hit::TrayKeep => toggle_flag(hwnd, 3, |s| {
                    s.minimize_to_tray = !s.minimize_to_tray;
                }),
                Hit::None => {}
            }
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            let y = ((lparam.0 as u32) >> 16) as i16 as i32;
            let x = (lparam.0 as u32 & 0xFFFF) as i16 as i32;
            if let Some(app) = app_mut(hwnd) {
                let h = hit_test(app, x, y);
                if h != app.hover {
                    let previous = app.hover;
                    app.hover = h;
                    // Only caption/action buttons paint a hover state. Crossing
                    // a toggle must not queue an expensive full-window redraw.
                    invalidate_hover(hwnd, previous);
                    invalidate_hover(hwnd, h);
                }
                let cursor = if matches!(
                    h,
                    Hit::Min
                        | Hit::Close
                        | Hit::Connect
                        | Hit::Eye
                        | Hit::Auto
                        | Hit::Remember
                        | Hit::Startup
                        | Hit::TrayKeep
                        | Hit::Combo
                ) {
                    IDC_HAND
                } else {
                    IDC_ARROW
                };
                if let Ok(c) = LoadCursorW(None, cursor) {
                    let _ = SetCursor(Some(c));
                }
            }
            LRESULT(0)
        }
        WM_KEYDOWN => {
            if wparam.0 as u16 == VK_ESCAPE.0 {
                if app_mut(hwnd).is_some_and(|app| app.combo_open) {
                    close_combo(hwnd);
                    return LRESULT(0);
                }
                dismiss_combo(hwnd);
                let _ = ShowWindow(hwnd, SW_HIDE);
                return LRESULT(0);
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_CLOSE => {
            request_close(hwnd);
            LRESULT(0)
        }
        WM_TIMER => {
            if wparam.0 == TIMER_POLL {
                on_timer(hwnd);
            } else if wparam.0 == TIMER_ANIM {
                on_anim_timer(hwnd);
            }
            LRESULT(0)
        }
        WM_APP_OPEN => {
            let _ = ShowWindow(hwnd, SW_RESTORE);
            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = SetForegroundWindow(hwnd);
            LRESULT(0)
        }
        WM_APP_CONNECT => {
            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = SetForegroundWindow(hwnd);
            start_connect(hwnd);
            LRESULT(0)
        }
        WM_APP_DISCONNECT => {
            start_disconnect(hwnd);
            LRESULT(0)
        }
        WM_APP_EXIT => {
            begin_exit(hwnd);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

fn hit_test(app: &App, x: i32, y: i32) -> Hit {
    let s = |v: i32| app.s(v);
    let w = s(CLIENT_W);
    if y >= 0 && y < s(TITLE_H) {
        if x >= w - s(46) {
            return Hit::Close;
        }
        if x >= w - s(92) {
            return Hit::Min;
        }
        return Hit::Drag;
    }
    if app.combo_open {
        // 列表在独立弹出窗口中，主窗口不再命中条目。
    }
    if x >= s(36)
        && x <= s(384)
        && y >= s(layout::COMBO_TOP)
        && y <= s(layout::COMBO_TOP + layout::FIELD_HEIGHT)
    {
        return Hit::Combo;
    }
    if x >= app.eye_rect.left
        && x < app.eye_rect.right
        && y >= app.eye_rect.top
        && y < app.eye_rect.bottom
    {
        return Hit::Eye;
    }
    if x >= s(20) && x <= s(400) && y >= s(layout::ACTION_TOP) && y <= s(layout::ACTION_BOTTOM) {
        return Hit::Connect;
    }
    if y >= s(layout::TOGGLE_TOP - 2) && y < s(layout::TOGGLE_TOP + 26) && x >= s(36) && x <= s(384)
    {
        return if x < s(210) { Hit::Auto } else { Hit::Remember };
    }
    if y >= s(layout::TOGGLE_SECOND_TOP - 2)
        && y < s(layout::TOGGLE_SECOND_TOP + 26)
        && x >= s(36)
        && x <= s(384)
    {
        return if x < s(210) {
            Hit::Startup
        } else {
            Hit::TrayKeep
        };
    }
    Hit::None
}

unsafe fn toggle_combo(hwnd: HWND) {
    let Some(app) = app_mut(hwnd) else {
        return;
    };
    if app.combo_open {
        close_combo(hwnd);
    } else {
        app.combo_open = true;
        let from = app.combo_visual.value();
        app.combo_visual = Anim::snap(from);
        if from == 0.0 {
            app.combo_scroll = 0;
            app.combo_hot = app.combo_sel;
        }
        show_combo_popup(hwnd);
        // First-time shadow rasterization must not consume the opening tween.
        if let Some(app) = app_mut(hwnd) {
            if app.combo_open {
                app.combo_visual = Anim::go(from, 1.0, COMBO_OPEN_MS);
            }
        }
        start_anim_timer(hwnd);
        invalidate_combo(hwnd);
    }
}

unsafe fn close_combo(hwnd: HWND) {
    let Some(app) = app_mut(hwnd) else {
        return;
    };
    // Focus notifications can repeat while closing. Never restart the fade.
    if !app.combo_open {
        return;
    }
    app.combo_open = false;
    app.combo_visual = Anim::go(app.combo_visual.value(), 0.0, COMBO_CLOSE_MS);
    if app.combo_visual.done() && !app.hwnd_popup.0.is_null() {
        let _ = ShowWindow(app.hwnd_popup, SW_HIDE);
    }
    start_anim_timer(hwnd);
    invalidate_combo(hwnd);
}

/// Stop popup presentation before hiding/minimizing the owner. A queued frame
/// must not show an owned popup again after its owner disappeared.
unsafe fn dismiss_combo(hwnd: HWND) {
    if let Some(app) = app_mut(hwnd) {
        app.combo_open = false;
        app.combo_visual = Anim::snap(0.0);
        if !app.hwnd_popup.0.is_null() {
            let _ = ShowWindow(app.hwnd_popup, SW_HIDE);
        }
    }
    invalidate_combo(hwnd);
}

unsafe fn show_combo_popup(main: HWND) {
    if !app_mut(main).is_some_and(|app| app.combo_open || !app.combo_visual.done()) {
        return;
    }
    let popup = ensure_combo_popup(main);
    if popup.0.is_null() {
        dismiss_combo(main);
        return;
    }
    let Some(app) = app_mut(main) else {
        return;
    };
    let s = |v: i32| app.s(v);
    let n = app.adapters.len() as i32 + 1;
    let item_h = s(40);
    let pad = s(8);
    let shadow = s(10);
    let content_h = pad * 2 + n * item_h;
    let card_w = s(348);
    let gap = s(4);

    let mut tl = POINT {
        x: s(36),
        y: s(layout::COMBO_TOP),
    };
    let mut br = POINT {
        x: s(36) + card_w,
        y: s(layout::COMBO_TOP + layout::FIELD_HEIGHT),
    };
    let _ = ClientToScreen(main, &mut tl);
    let _ = ClientToScreen(main, &mut br);
    let wa = winutil::monitor_work_area(main);
    let space_below = (wa.bottom - br.y - gap - shadow * 2).max(0);
    let space_above = (tl.y - wa.top - gap - shadow * 2).max(0);
    let min_h = item_h + pad * 2;
    let (card_x, card_y, card_h, opens_down) = if content_h <= space_below {
        (tl.x, br.y + gap, content_h, true)
    } else if content_h <= space_above {
        (tl.x, tl.y - gap - content_h, content_h, false)
    } else if space_below >= space_above {
        (tl.x, br.y + gap, space_below.max(min_h), true)
    } else {
        let h = space_above.max(min_h);
        (tl.x, tl.y - gap - h, h, false)
    };
    let t = app.combo_visual.value();
    let slide = ((1.0 - t) * s(8) as f32).round() as i32;
    let card_y = if opens_down {
        card_y - slide
    } else {
        card_y + slide
    };
    let pw = (card_w + shadow * 2).min(wa.right - wa.left);
    let ph = (card_h + shadow * 2).min(wa.bottom - wa.top);
    let px = (card_x - shadow).clamp(wa.left, wa.right - pw);
    let py = (card_y - shadow).clamp(wa.top, wa.bottom - ph);
    // Update position, size and alpha in one compositor submission. Moving the
    // HWND first exposes its old bitmap and causes a second paint per frame.
    paint_combo_popup(popup, main, POINT { x: px, y: py }, pw, ph);
    if !windows::Win32::UI::WindowsAndMessaging::IsWindowVisible(popup).as_bool() {
        let _ = ShowWindow(popup, SW_SHOWNA);
    }
}

unsafe fn ensure_combo_popup(main: HWND) -> HWND {
    let hinstance = windows::Win32::System::LibraryLoader::GetModuleHandleW(None).ok();
    let class_name = w!("DrcomComboPopup");
    static REGISTERED: std::sync::Once = std::sync::Once::new();
    REGISTERED.call_once(|| {
        if let Some(inst) = hinstance {
            let wc = WNDCLASSW {
                lpfnWndProc: Some(combo_popup_wndproc),
                hInstance: inst.into(),
                lpszClassName: class_name,
                style: CS_HREDRAW | CS_VREDRAW,
                ..Default::default()
            };
            let _ = RegisterClassW(&wc);
        }
    });
    let existing = app_mut(main)
        .map(|a| a.hwnd_popup)
        .unwrap_or(HWND(std::ptr::null_mut()));
    if !existing.0.is_null() {
        return existing;
    }
    let popup = CreateWindowExW(
        WINDOW_EX_STYLE(
            WS_EX_TOOLWINDOW.0
                | WS_EX_NOACTIVATE.0
                | windows::Win32::UI::WindowsAndMessaging::WS_EX_LAYERED.0,
        ),
        class_name,
        w!(""),
        WS_POPUP,
        0,
        0,
        0,
        0,
        Some(main),
        None,
        hinstance.map(|h| h.into()),
        None,
    )
    .unwrap_or(HWND(std::ptr::null_mut()));
    SetWindowLongPtrW(popup, GWLP_USERDATA, main.0 as isize);
    if let Some(app) = app_mut(main) {
        app.hwnd_popup = popup;
    }
    popup
}

unsafe extern "system" fn combo_popup_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let main = HWND(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut _);
    match msg {
        WM_ERASEBKGND => LRESULT(1),
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let _ = BeginPaint(hwnd, &mut ps);
            if !main.0.is_null() {
                show_combo_popup(main);
            }
            let _ = EndPaint(hwnd, &ps);
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            let y = ((lparam.0 as u32) >> 16) as i16 as i32;
            let x = (lparam.0 as u32 & 0xffff) as i16 as i32;
            if let Some(i) = popup_item_at(main, hwnd, x, y) {
                if let Some(app) = app_mut(main) {
                    app.combo_sel = i;
                }
                close_combo(main);
            }
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            let y = ((lparam.0 as u32) >> 16) as i16 as i32;
            let x = (lparam.0 as u32 & 0xffff) as i16 as i32;
            if let Some(app) = app_mut(main) {
                let hot = popup_item_at(main, hwnd, x, y).unwrap_or(-1);
                if hot != app.combo_hot {
                    app.combo_hot = hot;
                    let _ = windows::Win32::Graphics::Gdi::InvalidateRect(Some(hwnd), None, false);
                }
            }
            LRESULT(0)
        }
        WM_MOUSEWHEEL => {
            let delta = ((wparam.0 >> 16) as u16) as i16 as i32;
            if let Some(app) = app_mut(main) {
                let step = app.s(40);
                app.combo_scroll = (app.combo_scroll - delta / 120 * step).max(0);
                let max_s = combo_max_scroll(app, hwnd);
                if app.combo_scroll > max_s {
                    app.combo_scroll = max_s;
                }
                let _ = windows::Win32::Graphics::Gdi::InvalidateRect(Some(hwnd), None, false);
            }
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

fn combo_max_scroll(app: &App, popup: HWND) -> i32 {
    let s = |v: i32| app.s(v);
    let n = app.adapters.len() as i32 + 1;
    let shadow = s(10);
    let content = s(8) * 2 + n * s(40);
    let mut rc = RECT::default();
    unsafe {
        let _ = GetClientRect(popup, &mut rc);
    }
    let inner = (rc.bottom - shadow * 2).max(0);
    (content - inner).max(0)
}

fn popup_item_at(main: HWND, popup: HWND, x: i32, y: i32) -> Option<i32> {
    let app = unsafe { app_mut(main)? };
    if !app.combo_open {
        return None;
    }
    let s = |v: i32| app.s(v);
    let shadow = s(10);
    let pad = s(8);
    let item_h = s(40);
    let n = app.adapters.len() as i32 + 1;
    let mut rc = RECT::default();
    unsafe {
        let _ = GetClientRect(popup, &mut rc);
    }
    if x < shadow + pad
        || x >= rc.right - shadow - pad
        || y < shadow + pad
        || y >= rc.bottom - shadow - pad
    {
        return None;
    }
    let y = y - shadow + app.combo_scroll;
    if y < pad {
        return None;
    }
    let i = (y - pad) / item_h;
    if i >= 0 && i < n {
        Some(i)
    } else {
        None
    }
}

/// Cached premultiplied menu surface. Shadow rasterization is independent of
/// animation; text/highlights are repainted only when their content changes.
struct PopupSurface {
    bmp: winutil::SvgBmp,
    background: Vec<u8>,
    content: Option<(i32, i32, i32)>,
}

impl PopupSurface {
    fn new(app: &App, w: i32, h: i32) -> Option<Self> {
        let edge = app.s(10);
        let blur = app.s(3);
        let cw = w - edge * 2;
        let ch = h - edge * 2;
        let radius = winutil::component_radius(ch, app.dpi);
        let svg = format!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}">
          <defs><filter id="shadow" x="-50%" y="-50%" width="200%" height="200%" color-interpolation-filters="sRGB"><feGaussianBlur stdDeviation="{blur}"/></filter></defs>
          <rect x="{edge}" y="{}" width="{cw}" height="{ch}" rx="{radius}" fill="#000" opacity=".12" filter="url(#shadow)"/>
          <rect x="{}" y="{}" width="{}" height="{}" rx="{radius}" fill="#fff" stroke="#dedede" stroke-width="1"/>
          </svg>"##,
            edge + app.s(2),
            edge as f32 + 0.5,
            edge as f32 + 0.5,
            cw - 1,
            ch - 1
        );
        let bmp = winutil::rasterize_svg(svg.as_bytes(), w.max(h) as u32)?;
        let background =
            unsafe { std::slice::from_raw_parts(bmp.bits, (w * h * 4) as usize).to_vec() };
        Some(Self {
            bmp,
            background,
            content: None,
        })
    }

    unsafe fn update(&mut self, app: &App, hdc: HDC) {
        use windows::Win32::Graphics::Gdi::HGDIOBJ;
        let key = (app.combo_sel, app.combo_hot, app.combo_scroll);
        if self.content == Some(key) {
            return;
        }
        let pixels = std::slice::from_raw_parts_mut(self.bmp.bits, self.background.len());
        pixels.copy_from_slice(&self.background);
        let mem = CreateCompatibleDC(Some(hdc));
        let old = SelectObject(mem, HGDIOBJ(self.bmp.hbmp.0));
        paint_combo_popup_ui(app, mem, self.bmp.w, self.bmp.h);
        let _ = windows::Win32::Graphics::Gdi::GdiFlush();
        // GDI clears alpha when drawing text. The content clip lies entirely
        // inside the opaque card, so restoring its original alpha is sufficient.
        for (p, bg) in pixels
            .chunks_exact_mut(4)
            .zip(self.background.chunks_exact(4))
        {
            p[3] = bg[3];
        }
        let _ = SelectObject(mem, old);
        let _ = DeleteDC(mem);
        self.content = Some(key);
    }
}

unsafe fn paint_combo_popup(popup: HWND, main: HWND, position: POINT, w: i32, h: i32) {
    use windows::Win32::Graphics::Gdi::{AC_SRC_ALPHA, AC_SRC_OVER, BLENDFUNCTION, HGDIOBJ};
    use windows::Win32::UI::WindowsAndMessaging::{UpdateLayeredWindow, ULW_ALPHA};
    let Some(app) = app_mut(main) else {
        return;
    };
    if w <= 0 || h <= 0 {
        return;
    }
    let mut surface = match app.popup_surface.take() {
        Some(surface) if surface.bmp.w == w && surface.bmp.h == h => surface,
        _ => match PopupSurface::new(app, w, h) {
            Some(surface) => surface,
            None => return,
        },
    };
    let dc = windows::Win32::Graphics::Gdi::GetDC(None);
    surface.update(app, dc);
    let mem = CreateCompatibleDC(Some(dc));
    let old = SelectObject(mem, HGDIOBJ(surface.bmp.hbmp.0));
    let blend = BLENDFUNCTION {
        BlendOp: AC_SRC_OVER as u8,
        BlendFlags: 0,
        SourceConstantAlpha: (app.combo_visual.value() * 255.0).round().clamp(0.0, 255.0) as u8,
        AlphaFormat: AC_SRC_ALPHA as u8,
    };
    let _ = UpdateLayeredWindow(
        popup,
        Some(dc),
        Some(&position),
        Some(&windows::Win32::Foundation::SIZE { cx: w, cy: h }),
        Some(mem),
        Some(&POINT::default()),
        windows::Win32::Foundation::COLORREF(0),
        Some(&blend),
        ULW_ALPHA,
    );
    let _ = SelectObject(mem, old);
    let _ = DeleteDC(mem);
    let _ = windows::Win32::Graphics::Gdi::ReleaseDC(None, dc);
    app.popup_surface = Some(surface);
}

unsafe fn paint_combo_popup_ui(app: &App, hdc: HDC, w: i32, h: i32) {
    let rc = RECT {
        left: 0,
        top: 0,
        right: w,
        bottom: h,
    };
    let s = |v: i32| app.s(v);
    let shadow = s(10);
    let _ = SetBkMode(hdc, TRANSPARENT);
    let saved = windows::Win32::Graphics::Gdi::SaveDC(hdc);
    let _ = windows::Win32::Graphics::Gdi::IntersectClipRect(
        hdc,
        shadow + s(8),
        shadow + s(8),
        rc.right - shadow - s(8),
        rc.bottom - shadow - s(8),
    );
    let pad = s(8);
    let item_h = s(40);
    let n = app.adapters.len() as i32 + 1;
    let scroll = app.combo_scroll;
    let inner_bottom = rc.bottom - shadow;
    for i in 0..n {
        let y = shadow + pad + i * item_h - scroll;
        if y + item_h < shadow || y > inner_bottom {
            continue;
        }
        let selected = app.combo_sel == i;
        let hovered = app.combo_hot == i;
        if selected || hovered {
            let bg = if selected {
                windows::Win32::Foundation::COLORREF(0x00FCF3EB)
            } else {
                windows::Win32::Foundation::COLORREF(0x00F5F5F5)
            };
            winutil::fill_component(
                hdc,
                RECT {
                    left: shadow + s(8),
                    top: y + s(2),
                    right: rc.right - shadow - s(8),
                    bottom: y + item_h - s(2),
                },
                app.dpi,
                bg,
                None,
            );
        }
        if selected {
            let bar = solid_brush(COLOR_ACCENT);
            let prev = SelectObject(hdc, brush_as_gdi(bar));
            let _ = windows::Win32::Graphics::Gdi::RoundRect(
                hdc,
                shadow + s(8),
                y + s(10),
                shadow + s(11),
                y + item_h - s(10),
                s(2),
                s(2),
            );
            let _ = SelectObject(hdc, prev);
            delete_gdi(brush_as_gdi(bar));
        }
        if i == 0 {
            paint_text(
                hdc,
                app.font,
                COLOR_TEXT_PRIMARY,
                shadow + s(20),
                y + s(10),
                "自动选择",
            );
        } else if let Some(a) = app.adapters.get((i as usize) - 1) {
            paint_text(
                hdc,
                app.font,
                COLOR_TEXT_PRIMARY,
                shadow + s(20),
                y + s(4),
                &a.name,
            );
            let sub = format!(
                "{}  ·  {}",
                a.mac,
                if a.is_up { "已连接" } else { "未连接" }
            );
            paint_text(
                hdc,
                app.font_label,
                COLOR_TEXT_SECONDARY,
                shadow + s(20),
                y + s(22),
                &sub,
            );
        }
    }
    let _ = windows::Win32::Graphics::Gdi::RestoreDC(hdc, saved);
}

unsafe fn toggle_flag(hwnd: HWND, which: usize, f: impl FnOnce(&mut Settings)) {
    if let Some(app) = app_mut(hwnd) {
        f(&mut app.settings);
        let on = match which {
            0 => app.settings.auto_login,
            1 => app.settings.remember_password,
            2 => app.settings.start_with_windows,
            _ => app.settings.minimize_to_tray,
        };
        let from = app.toggle_anim[which].value();
        app.toggle_anim[which] = Anim::go(from, bool01(on), TOGGLE_ANIM_MS);
        let rc = toggle_rect(app, which);
        start_anim_timer(hwnd);
        let _ = windows::Win32::Graphics::Gdi::InvalidateRect(Some(hwnd), Some(&rc), false);
    }
}

fn snap_toggle_anims(app: &mut App) {
    app.toggle_anim[0] = Anim::snap(bool01(app.settings.auto_login));
    app.toggle_anim[1] = Anim::snap(bool01(app.settings.remember_password));
    app.toggle_anim[2] = Anim::snap(bool01(app.settings.start_with_windows));
    app.toggle_anim[3] = Anim::snap(bool01(app.settings.minimize_to_tray));
}

unsafe fn start_anim_timer(hwnd: HWND) {
    let Some(app) = app_mut(hwnd) else {
        return;
    };
    if app.anim_timer_running {
        return;
    }
    app.anim_timer_running = SetTimer(Some(hwnd), TIMER_ANIM, ANIM_MS, None) != 0;
    if !app.anim_timer_running {
        snap_toggle_anims(app);
        app.combo_visual = Anim::snap(bool01(app.combo_open));
        if app.combo_open {
            show_combo_popup(hwnd);
        } else {
            dismiss_combo(hwnd);
        }
        invalidate(hwnd);
    }
}

unsafe fn invalidate_combo(hwnd: HWND) {
    if let Some(app) = app_mut(hwnd) {
        let rc = RECT {
            left: app.s(36) - 2,
            top: app.s(layout::COMBO_TOP) - 2,
            right: app.s(384) + 2,
            bottom: app.s(layout::COMBO_TOP + layout::FIELD_HEIGHT) + 2,
        };
        let _ = windows::Win32::Graphics::Gdi::InvalidateRect(Some(hwnd), Some(&rc), false);
    }
}

unsafe fn invalidate_hover(hwnd: HWND, hit: Hit) {
    let Some(app) = app_mut(hwnd) else {
        return;
    };
    let rect = match hit {
        Hit::Min => RECT {
            left: CLIENT_W - 92,
            top: 0,
            right: CLIENT_W - 46,
            bottom: TITLE_H,
        },
        Hit::Close => RECT {
            left: CLIENT_W - 46,
            top: 0,
            right: CLIENT_W,
            bottom: TITLE_H,
        },
        Hit::Connect => RECT {
            left: layout::CARD_LEFT,
            top: layout::ACTION_TOP,
            right: layout::CARD_RIGHT,
            bottom: layout::ACTION_BOTTOM,
        },
        _ => return,
    };
    let rect = RECT {
        left: app.s(rect.left) - 2,
        top: app.s(rect.top) - 2,
        right: app.s(rect.right) + 2,
        bottom: app.s(rect.bottom) + 2,
    };
    let _ = windows::Win32::Graphics::Gdi::InvalidateRect(Some(hwnd), Some(&rect), false);
}

fn toggle_rect(app: &App, which: usize) -> RECT {
    let x = if which % 2 == 0 { 154 } else { 344 };
    let y = if which < 2 {
        layout::TOGGLE_TOP
    } else {
        layout::TOGGLE_SECOND_TOP
    };
    RECT {
        left: app.s(x - 1),
        top: app.s(y - 1),
        right: app.s(x + 41),
        bottom: app.s(y + 21),
    }
}

unsafe fn on_anim_timer(hwnd: HWND) {
    let Some(app) = app_mut(hwnd) else {
        return;
    };
    let now = Instant::now();
    for i in 0..4 {
        if app.toggle_anim[i].advance(now) {
            let rc = toggle_rect(app, i);
            let _ = windows::Win32::Graphics::Gdi::InvalidateRect(Some(hwnd), Some(&rc), false);
        }
    }
    let combo_changed = app.combo_visual.advance(now);
    let combo_visible = app.combo_open || !app.combo_visual.done();
    if app.toggle_anim.iter().all(Anim::done) && app.combo_visual.done() {
        let _ = KillTimer(Some(hwnd), TIMER_ANIM);
        app.anim_timer_running = false;
    }
    if combo_changed {
        invalidate_combo(hwnd);
        if combo_visible {
            show_combo_popup(hwnd);
        } else if let Some(app) = app_mut(hwnd) {
            let _ = ShowWindow(app.hwnd_popup, SW_HIDE);
        }
    }
}

unsafe fn create_controls(hwnd: HWND) {
    let Some(app) = app_mut(hwnd) else {
        return;
    };
    let dpi = app.dpi;
    let font = app.font;
    let controls = layout::controls(dpi);
    let user_rc = controls.user;
    let pass_rc = controls.pass;
    app.eye_rect = controls.eye_hit;
    let user = create_child(
        hwnd,
        w!("EDIT"),
        w!(""),
        child_style(ES_AUTOHSCROLL as u32),
        WINDOW_EX_STYLE(0),
        user_rc.left,
        user_rc.top,
        user_rc.right - user_rc.left,
        user_rc.bottom - user_rc.top,
        ID_USER,
    );
    let pass = create_child(
        hwnd,
        w!("EDIT"),
        w!(""),
        child_style((ES_AUTOHSCROLL | ES_PASSWORD) as u32),
        WINDOW_EX_STYLE(0),
        pass_rc.left,
        pass_rc.top,
        pass_rc.right - pass_rc.left,
        pass_rc.bottom - pass_rc.top,
        ID_PASS,
    );
    app.hwnd_user = user;
    app.hwnd_pass = pass;
    strip_theme(user);
    strip_theme(pass);
    set_font(user, font);
    set_font(pass, font);
    super::textfield::attach(user);
    super::textfield::attach(pass);
    let _ = SendMessageW(pass, EM_SETPASSWORDCHAR, Some(WPARAM(0x2022)), None);
    app.hwnd_eye = create_child(
        hwnd,
        w!("BUTTON"),
        w!("显示密码"),
        child_style(0xB),
        WINDOW_EX_STYLE(0),
        controls.eye.left,
        controls.eye.top,
        controls.eye.right - controls.eye.left,
        controls.eye.bottom - controls.eye.top,
        ID_EYE,
    );
    set_font(app.hwnd_eye, font);
    use windows::Win32::UI::Controls::{
        TOOLTIPS_CLASSW, TTF_IDISHWND, TTF_SUBCLASS, TTM_ADDTOOLW, TTTOOLINFOW as TOOLINFOW,
    };
    app.hwnd_eye_tip = CreateWindowExW(
        WS_EX_TOOLWINDOW,
        TOOLTIPS_CLASSW,
        w!(""),
        WS_POPUP,
        0,
        0,
        0,
        0,
        Some(hwnd),
        None,
        None,
        None,
    )
    .unwrap_or_default();
    let info = TOOLINFOW {
        cbSize: std::mem::size_of::<TOOLINFOW>() as u32,
        uFlags: TTF_IDISHWND | TTF_SUBCLASS,
        hwnd,
        uId: app.hwnd_eye.0 as usize,
        lpszText: windows::core::PWSTR(w!("显示密码").as_ptr() as *mut _),
        ..Default::default()
    };
    let _ = SendMessageW(
        app.hwnd_eye_tip,
        TTM_ADDTOOLW,
        None,
        Some(LPARAM(&info as *const _ as isize)),
    );
}

unsafe fn initialize(hwnd: HWND) {
    let Some(app) = app_mut(hwnd) else {
        return;
    };
    if app.preview {
        app.settings.username = "202612345678".into();
        app.settings.password = "Ag09Test!".into();
        fill_controls(app);
        snap_toggle_anims(app);
        app.logo_title = winutil::rasterize_app_svg(app.s(52) as u32);
        app.logo_status = winutil::rasterize_app_svg(app.s(152) as u32);
        app.eye_on = winutil::rasterize_eye_on(app.s(40) as u32);
        app.eye_off = winutil::rasterize_eye_off(app.s(40) as u32);
        invalidate(hwnd);
        return;
    }

    if let Some(path) = winutil::ensure_icon_file() {
        let wp = wide(&path.to_string_lossy());
        let load = |cx: i32, cy: i32| {
            LoadImageW(
                None,
                PCWSTR(wp.as_ptr()),
                IMAGE_ICON,
                cx,
                cy,
                LR_LOADFROMFILE,
            )
            .ok()
            .map(|h| windows::Win32::UI::WindowsAndMessaging::HICON(h.0))
        };
        if let Some(h) = load(16, 16) {
            app.icon_small = h;
        }
        app.tray = Tray::create(hwnd, HIconOrFile::File(path, 16, 16));
    }
    let dpi = app.dpi;
    app.logo_title = winutil::rasterize_app_svg(winutil::scale(52, dpi) as u32);
    app.logo_status = winutil::rasterize_app_svg(winutil::scale(152, dpi) as u32);
    app.eye_on = winutil::rasterize_eye_on(winutil::scale(40, dpi) as u32);
    app.eye_off = winutil::rasterize_eye_off(winutil::scale(40, dpi) as u32);

    if paths::is_installed() {
        match paths::ensure_dirs() {
            Ok(()) => {
                let _ = crate::paths::mark_user_data_ready(&paths::root().unwrap_or_default());
                if let (Some(legacy), Some(dest)) =
                    (paths::legacy_local_app_data_root(), paths::root())
                {
                    if let Err(e) = crate::install::migrate::migrate_user_data(&legacy, &dest) {
                        let msg = wide(&format!("{}\n将使用默认设置。", e.message()));
                        let _ = MessageBoxW(
                            Some(hwnd),
                            PCWSTR(msg.as_ptr()),
                            w!("设置迁移失败"),
                            MB_OK | MB_ICONWARNING,
                        );
                    }
                }
            }
            Err(e) => {
                app.connect_enabled = false;
                app.link_state = LinkState::Error;
                app.status_title = "需要初始化".into();
                app.status_detail =
                    "当前用户数据目录无法创建。请重新运行安装包以初始化此账户，不要回落到 LocalAppData。".into();
                let msg = wide(&format!(
                    "安装版不能把数据写到 LocalAppData。请重新运行安装包完成本用户初始化。\n{e}"
                ));
                let _ = MessageBoxW(
                    Some(hwnd),
                    PCWSTR(msg.as_ptr()),
                    w!("用户数据未初始化"),
                    MB_OK | MB_ICONWARNING,
                );
                refresh_tray(app);
                return;
            }
        }
    }

    match settings::migrate_legacy() {
        Ok(s) => app.settings = s,
        Err(e) => {
            let msg = wide(&format!("{}\n将使用默认设置。", e.message()));
            let _ = MessageBoxW(
                Some(hwnd),
                PCWSTR(msg.as_ptr()),
                w!("设置迁移失败"),
                MB_OK | MB_ICONWARNING,
            );
            app.settings = Settings::default();
        }
    }

    fill_controls(app);
    snap_toggle_anims(app);
    let _ = paths::ensure_default_config();

    match platform::npcap_status() {
        NpcapStatus::Available => {}
        other => {
            app.connect_enabled = false;
            app.link_state = LinkState::Error;
            app.status_title = "缺少 Npcap".into();
            app.status_detail = "安装 Npcap 并启用 WinPcap 兼容模式后重新打开程序".into();
            let ask = if other == NpcapStatus::FoundButNotX64 {
                "检测到 Npcap，但缺少 x64 的 wpcap.dll。请重新安装并启用 WinPcap API 兼容模式。\n\n是否打开官方下载页？"
            } else {
                "校园网核心需要 Npcap 驱动。安装时请启用 WinPcap API 兼容模式。\n\n是否打开 Npcap 官方下载页？"
            };
            let ask_w = wide(ask);
            let r = MessageBoxW(
                Some(hwnd),
                PCWSTR(ask_w.as_ptr()),
                w!("需要安装 Npcap"),
                MB_YESNO | MB_ICONINFORMATION,
            );
            if r == IDYES {
                let _ = windows::Win32::UI::Shell::ShellExecuteW(
                    None,
                    w!("open"),
                    w!("https://npcap.com/#download"),
                    None,
                    None,
                    SW_SHOW,
                );
            }
            refresh_tray(app);
            return;
        }
    }

    match coreproc::ensure_core_extracted() {
        Ok(p) => app.core_path = Some(p),
        Err(e) => {
            app.connect_enabled = false;
            app.link_state = LinkState::Error;
            app.status_title = "需要修复安装".into();
            app.status_detail = format!("{e}");
            let msg = wide(&format!(
                "{e}\n\n安装版核心位于受保护目录，当前用户不能重写。请重新运行安装包修复。"
            ));
            let _ = MessageBoxW(
                Some(hwnd),
                PCWSTR(msg.as_ptr()),
                w!("核心缺失或损坏"),
                MB_OK | MB_ICONWARNING,
            );
            refresh_tray(app);
            return;
        }
    }

    platform::remove_legacy_run_value();
    if app.settings.start_with_windows {
        if let Ok(exe) = std::env::current_exe() {
            let _ = apply_startup(true, &exe);
        }
    }

    let complete = credentials_complete(app);
    app.desired_running = app.settings.auto_login && complete;
    app.manual_pause = !app.desired_running;
    if app.desired_running {
        attempt_start(hwnd);
    } else {
        refresh_tray(app);
        invalidate(hwnd);
    }
}

unsafe fn fill_controls(app: &mut App) {
    let user = wide(&app.settings.username);
    let _ = SetWindowTextW(app.hwnd_user, PCWSTR(user.as_ptr()));
    if app.settings.remember_password {
        let pass = wide(&app.settings.password);
        let _ = SetWindowTextW(app.hwnd_pass, PCWSTR(pass.as_ptr()));
    }

    app.popup_surface = None;
    app.adapters = adapters::enumerate();
    app.combo_sel = adapters::select(&app.settings.adapter_id, &app.settings.mac, &app.adapters)
        .map(|i| i as i32 + 1)
        .unwrap_or(0);
}

unsafe fn edit_text(hwnd: HWND) -> String {
    let mut buf = [0u16; 512];
    let n = GetWindowTextW(hwnd, &mut buf) as usize;
    String::from_utf16_lossy(&buf[..n])
}

unsafe fn credentials_complete(app: &App) -> bool {
    !edit_text(app.hwnd_user).trim().is_empty() && !edit_text(app.hwnd_pass).trim().is_empty()
}

fn combo_index(app: &App) -> i32 {
    app.combo_sel
}

unsafe fn capture_settings(app: &App) -> Settings {
    let idx = combo_index(app);
    let (adapter_id, mac) = if idx <= 0 {
        (String::new(), String::new())
    } else {
        app.adapters
            .get((idx as usize) - 1)
            .map(|a| (a.id.clone(), a.mac.clone()))
            .unwrap_or_default()
    };
    Settings {
        username: edit_text(app.hwnd_user).trim().to_string(),
        password: edit_text(app.hwnd_pass),
        adapter_id,
        mac,
        auto_login: app.settings.auto_login,
        remember_password: app.settings.remember_password,
        start_with_windows: app.settings.start_with_windows,
        minimize_to_tray: app.settings.minimize_to_tray,
    }
}

unsafe fn selected_adapter(app: &App) -> Option<&Adapter> {
    let idx = combo_index(app);
    if idx <= 0 {
        None
    } else {
        app.adapters.get((idx as usize) - 1)
    }
}

fn adapter_available(app: &App) -> bool {
    let chosen = if app.combo_sel > 0 {
        app.adapters.get((app.combo_sel - 1) as usize)
    } else {
        None
    };
    adapters::connection_available(chosen, &adapters::enumerate())
}

unsafe fn on_action(hwnd: HWND) {
    let Some(app) = app_mut(hwnd) else {
        return;
    };
    let active = app.desired_running
        || matches!(
            app.link_state,
            LinkState::Online | LinkState::Connecting | LinkState::Waiting
        )
        || app.core.as_ref().map(|c| c.is_running()).unwrap_or(false);
    if active {
        start_disconnect(hwnd);
    } else {
        start_connect(hwnd);
    }
}

unsafe fn start_connect(hwnd: HWND) {
    let Some(app) = app_mut(hwnd) else {
        return;
    };
    if app.preview {
        return;
    }
    if app.starting || app.exiting || !app.connect_enabled {
        return;
    }
    if !credentials_complete(app) {
        apply_status(hwnd, LinkState::Error, "信息不完整", "请填写学号和密码");
        return;
    }
    let captured = capture_settings(app);
    if let Err(e) = settings::save(&captured) {
        apply_status(hwnd, LinkState::Error, "保存失败", e.message());
        return;
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Err(e) = apply_startup(captured.start_with_windows, &exe) {
            let msg = wide(&e);
            let _ = MessageBoxW(
                Some(hwnd),
                PCWSTR(msg.as_ptr()),
                w!("开机启动"),
                MB_OK | MB_ICONWARNING,
            );
        }
    }
    app.settings = captured;
    app.manual_pause = false;
    app.desired_running = true;
    app.backoff.reset();
    attempt_start(hwnd);
}

unsafe fn apply_startup(enable: bool, exe: &std::path::Path) -> Result<(), String> {
    platform::remove_legacy_run_value();
    if enable {
        // 始终 /F 覆盖，升级后把旧任务改成带 --autostart 的命令行。
        platform::install(&exe.to_string_lossy())
    } else {
        platform::remove()
    }
}

unsafe fn attempt_start(hwnd: HWND) {
    let Some(app) = app_mut(hwnd) else {
        return;
    };
    if app.starting || app.exiting || !app.desired_running {
        return;
    }
    if app.core.as_ref().map(|c| c.is_running()).unwrap_or(false) {
        return;
    }
    if !adapter_available(app) {
        apply_status(hwnd, LinkState::Waiting, "等待网络", "网络可用后将自动连接");
        return;
    }
    if coreproc::same_name_running() {
        app.desired_running = false;
        apply_status(
            hwnd,
            LinkState::Error,
            "启动失败",
            "检测到另一个校园网核心正在运行。为避免中断现有连接，本应用不会接管或关闭它。",
        );
        return;
    }
    let Some(core_path) = app.core_path.clone() else {
        apply_status(hwnd, LinkState::Error, "启动失败", "核心尚未释放");
        return;
    };
    let Some(config_path) = paths::config_file() else {
        apply_status(hwnd, LinkState::Error, "启动失败", "无法确定配置路径");
        return;
    };
    let username = edit_text(app.hwnd_user).trim().to_string();
    let password = edit_text(app.hwnd_pass);
    let extra: Vec<String> = selected_adapter(app)
        .filter(|a| !a.mac.is_empty())
        .map(|a| vec!["--mac".into(), a.mac.clone()])
        .unwrap_or_default();

    app.starting = true;
    let now = Instant::now();
    let log_tail = paths::core_log_file()
        .map(|p| LogTail::before_launch(&p))
        .unwrap_or_default();
    app.backoff.record_launch(now);
    match coreproc::spawn_with_args(&core_path, &config_path, &username, &password, &extra) {
        Ok(core) => {
            app.core = Some(core);
            app.health = Some(HealthMonitor::new(now));
            app.log_tail = log_tail;
            apply_status(
                hwnd,
                LinkState::Connecting,
                "正在连接",
                "正在进行校园网认证",
            );
        }
        Err(e) => {
            let delay = app.backoff.record_failure(now);
            let msg = e.to_string();
            if msg.contains("另一个校园网核心") {
                app.desired_running = false;
            }
            if app.desired_running {
                apply_status(
                    hwnd,
                    LinkState::Error,
                    "启动失败",
                    &format!("{} 秒后重试", delay.as_secs().max(1)),
                );
            } else {
                apply_status(hwnd, LinkState::Error, "启动失败", &msg);
            }
        }
    }
    app.starting = false;
}

unsafe fn start_disconnect(hwnd: HWND) {
    let Some(app) = app_mut(hwnd) else {
        return;
    };
    app.desired_running = false;
    app.manual_pause = true;
    app.backoff.reset();
    app.health = None;
    app.suppress_exit_failure = true;
    if let Some(mut core) = app.core.take() {
        let _ = core.stop();
    }
    app.suppress_exit_failure = false;
    apply_status(hwnd, LinkState::Offline, "未连接", "已手动断开");
}

unsafe fn on_timer(hwnd: HWND) {
    let Some(app) = app_mut(hwnd) else {
        return;
    };
    if app.preview || app.starting || app.exiting {
        return;
    }
    let running = app.core.as_ref().map(|c| c.is_running()).unwrap_or(false);
    if app.core.is_some() && !running && !app.suppress_exit_failure {
        app.core = None;
        app.health = None;
        if app.desired_running && !app.manual_pause {
            app.backoff.record_failure(Instant::now());
            apply_status(
                hwnd,
                LinkState::Waiting,
                "等待重连",
                "核心意外退出，将自动重试",
            );
        }
    }

    if running {
        let now = Instant::now();
        let lines = paths::core_log_file().and_then(|p| app.log_tail.poll(&p).ok());
        let decision = app
            .health
            .get_or_insert_with(|| HealthMonitor::new(now))
            .observe(now, lines.as_deref());
        if decision.stable {
            app.backoff.reset();
        }
        match decision.state {
            LinkState::Online => {
                apply_status(hwnd, LinkState::Online, "已连接", "校园网认证成功");
            }
            LinkState::Degraded | LinkState::Error => {
                apply_status(
                    hwnd,
                    LinkState::Degraded,
                    "正在恢复",
                    "认证核心正在自动恢复连接",
                );
            }
            LinkState::Waiting => {
                apply_status(
                    hwnd,
                    LinkState::Waiting,
                    "等待开放",
                    "当前时段禁止上网，核心将按服务器规则重试",
                );
            }
            _ => {
                apply_status(
                    hwnd,
                    LinkState::Connecting,
                    "正在连接",
                    "正在进行校园网认证",
                );
            }
        }

        if decision.monitoring_unavailable {
            apply_status(
                hwnd,
                LinkState::Degraded,
                "连接状态未知",
                "暂时无法读取认证日志，保留当前连接并等待监控恢复",
            );
        }

        if app.desired_running && !app.manual_pause && decision.restart_stalled {
            app.suppress_exit_failure = true;
            if let Some(mut core) = app.core.take() {
                let _ = core.stop();
            }
            app.suppress_exit_failure = false;
            app.backoff.record_failure(now);
            app.health = None;
            apply_status(
                hwnd,
                LinkState::Waiting,
                "等待重连",
                "认证核心长时间未恢复，正在重新连接",
            );
        }
        return;
    }

    if !app.desired_running || app.manual_pause || !app.connect_enabled {
        return;
    }
    let now = Instant::now();
    if !app.backoff.can_attempt(now) {
        let secs = app.backoff.seconds_until_next_attempt(now).unwrap_or(1);
        let title = if app.backoff.in_cooldown(now) {
            "暂停重试"
        } else {
            "等待重连"
        };
        apply_status(
            hwnd,
            LinkState::Waiting,
            title,
            &format!("约 {secs} 秒后重试"),
        );
        return;
    }
    attempt_start(hwnd);
}

unsafe fn apply_status(hwnd: HWND, state: LinkState, title: &str, detail: &str) {
    let Some(app) = app_mut(hwnd) else {
        return;
    };
    if app.link_state == state && app.status_title == title && app.status_detail == detail {
        return;
    }
    app.link_state = state;
    app.status_title = title.to_string();
    app.status_detail = detail.to_string();
    refresh_tray(app);
    invalidate(hwnd);
}

unsafe fn refresh_tray(app: &App) {
    if let Some(tray) = &app.tray {
        let tip = format!("校园网 · {}", app.status_title);
        tray.set_tooltip(&tip);
    }
}

unsafe fn invalidate(hwnd: HWND) {
    let _ = windows::Win32::Graphics::Gdi::InvalidateRect(Some(hwnd), None, false);
}

unsafe fn request_close(hwnd: HWND) {
    dismiss_combo(hwnd);
    let Some(app) = app_mut(hwnd) else {
        return;
    };
    if app.exiting {
        return;
    }
    if app.settings.minimize_to_tray {
        let _ = ShowWindow(hwnd, SW_HIDE);
    } else {
        begin_exit(hwnd);
    }
}

unsafe fn begin_exit(hwnd: HWND) {
    let Some(app) = app_mut(hwnd) else {
        return;
    };
    if app.preview {
        let _ = DestroyWindow(hwnd);
        return;
    }
    if app.exiting {
        return;
    }
    app.exiting = true;
    app.desired_running = false;
    app.manual_pause = true;
    let captured = capture_settings(app);
    if let Err(e) = settings::save(&captured) {
        let msg = wide(e.message());
        let _ = MessageBoxW(
            Some(hwnd),
            PCWSTR(msg.as_ptr()),
            w!("无法保存设置"),
            MB_OK | MB_ICONWARNING,
        );
    }
    if let Ok(exe) = std::env::current_exe() {
        let _ = apply_startup(captured.start_with_windows, &exe);
    }
    app.suppress_exit_failure = true;
    if let Some(mut core) = app.core.take() {
        let _ = core.stop();
    }
    app.tray.take();
    let _ = DestroyWindow(hwnd);
}

unsafe fn toggle_password_reveal(hwnd: HWND) {
    let on = app_mut(hwnd).map(|a| !a.pass_revealed).unwrap_or(false);
    set_password_reveal(hwnd, on);
}

unsafe fn set_password_reveal(hwnd: HWND, on: bool) {
    let Some(app) = app_mut(hwnd) else {
        return;
    };
    if on == app.pass_revealed {
        return;
    }
    app.pass_revealed = on;
    let ch = if on { 0usize } else { 0x2022 };
    let _ = SendMessageW(app.hwnd_pass, EM_SETPASSWORDCHAR, Some(WPARAM(ch)), None);
    let _ = windows::Win32::Graphics::Gdi::InvalidateRect(Some(app.hwnd_pass), None, true);
    let label = if on {
        w!("隐藏密码")
    } else {
        w!("显示密码")
    };
    let _ = SetWindowTextW(app.hwnd_eye, label);
    use windows::Win32::UI::Controls::{
        TTF_IDISHWND, TTF_SUBCLASS, TTM_UPDATETIPTEXTW, TTTOOLINFOW as TOOLINFOW,
    };
    let info = TOOLINFOW {
        cbSize: std::mem::size_of::<TOOLINFOW>() as u32,
        uFlags: TTF_IDISHWND | TTF_SUBCLASS,
        hwnd,
        uId: app.hwnd_eye.0 as usize,
        lpszText: windows::core::PWSTR(label.as_ptr() as *mut _),
        ..Default::default()
    };
    let _ = SendMessageW(
        app.hwnd_eye_tip,
        TTM_UPDATETIPTEXTW,
        None,
        Some(LPARAM(&info as *const _ as isize)),
    );
    invalidate(app.hwnd_eye);
    invalidate(hwnd);
}

unsafe fn paint(hwnd: HWND, hdc: HDC, damage: RECT) {
    use windows::Win32::Graphics::Gdi::{
        SetBrushOrgEx, SetGraphicsMode, SetStretchBltMode, SetWorldTransform, StretchBlt,
        GM_ADVANCED, HALFTONE, XFORM,
    };
    // HALFTONE samples neighboring pixels. Render a guard band so partial
    // updates have identical antialiasing to a full-window paint.
    let saved = windows::Win32::Graphics::Gdi::SaveDC(hdc);
    let _ = windows::Win32::Graphics::Gdi::IntersectClipRect(
        hdc,
        damage.left,
        damage.top,
        damage.right,
        damage.bottom,
    );
    let mut client = RECT::default();
    let _ = GetClientRect(hwnd, &mut client);
    let damage = RECT {
        left: (damage.left - 4).max(0),
        top: (damage.top - 4).max(0),
        right: (damage.right + 4).min(client.right),
        bottom: (damage.bottom + 4).min(client.bottom),
    };
    // Keep supersampling, but allocate and downsample only the damaged area.
    // A 40x20 toggle must not rebuild a 1680x2096 full-window bitmap each tick.
    let w = damage.right - damage.left;
    let h = damage.bottom - damage.top;
    if w <= 0 || h <= 0 {
        let _ = windows::Win32::Graphics::Gdi::RestoreDC(hdc, saved);
        return;
    }
    let sw = w * 4;
    let sh = h * 4;
    let mem = CreateCompatibleDC(Some(hdc));
    let bmp = CreateCompatibleBitmap(hdc, sw, sh);
    let old = SelectObject(mem, windows::Win32::Graphics::Gdi::HGDIOBJ(bmp.0));
    let _ = SetGraphicsMode(mem, GM_ADVANCED);
    let xf = XFORM {
        eM11: 4.0,
        eM12: 0.0,
        eM21: 0.0,
        eM22: 4.0,
        eDx: -(damage.left * 4) as f32,
        eDy: -(damage.top * 4) as f32,
    };
    let _ = SetWorldTransform(mem, &xf);
    paint_ui(hwnd, mem);
    let identity = XFORM {
        eM11: 1.0,
        eM12: 0.0,
        eM21: 0.0,
        eM22: 1.0,
        eDx: 0.0,
        eDy: 0.0,
    };
    let _ = SetWorldTransform(mem, &identity);
    let _ = SetStretchBltMode(hdc, HALFTONE);
    let _ = SetBrushOrgEx(hdc, 0, 0, None);
    let _ = StretchBlt(
        hdc,
        damage.left,
        damage.top,
        w,
        h,
        Some(mem),
        0,
        0,
        sw,
        sh,
        SRCCOPY,
    );
    let _ = SelectObject(mem, old);
    let _ =
        windows::Win32::Graphics::Gdi::DeleteObject(windows::Win32::Graphics::Gdi::HGDIOBJ(bmp.0));
    let _ = DeleteDC(mem);
    let _ = windows::Win32::Graphics::Gdi::RestoreDC(hdc, saved);
}

unsafe fn paint_ui(hwnd: HWND, hdc: HDC) {
    let Some(app) = app_mut(hwnd) else {
        return;
    };
    let s = |v: i32| app.s(v);
    let mut rc = RECT::default();
    let _ = GetClientRect(hwnd, &mut rc);
    let _ = FillRect(hdc, &rc, app.page_brush);
    let _ = SetBkMode(hdc, TRANSPARENT);

    winutil::fill_component(
        hdc,
        RECT {
            left: 0,
            top: 0,
            right: s(CLIENT_W),
            bottom: s(CLIENT_H),
        },
        app.dpi,
        winutil::COLOR_PAGE,
        Some(winutil::COLOR_WINDOW_BORDER),
    );

    if let Some(logo) = &app.logo_title {
        winutil::blit_svg(hdc, logo, s(layout::OUTER_PADDING), s(7), s(26), s(26));
    }
    paint_caption_btn(
        hdc,
        app,
        s(CLIENT_W) - s(92),
        0,
        s(46),
        s(TITLE_H),
        app.hover == Hit::Min,
        false,
    );
    paint_caption_btn(
        hdc,
        app,
        s(CLIENT_W) - s(46),
        0,
        s(46),
        s(TITLE_H),
        app.hover == Hit::Close,
        true,
    );

    winutil::fill_component(
        hdc,
        RECT {
            left: s(layout::CARD_LEFT),
            top: s(layout::STATUS_TOP),
            right: s(layout::CARD_RIGHT),
            bottom: s(layout::STATUS_BOTTOM),
        },
        app.dpi,
        COLOR_CARD,
        Some(winutil::COLOR_STROKE),
    );
    let dot = winutil::state_color(app.link_state);
    let brush = solid_brush(dot);
    let prev = SelectObject(hdc, brush_as_gdi(brush));
    let _ = Ellipse(
        hdc,
        s(36),
        s(layout::STATUS_TOP + 38),
        s(48),
        s(layout::STATUS_TOP + 50),
    );
    let _ = SelectObject(hdc, prev);
    delete_gdi(brush_as_gdi(brush));
    paint_text(
        hdc,
        app.font_title,
        COLOR_TEXT_PRIMARY,
        s(56),
        s(layout::STATUS_TOP + 20),
        &app.status_title,
    );
    paint_text(
        hdc,
        app.font_label,
        COLOR_TEXT_SECONDARY,
        s(56),
        s(layout::STATUS_TOP + 50),
        &app.status_detail,
    );
    if let Some(logo) = &app.logo_status {
        winutil::blit_svg(
            hdc,
            logo,
            s(308),
            s(layout::STATUS_TOP + (layout::STATUS_HEIGHT - 76) / 2),
            s(76),
            s(76),
        );
    }

    winutil::fill_component(
        hdc,
        RECT {
            left: s(layout::CARD_LEFT),
            top: s(layout::ACCOUNT_TOP),
            right: s(layout::CARD_RIGHT),
            bottom: s(layout::ACCOUNT_BOTTOM),
        },
        app.dpi,
        COLOR_CARD,
        Some(winutil::COLOR_STROKE),
    );
    paint_text(
        hdc,
        app.font,
        COLOR_TEXT_PRIMARY,
        s(46),
        s(layout::ACCOUNT_TOP + layout::CARD_PADDING),
        "账号",
    );
    paint_text(
        hdc,
        app.font_label,
        COLOR_TEXT_SECONDARY,
        s(46),
        s(layout::USER_TOP - 18),
        "学号",
    );
    let focus = windows::Win32::UI::Input::KeyboardAndMouse::GetFocus();
    paint_field(
        hdc,
        app,
        s(36),
        s(layout::USER_TOP),
        s(348),
        s(36),
        focus == app.hwnd_user,
    );
    paint_text(
        hdc,
        app.font_label,
        COLOR_TEXT_SECONDARY,
        s(46),
        s(layout::PASS_TOP - 18),
        "密码",
    );
    paint_field(
        hdc,
        app,
        s(36),
        s(layout::PASS_TOP),
        s(348),
        s(36),
        focus == app.hwnd_pass,
    );
    paint_text(
        hdc,
        app.font_label,
        COLOR_TEXT_SECONDARY,
        s(46),
        s(layout::COMBO_TOP - 18),
        "网卡",
    );
    paint_field(
        hdc,
        app,
        s(36),
        s(layout::COMBO_TOP),
        s(348),
        s(36),
        app.combo_open || app.combo_visual.value() > 0.08,
    );
    let combo_txt = combo_label(app);
    paint_text_centered(
        hdc,
        app.font,
        COLOR_TEXT_PRIMARY,
        s(46),
        s(layout::COMBO_TOP),
        s(300),
        s(36),
        &combo_txt,
    );
    paint_chevron(
        hdc,
        s(368),
        s(layout::COMBO_TOP) + s(18),
        s(8),
        app.combo_visual.value(),
    );

    paint_toggle_row(
        hdc,
        app,
        s(36),
        s(layout::TOGGLE_TOP),
        "启动后自动连接",
        app.toggle_anim[0].value(),
        "保留密码",
        app.toggle_anim[1].value(),
        s(220),
        s(344),
    );
    paint_toggle_row(
        hdc,
        app,
        s(36),
        s(layout::TOGGLE_SECOND_TOP),
        "开机启动",
        app.toggle_anim[2].value(),
        "保留系统托盘",
        app.toggle_anim[3].value(),
        s(220),
        s(344),
    );

    let disconnect = app.desired_running
        || matches!(
            app.link_state,
            LinkState::Online | LinkState::Connecting | LinkState::Waiting
        );
    let hovered = app.hover == Hit::Connect;
    let bg = if !app.connect_enabled {
        windows::Win32::Foundation::COLORREF(0x00FAD6B4)
    } else if disconnect {
        if hovered {
            windows::Win32::Foundation::COLORREF(0x000D26A1)
        } else {
            COLOR_DANGER
        }
    } else if hovered {
        winutil::COLOR_ACCENT_HOVER
    } else {
        COLOR_ACCENT
    };
    winutil::fill_component(
        hdc,
        RECT {
            left: s(layout::CARD_LEFT),
            top: s(layout::ACTION_TOP),
            right: s(layout::CARD_RIGHT),
            bottom: s(layout::ACTION_BOTTOM),
        },
        app.dpi,
        bg,
        None,
    );
    let label = if disconnect { "断开" } else { "连接" };
    paint_text_centered(
        hdc,
        app.font_btn,
        windows::Win32::Foundation::COLORREF(0x00FFFFFF),
        s(20),
        s(layout::ACTION_TOP),
        s(380),
        s(36),
        label,
    );
}

fn combo_label(app: &App) -> String {
    if app.combo_sel <= 0 {
        "自动选择".into()
    } else {
        app.adapters
            .get((app.combo_sel as usize) - 1)
            .map(|a| {
                format!(
                    "{}  ·  {}",
                    a.name,
                    if a.is_up { "已连接" } else { "未连接" }
                )
            })
            .unwrap_or_else(|| "自动选择".into())
    }
}

unsafe fn paint_chevron(hdc: HDC, cx: i32, cy: i32, arm: i32, t: f32) {
    use windows::Win32::Graphics::Gdi::{GetWorldTransform, SetWorldTransform, XFORM};
    let mut original = XFORM::default();
    let _ = GetWorldTransform(hdc, &mut original);
    let (sin, cos) = (std::f32::consts::PI * t).sin_cos();
    let rotated = XFORM {
        eM11: cos * original.eM11,
        eM12: sin * original.eM22,
        eM21: -sin * original.eM11,
        eM22: cos * original.eM22,
        eDx: original.eDx + cx as f32 * original.eM11,
        eDy: original.eDy + cy as f32 * original.eM22,
    };
    let _ = SetWorldTransform(hdc, &rotated);
    let pen = windows::Win32::Graphics::Gdi::CreatePen(
        windows::Win32::Graphics::Gdi::PS_SOLID,
        (arm / 4).max(1),
        COLOR_TEXT_SECONDARY,
    );
    let old = SelectObject(hdc, windows::Win32::Graphics::Gdi::HGDIOBJ(pen.0));
    let _ = MoveToEx(hdc, -arm, -arm / 2, None);
    let _ = LineTo(hdc, 0, arm / 2);
    let _ = LineTo(hdc, arm, -arm / 2);
    let _ = SelectObject(hdc, old);
    let _ =
        windows::Win32::Graphics::Gdi::DeleteObject(windows::Win32::Graphics::Gdi::HGDIOBJ(pen.0));
    let _ = SetWorldTransform(hdc, &original);
}

unsafe fn paint_text(
    hdc: HDC,
    font: windows::Win32::Graphics::Gdi::HFONT,
    color: windows::Win32::Foundation::COLORREF,
    x: i32,
    y: i32,
    s: &str,
) {
    let old = SelectObject(hdc, font_as_gdi(font));
    let _ = SetTextColor(hdc, color);
    let t = wide(s);
    if t.len() > 1 {
        let _ = TextOutW(hdc, x, y, &t[..t.len() - 1]);
    }
    let _ = SelectObject(hdc, old);
}

unsafe fn paint_text_centered(
    hdc: HDC,
    font: windows::Win32::Graphics::Gdi::HFONT,
    color: windows::Win32::Foundation::COLORREF,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    s: &str,
) {
    let old = SelectObject(hdc, font_as_gdi(font));
    let _ = SetTextColor(hdc, color);
    let t = wide(s);
    let mut sz = windows::Win32::Foundation::SIZE::default();
    if t.len() > 1 {
        let _ = GetTextExtentPoint32W(hdc, &t[..t.len() - 1], &mut sz);
        let tx = x + (w - sz.cx) / 2;
        let ty = y + (h - sz.cy) / 2;
        let _ = TextOutW(hdc, tx, ty, &t[..t.len() - 1]);
    }
    let _ = SelectObject(hdc, old);
}

unsafe fn paint_field(hdc: HDC, app: &App, x: i32, y: i32, w: i32, h: i32, focus: bool) {
    let border = if focus {
        COLOR_ACCENT
    } else {
        winutil::COLOR_STROKE
    };
    winutil::fill_component(
        hdc,
        RECT {
            left: x,
            top: y,
            right: x + w,
            bottom: y + h,
        },
        app.dpi,
        winutil::COLOR_CONTROL,
        Some(border),
    );
}

unsafe fn paint_toggle_row(
    hdc: HDC,
    app: &App,
    x: i32,
    y: i32,
    left: &str,
    left_t: f32,
    right: &str,
    right_t: f32,
    right_x: i32,
    toggle2_x: i32,
) {
    paint_text(
        hdc,
        app.font_label,
        COLOR_TEXT_PRIMARY,
        x,
        y + app.s(2),
        left,
    );
    paint_toggle(hdc, app, x + app.s(118), y, left_t);
    paint_text(
        hdc,
        app.font_label,
        COLOR_TEXT_PRIMARY,
        right_x,
        y + app.s(2),
        right,
    );
    paint_toggle(hdc, app, toggle2_x, y, right_t);
}

unsafe fn paint_toggle(hdc: HDC, app: &App, x: i32, y: i32, t: f32) {
    use windows::Win32::Graphics::Gdi::{GetWorldTransform, SetWorldTransform, XFORM};
    let w = app.s(40);
    let h = app.s(20);
    let fill = lerp_color(winutil::COLOR_TOGGLE_OFF, COLOR_ACCENT, t);
    winutil::fill_round(
        hdc,
        RECT {
            left: x,
            top: y,
            right: x + w,
            bottom: y + h,
        },
        h / 2,
        fill,
        None,
    );
    let thumb = app.s(16);
    let inset = (h - thumb) as f32 / 2.0;
    let travel = (w - thumb) as f32 - inset * 2.0;
    let tx = x as f32 + inset + travel * t.clamp(0.0, 1.0);
    let ty = y as f32 + inset;
    let mut original = XFORM::default();
    let _ = GetWorldTransform(hdc, &mut original);
    let mut shifted = original;
    shifted.eDx += tx.fract() * original.eM11;
    shifted.eDy += ty.fract() * original.eM22;
    let _ = SetWorldTransform(hdc, &shifted);
    winutil::fill_round(
        hdc,
        RECT {
            left: tx as i32,
            top: ty as i32,
            right: tx as i32 + thumb,
            bottom: ty as i32 + thumb,
        },
        thumb / 2,
        COLOR_CARD,
        None,
    );
    let _ = SetWorldTransform(hdc, &original);
}

unsafe fn paint_caption_btn(
    hdc: HDC,
    app: &App,
    x: i32,
    y: i32,
    bw: i32,
    bh: i32,
    hover: bool,
    close: bool,
) {
    if hover {
        let fill = if close {
            COLOR_DANGER
        } else {
            windows::Win32::Foundation::COLORREF(0x00E8E8E8)
        };
        winutil::fill_component(
            hdc,
            RECT {
                left: x,
                top: y,
                right: x + bw,
                bottom: y + bh,
            },
            app.dpi,
            fill,
            None,
        );
    }
    let color = if hover && close {
        windows::Win32::Foundation::COLORREF(0x00FFFFFF)
    } else {
        COLOR_TEXT_PRIMARY
    };
    let pen =
        windows::Win32::Graphics::Gdi::CreatePen(windows::Win32::Graphics::Gdi::PS_SOLID, 1, color);
    let old = SelectObject(hdc, windows::Win32::Graphics::Gdi::HGDIOBJ(pen.0));
    let cx = x + bw / 2;
    let cy = y + bh / 2;
    let arm = (bw / 9).max(5);
    if close {
        let _ = MoveToEx(hdc, cx - arm, cy - arm, None);
        let _ = LineTo(hdc, cx + arm + 1, cy + arm + 1);
        let _ = MoveToEx(hdc, cx + arm, cy - arm, None);
        let _ = LineTo(hdc, cx - arm - 1, cy + arm + 1);
    } else {
        let _ = MoveToEx(hdc, cx - arm, cy, None);
        let _ = LineTo(hdc, cx + arm + 1, cy);
    }
    let _ = SelectObject(hdc, old);
    let _ =
        windows::Win32::Graphics::Gdi::DeleteObject(windows::Win32::Graphics::Gdi::HGDIOBJ(pen.0));
}
