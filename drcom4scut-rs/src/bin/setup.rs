#![windows_subsystem = "windows"]
//! Native installer: responsive unelevated UI, elevated file/driver worker.
use drcom4scut_gui::install::{
    acl, driver, driver_flow, flow, identity, knownfolder, origin, process, sid,
};
use flow::{InstallOptions, InstallResult};
use std::path::{Path, PathBuf};
use std::time::Duration;
use windows::core::{w, PCWSTR};

#[path = "../install/setup_window.rs"]
mod setup_window;

fn relaunch_elevated(args: &[String]) -> Result<(), String> {
    use windows::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
    use windows::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject};
    use windows::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let executable = drcom4scut_gui::ui::winutil::wide(&exe.to_string_lossy());
    let arguments = args
        .iter()
        .map(|a| quote_arg(a))
        .collect::<Vec<_>>()
        .join(" ");
    let parameters = drcom4scut_gui::ui::winutil::wide(&arguments);
    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS,
        lpVerb: w!("runas"),
        lpFile: PCWSTR(executable.as_ptr()),
        lpParameters: PCWSTR(parameters.as_ptr()),
        nShow: 1,
        ..Default::default()
    };
    unsafe {
        ShellExecuteExW(&mut info).map_err(|e| {
            if e.code().0 as u32 == 0x800704c7 {
                "已取消管理员授权，尚未开始安装。".into()
            } else {
                format!("无法启动安装：{e}")
            }
        })?;
        if info.hProcess.is_invalid() {
            return Err("安装进程没有返回句柄。".into());
        }
        let wait = WaitForSingleObject(info.hProcess, u32::MAX);
        let mut code = 0;
        let result = GetExitCodeProcess(info.hProcess, &mut code);
        let _ = CloseHandle(info.hProcess);
        if wait != WAIT_OBJECT_0 || result.is_err() {
            return Err("无法读取安装进程结果。".into());
        }
        if code != 0 {
            return Err(format!("安装进程未正常完成（{code}）。"));
        }
    }
    Ok(())
}

fn quote_arg(arg: &str) -> String {
    let mut out = String::from("\"");
    let mut slashes = 0;
    for c in arg.chars() {
        if c == '\\' {
            slashes += 1;
            continue;
        }
        out.extend(std::iter::repeat_n(
            '\\',
            if c == '"' { slashes * 2 + 1 } else { slashes },
        ));
        out.push(c);
        slashes = 0;
    }
    out.extend(std::iter::repeat_n('\\', slashes * 2));
    out.push('"');
    out
}

fn write_options(dir: &Path, opts: &InstallOptions, driver_only: bool) -> Result<(), String> {
    let value = serde_json::json!({ "installDir": opts.install_dir.to_string_lossy(),
        "desktopShortcut": opts.desktop_shortcut, "startMenuShortcut": opts.start_menu_shortcut,
        "launchAfter": opts.launch_after, "driverOnly": driver_only });
    std::fs::write(dir.join("options.json"), value.to_string()).map_err(|e| e.to_string())
}

fn read_options(dir: &Path) -> Result<(InstallOptions, bool), String> {
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Options {
        install_dir: PathBuf,
        desktop_shortcut: bool,
        start_menu_shortcut: bool,
        launch_after: bool,
        #[serde(default)]
        driver_only: bool,
    }
    let raw = std::fs::read(dir.join("options.json")).map_err(|e| e.to_string())?;
    let o: Options = serde_json::from_slice(&raw).map_err(|e| e.to_string())?;
    let mut options = InstallOptions {
        install_dir: o.install_dir,
        desktop_shortcut: o.desktop_shortcut,
        start_menu_shortcut: o.start_menu_shortcut,
        launch_after: o.launch_after,
        ..Default::default()
    };
    let system = std::env::var_os("SystemRoot")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
    options.install_dir = flow::validate_options(
        &options,
        &system,
        knownfolder::program_files_x86().ok().as_deref(),
    )
    .map_err(|e| e.message())?;
    Ok((options, o.driver_only))
}

fn progress(dir: &Path, percent: u32, message: &str) {
    origin::write_status(
        dir,
        &serde_json::json!({"percent":percent,"message":message}).to_string(),
    );
}

fn worker_main(dir: PathBuf, nonce: String) -> i32 {
    if !sid::is_elevated() {
        return 1;
    }
    // Never write a result to a directory that failed validation.
    let source = match origin::verify_origin(&dir, &nonce) {
        Ok(o) => o,
        Err(_) => return 1,
    };
    let action = || -> Result<InstallResult, String> {
        let _guard = drcom4scut_gui::install::maintenance::acquire()?;
        let (opts, driver_only) = read_options(&dir)?;
        let registered = drcom4scut_gui::install::registry::registered_install_dir()?;
        drcom4scut_gui::install::registry::enforce_single_install(
            &opts.install_dir,
            registered.as_deref(),
        )?;
        if origin::cancel_requested(&dir) {
            return Err("安装已取消。".into());
        }
        if driver_only {
            let state = identity::load_state(&opts.install_dir.join("install-state.json"))?;
            if !identity::state_matches_dir(&state, &opts.install_dir) {
                return Err("安装位置已改变，请重新运行安装器。".into());
            }
        } else {
            progress(&dir, 8, "正在准备安装文件…");
            let payload = flow::default_payload()?;
            process::stop_owned(&opts.install_dir, Duration::from_secs(8))?;
            if origin::cancel_requested(&dir) {
                return Err("安装已取消。".into());
            }
            progress(&dir, 16, "正在清理旧安装残留…");
            flow::prepare_install_dir(&opts.install_dir)?;
            progress(&dir, 25, "正在安装校园网客户端…");
            flow::install_files(&opts, &source, &payload, false)?;
            progress(&dir, 58, "正在创建快捷方式和卸载入口…");
            flow::create_shortcuts(&opts)?;
            flow::register_uninstall(&opts, &payload)?;
        }
        // Executed as administrator: cache must not be replaceable by normal users.
        let cache = opts.install_dir.join(format!(".npcap-setup-{nonce}"));
        acl::create_dir_with_sddl(&cache, &acl::data_container_sddl())?;
        let (driver, need_reboot, message) = driver_flow::ensure_driver(
            &driver::RealDriverHost,
            &opts,
            &cache,
            &mut |p| progress(&dir, p.percent, &p.message),
            &|| origin::cancel_requested(&dir),
        );
        let _ = std::fs::remove_dir_all(&cache);
        let launch = opts.launch_after
            && driver_flow::ready(&driver)
            && !need_reboot
            && !origin::cancel_requested(&dir);
        Ok(InstallResult {
            ok: true,
            install_dir: opts.install_dir.to_string_lossy().into_owned(),
            version: env!("CARGO_PKG_VERSION").into(),
            driver,
            need_reboot,
            message,
            launch,
        })
    };
    let result = action().unwrap_or_else(|message| InstallResult {
        ok: false,
        install_dir: String::new(),
        version: env!("CARGO_PKG_VERSION").into(),
        driver: String::new(),
        need_reboot: false,
        message,
        launch: false,
    });
    progress(
        &dir,
        100,
        if result.ok {
            "安装步骤已完成"
        } else {
            "安装未完成"
        },
    );
    origin::write_result(&dir, &serde_json::to_string(&result).unwrap());
    0
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--worker") {
        let dir = args
            .iter()
            .find_map(|a| a.strip_prefix("--origin="))
            .map(PathBuf::from);
        let nonce = args
            .iter()
            .find_map(|a| a.strip_prefix("--nonce="))
            .map(str::to_owned);
        if let (Some(dir), Some(nonce)) = (dir, nonce) {
            std::process::exit(worker_main(dir, nonce));
        }
        return;
    }
    setup_window::run();
}

#[cfg(test)]
mod tests {
    use super::*;
    use drcom4scut_gui::ui::winutil::*;
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
    use windows::Win32::Graphics::Gdi::*;
    use windows::Win32::UI::Input::KeyboardAndMouse::EnableWindow;
    use windows::Win32::UI::WindowsAndMessaging::*;

    #[test]
    fn worker_arguments_quote_spaces_and_trailing_slashes() {
        assert_eq!(
            quote_arg(r"--origin=C:\Users\Test User\Temp\setup"),
            r#""--origin=C:\Users\Test User\Temp\setup""#
        );
        assert_eq!(quote_arg("a\"b"), "\"a\\\"b\"");
        assert_eq!(quote_arg("C:\\folder\\"), "\"C:\\folder\\\\\"");
    }

    struct TestCanvas {
        bmp: SvgBmp,
        dc: HDC,
        old: HGDIOBJ,
    }
    impl TestCanvas {
        unsafe fn new(w: i32, h: i32) -> Self {
            let svg =
                format!(r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}"/>"#);
            let bmp = rasterize_svg(svg.as_bytes(), w.max(h) as u32).unwrap();
            let dc = CreateCompatibleDC(None);
            let old = SelectObject(dc, HGDIOBJ(bmp.hbmp.0));
            Self { bmp, dc, old }
        }
        unsafe fn save_png(&self, path: &std::path::Path) {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = GdiFlush();
            let mut pixels =
                std::slice::from_raw_parts(self.bmp.bits, (self.bmp.w * self.bmp.h * 4) as usize)
                    .to_vec();
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
    impl Drop for TestCanvas {
        fn drop(&mut self) {
            unsafe {
                let _ = SelectObject(self.dc, self.old);
                let _ = DeleteDC(self.dc);
            }
        }
    }

    unsafe extern "system" fn fixture_proc(
        hwnd: HWND,
        msg: u32,
        wp: WPARAM,
        lp: LPARAM,
    ) -> LRESULT {
        if matches!(
            msg,
            WM_DRAWITEM | WM_CTLCOLORBTN | WM_CTLCOLORSTATIC | WM_CTLCOLOREDIT
        ) {
            setup_window::wndproc(hwnd, msg, wp, lp)
        } else {
            DefWindowProcW(hwnd, msg, wp, lp)
        }
    }

    unsafe fn print_children(canvas: &TestCanvas, parent: HWND, state: &setup_window::State) {
        let bounds = RECT {
            left: 0,
            top: 0,
            right: canvas.bmp.w,
            bottom: canvas.bmp.h,
        };
        setup_window::paint_to_dc(canvas.dc, bounds, state);
        let mut parent_rect = RECT::default();
        GetWindowRect(parent, &mut parent_rect).unwrap();
        for child in [
            state.path,
            state.browse,
            state.desktop,
            state.menu,
            state.launch,
            state.primary,
            state.secondary,
            state.close,
        ] {
            if GetWindowLongPtrW(child, GWL_STYLE) & WS_VISIBLE.0 as isize == 0 {
                continue;
            }
            let mut child_rect = RECT::default();
            GetWindowRect(child, &mut child_rect).unwrap();
            let saved = SaveDC(canvas.dc);
            let x = child_rect.left - parent_rect.left;
            let y = child_rect.top - parent_rect.top;
            let _ = IntersectClipRect(
                canvas.dc,
                x,
                y,
                x + child_rect.right - child_rect.left,
                y + child_rect.bottom - child_rect.top,
            );
            let _ = SetViewportOrgEx(canvas.dc, x, y, None);
            let _ = SendMessageW(
                child,
                WM_PRINT,
                Some(WPARAM(canvas.dc.0 as usize)),
                Some(LPARAM((PRF_CLIENT | PRF_NONCLIENT) as isize)),
            );
            let _ = RestoreDC(canvas.dc, saved);
        }
    }

    unsafe fn fixture_button(parent: HWND, id: isize, r: RECT, label: &str, font: HFONT) -> HWND {
        let child = drcom4scut_gui::install::ui::create_child_button(
            parent,
            id,
            r.left,
            r.top,
            r.right - r.left,
            r.bottom - r.top,
            label,
        );
        SetWindowLongPtrW(
            child,
            GWL_STYLE,
            (GetWindowLongPtrW(child, GWL_STYLE) & !15) | 11,
        );
        drcom4scut_gui::ui::winutil::suppress_button_erase(child);
        drcom4scut_gui::install::ui::apply_font(child, font);
        child
    }

    unsafe fn attach_fixture(parent: HWND, state: &mut setup_window::State) {
        use drcom4scut_gui::install::ui;
        let dpi = state.dpi;
        let r = |x, y, w, h| RECT {
            left: scale(x, dpi),
            top: scale(y, dpi),
            right: scale(x + w, dpi),
            bottom: scale(y + h, dpi),
        };
        let path_rect = r(48, 166, 340, 24);
        state.path = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("EDIT"),
            PCWSTR(wide(&state.options.install_dir.to_string_lossy()).as_ptr()),
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(0x80),
            path_rect.left,
            path_rect.top,
            path_rect.right - path_rect.left,
            path_rect.bottom - path_rect.top,
            Some(parent),
            Some(HMENU(ui::ID_PATH as *mut _)),
            None,
            None,
        )
        .unwrap();
        ui::apply_font(state.path, state.font);
        let b = r(410, 156, 102, 42);
        state.browse = fixture_button(parent, ui::ID_BROWSE, b, "浏览…", state.font);
        state.desktop = ui::create_child_check(
            parent,
            ui::ID_DESKTOP,
            scale(40, dpi),
            scale(278, dpi),
            scale(220, dpi),
            scale(28, dpi),
            "创建桌面快捷方式",
            true,
        );
        state.menu = ui::create_child_check(
            parent,
            ui::ID_STARTMENU,
            scale(284, dpi),
            scale(278, dpi),
            scale(228, dpi),
            scale(28, dpi),
            "添加到开始菜单",
            true,
        );
        state.launch = ui::create_child_check(
            parent,
            ui::ID_LAUNCH,
            scale(40, dpi),
            scale(314, dpi),
            scale(460, dpi),
            scale(28, dpi),
            "完成后打开校园网客户端",
            true,
        );
        for c in [state.desktop, state.menu, state.launch] {
            ui::apply_font(c, state.font);
            ui::theme_checkbox(c, state.is_dark, dpi);
            assert!(ui::is_checked(c));
            let _ = SendMessageW(c, BM_CLICK, None, None);
            assert!(!ui::is_checked(c), "checkbox must remain interactive");
            let _ = SendMessageW(c, BM_CLICK, None, None);
            assert!(ui::is_checked(c));
        }
        state.primary = fixture_button(parent, 3001, r(356, 544, 180, 40), "安装", state.font);
        state.secondary = fixture_button(parent, 3002, r(244, 544, 96, 40), "取消", state.font);
        state.close = fixture_button(parent, 3003, r(508, 16, 28, 28), "×", state.font);
        state.controls = vec![
            state.path,
            state.browse,
            state.desktop,
            state.menu,
            state.launch,
        ];
        SetWindowLongPtrW(parent, GWLP_USERDATA, state as *mut _ as isize);
        setup_window::layout_controls(state, dpi);
    }

    unsafe fn create_fixture_parent(w: i32, h: i32) -> (HWND, isize) {
        let parent = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("STATIC"),
            w!(""),
            WS_POPUP,
            0,
            0,
            w,
            h,
            None,
            None,
            None,
            None,
        )
        .unwrap();
        let old_proc = SetWindowLongPtrW(
            parent,
            GWLP_WNDPROC,
            fixture_proc as *const () as usize as isize,
        );
        (parent, old_proc)
    }

    unsafe fn fixture_state(is_dark: bool, dpi: u32) -> Box<setup_window::State> {
        let palette = Palette::for_dark(is_dark);
        Box::new(setup_window::State {
            is_dark,
            dpi,
            font: create_font(14, dpi, false),
            small: create_font(UI_CAPTION_SIZE, dpi, false),
            title: create_font(28, dpi, true),
            card: solid_brush(palette.card),
            field: solid_brush(palette.control),
            controls: Vec::new(),
            path: HWND::default(),
            browse: HWND::default(),
            desktop: HWND::default(),
            menu: HWND::default(),
            launch: HWND::default(),
            primary: HWND::default(),
            secondary: HWND::default(),
            close: HWND::default(),
            hot: 0,
            page: setup_window::Page::Configure,
            options: flow::InstallOptions {
                install_dir: PathBuf::from(r"C:\Program Files\drcom4scut"),
                desktop_shortcut: true,
                start_menu_shortcut: true,
                launch_after: true,
                ..Default::default()
            },
            detection: std::sync::mpsc::channel().1,
            driver_text: "已检测到兼容的 x64 驱动，安装时将直接使用。".into(),
            work: None,
            result: None,
            percent: 0,
            message: String::new(),
            cancelling: false,
        })
    }

    #[test]
    fn checkbox_roundtrip_stays_interactive() {
        unsafe {
            // attach_fixture 内含 BM_CLICK/BM_GETCHECK 勾选往返断言；
            // 不触发安装初始化、驱动探测、工作线程或提权。
            let (parent, old_proc) = create_fixture_parent(scale(560, 96), scale(720, 96));
            let mut state = fixture_state(false, 96);
            attach_fixture(parent, &mut state);
            SetWindowLongPtrW(parent, GWLP_USERDATA, 0);
            SetWindowLongPtrW(parent, GWLP_WNDPROC, old_proc);
            let _ = DestroyWindow(parent);
        }
    }

    #[test]
    #[ignore = "explicit offscreen setup snapshot rendering and PNG artifacts"]
    fn render_heroui_setup_snapshots() {
        unsafe {
            let out_dir = std::path::PathBuf::from(
                std::env::var_os("DRCOM_UI_ARTIFACT_DIR").expect("set artifact directory"),
            );
            let _ = std::fs::create_dir_all(&out_dir);

            for (mode_name, is_dark) in [("light", false), ("dark", true)] {
                for dpi in [96, 120, 144, 192] {
                    let w = scale(560, dpi);
                    let h = scale(720, dpi);
                    let (parent, old_proc) = create_fixture_parent(w, h);

                    // All child controls are native Win32 controls; no installer init,
                    // driver detection, worker or elevation is involved.
                    // 1. Configure page
                    let mut state = fixture_state(is_dark, dpi);
                    attach_fixture(parent, &mut state);

                    let canvas = TestCanvas::new(w, h);
                    print_children(&canvas, parent, &state);
                    canvas.save_png(
                        &out_dir.join(format!("setup-{mode_name}-{dpi}dpi-configure.png")),
                    );

                    state.message = "安装位置不可写，请选择其他文件夹。".into();
                    let canvas = TestCanvas::new(w, h);
                    print_children(&canvas, parent, &state);
                    canvas.save_png(
                        &out_dir.join(format!("setup-{mode_name}-{dpi}dpi-configure-error.png")),
                    );
                    state.message.clear();

                    // 2. Running page
                    state.page = setup_window::Page::Running;
                    state.percent = 45;
                    state.message = "正在解压并写入客户端文件 (45%)…".into();
                    for c in &state.controls {
                        let _ = ShowWindow(*c, SW_HIDE);
                    }
                    let _ = EnableWindow(state.primary, false);
                    let _ = SetWindowTextW(state.primary, PCWSTR(wide("正在安装…").as_ptr()));
                    let canvas = TestCanvas::new(w, h);
                    print_children(&canvas, parent, &state);
                    canvas
                        .save_png(&out_dir.join(format!("setup-{mode_name}-{dpi}dpi-running.png")));

                    // 3. Finished (Success)
                    state.page = setup_window::Page::Finished;
                    state.percent = 100;
                    state.result = Some(flow::InstallResult {
                        ok: true,
                        install_dir: r"C:\Program Files\drcom4scut".into(),
                        version: "0.3.6".into(),
                        need_reboot: false,
                        launch: true,
                        message: "客户端已安装完成，可以立即开始连接校园网。".into(),
                        driver: "present".into(),
                    });
                    state.message = "客户端已安装完成，可以立即开始连接校园网。".into();
                    let _ = EnableWindow(state.primary, true);
                    let _ = SetWindowTextW(state.primary, PCWSTR(wide("打开校园网").as_ptr()));
                    let _ = SetWindowTextW(state.secondary, PCWSTR(wide("关闭").as_ptr()));
                    let canvas = TestCanvas::new(w, h);
                    print_children(&canvas, parent, &state);
                    canvas.save_png(
                        &out_dir.join(format!("setup-{mode_name}-{dpi}dpi-finished-success.png")),
                    );

                    // 4. Finished (Driver retry)
                    state.result = Some(flow::InstallResult {
                        ok: true,
                        install_dir: r"C:\Program Files\drcom4scut".into(),
                        version: "0.3.6".into(),
                        need_reboot: false,
                        launch: false,
                        message: "应用文件已写入，但 Npcap 驱动安装未完成。请点击重试。".into(),
                        driver: "missing".into(),
                    });
                    state.message = "应用文件已写入，但 Npcap 驱动安装未完成。请点击重试。".into();
                    let _ = SetWindowTextW(state.primary, PCWSTR(wide("重试驱动").as_ptr()));
                    let canvas = TestCanvas::new(w, h);
                    print_children(&canvas, parent, &state);
                    canvas.save_png(&out_dir.join(format!(
                        "setup-{mode_name}-{dpi}dpi-finished-driver-retry.png"
                    )));

                    // 5. Finished (Reboot needed)
                    state.result = Some(flow::InstallResult {
                        ok: true,
                        install_dir: r"C:\Program Files\drcom4scut".into(),
                        version: "0.3.6".into(),
                        need_reboot: true,
                        launch: false,
                        message: "驱动配置已更新，需要重启系统后生效。".into(),
                        driver: "need_reboot".into(),
                    });
                    state.message = "驱动配置已更新，需要重启系统后生效。".into();
                    let _ = SetWindowTextW(state.primary, PCWSTR(wide("完成").as_ptr()));
                    let canvas = TestCanvas::new(w, h);
                    print_children(&canvas, parent, &state);
                    canvas.save_png(
                        &out_dir.join(format!("setup-{mode_name}-{dpi}dpi-finished-reboot.png")),
                    );

                    // 6. Finished (Failed)
                    state.result = Some(flow::InstallResult {
                        ok: false,
                        install_dir: r"C:\Program Files\drcom4scut".into(),
                        version: "0.3.6".into(),
                        need_reboot: false,
                        launch: false,
                        message: "用户取消了管理员授权或安装过程中止。".into(),
                        driver: "missing".into(),
                    });
                    state.message = "用户取消了管理员授权或安装过程中止。".into();
                    let _ = SetWindowTextW(state.primary, PCWSTR(wide("返回设置").as_ptr()));
                    let canvas = TestCanvas::new(w, h);
                    print_children(&canvas, parent, &state);
                    canvas.save_png(
                        &out_dir.join(format!("setup-{mode_name}-{dpi}dpi-finished-failed.png")),
                    );
                    SetWindowLongPtrW(parent, GWLP_USERDATA, 0);
                    SetWindowLongPtrW(parent, GWLP_WNDPROC, old_proc);
                    let _ = DestroyWindow(parent);
                }
            }
        }
    }
}
