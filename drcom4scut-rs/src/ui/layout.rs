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
