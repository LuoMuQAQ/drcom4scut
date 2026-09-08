#![windows_subsystem = "windows"]

//! Native uninstaller: confirm UI, then a hashed copy deletes the install tree.

use std::path::PathBuf;
use std::time::Duration;

use drcom4scut_gui::install::flow::{plan_uninstall, UninstallPlan};
use drcom4scut_gui::install::selfdelete;
use drcom4scut_gui::install::ui;
use drcom4scut_gui::install::UNINSTALL_MUTEX_NAME;
use drcom4scut_gui::platform::{self, AlreadyRunning};
use drcom4scut_gui::ui::winutil::*;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, CreateRoundRectRgn, DeleteDC,
    DrawFocusRect, DrawTextW, EndPaint, FillRect, InflateRect, SelectObject, SetBkMode,
    SetTextColor, SetWindowRgn, DRAW_TEXT_FORMAT, DT_CENTER, DT_NOPREFIX, DT_SINGLELINE,
    DT_VCENTER, DT_WORDBREAK, HDC, HFONT, HGDIOBJ, PAINTSTRUCT, SRCCOPY, TRANSPARENT,
};
use windows::Win32::UI::Controls::{DRAWITEMSTRUCT, ODS_FOCUS, ODS_SELECTED};
use windows::Win32::UI::WindowsAndMessaging::{
    DefWindowProcW, DestroyWindow, GetClientRect, GetWindowLongPtrW, GetWindowLongPtrW as GetStyle,
    PostQuitMessage, SendMessageW, SetWindowLongPtrW, SetWindowLongPtrW as SetStyle, GWLP_USERDATA,
    GWL_STYLE, HTCAPTION, WM_CLOSE, WM_COMMAND, WM_CTLCOLORBTN, WM_CTLCOLORSTATIC, WM_DESTROY,
    WM_DRAWITEM, WM_ERASEBKGND, WM_LBUTTONDOWN, WM_NCLBUTTONDOWN, WM_PAINT,
};

const CLOSE: isize = 3003;
const WIDTH: i32 = 480;
const HEIGHT: i32 = 340;

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
    use windows::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};
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
            OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE,
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
    dpi: u32,
    font: HFONT,
    small: HFONT,
    title: HFONT,
}

impl Drop for UnUi {
    fn drop(&mut self) {
        for obj in [
            font_as_gdi(self.font),
            font_as_gdi(self.small),
            font_as_gdi(self.title),
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

unsafe fn paint(hwnd: HWND, s: &UnUi) {
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
    draw_text(
        mem,
        s.title,
        r(24, 28, 400, 36),
        "卸载校园网认证客户端",
        COLOR_TEXT_PRIMARY,
        DT_SINGLELINE,
    );
    fill_component(mem, r(24, 84, 432, 168), s.dpi, COLOR_CARD, None);
    draw_text(
        mem,
        s.font,
        r(40, 100, 400, 28),
        "将删除本次安装",
        COLOR_TEXT_PRIMARY,
        DT_SINGLELINE,
    );
    draw_text(
        mem,
        s.small,
        r(40, 136, 400, 96),
        "账号设置、配置、日志、已释放核心、快捷方式和系统卸载条目都会被移除。\n不会卸载系统中的 Npcap / WinPcap。\n此操作无法撤销。",
        COLOR_TEXT_SECONDARY,
        DT_WORDBREAK,
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
    let _ = SelectObject(mem, old);
    let _ = windows::Win32::Graphics::Gdi::DeleteObject(HGDIOBJ(bmp.0));
    let _ = DeleteDC(mem);
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
        let dpi = screen_dpi();
        let font = create_font(14, dpi, false);
        let small = create_font(12, dpi, false);
        let title = create_font(22, dpi, true);
        let _ok = owner_button(hwnd, ui::ID_OK, rect(dpi, 336, 276, 120, 40), "卸载", font);
        let _cancel = owner_button(
            hwnd,
            ui::ID_CANCEL,
            rect(dpi, 228, 276, 96, 40),
            "取消",
            font,
        );
        owner_button(hwnd, CLOSE, rect(dpi, 428, 16, 28, 28), "×", font);
        let region = CreateRoundRectRgn(
            0,
            0,
            scale(WIDTH, dpi) + 1,
            scale(HEIGHT, dpi) + 1,
            scale(16, dpi),
            scale(16, dpi),
        );
        if SetWindowRgn(hwnd, Some(region), true) == 0 {
            delete_gdi(HGDIOBJ(region.0));
        }
        let state = Box::new(UnUi {
            dpi,
            font,
            small,
            title,
        });
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
        WM_CTLCOLORSTATIC | WM_CTLCOLORBTN => {
            let dc = HDC(wparam.0 as *mut _);
            let _ = SetTextColor(dc, COLOR_TEXT_PRIMARY);
            let _ = SetBkMode(dc, TRANSPARENT);
            LRESULT(solid_brush(COLOR_PAGE).0 as isize)
        }
        WM_DRAWITEM => {
            let d = &*(lparam.0 as *const DRAWITEMSTRUCT);
            let background = solid_brush(COLOR_PAGE);
            let _ = FillRect(d.hDC, &d.rcItem, background);
            delete_gdi(brush_as_gdi(background));
            let uninstall = d.CtlID == ui::ID_OK as u32;
            let fill = if uninstall {
                if d.itemState.0 & ODS_SELECTED.0 != 0 {
                    COLOR_ACCENT_HOVER
                } else {
                    COLOR_DANGER
                }
            } else {
                COLOR_PAGE
            };
            fill_component(
                d.hDC,
                d.rcItem,
                s.dpi,
                fill,
                if uninstall { None } else { Some(COLOR_STROKE) },
            );
            draw_text(
                d.hDC,
                s.font,
                d.rcItem,
                &ui::edit_text(d.hwndItem),
                if uninstall {
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
}
