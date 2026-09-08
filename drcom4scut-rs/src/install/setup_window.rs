//! Setup-only window. All privileged work runs outside the UI thread.
use drcom4scut_gui::{
    install::{
        driver::{self, DriverStatus},
        driver_flow,
        flow::{self, InstallOptions, InstallResult},
        knownfolder, origin, ui, GUI_EXE_NAME,
    },
    ui::winutil::*,
};
use std::{
    path::PathBuf,
    sync::mpsc::{self, Receiver},
};
use windows::{
    core::{w, PCWSTR},
    Win32::{
        Foundation::*,
        Graphics::Gdi::*,
        UI::{
            Controls::{DRAWITEMSTRUCT, ODS_FOCUS, ODS_SELECTED},
            Input::KeyboardAndMouse::{EnableWindow, SetFocus},
            WindowsAndMessaging::*,
        },
    },
};

const PRIMARY: isize = 3001;
const SECONDARY: isize = 3002;
const CLOSE: isize = 3003;
#[derive(PartialEq)]
enum Page {
    Configure,
    Running,
    Finished,
}
struct State {
    dpi: u32,
    font: HFONT,
    small: HFONT,
    title: HFONT,
    card: HBRUSH,
    field: HBRUSH,
    controls: Vec<HWND>,
    path: HWND,
    desktop: HWND,
    menu: HWND,
    launch: HWND,
    primary: HWND,
    secondary: HWND,
    page: Page,
    options: InstallOptions,
    detection: Receiver<DriverStatus>,
    driver_text: String,
    work: Option<(PathBuf, Receiver<Result<(), String>>)>,
    result: Option<InstallResult>,
    percent: u32,
    message: String,
    cancelling: bool,
}
impl Drop for State {
    fn drop(&mut self) {
        for obj in [
            font_as_gdi(self.font),
            font_as_gdi(self.small),
            font_as_gdi(self.title),
            brush_as_gdi(self.card),
            brush_as_gdi(self.field),
        ] {
            delete_gdi(obj);
        }
    }
}
fn rect(dpi: u32, x: i32, y: i32, width: i32, height: i32) -> RECT {
    RECT {
        left: scale(x, dpi),
        top: scale(y, dpi),
        right: scale(x + width, dpi),
        bottom: scale(y + height, dpi),
    }
}
fn label(hwnd: HWND, text: &str) {
    unsafe {
        let _ = SetWindowTextW(hwnd, PCWSTR(wide(text).as_ptr()));
    }
}
unsafe fn button(hwnd: HWND, id: isize, r: RECT, text: &str, font: HFONT) -> HWND {
    let b = ui::create_child_button(
        hwnd,
        id,
        r.left,
        r.top,
        r.right - r.left,
        r.bottom - r.top,
        text,
    );
    SetWindowLongPtrW(b, GWL_STYLE, (GetWindowLongPtrW(b, GWL_STYLE) & !15) | 11); // BS_OWNERDRAW
    ui::apply_font(b, font);
    b
}
unsafe fn init(hwnd: HWND) -> State {
    let dpi = screen_dpi();
    let font = create_font(14, dpi, false);
    let small = create_font(12, dpi, false);
    let title = create_font(24, dpi, true);
    let mut options = InstallOptions::default();
    if let Ok(pf) = knownfolder::program_files() {
        options.install_dir = pf.join("drcom4scutGUI");
    }
    if let Ok(Some(existing)) = drcom4scut_gui::install::registry::registered_install_dir() {
        options.install_dir = existing;
    }
    let r = rect(dpi, 48, 166, 340, 24);
    let path = CreateWindowExW(
        WINDOW_EX_STYLE(0),
        w!("EDIT"),
        PCWSTR(wide(&options.install_dir.to_string_lossy()).as_ptr()),
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(0x80),
        r.left,
        r.top,
        r.right - r.left,
        r.bottom - r.top,
        Some(hwnd),
        Some(HMENU(ui::ID_PATH as *mut _)),
        None,
        None,
    )
    .unwrap_or_default();
    ui::apply_font(path, font);
    let browse = button(
        hwnd,
        ui::ID_BROWSE,
        rect(dpi, 410, 156, 102, 42),
        "浏览…",
        font,
    );
    let desktop = ui::create_child_check(
        hwnd,
        ui::ID_DESKTOP,
        scale(40, dpi),
        scale(278, dpi),
        scale(220, dpi),
        scale(28, dpi),
        "创建桌面快捷方式",
        true,
    );
    let menu = ui::create_child_check(
        hwnd,
        ui::ID_STARTMENU,
        scale(284, dpi),
        scale(278, dpi),
        scale(228, dpi),
        scale(28, dpi),
        "添加到开始菜单",
        true,
    );
    let launch = ui::create_child_check(
        hwnd,
        ui::ID_LAUNCH,
        scale(40, dpi),
        scale(314, dpi),
        scale(460, dpi),
        scale(28, dpi),
        "完成后打开校园网客户端",
        true,
    );
    for c in [desktop, menu, launch] {
        ui::apply_font(c, font);
    }
    let primary = button(hwnd, PRIMARY, rect(dpi, 356, 544, 180, 40), "安装", font);
    let secondary = button(hwnd, SECONDARY, rect(dpi, 244, 544, 96, 40), "取消", font);
    button(hwnd, CLOSE, rect(dpi, 508, 16, 28, 28), "×", font);
    let (tx, detection) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(driver::detect_with(&driver::RealDriverHost));
    });
    SetTimer(Some(hwnd), 1, 150, None);
    let region = CreateRoundRectRgn(
        0,
        0,
        scale(560, dpi) + 1,
        scale(608, dpi) + 1,
        scale(16, dpi),
        scale(16, dpi),
    );
    if SetWindowRgn(hwnd, Some(region), true) == 0 {
        delete_gdi(HGDIOBJ(region.0));
    }
    State {
        dpi,
        font,
        small,
        title,
        card: solid_brush(COLOR_CARD),
        field: solid_brush(COLOR_CONTROL),
        controls: vec![path, browse, desktop, menu, launch],
        path,
        desktop,
        menu,
        launch,
        primary,
        secondary,
        page: Page::Configure,
        options,
        detection,
        driver_text: "正在检测已安装的兼容驱动…".into(),
        work: None,
        result: None,
        percent: 0,
        message: String::new(),
        cancelling: false,
    }
}
unsafe fn text(
    hdc: HDC,
    font: HFONT,
    r: RECT,
    value: &str,
    color: COLORREF,
    flags: DRAW_TEXT_FORMAT,
) {
    let old = SelectObject(hdc, font_as_gdi(font));
    SetTextColor(hdc, color);
    SetBkMode(hdc, TRANSPARENT);
    let mut r = r;
    let mut value: Vec<u16> = value.encode_utf16().collect();
    DrawTextW(hdc, &mut value, &mut r, flags | DT_NOPREFIX);
    SelectObject(hdc, old);
}
unsafe fn paint(hwnd: HWND, s: &State) {
    let mut ps = PAINTSTRUCT::default();
    let dc = BeginPaint(hwnd, &mut ps);
    let mut bounds = RECT::default();
    let _ = GetClientRect(hwnd, &mut bounds);
    let mem = CreateCompatibleDC(Some(dc));
    let bmp = CreateCompatibleBitmap(dc, bounds.right, bounds.bottom);
    let old = SelectObject(mem, HGDIOBJ(bmp.0));
    ui::fill_page(mem, hwnd);
    fill_component(mem, bounds, s.dpi, COLOR_PAGE, Some(COLOR_WINDOW_BORDER));
    let r = |x, y, w, h| rect(s.dpi, x, y, w, h);
    text(
        mem,
        s.title,
        r(24, 28, 464, 36),
        "安装校园网客户端",
        COLOR_TEXT_PRIMARY,
        DT_SINGLELINE,
    );
    text(
        mem,
        s.small,
        r(24, 74, 512, 24),
        &format!("Windows x64  ·  版本 {}", env!("CARGO_PKG_VERSION")),
        COLOR_TEXT_SECONDARY,
        DT_SINGLELINE,
    );
    if s.page == Page::Configure {
        fill_component(mem, r(24, 112, 512, 112), s.dpi, COLOR_CARD, None);
        text(
            mem,
            s.font,
            r(40, 126, 472, 24),
            "安装位置",
            COLOR_TEXT_PRIMARY,
            DT_SINGLELINE,
        );
        fill_component(
            mem,
            r(40, 156, 356, 42),
            s.dpi,
            COLOR_CONTROL,
            Some(COLOR_STROKE),
        );
        fill_component(mem, r(24, 236, 512, 122), s.dpi, COLOR_CARD, None);
        text(
            mem,
            s.font,
            r(40, 250, 472, 24),
            "快捷方式与启动",
            COLOR_TEXT_PRIMARY,
            DT_SINGLELINE,
        );
        fill_component(mem, r(24, 370, 512, 116), s.dpi, COLOR_CARD, None);
        text(
            mem,
            s.font,
            r(40, 386, 472, 24),
            "网络驱动 · 自动配置",
            COLOR_TEXT_PRIMARY,
            DT_SINGLELINE,
        );
        text(
            mem,
            s.small,
            r(40, 420, 472, 50),
            &s.driver_text,
            COLOR_TEXT_SECONDARY,
            DT_WORDBREAK,
        );
        let note = if s.message.is_empty() {
            "缺少驱动时将自动下载并打开 Npcap 官方安装窗口。\n请按提示完成许可确认；客户端附带原生卸载程序。"
        } else {
            &s.message
        };
        text(
            mem,
            s.small,
            r(24, 494, 512, 46),
            note,
            if s.message.is_empty() {
                COLOR_TEXT_SECONDARY
            } else {
                COLOR_DANGER
            },
            DT_WORDBREAK,
        );
    } else {
        fill_component(mem, r(24, 112, 512, 374), s.dpi, COLOR_CARD, None);
        let heading = if s.page == Page::Running {
            "正在安装"
        } else if let Some(result) = &s.result {
            if !result.ok {
                "安装未完成"
            } else if result.need_reboot {
                "请重启电脑后继续"
            } else if driver_flow::ready(&result.driver) {
                "安装完成"
            } else {
                "应用已安装，驱动待完成"
            }
        } else {
            "安装未完成"
        };
        text(
            mem,
            s.title,
            r(40, 140, 472, 40),
            heading,
            COLOR_TEXT_PRIMARY,
            DT_SINGLELINE,
        );
        text(
            mem,
            s.font,
            r(40, 198, 472, 132),
            &s.message,
            COLOR_TEXT_SECONDARY,
            DT_WORDBREAK,
        );
        text(
            mem,
            s.small,
            r(40, 350, 472, 70),
            &format!("安装位置\n{}", s.options.install_dir.display()),
            COLOR_TEXT_SECONDARY,
            DT_WORDBREAK,
        );
        fill_component(mem, r(40, 442, 472, 8), s.dpi, COLOR_STROKE, None);
        if s.percent > 0 {
            fill_component(
                mem,
                r(40, 442, (472 * s.percent.min(100) / 100) as i32, 8),
                s.dpi,
                COLOR_ACCENT,
                None,
            );
        }
        let note = if s.page == Page::Running {
            "请保持窗口打开。Npcap 安装时可能需要在官方窗口确认。"
        } else {
            "可以通过 Windows“已安装的应用”卸载本程序。"
        };
        text(
            mem,
            s.small,
            r(24, 502, 512, 36),
            note,
            COLOR_TEXT_SECONDARY,
            DT_WORDBREAK,
        );
    }
    let _ = BitBlt(
        dc,
        0,
        0,
        bounds.right,
        bounds.bottom,
        Some(mem),
        0,
        0,
        SRCCOPY,
    );
    SelectObject(mem, old);
    delete_gdi(HGDIOBJ(bmp.0));
    let _ = DeleteDC(mem);
    let _ = EndPaint(hwnd, &ps);
}
unsafe fn refresh(hwnd: HWND) {
    let _ = InvalidateRect(Some(hwnd), None, false);
}
/// Shell elevation is required to open the client's requireAdministrator manifest.
unsafe fn launch_client(owner: HWND, dir: &std::path::Path) -> Result<(), String> {
    use windows::Win32::UI::Shell::{ShellExecuteExW, SHELLEXECUTEINFOW};
    let executable = wide(&dir.join(GUI_EXE_NAME).to_string_lossy());
    let workdir = wide(&dir.to_string_lossy());
    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        hwnd: owner,
        lpVerb: w!("runas"),
        lpFile: PCWSTR(executable.as_ptr()),
        lpDirectory: PCWSTR(workdir.as_ptr()),
        nShow: SW_SHOWNORMAL.0,
        ..Default::default()
    };
    ShellExecuteExW(&mut info).map_err(|e| {
        if e.code().0 as u32 == 0x800704c7 {
            "已取消管理员授权，可再次点击“打开校园网”启动客户端。".into()
        } else {
            format!("无法启动客户端：{e}")
        }
    })
}

unsafe fn finish(hwnd: HWND, s: &mut State, result: Result<InstallResult, String>) {
    s.page = Page::Finished;
    s.result = match result {
        Ok(r) => Some(r),
        Err(e) => {
            s.message = e;
            None
        }
    };
    if let Some(r) = &s.result {
        s.message = r.message.clone();
        if r.ok && (r.need_reboot || driver_flow::ready(&r.driver)) {
            s.percent = 100;
        }
    }
    let retry_driver = s
        .result
        .as_ref()
        .is_some_and(|r| r.ok && !r.need_reboot && !driver_flow::ready(&r.driver));
    let failed = s.result.as_ref().is_none_or(|r| !r.ok);
    label(
        s.primary,
        if retry_driver {
            "重试驱动"
        } else if failed {
            "返回设置"
        } else if s.result.as_ref().is_some_and(|r| r.launch) {
            "打开校园网"
        } else {
            "完成"
        },
    );
    label(s.secondary, "关闭");
    let _ = EnableWindow(s.secondary, true);
    refresh(hwnd);
}
unsafe fn begin(hwnd: HWND, s: &mut State, driver_only: bool) {
    if !driver_only {
        s.options.install_dir = PathBuf::from(ui::edit_text(s.path).trim());
        s.options.desktop_shortcut = ui::is_checked(s.desktop);
        s.options.start_menu_shortcut = ui::is_checked(s.menu);
        s.options.launch_after = ui::is_checked(s.launch);
        let system = std::env::var_os("SystemRoot")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
        match flow::validate_options(
            &s.options,
            &system,
            knownfolder::program_files_x86().ok().as_deref(),
        ) {
            Ok(p) => s.options.install_dir = p,
            Err(e) => {
                s.message = e.message();
                refresh(hwnd);
                return;
            }
        }
    }
    let nonce = origin::new_nonce();
    let prepared = origin::capture_current_origin(&nonce)
        .and_then(|o| origin::write_origin(&o))
        .and_then(
            |p| match super::write_options(&p, &s.options, driver_only) {
                Ok(()) => Ok(p),
                Err(e) => {
                    origin::cleanup_origin(&p);
                    Err(e)
                }
            },
        );
    let dir = match prepared {
        Ok(p) => p,
        Err(e) => {
            s.message = e;
            refresh(hwnd);
            return;
        }
    };
    s.page = Page::Running;
    s.result = None;
    s.percent = 2;
    s.message = "请在系统提示中允许安装，然后等待文件和驱动配置完成。".into();
    s.cancelling = false;
    for c in &s.controls {
        let _ = ShowWindow(*c, SW_HIDE);
    }
    label(s.primary, "正在安装…");
    let _ = EnableWindow(s.primary, false);
    label(s.secondary, "取消");
    let args = vec![
        "--worker".into(),
        format!("--origin={}", dir.display()),
        format!("--nonce={nonce}"),
    ];
    let (tx, rx) = mpsc::channel();
    s.work = Some((dir, rx));
    std::thread::spawn(move || {
        let _ = tx.send(super::relaunch_elevated(&args));
    });
    refresh(hwnd);
}
unsafe fn cancel(hwnd: HWND, s: &mut State) {
    if s.page == Page::Running {
        if let Some((dir, _)) = &s.work {
            origin::request_cancel(dir);
        }
        s.cancelling = true;
        s.message =
            "已请求取消，正在等待当前步骤结束。若 Npcap 官方窗口已打开，请在该窗口取消或完成安装。"
                .into();
        label(s.secondary, "正在取消");
        let _ = EnableWindow(s.secondary, false);
        refresh(hwnd);
    } else {
        let _ = DestroyWindow(hwnd);
    }
}
unsafe fn tick(hwnd: HWND, s: &mut State) {
    if let Ok(status) = s.detection.try_recv() {
        s.driver_text = if status == DriverStatus::Available {
            "已检测到兼容的 x64 驱动，安装时将直接使用。"
        } else {
            "未检测到可用的 x64 驱动，安装时将自动下载并安装。"
        }
        .into();
        refresh(hwnd);
    }
    let Some((dir, rx)) = &s.work else {
        return;
    };
    if !s.cancelling {
        if let Ok(raw) = std::fs::read(dir.join("status.json")) {
            if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&raw) {
                s.percent = v["percent"].as_u64().unwrap_or(0).min(100) as u32;
                if let Some(m) = v["message"].as_str() {
                    s.message = m.into();
                }
                refresh(hwnd);
            }
        }
    }
    let completed = match rx.try_recv() {
        Ok(r) => Some(r),
        Err(mpsc::TryRecvError::Disconnected) => {
            Some(Err("安装进程连接已中断，请重新运行安装器。".into()))
        }
        Err(_) => None,
    };
    if let Some(completed) = completed {
        let result = completed
            .and_then(|_| {
                std::fs::read(dir.join("result.json")).map_err(|e| format!("未收到安装结果：{e}"))
            })
            .and_then(|raw| {
                serde_json::from_slice::<InstallResult>(&raw).map_err(|e| e.to_string())
            });
        origin::cleanup_origin(dir);
        s.work = None;
        if let Err(e) = &result {
            s.message = e.clone();
        }
        let _ = EnableWindow(s.primary, true);
        finish(hwnd, s, result);
    }
}
unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if msg == WM_CREATE {
        let s = Box::new(init(hwnd));
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(s) as isize);
        return LRESULT(0);
    }
    let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut State;
    if ptr.is_null() {
        return DefWindowProcW(hwnd, msg, wp, lp);
    }
    let s = &mut *ptr;
    match msg {
        WM_PAINT => {
            paint(hwnd, s);
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        WM_TIMER => {
            tick(hwnd, s);
            LRESULT(0)
        }
        WM_CTLCOLORSTATIC | WM_CTLCOLORBTN | WM_CTLCOLOREDIT => {
            let dc = HDC(wp.0 as *mut _);
            SetTextColor(dc, COLOR_TEXT_PRIMARY);
            SetBkMode(dc, TRANSPARENT);
            let brush = if msg == WM_CTLCOLOREDIT {
                SetBkColor(dc, COLOR_CONTROL);
                s.field
            } else {
                SetBkColor(dc, COLOR_CARD);
                s.card
            };
            LRESULT(brush.0 as isize)
        }
        WM_DRAWITEM => {
            let d = &*(lp.0 as *const DRAWITEMSTRUCT);
            // Owner-drawn controls must clear their corners as well as the rounded body.
            let background = solid_brush(if d.CtlID == ui::ID_BROWSE as u32 {
                COLOR_CARD
            } else {
                COLOR_PAGE
            });
            let _ = FillRect(d.hDC, &d.rcItem, background);
            delete_gdi(brush_as_gdi(background));
            let accent = d.CtlID == PRIMARY as u32;
            let fill = if accent {
                if d.itemState.0 & ODS_SELECTED.0 != 0 {
                    COLOR_ACCENT_HOVER
                } else {
                    COLOR_ACCENT
                }
            } else {
                COLOR_PAGE
            };
            fill_component(
                d.hDC,
                d.rcItem,
                s.dpi,
                fill,
                if accent { None } else { Some(COLOR_STROKE) },
            );
            text(
                d.hDC,
                s.font,
                d.rcItem,
                &ui::edit_text(d.hwndItem),
                if accent {
                    COLOR_CARD
                } else {
                    COLOR_TEXT_PRIMARY
                },
                DT_CENTER | DT_VCENTER | DT_SINGLELINE,
            );
            if d.itemState.0 & ODS_FOCUS.0 != 0 {
                let mut r = d.rcItem;
                let _ = InflateRect(&mut r, -scale(4, s.dpi), -scale(4, s.dpi));
                let _ = DrawFocusRect(d.hDC, &r);
            }
            LRESULT(1)
        }
        WM_COMMAND => {
            match (wp.0 & 0xffff) as isize {
                CLOSE | SECONDARY | 2 => cancel(hwnd, s),
                ui::ID_BROWSE if s.page == Page::Configure => {
                    if let Some(p) = ui::pick_directory(hwnd, &ui::edit_text(s.path)) {
                        label(s.path, &p.to_string_lossy());
                    }
                }
                PRIMARY | 1 => {
                    if s.page == Page::Configure {
                        begin(hwnd, s, false);
                    } else if s.page == Page::Finished {
                        if s.result.as_ref().is_none_or(|r| !r.ok) {
                            s.page = Page::Configure;
                            s.message.clear();
                            for c in &s.controls {
                                let _ = ShowWindow(*c, SW_SHOW);
                            }
                            label(s.primary, "安装");
                            label(s.secondary, "取消");
                            let _ = SetFocus(Some(s.path));
                            refresh(hwnd);
                        } else if s
                            .result
                            .as_ref()
                            .is_some_and(|r| !r.need_reboot && !driver_flow::ready(&r.driver))
                        {
                            begin(hwnd, s, true);
                        } else {
                            if s.result.as_ref().is_some_and(|r| r.launch) {
                                if let Err(e) = launch_client(hwnd, &s.options.install_dir) {
                                    s.message = format!("客户端已安装，但无法打开：{e}");
                                    refresh(hwnd);
                                    return LRESULT(0);
                                }
                            }
                            let _ = DestroyWindow(hwnd);
                        }
                    }
                }
                _ => {}
            }
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            if ((lp.0 >> 16) & 0xffff) < scale(100, s.dpi) as isize {
                let _ = SendMessageW(
                    hwnd,
                    WM_NCLBUTTONDOWN,
                    Some(WPARAM(HTCAPTION as usize)),
                    Some(lp),
                );
            }
            LRESULT(0)
        }
        WM_CLOSE => {
            cancel(hwnd, s);
            LRESULT(0)
        }
        WM_DESTROY => {
            let _ = KillTimer(Some(hwnd), 1);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            drop(Box::from_raw(ptr));
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}
pub fn run() {
    unsafe {
        if let Some(hwnd) = ui::create_popup(
            w!("DrcomSetup"),
            "校园网客户端安装",
            560,
            608,
            Some(wndproc),
        ) {
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
                if !IsDialogMessageW(hwnd, &msg).as_bool() {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
        }
    }
}
