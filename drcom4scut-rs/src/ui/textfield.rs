//! Single-line native editing with an explicitly laid out non-client text viewport.
//! The EDIT retains selection, undo, IME, scrolling and password protection.

use super::winutil;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::*;

const EM_GETPASSWORDCHAR: u32 = 0x00D2;
const EM_SETPASSWORDCHAR: u32 = 0x00CC;

pub unsafe fn attach(hwnd: HWND) {
    let _ = SetWindowSubclass(hwnd, Some(field_proc), 1, 0);
    relayout(hwnd);
}

unsafe fn relayout(hwnd: HWND) {
    let _ = SetWindowPos(
        hwnd,
        None,
        0,
        0,
        0,
        0,
        SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
    );
    let _ = RedrawWindow(
        Some(hwnd),
        None,
        None,
        RDW_INVALIDATE | RDW_FRAME | RDW_UPDATENOW,
    );
}

fn line_top(height: i32, ascent: i32, ink_origin: i32, ink_height: i32) -> i32 {
    (height - ink_height) / 2 - (ascent - ink_origin)
}

unsafe extern "system" fn field_proc(
    hwnd: HWND,
    msg: u32,
    wp: WPARAM,
    lp: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    match msg {
        WM_LBUTTONDOWN => {
            if let Ok(parent) = GetParent(hwnd) {
                let _ = PostMessageW(
                    Some(parent),
                    super::window::WM_APP_FIELD_CLICK,
                    WPARAM(0),
                    LPARAM(0),
                );
            }
        }
        WM_NCCALCSIZE => {
            // Both NCCALCSIZE payloads start with the proposed outer RECT.
            let rc = &mut *(lp.0 as *mut RECT);
            let dc = GetDC(None);
            let font = SendMessageW(hwnd, WM_GETFONT, None, None);
            let old = SelectObject(dc, HGDIOBJ(font.0 as *mut _));
            let mut tm = TEXTMETRICW::default();
            let _ = GetTextMetricsW(dc, &mut tm);
            let mask = SendMessageW(hwnd, EM_GETPASSWORDCHAR, None, None).0 as u32;
            let mut gm = GLYPHMETRICS::default();
            let transform = MAT2 {
                eM11: FIXED { fract: 0, value: 1 },
                eM22: FIXED { fract: 0, value: 1 },
                ..Default::default()
            };
            let measured = GetGlyphOutlineW(
                dc,
                if mask == 0 { '0' as u32 } else { mask },
                GGO_METRICS,
                &mut gm,
                0,
                None,
                &transform,
            );
            let height = rc.bottom - rc.top;
            let top = if measured != u32::MAX && gm.gmBlackBoxY > 0 {
                line_top(
                    height,
                    tm.tmAscent,
                    gm.gmptGlyphOrigin.y,
                    gm.gmBlackBoxY as i32,
                )
            } else {
                (height - tm.tmHeight) / 2
            };
            let top = top.clamp(0, (height - tm.tmHeight).max(0));
            rc.top += top;
            // Keep the remaining descent area: EDIT's own baseline padding must
            // not clip underscores, IME underlines or selection at the bottom.
            let _ = SelectObject(dc, old);
            let _ = ReleaseDC(None, dc);
            return LRESULT(0);
        }
        WM_NCPAINT => {
            let dc = GetWindowDC(Some(hwnd));
            let mut outer = RECT::default();
            let mut client = RECT::default();
            let _ = GetWindowRect(hwnd, &mut outer);
            let _ = GetClientRect(hwnd, &mut client);
            let mut origin = POINT::default();
            let _ = ClientToScreen(hwnd, &mut origin);
            let x = origin.x - outer.left;
            let y = origin.y - outer.top;
            let _ = ExcludeClipRect(dc, x, y, x + client.right, y + client.bottom);
            let brush = winutil::solid_brush(winutil::COLOR_CONTROL);
            let _ = FillRect(
                dc,
                &RECT {
                    left: 0,
                    top: 0,
                    right: outer.right - outer.left,
                    bottom: outer.bottom - outer.top,
                },
                brush,
            );
            winutil::delete_gdi(winutil::brush_as_gdi(brush));
            let _ = ReleaseDC(Some(hwnd), dc);
            return LRESULT(0);
        }
        WM_NCHITTEST => return LRESULT(HTCLIENT as isize),
        WM_NCDESTROY => {
            let _ = windows::Win32::UI::Shell::RemoveWindowSubclass(hwnd, Some(field_proc), 1);
        }
        WM_SETFONT | EM_SETPASSWORDCHAR => {
            let result = DefSubclassProc(hwnd, msg, wp, lp);
            relayout(hwnd);
            return result;
        }
        WM_KEYDOWN if wp.0 == 13 || wp.0 == 27 || wp.0 == 9 => {
            if let Ok(parent) = GetParent(hwnd) {
                if wp.0 == 13 {
                    let _ = PostMessageW(
                        Some(parent),
                        super::window::WM_APP_CONNECT,
                        WPARAM(0),
                        LPARAM(0),
                    );
                } else if wp.0 == 27 {
                    let _ = PostMessageW(Some(parent), WM_KEYDOWN, wp, lp);
                } else {
                    let shift = windows::Win32::UI::Input::KeyboardAndMouse::GetKeyState(16) < 0;
                    if let Ok(next) = GetNextDlgTabItem(parent, Some(hwnd), shift) {
                        let _ = windows::Win32::UI::Input::KeyboardAndMouse::SetFocus(Some(next));
                    }
                }
            }
            return LRESULT(0);
        }
        WM_CHAR if matches!(wp.0, 9 | 10 | 13 | 27) => return LRESULT(0),
        _ => {}
    }
    DefSubclassProc(hwnd, msg, wp, lp)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn glyph_ink_is_centered_for_digits_and_password_masks() {
        for (height, ascent, origin, ink) in [
            (36, 15, 11, 11),
            (36, 15, 9, 6),
            (54, 23, 15, 9),
            (72, 30, 22, 22),
        ] {
            let top = line_top(height, ascent, origin, ink);
            let ink_top = top + ascent - origin;
            assert!((ink_top - (height - ink_top - ink)).abs() <= 1);
        }
    }

    #[test]
    fn native_edit_preserves_text_selection_and_undo_across_reveal_at_multiple_dpis() {
        use windows::core::{w, PCWSTR};
        unsafe {
            for dpi in [96, 120, 144, 192] {
                let parent = CreateWindowExW(
                    WINDOW_EX_STYLE(0),
                    w!("STATIC"),
                    w!(""),
                    WS_POPUP,
                    0,
                    0,
                    800,
                    300,
                    None,
                    None,
                    None,
                    None,
                )
                .unwrap();
                let edit = CreateWindowExW(
                    WINDOW_EX_STYLE(0),
                    w!("EDIT"),
                    w!(""),
                    WINDOW_STYLE(WS_CHILD.0 | 0x80 | 0x20),
                    10,
                    10,
                    300,
                    winutil::scale(36, dpi) - 2,
                    Some(parent),
                    None,
                    None,
                    None,
                )
                .unwrap();
                let font = winutil::create_font(14, dpi, false);
                let _ = SendMessageW(edit, WM_SETFONT, Some(WPARAM(font.0 as usize)), None);
                attach(edit);
                let text = winutil::wide("_Ag09_中文");
                SetWindowTextW(edit, PCWSTR(text.as_ptr())).unwrap();
                let _ = SendMessageW(edit, 0x00B1, Some(WPARAM(1)), Some(LPARAM(4))); // EM_SETSEL
                for mask in [0x2022, 0, 0x2022, 0] {
                    let _ = SendMessageW(edit, EM_SETPASSWORDCHAR, Some(WPARAM(mask)), None);
                    assert_eq!(SendMessageW(edit, 0x00B0, None, None).0, 1 | (4 << 16));
                    let mut actual = [0u16; 64];
                    let count = GetWindowTextW(edit, &mut actual) as usize;
                    assert_eq!(&actual[..count], &text[..text.len() - 1]);
                }
                let _ = SendMessageW(edit, WM_CHAR, Some(WPARAM('X' as usize)), None);
                let _ = SendMessageW(edit, EM_SETPASSWORDCHAR, Some(WPARAM(0x2022)), None);
                assert_ne!(SendMessageW(edit, 0x00C6, None, None).0, 0); // EM_CANUNDO
                let _ = SendMessageW(edit, 0x00C7, None, None); // EM_UNDO
                let mut actual = [0u16; 64];
                let count = GetWindowTextW(edit, &mut actual) as usize;
                assert_eq!(&actual[..count], &text[..text.len() - 1]);
                // Exercise the actual native paint path, including the glyph
                // below the baseline that a too-short EDIT viewport clips.
                let _ = SendMessageW(edit, EM_SETPASSWORDCHAR, Some(WPARAM(0)), None);
                SetWindowTextW(edit, w!("_")).unwrap();
                let bmp = winutil::rasterize_svg(br##"<svg xmlns="http://www.w3.org/2000/svg" width="300" height="100"><rect width="300" height="100" fill="#fff"/></svg>"##, 300).unwrap();
                let dc = CreateCompatibleDC(None);
                let previous = SelectObject(dc, HGDIOBJ(bmp.hbmp.0));
                let _ = SendMessageW(
                    edit,
                    WM_PRINTCLIENT,
                    Some(WPARAM(dc.0 as usize)),
                    Some(LPARAM(PRF_CLIENT as isize)),
                );
                let _ = GdiFlush();
                let pixels = std::slice::from_raw_parts(bmp.bits, 300 * 100 * 4);
                assert!(
                    pixels
                        .chunks_exact(4)
                        .any(|p| p[0] < 100 && p[1] < 100 && p[2] < 100),
                    "underscore must survive native EDIT painting at {dpi} DPI"
                );
                SetWindowTextW(edit, w!("0")).unwrap();
                for mask in [0, 0x2022] {
                    let _ = SendMessageW(edit, EM_SETPASSWORDCHAR, Some(WPARAM(mask)), None);
                    std::slice::from_raw_parts_mut(bmp.bits, 300 * 100 * 4).fill(255);
                    let _ = SendMessageW(
                        edit,
                        WM_PRINTCLIENT,
                        Some(WPARAM(dc.0 as usize)),
                        Some(LPARAM(PRF_CLIENT as isize)),
                    );
                    let _ = GdiFlush();
                    let pixels = std::slice::from_raw_parts(bmp.bits, 300 * 100 * 4);
                    let ys: Vec<i32> = pixels
                        .chunks_exact(4)
                        .enumerate()
                        .filter(|(_, p)| p[0] < 100 && p[1] < 100 && p[2] < 100)
                        .map(|(i, _)| (i / 300) as i32)
                        .collect();
                    let mut outer = RECT::default();
                    GetWindowRect(edit, &mut outer).unwrap();
                    let mut client_origin = POINT::default();
                    let _ = ClientToScreen(edit, &mut client_origin);
                    let offset = client_origin.y - outer.top;
                    let center2 = ys.iter().min().unwrap() + ys.iter().max().unwrap() + 2 * offset;
                    assert!((center2 - (outer.bottom - outer.top - 1)).abs() <= 2,
                        "native rendered ink must be centered at {dpi} DPI (mask {mask}, center2 {center2}, height {})", outer.bottom - outer.top);
                }
                let _ = SelectObject(dc, previous);
                let _ = DeleteDC(dc);
                DestroyWindow(parent).unwrap();
                winutil::delete_gdi(winutil::font_as_gdi(font));
            }
        }
    }
}
