//! Runtime DPI/display changes for the fixed logical client layout.
use super::*;
use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowRect, IsIconic, PostMessageW, SetWindowPos, SWP_FRAMECHANGED, SWP_NOACTIVATE,
    SWP_NOZORDER,
};

pub(super) const WM_APP_LAYOUT: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 15;

pub(super) unsafe fn schedule(hwnd: HWND) {
    let Some(app) = app_mut(hwnd) else {
        return;
    };
    if !app.layout_pending && !app.layout_in_progress {
        app.layout_pending = PostMessageW(Some(hwnd), WM_APP_LAYOUT, WPARAM(0), LPARAM(0)).is_ok();
    }
}

pub(super) unsafe fn refresh(hwnd: HWND) {
    if let Some(app) = app_mut(hwnd) {
        app.layout_pending = false;
    }
    let mut rect = RECT::default();
    if GetWindowRect(hwnd, &mut rect).is_ok() {
        change(hwnd, winutil::window_dpi(hwnd), rect);
    }
}

pub(super) unsafe fn change(hwnd: HWND, dpi: u32, suggested: RECT) {
    dismiss_combo(hwnd);
    if let Some(app) = app_mut(hwnd) {
        app.popup_surface = None;
    }
    let work = winutil::rect_work_area(suggested);
    apply(hwnd, dpi, suggested, work);
}

unsafe fn apply(hwnd: HWND, display_dpi: u32, anchor: RECT, work: RECT) {
    let Some(app) = app_mut(hwnd) else {
        return;
    };
    // Minimized HWNDs have special coordinates. Apply the current monitor's
    // layout when restored, without making hidden/autostart windows visible.
    if app.layout_in_progress || IsIconic(hwnd).as_bool() {
        return;
    }
    let (dpi, bounds) =
        layout::display_layout(app.preview_dpi.unwrap_or(display_dpi), anchor, work);
    let mut current = RECT::default();
    let _ = GetWindowRect(hwnd, &mut current);
    if dpi == app.dpi && current == bounds {
        return;
    }
    app.layout_in_progress = true;
    dismiss_combo(hwnd);
    let app = app_mut(hwnd).unwrap();
    app.popup_surface = None;
    app.combo_scroll = 0;
    app.combo_hot = -1;
    if dpi != app.dpi {
        app.dpi = dpi;
        let old_fonts = [app.font, app.font_title, app.font_label, app.font_btn];
        app.font = create_font(14, dpi, false);
        app.font_title = create_font(20, dpi, true);
        app.font_label = create_font(12, dpi, false);
        app.font_btn = create_font(14, dpi, true);
        app.logo_title = winutil::rasterize_app_svg(app.s(52).max(1) as u32);
        app.logo_status = winutil::rasterize_app_svg(app.s(152).max(1) as u32);
        app.eye_on = winutil::rasterize_eye_on(app.s(40).max(1) as u32);
        app.eye_off = winutil::rasterize_eye_off(app.s(40).max(1) as u32);
        for child in [app.hwnd_user, app.hwnd_pass, app.hwnd_eye, app.hwnd_eye_tip] {
            if !child.0.is_null() {
                set_font(child, app.font);
            }
        }
        // EDIT must stop referring to the old font before its GDI handle dies.
        for font in old_fonts {
            delete_gdi(font_as_gdi(font));
        }
    }
    let controls = layout::controls(dpi);
    app.eye_rect = controls.eye_hit;
    for (child, rect) in [
        (app.hwnd_user, controls.user),
        (app.hwnd_pass, controls.pass),
        (app.hwnd_eye, controls.eye),
    ] {
        if !child.0.is_null() {
            let _ = SetWindowPos(
                child,
                None,
                rect.left,
                rect.top,
                rect.right - rect.left,
                rect.bottom - rect.top,
                SWP_NOACTIVATE | SWP_NOZORDER | SWP_FRAMECHANGED,
            );
        }
    }
    let _ = SetWindowPos(
        hwnd,
        None,
        bounds.left,
        bounds.top,
        bounds.right - bounds.left,
        bounds.bottom - bounds.top,
        SWP_NOACTIVATE | SWP_NOZORDER,
    );
    round_corners(hwnd);
    if let Some(app) = app_mut(hwnd) {
        app.layout_in_progress = false;
    }
    // Include the native children and their non-client text viewport.
    let _ = windows::Win32::Graphics::Gdi::RedrawWindow(
        Some(hwnd),
        None,
        None,
        windows::Win32::Graphics::Gdi::RDW_INVALIDATE
            | windows::Win32::Graphics::Gdi::RDW_ALLCHILDREN
            | windows::Win32::Graphics::Gdi::RDW_FRAME,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Graphics::Gdi::{GetObjectW, LOGFONTW};
    use windows::Win32::UI::WindowsAndMessaging::{
        IsWindowVisible, WM_CHAR, WM_DPICHANGED, WM_GETFONT, WM_SIZE,
    };

    struct Fixture {
        hwnd: HWND,
        app: Box<App>,
    }
    impl Fixture {
        unsafe fn new() -> Self {
            let mut app = Box::new(App::new());
            app.preview = true;
            app.preview_dpi = None;
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("STATIC"),
                w!(""),
                WS_POPUP,
                20,
                20,
                app.s(CLIENT_W),
                app.s(CLIENT_H),
                None,
                None,
                None,
                None,
            )
            .unwrap();
            SetWindowLongPtrW(
                hwnd,
                windows::Win32::UI::WindowsAndMessaging::GWLP_USERDATA,
                (&mut *app as *mut App) as isize,
            );
            create_controls(hwnd);
            app.logo_title = winutil::rasterize_app_svg(app.s(52) as u32);
            app.eye_on = winutil::rasterize_eye_on(app.s(40) as u32);
            SetWindowTextW(app.hwnd_user, w!("test-account")).unwrap();
            SetWindowTextW(app.hwnd_pass, w!("_Ag09_中文")).unwrap();
            Self { hwnd, app }
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            unsafe {
                // Do not dispatch WM_DESTROY to the production window procedure:
                // this fixture owns App and never runs client initialization.
                SetWindowLongPtrW(
                    self.hwnd,
                    windows::Win32::UI::WindowsAndMessaging::GWLP_USERDATA,
                    0,
                );
                let _ = DestroyWindow(self.hwnd);
            }
        }
    }

    unsafe fn assert_geometry(f: &Fixture) {
        let mut client = RECT::default();
        GetClientRect(f.hwnd, &mut client).unwrap();
        assert_eq!(
            (client.right, client.bottom),
            (f.app.s(CLIENT_W), f.app.s(CLIENT_H))
        );
        let controls = layout::controls(f.app.dpi);
        let mut parent = RECT::default();
        GetWindowRect(f.hwnd, &mut parent).unwrap();
        for (child, expected) in [
            (f.app.hwnd_user, controls.user),
            (f.app.hwnd_pass, controls.pass),
            (f.app.hwnd_eye, controls.eye),
        ] {
            let mut actual = RECT::default();
            GetWindowRect(child, &mut actual).unwrap();
            assert_eq!(
                RECT {
                    left: actual.left - parent.left,
                    top: actual.top - parent.top,
                    right: actual.right - parent.left,
                    bottom: actual.bottom - parent.top
                },
                expected
            );
            assert_eq!(
                SendMessageW(child, WM_GETFONT, None, None).0,
                f.app.font.0 as isize
            );
        }
        let mut font = LOGFONTW::default();
        assert_ne!(
            GetObjectW(
                font_as_gdi(f.app.font),
                std::mem::size_of::<LOGFONTW>() as i32,
                Some((&mut font as *mut LOGFONTW).cast())
            ),
            0
        );
        assert_eq!(font.lfHeight, -f.app.s(14));
        assert_eq!(
            hit_test(&f.app, f.app.s(368), f.app.s(layout::COMBO_TOP + 18)),
            Hit::Combo
        );
        assert_eq!(
            hit_test(&f.app, f.app.s(175), f.app.s(layout::TOGGLE_TOP + 10)),
            Hit::Auto
        );
        assert_eq!(hit_test(&f.app, f.app.s(380), f.app.s(20)), Hit::Close);
        assert_eq!(f.app.eye_rect, controls.eye_hit);
        assert!(
            !IsWindowVisible(f.hwnd).as_bool(),
            "display change must not show a hidden window"
        );
    }

    #[test]
    fn runtime_scale_changes_preserve_native_controls_text_selection_undo_and_state() {
        unsafe {
            let mut f = Fixture::new();
            let children = [f.app.hwnd_user, f.app.hwnd_pass, f.app.hwnd_eye];
            f.app.desired_running = true;
            f.app.link_state = LinkState::Online;
            let _ = SendMessageW(f.app.hwnd_pass, 0xB1, Some(WPARAM(1)), Some(LPARAM(1))); // EM_SETSEL
            let _ = SendMessageW(f.app.hwnd_pass, WM_CHAR, Some(WPARAM('X' as usize)), None);
            let selection = SendMessageW(f.app.hwnd_pass, 0xB0, None, None).0;
            let text = edit_text(f.app.hwnd_pass);
            let work = RECT {
                left: 0,
                top: 0,
                right: 3840,
                bottom: 2160,
            };
            let anchor = RECT {
                left: 20,
                top: 20,
                right: 860,
                bottom: 1136,
            };
            for dpi in [144, 192, 120, 96, 192, 96] {
                apply(f.hwnd, dpi, anchor, work);
                assert_eq!(f.app.dpi, dpi);
                assert_geometry(&f);
                assert_eq!([f.app.hwnd_user, f.app.hwnd_pass, f.app.hwnd_eye], children);
                assert_eq!(edit_text(f.app.hwnd_user), "test-account");
                assert_eq!(edit_text(f.app.hwnd_pass), text);
                assert_eq!(SendMessageW(f.app.hwnd_pass, 0xB0, None, None).0, selection);
                assert_ne!(SendMessageW(f.app.hwnd_pass, 0xC6, None, None).0, 0); // EM_CANUNDO
                assert_eq!(SendMessageW(f.app.hwnd_pass, 0xD2, None, None).0, 0x2022);
                assert!(f.app.desired_running && f.app.link_state == LinkState::Online);
                assert_eq!(f.app.logo_title.as_ref().unwrap().w, f.app.s(52));
                assert_eq!(f.app.eye_on.as_ref().unwrap().w, f.app.s(40));
                assert!(f.app.popup_surface.is_none());
            }
            let _ = SendMessageW(f.app.hwnd_pass, 0xC7, None, None); // EM_UNDO
            assert_eq!(edit_text(f.app.hwnd_pass), "_Ag09_中文");
        }
    }

    #[test]
    fn smaller_work_area_scales_contents_then_restores_without_accumulated_rounding() {
        unsafe {
            let f = Fixture::new();
            let anchor = RECT {
                left: 1700,
                top: 900,
                right: 2540,
                bottom: 2016,
            };
            for height in [680, 480, 2160, 680, 2160] {
                let work = RECT {
                    left: 0,
                    top: 0,
                    right: 3840,
                    bottom: height,
                };
                apply(f.hwnd, 192, anchor, work);
                assert_geometry(&f);
                let mut bounds = RECT::default();
                GetWindowRect(f.hwnd, &mut bounds).unwrap();
                assert!(bounds.top >= 0 && bounds.bottom <= height);
                assert!(f.app.s(layout::ACTION_BOTTOM) < bounds.bottom - bounds.top);
                if height == 2160 {
                    assert_eq!(f.app.dpi, 192);
                }
            }
        }
    }

    #[test]
    fn dpi_message_relayouts_and_size_message_repairs_external_resize() {
        unsafe {
            let mut f = Fixture::new();
            let proposed = RECT {
                left: 20,
                top: 20,
                right: 650,
                bottom: 857,
            };
            f.app.combo_open = true;
            f.app.combo_visual = Anim::snap(1.0);
            f.app.popup_surface = PopupSurface::new(&f.app, 368, 116);
            wndproc(
                f.hwnd,
                WM_DPICHANGED,
                WPARAM(144 | (144 << 16)),
                LPARAM((&proposed as *const RECT) as isize),
            );
            assert!(!f.app.combo_open && f.app.popup_surface.is_none());
            assert_eq!(
                f.app.dpi,
                layout::display_layout(144, proposed, winutil::rect_work_area(proposed)).0
            );
            assert_geometry(&f);
            let _ = SetWindowPos(f.hwnd, None, 0, 0, 800, 300, SWP_NOACTIVATE | SWP_NOZORDER);
            wndproc(f.hwnd, WM_SIZE, WPARAM(0), LPARAM(0));
            assert!(f.app.layout_pending);
            wndproc(f.hwnd, WM_APP_LAYOUT, WPARAM(0), LPARAM(0));
            assert!(!f.app.layout_pending);
            assert_geometry(&f);
        }
    }
}
