# Seamless Music Player — build spec (2026-11-24)

The owner's own music player, a Rust rewrite of Dopamine Enhanced
(`H:\System Configuration\dopamine-fork`, branch `enhanced`, Angular + Electron). Goal: same
features and look, a fraction of the memory and CPU. Not a fork any more: own name, icon,
data folder, no Dopamine branding anywhere.

## Decisions (made by the orchestrator, final)

| Area | Choice |
|---|---|
| Window | `tao` + `wry` (system WebView2), like Etcher (`work\mocha-audio\src\bin\app.rs`). No Electron, no Node, no Angular. |
| UI | One plain `ui/` folder: `index.html`, `app.css`, `app.js`. No framework, no bundler, no npm. Embedded into the exe with `include_dir`/`include_bytes`. |
| UI transport | Custom protocol `smp://app/...` (no TCP port). Artwork at `smp://art/<artworkId>`. |
| IPC | UI -> Rust: `window.ipc.postMessage(JSON.stringify({id, cmd, args}))`. Rust -> UI reply: `window.__smp.reply(id, ok, data)`. Rust -> UI push: `window.__smp.event(name, payload)`. |
| Audio | Decoding and output in Rust: `symphonia` (mp3, flac, aac/m4a, alac, vorbis, opus via symphonia where possible, wav, aiff) -> `rubato` resample -> `cpal` WASAPI shared mode on the chosen device. Gapless and crossfade, ReplayGain, 10-band EQ, volume. The webview never plays audio. |
| Library | `rusqlite` (bundled) in `%APPDATA%\Seamless Music Player\library.db`. Tags with `lofty`. Folder watch with `notify` (debounced). |
| Settings | `%APPDATA%\Seamless Music Player\settings.json` (serde, defaults for every key). |
| Media keys | Windows SMTC (`windows` crate, `Media_Playback`) so the keyboard keys and the Windows flyout work. |
| Tray | `tray-icon`, native Win32 menu (Seamless Flyouts styles it). |
| Discord | `discord-rich-presence` crate: title, artist, album, artwork URL when known, progress. |
| Scrobbling | Last.fm and ListenBrainz over `ureq` (both are on today). |
| Lyrics | Embedded tags first, then LRCLIB online (synced when available). |
| Online search and downloads | slskd REST API at `127.0.0.1:5030/api/v0` (login read from `C:\ProgramData\slskd\data\slskd.yml`) and `yt-dlp` as a hidden child process (`CREATE_NO_WINDOW`). Port the behaviour of `dopamine-fork\src\app\services\music-search\music-search.service.ts` exactly. |
| Visualizer | Rust computes FFT bars (`realfft`), pushes ~30 fps `viz` events only while the window is visible and the visualizer is shown. |
| Icon | New, ours: Mocha crust tile, blue `#89b4fa` to mauve `#cba6f7` gradient mark. Same family as Etcher's icon (`work\mocha-audio\etcher.ico`, `icon-preview.png`). Multi-size `.ico` embedded with `embed-resource`. |

## Layout (who owns what)

```
seamless-music-player/
  Cargo.toml, build.rs, app.rc         backend helper
  src/                                 backend helper
    main.rs        window, tray, protocol, IPC dispatch
    library.rs     db, scan, watch, queries
    migrate.rs     Dopamine import
    player.rs      decode, output, queue, gapless, crossfade, eq, replaygain
    smtc.rs, discord.rs, scrobble.rs, lyrics.rs, online.rs (slskd + yt-dlp), settings.rs
  ui/                                  UI helper
    index.html, app.css, app.js, fonts (none: FiraCode Nerd Font is installed)
  assets/                              UI helper
    icon.svg, smp.ico, icon-preview.png, tools/make-icon.py
  notes/, reports/                     both (own files only)
```

The two helpers never edit each other's folders. The IPC contract below is the seam; change
it only by editing this file's contract section and saying so in the report.

## IPC contract (v1)

Commands (`cmd`, args -> reply data):

- `library.overview` -> `{tracks, albums, artists, genres, playlists, folders}` counts
- `library.tracks` `{sort, filter?, offset, limit}` -> `{total, items:[Track]}`
- `library.albums` `{sort, artist?, genre?}` -> `[Album]`
- `library.artists` `{kind:"track"|"album"}` -> `[Artist]`
- `library.genres` -> `[Genre]`
- `library.album` `{albumKey}` -> `{album, tracks}`
- `library.artist` `{name}` -> `{artist, albums, tracks, imageId?}`
- `library.folders` -> `[Folder]`; `library.folder` `{path}` -> `{subfolders, tracks}`
- `library.addFolder` `{path?}` (no path = native folder picker), `library.removeFolder` `{id}`, `library.rescan`
- `library.search` `{q}` -> `{tracks, albums, artists}` (local)
- `track.setLove` `{id, love}`, `track.setRating` `{id, rating 0-5}`, `track.delete` `{ids}` (to Recycle Bin), `track.showInFolder` `{id}`
- `playlists.list` -> `[Playlist]`; `playlists.tracks` `{path}`; `playlists.create` `{name}`; `playlists.rename`; `playlists.delete`; `playlists.add` `{path, trackIds}`; `playlists.remove` `{path, indexes}`; `playlists.reorder`
- `player.play` `{trackIds, startIndex}` | `{playlist}` | `{albumKey}` | `{artist}`; `player.toggle`; `player.next`; `player.prev`; `player.seek` `{seconds}`; `player.volume` `{value 0-1}`; `player.shuffle` `{on}`; `player.loop` `{mode:"off"|"all"|"one"}`; `player.queue` -> `{items, index}`; `player.queueAdd` `{trackIds, next?}`; `player.queueRemove` `{indexes}`; `player.queueMove`; `player.state` -> `PlayerState`
- `player.sleepTimer` `{minutes|null, endOfSong?}`
- `eq.get` / `eq.set` `{enabled, preset, gains[10]}`
- `lyrics.get` `{trackId}` -> `{synced:[{t,text}]|null, plain|null, source}`
- `online.search` `{q}` -> pushes `online.results` events (soulseek and youtube groups)
- `online.stream` `{resultId}`; `online.download` `{resultId}`; `online.cancel` `{jobId}`
- `settings.get` -> full settings; `settings.set` `{key, value}`
- `devices.list` -> output devices; `devices.set` `{id}`
- `window.minimize|maximize|close|startDrag` (native title bar stays; these are for the mini player)
- `app.info` -> `{version, memoryMb}`

Pop-ups (added 2026-11-24, owner rule: real acrylic, never in-page CSS blur). Every menu,
context menu, pick list, pop-over and dialog opens in one native acrylic window that loads
`index.html?popup=1`. In that mode the page draws only the pop-up surface on a transparent
body with no CSS blur and no fill of its own (the window is the glass), and exposes
`window.__smpPopup = {render(payload), shown(), hide()}`.
- Main page: `popup.open {token, kind:"menu"|"list"|"popover"|"dialog", anchor:{x,y,w,h}
  (CSS px from the page's top-left), placement?:"below"|"above"|"end"|"right"|"point"|"center",
  title?, items?:[{id, label, icon?, hint?, checked?, disabled?, sep?, danger?}], body?}`;
  `popup.close`. Listen for events `popup.picked {token, id, values?}` and `popup.closed {token}`.
- Pop-up page: after `render`, measure the surface and send `popup.size {w,h}` (CSS px);
  Rust places it inside the monitor, flips when there is no room, shows and focuses it,
  then calls `shown()` (start the fade plus short slide). On click send
  `popup.pick {id, values?}`; Esc sends `popup.close`. `hide()` plays the close animation
  (none when motion is off) then sends `popup.hidden`. Menus and lists close when the
  window loses focus or the main window moves; dialogs stay until closed and drag with
  `window.startDrag`. Other commands from a dialog are answered into the pop-up page.
- Toasts (added 2026-11-24, owner rule covers toasts too): one native acrylic window that
  never takes focus, loading `index.html?toast=1`. Main page `toast()` sends
  `toast.show {kind, title, text}`; Rust calls `window.__smpToast.add(payload)` in the
  toast page (skipped when the main window is hidden or minimised). The toast page shows one
  toast at a time (a new one replaces the old), measures it and sends `toast.size {w,h}`;
  Rust centres it 116 px above the main window's bottom edge, shows it without activating,
  then calls `shown()`. After about 3.4 s, or `clear()` (sent when the main window moves),
  the page plays the close animation and sends `toast.hidden`.
- Stay in the page: tooltips and anything nested inside a pop-up.

Events: `player.state` (on change, plus 4 Hz position while playing), `viz` `{bars:[f32]}`,
`library.changed`, `scan.progress` `{done,total}`, `online.results`, `download.progress`
`{jobId, pct, state}`, `toast` `{text, kind}`.

Track: `{id, path, title, artists[], albumArtists[], album, albumKey, genres[], year,
trackNo, discNo, duration, bitrate, sampleRate, rating, love, playCount, skipCount,
dateAdded, lastPlayed, artworkId}`. PlayerState: `{trackId, track, playing, position,
duration, volume, shuffle, loop, sleepTimer}`.

## Features to keep (from Dopamine Enhanced, all of them)

Collection pages: Artists, Genres, Albums, Songs, Playlists, Folders. Home page, search
page, artist and album pages (the `enh-browse` work), radio (play similar). Now playing
page with big artwork, rich synced lyrics, audio visualizer (blue to mauve). Playback bar
with wave or flat progress, volume, shuffle, loop, sleep timer, love, rating, speed.
Playback queue. Mini player (cover and compact). Songs table with chosen columns, artwork in
rows, sorting, multi-select with Shift and Ctrl, context menus (play next, add to queue,
add to playlist, love, show in folder, delete). Played songs from online stay out of the
library unless "Save to library". Live library pickup from the watched folders. Online
panel: Soulseek and YouTube results while typing in the search bar, stream or download into
the watched Downloads folder with a spinner until progress shows. Settings page (appearance:
font family and weight pickers, blur amount; behaviour; online; advanced). Tray icon and
menu. Remember playback state after restart. Discord status. Scrobbling.

## Data import (one time, first run)

From `%APPDATA%\Dopamine Enhanced\Dopamine.db` (read a copy, never write the original) and
`config.json`: folders (`G:\Dopamine`), every track with Love, Rating, PlayCount, SkipCount,
DateLastPlayed, DateAdded; album and artist artwork from the Dopamine cache folder; the
queue and position; settings that still mean something (volume, device, crossfade, gapless,
EQ, font, blur, scrobbling and their sessions, columns). Playlists: `Music\Dopamine\Playlists`
(m3u, including `Liked.m3u`) are copied to `Music\Seamless Music Player\Playlists`.
`Music\Dopamine\Ratings\ratings-backup.json` is merged too. The import writes a report of
counts in and counts out; numbers must match (704 tracks, 22 loved, 478 plays as of today).

## Speed targets (gate)

Measured on Warden with the same library, playing a song, window open:
- Private memory of all processes (exe + WebView2 children) at most a third of Dopamine
  Enhanced's. Baseline is recorded by the orchestrator in `reports\baseline.md`.
- CPU while playing with the window minimised: under 1% of one core.
- Cold start to a filled Songs page: under 1 second.

## House rules

- Builds only through the load gate:
  `python "H:\System Configuration\agent-tools\LoadGate\heavy.py" run --wait 6600 --reason "smp build" -- cargo build --release`
  (exit 75 = nothing ran, retry later). Never raise a priority.
- Never open windows on screen, click or type. Check the UI headless (Playwright against the
  `ui/` folder with a stub `window.ipc`, see the UI brief). The orchestrator does the live run.
- Never touch the installed Dopamine Enhanced or its data (read copies only).
- Back up any file outside this folder before changing it. No secrets in files or logs.
- Plain English in every visible string. No emoji. No em dashes in UI text.
- Commit to this repo often, small commits, message in plain words.
