//! Name parsing and ranking helpers, ported from the desktop player's
//! src/net/mod.rs and src/online.rs so the phone ranks results the same way.

use regex::Regex;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::Path;
use std::sync::OnceLock;

/// Formats the iPhone plays. Opus, Ogg, WMA, APE and WavPack are left out:
/// Amperfy downloads the original file and the phone cannot play those.
pub const PHONE_AUDIO: &[&str] = &["flac", "mp3", "m4a", "aac", "wav", "aiff", "aif", "alac"];
pub const LOSSLESS: &[&str] = &["flac", "wav", "aiff", "aif", "alac"];

fn re(cell: &'static OnceLock<Regex>, pattern: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pattern).expect("fixed pattern"))
}

fn ext_of(filename: &str) -> Option<String> {
    let name = filename.rsplit('\\').next().unwrap_or(filename);
    Some(Path::new(name).extension()?.to_str()?.to_ascii_lowercase())
}

pub fn is_phone_audio(filename: &str) -> bool {
    ext_of(filename).is_some_and(|e| PHONE_AUDIO.contains(&e.as_str()))
}

pub fn is_lossless(filename: &str) -> bool {
    ext_of(filename).is_some_and(|e| LOSSLESS.contains(&e.as_str()))
}

/// Lowercase, accents stripped, anything not a-z0-9 to spaces. With for_key,
/// bracketed bits like [FLAC] or (Remastered) drop out first.
pub fn normalize(text: &str, for_key: bool) -> String {
    static BRACKETS: OnceLock<Regex> = OnceLock::new();
    let mut s = text.to_lowercase();
    if for_key {
        s = re(&BRACKETS, r"[\[\(][^\]\)]*[\]\)]").replace_all(&s, " ").to_string();
    }
    let folded: String = unicode_normalization::UnicodeNormalization::nfd(s.chars())
        .filter(|c| !(0x0300..=0x036f).contains(&(*c as u32)))
        .collect();
    folded
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<&str>>()
        .join(" ")
}

/// "artist - title" from the file name, album from the folder.
pub fn parse_name(filename: &str) -> (String, String, String) {
    static PREFIX: OnceLock<Regex> = OnceLock::new();
    let mut parts: Vec<&str> = filename.split('\\').filter(|p| !p.is_empty()).collect();
    let file = parts.pop().unwrap_or("");
    let (f_artist, f_album) = parse_folder(&parts);
    let stem = Path::new(file).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let base = stem.replace('_', " ");
    let base = re(&PREFIX, r"(?i)^\s*(cd\s*\d+\s*[-.]?\s*)?\d{1,3}\s*[-.)\]]?\s*")
        .replace(&base, "")
        .trim()
        .to_string();
    let pieces: Vec<String> = base
        .split(" - ")
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty() && !p.chars().all(|c| c.is_ascii_digit()))
        .collect();
    if pieces.len() >= 2 {
        return (pieces[0].clone(), pieces[pieces.len() - 1].clone(), f_album);
    }
    (f_artist, pieces.first().cloned().unwrap_or(base), f_album)
}

/// Artist and album out of the peer's folder path.
pub fn parse_folder(parts: &[&str]) -> (String, String) {
    static TAGS: OnceLock<Regex> = OnceLock::new();
    static CD: OnceLock<Regex> = OnceLock::new();
    static YEAR: OnceLock<Regex> = OnceLock::new();
    static GENERIC: OnceLock<Regex> = OnceLock::new();
    let clean = |s: &str| -> String {
        re(&TAGS, r"(?i)[\[\(][^\]\)]*(flac|mp3|320|v0|web|cd|24|16|bit|khz)[^\]\)]*[\]\)]")
            .replace_all(s, "")
            .replace('_', " ")
            .trim()
            .to_string()
    };
    let mut work: Vec<&str> = parts.to_vec();
    let mut album_folder = clean(work.last().copied().unwrap_or(""));
    if re(&CD, r"(?i)^(cd|disc|disk)\s*\d+$").is_match(&album_folder) && work.len() > 1 {
        album_folder = clean(work[work.len() - 2]);
        work.pop();
    }
    let year = re(&YEAR, r"^\(?\d{4}\)?\s*-?\s*");
    let pieces: Vec<&str> = album_folder.split(" - ").filter(|p| !p.is_empty()).collect();
    if pieces.len() >= 2 {
        let album = year.replace(&pieces[1..].join(" - "), "").trim().to_string();
        let album = if album.is_empty() { pieces[1].to_string() } else { album };
        return (pieces[0].to_string(), album);
    }
    let artist_folder = if work.len() > 1 { clean(work[work.len() - 2]) } else { String::new() };
    let generic = re(&GENERIC, r"(?i)^(music|mp3|flac|albums?|downloads?|shared?|complete|soulseek|[a-z]:)$");
    let artist = if generic.is_match(&artist_folder) { String::new() } else { artist_folder };
    (artist, year.replace(&album_folder, "").trim().to_string())
}

/// "FLAC 16/44", "MP3 320" or just the format name.
pub fn quality_text(extension: &str, raw: &Value) -> String {
    let name = extension.to_uppercase();
    if let (Some(d), Some(r)) = (raw["bitDepth"].as_i64(), raw["sampleRate"].as_i64()) {
        return format!("{name} {d}/{}", (r as f64 / 1000.0).round() as i64);
    }
    if let Some(br) = raw["bitRate"].as_i64() {
        return format!("{name} {br}");
    }
    name
}

pub fn duration_text(seconds: Option<f64>) -> String {
    let Some(s) = seconds.filter(|s| *s > 0.0) else { return String::new() };
    let s = s.round() as i64;
    format!("{}:{:02}", s / 60, s % 60)
}

fn most_common(values: &[String]) -> String {
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for v in values {
        *counts.entry(v.as_str()).or_insert(0) += 1;
    }
    counts.into_iter().max_by_key(|(_, n)| *n).map(|(v, _)| v.to_string()).unwrap_or_default()
}

/// yt-dlp tag arguments: "@value" is plain text, % and : escaped.
pub fn tag_args(artist: &str, title: &str) -> Vec<String> {
    let mut args = Vec::new();
    for (field, text) in [("artist", artist), ("title", title)] {
        let text = text.split_whitespace().collect::<Vec<&str>>().join(" ");
        if !text.is_empty() {
            args.push("--parse-metadata".into());
            args.push(format!("@{}:@(?P<meta_{field}>.+)", text.replace('%', "%%").replace(':', "\\:")));
        }
    }
    args
}

/// Rough seconds until the file is here; a busy peer costs a long wait.
pub fn source_cost(s: &Value) -> f64 {
    let size = s["size"].as_f64().unwrap_or(0.0);
    let speed = s["speed"].as_f64().unwrap_or(0.0).max(50_000.0);
    let seconds = size / speed;
    let waiting = if s["freeSlot"].as_bool().unwrap_or(false) {
        0.0
    } else {
        60.0 + s["queueLength"].as_i64().unwrap_or(0) as f64 * 20.0
    };
    let bonus = if s["lossless"].as_bool().unwrap_or(false) && seconds < 15.0 { -3.0 } else { 0.0 };
    seconds + waiting + bonus
}

fn album_rank(free_slot: bool, lossless: bool, files: usize) -> f64 {
    free_slot as i32 as f64 * 100.0 + lossless as i32 as f64 * 10.0 + (files.min(30) as f64) / 10.0
}

/// slskd responses into songs (merged across peers, ranked) and albums
/// (one folder with two or more songs from one peer).
pub fn build_results(responses: &[Value], query: &str) -> (Vec<Value>, Vec<Value>) {
    let mut songs: HashMap<String, Value> = HashMap::new();
    let mut albums: HashMap<String, Value> = HashMap::new();
    let words: Vec<String> = normalize(query, false).split(' ').filter(|w| !w.is_empty()).map(String::from).collect();

    for response in responses {
        let username = response["username"].as_str().unwrap_or("");
        let free_slot = response["hasFreeUploadSlot"].as_bool().unwrap_or(false);
        let queue_len = response["queueLength"].as_i64().unwrap_or(0);
        let speed = response["uploadSpeed"].as_f64().unwrap_or(0.0);
        let mut folders: HashMap<String, Vec<Value>> = HashMap::new();

        for raw in response["files"].as_array().map(|a| a.as_slice()).unwrap_or(&[]) {
            let filename = raw["filename"].as_str().unwrap_or("").to_string();
            if !is_phone_audio(&filename) {
                continue;
            }
            let ext = ext_of(&filename).unwrap_or_default();
            let folder = filename.rsplit_once('\\').map(|(f, _)| f.to_string()).unwrap_or_default();
            let lossless = is_lossless(&filename);
            let quality = quality_text(&ext, raw);
            let source = json!({
                "username": username,
                "filename": filename,
                "size": raw["size"].as_f64().unwrap_or(0.0),
                "freeSlot": free_slot,
                "queueLength": queue_len,
                "speed": speed,
                "lossless": lossless,
                "quality": quality,
            });
            folders.entry(folder.clone()).or_default().push(source.clone());

            let (mut f_artist, mut f_title, f_album) = parse_name(&filename);
            // "(2018)" or "2014-12-29" is a date from the folder, not an artist.
            if f_artist.chars().all(|c| c.is_ascii_digit() || "()[]-./ ".contains(c)) {
                f_artist.clear();
            }
            // Some files are named "Title - Artist": if only the artist side matches the search, swap.
            let hits = |s: &str| { let n = normalize(s, false); n.split(' ').filter(|w| w.len() > 1 && words.iter().any(|q| q == w)).count() };
            if !f_artist.is_empty() && hits(&f_title) == 0 && hits(&f_artist) > 0 {
                std::mem::swap(&mut f_artist, &mut f_title);
            }
            // "27 Song" from "Artist - 27 Song": drop a track number the search did not ask for.
            if let Some((num, rest)) = f_title.split_once(' ') {
                if num.len() <= 3 && num.chars().all(|c| c.is_ascii_digit()) && !rest.trim().is_empty() && !words.iter().any(|w| w == num) {
                    f_title = rest.trim().to_string();
                }
            }
            let key = format!("{}|{}", normalize(&f_artist, true), normalize(&f_title, true));
            let dur = raw["length"].as_f64();
            let song = songs.entry(key.clone()).or_insert_with(|| {
                json!({
                    "resultId": format!("song|{key}"),
                    "title": f_title,
                    "artist": f_artist,
                    "album": f_album,
                    "duration": duration_text(dur),
                    "lossless": lossless,
                    "quality": quality,
                    "sources": [],
                })
            });
            if song["duration"].as_str().unwrap_or("").is_empty() {
                song["duration"] = json!(duration_text(dur));
            }
            let sources = song["sources"].as_array_mut().expect("sources array");
            if !sources.iter().any(|s| s["username"].as_str() == Some(username)) {
                sources.push(source);
            }
        }

        for (folder, mut files) in folders {
            if files.len() < 2 {
                continue;
            }
            files.sort_by(|a, b| a["filename"].as_str().unwrap_or("").cmp(b["filename"].as_str().unwrap_or("")));
            let parts: Vec<&str> = folder.split('\\').filter(|p| !p.is_empty()).collect();
            let (artist, album) = parse_folder(&parts);
            if album.is_empty() {
                continue;
            }
            let key = format!("{}|{}", normalize(&artist, true), normalize(&album, true));
            let lossless = files.iter().all(|f| f["lossless"].as_bool().unwrap_or(false));
            let quality = most_common(&files.iter().filter_map(|f| f["quality"].as_str().map(String::from)).collect::<Vec<_>>());
            let rank = album_rank(free_slot, lossless, files.len());
            let candidate = json!({
                "resultId": format!("album|{username}|{folder}"),
                "title": album,
                "artist": artist,
                "user": username,
                "freeSlot": free_slot,
                "lossless": lossless,
                "quality": quality,
                "folder": folder,
                "files": files,
                "rank": rank,
                "copies": 1,
            });
            match albums.get_mut(&key) {
                None => {
                    albums.insert(key, candidate);
                }
                Some(existing) => {
                    let copies = existing["copies"].as_i64().unwrap_or(1) + 1;
                    if rank > existing["rank"].as_f64().unwrap_or(0.0) {
                        let mut candidate = candidate;
                        candidate["copies"] = json!(copies);
                        albums.insert(key, candidate);
                    } else {
                        existing["copies"] = json!(copies);
                    }
                }
            }
        }
    }

    let mut song_list: Vec<Value> = songs.into_values().filter(|s| !s["title"].as_str().unwrap_or("").is_empty()).collect();
    for song in song_list.iter_mut() {
        let (quality, lossless, count, any_free) = {
            let sources = song["sources"].as_array_mut().expect("sources array");
            sources.sort_by(|a, b| source_cost(a).partial_cmp(&source_cost(b)).unwrap_or(std::cmp::Ordering::Equal));
            let first = sources.first();
            (
                first.map(|f| f["quality"].clone()).unwrap_or(Value::Null),
                first.map(|f| f["lossless"].clone()).unwrap_or(Value::Null),
                sources.len(),
                sources.iter().any(|s| s["freeSlot"].as_bool().unwrap_or(false)),
            )
        };
        song["quality"] = quality;
        song["lossless"] = lossless;
        let own = normalize(&format!("{} {}", song["artist"].as_str().unwrap_or(""), song["title"].as_str().unwrap_or("")), false);
        let matched = words.iter().filter(|w| own.contains(w.as_str())).count();
        let score = if words.is_empty() { 0.0 } else { matched as f64 / words.len() as f64 } * 10.0
            + (1.0 + count as f64).log2() * 3.0
            + if any_free { 2.0 } else { 0.0 };
        song["score"] = json!(score);
    }
    song_list.sort_by(|a, b| {
        b["score"].as_f64().unwrap_or(0.0).partial_cmp(&a["score"].as_f64().unwrap_or(0.0)).unwrap_or(std::cmp::Ordering::Equal)
    });
    song_list.truncate(12);

    let mut album_list: Vec<Value> = albums.into_values().collect();
    album_list.sort_by(|a, b| {
        let ca = a["copies"].as_i64().unwrap_or(1);
        let cb = b["copies"].as_i64().unwrap_or(1);
        cb.cmp(&ca).then_with(|| {
            b["rank"].as_f64().unwrap_or(0.0).partial_cmp(&a["rank"].as_f64().unwrap_or(0.0)).unwrap_or(std::cmp::Ordering::Equal)
        })
    });
    album_list.truncate(6);
    (song_list, album_list)
}

/// One yt-dlp --dump-json line into a result, or None for mixes and shorts.
pub fn youtube_song(line: &str) -> Option<Value> {
    let v: Value = serde_json::from_str(line.trim()).ok()?;
    let id = v["id"].as_str()?.to_string();
    if id.len() != 11 || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return None;
    }
    let dur = v["duration"].as_f64();
    if let Some(d) = dur {
        if !(30.0..=900.0).contains(&d) {
            return None;
        }
    }
    let channel = v["channel"].as_str().or_else(|| v["uploader"].as_str()).unwrap_or("").trim_end_matches("- Topic").trim().to_string();
    Some(json!({
        "resultId": format!("video|{id}"),
        "title": v["title"].as_str().unwrap_or(&id),
        "artist": channel,
        "duration": duration_text(dur),
        "youtubeUrl": format!("https://www.youtube.com/watch?v={id}"),
    }))
}
