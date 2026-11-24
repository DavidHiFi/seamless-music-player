//! One-time import from Dopamine Enhanced: a copy of its SQLite database and
//! config, artwork and artist images from its cache, the play queue, the
//! settings that still mean something, the m3u playlists and the ratings
//! backup. Runs once (a done marker), is safe to run twice (idempotent
//! writes), and writes reports/import-report.md with counts in vs out.
//! The scrobbling secrets move into settings only; they are never printed,
//! logged or written into the report.

use crate::library::Library;
use crate::logic;
use serde_json::{json, Map, Value};
use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::{Path, PathBuf};

// ---- paths ----

/// The Dopamine Enhanced data folder. SMP_DOPAMINE_DIR overrides it for
/// tests; the real folder is only ever read, never written.
fn dop_dir() -> PathBuf {
    if let Ok(d) = std::env::var("SMP_DOPAMINE_DIR") {
        return PathBuf::from(d);
    }
    dirs::data_dir()
        .unwrap_or_default()
        .join("Dopamine Enhanced")
}

fn done_marker_path() -> PathBuf {
    crate::settings::app_dir().join("import-done.txt")
}

fn report_path() -> PathBuf {
    // The project folder when the exe was built from it, else the app data
    // folder. The orchestrator reads the project copy.
    let project = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("reports");
    if project.is_dir() {
        return project.join("import-report.md");
    }
    crate::settings::app_dir().join("import-report.md")
}

// ---- dates ----

/// Dopamine stores dates as .NET ticks (100 ns since year 1) in the current
/// database; older values may arrive as ms or s since the Unix epoch.
/// Check the size and convert accordingly. Returns Unix seconds.
pub fn dop_date_to_secs(v: i64) -> Option<i64> {
    if v <= 0 {
        return None;
    }
    if v >= 1_000_000_000_000_000 {
        // .NET ticks: 10,000,000 per second, epoch offset 62135596800 s.
        let secs = (v - 621_355_968_000_000_000) / 10_000_000;
        (secs > 0).then_some(secs)
    } else if v >= 100_000_000_000 {
        Some(v / 1000) // milliseconds
    } else if v >= 100_000_000 {
        Some(v) // seconds
    } else {
        None
    }
}

fn file_mtime(path: &str) -> i64 {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

// ---- the one-time import, on first start ----

/// The import itself, on a background thread at startup. Copies the database
/// to a temp file first; the original is never opened. A done marker keeps
/// it from running twice.
pub fn maybe_import(app: &crate::App) {
    if done_marker_path().exists() {
        return;
    }
    let db = dop_dir().join("Dopamine.db");
    if !db.exists() {
        return;
    }
    let config = dop_dir().join("config.json");
    let tmp = std::env::temp_dir().join(format!("smp-import-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&tmp);
    if std::fs::copy(&db, &tmp).is_err() {
        // Dopamine may hold the file; one quiet retry after a moment.
        std::thread::sleep(std::time::Duration::from_millis(1500));
        if std::fs::copy(&db, &tmp).is_err() {
            return;
        }
    }
    // Write-ahead sidecars, when Dopamine was running: copy them too so the
    // temp copy opens complete.
    for ext in ["-wal", "-shm"] {
        let src = db.with_extension(format!("db{ext}"));
        let dst = tmp.with_extension(format!("db{ext}"));
        if src.exists() {
            let _ = std::fs::copy(&src, &dst);
        }
    }

    let report = run_import(
        &tmp,
        if config.exists() { Some(config.as_path()) } else { None },
        &app.lib,
    );

    // Queue and position, from the report into the remembered player state.
    if let Ok(rep) = &report {
        apply_queue(app, rep);
        apply_imported_settings(app, rep);
    }

    write_report(&report);
    let _ = std::fs::remove_file(&tmp);
    let _ = std::fs::remove_file(tmp.with_extension("db-wal"));
    let _ = std::fs::remove_file(tmp.with_extension("db-shm"));

    // The done marker: counts only, never secrets.
    let (ti, to, li, lo, pi, po) = match &report {
        Ok(r) => (
            r["counts"]["tracksIn"].as_i64().unwrap_or(0),
            r["counts"]["tracksOut"].as_i64().unwrap_or(0),
            r["counts"]["lovedIn"].as_i64().unwrap_or(0),
            r["counts"]["lovedOut"].as_i64().unwrap_or(0),
            r["counts"]["playsIn"].as_i64().unwrap_or(0),
            r["counts"]["playsOut"].as_i64().unwrap_or(0),
        ),
        Err(_) => (0, 0, 0, 0, 0, 0),
    };
    let marker = format!(
        "Dopamine import done {} UTC\ntracks in {} out {}\nloved in {} out {}\nplays in {} out {}\n",
        crate::library::now_secs(),
        ti, to, li, lo, pi, po
    );
    let _ = std::fs::write(done_marker_path(), marker);
}

/// Map the imported queue (paths in play order) onto remembered player state.
fn apply_queue(app: &crate::App, rep: &Value) {
    let queue = match rep["queue"].as_array() {
        Some(q) if !q.is_empty() => q,
        _ => return,
    };
    let mut ids = Vec::new();
    let mut index = 0usize;
    let mut position = 0.0f64;
    let mut was_playing = false;
    for item in queue {
        let path = item["path"].as_str().unwrap_or("");
        let id = id_by_path(&app.lib, path);
        if let Some(id) = id {
            if item["playing"].as_bool().unwrap_or(false) {
                index = ids.len();
                position = item["progress"].as_f64().unwrap_or(0.0);
                was_playing = true;
            }
            ids.push(id);
        }
    }
    if ids.is_empty() {
        return;
    }
    let track_id = ids.get(index).copied();
    app.settings.update(|s| {
        s.last_queue = ids.clone();
        s.last_index = index.min(ids.len().saturating_sub(1));
        s.last_position = position;
        s.last_track = track_id;
        s.was_playing = was_playing;
    });
}

fn id_by_path(lib: &Library, path: &str) -> Option<i64> {
    let conn = lib.conn.lock().unwrap();
    conn.query_row(
        "SELECT id FROM tracks WHERE path = ?1 COLLATE NOCASE",
        [path],
        |r| r.get(0),
    )
    .ok()
}

/// Settings that still mean something, read from config.json by run_import
/// and written here. The Last.fm password never moves; only the session key
/// does, and it is kept out of the serialized file's visible fields by
/// living in extra under lastfm_session (settings.rs skips serializing the
/// lastfm_key field).
fn apply_imported_settings(app: &crate::App, rep: &Value) {
    let imported = match rep["settings"].as_object() {
        Some(m) if !m.is_empty() => m,
        _ => return,
    };
    app.settings.update(|s| {
        for (key, value) in imported {
            let _ = s.set_key(key, value.clone());
        }
        // The session key goes to extra so it survives the save; set_key on
        // lastfm_key would drop it, because that field is not serialized.
        if let Some(v) = imported.get("lastfm_key") {
            s.extra.insert("lastfm_session".into(), v.clone());
        }
    });
}

// ---- the import proper ----

/// Import from a copy of Dopamine.db (never the original), plus config.json
/// when given. Writes into the library database, the artwork cache, the
/// playlists folder and the ratings, all idempotently. Returns the report.
pub fn run_import(db_copy: &Path, config: Option<&Path>, lib: &Library) -> Result<Value, String> {
    let conn = rusqlite::Connection::open(db_copy).map_err(|e| format!("opening the copy: {e}"))?;

    // ---- counts in ----
    let tracks_in: i64 = conn
        .query_row("SELECT COUNT(*) FROM Track", [], |r| r.get(0))
        .unwrap_or(0);
    let loved_in: i64 = conn
        .query_row("SELECT COUNT(*) FROM Track WHERE Love = 1", [], |r| r.get(0))
        .unwrap_or(0);
    let plays_in: i64 = conn
        .query_row("SELECT SUM(PlayCount) FROM Track", [], |r| r.get(0))
        .unwrap_or(0);

    // ---- folders ----
    let mut folders_out = 0i64;
    {
        let mut st = conn
            .prepare("SELECT Path FROM Folder")
            .map_err(|e| e.to_string())?;
        let paths: Vec<String> = st
            .query_map([], |r| r.get(0))
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect();
        let mut lc = lib.conn.lock().unwrap();
        for p in paths {
            if lc.execute("INSERT OR IGNORE INTO folders(path) VALUES (?1)", [p]).is_ok() {
                folders_out += 1;
            }
        }
    }

    // ---- album artwork: AlbumKey -> cache file ----
    let cache_dir = dop_dir().join("Cache");
    let mut album_art: HashMap<String, String> = HashMap::new();
    let mut artwork_copied = 0i64;
    {
        let mut st = conn
            .prepare("SELECT AlbumKey, ArtworkID FROM AlbumArtwork")
            .map_err(|e| e.to_string())?;
        let rows: Vec<(String, String)> = st
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect();
        let mut file_cache: HashMap<String, Option<String>> = HashMap::new();
        for (key, art_id) in rows {
            let file = cache_dir.join("CoverArt").join(format!("{art_id}.jpg"));
            let entry = file_cache.entry(art_id).or_insert_with(|| {
                import_artwork_file(&file).map(|id| {
                    artwork_copied += 1;
                    id
                })
            });
            if let Some(id) = entry {
                album_art.insert(key, id.clone());
            }
        }
    }

    // ---- tracks ----
    let mut st = conn
        .prepare(
            "SELECT Path, TrackTitle, FileName, Artists, AlbumArtists, AlbumTitle, AlbumKey, \
             Genres, Year, TrackNumber, DiscNumber, Duration, BitRate, SampleRate, Rating, \
             NewRating, Love, PlayCount, SkipCount, DateAdded, DateLastPlayed, DateFileModified \
             FROM Track",
        )
        .map_err(|e| e.to_string())?;
    let rows: Vec<(String, String, String, String, String, String, String, String, Option<i64>,
        Option<i64>, Option<i64>, f64, f64, i64, Option<i64>, Option<i64>, i64, i64, i64,
        Option<i64>, Option<i64>, Option<i64>)> = st
        .query_map([], |r| {
            Ok((
                r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?,
                r.get(7)?, r.get(8)?, r.get(9)?, r.get(10)?, r.get(11)?, r.get(12)?, r.get(13)?,
                r.get(14)?, r.get(15)?, r.get(16)?, r.get(17)?, r.get(18)?, r.get(19)?,
                r.get(20)?, r.get(21)?,
            ))
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();
    drop(st);

    let arr = |s: &str| -> String {
        serde_json::to_string(&logic::from_delimited_string(s)).unwrap_or_else(|_| "[]".into())
    };
    let rating_of = |rating: Option<i64>, new_rating: Option<i64>| -> i64 {
        // NewRating is a 0..10 half-star scale in Dopamine Enhanced; ours is
        // 0..5 stars. The old Rating column was already 0..5.
        if let Some(nr) = new_rating {
            return ((nr as f64 / 2.0).round() as i64).clamp(0, 5);
        }
        rating.unwrap_or(0).clamp(0, 5)
    };

    let mut inserted = 0i64;
    {
        let mut lc = lib.conn.lock().unwrap();
        for row in &rows {
            let (
                path, title, file_name, artists, album_artists, album, album_key_src, genres,
                year, track_no, disc_no, duration_ms, bitrate, sample_rate, rating, new_rating,
                love, play_count, skip_count, date_added, date_last_played, date_file_modified,
            ) = row;
            let fallback_title = Path::new(file_name)
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();
            let title = if title.trim().is_empty() { fallback_title } else { title.clone() };
            let artists_list = logic::from_delimited_string(artists);
            let album_artists_list = logic::from_delimited_string(album_artists);
            let album_key = if album_key_src.trim().is_empty() {
                logic::album_key(album, &album_artists_list)
            } else {
                album_key_src.clone()
            };
            let artwork_id = album_art.get(&album_key).cloned();
            let mtime = {
                let from_disk = file_mtime(path);
                if from_disk > 0 {
                    from_disk
                } else {
                    dop_date_to_secs(date_file_modified.unwrap_or(0)).unwrap_or(0)
                }
            };
            let done = lc
                .execute(
                    "INSERT OR IGNORE INTO tracks (path, title, artists, album_artists, album, \
                     album_key, genres, year, track_no, disc_no, duration, bitrate, sample_rate, \
                     rating, love, play_count, skip_count, date_added, last_played, artwork_id, \
                     mtime) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21)",
                    rusqlite::params![
                        path,
                        title,
                        arr(artists),
                        arr(album_artists),
                        album,
                        album_key,
                        arr(genres),
                        year,
                        track_no,
                        disc_no,
                        duration_ms / 1000.0,
                        *bitrate as i64,
                        sample_rate,
                        rating_of(*rating, *new_rating),
                        love,
                        play_count,
                        skip_count,
                        dop_date_to_secs(date_added.unwrap_or(0)),
                        dop_date_to_secs(date_last_played.unwrap_or(0)),
                        artwork_id,
                        mtime,
                    ],
                )
                .is_ok();
            if done {
                inserted += 1;
            }
        }
    }

    // ---- artist images: kept in the art folder, mapped in the kv table ----
    let mut artist_images = 0i64;
    {
        let mut st = conn
            .prepare("SELECT Artist, ArtworkID FROM ArtistArtwork")
            .map_err(|e| e.to_string())?;
        let rows: Vec<(String, String)> = st
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect();
        let mut lc = lib.conn.lock().unwrap();
        let mut seen: HashSet<String> = HashSet::new();
        for (artist, art_id) in rows {
            let key = artist.trim().to_lowercase();
            if key.is_empty() || !seen.insert(key.clone()) {
                continue;
            }
            let file = cache_dir.join("ArtistArt").join(format!("{art_id}.jpg"));
            if let Some(id) = import_artwork_file(&file) {
                let _ = lc.execute(
                    "INSERT OR REPLACE INTO kv(key, value) VALUES (?1, ?2)",
                    rusqlite::params![format!("artistimg|{key}"), id],
                );
                artist_images += 1;
            }
        }
    }

    // ---- play queue ----
    let mut queue: Vec<Value> = Vec::new();
    {
        let mut st = conn
            .prepare(
                "SELECT Path, IsPlaying, ProgressSeconds FROM QueuedTrack ORDER BY OrderID",
            )
            .map_err(|e| e.to_string())?;
        let rows: Vec<(String, i64, f64)> = st
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect();
        for (path, playing, progress) in rows {
            queue.push(json!({ "path": path, "playing": playing == 1, "progress": progress }));
        }
    }

    // ---- settings that still mean something ----
    let mut imported_settings = Map::new();
    if let Some(cfg) = config {
        import_settings_from_config(cfg, &mut imported_settings);
    }

    // ---- ratings backup merge ----
    let ratings_merged = merge_ratings_backup(lib);

    // ---- playlists ----
    let playlists = import_playlists();

    // ---- counts out, by matching paths ----
    let our: HashMap<String, (i64, i64)> = {
        let lc = lib.conn.lock().unwrap();
        let mut st = lc
            .prepare("SELECT path, love, play_count FROM tracks")
            .map_err(|e| e.to_string())?;
        let rows: Vec<(String, i64, i64)> = st
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect();
        rows.into_iter()
            .map(|(p, l, c)| (p.to_lowercase(), (l, c)))
            .collect()
    };
    let mut tracks_out = 0i64;
    let mut loved_out = 0i64;
    let mut plays_out = 0i64;
    for row in &rows {
        if let Some((love, plays)) = our.get(&row.0.to_lowercase()) {
            tracks_out += 1;
            if row.16 == 1 && *love == 1 {
                loved_out += 1;
            }
            plays_out += plays;
        }
    }

    let report = json!({
        "counts": {
            "tracksIn": tracks_in,
            "tracksOut": tracks_out,
            "lovedIn": loved_in,
            "lovedOut": loved_out,
            "playsIn": plays_in,
            "playsOut": plays_out,
        },
        "foldersIn": 0,
        "foldersOut": folders_out,
        "tracksInserted": inserted,
        "artworkCopied": artwork_copied,
        "artistImages": artist_images,
        "ratingsMerged": ratings_merged,
        "playlists": playlists,
        "queue": queue,
        "queueOut": queue.len(),
        "settingsImported": imported_settings.len(),
        "settings": Value::Object(imported_settings),
    });
    Ok(report)
}

/// Copy one artwork image into our artwork cache. Returns the id, or None
/// when the file is missing or a stub (some artist images are 1-byte).
fn import_artwork_file(file: &Path) -> Option<String> {
    let bytes = std::fs::read(file).ok()?;
    if bytes.len() < 1000 {
        return None;
    }
    crate::library::store_artwork(&bytes)
}

/// The m3u playlists, copied as they are (contents untouched), including
/// Liked.m3u. Under SMP_DATA_DIR (tests) they land in the sandbox app data
/// folder instead of the real Music folder.
fn import_playlists() -> Value {
    let src = dirs::audio_dir()
        .map(|d| d.join("Dopamine").join("Playlists"))
        .unwrap_or_default();
    let dst = if std::env::var_os("SMP_DATA_DIR").is_some() {
        crate::settings::app_dir().join("Playlists")
    } else {
        dirs::audio_dir()
            .map(|d| d.join("Seamless Music Player").join("Playlists"))
            .unwrap_or_else(|| crate::settings::app_dir().join("Playlists"))
    };
    let mut copied = 0i64;
    let mut names: Vec<String> = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&src) {
        let _ = std::fs::create_dir_all(&dst);
        for e in rd.flatten() {
            let p = e.path();
            let is_m3u = p
                .extension()
                .and_then(|s| s.to_str())
                .map(|s| s.eq_ignore_ascii_case("m3u") || s.eq_ignore_ascii_case("m3u8"))
                .unwrap_or(false);
            if !is_m3u {
                continue;
            }
            let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            let target = dst.join(&name);
            if !target.exists() {
                if std::fs::copy(&p, &target).is_ok() {
                    copied += 1;
                }
            } else {
                copied += 1; // already there: counts as carried over
            }
            names.push(name.trim_end_matches(".m3u").to_string());
        }
    }
    json!({ "in": names.len(), "out": copied, "names": names })
}

/// Merge Music\Dopamine\Ratings\ratings-backup.json: love is OR-ed in,
/// ratings only ever go up. Returns how many entries changed a track.
fn merge_ratings_backup(lib: &Library) -> i64 {
    let file = dirs::audio_dir()
        .map(|d| d.join("Dopamine").join("Ratings").join("ratings-backup.json"))
        .unwrap_or_default();
    let Ok(body) = std::fs::read_to_string(&file) else {
        return 0;
    };
    let Ok(v) = serde_json::from_str::<Value>(&body) else {
        return 0;
    };
    let Some(entries) = v["ratings"].as_array() else {
        return 0;
    };
    let mut applied = 0i64;
    let mut lc = lib.conn.lock().unwrap();
    for e in entries {
        let path = e["trackPath"].as_str().unwrap_or("");
        let love = e["love"].as_i64().unwrap_or(0);
        let rating = e["rating"].as_i64().unwrap_or(0);
        if path.is_empty() || (love == 0 && rating == 0) {
            continue;
        }
        let current: Option<(i64, i64)> = lc
            .query_row(
                "SELECT love, rating FROM tracks WHERE path = ?1 COLLATE NOCASE",
                [path],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .ok();
        if let Some((cur_love, cur_rating)) = current {
            let new_love = cur_love.max(love.min(1));
            let new_rating = cur_rating.max(rating.clamp(0, 5));
            if new_love != cur_love || new_rating != cur_rating {
                let _ = lc.execute(
                    "UPDATE tracks SET love = ?2, rating = ?3 WHERE path = ?1 COLLATE NOCASE",
                    rusqlite::params![path, new_love, new_rating],
                );
                applied += 1;
            }
        } else {
            // A track the library has not seen: remembered in kv so a later
            // scan can pick it up.
            let _ = lc.execute(
                "INSERT OR REPLACE INTO kv(key, value) VALUES (?1, ?2)",
                rusqlite::params![format!("rating|{}", path.to_lowercase()), json!({ "love": love, "rating": rating }).to_string()],
            );
        }
    }
    applied
}

/// config.json -> the settings that still mean something. Keys match
/// settings.rs names; unknown-but-kept keys go in extra by the caller.
fn import_settings_from_config(cfg: &Path, out: &mut Map<String, Value>) {
    let Ok(body) = std::fs::read_to_string(cfg) else { return };
    let Ok(v) = serde_json::from_str::<Value>(&body) else { return };
    let mut set = |k: &str, val: Value| {
        if !val.is_null() {
            out.insert(k.into(), val);
        }
    };
    set("volume", json!(v["volume"].as_f64().unwrap_or(0.5).clamp(0.0, 1.0)));
    set("device_id", v["preferredAudioOutputDeviceId"].clone());
    set("gapless", json!(v["useGaplessPlayback"].as_bool().unwrap_or(true)));
    let crossfade = if v["useCrossfade"].as_bool().unwrap_or(false) {
        v["crossfadeDuration"].as_f64().unwrap_or(0.0)
    } else {
        0.0
    };
    set("crossfade_seconds", json!(crossfade));
    set("eq_enabled", json!(v["audioEqualizerEnabled"].as_bool().unwrap_or(false)));
    let preset = v["audioEqualizerPreset"].as_str().unwrap_or("flat").to_string();
    set("eq_preset", json!(format!("{}{}", preset[..1].to_uppercase(), &preset[1..])));
    if let Some(gains) = v["audioEqualizerGains"].as_str() {
        let g: Vec<f64> = gains
            .split(';')
            .filter_map(|s| s.trim().parse::<f64>().ok())
            .collect();
        if g.len() == 10 {
            set("eq_gains", json!(g));
        }
    }
    let rg_on = v["useReplayGainNormalization"].as_bool().unwrap_or(false);
    let rg_mode = v["replayGainMode"].as_str().unwrap_or("track").to_string();
    set("replaygain", json!(if rg_on { rg_mode } else { "off".into() }));
    set("preamp_db", json!(v["replayGainPreAmp"].as_f64().unwrap_or(0.0)));
    set("font_family", v["fontFamily"].clone());
    set("font_weight", match &v["fontWeight"] { Value::Number(n) => json!(n.to_string()), Value::String(s) => json!(s), _ => Value::Null });
    set("blur_amount", json!(v["windowBlurAmount"].as_f64().unwrap_or(30.0)));
    set("blur_enabled", json!(v["useWindowBlur"].as_bool().unwrap_or(true)));
    set("scrobble_lastfm", json!(v["enableLastFmScrobbling"].as_bool().unwrap_or(false)));
    set("lastfm_user", v["lastFmUsername"].clone());
    // The session key, not the password. It lands in extra as lastfm_session
    // in apply_imported_settings; the value itself is never reported.
    set("lastfm_key", v["lastFmSessionKey"].clone());
    set("scrobble_listenbrainz", json!(v["enableListenbrainzScrobbling"].as_bool().unwrap_or(false)));
    set("listenbrainz_token", v["listenbrainzToken"].clone());
    set("shuffle", json!(v["playbackControlsShuffle"].as_bool().unwrap_or(false)));
    let loop_mode = match v["playbackControlsLoop"].as_i64().unwrap_or(0) {
        1 => "one",
        2 => "all",
        _ => "off",
    };
    set("loop_mode", json!(loop_mode));
    if let Some(cols) = v["tracksPageVisibleColumns"].as_str() {
        let list: Vec<&str> = cols.split(';').filter(|s| !s.trim().is_empty()).collect();
        set("tracks_columns", json!(list));
    }
}

// ---- the report ----

fn write_report(report: &Result<Value, String>) {
    let body = match report {
        Ok(r) => {
            let c = &r["counts"];
            let pl = &r["playlists"];
            let mut s = String::new();
            s.push_str("# Dopamine import report\n\n");
            s.push_str(&format!("Imported {} UTC\n\n", crate::library::now_secs()));
            s.push_str("## Counts in vs out (must match)\n\n");
            s.push_str(&format!(
                "| what | in | out |\n|---|---|---|\n| tracks | {} | {} |\n| loved | {} | {} |\n| plays | {} | {} |\n\n",
                c["tracksIn"], c["tracksOut"], c["lovedIn"], c["lovedOut"], c["playsIn"], c["playsOut"]
            ));
            s.push_str("## Everything else\n\n");
            s.push_str(&format!("- folders carried over: {}\n", r["foldersOut"]));
            s.push_str(&format!("- album artwork copied: {}\n", r["artworkCopied"]));
            s.push_str(&format!("- artist images kept: {}\n", r["artistImages"]));
            s.push_str(&format!("- ratings backup entries applied: {}\n", r["ratingsMerged"]));
            s.push_str(&format!(
                "- playlists copied: {} of {}{}\n",
                pl["out"], pl["in"],
                pl["names"].as_array().map(|n| format!(" ({})", n.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(", "))).unwrap_or_default()
            ));
            s.push_str(&format!("- queue entries imported: {}\n", r["queueOut"]));
            s.push_str(&format!("- settings imported: {}\n", r["settingsImported"]));
            s.push_str("- scrobbling sessions: moved into settings, values not shown here\n");
            s
        }
        Err(e) => format!("# Dopamine import report\n\nThe import failed: {e}\n"),
    };
    let path = report_path();
    if let Some(p) = path.parent() {
        let _ = std::fs::create_dir_all(p);
    }
    if let Ok(mut f) = std::fs::File::create(&path) {
        let _ = f.write_all(body.as_bytes());
    }
}

// ---- dry run ----

/// --import-dry-run: read the real Dopamine data (or a copy given on the
/// command line), count everything, write nothing anywhere. Returns the
/// process exit code and prints the report JSON to stdout.
pub fn dry_run_cli() -> i32 {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let pos = args.iter().position(|a| a == "--import-dry-run");
    let db_arg = pos
        .and_then(|i| args.get(i + 1))
        .filter(|a| !a.starts_with("--"))
        .map(PathBuf::from);
    let db = db_arg.unwrap_or_else(|| dop_dir().join("Dopamine.db"));
    let config = dop_dir().join("config.json");
    let mut tmp: Option<PathBuf> = None;
    let db_ref: PathBuf = if db.exists() && db.starts_with(dop_dir()) {
        // The original: still never opened. Copy to temp first.
        let t = std::env::temp_dir().join(format!("smp-dryrun-{}.db", std::process::id()));
        if std::fs::copy(&db, &t).is_err() {
            println!("dry run: could not copy {}", db.display());
            return 1;
        }
        for ext in ["-wal", "-shm"] {
            let src = db.with_extension(format!("db{ext}"));
            let dst = t.with_extension(format!("db{ext}"));
            if src.exists() {
                let _ = std::fs::copy(&src, &dst);
            }
        }
        tmp = Some(t.clone());
        t
    } else {
        db.clone()
    };
    let cfg_ref = if config.exists() { Some(config) } else { None };
    let result = dry_run(&db_ref, cfg_ref.as_deref());
    if let Some(t) = tmp {
        let _ = std::fs::remove_file(&t);
        let _ = std::fs::remove_file(t.with_extension("db-wal"));
        let _ = std::fs::remove_file(t.with_extension("db-shm"));
    }
    match result {
        Ok(mut r) => {
            // The dry run writes nothing, so "out" is what would be written:
            // shown as the same numbers, with the plan in place of the queue.
            if let Some(o) = r.as_object_mut() {
                o.insert("dryRun".into(), json!(true));
            }
            println!("{}", serde_json::to_string_pretty(&r).unwrap_or_default());
            0
        }
        Err(e) => {
            println!("dry run failed: {e}");
            1
        }
    }
}

/// Count what the import would bring over, write nothing. Any Library works
/// (usually an empty one); it is only used to keep the signature parallel to
/// run_import.
pub fn dry_run(db_copy: &Path, config: Option<&Path>) -> Result<Value, String> {
    let conn = rusqlite::Connection::open_with_flags(
        db_copy,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .map_err(|e| format!("opening the copy: {e}"))?;
    let tracks_in: i64 = conn
        .query_row("SELECT COUNT(*) FROM Track", [], |r| r.get(0))
        .unwrap_or(0);
    let loved_in: i64 = conn
        .query_row("SELECT COUNT(*) FROM Track WHERE Love = 1", [], |r| r.get(0))
        .unwrap_or(0);
    let plays_in: i64 = conn
        .query_row("SELECT SUM(PlayCount) FROM Track", [], |r| r.get(0))
        .unwrap_or(0);
    let queue: i64 = conn
        .query_row("SELECT COUNT(*) FROM QueuedTrack", [], |r| r.get(0))
        .unwrap_or(0);
    let mut settings_count = 0i64;
    if let Some(cfg) = config {
        let mut m = Map::new();
        import_settings_from_config(cfg, &mut m);
        settings_count = m.len() as i64;
    }
    let cache_dir = dop_dir().join("Cache");
    let artwork: i64 = std::fs::read_dir(cache_dir.join("CoverArt"))
        .map(|rd| rd.flatten().filter(|e| e.metadata().map(|m| m.len() > 1000).unwrap_or(false)).count() as i64)
        .unwrap_or(0);
    let playlists_src = dirs::audio_dir().map(|d| d.join("Dopamine").join("Playlists"));
    let playlists = playlists_src
        .and_then(|d| std::fs::read_dir(d).ok())
        .map(|rd| {
            rd.flatten()
                .filter(|e| {
                    e.path()
                        .extension()
                        .and_then(|s| s.to_str())
                        .map(|s| s.eq_ignore_ascii_case("m3u"))
                        .unwrap_or(false)
                })
                .count() as i64
        })
        .unwrap_or(0);
    Ok(json!({
        "dryRun": true,
        "counts": {
            "tracksIn": tracks_in,
            "tracksOut": "n/a (nothing written)",
            "lovedIn": loved_in,
            "lovedOut": "n/a (nothing written)",
            "playsIn": plays_in,
            "playsOut": "n/a (nothing written)",
        },
        "queueIn": queue,
        "settingsImported": settings_count,
        "artworkAvailable": artwork,
        "playlistsFound": playlists,
    }))
}
