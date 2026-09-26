//! Main-window presentation and native keyboard controls for the approved v3 sketch.
use super::*;
use crate::ui::hero::{self, Icon};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetFocus, SetFocus};
use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::*;

const BUTTONS: [Hit; 8] = [
    Hit::TabConnect,
    Hit::TabSettings,
    Hit::Combo,
    Hit::Remember,
    Hit::Connect,
    Hit::Auto,
    Hit::Startup,
    Hit::TrayKeep,
];
const BASE: usize = 1100;
const WM_MOUSELEAVE: u32 = 0x02a3;

pub(super) fn bounds(app: &App, hit: Hit) -> Option<RECT> {
    let r = |x, y, w, h| hero::rect(app.dpi, x, y, w, h);
    match hit {
        Hit::TabConnect => Some(r(34, layout::TABS_TOP + 4, 204, 32)),
        Hit::TabSettings => Some(r(242, layout::TABS_TOP + 4, 204, 32)),
        Hit::Combo if !app.preferences => Some(r(30, layout::COMBO_TOP, 420, 46)),
        Hit::Connect if !app.preferences => Some(r(30, layout::ACTION_TOP, 420, 44)),
        Hit::Remember if !app.preferences => Some(r(30, layout::REMEMBER_TOP, 150, 36)),
        Hit::Auto | Hit::Remember | Hit::Startup | Hit::TrayKeep if app.preferences => {
            let index = match hit {
                Hit::Auto => 0,
                Hit::Remember => 1,
                Hit::Startup => 2,
                _ => 3,
            };
            Some(r(
                30,
                layout::SETTINGS_TOP + index * layout::SETTINGS_ROW,
                420,
                68,
            ))
        }
        _ => None,
    }
}

fn label(app: &App, hit: Hit) -> String {
    let disconnect = app.desired_running
        || matches!(
            app.link_state,
            LinkState::Online | LinkState::Connecting | LinkState::Waiting
        );
    match hit {
        Hit::TabConnect => "连接".into(),
        Hit::TabSettings => "偏好设置".into(),
        Hit::Combo => format!("网络适配器：{}", combo_label(app)),
        Hit::Connect => if disconnect {
            "断开连接"
        } else {
            "连接网络"
        }
        .into(),
        _ => {
            let (name, on) = match hit {
                Hit::Auto => ("启动后自动连接", app.settings.auto_login),
                Hit::Remember => ("记住密码", app.settings.remember_password),
                Hit::Startup => ("开机启动", app.settings.start_with_windows),
                _ => ("保留系统托盘", app.settings.minimize_to_tray),
            };
            format!("{name}：{}", if on { "已开启" } else { "已关闭" })
        }
    }
}

pub(super) unsafe fn create_buttons(hwnd: HWND) {
    for (index, hit) in BUTTONS.into_iter().enumerate() {
        let child = create_child(
            hwnd,
            w!("BUTTON"),
            w!(""),
            child_style(11),
            WINDOW_EX_STYLE(0),
            0,
            0,
            0,
            0,
            (BASE + index) as isize,
        );
        let _ = SetWindowSubclass(child, Some(button_proc), 1, 0);
        if let Some(app) = app_mut(hwnd) {
            app.buttons.push((hit, child));
            set_font(child, app.font);
        }
    }
    if let Some(app) = app_mut(hwnd) {
        // Native dialog navigation follows child Z order. Keep it aligned with
        // the visible form, including STATIC labels preceding their EDITs.
        let mut order = vec![
            app.buttons[0].1,
            app.buttons[1].1,
            app.hwnd_user_label,
            app.hwnd_user,
            app.hwnd_pass_label,
            app.hwnd_pass,
            app.hwnd_eye,
        ];
        order.extend(app.buttons.iter().skip(2).map(|(_, child)| *child));
        for child in order {
            let _ = SetWindowPos(
                child,
                Some(HWND_BOTTOM),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
        }
    }
    layout_buttons(hwnd);
}

pub(super) unsafe fn layout_buttons(hwnd: HWND) {
    let Some(app) = app_mut(hwnd) else { return };
    for (hit, child) in &app.buttons {
        if let Some(r) = bounds(app, *hit) {
            let _ = SetWindowPos(
                *child,
                None,
                r.left,
                r.top,
                r.right - r.left,
                r.bottom - r.top,
                SWP_NOACTIVATE | SWP_NOZORDER,
            );
            let _ = ShowWindow(*child, SW_SHOWNA);
            let _ = SetWindowTextW(*child, PCWSTR(wide(&label(app, *hit)).as_ptr()));
            set_font(*child, app.font);
        } else {
            let _ = ShowWindow(*child, SW_HIDE);
        }
    }
    for (label, top) in [
        (app.hwnd_user_label, layout::USER_TOP),
        (app.hwnd_pass_label, layout::PASS_TOP),
    ] {
        let r = hero::rect(app.dpi, 30, top - 26, 420, 20);
        let _ = SetWindowPos(
            label,
            None,
            r.left,
            r.top,
            r.right - r.left,
            r.bottom - r.top,
            SWP_NOACTIVATE | SWP_NOZORDER,
        );
        set_font(label, app.font);
    }
    for child in [
        app.hwnd_user,
        app.hwnd_pass,
        app.hwnd_eye,
        app.hwnd_user_label,
        app.hwnd_pass_label,
    ] {
        let _ = ShowWindow(child, if app.preferences { SW_HIDE } else { SW_SHOWNA });
    }
}

pub(super) unsafe fn switch_page(hwnd: HWND, preferences: bool) {
    dismiss_combo(hwnd);
    if let Some(app) = app_mut(hwnd) {
        app.preferences = preferences;
    }
    layout_buttons(hwnd);
    if let Some(app) = app_mut(hwnd) {
        let target = if preferences {
            Hit::TabSettings
        } else {
            Hit::TabConnect
        };
        if let Some((_, child)) = app.buttons.iter().find(|(h, _)| *h == target) {
            let _ = SetFocus(Some(*child));
        }
    }
    invalidate(hwnd);
}

pub(super) unsafe fn command(hwnd: HWND, id: usize) -> bool {
    let Some(hit) = id.checked_sub(BASE).and_then(|i| BUTTONS.get(i)).copied() else {
        return false;
    };
    match hit {
        Hit::TabConnect => switch_page(hwnd, false),
        Hit::TabSettings => switch_page(hwnd, true),
        Hit::Combo => toggle_combo(hwnd),
        Hit::Connect => on_action(hwnd),
        Hit::Auto => toggle_flag(hwnd, 0, |s| s.auto_login = !s.auto_login),
        Hit::Remember => toggle_flag(hwnd, 1, |s| s.remember_password = !s.remember_password),
        Hit::Startup => toggle_flag(hwnd, 2, |s| s.start_with_windows = !s.start_with_windows),
        Hit::TrayKeep => toggle_flag(hwnd, 3, |s| s.minimize_to_tray = !s.minimize_to_tray),
        _ => {}
    }
    if let Some(app) = app_mut(hwnd) {
        for (hit, child) in &app.buttons {
            sync_button(app, *hit, *child);
        }
    }
    true
}

unsafe extern "system" fn button_proc(
    child: HWND,
    msg: u32,
    wp: WPARAM,
    lp: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    if msg == WM_ERASEBKGND {
        return LRESULT(1);
    }
    if msg == WM_NCDESTROY {
        let _ = RemoveWindowSubclass(child, Some(button_proc), 1);
    }
    if let Ok(parent) = GetParent(child) {
        if msg == WM_KEYDOWN && GetDlgCtrlID(child) as usize == BASE + 2 {
            if let Some(app) = app_mut(parent) {
                if app.combo_open && matches!(wp.0, 13 | 27) {
                    if wp.0 == 13 {
                        app.combo_sel = app.combo_hot.clamp(0, app.adapters.len() as i32);
                    }
                    close_combo(parent);
                    return LRESULT(0);
                }
                if matches!(wp.0, 38 | 40 | 36 | 35) {
                    if !app.combo_open {
                        toggle_combo(parent);
                    }
                    if let Some(app) = app_mut(parent) {
                        let last = app.adapters.len() as i32;
                        app.combo_hot = match wp.0 {
                            36 => 0,
                            35 => last,
                            38 => (app.combo_hot - 1).max(0),
                            _ => (app.combo_hot + 1).min(last),
                        };
                        let row = app.s(40);
                        let mut rect = RECT::default();
                        let _ = GetClientRect(app.hwnd_popup, &mut rect);
                        let visible = (rect.bottom - app.s(36)).max(row);
                        if app.combo_hot * row < app.combo_scroll {
                            app.combo_scroll = app.combo_hot * row;
                        }
                        if (app.combo_hot + 1) * row > app.combo_scroll + visible {
                            app.combo_scroll = (app.combo_hot + 1) * row - visible;
                        }
                        show_combo_popup(parent);
                    }
                    return LRESULT(0);
                }
            }
        }
        if msg == WM_KEYDOWN && wp.0 == 13 {
            let id = GetDlgCtrlID(child) as usize;
            let _ = SendMessageW(
                parent,
                WM_COMMAND,
                Some(WPARAM(id)),
                Some(LPARAM(child.0 as isize)),
            );
            return LRESULT(0);
        }
        if matches!(
            msg,
            WM_SETFOCUS | WM_KILLFOCUS | WM_MOUSEMOVE | WM_MOUSELEAVE
        ) {
            let mut changed = matches!(msg, WM_SETFOCUS | WM_KILLFOCUS);
            if let Some(app) = app_mut(parent) {
                let hit = app
                    .buttons
                    .iter()
                    .find(|(_, h)| *h == child)
                    .map(|(hit, _)| *hit)
                    .unwrap_or(Hit::None);
                if msg == WM_MOUSEMOVE && app.hover != hit {
                    let previous = app.hover;
                    app.hover = hit;
                    if !repaint_button(app, previous) {
                        invalidate_hover(parent, previous);
                    }
                    changed = true;
                    use windows::Win32::UI::Input::KeyboardAndMouse::*;
                    let mut tracking = TRACKMOUSEEVENT {
                        cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                        dwFlags: TME_LEAVE,
                        hwndTrack: child,
                        dwHoverTime: 0,
                    };
                    let _ = TrackMouseEvent(&mut tracking);
                }
                if msg == WM_MOUSELEAVE && app.hover == hit {
                    app.hover = Hit::None;
                    changed = true;
                }
            }
            if changed {
                let _ = InvalidateRect(Some(child), None, false);
            }
        }
    }
    DefSubclassProc(child, msg, wp, lp)
}

pub(super) unsafe fn draw_item(
    hwnd: HWND,
    d: &windows::Win32::UI::Controls::DRAWITEMSTRUCT,
) -> bool {
    let Some(hit) = (d.CtlID as usize)
        .checked_sub(BASE)
        .and_then(|i| BUTTONS.get(i))
        .copied()
    else {
        return false;
    };
    let Some(app) = app_mut(hwnd) else {
        return false;
    };
    let Some(r) = bounds(app, hit) else {
        return true;
    };
    let pressed = d.itemState.0 & 0x1 != 0; // ODS_SELECTED
    winutil::paint_buffered(d.hDC, d.rcItem, |dc| {
        let _ = OffsetViewportOrgEx(dc, -r.left, -r.top, None);
        paint_button(app, dc, hit, pressed);
        let _ = OffsetViewportOrgEx(dc, r.left, r.top, None);
    });
    true
}

/// Returns true for native controls, false for the parent-only fixture renderer.
pub(super) unsafe fn repaint_button(app: &App, hit: Hit) -> bool {
    let Some((_, child)) = app.buttons.iter().find(|(h, _)| *h == hit) else {
        return false;
    };
    sync_button(app, hit, *child);
    if bounds(app, hit).is_some() {
        let _ = InvalidateRect(Some(*child), None, false);
    }
    true
}

unsafe fn sync_button(app: &App, hit: Hit, child: HWND) {
    let value = label(app, hit);
    let mut current = [0u16; 512];
    let n = GetWindowTextW(child, &mut current) as usize;
    if String::from_utf16_lossy(&current[..n]) != value {
        let _ = SetWindowTextW(child, PCWSTR(wide(&value).as_ptr()));
    }
    if hit == Hit::Connect {
        use windows::Win32::UI::Input::KeyboardAndMouse::{EnableWindow, IsWindowEnabled};
        if IsWindowEnabled(child).as_bool() != app.connect_enabled {
            let _ = EnableWindow(child, app.connect_enabled);
        }
    }
}

pub(super) unsafe fn repaint_buttons(app: &App) {
    for (hit, _) in &app.buttons {
        repaint_button(app, *hit);
    }
}

/// Text width at the current device resolution, for centering icon+label groups.
unsafe fn text_width(dc: HDC, font: windows::Win32::Graphics::Gdi::HFONT, value: &str) -> i32 {
    let saved = SaveDC(dc);
    SelectObject(dc, winutil::font_as_gdi(font));
    let mut r = RECT::default();
    let mut value: Vec<u16> = value.encode_utf16().collect();
    let _ = DrawTextW(
        dc,
        &mut value,
        &mut r,
        DT_CALCRECT | DT_SINGLELINE | DT_NOPREFIX,
    );
    let _ = RestoreDC(dc, saved);
    r.right - r.left
}

pub(super) unsafe fn paint_button(app: &App, dc: HDC, hit: Hit, pressed: bool) {
    let Some(r) = bounds(app, hit) else { return };
    let p = winutil::Palette::for_dark(app.is_dark);
    let s = |v| app.s(v);
    let hot = app.hover == hit;
    let saved = SaveDC(dc);
    let _ = IntersectClipRect(dc, r.left, r.top, r.right, r.bottom);
    hero::line(dc, r, p.page);
    match hit {
        Hit::TabConnect | Hit::TabSettings => {
            // Shared track; redrawn here so partial button repaints stay whole.
            hero::surface(
                dc,
                hero::rect(app.dpi, 30, layout::TABS_TOP, 420, 40),
                app.dpi,
                p.toggle_off,
                20,
            );
            let selected = app.preferences == (hit == Hit::TabSettings);
            if selected {
                if !app.is_dark {
                    hero::box_shadow(dc, r, app.dpi, s(24), &hero::FIELD_SHADOW);
                }
                hero::surface(dc, r, app.dpi, p.segment, 24);
            }
            let (symbol, caption) = if hit == Hit::TabConnect {
                (Icon::PlugZap, "连接")
            } else {
                (Icon::SlidersHorizontal, "偏好设置")
            };
            let color = if selected {
                p.text_primary
            } else {
                p.text_secondary
            };
            let tw = text_width(dc, app.font_medium, caption);
            let icon_w = s(16);
            let gap = s(7);
            let total = icon_w + gap + tw;
            let mut x = r.left + (r.right - r.left - total) / 2;
            hero::icon(
                dc,
                RECT {
                    left: x,
                    top: r.top + (r.bottom - r.top - icon_w) / 2,
                    right: x + icon_w,
                    bottom: r.top + (r.bottom - r.top - icon_w) / 2 + icon_w,
                },
                color,
                symbol,
            );
            x += icon_w + gap;
            hero::text(
                dc,
                app.font_medium,
                color,
                RECT {
                    left: x,
                    top: r.top,
                    right: r.right,
                    bottom: r.bottom,
                },
                caption,
                DT_VCENTER | DT_SINGLELINE,
            );
        }
        Hit::Combo => {
            let mut field = r;
            field.bottom -= s(2);
            let state = if hot && !app.combo_open {
                hero::FieldState::Hover
            } else {
                hero::FieldState::Normal
            };
            hero::field(dc, field, app.dpi, p, state);
            hero::icon(
                dc,
                hero::rect(app.dpi, 42, layout::COMBO_TOP + 13, 18, 18),
                p.text_secondary,
                Icon::EthernetPort,
            );
            hero::text(
                dc,
                app.font,
                p.text_primary,
                hero::rect(app.dpi, 68, layout::COMBO_TOP, 342, 44),
                &combo_label(app),
                DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS,
            );
            paint_chevron(
                dc,
                s(428),
                s(layout::COMBO_TOP + 22),
                s(6),
                app.combo_visual.value(),
                p.text_secondary,
            );
        }
        Hit::Connect => {
            let disconnect = app.desired_running
                || matches!(
                    app.link_state,
                    LinkState::Online | LinkState::Connecting | LinkState::Waiting
                );
            let normal = if disconnect { p.danger_soft } else { p.accent };
            let hover = if disconnect {
                p.stroke_hover
            } else {
                p.accent_hover
            };
            let active = if disconnect {
                p.stroke_hover
            } else {
                p.accent_active
            };
            // Disabled follows the sketch's 50% opacity semantics.
            let fill = if !app.connect_enabled {
                hero::mix(p.page, normal, 50)
            } else if pressed {
                active
            } else if hot {
                hover
            } else {
                normal
            };
            hero::action_surface(dc, r, app.dpi, fill);
            let color = if !app.connect_enabled {
                hero::mix(fill, p.on_accent, 50)
            } else if disconnect {
                p.danger_text
            } else {
                p.on_accent
            };
            let caption = label(app, hit);
            let tw = text_width(dc, app.font_btn, &caption);
            let icon_w = s(18);
            let gap = s(8);
            let total = icon_w + gap + tw;
            let mut x = r.left + (r.right - r.left - total) / 2;
            hero::icon(
                dc,
                RECT {
                    left: x,
                    top: r.top + (r.bottom - r.top - icon_w) / 2,
                    right: x + icon_w,
                    bottom: r.top + (r.bottom - r.top - icon_w) / 2 + icon_w,
                },
                color,
                Icon::ArrowRight,
            );
            x += icon_w + gap;
            hero::text(
                dc,
                app.font_btn,
                color,
                RECT {
                    left: x,
                    top: r.top,
                    right: r.right,
                    bottom: r.bottom,
                },
                &caption,
                DT_VCENTER | DT_SINGLELINE,
            );
        }
        Hit::Remember if !app.preferences => {
            let on = app.toggle_anim[1].value();
            hero::checkbox(dc, 30, layout::REMEMBER_TOP + 9, app.dpi, p, on);
            hero::text(
                dc,
                app.font_chip,
                p.text_primary,
                hero::rect(app.dpi, 56, layout::REMEMBER_TOP, 124, 36),
                "记住密码",
                DT_VCENTER | DT_SINGLELINE,
            );
        }
        _ => {
            let (index, title, detail) = match hit {
                Hit::Auto => (0, "启动后自动连接", "打开客户端后自动认证校园网络"),
                Hit::Remember => (1, "记住密码", "下次连接时无需重新输入"),
                Hit::Startup => (2, "开机启动", "登录 Windows 时启动客户端"),
                _ => (3, "保留系统托盘", "关闭主窗口后继续在托盘运行"),
            };
            let y = layout::SETTINGS_TOP + index as i32 * layout::SETTINGS_ROW;
            hero::text(
                dc,
                app.font_medium,
                p.text_primary,
                hero::rect(app.dpi, 30, y + 7, 354, 22),
                title,
                DT_SINGLELINE | DT_END_ELLIPSIS,
            );
            hero::text(
                dc,
                app.font_chip,
                p.text_secondary,
                hero::rect(app.dpi, 30, y + 33, 354, 20),
                detail,
                DT_SINGLELINE | DT_END_ELLIPSIS,
            );
            hero::switch(dc, 404, y + 19, app.dpi, p, app.toggle_anim[index].value());
            if index < 3 {
                hero::line(dc, hero::rect(app.dpi, 30, y + 69, 420, 1), p.stroke);
            }
        }
    }
    // Only the username and password fields draw a blue focus outline.
    let _ = RestoreDC(dc, saved);
}

pub(super) unsafe fn paint_ui(hwnd: HWND, dc: HDC) {
    let Some(app) = app_mut(hwnd) else { return };
    let p = winutil::Palette::for_dark(app.is_dark);
    let r = |x, y, w, h| hero::rect(app.dpi, x, y, w, h);
    let s = |v: i32| app.s(v);
    hero::line(dc, r(0, 0, CLIENT_W, CLIENT_H), p.page);
    hero::line(dc, r(0, 47, CLIENT_W, 1), hero::mix(p.page, p.stroke, 55));
    hero::logo(dc, r(16, 14, 20, 20));
    hero::text(
        dc,
        app.font_brand,
        p.text_primary,
        r(44, 0, 300, 48),
        "drcom4scut",
        DT_SINGLELINE | DT_VCENTER,
    );
    paint_caption_btn(
        dc,
        app,
        app.s(CLIENT_W - 84),
        0,
        app.s(42),
        app.s(TITLE_H),
        app.hover == Hit::Min,
        false,
    );
    paint_caption_btn(
        dc,
        app,
        app.s(CLIENT_W - 42),
        0,
        app.s(42),
        app.s(TITLE_H),
        app.hover == Hit::Close,
        true,
    );
    let status = winutil::state_color(&p, app.link_state);
    let (status_bg, status_fg) = winutil::state_pair(&p, app.link_state);
    hero::surface(dc, r(30, 78, 44, 44), app.dpi, status_bg, 14);
    // The status chip keeps the original brand artwork.
    hero::logo(dc, r(35, 83, 34, 34));
    let chip = match app.link_state {
        LinkState::Online => "已连接",
        LinkState::Connecting => "连接中",
        LinkState::Waiting => "等待网络",
        LinkState::Degraded => "连接异常",
        LinkState::Error => "连接失败",
        LinkState::Offline => "离线",
    };
    hero::chip(
        dc,
        app.font_chip,
        s(450),
        s(87),
        app.dpi,
        status_bg,
        status_fg,
        Some(status),
        chip,
    );
    let title = if app.link_state == LinkState::Offline {
        "连接校园网络"
    } else {
        &app.status_title
    };
    hero::text(
        dc,
        app.font_title,
        p.text_primary,
        r(30, 140, 420, 40),
        title,
        DT_SINGLELINE | DT_END_ELLIPSIS,
    );
    hero::text(
        dc,
        app.font_label,
        p.text_secondary,
        r(30, 184, 420, 40),
        &app.status_detail,
        DT_WORDBREAK | DT_WORD_ELLIPSIS,
    );
    // Full segmented-control track (outer corners live outside the button
    // clips, so partial WM_DRAWITEM repaints cannot draw them).
    hero::surface(
        dc,
        r(30, layout::TABS_TOP, 420, 40),
        app.dpi,
        p.toggle_off,
        20,
    );
    if !app.preferences {
        for (top, name, icon) in [
            (layout::USER_TOP, "学号", Icon::User),
            (layout::PASS_TOP, "密码", Icon::Lock),
            (layout::COMBO_TOP, "网络适配器", Icon::EthernetPort),
        ] {
            hero::text(
                dc,
                app.font_medium,
                p.text_primary,
                r(30, top - 26, 420, 20),
                name,
                DT_SINGLELINE,
            );
            if top != layout::COMBO_TOP {
                let focus = GetFocus();
                let child = if top == layout::USER_TOP {
                    app.hwnd_user
                } else {
                    app.hwnd_pass
                };
                hero::field(
                    dc,
                    r(30, top, 420, 44),
                    app.dpi,
                    p,
                    if !child.0.is_null() && focus == child {
                        hero::FieldState::Focus
                    } else {
                        hero::FieldState::Normal
                    },
                );
                hero::icon(dc, r(42, top + 13, 18, 18), p.text_secondary, icon);
                if app.preview && child.0.is_null() {
                    hero::text(
                        dc,
                        app.font,
                        p.text_primary,
                        r(68, top, 322, 44),
                        if top == layout::USER_TOP {
                            "202600000001"
                        } else {
                            "••••••••"
                        },
                        DT_SINGLELINE | DT_VCENTER,
                    );
                }
            }
        }
        if app.hwnd_eye.0.is_null() {
            if let Some(eye) = &app.eye_on {
                winutil::blit_svg(
                    dc,
                    eye,
                    app.s(420),
                    app.s(layout::PASS_TOP + 12),
                    app.s(20),
                    app.s(20),
                );
            }
        }
    }
    if app.buttons.is_empty() {
        for hit in BUTTONS {
            paint_button(app, dc, hit, false);
        }
    }
    hero::line(dc, r(0, layout::FOOTER_TOP, CLIENT_W, 1), p.stroke);
    hero::icon(dc, r(30, 691, 13, 13), p.text_secondary, Icon::Monitor);
    hero::text(
        dc,
        app.font_footer,
        p.text_secondary,
        r(50, layout::FOOTER_TOP, 250, 48),
        "Windows 客户端",
        DT_SINGLELINE | DT_VCENTER,
    );
    hero::text(
        dc,
        app.font_footer,
        p.text_secondary,
        r(350, layout::FOOTER_TOP, 100, 48),
        concat!("v", env!("CARGO_PKG_VERSION")),
        DT_RIGHT | DT_SINGLELINE | DT_VCENTER,
    );
    // 1 DIP window edge in place of the DWM frame/shadow WS_POPUP cannot show.
    for edge in [
        r(0, 0, CLIENT_W, 1),
        r(0, CLIENT_H - 1, CLIENT_W, 1),
        r(0, 0, 1, CLIENT_H),
        r(CLIENT_W - 1, 0, 1, CLIENT_H),
    ] {
        hero::line(dc, edge, p.window_border);
    }
}
