//! Open and close motion for the main window: a fade plus an 8px rise, the
//! house rule in the rice style reference. The window is made layered only
//! while it moves, so the glass is off during the fade and comes back once
//! the window has settled. A new open or close turns round from wherever the
//! last one got to.

use serde_json::Value;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::{Duration, Instant};
use windows::core::w;
use windows::Win32::Foundation::{COLORREF, HWND, RECT};
use windows::Win32::System::Registry::{
    RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RRF_RT_REG_SZ,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowLongPtrW, GetWindowRect, IsZoomed, SetLayeredWindowAttributes, SetWindowLongPtrW,
    SetWindowPos, SystemParametersInfoW, GWL_EXSTYLE, LWA_ALPHA, SPI_GETCLIENTAREAANIMATION,
    SWP_NOACTIVATE, SWP_NOSIZE, SWP_NOZORDER, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
    WS_EX_LAYERED, WS_EX_TRANSPARENT,
};

const OPEN_MS: f32 = 160.0;
const CLOSE_MS: f32 = 120.0;
const RISE: f32 = 8.0;

static GEN: AtomicU64 = AtomicU64::new(0);
/// How far shown the window is, 0 to 1000.
static LEVEL: AtomicU32 = AtomicU32::new(0);

/// How far shown the main window is, 0 to 1.
pub fn level() -> f32 {
    LEVEL.load(Ordering::SeqCst) as f32 / 1000.0
}

/// Scale for a speed level name: very quick 0.5x up to slow 2x.
pub fn level_scale(level: &str) -> Option<f32> {
    Some(match level {
        "veryquick" => 0.5,
        "quick" => 0.75,
        "normal" => 1.0,
        "relaxed" => 1.5,
        "slow" => 2.0,
        _ => return None,
    })
}

fn reg_dword(name: windows::core::PCWSTR) -> Option<u32> {
    let mut v: u32 = 0;
    let mut n = 4u32;
    let r = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\DavidHiFi\\Motion"),
            name,
            RRF_RT_REG_DWORD,
            None,
            Some(&mut v as *mut u32 as *mut _),
            Some(&mut n),
        )
    };
    r.is_ok().then_some(v)
}

fn reg_string(name: windows::core::PCWSTR) -> Option<String> {
    let mut buf = [0u16; 64];
    let mut n = (buf.len() * 2) as u32;
    let r = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\DavidHiFi\\Motion"),
            name,
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr() as *mut _),
            Some(&mut n),
        )
    };
    if r.is_err() {
        return None;
    }
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    Some(String::from_utf16_lossy(&buf[..len]).trim().to_lowercase())
}

/// The shared motion switch every one of the owner's apps follows. A missing
/// key means on at normal speed.
pub fn global() -> (bool, String) {
    let enabled = reg_dword(w!("Enabled")).map_or(true, |v| v != 0);
    let speed = match reg_string(w!("Speed")) {
        Some(s) if level_scale(&s).is_some() => s,
        _ => match reg_dword(w!("Speed")) {
            Some(p) if p <= 75 && p > 0 => if p <= 50 { "veryquick" } else { "quick" }.into(),
            Some(p) if p >= 200 => "slow".into(),
            Some(p) if p >= 150 => "relaxed".into(),
            _ => "normal".into(),
        },
    };
    (enabled, speed)
}

/// Windows "Animation effects".
pub fn system_on() -> bool {
    let mut on = windows::Win32::Foundation::BOOL(1);
    unsafe {
        let _ = SystemParametersInfoW(
            SPI_GETCLIENTAREAANIMATION,
            0,
            Some(&mut on as *mut _ as *mut _),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        );
    }
    on.as_bool()
}

/// What the page should use: animations on or off, and the speed level.
/// "follow" (or no choice) takes the shared speed.
pub fn resolve(motion: Option<&Value>) -> (bool, String) {
    let (g_on, g_speed) = global();
    let mine_on = motion.and_then(|m| m.get("enabled")).and_then(Value::as_bool).unwrap_or(true);
    let mine_speed = motion.and_then(|m| m.get("speed")).and_then(Value::as_str).unwrap_or("follow");
    let speed = if level_scale(mine_speed).is_some() { mine_speed.to_string() } else { g_speed };
    (g_on && mine_on && system_on(), speed)
}

/// The main window's own switch, under "Open and close animation".
pub fn window_scale(motion: Option<&Value>) -> Option<f32> {
    let (on, speed) = resolve(motion);
    let mine = motion
        .and_then(|m| m.get("surfaces"))
        .and_then(|s| s.get("windows"))
        .and_then(Value::as_bool)
        .unwrap_or(true);
    (on && mine).then(|| level_scale(&speed).unwrap_or(1.0))
}

fn set_layered(hwnd: HWND, on: bool, click_through: bool) {
    unsafe {
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
        let mut new = ex & !(WS_EX_LAYERED.0 | WS_EX_TRANSPARENT.0);
        if on {
            new |= WS_EX_LAYERED.0;
            if click_through {
                new |= WS_EX_TRANSPARENT.0;
            }
        }
        if new != ex {
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, new as isize);
        }
    }
}

fn alpha(hwnd: HWND, level: f32) {
    let a = (level.clamp(0.0, 1.0) * 255.0).round() as u8;
    unsafe {
        let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), a, LWA_ALPHA);
    }
}

fn top_left(hwnd: HWND) -> (i32, i32) {
    let mut r = RECT::default();
    unsafe {
        let _ = GetWindowRect(hwnd, &mut r);
    }
    (r.left, r.top)
}

fn move_to(hwnd: HWND, x: i32, y: i32) {
    unsafe {
        let _ = SetWindowPos(hwnd, None, x, y, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
    }
}

fn ease_out(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(3)
}

fn ease_in(t: f32) -> f32 {
    t * t * t
}

/// Where the window rests while it is not moving, kept so a turn-round
/// mid-move never leaves it a few pixels off.
static REST: std::sync::Mutex<Option<(i32, i32)>> = std::sync::Mutex::new(None);

/// Starts an open (`opening`) or close. `done(gen)` runs on the animation
/// thread when it finishes; pass it on to the event loop, and call
/// `finish(gen)` there. Returns the generation, or None when motion is off.
pub fn start(hwnd: isize, opening: bool, scale: Option<f32>, done: impl FnOnce(u64) + Send + 'static) -> Option<u64> {
    let hwnd_v = HWND(hwnd as *mut _);
    let scale = scale?;
    let gen = GEN.fetch_add(1, Ordering::SeqCst) + 1;
    let slide = unsafe { !IsZoomed(hwnd_v).as_bool() };
    {
        let mut rest = REST.lock().unwrap();
        if rest.is_none() {
            let (x, y) = top_left(hwnd_v);
            // An open starts from 8px low, so the resting place is where the window is now.
            *rest = Some((x, y));
        }
    }
    let from = LEVEL.load(Ordering::SeqCst) as f32 / 1000.0;
    let from = if opening && from >= 1.0 { 0.0 } else { from };
    set_layered(hwnd_v, true, !opening);
    alpha(hwnd_v, from);
    let total = if opening { OPEN_MS } else { CLOSE_MS } * scale;
    let to = if opening { 1.0f32 } else { 0.0 };
    // Time left is the share of the trip still to go.
    let span = ((to - from).abs() * total).max(1.0);
    std::thread::spawn(move || {
        let hwnd = HWND(hwnd as *mut _);
        let (rx, ry) = REST.lock().unwrap().unwrap_or_else(|| top_left(hwnd));
        let t0 = Instant::now();
        loop {
            if GEN.load(Ordering::SeqCst) != gen {
                return;
            }
            let t = (t0.elapsed().as_secs_f32() * 1000.0 / span).min(1.0);
            let k = if opening { ease_out(t) } else { ease_in(t) };
            let level = from + (to - from) * k;
            LEVEL.store((level * 1000.0) as u32, Ordering::SeqCst);
            alpha(hwnd, level);
            if slide {
                move_to(hwnd, rx, ry + ((1.0 - level) * RISE).round() as i32);
            }
            if t >= 1.0 {
                break;
            }
            std::thread::sleep(Duration::from_millis(8));
        }
        if slide {
            move_to(hwnd, rx, ry);
        }
        done(gen);
    });
    Some(gen)
}

/// Settles the window after the last move: the glass back on when shown.
/// Returns false when a newer move has taken over.
pub fn finish(hwnd: isize, gen: u64, opened: bool) -> bool {
    if GEN.load(Ordering::SeqCst) != gen {
        return false;
    }
    let hwnd = HWND(hwnd as *mut _);
    LEVEL.store(if opened { 1000 } else { 0 }, Ordering::SeqCst);
    if opened {
        set_layered(hwnd, false, false);
    }
    *REST.lock().unwrap() = None;
    true
}

/// A show or hide with no motion: drop any move in flight and settle.
pub fn cancel(hwnd: isize, shown: bool) {
    GEN.fetch_add(1, Ordering::SeqCst);
    let hwnd_v = HWND(hwnd as *mut _);
    if let Some((x, y)) = REST.lock().unwrap().take() {
        move_to(hwnd_v, x, y);
    }
    LEVEL.store(if shown { 1000 } else { 0 }, Ordering::SeqCst);
    set_layered(hwnd_v, false, false);
}

/// After a close has hidden the window: put it back to normal for next time.
pub fn after_hide(hwnd: isize) {
    set_layered(HWND(hwnd as *mut _), false, false);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_scale_every_duration() {
        assert_eq!(level_scale("veryquick"), Some(0.5));
        assert_eq!(level_scale("slow"), Some(2.0));
        assert_eq!(level_scale("follow"), None);
    }

    #[test]
    fn own_switch_turns_the_window_still() {
        let off = serde_json::json!({ "surfaces": { "windows": false } });
        assert_eq!(window_scale(Some(&off)), None);
        let all_off = serde_json::json!({ "enabled": false });
        assert_eq!(window_scale(Some(&all_off)), None);
    }
}
