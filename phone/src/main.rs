//! Music Search for the phone. A small web server on the always-on laptop:
//! search Soulseek (slskd on the tower) and YouTube, tap Add, the song or
//! album lands in the library folder Navidrome reads, then the page says when
//! Amperfy can see it. Logins come from the sign-in doors key folder at run
//! time and are never sent to the page. YouTube work runs on the tower over
//! ssh, because the laptop cannot write the music share and heavy work
//! belongs on the tower.

#![windows_subsystem = "windows"]

mod names;

use md5::{Digest, Md5};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const LISTEN: &str = "127.0.0.1:8332";
const SLSKD_ADDR: &str = "192.168.0.11:5030";
const SLSKD: &str = "http://192.168.0.11:5030/api/v0";
const NAVIDROME: &str = "http://127.0.0.1:4533/rest";
const WAKE_URL: &str = "http://127.0.0.1:8899/wake?machine=tower";
const SSH: &str = r"C:\Windows\System32\OpenSSH\ssh.exe";
const SSH_TARGET: &str = "localuser@192.168.0.11";
const YTDLP: &str = r"C:\Users\localuser\AppData\Local\Programs\Python\Python313\Scripts\yt-dlp.exe";
const YT_FOLDER: &str = r"G:\Music\Downloads\YouTube";
const YT_TEMP: &str = r"G:\Music Downloads\Incomplete";
const PAGE: &str = include_str!("../web/index.html");
const NO_WINDOW: u32 = 0x0800_0000;

const PARALLEL_PEERS: usize = 3;
const MAX_PEERS: usize = 8;
const YOUTUBE_AFTER: Duration = Duration::from_secs(12);
const STALL: Duration = Duration::from_secs(20);
const QUEUE_WAIT: Duration = Duration::from_secs(25);
const ALBUM_STALL: Duration = Duration::from_secs(300);
const YT_LIMIT: Duration = Duration::from_secs(300);
const READY_WAIT: Duration = Duration::from_secs(240);
const MIN_QUERY: usize = 2;
const KEEP_SEARCHES: usize = 6;
const KEEP_JOBS: usize = 40;

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn now_ms() -> u128 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0)
}

fn local_app_data() -> PathBuf {
    std::env::var("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("."))
}

fn log(msg: &str) {
    let dir = local_app_data().join("music-search-phone");
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("log.txt");
    if std::fs::metadata(&path).map(|m| m.len() > 1_000_000).unwrap_or(false) {
        let _ = std::fs::rename(&path, dir.join("log.old.txt"));
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{} {msg}", now_ms() / 1000);
    }
}

fn secret(name: &str) -> Result<String, String> {
    std::fs::read_to_string(local_app_data().join("signin-doors").join(name))
        .map(|s| s.trim().to_string())
        .map_err(|_| format!("key file {name} missing"))
}

fn enc(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn agent(timeout: u64) -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(4))
        .timeout(Duration::from_secs(timeout))
        .build()
}

// ---------------------------------------------------------------------------
// slskd
// ---------------------------------------------------------------------------

struct Slskd {
    token: Mutex<String>,
    agent: ureq::Agent,
}

impl Slskd {
    fn new() -> Self {
        Slskd { token: Mutex::new(String::new()), agent: agent(10) }
    }

    fn raw(&self, method: &str, route: &str, body: Option<&Value>, token: &str) -> Result<Value, String> {
        let mut req = self.agent.request(method, &format!("{SLSKD}{route}")).set("Accept", "application/json");
        if !token.is_empty() {
            req = req.set("Authorization", &format!("Bearer {token}"));
        }
        let resp = match body {
            Some(b) => req.send_json(b.clone()),
            None => req.call(),
        };
        match resp {
            Ok(r) => {
                let text = r.into_string().unwrap_or_default();
                if text.trim().is_empty() {
                    Ok(json!({}))
                } else {
                    Ok(serde_json::from_str(&text).unwrap_or(json!({})))
                }
            }
            Err(ureq::Error::Status(code, _)) => Err(code.to_string()),
            Err(e) => Err(format!("connect: {e}")),
        }
    }

    fn sign_in(&self) -> Result<String, String> {
        let body = json!({ "username": "music", "password": secret("slskd.txt")? });
        let v = self.raw("POST", "/session", Some(&body), "")?;
        let token = v["token"].as_str().unwrap_or("").to_string();
        if token.is_empty() {
            return Err("signin".into());
        }
        *lock(&self.token) = token.clone();
        Ok(token)
    }

    fn api(&self, method: &str, route: &str, body: Option<&Value>) -> Result<Value, String> {
        let token = {
            let t = lock(&self.token).clone();
            if t.is_empty() { self.sign_in()? } else { t }
        };
        match self.raw(method, route, body, &token) {
            Err(e) if e == "401" => {
                let fresh = self.sign_in()?;
                self.raw(method, route, body, &fresh)
            }
            other => other,
        }
    }
}

fn peers_error(e: &str) -> String {
    if e == "401" || e == "signin" || e.starts_with("key file") {
        "Soulseek would not let us sign in.".into()
    } else if e.starts_with("connect") {
        "Soulseek on the tower is not answering.".into()
    } else {
        "Soulseek did not answer.".into()
    }
}

// ---------------------------------------------------------------------------
// Navidrome (the library Amperfy reads)
// ---------------------------------------------------------------------------

fn nav(endpoint: &str, params: &[(&str, &str)]) -> Result<Value, String> {
    let pw = secret("navidrome.txt")?;
    let salt = format!("{:x}", now_ms() ^ 0x5bd1_e995);
    let token: String = Md5::digest(format!("{pw}{salt}").as_bytes()).iter().map(|b| format!("{b:02x}")).collect();
    let mut url = format!("{NAVIDROME}/{endpoint}?u=music&t={token}&s={salt}&v=1.16.1&c=music-search-phone&f=json");
    for (k, v) in params {
        url.push('&');
        url.push_str(k);
        url.push('=');
        url.push_str(&enc(v));
    }
    let text = agent(10).get(&url).call().map_err(|e| e.to_string())?.into_string().map_err(|e| e.to_string())?;
    let v: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    let r = v["subsonic-response"].clone();
    if r["status"].as_str() == Some("ok") {
        Ok(r)
    } else {
        Err("navidrome said no".into())
    }
}

#[derive(Default)]
struct Library {
    songs: Vec<(String, String)>,
    albums: Vec<(String, String)>,
}

fn library_for(query: &str) -> Option<Library> {
    let r = nav("search3", &[("query", query), ("songCount", "200"), ("albumCount", "40"), ("artistCount", "0")]).ok()?;
    let res = &r["searchResult3"];
    let pairs = |list: &Value, name: &str| -> Vec<(String, String)> {
        list.as_array()
            .map(|a| {
                a.iter()
                    .map(|s| (names::normalize(s[name].as_str().unwrap_or(""), true), names::normalize(s["artist"].as_str().unwrap_or(""), true)))
                    .collect()
            })
            .unwrap_or_default()
    };
    Some(Library { songs: pairs(&res["song"], "title"), albums: pairs(&res["album"], "name") })
}

fn in_library(list: &[(String, String)], title: &str, artist: &str) -> bool {
    let t = names::normalize(title, true);
    let a = names::normalize(artist, true);
    !t.is_empty()
        && list.iter().any(|(lt, la)| *lt == t && (a.is_empty() || la.is_empty() || la.contains(&a) || a.contains(la.as_str())))
}

// ---------------------------------------------------------------------------
// The tower: awake check, wake, ssh to PowerShell
// ---------------------------------------------------------------------------

fn tower_up(sh: &Shared) -> bool {
    {
        let g = lock(&sh.tower);
        if g.0.elapsed() < Duration::from_secs(3) {
            return g.1;
        }
    }
    let up = SLSKD_ADDR
        .parse()
        .ok()
        .map(|a| std::net::TcpStream::connect_timeout(&a, Duration::from_millis(1500)).is_ok())
        .unwrap_or(false);
    *lock(&sh.tower) = (Instant::now(), up);
    up
}

fn wake(sh: &Shared) {
    let mut g = lock(&sh.woke);
    if g.map(|t| t.elapsed() < Duration::from_secs(180)).unwrap_or(false) {
        return;
    }
    *g = Some(Instant::now());
    log("tower asleep, asking the wake relay");
    thread::spawn(|| {
        let _ = agent(20).get(WAKE_URL).call();
    });
}

fn waking(sh: &Shared) -> bool {
    lock(&sh.woke).map(|t| t.elapsed() < Duration::from_secs(180)).unwrap_or(false)
}

/// PowerShell single-quoted literal.
fn psq(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// User text going into a command: no double quotes (Windows PowerShell
/// mangles them on the way to a program), no control characters, kept short.
fn safe_text(s: &str) -> String {
    s.chars().filter(|c| !c.is_control() && *c != '"' && *c != '`').take(120).collect::<String>().trim().to_string()
}

fn b64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = (chunk[0] as u32) << 16 | (*chunk.get(1).unwrap_or(&0) as u32) << 8 | *chunk.get(2).unwrap_or(&0) as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

/// Run a PowerShell script on the tower, hidden, output piped back.
fn ssh_ps(script: &str) -> std::io::Result<Child> {
    let utf16: Vec<u8> = script.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
    Command::new(SSH)
        .args([
            "-o", "BatchMode=yes", "-o", "ConnectTimeout=8", "-o", "ServerAliveInterval=15", "-o", "ServerAliveCountMax=3",
            SSH_TARGET,
        ])
        .arg(format!("powershell -NoProfile -NonInteractive -EncodedCommand {}", b64(&utf16)))
        .creation_flags(NO_WINDOW)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
}

fn yt_args(args: &[String]) -> String {
    format!(
        "[Console]::OutputEncoding=[Text.Encoding]::UTF8; $env:PYTHONIOENCODING='utf-8'; & {} {}",
        psq(YTDLP),
        args.iter().map(|a| psq(a)).collect::<Vec<_>>().join(" ")
    )
}

// ---------------------------------------------------------------------------
// Shared state
// ---------------------------------------------------------------------------

struct Search {
    id: u64,
    query: String,
    songs: Vec<Value>,
    albums: Vec<Value>,
    videos: Vec<Value>,
    peers_busy: bool,
    yt_busy: bool,
    problem: String,
    asleep: bool,
    lib: Option<Library>,
    lib_done: bool,
}

#[derive(Clone)]
struct JobView {
    id: u64,
    key: String,
    kind: &'static str,
    title: String,
    artist: String,
    state: &'static str, // working | ready | failed
    label: String,
    percent: i64,
    source: String,
    made: u128,
    result: Value,
}

impl JobView {
    fn to_json(&self) -> Value {
        json!({
            "id": self.id, "key": self.key, "kind": self.kind, "title": self.title, "artist": self.artist,
            "state": self.state, "label": self.label, "percent": self.percent, "source": self.source, "made": self.made as u64,
        })
    }
}

struct Shared {
    searches: Mutex<Vec<Search>>,
    jobs: Mutex<Vec<JobView>>,
    slskd: Slskd,
    next: AtomicU64,
    tower: Mutex<(Instant, bool)>,
    woke: Mutex<Option<Instant>>,
}

fn with_search(sh: &Shared, id: u64, f: impl FnOnce(&mut Search)) -> bool {
    let mut g = lock(&sh.searches);
    match g.iter_mut().find(|s| s.id == id) {
        Some(s) => {
            f(s);
            true
        }
        None => false,
    }
}

fn upd(sh: &Shared, id: u64, f: impl FnOnce(&mut JobView)) {
    if let Some(j) = lock(&sh.jobs).iter_mut().find(|j| j.id == id) {
        f(j);
    }
}

fn label(sh: &Shared, id: u64, text: &str, percent: Option<i64>) {
    upd(sh, id, |j| {
        j.label = text.to_string();
        if let Some(p) = percent {
            j.percent = p.clamp(0, 100);
        }
    });
}

// ---------------------------------------------------------------------------
// Searching
// ---------------------------------------------------------------------------

fn start_search(sh: &Arc<Shared>, query: &str) -> u64 {
    let id = sh.next.fetch_add(1, Ordering::SeqCst);
    let asleep = !tower_up(sh);
    {
        let mut g = lock(&sh.searches);
        g.push(Search {
            id,
            query: query.to_string(),
            songs: vec![],
            albums: vec![],
            videos: vec![],
            peers_busy: !asleep,
            yt_busy: !asleep,
            problem: String::new(),
            asleep,
            lib: None,
            lib_done: false,
        });
        let extra = g.len().saturating_sub(KEEP_SEARCHES);
        g.drain(0..extra);
    }
    log(&format!("search {id}: {} chars{}", query.chars().count(), if asleep { ", tower asleep" } else { "" }));
    {
        let (sh, q) = (Arc::clone(sh), query.to_string());
        thread::spawn(move || {
            let lib = library_for(&q);
            with_search(&sh, id, |s| {
                s.lib = lib;
                s.lib_done = true;
            });
        });
    }
    if asleep {
        wake(sh);
        return id;
    }
    {
        let (sh, q) = (Arc::clone(sh), query.to_string());
        thread::spawn(move || peers_search(&sh, id, &q));
    }
    {
        let (sh, q) = (Arc::clone(sh), query.to_string());
        thread::spawn(move || youtube_search(&sh, id, &q));
    }
    id
}

fn peers_search(sh: &Shared, id: u64, query: &str) {
    let body = json!({ "searchText": query, "searchTimeout": 15000, "responseLimit": 150, "fileLimit": 10000 });
    let created = match sh.slskd.api("POST", "/searches", Some(&body)) {
        Ok(v) => v["id"].as_str().unwrap_or("").to_string(),
        Err(e) => {
            log(&format!("search {id}: soulseek {e}"));
            with_search(sh, id, |s| {
                s.problem = peers_error(&e);
                s.peers_busy = false;
            });
            return;
        }
    };
    for _ in 0..20 {
        thread::sleep(Duration::from_millis(1500));
        let done = sh
            .slskd
            .api("GET", &format!("/searches/{created}"), None)
            .map(|v| v["isComplete"].as_bool().unwrap_or(false) || v["state"].as_str().unwrap_or("").contains("Completed"))
            .unwrap_or(false);
        if let Ok(r) = sh.slskd.api("GET", &format!("/searches/{created}/responses"), None) {
            let (songs, albums) = names::build_results(r.as_array().map(|a| a.as_slice()).unwrap_or(&[]), query);
            if !with_search(sh, id, |s| {
                s.songs = songs;
                s.albums = albums;
            }) {
                break;
            }
        }
        if done {
            break;
        }
    }
    let _ = sh.slskd.api("DELETE", &format!("/searches/{created}"), None);
    with_search(sh, id, |s| s.peers_busy = false);
}

fn youtube_search(sh: &Shared, id: u64, query: &str) {
    let args = vec![
        "--flat-playlist".to_string(),
        "--dump-json".into(),
        "--no-warnings".into(),
        "--encoding".into(),
        "utf-8".into(),
        format!("ytsearch8:{}", safe_text(query)),
    ];
    if let Ok(mut child) = ssh_ps(&yt_args(&args)) {
        let out = child.stdout.take();
        if let Some(err) = child.stderr.take() {
            thread::spawn(move || {
                let _ = std::io::copy(&mut BufReader::new(err), &mut std::io::sink());
            });
        }
        let child = Arc::new(Mutex::new(child));
        {
            let child = Arc::clone(&child);
            thread::spawn(move || {
                thread::sleep(Duration::from_secs(45));
                let _ = lock(&child).kill();
            });
        }
        if let Some(out) = out {
            let mut found: Vec<Value> = vec![];
            for line in BufReader::new(out).lines().map_while(Result::ok) {
                if let Some(v) = names::youtube_song(&line) {
                    found.push(v);
                    let list = found.clone();
                    if !with_search(sh, id, |s| s.videos = list) {
                        break;
                    }
                }
            }
        }
        let _ = lock(&child).wait();
    }
    with_search(sh, id, |s| s.yt_busy = false);
}

fn snapshot(sh: &Shared, id: u64) -> Value {
    let g = lock(&sh.searches);
    let Some(s) = g.iter().find(|s| s.id == id) else {
        return json!({ "ok": false, "gone": true });
    };
    let lib = s.lib.as_ref();
    let songs: Vec<Value> = s
        .songs
        .iter()
        .map(|v| {
            let (t, a) = (v["title"].as_str().unwrap_or(""), v["artist"].as_str().unwrap_or(""));
            json!({
                "resultId": v["resultId"], "title": t, "artist": a, "album": v["album"], "quality": v["quality"],
                "lossless": v["lossless"], "duration": v["duration"],
                "copies": v["sources"].as_array().map(|a| a.len()).unwrap_or(0),
                "inLibrary": lib.map(|l| in_library(&l.songs, t, a)).unwrap_or(false),
            })
        })
        .collect();
    let albums: Vec<Value> = s
        .albums
        .iter()
        .map(|v| {
            let (t, a) = (v["title"].as_str().unwrap_or(""), v["artist"].as_str().unwrap_or(""));
            json!({
                "resultId": v["resultId"], "title": t, "artist": a, "quality": v["quality"], "lossless": v["lossless"],
                "songs": v["files"].as_array().map(|a| a.len()).unwrap_or(0), "copies": v["copies"],
                "inLibrary": lib.map(|l| in_library(&l.albums, t, a)).unwrap_or(false),
            })
        })
        .collect();
    let videos: Vec<Value> = s
        .videos
        .iter()
        .map(|v| json!({ "resultId": v["resultId"], "title": v["title"], "artist": v["artist"], "duration": v["duration"] }))
        .collect();
    json!({
        "ok": true, "id": s.id, "query": s.query, "songs": songs, "albums": albums, "videos": videos,
        "peersBusy": s.peers_busy, "ytBusy": s.yt_busy, "problem": s.problem, "asleep": s.asleep, "libDone": s.lib_done,
    })
}

// ---------------------------------------------------------------------------
// Adding: jobs
// ---------------------------------------------------------------------------

fn add(sh: &Arc<Shared>, search_id: u64, result_id: &str) -> Value {
    let found = {
        let g = lock(&sh.searches);
        g.iter().find(|s| s.id == search_id).and_then(|s| {
            s.songs.iter().chain(s.albums.iter()).chain(s.videos.iter()).find(|r| r["resultId"].as_str() == Some(result_id)).cloned()
        })
    };
    let Some(result) = found else {
        return json!({ "ok": false, "error": "That result is gone. Search again." });
    };
    if let Some(j) = lock(&sh.jobs).iter().find(|j| j.key == result_id && j.state == "working") {
        return json!({ "ok": true, "job": j.id });
    }
    if !tower_up(sh) {
        wake(sh);
        return json!({ "ok": false, "error": "The tower is asleep. Waking it now, try again in a minute." });
    }
    let id = sh.next.fetch_add(1, Ordering::SeqCst);
    let kind = if result_id.starts_with("album|") {
        "album"
    } else if result_id.starts_with("video|") {
        "video"
    } else {
        "song"
    };
    let view = JobView {
        id,
        key: result_id.to_string(),
        kind,
        title: result["title"].as_str().unwrap_or("Song").to_string(),
        artist: result["artist"].as_str().unwrap_or("").to_string(),
        state: "working",
        label: "Starting".into(),
        percent: 0,
        source: if kind == "video" { "YouTube".into() } else { "Soulseek".into() },
        made: now_ms(),
        result,
    };
    {
        let mut g = lock(&sh.jobs);
        g.push(view.clone());
        let extra = g.len().saturating_sub(KEEP_JOBS);
        g.drain(0..extra);
    }
    spawn_job(sh, view);
    json!({ "ok": true, "job": id })
}

fn retry(sh: &Arc<Shared>, job_id: u64) -> Value {
    let view = {
        let mut g = lock(&sh.jobs);
        let Some(j) = g.iter_mut().find(|j| j.id == job_id) else {
            return json!({ "ok": false, "error": "That add is gone." });
        };
        if j.state == "working" {
            return json!({ "ok": true, "job": j.id });
        }
        j.state = "working";
        j.label = "Starting again".into();
        j.percent = 0;
        j.made = now_ms();
        j.clone()
    };
    if !tower_up(sh) {
        wake(sh);
        upd(sh, job_id, |j| {
            j.state = "failed";
            j.label = "The tower is asleep. Waking it, try again in a minute.".into();
        });
        return json!({ "ok": false, "error": "The tower is asleep. Waking it now." });
    }
    spawn_job(sh, view);
    json!({ "ok": true, "job": job_id })
}

enum Check {
    Song { title: String, artist: String },
    Album { title: String },
}

fn spawn_job(sh: &Arc<Shared>, view: JobView) {
    let sh = Arc::clone(sh);
    thread::spawn(move || {
        log(&format!("job {}: {} started", view.id, view.kind));
        let outcome = match view.kind {
            "album" => album_job(&sh, view.id, &view.result),
            "video" => {
                let url = view.result["youtubeUrl"].as_str().unwrap_or("").to_string();
                youtube_fetch(&sh, view.id, &url, None)
            }
            _ => song_job(&sh, view.id, &view.result),
        };
        match outcome {
            Ok(check) => wait_ready(&sh, view.id, check),
            Err(msg) => {
                log(&format!("job {}: failed: {msg}", view.id));
                upd(&sh, view.id, |j| {
                    j.state = "failed";
                    j.label = msg;
                });
            }
        }
    });
}

struct Try {
    user: String,
    file: String,
    id: Option<String>,
    asked: Instant,
    last_move: Instant,
    bytes: f64,
    percent: f64,
    moving: bool,
    dead: bool,
}

/// One user's transfers: (filename, id, state, bytes, percent), newest
/// record per file.
fn transfers(sh: &Shared, user: &str) -> Option<Vec<(String, String, String, f64, f64)>> {
    let v = sh.slskd.api("GET", &format!("/transfers/downloads/{}", enc(user)), None).ok()?;
    let mut out: Vec<(String, String, String, f64, f64, String)> = vec![];
    for dir in v["directories"].as_array().map(|a| a.as_slice()).unwrap_or(&[]) {
        for f in dir["files"].as_array().map(|a| a.as_slice()).unwrap_or(&[]) {
            let name = f["filename"].as_str().unwrap_or("").to_string();
            let requested = f["requestedAt"].as_str().unwrap_or("").to_string();
            let rec = (
                name.clone(),
                f["id"].as_str().map(String::from).unwrap_or_else(|| f["id"].to_string()),
                f["state"].as_str().unwrap_or("").to_string(),
                f["bytesTransferred"].as_f64().unwrap_or(0.0),
                f["percentComplete"].as_f64().unwrap_or(0.0),
                requested.clone(),
            );
            match out.iter_mut().find(|r| r.0 == name) {
                Some(r) if r.5 < requested => *r = rec,
                Some(_) => {}
                None => out.push(rec),
            }
        }
    }
    Some(out.into_iter().map(|r| (r.0, r.1, r.2, r.3, r.4)).collect())
}

fn cancel(sh: &Shared, user: &str, id: &Option<String>) {
    if let Some(id) = id {
        let _ = sh.slskd.api("DELETE", &format!("/transfers/downloads/{}/{}?remove=true", enc(user), enc(id)), None);
    }
}

fn song_job(sh: &Shared, id: u64, res: &Value) -> Result<Check, String> {
    let title = res["title"].as_str().unwrap_or("").to_string();
    let artist = res["artist"].as_str().unwrap_or("").to_string();
    let mut sources: Vec<Value> = res["sources"].as_array().cloned().unwrap_or_default();
    sources.sort_by(|a, b| names::source_cost(a).partial_cmp(&names::source_cost(b)).unwrap_or(std::cmp::Ordering::Equal));
    sources.truncate(MAX_PEERS);
    let started = Instant::now();
    let mut tries: Vec<Try> = vec![];
    let mut next = 0;
    label(sh, id, "Asking people who have it", Some(0));
    loop {
        let moving = tries.iter().any(|t| t.moving && !t.dead);
        if !moving {
            while tries.iter().filter(|t| !t.dead).count() < PARALLEL_PEERS && next < sources.len() {
                let s = &sources[next];
                next += 1;
                let user = s["username"].as_str().unwrap_or("").to_string();
                let file = s["filename"].as_str().unwrap_or("").to_string();
                let body = json!([{ "filename": file, "size": s["size"].as_f64().unwrap_or(0.0) as i64 }]);
                if sh.slskd.api("POST", &format!("/transfers/downloads/{}", enc(&user)), Some(&body)).is_ok() {
                    let now = Instant::now();
                    tries.push(Try { user, file, id: None, asked: now, last_move: now, bytes: 0.0, percent: 0.0, moving: false, dead: false });
                }
            }
        }
        thread::sleep(Duration::from_secs(1));

        let mut users: Vec<String> = tries.iter().filter(|t| !t.dead).map(|t| t.user.clone()).collect();
        users.sort();
        users.dedup();
        let mut winner: Option<usize> = None;
        for user in users {
            let list = transfers(sh, &user);
            for (i, t) in tries.iter_mut().enumerate().filter(|(_, t)| !t.dead && t.user == user) {
                let rec = list.as_ref().and_then(|l| l.iter().find(|r| r.0 == t.file));
                match rec {
                    Some((_, tid, state, bytes, pct)) => {
                        t.id = Some(tid.clone());
                        if state.contains("Succeeded") {
                            winner = Some(i);
                        } else if state.starts_with("Completed") {
                            t.dead = true;
                        } else {
                            if *bytes > t.bytes {
                                t.bytes = *bytes;
                                t.last_move = Instant::now();
                                t.moving = true;
                            }
                            t.percent = *pct;
                            let quiet = t.last_move.elapsed();
                            if (t.moving && quiet > STALL) || (!t.moving && t.asked.elapsed() > QUEUE_WAIT) {
                                cancel(sh, &t.user, &t.id);
                                t.dead = true;
                            }
                        }
                    }
                    None => {
                        if t.asked.elapsed() > QUEUE_WAIT {
                            t.dead = true;
                        }
                    }
                }
            }
        }
        if let Some(w) = winner {
            for (i, t) in tries.iter().enumerate() {
                if i != w && !t.dead {
                    cancel(sh, &t.user, &t.id);
                }
            }
            let (_, file_title, _) = names::parse_name(&tries[w].file);
            label(sh, id, "Downloaded", Some(100));
            return Ok(Check::Song { title: if file_title.is_empty() { title } else { file_title }, artist });
        }
        if let Some(best) = tries.iter().filter(|t| t.moving && !t.dead).map(|t| t.percent).fold(None, |m: Option<f64>, p| Some(m.map_or(p, |m| m.max(p)))) {
            for t in tries.iter_mut().filter(|t| !t.moving && !t.dead) {
                cancel(sh, &t.user, &t.id);
                t.dead = true;
            }
            label(sh, id, "Downloading from Soulseek", Some(best as i64));
            continue;
        }
        let exhausted = next >= sources.len() && tries.iter().all(|t| t.dead);
        if started.elapsed() > YOUTUBE_AFTER || exhausted {
            for t in tries.iter().filter(|t| !t.dead) {
                cancel(sh, &t.user, &t.id);
            }
            log(&format!("job {id}: no peer started, YouTube"));
            let target = format!("ytsearch1:{} {}", safe_text(&artist), safe_text(&title));
            return youtube_fetch(sh, id, &target, Some((artist.as_str(), title.as_str())));
        }
        label(sh, id, "Waiting for someone to answer", None);
    }
}

fn album_job(sh: &Shared, id: u64, res: &Value) -> Result<Check, String> {
    let user = res["user"].as_str().unwrap_or("").to_string();
    let files: Vec<(String, f64)> = res["files"]
        .as_array()
        .map(|a| a.iter().map(|f| (f["filename"].as_str().unwrap_or("").to_string(), f["size"].as_f64().unwrap_or(0.0))).collect())
        .unwrap_or_default();
    if user.is_empty() || files.is_empty() {
        return Err("That album has no songs we can use.".into());
    }
    let body: Vec<Value> = files.iter().map(|(f, s)| json!({ "filename": f, "size": *s as i64 })).collect();
    sh.slskd
        .api("POST", &format!("/transfers/downloads/{}", enc(&user)), Some(&Value::Array(body)))
        .map_err(|e| peers_error(&e))?;
    let total: f64 = files.iter().map(|f| f.1).sum::<f64>().max(1.0);
    let n = files.len();
    let mut last_bytes = 0.0;
    let mut last_move = Instant::now();
    label(sh, id, "Waiting in their line", Some(0));
    loop {
        thread::sleep(Duration::from_secs(2));
        let Some(list) = transfers(sh, &user) else { continue };
        let mine: Vec<&(String, String, String, f64, f64)> = list.iter().filter(|r| files.iter().any(|f| f.0 == r.0)).collect();
        let ok = mine.iter().filter(|r| r.2.contains("Succeeded")).count();
        let bad = mine.iter().filter(|r| r.2.starts_with("Completed") && !r.2.contains("Succeeded")).count();
        let bytes: f64 = mine.iter().map(|r| if r.2.contains("Succeeded") { files.iter().find(|f| f.0 == r.0).map(|f| f.1).unwrap_or(r.3) } else { r.3 }).sum();
        if bytes > last_bytes {
            last_bytes = bytes;
            last_move = Instant::now();
        }
        if ok + bad >= n {
            if ok == 0 {
                return Err("That person stopped sharing it. Try another copy.".into());
            }
            label(sh, id, &format!("Downloaded {ok} of {n} songs"), Some(100));
            break;
        }
        if last_move.elapsed() > ALBUM_STALL {
            for r in mine.iter().filter(|r| !r.2.starts_with("Completed")) {
                cancel(sh, &user, &Some(r.1.clone()));
            }
            if ok == 0 {
                return Err("That person never started sending. Try another copy.".into());
            }
            label(sh, id, &format!("Got {ok} of {n} songs, the rest stalled"), Some(100));
            break;
        }
        let text = if bytes == 0.0 { "Waiting in their line".to_string() } else { format!("{ok} of {n} songs") };
        label(sh, id, &text, Some((bytes * 100.0 / total) as i64));
    }
    Ok(Check::Album { title: res["title"].as_str().unwrap_or("").to_string() })
}

fn youtube_fetch(sh: &Shared, id: u64, target: &str, tags: Option<(&str, &str)>) -> Result<Check, String> {
    upd(sh, id, |j| {
        j.source = "YouTube".into();
        j.label = "Getting it from YouTube".into();
        j.percent = 0;
    });
    let mut args: Vec<String> = [
        "--no-playlist", "--no-warnings", "--newline", "--progress", "--encoding", "utf-8", "--no-mtime",
        "--progress-template", "download:pct %(progress._percent_str)s",
        "--print", "after_move:path %(filepath)s",
        "-f", "bestaudio[ext=m4a]/bestaudio", "-x", "--audio-format", "m4a", "--embed-metadata",
        "--parse-metadata", "uploader:(?P<uploader>.+?)(?: - Topic)?$",
        "-P", YT_FOLDER, "-P",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    args.push(format!("temp:{YT_TEMP}"));
    args.push("-o".into());
    args.push("%(meta_artist,artist,uploader)s - %(meta_title,track,title)s.%(ext)s".into());
    if let Some((artist, title)) = tags {
        args.extend(names::tag_args(artist, title));
    }
    args.push(target.to_string());
    let script = format!(
        "$ff=(Get-ChildItem \"$env:LOCALAPPDATA\\Microsoft\\WinGet\\Packages\\Gyan.FFmpeg*\\*\\bin\\ffmpeg.exe\" -ErrorAction SilentlyContinue | Select-Object -First 1).DirectoryName; {} --ffmpeg-location $ff",
        yt_args(&args)
    );
    let mut child = ssh_ps(&script).map_err(|_| "Could not reach the tower.".to_string())?;
    let lines = Arc::new(Mutex::new(Vec::<String>::new()));
    let reader = child.stdout.take().map(|out| {
        let lines = Arc::clone(&lines);
        thread::spawn(move || {
            for l in BufReader::new(out).lines().map_while(Result::ok) {
                lock(&lines).push(l.trim().to_string());
            }
        })
    });
    let last_err = Arc::new(Mutex::new(String::new()));
    if let Some(err) = child.stderr.take() {
        let last_err = Arc::clone(&last_err);
        thread::spawn(move || {
            for l in BufReader::new(err).lines().map_while(Result::ok) {
                if !l.trim().is_empty() {
                    *lock(&last_err) = l.trim().chars().take(300).collect();
                }
            }
        });
    }
    let started = Instant::now();
    let mut path: Option<String> = None;
    let mut read = |path: &mut Option<String>| {
        for l in std::mem::take(&mut *lock(&lines)) {
            if let Some(p) = l.strip_prefix("download:pct ") {
                if let Ok(n) = p.trim().trim_end_matches('%').trim().parse::<f64>() {
                    label(sh, id, "Getting it from YouTube", Some((n as i64).min(99)));
                }
            } else if let Some(p) = l.strip_prefix("path ") {
                *path = Some(p.trim().to_string());
            }
        }
    };
    loop {
        thread::sleep(Duration::from_secs(1));
        read(&mut path);
        if child.try_wait().map(|s| s.is_some()).unwrap_or(true) {
            break;
        }
        if started.elapsed() > YT_LIMIT {
            let _ = child.kill();
            return Err("YouTube took too long.".into());
        }
    }
    if let Some(r) = reader {
        let _ = r.join();
    }
    read(&mut path);
    let Some(path) = path else {
        log(&format!("job {id}: youtube gave nothing: {}", lock(&last_err)));
        return Err("YouTube had nothing for this one.".into());
    };
    let stem = path.rsplit('\\').next().unwrap_or(&path);
    let stem = stem.rsplit_once('.').map(|(s, _)| s).unwrap_or(stem);
    let (artist, title) = match stem.split_once(" - ") {
        Some((a, t)) => (a.trim().to_string(), t.trim().to_string()),
        None => (String::new(), stem.trim().to_string()),
    };
    label(sh, id, "Downloaded", Some(100));
    Ok(Check::Song { title, artist })
}

fn found_in_library(check: &Check) -> bool {
    let (query, kind) = match check {
        Check::Song { title, .. } => (title.clone(), "song"),
        Check::Album { title } => (title.clone(), "album"),
    };
    let q = names::normalize(&query, true);
    if q.is_empty() {
        return false;
    }
    let Ok(r) = nav("search3", &[("query", &q), ("songCount", "50"), ("albumCount", "20"), ("artistCount", "0")]) else {
        return false;
    };
    let field = if kind == "song" { "title" } else { "name" };
    let artist_want = match check {
        Check::Song { artist, .. } => names::normalize(artist, true),
        _ => String::new(),
    };
    r["searchResult3"][kind]
        .as_array()
        .map(|list| {
            list.iter().any(|s| {
                let t = names::normalize(s[field].as_str().unwrap_or(""), true);
                let a = names::normalize(s["artist"].as_str().unwrap_or(""), true);
                let title_ok = !t.is_empty() && (t.contains(&q) || q.contains(&t));
                let artist_ok = artist_want.is_empty() || a.is_empty() || a.contains(&artist_want) || artist_want.contains(&a) || t.contains(&artist_want);
                title_ok && artist_ok
            })
        })
        .unwrap_or(false)
}

fn wait_ready(sh: &Shared, id: u64, check: Check) {
    label(sh, id, "Adding to your library", Some(100));
    let started = Instant::now();
    let _ = nav("startScan", &[]);
    let mut kicked = Instant::now();
    loop {
        thread::sleep(Duration::from_secs(4));
        if found_in_library(&check) {
            log(&format!("job {id}: ready after {} s", started.elapsed().as_secs()));
            upd(sh, id, |j| {
                j.state = "ready";
                j.label = "Ready in Amperfy".into();
            });
            return;
        }
        if kicked.elapsed() > Duration::from_secs(60) {
            let _ = nav("startScan", &[]);
            kicked = Instant::now();
        }
        if started.elapsed() > READY_WAIT {
            log(&format!("job {id}: saved, library has not shown it yet"));
            upd(sh, id, |j| {
                j.state = "ready";
                j.label = "Saved. Amperfy shows it after the next library check.".into();
            });
            return;
        }
    }
}

// ---------------------------------------------------------------------------
// HTTP
// ---------------------------------------------------------------------------

fn header(name: &str, value: &str) -> tiny_http::Header {
    tiny_http::Header::from_bytes(name.as_bytes(), value.as_bytes()).expect("valid header")
}

fn respond(rq: tiny_http::Request, code: u16, kind: &str, body: String, cache: &str) {
    let resp = tiny_http::Response::from_string(body)
        .with_status_code(code)
        .with_header(header("Content-Type", kind))
        .with_header(header("Cache-Control", cache))
        .with_header(header("X-Content-Type-Options", "nosniff"))
        .with_header(header("Referrer-Policy", "no-referrer"));
    let _ = rq.respond(resp);
}

fn param<'a>(qs: &'a str, name: &str) -> &'a str {
    qs.split('&').find_map(|kv| kv.split_once('=').filter(|(k, _)| *k == name).map(|(_, v)| v)).unwrap_or("")
}

fn handle(mut rq: tiny_http::Request, sh: Arc<Shared>) {
    let method = rq.method().as_str().to_string();
    let url = rq.url().to_string();
    let (path, qs) = url.split_once('?').unwrap_or((url.as_str(), ""));
    let mut body = Value::Null;
    if method == "POST" {
        let json_type = rq.headers().iter().any(|h| h.field.equiv("Content-Type") && h.value.as_str().starts_with("application/json"));
        if !json_type {
            return respond(rq, 415, "text/plain", "JSON only".into(), "no-store");
        }
        let mut text = String::new();
        let _ = rq.as_reader().take(65_536).read_to_string(&mut text);
        body = serde_json::from_str(&text).unwrap_or(Value::Null);
    }
    let reply = match (method.as_str(), path) {
        ("GET", "/") | ("GET", "/index.html") => {
            return respond(rq, 200, "text/html; charset=utf-8", PAGE.to_string(), "no-cache");
        }
        ("GET", "/api/status") => json!({ "ok": true, "tower": tower_up(&sh), "waking": waking(&sh) }),
        ("POST", "/api/search") => {
            let q = body["q"].as_str().unwrap_or("").trim().chars().take(120).collect::<String>();
            if q.chars().count() < MIN_QUERY {
                json!({ "ok": false, "error": "Type a few more letters." })
            } else {
                json!({ "ok": true, "id": start_search(&sh, &q) })
            }
        }
        ("GET", "/api/search") => snapshot(&sh, param(qs, "id").parse().unwrap_or(0)),
        ("POST", "/api/add") => add(&sh, body["search"].as_u64().unwrap_or(0), body["result"].as_str().unwrap_or("")),
        ("POST", "/api/retry") => retry(&sh, body["job"].as_u64().unwrap_or(0)),
        ("POST", "/api/dismiss") => {
            let ids: HashSet<u64> = body["jobs"].as_array().map(|a| a.iter().filter_map(|v| v.as_u64()).collect()).unwrap_or_default();
            lock(&sh.jobs).retain(|j| j.state == "working" || !ids.contains(&j.id));
            json!({ "ok": true })
        }
        ("GET", "/api/jobs") => {
            let list: Vec<Value> = lock(&sh.jobs).iter().rev().map(|j| j.to_json()).collect();
            json!({ "ok": true, "jobs": list })
        }
        ("GET", "/health") => json!({ "ok": true }),
        _ => return respond(rq, 404, "text/plain", "Not here".into(), "no-store"),
    };
    respond(rq, 200, "application/json; charset=utf-8", reply.to_string(), "no-store");
}

fn main() {
    let server = match tiny_http::Server::http(LISTEN) {
        Ok(s) => s,
        Err(e) => {
            log(&format!("could not listen on {LISTEN}: {e}"));
            return;
        }
    };
    log(&format!("listening on {LISTEN}"));
    let sh = Arc::new(Shared {
        searches: Mutex::new(vec![]),
        jobs: Mutex::new(vec![]),
        slskd: Slskd::new(),
        next: AtomicU64::new(1),
        tower: Mutex::new((Instant::now() - Duration::from_secs(60), false)),
        woke: Mutex::new(None),
    });
    for rq in server.incoming_requests() {
        let sh = Arc::clone(&sh);
        thread::spawn(move || handle(rq, sh));
    }
}
