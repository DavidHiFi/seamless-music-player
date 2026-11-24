//! Native pop-up host. Menus, pick lists, pop-overs and dialogs open in one
//! warm, hidden, owned window with real Windows acrylic, never in-page CSS
//! blur (owner rule, 2026-11-24). The window loads the same UI in pop-up mode
//! (`index.html?popup=1`), draws only the one surface on a transparent page,
//! and talks to the main page through Rust:
//!
//! main page  -> `popup.open {token, kind, ..., anchor:{x,y,w,h}, placement?}`
//! Rust       -> popup page `window.__smpPopup.render(payload)`
//! popup page -> `popup.size {w,h}`  (Rust places and shows the window)
//! popup page -> `popup.pick {token, id, values?}` or `popup.close {token}`
//! Rust       -> main page event `popup.picked {token,id,values}` / `popup.closed {token}`

use serde_json::Value;
use std::ffi::c_void;
use windows::core::{w, PCSTR};
use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::Graphics::Dwm::{
    DwmSetWindowAttribute, DWMWA_USE_IMMERSIVE_DARK_MODE, DWMWA_WINDOW_CORNER_PREFERENCE,
    DWMWCP_ROUND,
};
use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};

#[repr(C)]
struct AccentPolicy {
    state: u32,
    flags: u32,
    gradient: u32,
    anim: u32,
}

#[repr(C)]
struct WinCompAttrData {
    attr: u32,
    data: *mut c_void,
    size: usize,
}

/// Mocha base tint, 85% (the desktop and Steam menus' value). AABBGGRR.
pub const TINT: u32 = 0xD82E1E1E;

/// Real acrylic through the accent policy (ACCENT_ENABLE_ACRYLICBLURBEHIND),
/// rounded corners and a dark frame. Never the system backdrop attribute: a
/// window that is never active gets plain grey from it.
pub fn apply_acrylic(hwnd: isize, tint: u32) {
    let h = HWND(hwnd as *mut c_void);
    unsafe {
        let dark: i32 = 1;
        let _ = DwmSetWindowAttribute(h, DWMWA_USE_IMMERSIVE_DARK_MODE, &dark as *const i32 as *const c_void, 4);
        let round: i32 = DWMWCP_ROUND.0;
        let _ = DwmSetWindowAttribute(h, DWMWA_WINDOW_CORNER_PREFERENCE, &round as *const i32 as *const c_void, 4);
        let Ok(lib) = LoadLibraryW(w!("user32.dll")) else { return };
        let Some(f) = GetProcAddress(lib, PCSTR(b"SetWindowCompositionAttribute\0".as_ptr())) else { return };
        let set: extern "system" fn(HWND, *mut WinCompAttrData) -> i32 = std::mem::transmute(f);
        let mut policy = AccentPolicy { state: 4, flags: 2, gradient: tint, anim: 0 };
        let mut data = WinCompAttrData {
            attr: 19, // WCA_ACCENT_POLICY
            data: &mut policy as *mut AccentPolicy as *mut c_void,
            size: std::mem::size_of::<AccentPolicy>(),
        };
        set(h, &mut data);
    }
}

/// Toasts never take focus from the owner's game or the main window.
pub fn make_no_activate(hwnd: isize) {
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, SetWindowLongPtrW, GWL_EXSTYLE, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    };
    let h = HWND(hwnd as *mut c_void);
    unsafe {
        let ex = GetWindowLongPtrW(h, GWL_EXSTYLE);
        SetWindowLongPtrW(h, GWL_EXSTYLE, ex | (WS_EX_NOACTIVATE.0 | WS_EX_TOOLWINDOW.0) as isize);
    }
}

/// Move, size and show a window without activating it.
pub fn show_at_no_activate(hwnd: isize, x: i32, y: i32, w: i32, h: i32) {
    use windows::Win32::UI::WindowsAndMessaging::{
        SetWindowPos, HWND_TOP, SWP_NOACTIVATE, SWP_SHOWWINDOW,
    };
    let hw = HWND(hwnd as *mut c_void);
    unsafe {
        let _ = SetWindowPos(hw, HWND_TOP, x, y, w, h, SWP_NOACTIVATE | SWP_SHOWWINDOW);
    }
}

/// Hide a window without touching focus.
pub fn hide_window(hwnd: isize) {
    use windows::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_HIDE};
    unsafe {
        let _ = ShowWindow(HWND(hwnd as *mut c_void), SW_HIDE);
    }
}

/// The work area (screen minus taskbar) of the monitor under a point.
pub fn work_area(x: i32, y: i32) -> (i32, i32, i32, i32) {
    unsafe {
        let mon = MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONEAREST);
        let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        if GetMonitorInfoW(mon, &mut mi).as_bool() {
            let RECT { left, top, right, bottom } = mi.rcWork;
            return (left, top, right, bottom);
        }
    }
    (0, 0, 1920, 1080)
}

/// Where the pop-up goes, in screen pixels. `anchor` is the element box in
/// screen pixels, `size` the pop-up size. Below the anchor by default, above
/// when there is no room; "right" opens beside it (sub-menus); "center" sits
/// in the middle of the anchor (dialogs over the main window); "point" opens
/// at the anchor's top-left (context menus at the cursor).
pub fn place(anchor: (i32, i32, i32, i32), size: (i32, i32), placement: &str, gap: i32) -> (i32, i32) {
    let (ax, ay, aw, ah) = anchor;
    let (w, h) = size;
    let (l, t, r, b) = work_area(ax + aw / 2, ay + ah / 2);
    let (mut x, mut y) = match placement {
        "center" => (ax + (aw - w) / 2, ay + (ah - h) / 2),
        "point" => (ax, ay),
        "right" => (ax + aw + gap, ay),
        "above" => (ax, ay - h - gap),
        "end" => (ax + aw - w, ay + ah + gap),
        _ => (ax, ay + ah + gap),
    };
    match placement {
        "right" if x + w > r => x = ax - w - gap,
        "point" if y + h > b => y = ay - h,
        "point" | "center" | "right" => {}
        "above" if y < t => y = ay + ah + gap,
        _ if y + h > b => y = ay - h - gap,
        _ => {}
    }
    if placement == "point" && x + w > r {
        x = ax - w;
    }
    x = x.clamp(l, (r - w).max(l));
    y = y.clamp(t, (b - h).max(t));
    (x, y)
}

/// The pending pop-up: what the main page asked for, waiting on the pop-up
/// page to measure itself.
#[derive(Default)]
pub struct Pending {
    pub token: String,
    pub anchor: (i32, i32, i32, i32),
    pub placement: String,
    pub open: bool,
}

/// Turn the page's anchor (CSS pixels, relative to the main page) into
/// screen pixels.
pub fn anchor_to_screen(args: &Value, origin: (i32, i32), scale: f64) -> (i32, i32, i32, i32) {
    let a = &args["anchor"];
    let f = |k: &str| a[k].as_f64().unwrap_or(0.0);
    (
        origin.0 + (f("x") * scale).round() as i32,
        origin.1 + (f("y") * scale).round() as i32,
        (f("w") * scale).round() as i32,
        (f("h") * scale).round() as i32,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stays_inside_the_work_area() {
        let (l, t, r, b) = work_area(100, 100);
        let (x, y) = place((r - 10, b - 10, 5, 5), (300, 200), "below", 4);
        assert!(x >= l && x + 300 <= r);
        assert!(y >= t && y + 200 <= b);
    }

    #[test]
    fn flips_above_near_the_bottom() {
        let (_, _, _, b) = work_area(100, 100);
        let (_, y) = place((100, b - 40, 80, 30), (200, 200), "below", 4);
        assert_eq!(y, b - 40 - 200 - 4);
    }
}
