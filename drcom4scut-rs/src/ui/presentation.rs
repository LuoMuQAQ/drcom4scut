//! Present the native window tree after its complete frame has been painted.
use super::*;
use windows::Win32::Graphics::Gdi::{
    GdiFlush, RedrawWindow, RDW_ALLCHILDREN, RDW_ERASE, RDW_FRAME, RDW_INVALIDATE, RDW_UPDATENOW,
};
use windows::Win32::UI::WindowsAndMessaging::{IsIconic, SHOW_WINDOW_CMD};

#[link(name = "dwmapi")]
extern "system" {
    fn DwmSetWindowAttribute(
        hwnd: HWND,
        attribute: u32,
        value: *const core::ffi::c_void,
        size: u32,
    ) -> i32;
    fn DwmFlush() -> i32;
}

struct PendingFrame {
    hwnd: HWND,
    cloaked: bool,
}

impl PendingFrame {
    unsafe fn new(hwnd: HWND) -> Self {
        // DWMWA_CLOAK keeps a visible HWND composed but off the desktop. Unlike
        // SW_HIDE / WM_SETREDRAW, it still allows every native child to paint.
        let cloak: i32 = 1;
        Self {
            hwnd,
            cloaked: DwmSetWindowAttribute(hwnd, 13, (&cloak as *const i32).cast(), 4) == 0,
        }
    }
}

impl Drop for PendingFrame {
    fn drop(&mut self) {
        if self.cloaked {
            unsafe {
                let cloak: i32 = 0;
                let _ = DwmSetWindowAttribute(self.hwnd, 13, (&cloak as *const i32).cast(), 4);
            }
        }
    }
}

pub(super) unsafe fn show(hwnd: HWND, command: SHOW_WINDOW_CMD) {
    let reveal = !IsWindowVisible(hwnd).as_bool() || IsIconic(hwnd).as_bool();
    let pending = reveal.then(|| PendingFrame::new(hwnd));
    let _ = ShowWindow(hwnd, command);
    if reveal {
        // Restore can change DPI/work area. Finish layout before the first paint
        // instead of exposing the later posted WM_APP_LAYOUT update.
        display::refresh(hwnd);
        let _ = RedrawWindow(
            Some(hwnd),
            None,
            None,
            RDW_INVALIDATE | RDW_ERASE | RDW_FRAME | RDW_ALLCHILDREN | RDW_UPDATENOW,
        );
        let _ = GdiFlush();
        if pending.as_ref().is_some_and(|frame| frame.cloaked) {
            let _ = DwmFlush();
        }
        // Without DWM cloaking, the synchronous full-tree paint is still useful.
        // PendingFrame always releases our cloak, including error/fallback paths.
    }
}
