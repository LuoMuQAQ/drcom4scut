#![windows_subsystem = "windows"]

//! Native uninstaller: confirm UI, then a hashed copy deletes the install tree.

use std::path::PathBuf;
use std::time::Duration;

use drcom4scut_gui::install::UNINSTALL_MUTEX_NAME;
use drcom4scut_gui::install::flow::{UninstallPlan, plan_uninstall};
use drcom4scut_gui::install::selfdelete;
use drcom4scut_gui::install::ui;
use drcom4scut_gui::platform::{self, AlreadyRunning};
use drcom4scut_gui::ui::hero::{self, Icon};
use drcom4scut_gui::ui::winutil::*;
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DRAW_TEXT_FORMAT, DT_CENTER,
    DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER, DeleteDC, DrawTextW, EndPaint, FillRect, HBRUSH, HDC,
    HFONT, HGDIOBJ, InvalidateRect, MapWindowPoints, PAINTSTRUCT, PtInRect, SRCCOPY, SelectObject,
    SetBkMode, SetTextColor, TRANSPARENT,
};
use windows::Win32::UI::Controls::{DRAWITEMSTRUCT, ODS_SELECTED};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    TRACKMOUSEEVENT, TRACKMOUSEEVENT_FLAGS, TrackMouseEvent,
};
use windows::Win32::UI::WindowsAndMessaging::{
    ChildWindowFromPoint, DefWindowProcW, DestroyWindow, GWL_STYLE, GWLP_USERDATA, GetClientRect,
    GetCursorPos, GetWindowLongPtrW, GetWindowLongPtrW as GetStyle, GetWindowRect, HTCAPTION,
    IsWindowVisible, PostQuitMessage, SWP_NOACTIVATE, SWP_NOZORDER, SendMessageW,
    SetWindowLongPtrW, SetWindowLongPtrW as SetStyle, SetWindowPos, WM_CLOSE, WM_COMMAND,
    WM_CTLCOLORBTN, WM_CTLCOLORSTATIC, WM_DESTROY, WM_DPICHANGED, WM_DRAWITEM, WM_ERASEBKGND,
    WM_LBUTTONDOWN, WM_MOUSEMOVE, WM_NCLBUTTONDOWN, WM_PAINT, WM_SETCURSOR, WM_SETTINGCHANGE,
};
use windows::core::{PCWSTR, w};

const CLOSE: isize = 3003;
const TME_LEAVE: TRACKMOUSEEVENT_FLAGS = TRACKMOUSEEVENT_FLAGS(0x2);
const WM_MOUSELEAVE: u32 = 0x02A3;
const WIDTH: i32 = 560;
const HEIGHT: i32 = 672;

fn is_elevated() -> bool {
    drcom4scut_gui::install::sid::is_elevated()
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

fn relaunch_elevated(args: &[String]) -> Result<(), String> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::WaitForSingleObject;
    use windows::Win32::UI::Shell::{SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW};
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let exe_w: Vec<u16> = {
        use std::os::windows::ffi::OsStrExt;
        exe.as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    };
    let arguments = args
        .iter()
        .map(|a| quote_arg(a))
        .collect::<Vec<_>>()
        .join(" ");
    let args_w: Vec<u16> = arguments.encode_utf16().chain(std::iter::once(0)).collect();
    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS,
        lpVerb: w!("runas"),
        lpFile: PCWSTR(exe_w.as_ptr()),
        lpParameters: PCWSTR(args_w.as_ptr()),
        nShow: 1,
        ..Default::default()
    };
    unsafe {
        ShellExecuteExW(&mut info).map_err(|e| {
            if e.code().0 as u32 == 0x800704c7 {
                "已取消管理员授权，尚未开始卸载。".into()
            } else {
                format!("提权失败：{e}")
            }
        })?;
        if !info.hProcess.is_invalid() {
            let _ = WaitForSingleObject(info.hProcess, u32::MAX);
            let _ = CloseHandle(info.hProcess);
        }
    }
    Ok(())
}

fn run_cleanup(plan: &UninstallPlan) -> Vec<String> {
    drcom4scut_gui::install::flow::run_uninstall(plan)
}

fn origin_pid_from(args: &[String]) -> u32 {
    args.iter()
        .find_map(|a| a.strip_prefix("--origin-pid="))
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
}

fn is_cleanup_phase(args: &[String]) -> bool {
    args.iter().any(|a| a == "--cleanup-phase")
}

fn cleanup_phase(origin_pid: u32) -> i32 {
    let self_exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            ui::alert(HWND::default(), "卸载失败", &e.to_string());
            return 1;
        }
    };
    if !is_elevated() {
        return 1;
    }
    let code = cleanup_install(&self_exe, origin_pid);
    if let Err(e) = selfdelete::cleanup_copy(&self_exe) {
        ui::alert(HWND::default(), "卸载临时文件清理未完成", &e);
        return 1;
    }
    code
}

fn cleanup_install(self_exe: &std::path::Path, origin_pid: u32) -> i32 {
    let target = match selfdelete::read_cleanup_target(&self_exe) {
        Ok(t) => t,
        Err(e) => {
            ui::alert(HWND::default(), "卸载失败", &e);
            return 1;
        }
    };
    let install_dir = PathBuf::from(&target.install_dir);
    if drcom4scut_gui::install::validate::is_reparse_point(&install_dir) {
        ui::alert(HWND::default(), "卸载失败", "安装目录是重解析点，已停止。");
        return 1;
    }
    drcom4scut_gui::install::flow::leave_install_dir(&install_dir);
    if let Some(parent) = self_exe.parent() {
        let _ = std::env::set_current_dir(parent);
    }
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    while origin_pid != 0 && std::time::Instant::now() < deadline {
        use windows::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
        use windows::Win32::System::Threading::{
            OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject,
        };
        unsafe {
            if let Ok(h) = OpenProcess(PROCESS_SYNCHRONIZE, false, origin_pid) {
                let w = WaitForSingleObject(h, 200);
                let _ = CloseHandle(h);
                if w == WAIT_OBJECT_0 {
                    break;
                }
            } else {
                break;
            }
        }
    }
    let _guard = match drcom4scut_gui::install::maintenance::acquire() {
        Ok(g) => g,
        Err(e) => {
            ui::alert(HWND::default(), "卸载未开始", &e);
            return 1;
        }
    };
    let state = match drcom4scut_gui::install::identity::load_state(
        &install_dir.join("install-state.json"),
    ) {
        Ok(s) => s,
        Err(e) => {
            ui::alert(HWND::default(), "卸载失败", &e);
            return 1;
        }
    };
    if state.install_id != target.install_id
        || !drcom4scut_gui::install::identity::state_matches_dir(&state, &install_dir)
    {
        ui::alert(
            HWND::default(),
            "卸载失败",
            "卸载目标与受保护交接信息不一致。",
        );
        return 1;
    }
    let plan = UninstallPlan { install_dir, state };
    let leftover = run_cleanup(&plan);
    if leftover.is_empty() {
        0
    } else {
        ui::alert(
            HWND::default(),
            "卸载未完全完成",
            &format!(
                "卸载入口已保留，请关闭占用或重启后重新运行卸载：\n{}",
                leftover.join("\n")
            ),
        );
        1
    }
}

fn launch_cleanup() -> Result<(), String> {
    let self_exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let plan = plan_uninstall(&self_exe)?;
    let copy = selfdelete::copy_self_to_secure_temp(&self_exe, &plan.state.install_id)?;
    let launch = (|| -> Result<(), String> {
        selfdelete::write_cleanup_target(&copy, &plan.install_dir, &plan.state.install_id)?;
        use std::os::windows::process::CommandExt;
        let mut cmd = std::process::Command::new(&copy);
        cmd.arg("--cleanup-phase")
            .arg(format!("--origin-pid={}", std::process::id()))
            .creation_flags(0);
        if let Some(parent) = copy.parent() {
            cmd.current_dir(parent);
        }
        cmd.spawn().map_err(|e| format!("无法启动清理进程：{e}"))?;
        Ok(())
    })();
    if let Err(e) = launch {
        if let Err(cleanup) = selfdelete::cleanup_copy(&copy) {
            return Err(format!("{e}；{cleanup}"));
        }
        return Err(e);
    }
    drcom4scut_gui::install::flow::leave_install_dir(&plan.install_dir);
    Ok(())
}

struct UnUi {
    is_dark: bool,
    dpi: u32,
    font: HFONT,
    small: HFONT,
    title: HFONT,
    background: HBRUSH,
    btn_ok: HWND,
    btn_cancel: HWND,
    btn_close: HWND,
    hot: isize,
}

unsafe fn invalidate_button(hwnd: HWND, s: &UnUi, id: isize) {
    let btn = match id {
        ui::ID_OK => s.btn_ok,
        ui::ID_CANCEL => s.btn_cancel,
        CLOSE => s.btn_close,
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

unsafe fn layout_controls(s: &UnUi, dpi: u32) {
    let r = rect(dpi, 410, 616, 120, 40);
    let _ = SetWindowPos(
        s.btn_ok,
        None,
        r.left,
        r.top,
        r.right - r.left,
        r.bottom - r.top,
        SWP_NOZORDER | SWP_NOACTIVATE,
    );
    let r = rect(dpi, 304, 616, 96, 40);
    let _ = SetWindowPos(
        s.btn_cancel,
        None,
        r.left,
        r.top,
        r.right - r.left,
        r.bottom - r.top,
        SWP_NOZORDER | SWP_NOACTIVATE,
    );
    let r = rect(dpi, 516, 10, 28, 28);
    let _ = SetWindowPos(
        s.btn_close,
        None,
        r.left,
        r.top,
        r.right - r.left,
        r.bottom - r.top,
        SWP_NOZORDER | SWP_NOACTIVATE,
    );
}

impl Drop for UnUi {
    fn drop(&mut self) {
        for obj in [
            font_as_gdi(self.font),
            font_as_gdi(self.small),
            font_as_gdi(self.title),
            brush_as_gdi(self.background),
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

unsafe fn owner_button(hwnd: HWND, id: isize, r: RECT, text: &str, font: HFONT) -> HWND {
    let b = ui::create_child_button(
        hwnd,
        id,
        r.left,
        r.top,
        r.right - r.left,
        r.bottom - r.top,
        text,
    );
    SetStyle(b, GWL_STYLE, (GetStyle(b, GWL_STYLE) & !15) | 11);
    drcom4scut_gui::ui::winutil::suppress_button_erase(b);
    ui::apply_font(b, font);
    b
}

unsafe fn draw_text(
    hdc: HDC,
    font: HFONT,
    r: RECT,
    value: &str,
    color: windows::Win32::Foundation::COLORREF,
    flags: DRAW_TEXT_FORMAT,
) {
    let old = SelectObject(hdc, font_as_gdi(font));
    let _ = SetTextColor(hdc, color);
    let _ = SetBkMode(hdc, TRANSPARENT);
    let mut r = r;
    let mut value: Vec<u16> = value.encode_utf16().collect();
    let _ = DrawTextW(hdc, &mut value, &mut r, flags | DT_NOPREFIX);
    let _ = SelectObject(hdc, old);
}

unsafe fn paint_action(dc: HDC, r: RECT, s: &UnUi, id: isize, hot: bool) {
    let p = Palette::for_dark(s.is_dark);
    hero::line(dc, r, p.page);
    let (label, fill, color) = if id == ui::ID_OK {
        (
            "卸载",
            if hot { p.danger_hover } else { p.danger },
            COLORREF(0xFFFFFF),
        )
    } else if id == CLOSE {
        (
            "×",
            if hot { hero::soft(p, p.danger) } else { p.page },
            p.text_primary,
        )
    } else {
        (
            "取消",
            if hot { p.stroke } else { p.toggle_off },
            p.text_primary,
        )
    };
    hero::surface(dc, r, s.dpi, fill, 24);
    draw_text(
        dc,
        s.font,
        r,
        label,
        color,
        DT_CENTER | DT_VCENTER | DT_SINGLELINE,
    );
}

unsafe fn paint_to_dc(dc: HDC, bounds: RECT, s: &UnUi) {
    let mem = CreateCompatibleDC(Some(dc));
    let bmp = CreateCompatibleBitmap(dc, bounds.right, bounds.bottom);
    let old = SelectObject(mem, HGDIOBJ(bmp.0));
    let palette = Palette::for_dark(s.is_dark);
    let bg = solid_brush(palette.page);
    let _ = FillRect(mem, &bounds, bg);
    delete_gdi(brush_as_gdi(bg));
    let r = |x, y, w, h| rect(s.dpi, x, y, w, h);
    hero::line(mem, r(0, 47, 560, 1), palette.stroke);
    hero::logo(mem, r(16, 11, 26, 26));
    draw_text(
        mem,
        s.small,
        r(46, 0, 420, 48),
        "drcom4scut 卸载程序",
        palette.text_primary,
        DT_SINGLELINE | DT_VCENTER,
    );
    hero::surface(
        mem,
        r(30, 78, 44, 44),
        s.dpi,
        hero::soft(palette, palette.danger),
        14,
    );
    hero::icon(mem, r(41, 89, 22, 22), palette.danger_text, Icon::Trash);
    draw_text(
        mem,
        s.title,
        r(30, 144, 500, 42),
        "卸载校园网客户端？",
        palette.text_primary,
        DT_SINGLELINE,
    );
    draw_text(
        mem,
        s.small,
        r(30, 196, 500, 24),
        "将从这台电脑移除 drcom4scut 及以下内容。",
        palette.text_secondary,
        DT_SINGLELINE,
    );
    hero::surface(mem, r(30, 230, 500, 214), s.dpi, palette.card, 24);
    for (i, (title, detail, icon)) in [
        ("账号与设置", "保存的账号、密码及客户端配置", Icon::User),
        ("本地程序文件", "客户端、已释放核心和日志", Icon::Folder),
        (
            "快捷方式与系统条目",
            "桌面、开始菜单快捷方式及卸载条目",
            Icon::Monitor,
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let y = 244 + i as i32 * 68;
        hero::icon(mem, r(48, y + 14, 20, 20), palette.text_secondary, icon);
        draw_text(
            mem,
            s.font,
            r(82, y, 430, 25),
            title,
            palette.text_primary,
            DT_SINGLELINE,
        );
        draw_text(
            mem,
            s.small,
            r(82, y + 28, 430, 24),
            detail,
            palette.text_secondary,
            DT_SINGLELINE,
        );
        if i < 2 {
            hero::line(mem, r(48, y + 59, 464, 1), palette.stroke);
        }
    }
    hero::surface(mem, r(30, 462, 500, 48), s.dpi, palette.toggle_off, 12);
    hero::icon(mem, r(44, 477, 18, 18), palette.text_secondary, Icon::Check);
    draw_text(
        mem,
        s.small,
        r(74, 462, 438, 48),
        "保留系统中的 Npcap / WinPcap 网络驱动。",
        palette.text_primary,
        DT_VCENTER | DT_SINGLELINE,
    );
    hero::surface(
        mem,
        r(30, 528, 500, 48),
        s.dpi,
        hero::soft(palette, palette.danger),
        12,
    );
    hero::icon(mem, r(44, 543, 18, 18), palette.danger_text, Icon::Info);
    draw_text(
        mem,
        s.small,
        r(74, 528, 438, 48),
        "此操作无法撤销，账号设置与日志将被删除。",
        palette.danger_text,
        DT_VCENTER | DT_SINGLELINE,
    );
    hero::line(mem, r(0, 600, 560, 1), palette.stroke);
    draw_text(
        mem,
        s.small,
        r(30, 616, 200, 40),
        "drcom4scut",
        palette.text_secondary,
        DT_VCENTER | DT_SINGLELINE,
    );
    if s.btn_ok.0.is_null() {
        paint_action(mem, r(410, 616, 120, 40), s, ui::ID_OK, false);
        paint_action(mem, r(304, 616, 96, 40), s, ui::ID_CANCEL, false);
        paint_action(mem, r(516, 10, 28, 28), s, CLOSE, false);
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
    let _ = SelectObject(mem, old);
    let _ = windows::Win32::Graphics::Gdi::DeleteObject(HGDIOBJ(bmp.0));
    let _ = DeleteDC(mem);
}

unsafe fn paint(hwnd: HWND, s: &UnUi) {
    let mut ps = PAINTSTRUCT::default();
    let dc = BeginPaint(hwnd, &mut ps);
    let mut bounds = RECT::default();
    let _ = GetClientRect(hwnd, &mut bounds);
    paint_to_dc(dc, bounds, s);
    let _ = EndPaint(hwnd, &ps);
}

fn begin_uninstall(hwnd: HWND) {
    if !ui::confirm(
        hwnd,
        "确认卸载",
        "将永久删除本次安装目录内的账号设置、配置、日志和已释放核心，并移除快捷方式与系统卸载条目。\n不会卸载系统中的 Npcap/WinPcap。\n\n确定卸载？",
    ) {
        return;
    }
    match launch_cleanup() {
        Ok(()) => unsafe {
            let _ = DestroyWindow(hwnd);
        },
        Err(e) => ui::alert(hwnd, "卸载失败", &e),
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if msg == windows::Win32::UI::WindowsAndMessaging::WM_CREATE {
        let dpi = window_dpi(hwnd);
        let is_dark = is_system_dark_mode();
        let font = create_font(14, dpi, false);
        let small = create_font(UI_CAPTION_SIZE, dpi, false);
        let title = create_font(28, dpi, true);
        let btn_ok = owner_button(hwnd, ui::ID_OK, rect(dpi, 410, 616, 120, 40), "卸载", font);
        let btn_cancel = owner_button(
            hwnd,
            ui::ID_CANCEL,
            rect(dpi, 304, 616, 96, 40),
            "取消",
            font,
        );
        let btn_close = owner_button(hwnd, CLOSE, rect(dpi, 516, 10, 28, 28), "×", font);
        ui::round_corners(hwnd);
        set_window_dark_mode(hwnd, is_dark);
        let _ = windows::Win32::UI::Input::KeyboardAndMouse::SetFocus(Some(btn_cancel));
        let state = Box::new(UnUi {
            is_dark,
            dpi,
            font,
            small,
            title,
            background: solid_brush(Palette::for_dark(is_dark).page),
            btn_ok,
            btn_cancel,
            btn_close,
            hot: 0,
        });
        layout_controls(&state, dpi);
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(state) as isize);
        return LRESULT(0);
    }
    let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut UnUi;
    if ptr.is_null() {
        return DefWindowProcW(hwnd, msg, wparam, lparam);
    }
    let s = &mut *ptr;
    match msg {
        WM_PAINT => {
            paint(hwnd, s);
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        WM_SETTINGCHANGE => {
            let is_dark = is_system_dark_mode();
            if is_dark != s.is_dark {
                s.is_dark = is_dark;
                let new_brush = solid_brush(Palette::for_dark(is_dark).page);
                let old_brush = std::mem::replace(&mut s.background, new_brush);
                delete_gdi(brush_as_gdi(old_brush));
                set_window_dark_mode(hwnd, is_dark);
                let _ = InvalidateRect(Some(hwnd), None, false);
            }
            LRESULT(0)
        }
        WM_CTLCOLORSTATIC | WM_CTLCOLORBTN => {
            let dc = HDC(wparam.0 as *mut _);
            let palette = Palette::for_dark(s.is_dark);
            let _ = SetTextColor(dc, palette.text_primary);
            let _ = SetBkMode(dc, TRANSPARENT);
            LRESULT(s.background.0 as isize)
        }
        WM_DRAWITEM => {
            let d = &*(lparam.0 as *const DRAWITEMSTRUCT);
            drcom4scut_gui::ui::winutil::paint_buffered(d.hDC, d.rcItem, |dc| {
                paint_action(
                    dc,
                    d.rcItem,
                    s,
                    d.CtlID as isize,
                    s.hot == d.CtlID as isize || d.itemState.0 & ODS_SELECTED.0 != 0,
                );
            });
            LRESULT(1)
        }
        WM_COMMAND => {
            match (wparam.0 as u16) as isize {
                ui::ID_OK | 1 => begin_uninstall(hwnd),
                ui::ID_CANCEL | CLOSE | 2 => {
                    let _ = DestroyWindow(hwnd);
                }
                _ => {}
            }
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            if ((lparam.0 >> 16) & 0xffff) < scale(64, s.dpi) as isize {
                let _ = SendMessageW(
                    hwnd,
                    WM_NCLBUTTONDOWN,
                    Some(WPARAM(HTCAPTION as usize)),
                    Some(lparam),
                );
            }
            LRESULT(0)
        }
        WM_DPICHANGED => {
            if lparam.0 != 0 {
                let suggested = &*(lparam.0 as *const RECT);
                let _ = SetWindowPos(
                    hwnd,
                    None,
                    suggested.left,
                    suggested.top,
                    suggested.right - suggested.left,
                    suggested.bottom - suggested.top,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
                let new_dpi = (wparam.0 & 0xffff) as u32;
                s.dpi = new_dpi;
                let new_font = create_font(14, new_dpi, false);
                let new_small = create_font(UI_CAPTION_SIZE, new_dpi, false);
                let new_title = create_font(28, new_dpi, true);
                for child in [s.btn_ok, s.btn_cancel, s.btn_close] {
                    ui::apply_font(child, new_font);
                }
                let old_fonts = [s.font, s.small, s.title];
                s.font = new_font;
                s.small = new_small;
                s.title = new_title;
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
                x: (lparam.0 & 0xffff) as i16 as i32,
                y: ((lparam.0 >> 16) & 0xffff) as i16 as i32,
            };
            let child = ChildWindowFromPoint(hwnd, pt);
            let hot = if child == s.btn_ok && IsWindowVisible(s.btn_ok).as_bool() {
                ui::ID_OK
            } else if child == s.btn_cancel && IsWindowVisible(s.btn_cancel).as_bool() {
                ui::ID_CANCEL
            } else if child == s.btn_close && IsWindowVisible(s.btn_close).as_bool() {
                CLOSE
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
            if ((lparam.0 >> 16) & 0xffff) as u32 == WM_MOUSEMOVE {
                let child = HWND(wparam.0 as *mut _);
                let hot = if child == s.btn_ok && IsWindowVisible(s.btn_ok).as_bool() {
                    ui::ID_OK
                } else if child == s.btn_cancel && IsWindowVisible(s.btn_cancel).as_bool() {
                    ui::ID_CANCEL
                } else if child == s.btn_close && IsWindowVisible(s.btn_close).as_bool() {
                    CLOSE
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
            DefWindowProcW(hwnd, msg, wparam, lparam)
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
            let _ = DestroyWindow(hwnd);
            LRESULT(0)
        }
        WM_DESTROY => {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            drop(Box::from_raw(ptr));
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

fn quiet_mode() -> bool {
    std::env::args().any(|a| a == "--quiet")
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if is_cleanup_phase(&args) {
        std::process::exit(cleanup_phase(origin_pid_from(&args)));
    }
    if !is_elevated() {
        let rest: Vec<String> = args.iter().skip(1).cloned().collect();
        if let Err(e) = relaunch_elevated(&rest) {
            ui::alert(HWND::default(), "卸载失败", &e);
        }
        return;
    }
    match platform::acquire(UNINSTALL_MUTEX_NAME) {
        Ok(_guard) => run_elevated_ui(),
        Err(AlreadyRunning::InstanceRunning) => {}
        Err(_) => run_elevated_ui(),
    }
}

fn run_elevated_ui() {
    if quiet_mode() {
        if let Err(e) = launch_cleanup() {
            ui::alert(HWND::default(), "卸载失败", &e);
        }
        return;
    }
    let hwnd = match ui::create_popup(
        w!("DrcomUninstall"),
        "卸载校园网认证客户端",
        WIDTH,
        HEIGHT,
        Some(wndproc),
    ) {
        Some(h) => h,
        None => {
            ui::alert(HWND::default(), "卸载失败", "无法创建卸载窗口。");
            return;
        }
    };
    let _ = hwnd;
    ui::run_message_loop(hwnd);
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Graphics::Gdi::GdiFlush;

    #[test]
    fn cleanup_phase_does_not_need_install_dir() {
        let args = vec![
            "uninstall.exe".into(),
            "--cleanup-phase".into(),
            "--origin-pid=12".into(),
        ];
        assert!(is_cleanup_phase(&args));
        assert_eq!(origin_pid_from(&args), 12);
        assert!(!is_cleanup_phase(&["uninstall.exe".into()]));
    }

    #[test]
    fn quote_arg_covers_spaces() {
        assert_eq!(quote_arg("--quiet"), "\"--quiet\"");
        assert_eq!(
            quote_arg(r"C:\Program Files\app"),
            r#""C:\Program Files\app""#
        );
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

    #[test]
    #[ignore = "explicit offscreen uninstall snapshot rendering and PNG artifacts"]
    fn render_heroui_uninstall_snapshots() {
        unsafe {
            let out_dir = std::path::PathBuf::from(
                std::env::var_os("DRCOM_UI_ARTIFACT_DIR").expect("set artifact directory"),
            );
            let _ = std::fs::create_dir_all(&out_dir);

            for (mode_name, is_dark) in [("light", false), ("dark", true)] {
                for dpi in [96, 120, 144, 192] {
                    let w = scale(WIDTH, dpi);
                    let h = scale(HEIGHT, dpi);
                    let bounds = RECT {
                        left: 0,
                        top: 0,
                        right: w,
                        bottom: h,
                    };

                    let font = create_font(14, dpi, false);
                    let small = create_font(UI_CAPTION_SIZE, dpi, false);
                    let title = create_font(28, dpi, true);

                    let state = UnUi {
                        is_dark,
                        dpi,
                        font,
                        small,
                        title,
                        background: solid_brush(Palette::for_dark(is_dark).page),
                        btn_ok: HWND::default(),
                        btn_cancel: HWND::default(),
                        btn_close: HWND::default(),
                        hot: 0,
                    };

                    let canvas = TestCanvas::new(w, h);
                    paint_to_dc(canvas.dc, bounds, &state);
                    canvas.save_png(&out_dir.join(format!("uninstall-{mode_name}-{dpi}dpi.png")));
                }
            }
        }
    }
}
