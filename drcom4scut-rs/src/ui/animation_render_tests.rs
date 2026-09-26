//! Offscreen regression tests: no client initialization, visible UI or network.
use super::*;
use windows::Win32::Graphics::Gdi::{GdiFlush, HGDIOBJ};

// An invisible STATIC window supplies client geometry only.
struct Scene {
    hwnd: HWND,
    app: Box<App>,
}
impl Scene {
    unsafe fn attach_native_controls(&mut self) {
        use windows::Win32::UI::WindowsAndMessaging::GWLP_WNDPROC;
        SetWindowLongPtrW(
            self.hwnd,
            GWLP_WNDPROC,
            fixture_proc as *const () as usize as isize,
        );
        create_controls(self.hwnd);
        SetWindowTextW(self.app.hwnd_user, w!("202600000001")).unwrap();
        SetWindowTextW(self.app.hwnd_pass, w!("campus-demo")).unwrap();
    }

    unsafe fn render_native(&self, canvas: &Canvas) {
        use windows::Win32::Graphics::Gdi::*;
        use windows::Win32::UI::WindowsAndMessaging::*;
        // Use the production WM_PAINT path, not only its inner renderer.
        paint(self.hwnd, canvas.dc, self.full());
        let mut children = vec![
            self.app.hwnd_user_label,
            self.app.hwnd_pass_label,
            self.app.hwnd_user,
            self.app.hwnd_pass,
            self.app.hwnd_eye,
        ];
        children.extend(self.app.buttons.iter().map(|(_, child)| *child));
        for child in children {
            if GetWindowLongPtrW(child, GWL_STYLE) & WS_VISIBLE.0 as isize == 0 {
                continue;
            }
            let mut r = RECT::default();
            GetWindowRect(child, &mut r).unwrap();
            let points = std::slice::from_raw_parts_mut((&mut r as *mut RECT).cast::<POINT>(), 2);
            let _ = MapWindowPoints(None, Some(self.hwnd), points);
            let saved = SaveDC(canvas.dc);
            let _ = IntersectClipRect(canvas.dc, r.left, r.top, r.right, r.bottom);
            let _ = SetViewportOrgEx(canvas.dc, r.left, r.top, None);
            let _ = SendMessageW(
                child,
                WM_PRINT,
                Some(WPARAM(canvas.dc.0 as usize)),
                Some(LPARAM((PRF_CLIENT | PRF_NONCLIENT) as isize)),
            );
            let _ = RestoreDC(canvas.dc, saved);
        }
    }

    unsafe fn new(dpi: u32) -> Self {
        Self::new_themed(dpi, false)
    }

    unsafe fn new_themed(dpi: u32, is_dark: bool) -> Self {
        let mut app = Box::new(App::new());
        app.preview = true;
        app.dpi = dpi;
        app.is_dark = is_dark;
        let palette = winutil::Palette::for_dark(is_dark);
        delete_gdi(brush_as_gdi(app.page_brush));
        delete_gdi(brush_as_gdi(app.card_brush));
        delete_gdi(brush_as_gdi(app.control_brush));
        app.page_brush = solid_brush(palette.page);
        app.card_brush = solid_brush(palette.card);
        app.control_brush = solid_brush(palette.control);
        for (font, size, weight) in [
            (&mut app.font, 14, 400),
            (&mut app.font_title, 28, 600),
            (&mut app.font_label, layout::CAPTION_SIZE, 400),
            (&mut app.font_btn, 14, 500),
            (&mut app.font_medium, 13, 500),
            (&mut app.font_chip, 12, 400),
            (&mut app.font_brand, 12, 600),
            (&mut app.font_footer, 11, 400),
        ] {
            delete_gdi(font_as_gdi(*font));
            *font = winutil::create_font_weight(size, dpi, weight);
        }
        app.adapters = vec![Adapter {
            id: "fixture".into(),
            mac: "00:11:22:33:44:55".into(),
            name: "无线网络 · 测试网卡".into(),
            is_up: true,
            has_ipv4_gateway: true,
        }];
        app.logo_title = winutil::rasterize_app_svg(app.s(52) as u32);
        app.logo_status = winutil::rasterize_app_svg(app.s(152) as u32);
        let eye_color = winutil::Palette::for_dark(is_dark).text_secondary;
        app.eye_on = winutil::rasterize_eye_on(app.s(40) as u32, eye_color);
        app.eye_off = winutil::rasterize_eye_off(app.s(40) as u32, eye_color);
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("STATIC"),
            w!(""),
            WS_POPUP,
            0,
            0,
            app.s(CLIENT_W),
            app.s(CLIENT_H),
            None,
            None,
            None,
            None,
        )
        .unwrap();
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, (&mut *app as *mut App) as isize);
        Self { hwnd, app }
    }
    fn full(&self) -> RECT {
        RECT {
            left: 0,
            top: 0,
            right: self.app.s(CLIENT_W),
            bottom: self.app.s(CLIENT_H),
        }
    }
}

unsafe extern "system" fn fixture_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    use windows::Win32::UI::WindowsAndMessaging::*;
    if matches!(
        msg,
        WM_DRAWITEM | WM_CTLCOLOREDIT | WM_CTLCOLORSTATIC | WM_COMMAND
    ) {
        wndproc(hwnd, msg, wp, lp)
    } else {
        DefWindowProcW(hwnd, msg, wp, lp)
    }
}
impl Drop for Scene {
    fn drop(&mut self) {
        unsafe {
            let _ = KillTimer(Some(self.hwnd), TIMER_ANIM);
            SetWindowLongPtrW(self.hwnd, GWLP_USERDATA, 0);
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

struct Canvas {
    bmp: winutil::SvgBmp,
    dc: HDC,
    old: HGDIOBJ,
}
impl Canvas {
    unsafe fn new(w: i32, h: i32) -> Self {
        let svg = format!(r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}"/>"#);
        let bmp = winutil::rasterize_svg(svg.as_bytes(), w.max(h) as u32).unwrap();
        let dc = CreateCompatibleDC(None);
        let old = SelectObject(dc, HGDIOBJ(bmp.hbmp.0));
        Self { bmp, dc, old }
    }
    unsafe fn pixels(&self) -> Vec<u8> {
        let _ = GdiFlush();
        std::slice::from_raw_parts(self.bmp.bits, (self.bmp.w * self.bmp.h * 4) as usize).to_vec()
    }
    unsafe fn save_png(&self, path: &std::path::Path) {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let mut pixels = self.pixels();
        for p in pixels.chunks_exact_mut(4) {
            p.swap(0, 2);
            p[3] = 255;
        }
        let pixmap = resvg::tiny_skia::Pixmap::from_vec(
            pixels,
            resvg::tiny_skia::IntSize::from_wh(self.bmp.w as u32, self.bmp.h as u32).unwrap(),
        )
        .unwrap();
        pixmap.save_png(path).unwrap();
    }
}
impl Drop for Canvas {
    fn drop(&mut self) {
        unsafe {
            let _ = SelectObject(self.dc, self.old);
            let _ = DeleteDC(self.dc);
        }
    }
}

#[test]
fn partial_animation_frames_match_full_paint_at_multiple_dpis() {
    unsafe {
        for dpi in [96, 120, 144, 192] {
            let mut scene = Scene::new(dpi);
            let full = scene.full();
            let partial = Canvas::new(full.right, full.bottom);
            let reference = Canvas::new(full.right, full.bottom);
            paint(scene.hwnd, partial.dc, full);
            for t in [0.125, 0.5, 0.875, 1.0, 0.0] {
                for i in 0..4 {
                    scene.app.toggle_anim[i] = Anim::snap(t);
                    paint(scene.hwnd, partial.dc, toggle_rect(&scene.app, i));
                }
                scene.app.combo_visual = Anim::snap(t);
                let combo = RECT {
                    left: scene.app.s(layout::FIELD_LEFT) - 2,
                    top: scene.app.s(layout::COMBO_TOP) - 2,
                    right: scene.app.s(layout::FIELD_RIGHT) + 2,
                    bottom: scene.app.s(layout::COMBO_TOP + layout::FIELD_HEIGHT) + 2,
                };
                paint(scene.hwnd, partial.dc, combo);
                paint(scene.hwnd, reference.dc, full);
                let a = partial.pixels();
                let b = reference.pixels();
                let different = a.iter().zip(&b).filter(|(x, y)| x != y).count();
                if different != 0 {
                    let points: Vec<_> = a
                        .chunks_exact(4)
                        .zip(b.chunks_exact(4))
                        .enumerate()
                        .filter(|(_, (x, y))| x != y)
                        .map(|(i, (x, y))| (i % full.right as usize, i / full.right as usize, x, y))
                        .collect();
                    eprintln!("first differences: {:?}", &points[..points.len().min(12)]);
                    eprintln!(
                        "difference bounds: {:?}",
                        (
                            points.iter().map(|p| p.0).min(),
                            points.iter().map(|p| p.1).min(),
                            points.iter().map(|p| p.0).max(),
                            points.iter().map(|p| p.1).max()
                        )
                    );
                }
                assert_eq!(
                    different, 0,
                    "partial repaint left stale/different pixels: DPI={dpi}, t={t}"
                );
            }
        }
    }
}

#[test]
fn popup_cache_preserves_alpha_and_removes_previous_highlight() {
    unsafe {
        for dpi in [96, 144, 192] {
            let mut scene = Scene::new(dpi);
            let dc = CreateCompatibleDC(None);
            let mut surface =
                PopupSurface::new(&scene.app, scene.app.s(368), scene.app.s(116)).unwrap();
            surface.update(&scene.app, dc);
            let bitmap = surface.bmp.hbmp;
            let initial =
                std::slice::from_raw_parts(surface.bmp.bits, surface.background.len()).to_vec();
            scene.app.combo_visual = Anim::snap(0.25);
            surface.update(&scene.app, dc);
            assert_eq!(surface.bmp.hbmp, bitmap);
            assert_eq!(
                std::slice::from_raw_parts(surface.bmp.bits, initial.len()),
                initial
            );
            scene.app.combo_hot = 1;
            surface.update(&scene.app, dc);
            assert_ne!(
                std::slice::from_raw_parts(surface.bmp.bits, initial.len()),
                initial
            );
            scene.app.combo_hot = -1;
            surface.update(&scene.app, dc);
            assert_eq!(
                std::slice::from_raw_parts(surface.bmp.bits, initial.len()),
                initial
            );
            for pixel in initial.chunks_exact(4) {
                assert!(
                    pixel[..3].iter().all(|c| *c <= pixel[3]),
                    "invalid premultiplied alpha"
                );
            }
            let _ = DeleteDC(dc);
        }
    }
}

#[test]
fn repeated_close_does_not_restart_and_final_tick_stops_timer() {
    unsafe {
        let mut scene = Scene::new(96);
        scene.app.combo_open = true;
        scene.app.combo_visual = Anim::snap(1.0);
        close_combo(scene.hwnd);
        scene
            .app
            .combo_visual
            .advance(Instant::now() + std::time::Duration::from_millis(40));
        let closing_value = scene.app.combo_visual.value();
        close_combo(scene.hwnd);
        assert_eq!(scene.app.combo_visual.value(), closing_value);
        scene
            .app
            .combo_visual
            .advance(Instant::now() + std::time::Duration::from_secs(1));
        on_anim_timer(scene.hwnd);
        assert!(scene.app.combo_visual.done());
        assert!(!scene.app.anim_timer_running);
        assert!(!scene.app.combo_open);
        assert!(popup_item_at(scene.hwnd, scene.hwnd, 30, 30).is_none());
        dismiss_combo(scene.hwnd);
        assert_eq!(scene.app.combo_visual.value(), 0.0);
    }
}

#[test]
fn layered_submission_moves_cached_surface_without_showing_window() {
    unsafe {
        use windows::Win32::UI::WindowsAndMessaging::{GetWindowRect, IsWindowVisible};
        let mut scene = Scene::new(144);
        let popup = ensure_combo_popup(scene.hwnd);
        assert!(!popup.0.is_null());
        let mut bitmap = None;
        for (i, t) in [0.0, 0.25, 0.5, 1.0, 0.0].into_iter().enumerate() {
            scene.app.combo_visual = Anim::snap(t);
            let position = POINT {
                x: 20,
                y: 20 + i as i32,
            };
            let w = scene.app.s(368);
            let h = scene.app.s(116);
            paint_combo_popup(popup, scene.hwnd, position, w, h);
            let mut rect = RECT::default();
            GetWindowRect(popup, &mut rect).unwrap();
            assert_eq!(
                (rect.left, rect.top, rect.right, rect.bottom),
                (20, position.y, 20 + w, position.y + h)
            );
            assert!(!IsWindowVisible(popup).as_bool());
            let handle = scene.app.popup_surface.as_ref().unwrap().bmp.hbmp;
            if let Some(previous) = bitmap {
                assert_eq!(handle, previous);
            }
            bitmap = Some(handle);
        }
    }
}

#[test]
#[ignore = "explicit offscreen release rendering benchmark and PNG artifacts"]
fn render_animation_artifacts_and_timings() {
    unsafe {
        let directory = std::env::var_os("DRCOM_UI_ARTIFACT_DIR").expect("set artifact directory");
        let directory = std::path::PathBuf::from(directory);
        std::fs::create_dir_all(&directory).unwrap();
        for dpi in [96, 144, 192] {
            let mut scene = Scene::new(dpi);
            let full = scene.full();
            let canvas = Canvas::new(full.right, full.bottom);
            paint(scene.hwnd, canvas.dc, full);
            let mut full_us = Vec::new();
            let mut partial_us = Vec::new();
            for frame in 0..60 {
                scene.app.toggle_anim[0] = Anim::snap(frame as f32 / 59.0);
                let start = Instant::now();
                paint(scene.hwnd, canvas.dc, full);
                let _ = GdiFlush();
                full_us.push(start.elapsed().as_micros());
                let start = Instant::now();
                paint(scene.hwnd, canvas.dc, toggle_rect(&scene.app, 0));
                let _ = GdiFlush();
                partial_us.push(start.elapsed().as_micros());
            }
            let mut popup_new = Vec::new();
            let mut popup_cached = Vec::new();
            let mut surface =
                PopupSurface::new(&scene.app, scene.app.s(368), scene.app.s(116)).unwrap();
            surface.update(&scene.app, canvas.dc);
            for _ in 0..20 {
                let start = Instant::now();
                let mut cold = PopupSurface::new(&scene.app, surface.bmp.w, surface.bmp.h).unwrap();
                cold.update(&scene.app, canvas.dc);
                popup_new.push(start.elapsed().as_micros());
                let start = Instant::now();
                surface.update(&scene.app, canvas.dc);
                popup_cached.push(start.elapsed().as_micros());
            }
            full_us.sort();
            partial_us.sort();
            popup_new.sort();
            popup_cached.sort();
            eprintln!(
                "DPI {dpi}: paint median/p95 full {}/{} us, partial {}/{} us; popup raster median {} us, cache lookup {} us (excludes compositor)",
                full_us[30],
                full_us[57],
                partial_us[30],
                partial_us[57],
                popup_new[10],
                popup_cached[10]
            );
            for (frame, t) in [0.0, 0.25, 0.5, 0.75, 1.0].into_iter().enumerate() {
                scene.app.toggle_anim = [Anim::snap(t); 4];
                scene.app.combo_visual = Anim::snap(t);
                paint(scene.hwnd, canvas.dc, full);
                let mut pixels = canvas.pixels();
                for p in pixels.chunks_exact_mut(4) {
                    p.swap(0, 2);
                    p[3] = 255;
                }
                let pixmap = resvg::tiny_skia::Pixmap::from_vec(
                    pixels,
                    resvg::tiny_skia::IntSize::from_wh(full.right as u32, full.bottom as u32)
                        .unwrap(),
                )
                .unwrap();
                pixmap
                    .save_png(directory.join(format!("ui-{dpi}-{frame}.png")))
                    .unwrap();
            }
            let mut pixels =
                std::slice::from_raw_parts(surface.bmp.bits, surface.background.len()).to_vec();
            for p in pixels.chunks_exact_mut(4) {
                p.swap(0, 2);
            }
            let pixmap = resvg::tiny_skia::Pixmap::from_vec(
                pixels,
                resvg::tiny_skia::IntSize::from_wh(surface.bmp.w as u32, surface.bmp.h as u32)
                    .unwrap(),
            )
            .unwrap();
            pixmap
                .save_png(directory.join(format!("popup-{dpi}.png")))
                .unwrap();
        }
    }
}

#[test]
#[ignore = "explicit offscreen main-window snapshot rendering and PNG artifacts"]
fn render_heroui_main_window_snapshots() {
    unsafe {
        let out = std::path::PathBuf::from(
            std::env::var_os("DRCOM_UI_ARTIFACT_DIR").expect("set artifact directory"),
        );
        std::fs::create_dir_all(&out).unwrap();
        let save = |canvas: &Canvas, name: String| {
            let path = out.join(name);
            canvas.save_png(&path);
            winutil::golden_review(&path);
        };
        for (theme, dark) in [("light", false), ("dark", true)] {
            for dpi in [96, 120, 144, 192] {
                for (name, state, title, detail, desired) in [
                    (
                        "offline",
                        LinkState::Offline,
                        "未连接",
                        "输入校园网账号，开始连接。",
                        false,
                    ),
                    (
                        "online",
                        LinkState::Online,
                        "校园网络已连接",
                        "认证成功，可以开始使用校园网络。",
                        true,
                    ),
                    (
                        "connecting",
                        LinkState::Connecting,
                        "正在连接…",
                        "正在验证账号与网络…",
                        true,
                    ),
                    (
                        "waiting",
                        LinkState::Waiting,
                        "等待网络就绪",
                        "请检查网线连接，客户端正在等待网络。",
                        true,
                    ),
                    (
                        "degraded",
                        LinkState::Degraded,
                        "正在恢复连接",
                        "网络暂时不稳定，正在尝试恢复。",
                        true,
                    ),
                    (
                        "error",
                        LinkState::Error,
                        "暂时无法连接",
                        "请检查账号、密码与所选网卡后重试。",
                        false,
                    ),
                ] {
                    let mut scene = Scene::new_themed(dpi, dark);
                    scene.app.link_state = state;
                    scene.app.status_title = title.into();
                    scene.app.status_detail = detail.into();
                    scene.app.desired_running = desired;
                    scene.app.settings.remember_password = true;
                    scene.app.settings.auto_login = true;
                    scene.app.settings.minimize_to_tray = true;
                    snap_toggle_anims(&mut scene.app);
                    scene.attach_native_controls();
                    let full = scene.full();
                    let canvas = Canvas::new(full.right, full.bottom);
                    scene.render_native(&canvas);
                    save(&canvas, format!("main-{theme}-{dpi}dpi-{name}.png"));
                    if name == "offline" {
                        v3::switch_page(scene.hwnd, true);
                        scene.render_native(&canvas);
                        save(&canvas, format!("main-{theme}-{dpi}dpi-settings.png"));
                        v3::switch_page(scene.hwnd, false);
                        scene.app.combo_sel = 1;
                        let combo = scene
                            .app
                            .buttons
                            .iter()
                            .find(|(hit, _)| *hit == Hit::Combo)
                            .unwrap()
                            .1;
                        let _ = windows::Win32::UI::Input::KeyboardAndMouse::SetFocus(Some(combo));
                        for (variant, open) in [("combo-focused", false), ("combo-open", true)] {
                            // Capture the selector's focused/open state without
                            // showing a real adapter popup over the test desktop.
                            scene.app.combo_open = open;
                            scene.app.combo_visual = Anim::snap(if open { 1.0 } else { 0.0 });
                            scene.render_native(&canvas);
                            save(&canvas, format!("main-{theme}-{dpi}dpi-{variant}.png"));
                        }
                        click_background(&scene);
                        // Capture the settled state after the popup's closing fade.
                        scene.app.combo_visual = Anim::snap(0.0);
                        scene.render_native(&canvas);
                        save(
                            &canvas,
                            format!("main-{theme}-{dpi}dpi-background-click.png"),
                        );
                        // Hover states: combo field and primary action.
                        for (variant, hit) in
                            [("combo-hover", Hit::Combo), ("connect-hover", Hit::Connect)]
                        {
                            scene.app.hover = hit;
                            scene.render_native(&canvas);
                            save(&canvas, format!("main-{theme}-{dpi}dpi-{variant}.png"));
                        }
                        scene.app.hover = Hit::None;
                    }
                }
            }
        }
    }
}

unsafe fn click_background(scene: &Scene) {
    let x = scene.app.s(18);
    let y = scene.app.s(580);
    wndproc(
        scene.hwnd,
        WM_LBUTTONDOWN,
        WPARAM(0),
        LPARAM(((y << 16) | x) as isize),
    );
}

#[test]
fn background_click_clears_native_focus_and_tab_can_restore_it() {
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetFocus, SetFocus, VK_TAB};
    use windows::Win32::UI::WindowsAndMessaging::{IsDialogMessageW, GWL_STYLE, MSG};
    unsafe {
        for dark in [false, true] {
            for dpi in [96, 120, 144, 192] {
                let mut scene = Scene::new_themed(dpi, dark);
                scene.attach_native_controls();
                let combo = scene
                    .app
                    .buttons
                    .iter()
                    .find(|(h, _)| *h == Hit::Combo)
                    .unwrap()
                    .1;
                let full = scene.full();
                let canvas = Canvas::new(full.right, full.bottom);
                let _ = SetFocus(Some(scene.hwnd));
                scene.render_native(&canvas);
                let unfocused = canvas.pixels();
                for preferences in [false, true] {
                    v3::switch_page(scene.hwnd, preferences);
                    let _ = SetFocus(Some(scene.hwnd));
                    scene.render_native(&canvas);
                    let baseline = canvas.pixels();
                    for (_, child) in &scene.app.buttons {
                        if GetWindowLongPtrW(*child, GWL_STYLE) & WS_VISIBLE.0 as isize == 0 {
                            continue;
                        }
                        let _ = SetFocus(Some(*child));
                        assert_eq!(GetFocus(), *child);
                        scene.render_native(&canvas);
                        assert!(
                            canvas.pixels() == baseline,
                            "non-input control must not gain a focus outline at {dpi} DPI"
                        );
                    }
                }
                v3::switch_page(scene.hwnd, false);
                for child in [scene.app.hwnd_user, scene.app.hwnd_pass, combo] {
                    let _ = SetFocus(Some(child));
                    assert_eq!(GetFocus(), child);
                    scene.app.combo_open = child == combo;
                    click_background(&scene);
                    assert_eq!(GetFocus(), scene.hwnd);
                    assert!(!scene.app.combo_open);
                    scene.render_native(&canvas);
                    assert_eq!(
                        canvas.pixels(),
                        unfocused,
                        "stale focus border after background click"
                    );
                }
                assert_eq!(edit_text(scene.app.hwnd_user), "202600000001");
                assert_eq!(edit_text(scene.app.hwnd_pass), "campus-demo");
                let message = MSG {
                    hwnd: scene.hwnd,
                    message: WM_KEYDOWN,
                    wParam: WPARAM(VK_TAB.0 as usize),
                    ..Default::default()
                };
                assert!(IsDialogMessageW(scene.hwnd, &message).as_bool());
                assert!(scene
                    .app
                    .buttons
                    .iter()
                    .any(|(_, child)| *child == GetFocus()));
                scene.render_native(&canvas);
                assert_eq!(
                    canvas.pixels(),
                    unfocused,
                    "Tab keeps buttons accessible without drawing a blue outline"
                );
                v3::switch_page(scene.hwnd, true);
                let setting = scene
                    .app
                    .buttons
                    .iter()
                    .find(|(h, _)| *h == Hit::Auto)
                    .unwrap()
                    .1;
                let _ = SetFocus(Some(setting));
                click_background(&scene);
                assert_eq!(GetFocus(), scene.hwnd);
                assert!(!scene.app.settings.auto_login);
            }
        }
    }
}

#[test]
fn native_pages_keep_edit_contents_and_setting_commands_at_multiple_dpis() {
    use windows::Win32::UI::WindowsAndMessaging::*;
    unsafe {
        for dpi in [96, 120, 144, 192] {
            let mut scene = Scene::new_themed(dpi, false);
            scene.attach_native_controls();
            let user = scene.app.hwnd_user;
            let pass = scene.app.hwnd_pass;
            SendMessageW(
                GetDlgItem(Some(scene.hwnd), 1101).unwrap(),
                0x00f5,
                None,
                None,
            ); // BM_CLICK
            assert!(scene.app.preferences);
            assert_eq!(
                GetWindowLongPtrW(user, GWL_STYLE) & WS_VISIBLE.0 as isize,
                0
            );
            assert_eq!(
                hit_test(&scene.app, scene.app.s(100), scene.app.s(layout::USER_TOP)),
                Hit::Auto
            );
            let old = scene.app.settings.auto_login;
            SendMessageW(
                GetDlgItem(Some(scene.hwnd), 1105).unwrap(),
                0x00f5,
                None,
                None,
            );
            assert_ne!(scene.app.settings.auto_login, old);
            SendMessageW(
                GetDlgItem(Some(scene.hwnd), 1100).unwrap(),
                0x00f5,
                None,
                None,
            );
            assert!(!scene.app.preferences);
            assert_ne!(
                GetWindowLongPtrW(user, GWL_STYLE) & WS_VISIBLE.0 as isize,
                0
            );
            assert_eq!(scene.app.hwnd_user, user);
            assert_eq!(scene.app.hwnd_pass, pass);
            let mut value = [0u16; 64];
            let n = GetWindowTextW(pass, &mut value) as usize;
            assert_eq!(String::from_utf16_lossy(&value[..n]), "campus-demo");
            assert_eq!(SendMessageW(pass, 0x00d2, None, None).0, 0x2022);
            let combo = scene
                .app
                .buttons
                .iter()
                .find(|(h, _)| *h == Hit::Combo)
                .unwrap()
                .1;
            SendMessageW(combo, WM_KEYDOWN, Some(WPARAM(40)), None);
            assert!(scene.app.combo_open);
            assert_eq!(scene.app.combo_hot, 1);
            SendMessageW(combo, WM_KEYDOWN, Some(WPARAM(13)), None);
            assert!(!scene.app.combo_open);
            assert_eq!(scene.app.combo_sel, 1);
            assert!(!IsWindowVisible(scene.hwnd).as_bool());
        }
    }
}

#[test]
fn buffered_surface_publishes_once_and_preserves_pixels_outside_damage() {
    use crate::ui::hero;
    use windows::Win32::Foundation::COLORREF;
    unsafe {
        let canvas = Canvas::new(80, 60);
        let full = RECT {
            left: 0,
            top: 0,
            right: 80,
            bottom: 60,
        };
        hero::line(canvas.dc, full, COLORREF(0xFFFFFF));
        let before = canvas.pixels();
        let r = RECT {
            left: 13,
            top: 9,
            right: 65,
            bottom: 47,
        };
        winutil::paint_buffered(canvas.dc, r, |mem| {
            hero::line(mem, r, COLORREF(0));
            // Background clearing and content drawing must stay invisible until
            // the completed frame is copied; this catches direct-HDC painting.
            assert_eq!(canvas.pixels(), before);
            hero::icon(
                mem,
                RECT {
                    left: 20,
                    top: 14,
                    right: 44,
                    bottom: 38,
                },
                COLORREF(0xFFFFFF),
                hero::Icon::Check,
            );
            assert_eq!(canvas.pixels(), before);
        });
        let after = canvas.pixels();
        let mut changed = false;
        for (i, (a, b)) in before
            .chunks_exact(4)
            .zip(after.chunks_exact(4))
            .enumerate()
        {
            let (x, y) = ((i % 80) as i32, (i / 80) as i32);
            if x < r.left || x >= r.right || y < r.top || y >= r.bottom {
                assert_eq!(a, b);
            } else {
                changed |= a != b;
            }
        }
        assert!(changed);
    }
}

#[test]
fn native_hover_and_animation_do_not_repaint_unchanged_siblings() {
    use windows::Win32::Graphics::Gdi::{GetUpdateRect, ValidateRect};
    use windows::Win32::UI::WindowsAndMessaging::*;
    unsafe {
        let mut scene = Scene::new(96);
        scene.attach_native_controls();
        // A hidden ancestor suppresses Win32 update regions. Keep this test
        // surface outside the desktop without activating it or running the app.
        SetWindowPos(
            scene.hwnd,
            None,
            -30000,
            -30000,
            0,
            0,
            SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_SHOWWINDOW,
        )
        .unwrap();
        let clear = |scene: &Scene| {
            let _ = ValidateRect(Some(scene.hwnd), None);
            for (_, child) in &scene.app.buttons {
                let _ = ValidateRect(Some(*child), None);
            }
        };
        let button =
            |scene: &Scene, hit| scene.app.buttons.iter().find(|(h, _)| *h == hit).unwrap().1;
        let connect = button(&scene, Hit::Connect);
        SendMessageW(
            connect,
            WM_MOUSEMOVE,
            Some(WPARAM(0)),
            Some(LPARAM(5 | (5 << 16))),
        );
        clear(&scene);
        for x in 6..46 {
            SendMessageW(
                connect,
                WM_MOUSEMOVE,
                Some(WPARAM(0)),
                Some(LPARAM(x | (5 << 16))),
            );
            assert!(
                !GetUpdateRect(connect, None, false).as_bool(),
                "hover motion without a state change must not repaint"
            );
        }
        v3::switch_page(scene.hwnd, true);
        clear(&scene);
        on_anim_timer(scene.hwnd);
        for (_, child) in &scene.app.buttons {
            assert!(
                !GetUpdateRect(*child, None, false).as_bool(),
                "idle frame invalidated a control"
            );
        }
        scene.app.toggle_anim[0] = Anim::go(0.0, 1.0, 10_000);
        on_anim_timer(scene.hwnd);
        assert!(GetUpdateRect(button(&scene, Hit::Auto), None, false).as_bool());
        for (hit, child) in &scene.app.buttons {
            if *hit != Hit::Auto {
                assert!(
                    !GetUpdateRect(*child, None, false).as_bool(),
                    "toggle frame invalidated {hit:?}"
                );
            }
        }
        assert!(
            !GetUpdateRect(scene.hwnd, None, false).as_bool(),
            "child animation must not repaint the parent"
        );
    }
}

#[test]
fn icons_chevrons_and_focus_rings_have_antialiased_coverage_at_every_dpi() {
    use crate::ui::hero;
    use windows::Win32::Foundation::COLORREF;
    unsafe {
        for dpi in [96, 120, 144, 192] {
            for shape in 0..3 {
                let r = hero::rect(dpi, 0, 0, 60, 40);
                let canvas = Canvas::new(r.right, r.bottom);
                winutil::paint_buffered(canvas.dc, r, |dc| {
                    hero::line(dc, r, COLORREF(0xFFFFFF));
                    match shape {
                        0 => hero::icon(
                            dc,
                            hero::rect(dpi, 10, 6, 24, 24),
                            COLORREF(0),
                            hero::Icon::User,
                        ),
                        1 => hero::chevron(dc, hero::rect(dpi, 10, 6, 24, 24), COLORREF(0), 0.25),
                        _ => hero::field(
                            dc,
                            r,
                            dpi,
                            winutil::Palette {
                                accent: COLORREF(0),
                                ..winutil::Palette::for_dark(false)
                            },
                            hero::FieldState::Focus,
                        ),
                    }
                });
                let pixels = canvas.pixels();
                let coverage = pixels
                    .chunks_exact(4)
                    .filter(|p| p[0] > 0 && p[0] < 255 && p[0] == p[1] && p[1] == p[2])
                    .count();
                assert!(
                    coverage > 8,
                    "shape {shape} has no antialiased contour at {dpi} DPI"
                );
            }
        }
    }
}

#[test]
fn native_adapter_has_no_blue_border_when_focused_or_open() {
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetFocus, SetFocus};
    unsafe {
        for dark in [false, true] {
            for dpi in [96, 120, 144, 192] {
                let mut scene = Scene::new_themed(dpi, dark);
                scene.attach_native_controls();
                let combo = scene
                    .app
                    .buttons
                    .iter()
                    .find(|(hit, _)| *hit == Hit::Combo)
                    .unwrap()
                    .1;
                let full = scene.full();
                let canvas = Canvas::new(full.right, full.bottom);
                let p = winutil::Palette::for_dark(dark);
                let accent = [
                    ((p.accent.0 >> 16) & 255) as i32,
                    ((p.accent.0 >> 8) & 255) as i32,
                    (p.accent.0 & 255) as i32,
                ];
                for (focused, open) in [(false, false), (true, false), (true, true), (false, true)]
                {
                    let _ = SetFocus(Some(if focused { combo } else { scene.app.hwnd_user }));
                    assert_eq!(GetFocus() == combo, focused);
                    scene.app.combo_open = open;
                    scene.app.combo_visual = Anim::snap(if open { 1.0 } else { 0.0 });
                    scene.render_native(&canvas);
                    let pixels = canvas.pixels();
                    let x = scene.app.s(240);
                    for (from, to) in [
                        (layout::COMBO_TOP - 2, layout::COMBO_TOP + 10),
                        (layout::COMBO_TOP + 34, layout::COMBO_TOP + 48),
                    ] {
                        let mut runs = 0;
                        let mut was_accent = false;
                        for y in scene.app.s(from)..scene.app.s(to) {
                            let at = ((y * full.right + x) * 4) as usize;
                            let blue =
                                (0..3).all(|c| (pixels[at + c] as i32 - accent[c]).abs() < 24);
                            if blue && !was_accent {
                                runs += 1;
                            }
                            was_accent = blue;
                        }
                        assert_eq!(
                            runs, 0,
                            "unexpected selector outline: dpi={dpi}, dark={dark}, focused={focused}, open={open}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn focused_input_draws_a_complete_outer_focus_ring() {
    use windows::Win32::UI::Input::KeyboardAndMouse::SetFocus;
    unsafe {
        for dark in [false, true] {
            for dpi in [96, 120, 144, 192] {
                let mut scene = Scene::new_themed(dpi, dark);
                scene.attach_native_controls();
                let _ = SetFocus(Some(scene.app.hwnd_user));
                scene.app.connect_enabled = true;
                let full = scene.full();
                let canvas = Canvas::new(full.right, full.bottom);
                scene.render_native(&canvas);
                let pixels = canvas.pixels();
                let color = winutil::Palette::for_dark(dark).accent.0;
                let accent = [
                    ((color >> 16) & 255) as i32,
                    ((color >> 8) & 255) as i32,
                    (color & 255) as i32,
                ];
                let is_accent = |x: i32, y: i32| {
                    let at = ((y * full.right + x) * 4) as usize;
                    (0..3).all(|c| (pixels[at + c] as i32 - accent[c]).abs() < 32)
                };
                // The v3 ring is a 2 DIP band OUTSIDE the field edge.
                let y = scene.app.s(layout::USER_TOP + 22);
                let field_left = scene.app.s(30);
                let field_right = scene.app.s(450);
                let ring = scene.app.s(2).max(1);
                for dx in 1..=ring {
                    assert!(
                        is_accent(field_left - dx, y),
                        "missing left outer ring at {dpi} DPI dx={dx}"
                    );
                    assert!(
                        is_accent(field_right + dx - 1, y),
                        "missing right outer ring at {dpi} DPI dx={dx}"
                    );
                }
                // No accent may bleed into the field interior at mid-height.
                assert!(
                    !is_accent(field_left + ring + 1, y),
                    "ring must stay outside the field at {dpi} DPI"
                );
                // Top and bottom edges mirror the sides.
                let x = scene.app.s(240);
                let field_top = scene.app.s(layout::USER_TOP);
                let field_bottom = scene.app.s(layout::USER_TOP + 44);
                for dy in 1..=ring {
                    assert!(
                        is_accent(x, field_top - dy),
                        "missing top ring at {dpi} DPI"
                    );
                    assert!(
                        is_accent(x, field_bottom + dy - 1),
                        "missing bottom ring at {dpi} DPI"
                    );
                }
            }
        }
    }
}
