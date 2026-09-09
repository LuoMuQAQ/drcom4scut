//! Offscreen regression tests: no client initialization, visible UI or network.
use super::*;
use windows::Win32::Graphics::Gdi::{GdiFlush, HGDIOBJ};

// An invisible STATIC window supplies client geometry only.
struct Scene {
    hwnd: HWND,
    app: Box<App>,
}
impl Scene {
    unsafe fn new(dpi: u32) -> Self {
        let mut app = Box::new(App::new());
        app.preview = true;
        app.dpi = dpi;
        for (font, size, bold) in [
            (&mut app.font, 14, false),
            (&mut app.font_title, 20, true),
            (&mut app.font_label, 12, false),
            (&mut app.font_btn, 14, true),
        ] {
            delete_gdi(font_as_gdi(*font));
            *font = create_font(size, dpi, bold);
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
                    left: scene.app.s(36) - 2,
                    top: scene.app.s(layout::COMBO_TOP) - 2,
                    right: scene.app.s(384) + 2,
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
            eprintln!("DPI {dpi}: paint median/p95 full {}/{} us, partial {}/{} us; popup raster median {} us, cache lookup {} us (excludes compositor)",
            full_us[30], full_us[57], partial_us[30], partial_us[57], popup_new[10], popup_cached[10]);
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
