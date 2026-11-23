//! Seamless Music Player: the window shell. One tao window with the page in
//! a wry webview, a tray icon with a native menu, the smp:// protocol that
//! serves the embedded UI and artwork, the IPC dispatch that carries commands
//! to the backend modules, and the one-time Dopamine import.

#![windows_subsystem = "windows"]

mod discord;
mod library;
mod logic;
mod lyrics;
mod migrate;
mod online;
mod player;
mod scrobble;
mod selftest;
mod settings;
mod smtc;

use library::Library;
use online::Online;
use player::Player;
use scrobble::Scrobbler;
use serde_json::{json, Value};
use settings::{app_dir, SettingsFile};
use smtc::Smtc;
use std::ffi::c_void;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tao::dpi::{LogicalSize, PhysicalPosition, PhysicalSize};
use tao::event::{Event, StartCause, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tao::platform::windows::WindowExtWindows;
use tao::window::{Icon, Window, WindowBuilder};
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use windows::core::{w, PCSTR};
use windows::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS, HWND, WAIT_OBJECT_0};
use windows::Win32::Graphics::Dwm::{
    DwmExtendFrameIntoClientArea, DwmSetWindowAttribute, DWMWA_CAPTION_COLOR,
    DWMWA_SYSTEMBACKDROP_TYPE, DWMWA_USE_IMMERSIVE_DARK_MODE, DWMWA_WINDOW_CORNER_PREFERENCE,
    DWM_SYSTEMBACKDROP_TYPE, DWMWCP_ROUND,
};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows::Win32::System::ProcessStatus::{
    GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS_EX,
};
use windows::Win32::System::Threading::{
    CreateEventW, CreateMutexW, GetCurrentProcess, OpenEventW, SetEventW, WaitForSingleObject,
    EVENT_MODIFY_STATE,
};
use windows::Win32::UI::Controls::MARGINS;
use wry::http::{Request, Response};
use wry::{WebView, WebViewBuilder, WebViewBuilderExtWindows};

/// Every UI write goes through here as a prepared script, so worker threads
/// never touch the webview directly.
pub enum UserEvent {
    Ipc(String),
    Emit(String),
    Tray(TrayIconEvent),
    Menu(MenuEvent),
    Loaded,
    Show,
    Quit,
}

pub struct Events {
    proxy: Mutex<Option<tao::event_loop::EventLoopProxy<UserEvent>>>,
}

impl Events {
    pub fn new() -> Arc<Events> {
        Arc::new(Events { proxy: Mutex::new(None) })
    }

    pub fn attach(&self, proxy: tao::event_loop::EventLoopProxy<UserEvent>) {
        *self.proxy.lock().unwrap() = Some(proxy);
    }

    fn send(&self, ev: UserEvent) {
        if let Ok(p) = self.proxy.lock() {
            if let Some(p) = p.as_ref() {
                let _ = p.send_event(ev);
            }
        }
    }

    pub fn eval(&self, script: String) {
        self.send(UserEvent::Emit(script));
    }

    /// Push one event to the page: window.__smp.event(name, payload).
    pub fn emit(&self, name: &str, payload: Value) {
        let n = serde_json::to_string(name).unwrap_or_else(|_| "\"\"".into());
        let p = payload.to_string();
        self.eval(format!("try{{window.__smp.event({n},{p})}}catch(e){{}}"));
    }
}

pub struct App {
    pub settings: Arc<SettingsFile>,
    pub events: Arc<Events>,
    pub lib: Arc<Library>,
    pub player: Arc<Player>,
    pub smtc: Mutex<Smtc>,
    pub discord: Mutex<discord::Discord>,
    pub scrobbler: Scrobbler,
    pub online: Arc<Online>,
    pub watcher: std::sync::mpsc::Sender<library::WatcherCmd>,
}

/// The UI folder, embedded at build time. The UI helper owns its files; when
/// index.html is not there yet the protocol serves a one-line placeholder.
static UI_DIR: include_dir::Dir = include_dir!("$CARGO_MANIFEST_DIR/ui");

const UI_PLACEHOLDER: &str =
    "<!doctype html><html><head><meta charset=\"utf-8\"><title>Seamless Music Player</title></head><body style=\"background:#1E1E2E;color:#cdd6f4;font-family:monospace;padding:2em\">Seamless Music Player: the interface is not built yet.</body></html>";

fn mime_for(path: &str) -> &'static str {
    let ext = path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "html" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

/// smp://app/<path> serves the embedded ui folder; smp://art/<id> serves the
/// artwork cache from disk.
fn serve_uri(path: &str) -> Response<std::borrow::Cow<'static, [u8]>> {
    use std::borrow::Cow;
    let not_found = || {
        Response::builder()
            .status(404)
            .body(Cow::Borrowed(&b""[..]))
            .unwrap()
    };
    let (path, is_art) = match path.strip_prefix("/art/") {
        Some(id) => (id.trim_start_matches('/'), true),
        None => (path.strip_prefix("/app/").unwrap_or(path), false),
    };
    if is_art {
        // Artwork ids are content hashes; anything but a plain id is refused.
        if path.is_empty() || path.contains("..") || path.contains('\\') || path.contains('/') {
            return not_found();
        }
        let file = library::artwork_path(path.split('.').next().unwrap_or(path));
        match std::fs::read(&file) {
            Ok(b) => Response::builder()
                .status(200)
                .header("Content-Type", "image/jpeg")
                .header("Cache-Control", "max-age=31536000, immutable")
                .body(Cow::Owned(b))
                .unwrap(),
            Err(_) => not_found(),
        }
    } else {
        let clean = path.trim_start_matches('/');
        let target = if clean.is_empty() { "index.html" } else { clean };
        let body: Cow<'static, [u8]> = match UI_DIR.get_file(target) {
            Some(f) => Cow::Borrowed(f.contents()),
            None if target == "index.html" => Cow::Borrowed(UI_PLACEHOLDER.as_bytes()),
            None => return not_found(),
        };
        let mime = mime_for(target);
        Response::builder()
            .status(200)
            .header("Content-Type", mime)
            .header("Cache-Control", "no-cache")
            .body(body)
            .unwrap()
    }
}

fn reply(events: &Arc<Events>, id: u64, ok: bool, data: Value) {
    let d = match &data {
        Value::String(s) => serde_json::to_string(s).unwrap_or_default(),
        other => other.to_string(),
    };
    events.eval(format!("try{{window.__smp.reply({id},{ok},{d})}}catch(e){{}}"));
}

/// One IPC command. Returns None when the reply is sent later by a worker
/// thread; otherwise the result is sent right away.
fn dispatch(app: &App, id: u64, cmd: &str, args: &Value) -> Option<Result<Value, String>> {
    let arg = |k: &str| args.get(k).cloned().unwrap_or(Value::Null);
    match cmd {
        // ---- library ----
        "library.overview" => Some(Ok(app.lib.overview())),
        "library.tracks" => {
            let sort = arg("sort").as_str().unwrap_or("title").to_string();
            let filter = arg("filter").as_str().map(String::from);
            let offset = arg("offset").as_i64().unwrap_or(0);
            let limit = arg("limit").as_i64().unwrap_or(200);
            Some(Ok(app.lib.tracks(&sort, filter.as_deref(), offset, limit)))
        }
        "library.albums" => {
            let sort = arg("sort").as_str().unwrap_or("artist").to_string();
            Some(Ok(app.lib.albums(&sort, arg("artist").as_str(), arg("genre").as_str())))
        }
        "library.artists" => {
            Some(Ok(app.lib.artists(arg("kind").as_str().unwrap_or("track"))))
        }
        "library.genres" => Some(Ok(app.lib.genres())),
        "library.album" => {
            let key = arg("albumKey").as_str().unwrap_or("").to_string();
            Some(Ok(app.lib.album(&key)))
        }
        "library.artist" => {
            let name = arg("name").as_str().unwrap_or("").to_string();
            Some(Ok(app.lib.artist(&name)))
        }
        "library.folders" => Some(Ok(app.lib.folders())),
        "library.folder" => {
            let path = arg("path").as_str().unwrap_or("").to_string();
            Some(Ok(app.lib.folder(&path)))
        }
        "library.addFolder" => {
            let path = match arg("path").as_str() {
                Some(p) if !p.is_empty() => p.to_string(),
                _ => {
                    // No path: the native folder picker, on the main thread.
                    let picked = rfd::FileDialog::new()
                        .set_title("Choose a folder to add to the library")
                        .pick_folder();
                    match picked {
                        Some(p) => p.to_string_lossy().to_string(),
                        None => return Some(Ok(json!({ "ok": false, "cancelled": true }))),
                    }
                }
            };
            let r = app.lib.add_folder(&path);
            if r.is_ok() {
                let _ = app.watcher.send(library::WatcherCmd::Add(path.clone()));
                let lib = Arc::clone(&app.lib);
                let ev = Arc::clone(&app.events);
                std::thread::Builder::new()
                    .name("rescan".into())
                    .spawn(move || lib.rescan(&ev))
                    .ok();
            }
            Some(r)
        }
        "library.removeFolder" => {
            let id = arg("id").as_i64().unwrap_or(0);
            let path = app
                .lib
                .folders()
                .as_array()
                .and_then(|a| a.iter().find(|f| f["id"].as_i64() == Some(id)))
                .and_then(|f| f["path"].as_str().map(String::from));
            let r = app.lib.remove_folder(id);
            if r.is_ok() {
                if let Some(p) = path {
                    let _ = app.watcher.send(library::WatcherCmd::Remove(p));
                }
            }
            Some(r)
        }
        "library.rescan" => {
            let lib = Arc::clone(&app.lib);
            let ev = Arc::clone(&app.events);
            std::thread::Builder::new()
                .name("rescan".into())
                .spawn(move || lib.rescan(&ev))
                .ok();
            None // scan.progress and library.changed events carry the result
        }
        "library.search" => {
            let q = arg("q").as_str().unwrap_or("").to_string();
            Some(Ok(app.lib.search(&q)))
        }

        // ---- track ----
        "track.setLove" => {
            let (id, love) = (arg("id").as_i64().unwrap_or(0), arg("love").as_bool().unwrap_or(false));
            let r = app.lib.set_love(id, love);
            if r.is_ok() {
                app.scrobbler.love(&app.lib.track_by_id(id).unwrap_or(Value::Null), love);
            }
            Some(r)
        }
        "track.setRating" => {
            let (id, rating) = (arg("id").as_i64().unwrap_or(0), arg("rating").as_i64().unwrap_or(0));
            Some(app.lib.set_rating(id, rating))
        }
        "track.delete" => {
            let ids: Vec<i64> = arg("ids")
                .as_array()
                .map(|a| a.iter().filter_map(|v| v.as_i64()).collect())
                .unwrap_or_default();
            Some(app.lib.delete_tracks(&ids))
        }
        "track.showInFolder" => {
            let id = arg("id").as_i64().unwrap_or(0);
            match app.lib.track_by_id(id).and_then(|t| t["path"].as_str().map(String::from)) {
                Some(p) => {
                    let _ = std::process::Command::new("explorer.exe").arg(format!("/select,{p}")).spawn();
                    Some(Ok(json!({ "ok": true })))
                }
                None => Some(Err("track not found".into())),
            }
        }

        // ---- playlists ----
        "playlists.list" => Some(Ok(app.lib.playlists())),
        "playlists.tracks" => {
            let path = arg("path").as_str().unwrap_or("").to_string();
            Some(Ok(app.lib.playlist_tracks(&path)))
        }
        "playlists.create" => {
            let name = arg("name").as_str().unwrap_or("").to_string();
            Some(app.lib.playlist_create(&name))
        }
        "playlists.rename" => {
            let (path, name) = (
                arg("path").as_str().unwrap_or("").to_string(),
                arg("name").as_str().unwrap_or("").to_string(),
            );
            Some(app.lib.playlist_rename(&path, &name))
        }
        "playlists.delete" => {
            let path = arg("path").as_str().unwrap_or("").to_string();
            Some(app.lib.playlist_delete(&path))
        }
        "playlists.add" => {
            let path = arg("path").as_str().unwrap_or("").to_string();
            let ids: Vec<i64> = arg("trackIds")
                .as_array()
                .map(|a| a.iter().filter_map(|v| v.as_i64()).collect())
                .unwrap_or_default();
            Some(app.lib.playlist_add(&path, &ids))
        }
        "playlists.remove" => {
            let path = arg("path").as_str().unwrap_or("").to_string();
            let idx: Vec<usize> = arg("indexes")
                .as_array()
                .map(|a| a.iter().filter_map(|v| v.as_usize()).collect())
                .unwrap_or_default();
            Some(app.lib.playlist_remove(&path, &idx))
        }
        "playlists.reorder" => {
            let path = arg("path").as_str().unwrap_or("").to_string();
            let order: Vec<usize> = arg("order")
                .as_array()
                .map(|a| a.iter().filter_map(|v| v.as_usize()).collect())
                .unwrap_or_default();
            Some(app.lib.playlist_reorder(&path, &order))
        }

        // ---- player ----
        "player.play" => {
            let ids = resolve_play_source(app, args);
            let start = arg("startIndex").as_usize().unwrap_or(0);
            Some(app.player.play(&ids, start))
        }
        "player.toggle" => Some(app.player.toggle()),
        "player.next" => Some(app.player.next()),
        "player.prev" => Some(app.player.prev()),
        "player.seek" => Some(app.player.seek(arg("seconds").as_f64().unwrap_or(0.0))),
        "player.volume" => Some(app.player.set_volume(arg("value").as_f64().unwrap_or(1.0))),
        "player.shuffle" => Some(app.player.set_shuffle(arg("on").as_bool().unwrap_or(false))),
        "player.loop" => {
            let mode = arg("mode").as_str().unwrap_or("off").to_string();
            Some(app.player.set_loop(&mode))
        }
        "player.queue" => Some(Ok(app.player.queue_json())),
        "player.queueAdd" => {
            let ids: Vec<i64> = arg("trackIds")
                .as_array()
                .map(|a| a.iter().filter_map(|v| v.as_i64()).collect())
                .unwrap_or_default();
            Some(app.player.queue_add(&ids, arg("next").as_bool().unwrap_or(false)))
        }
        "player.queueRemove" => {
            let idx: Vec<usize> = arg("indexes")
                .as_array()
                .map(|a| a.iter().filter_map(|v| v.as_usize()).collect())
                .unwrap_or_default();
            Some(app.player.queue_remove(&idx))
        }
        "player.queueMove" => {
            let from = arg("from").as_usize().unwrap_or(0);
            let to = arg("to").as_usize().unwrap_or(0);
            Some(app.player.queue_move(from, to))
        }
        "player.state" => Some(Ok(app.player.state_json())),
        "player.sleepTimer" => Some(app.player.sleep_timer(
            arg("minutes").as_f64(),
            arg("endOfSong").as_bool().unwrap_or(false),
        )),
        "eq.get" => Some(Ok(app.player.eq_get())),
        "eq.set" => {
            let gains: Vec<f64> = arg("gains")
                .as_array()
                .map(|a| a.iter().filter_map(|v| v.as_f64()).collect())
                .unwrap_or_default();
            Some(app.player.eq_set(
                arg("enabled").as_bool().unwrap_or(false),
                arg("preset").as_str().unwrap_or("Flat"),
                &gains,
            ))
        }

        // ---- lyrics / online ----
        "lyrics.get" => {
            let id = arg("trackId").as_i64().unwrap_or(0);
            match app.lib.track_by_id(id) {
                Some(t) => {
                    let path = t["path"].as_str().unwrap_or("").to_string();
                    let title = t["title"].as_str().unwrap_or("").to_string();
                    let artist = t["artists"]
                        .as_array()
                        .and_then(|a| a.first())
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let album = t["album"].as_str().unwrap_or("").to_string();
                    let dur = t["duration"].as_f64().unwrap_or(0.0);
                    Some(Ok(lyrics::get(&path, &title, &artist, &album, dur)))
                }
                None => Some(Err("track not found".into())),
            }
        }
        "online.search" => {
            let q = arg("q").as_str().unwrap_or("").to_string();
            let online = app.online_clone();
            std::thread::Builder::new()
                .name("online-search".into())
                .spawn(move || online.search(&q))
                .ok();
            None
        }
        "online.stream" => {
            let rid = arg("resultId").as_str().unwrap_or("").to_string();
            Some(Ok(app.online.stream(&rid)))
        }
        "online.download" => {
            let rid = arg("resultId").as_str().unwrap_or("").to_string();
            Some(Ok(app.online.download(&rid)))
        }
        "online.cancel" => {
            let job = arg("jobId").as_str().unwrap_or("").to_string();
            app.online.cancel(&job);
            Some(Ok(json!({ "ok": true })))
        }

        // ---- settings / devices / window / app ----
        "settings.get" => Some(Ok(app.settings.settings.lock().unwrap().to_json())),
        "settings.set" => {
            let key = arg("key").as_str().unwrap_or("").to_string();
            let value = arg("value");
            if key.is_empty() {
                return Some(Err("settings.set needs a key".into()));
            }
            let known = app.settings.update(|s| {
                let known = s.set_key(&key, value.clone());
                known
            });
            apply_setting_side_effects(app, &key, &known);
            Some(Ok(json!({ "ok": true })))
        }
        "devices.list" => Some(Ok(app.player.list_devices())),
        "devices.set" => {
            let id = arg("id").as_str().unwrap_or("").to_string();
            let name = arg("name").as_str().map(String::from);
            Some(app.player.set_device(&id, name.as_deref()))
        }
        "window.minimize" => None, // handled by the caller, needs the window
        "window.maximize" => None,
        "window.close" => None,
        "window.startDrag" => None,
        "app.info" => Some(Ok(json!({
            "version": env!("CARGO_PKG_VERSION"),
            "memoryMb": memory_mb(),
        }))),
        _ => Some(Err(format!("unknown command: {cmd}"))),
    }
}

/// player.play sources: trackIds+startIndex, a playlist path, an albumKey or
/// an artist name all resolve to a list of track ids.
fn resolve_play_source(app: &App, args: &Value) -> Vec<i64> {
    if let Some(ids) = args["trackIds"].as_array() {
        return ids.iter().filter_map(|v| v.as_i64()).collect();
    }
    if let Some(p) = args["playlist"].as_str() {
        return app.lib.playlist_track_ids(p);
    }
    if let Some(k) = args["albumKey"].as_str() {
        return app.lib.album_track_ids(k);
    }
    if let Some(a) = args["artist"].as_str() {
        return app.lib.artist_track_ids(a);
    }
    Vec::new()
}

/// Settings changes the backend must act on right away.
fn apply_setting_side_effects(app: &App, key: &str, _s: &settings::Settings) {
    match key {
        "discord_enabled" => {
            let on = app.settings.settings.lock().unwrap().discord_enabled;
            let mut d = app.discord.lock().unwrap();
            d.enabled = on;
            if !on {
                d.clear();
            }
        }
        _ => {}
    }
}

impl App {
    fn online_clone(&self) -> Arc<Online> {
        Arc::clone(&self.online)
    }
}

fn memory_mb() -> u64 {
    unsafe {
        let mut pmc = PROCESS_MEMORY_COUNTERS_EX::default();
        let size = std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32;
        let ok = GetProcessMemoryInfo(
            GetCurrentProcess(),
            &mut pmc as *mut _ as *mut _,
            size,
        );
        if ok.is_ok() {
            pmc.PrivateUsage.max(0) as u64 / (1024 * 1024)
        } else {
            0
        }
    }
}

/// The app icon, drawn in code: a Mocha crust rounded square with a blue to
/// mauve gradient and three sound bars, the same family as Etcher's icon.
fn icon_rgba(n: u32) -> Vec<u8> {
    let s = n as f32;
    let r = s * 0.24;
    let (a, b) = ([0x89, 0xb4, 0xfa], [0xcb, 0xa6, 0xf7]);
    let bars = [(0.25, 0.48), (0.44, 0.26), (0.63, 0.38)];
    let (bw, bottom) = (0.12, 0.76);
    let mut px = vec![0u8; (n * n * 4) as usize];
    for y in 0..n {
        for x in 0..n {
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            let dx = (r - fx).max(fx - (s - r)).max(0.0);
            let dy = (r - fy).max(fy - (s - r)).max(0.0);
            let d = (dx * dx + dy * dy).sqrt() - r;
            let cov = (0.5 - d).clamp(0.0, 1.0);
            if cov <= 0.0 {
                continue;
            }
            let t = ((fx + fy) / (2.0 * s)).clamp(0.0, 1.0);
            let mut c = [0u8; 3];
            for i in 0..3 {
                c[i] = (a[i] as f32 + (b[i] as f32 - a[i] as f32) * t) as u8;
            }
            for (bx, top) in bars {
                let (l, rr, tp, bt) = (bx * s, (bx + bw) * s, top * s, bottom * s);
                let inside = (fx - l).min(rr - fx).min(fy - tp).min(bt - fy);
                let k = (inside + 0.5).clamp(0.0, 1.0);
                if k > 0.0 {
                    let crust = [0x11u8, 0x11, 0x1b];
                    for i in 0..3 {
                        c[i] = (c[i] as f32 * (1.0 - k) + crust[i] as f32 * k) as u8;
                    }
                }
            }
            let i = ((y * n + x) * 4) as usize;
            px[i..i + 3].copy_from_slice(&c);
            px[i + 3] = (cov * 255.0) as u8;
        }
    }
    px
}

/// Dark tray menus, through uxtheme's unnamed SetPreferredAppMode.
fn set_menu_mode(dark: bool) {
    unsafe {
        let Ok(lib) = LoadLibraryW(w!("uxtheme.dll")) else { return };
        if let Some(f) = GetProcAddress(lib, PCSTR(135 as *const u8)) {
            let set: extern "system" fn(i32) -> i32 = std::mem::transmute(f);
            set(if dark { 2 } else { 3 });
        }
        if let Some(f) = GetProcAddress(lib, PCSTR(136 as *const u8)) {
            let flush: extern "system" fn() = std::mem::transmute(f);
            flush();
        }
    }
}

/// The glass: dark Mocha caption, rounded corners, acrylic backdrop. When
/// acrylic is not available the caption and the page background stay opaque
/// Mocha base, which is the fallback.
fn apply_glass(window: &Window) {
    let hwnd = HWND(window.hwnd() as *mut c_void);
    let bg: u32 = 0x1e1e2e;
    unsafe {
        let dark: i32 = 1;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            &dark as *const i32 as *const c_void,
            4,
        );
        let round: i32 = DWMWCP_ROUND.0;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &round as *const i32 as *const c_void,
            4,
        );
        let m = MARGINS { cxLeftWidth: -1, cxRightWidth: -1, cyTopHeight: -1, cyBottomHeight: -1 };
        let _ = DwmExtendFrameIntoClientArea(hwnd, &m);
        let acrylic = DWM_SYSTEMBACKDROP_TYPE(3); // DWMSBT_TRANSIENTWINDOW
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_SYSTEMBACKDROP_TYPE,
            &acrylic as *const DWM_SYSTEMBACKDROP_TYPE as *const c_void,
            4,
        );
        // COLORREF is 0x00BBGGRR; a dark caption so no system-painted strip
        // can ever show light over the page.
        let caption: u32 = ((bg & 0xff) << 16) | (bg & 0xff00) | ((bg >> 16) & 0xff);
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_CAPTION_COLOR,
            &caption as *const u32 as *const c_void,
            4,
        );
    }
}

fn show(window: &Window, webview: &WebView) {
    window.set_visible(true);
    if window.is_minimized() {
        window.set_minimized(false);
    }
    window.set_focus();
    let _ = webview.evaluate_script("window.__smpHidden&&window.__smpHidden(false)");
}

fn hide(window: &Window, webview: &WebView) {
    window.set_visible(false);
    let _ = webview.evaluate_script("window.__smpHidden&&window.__smpHidden(true)");
}

fn save_rect(settings: &SettingsFile, window: &Window) {
    if !window.is_visible() || window.is_minimized() {
        return;
    }
    if window.is_maximized() {
        settings.update(|s| {
            if let Some(r) = s.window.as_mut() {
                r.maximized = true;
            }
        });
        return;
    }
    let Ok(p) = window.outer_position() else { return };
    let sz = window.inner_size();
    let r = settings::WindowRect { x: p.x, y: p.y, w: sz.width, h: sz.height, maximized: false };
    settings.update(|s| s.window = Some(r));
}

fn close_requested(settings: &SettingsFile, window: &Window, webview: &WebView, cf: &mut ControlFlow, tray: &mut Option<TrayIcon>, player: &Arc<Player>, quit: bool) {
    if !quit && settings.settings.lock().unwrap().close_to_tray {
        hide(window, webview);
        return;
    }
    player.save_state();
    save_rect(settings, window);
    tray.take();
    *cf = ControlFlow::Exit;
}

/// A second launch: signal the running copy to show itself.
fn signal_running_copy() {
    unsafe {
        if let Ok(h) = OpenEventW(EVENT_MODIFY_STATE, false, w!("Local\\Smp.Show")) {
            let _ = SetEventW(h);
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--selftest") {
        let code = selftest();
        std::process::exit(code);
    }
    let hidden_arg = args.iter().any(|a| a == "--hidden");

    // One copy only: a second launch brings the first one forward.
    unsafe {
        let m = CreateMutexW(None, true, w!("Local\\Smp.App"));
        if m.is_ok() && GetLastError() == ERROR_ALREADY_EXISTS {
            signal_running_copy();
            return;
        }
        std::mem::forget(m);
    }

    library::ensure_app_dirs();
    let settings = Arc::new(SettingsFile::load(settings::settings_path()));
    let events = Events::new();
    let lib = match Library::open(&settings::db_path(), Arc::clone(&events)) {
        Ok(l) => Arc::new(l),
        Err(e) => {
            eprintln!("could not open the library database: {e}");
            return;
        }
    };
    let player = Player::new(Arc::clone(&events), Arc::clone(&lib), Arc::clone(&settings));
    let smtc = Smtc::new();
    let discord_on = settings.settings.lock().unwrap().discord_enabled;
    let watcher = library::Library::start_watcher(Arc::clone(&lib));
    let app = Arc::new(App {
        settings: Arc::clone(&settings),
        events: Arc::clone(&events),
        lib: Arc::clone(&lib),
        player: Arc::clone(&player),
        smtc: Mutex::new(smtc),
        discord: Mutex::new(discord::Discord::new(discord_on)),
        scrobbler: Scrobbler::new(),
        online: Arc::new(Online::new()),
        watcher,
    });

    // The one-time Dopamine import, when this library is still empty (step 3).
    {
        let app2 = Arc::clone(&app);
        std::thread::Builder::new()
            .name("import".into())
            .spawn(move || migrate::maybe_import(&app2))
            .ok();
    }

    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    events.attach(event_loop.create_proxy());
    {
        let p = event_loop.create_proxy();
        TrayIconEvent::set_event_handler(Some(move |e| {
            let _ = p.send_event(UserEvent::Tray(e));
        }));
    }
    {
        let p = event_loop.create_proxy();
        MenuEvent::set_event_handler(Some(move |e| {
            let _ = p.send_event(UserEvent::Menu(e));
        }));
    }

    let start_hidden = hidden_arg || settings.settings.lock().unwrap().start_hidden;
    let dark = true;
    set_menu_mode(dark);

    let mut wb = WindowBuilder::new()
        .with_title("Seamless Music Player")
        .with_inner_size(LogicalSize::new(1200.0, 800.0))
        .with_min_inner_size(LogicalSize::new(760.0, 560.0))
        .with_transparent(true)
        .with_visible(false)
        .with_window_icon(Icon::from_rgba(icon_rgba(64), 64, 64).ok());
    if let Some(r) = settings.settings.lock().unwrap().window {
        let on_screen = event_loop.available_monitors().any(|m| {
            let (mp, ms) = (m.position(), m.size());
            let (x, y) = (r.x + 80, r.y + 16);
            x >= mp.x && x < mp.x + ms.width as i32 && y >= mp.y && y < mp.y + ms.height as i32
        });
        if on_screen && r.w >= 760 && r.h >= 560 {
            wb = wb
                .with_position(PhysicalPosition::new(r.x, r.y))
                .with_inner_size(PhysicalSize::new(r.w, r.h))
                .with_maximized(r.maximized);
        }
    }
    let window = wb.build(&event_loop).expect("window");
    apply_glass(&window);

    let data_dir = app_dir().join("WebView2");
    let mut ctx = wry::WebContext::new(Some(data_dir));
    let ipc_proxy = event_loop.create_proxy();
    let load_proxy = event_loop.create_proxy();
    let webview = WebViewBuilder::new_with_web_context(&mut ctx)
        .with_url("smp://app/index.html")
        .with_transparent(true)
        .with_devtools(false)
        .with_default_context_menus(false)
        .with_hotkeys_zoom(false)
        .with_custom_protocol("smp".into(), move |_, req: Request<Vec<u8>>| {
            serve_uri(req.uri().path())
        })
        .with_ipc_handler(move |req| {
            let _ = ipc_proxy.send_event(UserEvent::Ipc(req.into_body()));
        })
        .with_on_page_load_handler(move |ev, _| {
            if matches!(ev, wry::PageLoadEvent::Finished) {
                let _ = load_proxy.send_event(UserEvent::Loaded);
            }
        })
        .with_navigation_handler(move |url| url.starts_with("smp://"))
        .build(&window)
        .expect("webview");

    let mut tray: Option<TrayIcon> = None;
    let item_show = MenuItem::with_id("show", "Show", true, None);
    let item_toggle = MenuItem::with_id("toggle", "Play", true, None);
    let item_next = MenuItem::with_id("next", "Next", true, None);
    let item_prev = MenuItem::with_id("prev", "Previous", true, None);
    let item_quit = MenuItem::with_id("quit", "Quit", true, None);
    let mut shown_once = false;
    let mut rect_dirty = false;
    let mut last_sync = Instant::now() - Duration::from_secs(10);

    // The show-event listener: a second launch wakes this window.
    {
        let p = event_loop.create_proxy();
        std::thread::Builder::new().name("single-instance".into()).spawn(move || unsafe {
            if let Ok(h) = CreateEventW(None, true, false, w!("Local\\Smp.Show")) {
                loop {
                    if WaitForSingleObject(h, 0xFFFFFFFF) == WAIT_OBJECT_0 {
                        let _ = p.send_event(UserEvent::Show);
                    }
                }
            }
        }).ok();
    }

    event_loop.run(move |event, _, cf| {
        *cf = ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(1000));
        let _keep = &ctx;
        match event {
            Event::NewEvents(StartCause::Init) => {
                let menu = Menu::new();
                let _ = menu.append_items(&[
                    &item_show,
                    &PredefinedMenuItem::separator(),
                    &item_toggle,
                    &item_next,
                    &item_prev,
                    &PredefinedMenuItem::separator(),
                    &item_quit,
                ]);
                tray = TrayIconBuilder::new()
                    .with_menu(Box::new(menu))
                    .with_menu_on_left_click(false)
                    .with_tooltip("Seamless Music Player")
                    .with_icon(tray_icon::Icon::from_rgba(icon_rgba(32), 32, 32).expect("icon"))
                    .build()
                    .ok();
            }
            Event::UserEvent(UserEvent::Loaded) => {
                if !shown_once {
                    shown_once = true;
                    if !start_hidden {
                        show(&window, &webview);
                    } else {
                        let _ = webview.evaluate_script("window.__smpHidden&&window.__smpHidden(true)");
                    }
                }
            }
            Event::UserEvent(UserEvent::Show) => show(&window, &webview),
            Event::UserEvent(UserEvent::Quit) => {
                close_requested(&settings, &window, &webview, cf, &mut tray, &player, true);
            }
            Event::UserEvent(UserEvent::Tray(TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            })) => {
                if window.is_visible() && !window.is_minimized() {
                    hide(&window, &webview);
                } else {
                    show(&window, &webview);
                }
            }
            Event::UserEvent(UserEvent::Tray(_)) => {}
            Event::UserEvent(UserEvent::Menu(e)) => match e.id.0.as_str() {
                "show" => show(&window, &webview),
                "toggle" => {
                    let _ = player.toggle();
                    player.push_state();
                }
                "next" => {
                    let _ = player.next();
                }
                "prev" => {
                    let _ = player.prev();
                }
                "quit" => {
                    close_requested(&settings, &window, &webview, cf, &mut tray, &player, true);
                }
                _ => {}
            },
            Event::UserEvent(UserEvent::Ipc(msg)) => {
                let v: Value = serde_json::from_str(&msg).unwrap_or(Value::Null);
                let id = v["id"].as_u64().unwrap_or(0);
                let cmd = v["cmd"].as_str().unwrap_or("").to_string();
                if cmd.is_empty() {
                    reply(&events, id, false, Value::from("empty command"));
                    return;
                }
                // window.* commands need the window itself; the rest dispatch.
                match cmd.as_str() {
                    "window.minimize" => {
                        if settings.settings.lock().unwrap().minimize_to_tray {
                            hide(&window, &webview);
                        } else {
                            window.set_minimized(true);
                        }
                        reply(&events, id, true, json!({ "ok": true }));
                    }
                    "window.maximize" => {
                        window.set_maximized(!window.is_maximized());
                        reply(&events, id, true, json!({ "ok": true }));
                    }
                    "window.close" => {
                        reply(&events, id, true, json!({ "ok": true }));
                        close_requested(&settings, &window, &webview, cf, &mut tray, &player, false);
                    }
                    "window.startDrag" => {
                        let _ = window.drag_window();
                        reply(&events, id, true, json!({ "ok": true }));
                    }
                    _ => match dispatch(&app, id, &cmd, &v["args"]) {
                        Some(Ok(data)) => reply(&events, id, true, data),
                        Some(Err(e)) => reply(&events, id, false, Value::from(e)),
                        None => {} // a worker thread replies when done
                    },
                }
            }
            Event::UserEvent(UserEvent::Emit(script)) => {
                let _ = webview.evaluate_script(&script);
            }
            Event::WindowEvent { event: WindowEvent::CloseRequested, .. } => {
                close_requested(&settings, &window, &webview, cf, &mut tray, &player, false);
            }
            Event::WindowEvent { event: WindowEvent::Moved(_), .. } => {
                rect_dirty = true;
            }
            Event::WindowEvent { event: WindowEvent::Resized(size), .. } => {
                rect_dirty = true;
                let _ = webview.set_bounds(wry::Rect {
                    position: wry::dpi::PhysicalPosition::new(0, 0).into(),
                    size: wry::dpi::PhysicalSize::new(size.width, size.height).into(),
                });
                let _ = size;
            }
            Event::MainEventsCleared => {}
            _ => {}
        }
        if last_sync.elapsed() >= Duration::from_millis(1000) {
            last_sync = Instant::now();
            if rect_dirty {
                rect_dirty = false;
                save_rect(&settings, &window);
            }
            if let Some(tr) = tray.as_ref() {
                let playing = player.state_json()["playing"].as_bool().unwrap_or(false);
                let label = if playing { "Pause" } else { "Play" };
                item_toggle.set_text(label);
                let _ = tr.set_tooltip(Some("Seamless Music Player"));
            }
        }
    });
}
