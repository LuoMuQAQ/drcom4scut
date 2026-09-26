//! Setup-only window. All privileged work runs outside the UI thread.
use drcom4scut_gui::{
    install::{
        driver::{self, DriverStatus},
        driver_flow,
        flow::{self, InstallOptions, InstallResult},
        knownfolder, origin, ui, GUI_EXE_NAME,
    },
    ui::hero::{self, Icon},
    ui::winutil::{self, *},
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
            Controls::{DRAWITEMSTRUCT, ODS_SELECTED},
            Input::KeyboardAndMouse::{
                EnableWindow, SetFocus, TrackMouseEvent, TRACKMOUSEEVENT, TRACKMOUSEEVENT_FLAGS,
            },
            WindowsAndMessaging::*,
        },
    },
};

const PRIMARY: isize = 3001;
const SECONDARY: isize = 3002;
const CLOSE: isize = 3003;
const TME_LEAVE: TRACKMOUSEEVENT_FLAGS = TRACKMOUSEEVENT_FLAGS(0x2);
const WM_MOUSELEAVE: u32 = 0x02A3;
#[derive(PartialEq)]
pub(crate) enum Page {
    Configure,
    Running,
    Finished,
}
pub(crate) struct State {
    pub(crate) is_dark: bool,
    pub(crate) dpi: u32,
    pub(crate) font: HFONT,
    pub(crate) small: HFONT,
    pub(crate) title: HFONT,
    pub(crate) brand: HFONT,
    pub(crate) chip: HFONT,
    pub(crate) medium: HFONT,
    pub(crate) heading: HFONT,
    pub(crate) card: HBRUSH,
    pub(crate) field: HBRUSH,
    pub(crate) controls: Vec<HWND>,
    pub(crate) path: HWND,
    pub(crate) browse: HWND,
    pub(crate) desktop: HWND,
    pub(crate) menu: HWND,
    pub(crate) launch: HWND,
    pub(crate) primary: HWND,
    pub(crate) secondary: HWND,
    pub(crate) close: HWND,
    pub(crate) hot: isize,
    pub(crate) page: Page,
    pub(crate) options: InstallOptions,
    pub(crate) detection: Receiver<DriverStatus>,
    pub(crate) driver_text: String,
    pub(crate) work: Option<(PathBuf, Receiver<Result<(), String>>)>,
    pub(crate) result: Option<InstallResult>,
    pub(crate) percent: u32,
    pub(crate) message: String,
    pub(crate) cancelling: bool,
}
impl Drop for State {
    fn drop(&mut self) {
        for obj in [
            font_as_gdi(self.font),
            font_as_gdi(self.small),
            font_as_gdi(self.title),
            font_as_gdi(self.brand),
            font_as_gdi(self.chip),
            font_as_gdi(self.medium),
            font_as_gdi(self.heading),
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
    winutil::suppress_button_erase(b);
    ui::apply_font(b, font);
    b
}
pub(crate) unsafe fn layout_controls(s: &State, dpi: u32) {
    for (child, r) in [
        (s.path, rect(dpi, 42, 274, 394, 24)),
        (s.browse, rect(dpi, 454, 266, 76, 40)),
        (s.desktop, rect(dpi, 30, 362, 500, 48)),
        (s.menu, rect(dpi, 30, 414, 500, 48)),
        (s.launch, rect(dpi, 30, 466, 500, 48)),
        (s.primary, rect(dpi, 350, 672, 180, 40)),
        (s.secondary, rect(dpi, 244, 672, 96, 40)),
        (s.close, rect(dpi, 516, 10, 28, 28)),
    ] {
        let _ = SetWindowPos(
            child,
            None,
            r.left,
            r.top,
            r.right - r.left,
            r.bottom - r.top,
            SWP_NOZORDER | SWP_NOACTIVATE,
        );
    }
    for c in [s.desktop, s.menu, s.launch] {
        ui::theme_checkbox(c, s.is_dark, dpi);
    }
}

unsafe fn invalidate_button(hwnd: HWND, s: &State, id: isize) {
    let btn = match id {
        PRIMARY => s.primary,
        SECONDARY => s.secondary,
        CLOSE => s.close,
        ui::ID_BROWSE => s.browse,
        _ => return,
    };
    let _ = InvalidateRect(Some(btn), None, false);
    let mut rc = RECT::default();
    if GetWindowRect(btn, &mut rc).is_ok() {
        let points = std::slice::from_raw_parts_mut(&mut rc as *mut _ as *mut POINT, 2);
        let _ = MapWindowPoints(None, Some(hwnd), points);
        let _ = InvalidateRect(Some(hwnd), Some(&rc), false);
    }
}
unsafe fn init(hwnd: HWND) -> State {
    let dpi = window_dpi(hwnd);
    let font = create_font(14, dpi, false);
    let small = create_font(UI_CAPTION_SIZE, dpi, false);
    let title = winutil::create_font_weight(28, dpi, 600);
    let brand = winutil::create_font_weight(12, dpi, 600);
    let chip = create_font(12, dpi, false);
    let medium = winutil::create_font_weight(13, dpi, 500);
    let heading = winutil::create_font_weight(14, dpi, 600);
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
    let close = button(hwnd, CLOSE, rect(dpi, 508, 16, 28, 28), "×", font);
    let (tx, detection) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(driver::detect_with(&driver::RealDriverHost));
    });
    let is_dark = winutil::is_system_dark_mode();
    for c in [desktop, menu, launch] {
        ui::theme_checkbox(c, is_dark, dpi);
    }
    let palette = winutil::Palette::for_dark(is_dark);
    SetTimer(Some(hwnd), 1, 150, None);
    ui::round_corners(hwnd);
    winutil::set_window_dark_mode(hwnd, is_dark);
    let state = State {
        is_dark,
        dpi,
        font,
        small,
        title,
        brand,
        chip,
        medium,
        heading,
        card: solid_brush(palette.card),
        field: solid_brush(palette.control),
        controls: vec![path, browse, desktop, menu, launch],
        path,
        browse,
        desktop,
        menu,
        launch,
        primary,
        secondary,
        close,
        hot: 0,
        page: Page::Configure,
        options,
        detection,
        driver_text: "正在检测已安装的兼容驱动…".into(),
        work: None,
        result: None,
        percent: 0,
        message: String::new(),
        cancelling: false,
    };
    layout_controls(&state, dpi);
    state
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
pub(crate) unsafe fn paint_to_dc(dc: HDC, bounds: RECT, s: &State) {
    let mem = CreateCompatibleDC(Some(dc));
    let bmp = CreateCompatibleBitmap(dc, bounds.right, bounds.bottom);
    let old = SelectObject(mem, HGDIOBJ(bmp.0));
    let palette = winutil::Palette::for_dark(s.is_dark);
    let bg = solid_brush(palette.page);
    let _ = FillRect(mem, &bounds, bg);
    delete_gdi(brush_as_gdi(bg));
    let r = |x, y, w, h| rect(s.dpi, x, y, w, h);
    hero::line(
        mem,
        r(0, 47, 560, 1),
        hero::mix(palette.page, palette.stroke, 55),
    );
    hero::logo(mem, r(16, 14, 20, 20));
    text(
        mem,
        s.brand,
        r(44, 0, 420, 48),
        "drcom4scut 安装程序",
        palette.text_primary,
        DT_SINGLELINE | DT_VCENTER,
    );
    if s.page != Page::Finished {
        hero::surface(mem, r(30, 78, 44, 44), s.dpi, palette.accent_soft, 14);
        hero::icon(
            mem,
            r(41, 89, 22, 22),
            palette.accent,
            if s.page == Page::Running {
                Icon::PackageOpen
            } else {
                Icon::Download
            },
        );
        let caption = if s.page == Page::Running {
            "正在安装".to_string()
        } else {
            format!("Windows x64 · {}", env!("CARGO_PKG_VERSION"))
        };
        hero::chip(
            mem,
            s.chip,
            scale(530, s.dpi),
            scale(87, s.dpi),
            s.dpi,
            palette.toggle_off,
            palette.text_primary,
            None,
            &caption,
        );
    }
    if s.page == Page::Configure {
        text(
            mem,
            s.title,
            r(30, 144, 500, 40),
            "安装校园网客户端",
            palette.text_primary,
            DT_SINGLELINE | DT_END_ELLIPSIS,
        );
        text(
            mem,
            s.small,
            r(30, 190, 500, 24),
            "完成以下设置，即可开始使用。",
            palette.text_secondary,
            DT_SINGLELINE,
        );
        text(
            mem,
            s.heading,
            r(30, 234, 500, 24),
            "安装位置",
            palette.text_primary,
            DT_SINGLELINE,
        );
        let path_focused = !s.path.0.is_null()
            && unsafe { windows::Win32::UI::Input::KeyboardAndMouse::GetFocus() == s.path };
        hero::field(
            mem,
            r(30, 264, 414, 44),
            s.dpi,
            palette,
            if path_focused {
                hero::FieldState::Focus
            } else {
                hero::FieldState::Normal
            },
        );
        text(
            mem,
            s.heading,
            r(30, 334, 500, 24),
            "安装选项",
            palette.text_primary,
            DT_SINGLELINE,
        );
        for y in [412, 464] {
            hero::line(mem, r(30, y, 500, 1), palette.stroke);
        }
        hero::surface(mem, r(30, 530, 500, 66), s.dpi, palette.accent_soft, 12);
        hero::icon(
            mem,
            r(44, 546, 18, 18),
            palette.accent_text,
            Icon::ShieldCheck,
        );
        text(
            mem,
            s.small,
            r(72, 540, 442, 46),
            &s.driver_text,
            palette.accent_text,
            DT_WORDBREAK | DT_WORD_ELLIPSIS,
        );
        let note = if s.message.is_empty() {
            "缺少驱动时将打开 Npcap 官方安装窗口，请按提示完成许可确认。"
        } else {
            &s.message
        };
        text(
            mem,
            s.small,
            r(30, 606, 500, 42),
            note,
            if s.message.is_empty() {
                palette.text_secondary
            } else {
                palette.danger_text
            },
            DT_WORDBREAK | DT_WORD_ELLIPSIS,
        );
    } else if s.page == Page::Finished {
        let (icon_bg, icon_fg, symbol) = if s.result.as_ref().is_none_or(|r| !r.ok) {
            (
                palette.danger_soft,
                palette.danger_text,
                Icon::TriangleAlert,
            )
        } else if s
            .result
            .as_ref()
            .is_some_and(|r| r.need_reboot || !driver_flow::ready(&r.driver))
        {
            (palette.warning_soft, palette.warning_text, Icon::Info)
        } else {
            (palette.success_soft, palette.success_text, Icon::Check)
        };
        let heading = if let Some(result) = &s.result {
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
        hero::surface(mem, r(254, 132, 52, 52), s.dpi, icon_bg, 14);
        hero::icon(mem, r(267, 145, 26, 26), icon_fg, symbol);
        text(
            mem,
            s.title,
            r(30, 204, 500, 40),
            heading,
            palette.text_primary,
            DT_CENTER | DT_SINGLELINE | DT_END_ELLIPSIS,
        );
        text(
            mem,
            s.small,
            r(60, 250, 440, 48),
            &s.message,
            palette.text_secondary,
            DT_CENTER | DT_WORDBREAK | DT_WORD_ELLIPSIS,
        );
        hero::surface(mem, r(30, 318, 500, 64), s.dpi, palette.toggle_off, 12);
        text(
            mem,
            s.chip,
            r(44, 330, 472, 40),
            &format!("安装位置\n{}", s.options.install_dir.display()),
            palette.text_secondary,
            DT_WORDBREAK | DT_WORD_ELLIPSIS,
        );
        text(
            mem,
            s.small,
            r(30, 568, 500, 60),
            "可以通过 Windows“已安装的应用”卸载本程序。",
            palette.text_secondary,
            DT_WORDBREAK | DT_WORD_ELLIPSIS,
        );
    } else {
        text(
            mem,
            s.title,
            r(30, 144, 500, 42),
            "正在准备客户端",
            palette.text_primary,
            DT_SINGLELINE | DT_END_ELLIPSIS,
        );
        text(
            mem,
            s.font,
            r(30, 206, 500, 144),
            &s.message,
            palette.text_secondary,
            DT_WORDBREAK | DT_WORD_ELLIPSIS,
        );
        hero::surface(mem, r(30, 382, 500, 8), s.dpi, palette.toggle_off, 4);
        if s.percent > 0 {
            hero::surface(
                mem,
                r(30, 382, (500 * s.percent.min(100) / 100) as i32, 8),
                s.dpi,
                palette.accent,
                4,
            );
        }
        text(
            mem,
            s.small,
            r(30, 402, 500, 24),
            "安装进度",
            palette.text_secondary,
            DT_SINGLELINE,
        );
        text(
            mem,
            s.small,
            r(430, 402, 100, 24),
            &format!("{}%", s.percent),
            palette.text_secondary,
            DT_SINGLELINE | DT_RIGHT,
        );
        hero::surface(mem, r(30, 452, 500, 64), s.dpi, palette.toggle_off, 12);
        text(
            mem,
            s.chip,
            r(44, 464, 472, 40),
            &format!("安装位置\n{}", s.options.install_dir.display()),
            palette.text_secondary,
            DT_WORDBREAK | DT_WORD_ELLIPSIS,
        );
        text(
            mem,
            s.small,
            r(30, 568, 500, 60),
            "请保持窗口打开。Npcap 安装时可能需要在官方窗口确认。",
            palette.text_secondary,
            DT_WORDBREAK | DT_WORD_ELLIPSIS,
        );
    }
    hero::line(mem, r(0, 656, 560, 1), palette.stroke);
    let footer = if s.page == Page::Finished {
        if s.result.as_ref().is_some_and(|r| r.ok) {
            "安装成功"
        } else {
            "安装未完成"
        }
    } else if s.page == Page::Running {
        "请稍候"
    } else {
        "安装到这台电脑"
    };
    text(
        mem,
        s.small,
        r(30, 672, 200, 40),
        footer,
        palette.text_secondary,
        DT_SINGLELINE | DT_VCENTER,
    );
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
}

unsafe fn paint(hwnd: HWND, s: &State) {
    let mut ps = PAINTSTRUCT::default();
    let dc = BeginPaint(hwnd, &mut ps);
    let mut bounds = RECT::default();
    let _ = GetClientRect(hwnd, &mut bounds);
    paint_to_dc(dc, bounds, s);
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
    let mut needs_refresh = false;

    if let Ok(status) = s.detection.try_recv() {
        s.driver_text = if status == DriverStatus::Available {
            "已检测到兼容的 x64 驱动，安装时将直接使用。"
        } else {
            "未检测到可用的 x64 驱动，安装时将自动下载并安装。"
        }
        .into();
        needs_refresh = true;
    }

    let Some((dir, rx)) = &s.work else {
        if needs_refresh {
            refresh(hwnd);
        }
        return;
    };

    if !s.cancelling {
        if let Ok(raw) = std::fs::read(dir.join("status.json")) {
            if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&raw) {
                let new_percent = v["percent"].as_u64().unwrap_or(0).min(100) as u32;
                let new_message = v["message"].as_str().map(String::from);

                // 只在有变化时刷新
                if new_percent != s.percent || new_message.as_ref() != Some(&s.message) {
                    s.percent = new_percent;
                    if let Some(m) = new_message {
                        s.message = m;
                    }
                    needs_refresh = true;
                }
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
    } else if needs_refresh {
        refresh(hwnd);
    }
}
pub(crate) unsafe extern "system" fn wndproc(
    hwnd: HWND,
    msg: u32,
    wp: WPARAM,
    lp: LPARAM,
) -> LRESULT {
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
            let palette = winutil::Palette::for_dark(s.is_dark);
            SetTextColor(dc, palette.text_primary);
            SetBkMode(dc, TRANSPARENT);
            let brush = if msg == WM_CTLCOLOREDIT {
                SetBkColor(dc, palette.control);
                s.field
            } else {
                SetBkColor(dc, palette.card);
                s.card
            };
            LRESULT(brush.0 as isize)
        }
        WM_DRAWITEM => {
            let d = &*(lp.0 as *const DRAWITEMSTRUCT);
            winutil::paint_buffered(d.hDC, d.rcItem, |dc| {
                let palette = winutil::Palette::for_dark(s.is_dark);
                // Owner-drawn controls must clear their corners as well as the rounded body.
                let background = solid_brush(palette.page);
                let _ = FillRect(dc, &d.rcItem, background);
                delete_gdi(brush_as_gdi(background));
                let accent = d.CtlID == PRIMARY as u32;
                let close = d.CtlID == CLOSE as u32;
                let hot = s.hot == d.CtlID as isize;
                let pressed = d.itemState.0 & ODS_SELECTED.0 != 0;
                let disabled =
                    !windows::Win32::UI::Input::KeyboardAndMouse::IsWindowEnabled(d.hwndItem)
                        .as_bool();
                let (fill, caption) = if accent {
                    let fill = if disabled {
                        hero::mix(palette.page, palette.accent, 50)
                    } else if pressed {
                        palette.accent_active
                    } else if hot {
                        palette.accent_hover
                    } else {
                        palette.accent
                    };
                    let caption = if disabled {
                        hero::mix(fill, palette.on_accent, 50)
                    } else {
                        palette.on_accent
                    };
                    (fill, caption)
                } else if close {
                    if hot {
                        (palette.danger_soft, palette.danger_text)
                    } else {
                        (palette.page, palette.text_secondary)
                    }
                } else {
                    // Secondary actions: filled capsule, no border (v3 default).
                    let fill = if disabled {
                        palette.disabled_bg
                    } else if pressed || hot {
                        palette.stroke_hover
                    } else {
                        palette.toggle_off
                    };
                    (fill, palette.text_primary)
                };
                if close {
                    hero::surface_ex(dc, d.rcItem, s.dpi, fill, 8, None);
                } else {
                    hero::action_surface(dc, d.rcItem, s.dpi, fill);
                }
                text(
                    dc,
                    s.medium,
                    d.rcItem,
                    &ui::edit_text(d.hwndItem),
                    caption,
                    DT_CENTER | DT_VCENTER | DT_SINGLELINE,
                );
            });
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
            // Drag by the 48-DIP title bar only, matching the main window.
            if ((lp.0 >> 16) & 0xffff) < scale(48, s.dpi) as isize {
                let _ = SendMessageW(
                    hwnd,
                    WM_NCLBUTTONDOWN,
                    Some(WPARAM(HTCAPTION as usize)),
                    Some(lp),
                );
            }
            LRESULT(0)
        }
        WM_SETTINGCHANGE => {
            let is_dark = winutil::is_system_dark_mode();
            if is_dark != s.is_dark {
                s.is_dark = is_dark;
                let palette = winutil::Palette::for_dark(is_dark);
                delete_gdi(brush_as_gdi(s.card));
                delete_gdi(brush_as_gdi(s.field));
                s.card = solid_brush(palette.card);
                s.field = solid_brush(palette.control);
                winutil::set_window_dark_mode(hwnd, is_dark);
                for c in [s.desktop, s.menu, s.launch] {
                    ui::theme_checkbox(c, is_dark, s.dpi);
                }
                refresh(hwnd);
            }
            LRESULT(0)
        }
        WM_DPICHANGED => {
            if lp.0 != 0 {
                let suggested = &*(lp.0 as *const RECT);
                let _ = SetWindowPos(
                    hwnd,
                    None,
                    suggested.left,
                    suggested.top,
                    suggested.right - suggested.left,
                    suggested.bottom - suggested.top,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
                let new_dpi = (wp.0 & 0xffff) as u32;
                s.dpi = new_dpi;
                let new_font = create_font(14, new_dpi, false);
                let new_small = create_font(UI_CAPTION_SIZE, new_dpi, false);
                let new_title = winutil::create_font_weight(28, new_dpi, 600);
                let new_brand = winutil::create_font_weight(12, new_dpi, 600);
                let new_chip = create_font(12, new_dpi, false);
                let new_medium = winutil::create_font_weight(13, new_dpi, 500);
                let new_heading = winutil::create_font_weight(14, new_dpi, 600);
                for child in [
                    s.path,
                    s.browse,
                    s.desktop,
                    s.menu,
                    s.launch,
                    s.primary,
                    s.secondary,
                    s.close,
                ] {
                    ui::apply_font(child, new_font);
                }
                let old_fonts = [
                    s.font, s.small, s.title, s.brand, s.chip, s.medium, s.heading,
                ];
                s.font = new_font;
                s.small = new_small;
                s.title = new_title;
                s.brand = new_brand;
                s.chip = new_chip;
                s.medium = new_medium;
                s.heading = new_heading;
                for old in old_fonts {
                    delete_gdi(font_as_gdi(old));
                }
                layout_controls(s, new_dpi);
                let _ = InvalidateRect(Some(hwnd), None, false);
            }
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            let pt = POINT {
                x: (lp.0 & 0xffff) as i16 as i32,
                y: ((lp.0 >> 16) & 0xffff) as i16 as i32,
            };
            let child = ChildWindowFromPoint(hwnd, pt);
            let hot = if child == s.primary && IsWindowVisible(s.primary).as_bool() {
                PRIMARY
            } else if child == s.secondary && IsWindowVisible(s.secondary).as_bool() {
                SECONDARY
            } else if child == s.close && IsWindowVisible(s.close).as_bool() {
                CLOSE
            } else if child == s.browse && IsWindowVisible(s.browse).as_bool() {
                ui::ID_BROWSE
            } else {
                0
            };
            if hot != s.hot {
                let old = s.hot;
                s.hot = hot;
                invalidate_button(hwnd, s, old);
                invalidate_button(hwnd, s, hot);
            }
            let mut tme = TRACKMOUSEEVENT {
                cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                dwFlags: TME_LEAVE,
                hwndTrack: hwnd,
                dwHoverTime: 0,
            };
            let _ = TrackMouseEvent(&mut tme);
            LRESULT(0)
        }
        WM_SETCURSOR => {
            if ((lp.0 >> 16) & 0xffff) as u32 == WM_MOUSEMOVE {
                let child = HWND(wp.0 as *mut _);
                let hot = if child == s.primary && IsWindowVisible(s.primary).as_bool() {
                    PRIMARY
                } else if child == s.secondary && IsWindowVisible(s.secondary).as_bool() {
                    SECONDARY
                } else if child == s.close && IsWindowVisible(s.close).as_bool() {
                    CLOSE
                } else if child == s.browse && IsWindowVisible(s.browse).as_bool() {
                    ui::ID_BROWSE
                } else {
                    0
                };
                if hot != s.hot {
                    let old = s.hot;
                    s.hot = hot;
                    invalidate_button(hwnd, s, old);
                    invalidate_button(hwnd, s, hot);
                }
            }
            DefWindowProcW(hwnd, msg, wp, lp)
        }
        WM_MOUSELEAVE => {
            let mut pt = POINT::default();
            let _ = GetCursorPos(&mut pt);
            let mut rect = RECT::default();
            let _ = GetWindowRect(hwnd, &mut rect);
            if !PtInRect(&rect, pt).as_bool() {
                s.hot = 0;
                let _ = InvalidateRect(Some(hwnd), None, false);
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
            720,
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
