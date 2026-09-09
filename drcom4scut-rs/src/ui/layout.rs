//! Shared logical geometry (96 DPI) for painting, child windows and hit testing.

pub const WIDTH: i32 = 420;
pub const TITLE_HEIGHT: i32 = 40;
pub const OUTER_PADDING: i32 = 20;
pub const SECTION_GAP: i32 = 12;
pub const CARD_PADDING: i32 = 16;
pub const STATUS_HEIGHT: i32 = 76 + CARD_PADDING * 2;
pub const STATUS_TOP: i32 = TITLE_HEIGHT + OUTER_PADDING;
pub const STATUS_BOTTOM: i32 = STATUS_TOP + STATUS_HEIGHT;
pub const ACCOUNT_TOP: i32 = STATUS_BOTTOM + SECTION_GAP;
pub const ACCOUNT_BOTTOM: i32 = ACCOUNT_TOP + 310;
pub const FIELD_HEIGHT: i32 = 36;
pub const USER_TOP: i32 = ACCOUNT_TOP + 62;
pub const PASS_TOP: i32 = USER_TOP + 64;
pub const COMBO_TOP: i32 = PASS_TOP + 64;
pub const TOGGLE_TOP: i32 = COMBO_TOP + FIELD_HEIGHT + 14;
pub const TOGGLE_SECOND_TOP: i32 = TOGGLE_TOP + 34;
pub const ACTION_TOP: i32 = ACCOUNT_BOTTOM + SECTION_GAP;
pub const ACTION_BOTTOM: i32 = ACTION_TOP + FIELD_HEIGHT;
pub const HEIGHT: i32 = ACTION_BOTTOM + OUTER_PADDING;

pub const CARD_LEFT: i32 = OUTER_PADDING;
pub const CARD_RIGHT: i32 = WIDTH - OUTER_PADDING;
pub const FIELD_LEFT: i32 = CARD_LEFT + CARD_PADDING;
pub const FIELD_RIGHT: i32 = CARD_RIGHT - CARD_PADDING;

use super::winutil::scale;
use windows::Win32::Foundation::RECT;

/// One effective scale for the window, drawing, native controls and hit tests.
/// A smaller work area limits the whole layout, never just its outer HWND.
pub fn display_layout(display_dpi: u32, anchor: RECT, work: RECT) -> (u32, RECT) {
    let available_w = (work.right - work.left).max(1);
    let available_h = (work.bottom - work.top).max(1);
    let dpi = display_dpi
        .max(96)
        .min((available_w as u64 * 96 / WIDTH as u64).max(1) as u32)
        .min((available_h as u64 * 96 / HEIGHT as u64).max(1) as u32);
    let w = scale(WIDTH, dpi).max(1);
    let h = scale(HEIGHT, dpi).max(1);
    let x = anchor
        .left
        .clamp(work.left, (work.right - w).max(work.left));
    let y = anchor.top.clamp(work.top, (work.bottom - h).max(work.top));
    (
        dpi,
        RECT {
            left: x,
            top: y,
            right: x + w,
            bottom: y + h,
        },
    )
}

pub struct Controls {
    pub user: RECT,
    pub pass: RECT,
    pub eye: RECT,
    pub eye_hit: RECT,
}

pub fn controls(dpi: u32) -> Controls {
    let s = |v| scale(v, dpi);
    let field = |top, inset| RECT {
        left: s(FIELD_LEFT) + s(10),
        top: s(top) + 1,
        right: s(FIELD_RIGHT) - s(inset),
        bottom: s(top + FIELD_HEIGHT) - 1,
    };
    let icon = s(20);
    Controls {
        user: field(USER_TOP, 10),
        pass: field(PASS_TOP, 40),
        eye: RECT {
            left: s(348),
            top: s(PASS_TOP + 4),
            right: s(348) + s(30),
            bottom: s(PASS_TOP + 4) + s(FIELD_HEIGHT - 8),
        },
        eye_hit: RECT {
            left: s(FIELD_RIGHT) - s(10) - icon,
            top: s(PASS_TOP) + (s(FIELD_HEIGHT) - icon) / 2,
            right: s(FIELD_RIGHT) - s(10),
            bottom: s(PASS_TOP) + (s(FIELD_HEIGHT) - icon) / 2 + icon,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dpi_changes_use_logical_size_not_the_previously_scaled_rectangle() {
        let work = RECT {
            left: -3840,
            top: 0,
            right: 0,
            bottom: 2160,
        };
        let mut anchor = RECT {
            left: -1600,
            top: 100,
            right: -1180,
            bottom: 658,
        };
        for requested in [96, 144, 192, 120, 96, 192, 96] {
            let (dpi, rect) = display_layout(requested, anchor, work);
            assert_eq!(dpi, requested);
            assert_eq!(rect.right - rect.left, scale(WIDTH, dpi));
            assert_eq!(rect.bottom - rect.top, scale(HEIGHT, dpi));
            assert!(rect.left >= work.left && rect.right <= work.right);
            anchor = rect;
        }
    }

    #[test]
    fn resolution_and_taskbar_changes_fit_and_then_restore_the_whole_layout() {
        let anchor = RECT {
            left: 1800,
            top: 900,
            right: 2640,
            bottom: 2016,
        };
        for work in [
            RECT {
                left: 0,
                top: 0,
                right: 1280,
                bottom: 680,
            },
            RECT {
                left: 40,
                top: 0,
                right: 800,
                bottom: 480,
            },
        ] {
            let (dpi, rect) = display_layout(192, anchor, work);
            assert!(dpi < 192);
            assert!(rect.left >= work.left && rect.top >= work.top);
            assert!(rect.right <= work.right && rect.bottom <= work.bottom);
            assert!(controls(dpi).pass.right < rect.right - rect.left);
            assert!(scale(ACTION_BOTTOM, dpi) < rect.bottom - rect.top);
            let (restored, _) = display_layout(
                192,
                rect,
                RECT {
                    left: 0,
                    top: 0,
                    right: 3840,
                    bottom: 2160,
                },
            );
            assert_eq!(restored, 192);
        }
    }
}
