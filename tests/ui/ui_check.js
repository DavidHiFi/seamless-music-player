/* Headless UI check for Seamless Music Player.
   Run with:  playwright-cli open
              playwright-cli --raw run-code --filename=tests/ui/ui_check.js
              playwright-cli close
   Loads ui/ with a stub window.ipc answering the SPEC.md contract with fake
   data, screenshots the main pages and a pop-up menu, and reports IPC checks
   and console errors as JSON. Headless only, never a visible window. */
async page => {
  const REPORTS = "H:/System Configuration/work/seamless-music-player/reports/";
  const URL = "file:///H:/System%20Configuration/work/seamless-music-player/ui/index.html";
  const errors = [];
  const results = {};
  const check = (name, ok) => { results[name] = ok ? "ok" : "FAIL"; };
  const watch = (pg, label) => {
    pg.on("console", (m) => { if (m.type() === "error") errors.push(label + ": " + m.text()); });
    pg.on("pageerror", (e) => errors.push(label + ": " + ((e && e.message) || e)));
  };

  const STUB = `
    window.__ART_BASE = "http://localhost:9/art/";
    window.__POSTED = [];
    const TRACKS = Array.from({ length: 36 }, (_, i) => ({
      id: "t" + (i + 1),
      path: "G:/Music/track" + (i + 1) + ".flac",
      title: "Song number " + (i + 1),
      artists: ["Artist " + (1 + (i % 6))],
      albumArtists: ["Artist " + (1 + (i % 6))],
      album: "Album " + (1 + (i % 8)),
      albumKey: "ak" + (1 + (i % 8)),
      genres: ["Genre " + (1 + (i % 4))],
      year: 2015 + (i % 9),
      trackNo: i + 1,
      discNo: 1,
      duration: 150 + i * 7,
      bitrate: 900 + i,
      sampleRate: 44100,
      rating: i % 6,
      love: i % 5 === 0,
      playCount: (i * 13) % 120,
      skipCount: 0,
      dateAdded: Date.now() - i * 86400000,
      lastPlayed: Date.now() - i * 3600000,
      artworkId: "art" + (1 + (i % 8)),
    }));
    const ALBUMS = Array.from({ length: 8 }, (_, i) => ({
      albumKey: "ak" + (i + 1), title: "Album " + (i + 1), artists: ["Artist " + (1 + (i % 6))],
      year: 2015 + i, artworkId: "art" + (i + 1),
    }));
    const ARTISTS = Array.from({ length: 6 }, (_, i) => ({ name: "Artist " + (i + 1), albumCount: 2, imageId: "art" + (i + 1) }));
    const GENRES = Array.from({ length: 4 }, (_, i) => ({ name: "Genre " + (i + 1), trackCount: 90 + i, artworkId: "art" + (i + 1) }));
    const PLAYLISTS = [
      { name: "Liked", path: "C:/p/Liked.m3u", trackCount: 22 },
      { name: "Late night drive", path: "C:/p/night.m3u", trackCount: 14 },
      { name: "Focus", path: "C:/p/focus.m3u", trackCount: 31 },
    ];
    const SETTINGS = {
      appearance: { fontFamily: "FiraCode Nerd Font", fontWeight: 500, blur: 22 },
      motion: { enabled: true, speed: "normal" },
      behaviour: { gapless: true },
      online: {}, advanced: {},
      eq: { enabled: false, preset: "flat", gains: [0, 0, 0, 0, 0, 0, 0, 0, 0, 0] },
    };
    window.ipc = {
      postMessage(msg) {
        let v; try { v = JSON.parse(msg); } catch (e) { return; }
        window.__POSTED.push({ cmd: v.cmd, args: v.args || {} });
        const reply = (data) => setTimeout(() => {
          try { window.__smp.reply(v.id, true, data); } catch (e) {}
        }, 5);
        switch (v.cmd) {
          case "settings.get": reply(SETTINGS); break;
          case "library.overview": reply({ tracks: 36, albums: 8, artists: 6, genres: 4, playlists: 3, folders: 1 }); break;
          case "library.tracks": {
            const off = (v.args && v.args.offset) || 0, lim = (v.args && v.args.limit) || 200;
            reply({ total: TRACKS.length, items: TRACKS.slice(off, off + lim) }); break;
          }
          case "library.albums": reply(ALBUMS); break;
          case "library.artists": reply(ARTISTS); break;
          case "library.genres": reply(GENRES); break;
          case "library.search": reply({ tracks: TRACKS.slice(0, 3), albums: ALBUMS.slice(0, 2), artists: ARTISTS.slice(0, 2) }); break;
          case "playlists.list": reply(PLAYLISTS); break;
          case "playlists.tracks": reply(TRACKS.slice(0, 5)); break;
          case "player.state": reply({ trackId: TRACKS[0].id, track: TRACKS[0], playing: true, position: 63, duration: TRACKS[0].duration, volume: 0.64, shuffle: false, loop: "all", sleepTimer: null, speed: 1 }); break;
          case "player.queue": reply({ items: TRACKS.slice(0, 6), index: 0 }); break;
          case "devices.list": reply([{ id: "d1", name: "Speakers" }, { id: "d2", name: "Headphones" }]); break;
          case "app.info": reply({ version: "0.1.0", memoryMb: 42 }); break;
          case "lyrics.get": reply(null); break;
          default: reply({ ok: true });
        }
      },
    };
  `;

  // stand-in artwork: one gradient tile per id
  await page.route("http://localhost:9/art/**", (route) => {
    const id = route.request().url().split("/art/")[1] || "0";
    let h = 0; for (const ch of id) h = (h * 31 + ch.charCodeAt(0)) >>> 0;
    const a = 180 + (h % 60), b = 40 + ((h >> 3) % 60), c = 60 + ((h >> 6) % 50);
    const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="120" height="120"><defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="rgb(${a},${b},${c})"/><stop offset="1" stop-color="rgb(${c},${a},${b})"/></linearGradient></defs><rect width="120" height="120" fill="url(#g)"/></svg>`;
    route.fulfill({ status: 200, contentType: "image/svg+xml", body: svg });
  });

  await page.addInitScript(STUB);
  await page.setViewportSize({ width: 1280, height: 800 });
  watch(page, "main");
  await page.goto(URL, { waitUntil: "load" });
  await page.waitForTimeout(700);
  // the real app has the native acrylic behind the transparent body; stand in for it
  await page.evaluate(() => { document.documentElement.style.background = "#1E1E2E"; });

  // Songs page
  await page.evaluate(() => nav("songs"));
  await page.waitForTimeout(600);
  await page.screenshot({ path: REPORTS + "ui-songs.png" });

  // Now playing page, with visualizer bars
  await page.evaluate(() => {
    window.__smp.event("viz", { bars: Array.from({ length: 28 }, (_, i) => 0.2 + 0.8 * Math.abs(Math.sin(i * 0.9))) });
    openNp();
  });
  await page.waitForTimeout(500);
  await page.screenshot({ path: REPORTS + "ui-now-playing.png" });
  check("now playing page opens", await page.evaluate(() => !document.querySelector("#npPage").classList.contains("hidden")));
  await page.evaluate(() => closeNp());

  // Settings page
  await page.evaluate(() => nav("settings"));
  await page.waitForTimeout(400);
  await page.screenshot({ path: REPORTS + "ui-settings.png" });

  // context menu posts popup.open and popup.picked runs the action
  await page.evaluate(() => { nav("songs"); });
  await page.waitForTimeout(300);
  await page.evaluate(() => {
    window.__PICKED = false;
    showMenu([{ label: "Play now", ic: "play", fn: () => { window.__PICKED = true; } }], 300, 200);
  });
  await page.waitForTimeout(200);
  const opened = await page.evaluate(() => {
    const opens = window.__POSTED.filter((m) => m.cmd === "popup.open");
    return opens.length ? opens[opens.length - 1].args : null;
  });
  check("showMenu posts popup.open", !!opened && opened.kind === "menu" && opened.placement === "point");
  check("popup.open carries a token", !!opened && !!opened.token);
  const tok = opened && opened.token;
  await page.evaluate((t) => window.__smp.event("popup.picked", { token: t, id: "m0" }), tok);
  await page.waitForTimeout(100);
  check("popup.picked runs the item action", await page.evaluate(() => window.__PICKED === true));

  // combo pick list opens as a pop-up list
  await page.evaluate(() => nav("settings"));
  await page.waitForTimeout(300);
  await page.evaluate(() => document.querySelector(".combo").click());
  await page.waitForTimeout(200);
  check("combo opens a pop-up list", await page.evaluate(() => {
    const opens = window.__POSTED.filter((m) => m.cmd === "popup.open");
    return opens.length && opens[opens.length - 1].args.kind === "list";
  }));

  // dialog values come back into the action
  await page.evaluate(() => newPlaylistDialog());
  await page.waitForTimeout(200);
  await page.evaluate(() => {
    const opens = window.__POSTED.filter((m) => m.cmd === "popup.open");
    window.__smp.event("popup.picked", { token: opens[opens.length - 1].args.token, id: "a1", values: { name: "Test list" } });
  });
  await page.waitForTimeout(200);
  check("dialog values reach the action", await page.evaluate(() =>
    window.__POSTED.some((m) => m.cmd === "playlists.create" && m.args.name === "Test list")));

  // ---- pop-up window page (?popup=1) ----
  const p2 = await page.context().newPage();
  watch(p2, "popup");
  await p2.addInitScript(STUB);
  await p2.setViewportSize({ width: 640, height: 480 });
  await p2.goto(URL + "?popup=1", { waitUntil: "load" });
  await p2.waitForTimeout(400);
  const bodyInfo = await p2.evaluate(() => {
    const visible = (sel) => { const n = document.querySelector(sel); return n && getComputedStyle(n).display !== "none"; };
    return {
      mainGone: !visible("#app") && !visible("#npPage") && !visible("#onlinePanel")
        && !visible("#queueDrawer") && !visible("#miniPlayer") && !visible("#toasts"),
      bg: getComputedStyle(document.body).backgroundColor,
      blur: getComputedStyle(document.body).backdropFilter + " / " + getComputedStyle(document.querySelector("#popupLayer")).backdropFilter,
    };
  });
  check("popup page draws only the surface", bodyInfo.mainGone);
  check("popup body stays transparent", bodyInfo.bg === "rgba(0, 0, 0, 0)");
  check("popup page has no backdrop blur", bodyInfo.blur === "none / none");

  // busy dark background behind the surface, plus a stand-in for the native glass tint
  await p2.evaluate(() => {
    document.body.style.background = "repeating-linear-gradient(45deg, #181825 0px, #181825 14px, #1E1E2E 14px, #1E1E2E 28px)";
    const d = document.createElement("div");
    d.style.cssText = "position:fixed;inset:0;z-index:0;color:rgba(205,214,244,0.6);font:12px monospace;padding:6px;white-space:pre-wrap;overflow:hidden;pointer-events:none";
    d.textContent = Array.from({ length: 30 }, (_, i) => "Busy text behind the pop-up surface, line " + (i + 1) + ", 0123456789").join("\n");
    document.body.appendChild(d);
  });
  // stand-in for the native acrylic: tint plus blur, what Windows renders behind the surface
  await p2.addStyleTag({ content: "#popupLayer .pop-surface { background: rgba(30,30,46,0.86); backdrop-filter: blur(24px) saturate(1.25); }" });

  await p2.evaluate(() => {
    window.__smpPopup.render({
      token: "t1", kind: "menu", placement: "point", anchor: { x: 320, y: 220, w: 0, h: 0 },
      items: [
        { id: "m0", label: "Play now", icon: "play" },
        { id: "m1", label: "Play next", icon: "next" },
        { id: "m2", label: "Add to queue", icon: "queueadd" },
        { id: "m3", sep: true },
        { id: "m4", label: "Add to playlist", icon: "plus", children: [
          { id: "m4.0", label: "Liked", icon: "playlists" },
          { id: "m4.1", label: "Late night drive", icon: "playlists" },
          { id: "m4.2", label: "New playlist", icon: "plus" },
        ] },
        { id: "m5", label: "Love", icon: "heart" },
        { id: "m6", label: "Rate", icon: "star", children: [
          { id: "m6.0", label: "No rating", icon: "star" },
          { id: "m6.1", label: "1 star", icon: "star" },
          { id: "m6.2", label: "2 stars", icon: "star" },
        ] },
        { id: "m7", sep: true },
        { id: "m8", label: "Show in folder", icon: "folders" },
        { id: "m9", label: "Delete", icon: "trash", danger: true },
      ],
    });
    window.__smpPopup.shown();
  });
  await p2.waitForTimeout(250);
  const sized = await p2.evaluate(() => {
    const s = window.__POSTED.filter((m) => m.cmd === "popup.size").pop();
    return s ? s.args : null;
  });
  check("popup.size measured the surface", !!sized && sized.w > 100 && sized.h > 100);

  // nested submenu opens in the same window and widens it
  await p2.click(".menu-item:has-text('Add to playlist')");
  await p2.waitForTimeout(250);
  check("nested submenu widens the pop-up", await p2.evaluate(() => {
    const sizes = window.__POSTED.filter((m) => m.cmd === "popup.size").map((m) => m.args.w);
    return sizes.length > 1 && sizes[sizes.length - 1] > sizes[0];
  }));
  await p2.screenshot({ path: REPORTS + "ui-popup-menu.png" });

  // clicking a row posts popup.pick
  await p2.click(".menu > .menu-item:has-text('Play next')");
  await p2.waitForTimeout(150);
  check("click posts popup.pick", await p2.evaluate(() => {
    const picks = window.__POSTED.filter((m) => m.cmd === "popup.pick");
    return picks.length && picks[picks.length - 1].args.id === "m1";
  }));

  // arrow keys and Enter move and pick
  await p2.evaluate(() => {
    window.__smpPopup.render({ token: "t2", kind: "menu", placement: "point", anchor: { x: 100, y: 100, w: 0, h: 0 }, items: [
      { id: "m0", label: "Alpha", icon: "play" },
      { id: "m1", label: "Beta", icon: "next" },
      { id: "m2", label: "Gamma", icon: "queueadd" },
    ] });
    window.__smpPopup.shown();
  });
  await p2.waitForTimeout(150);
  await p2.keyboard.press("ArrowDown");
  await p2.keyboard.press("ArrowDown");
  await p2.keyboard.press("Enter");
  await p2.waitForTimeout(100);
  check("arrow keys and Enter pick", await p2.evaluate(() => {
    const picks = window.__POSTED.filter((m) => m.cmd === "popup.pick");
    return picks.length && picks[picks.length - 1].args.id === "m1";
  }));

  // type-to-find in lists
  await p2.evaluate(() => {
    window.__smpPopup.render({ token: "t3", kind: "list", placement: "below", anchor: { x: 100, y: 100, w: 160, h: 30 }, items: [
      { id: "0", label: "FiraCode Nerd Font", checked: true },
      { id: "1", label: "Segoe UI" },
      { id: "2", label: "Cascadia Code" },
    ] });
    window.__smpPopup.shown();
  });
  await p2.waitForTimeout(150);
  await p2.keyboard.type("seg");
  await p2.keyboard.press("Enter");
  await p2.waitForTimeout(100);
  check("type-to-find picks the match", await p2.evaluate(() => {
    const picks = window.__POSTED.filter((m) => m.cmd === "popup.pick");
    return picks.length && picks[picks.length - 1].args.id === "1";
  }));

  // dialog: required input, values back, Escape closes, header drags
  await p2.evaluate(() => {
    window.__smpPopup.render({ token: "t4", kind: "dialog", placement: "center", anchor: { x: 200, y: 200, w: 0, h: 0 }, title: "New playlist",
      body: '<input class="txtbox" type="text" data-name="name" placeholder="Playlist name" required spellcheck="false">',
      items: [{ id: "a0", label: "Cancel" }, { id: "a1", label: "Create", primary: true }] });
    window.__smpPopup.shown();
  });
  await p2.waitForTimeout(150);
  await p2.click(".dlg-actions .btn.primary");
  await p2.waitForTimeout(100);
  check("required input blocks an empty pick", await p2.evaluate(() =>
    !window.__POSTED.some((m) => m.cmd === "popup.pick" && m.args.token === "t4")));
  await p2.fill('[data-name="name"]', "Evening mix");
  await p2.click(".dlg-actions .btn.primary");
  await p2.waitForTimeout(100);
  check("dialog values come back", await p2.evaluate(() => {
    const picks = window.__POSTED.filter((m) => m.cmd === "popup.pick");
    const p = picks[picks.length - 1];
    return p && p.args.values && p.args.values.name === "Evening mix";
  }));
  check("dialog header drag posts startDrag", await p2.evaluate(() => {
    const head = document.querySelector(".dlg-head");
    if (!head) return false;
    head.dispatchEvent(new MouseEvent("mousedown", { bubbles: true }));
    return window.__POSTED.some((m) => m.cmd === "window.startDrag");
  }));
  await p2.keyboard.press("Escape");
  await p2.waitForTimeout(100);
  check("Escape posts popup.close", await p2.evaluate(() => window.__POSTED.some((m) => m.cmd === "popup.close")));

  // hide() clears the surface and posts popup.hidden
  await p2.evaluate(() => {
    window.__smpPopup.render({ token: "t5", kind: "menu", placement: "point", anchor: { x: 100, y: 100, w: 0, h: 0 }, items: [{ id: "m0", label: "Only" }] });
    window.__smpPopup.hide();
  });
  await p2.waitForTimeout(300);
  check("hide posts popup.hidden", await p2.evaluate(() => window.__POSTED.some((m) => m.cmd === "popup.hidden")));
  check("surface cleared after hide", await p2.evaluate(() => document.querySelector("#popupLayer").children.length === 0));

  await p2.close();
  return JSON.stringify({ results, consoleErrors: errors }, null, 2);
}
