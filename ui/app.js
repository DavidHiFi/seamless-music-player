"use strict";
/* Seamless Music Player UI. No framework, no bundler.
   Talks to the Rust backend only through the IPC contract in SPEC.md:
   window.ipc.postMessage(JSON.stringify({id, cmd, args})), replies on
   window.__smp.reply(id, ok, data), pushes on window.__smp.event(name, payload). */

/* ---------- tiny helpers ---------- */
const $ = (sel) => document.querySelector(sel);
const esc = (s) => String(s == null ? "" : s)
  .replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;")
  .replace(/"/g, "&quot;").replace(/'/g, "&#39;");
const el = (tag, cls, html) => {
  const n = document.createElement(tag);
  if (cls) n.className = cls;
  if (html != null) n.innerHTML = html;
  return n;
};
const fmtTime = (s) => {
  if (!isFinite(s) || s < 0) s = 0;
  s = Math.round(s);
  const m = Math.floor(s / 60), h = Math.floor(m / 60);
  const sec = String(s % 60).padStart(2, "0");
  return h ? `${h}:${String(m % 60).padStart(2, "0")}:${sec}` : `${m}:${sec}`;
};
const fmtCount = (n) => (n == null ? "" : n.toLocaleString("en-GB"));
const fmtPlays = (n) => (n >= 1000 ? (n / 1000).toFixed(n >= 10000 ? 0 : 1) + "k" : String(n ?? 0));

/* ---------- icons: no emoji, line style, currentColor ---------- */
const ICONS = {
  home: '<path d="M3 10.5 12 3l9 7.5"/><path d="M5.5 9.5V20h13V9.5"/>',
  artists: '<circle cx="9" cy="8" r="3.2"/><path d="M3.5 19c.6-3 2.8-4.6 5.5-4.6S13.9 16 14.5 19"/><circle cx="16.5" cy="9.5" r="2.4"/><path d="M15.5 14.7c2.6.1 4.4 1.5 5 4.3"/>',
  genres: '<circle cx="6.5" cy="17.5" r="2.5"/><circle cx="17.5" cy="15.5" r="2.5"/><path d="M9 17.5V6l11-2v11.5"/>',
  albums: '<circle cx="12" cy="12" r="9"/><circle cx="12" cy="12" r="2.6"/>',
  songs: '<path d="M9 18.5V5.5l10-2v12.5"/><circle cx="6.5" cy="18.5" r="2.5"/><circle cx="16.5" cy="16" r="2.5"/>',
  playlists: '<path d="M4 6h11"/><path d="M4 10.5h11"/><path d="M4 15h6.5"/><circle cx="17" cy="16.5" r="2.5"/><path d="M19.5 16.5V9l2 1"/>',
  folders: '<path d="M3.5 7V17a1.5 1.5 0 0 0 1.5 1.5h14A1.5 1.5 0 0 0 20.5 17V9.5A1.5 1.5 0 0 0 19 8h-7L10 5.5H5A1.5 1.5 0 0 0 3.5 7Z"/>',
  settings: '<circle cx="12" cy="12" r="3"/><path d="M19 12c0-.5 0-1-.1-1.4l2-1.6-2-3.4-2.4 1a7 7 0 0 0-2.4-1.4L13.7 2h-3.4L9.9 5.2a7 7 0 0 0-2.4 1.4l-2.4-1-2 3.4 2 1.6a7.6 7.6 0 0 0 0 2.8l-2 1.6 2 3.4 2.4-1a7 7 0 0 0 2.4 1.4l.4 3.2h3.4l.4-3.2a7 7 0 0 0 2.4-1.4l2.4 1 2-3.4-2-1.6c.1-.4.1-.9.1-1.4Z"/>',
  search: '<circle cx="10.5" cy="10.5" r="6.5"/><path d="m20 20-4.8-4.8"/>',
  x: '<path d="M6 6l12 12"/><path d="M18 6 6 18"/>',
  play: '<path d="M7.5 4.8v14.4L19 12 7.5 4.8Z" fill="currentColor" stroke="none"/>',
  pause: '<rect x="6.5" y="5" width="3.6" height="14" rx="1" fill="currentColor" stroke="none"/><rect x="13.9" y="5" width="3.6" height="14" rx="1" fill="currentColor" stroke="none"/>',
  prev: '<path d="M17.5 5.5v13L8 12l9.5-6.5Z" fill="currentColor" stroke="none"/><rect x="5" y="5.5" width="2.2" height="13" rx="1" fill="currentColor" stroke="none"/>',
  next: '<path d="M6.5 5.5v13L16 12 6.5 5.5Z" fill="currentColor" stroke="none"/><rect x="16.8" y="5.5" width="2.2" height="13" rx="1" fill="currentColor" stroke="none"/>',
  shuffle: '<path d="M3.5 6.5h3.2c4.6 0 5.5 10 10.6 10h1.7"/><path d="m16.5 3.5 3 3-3 3"/><path d="M3.5 16.5h3.2c1.6 0 2.8-1.3 3.8-2.9"/><path d="M12.2 9c1-1.5 2.2-2.5 3.8-2.5h3"/><path d="m16.5 16.5 3 3-3 3"/>',
  repeat: '<path d="M17.5 5.5H8A4.5 4.5 0 0 0 8 14.5h8"/><path d="m13 11.5 3 3-3 3"/><path d="M6.5 18.5H16a4.5 4.5 0 0 0 0-9H8"/>',
  "repeat-one": '<path d="M17.5 5.5H8A4.5 4.5 0 0 0 8 14.5h8"/><path d="m13 11.5 3 3-3 3"/><path d="M6.5 18.5H16a4.5 4.5 0 0 0 0-9H8"/><path d="M11.5 7.5l1.5-1v5" stroke-width="1.4"/>',
  volume: '<path d="M3.5 9.5v5H7l4.5 3.8V5.7L7 9.5H3.5Z"/><path d="M14.5 9a4.5 4.5 0 0 1 0 6"/><path d="M16.8 6.8a7.5 7.5 0 0 1 0 10.4"/>',
  mute: '<path d="M3.5 9.5v5H7l4.5 3.8V5.7L7 9.5H3.5Z"/><path d="m15 9.5 5 5"/><path d="m20 9.5-5 5"/>',
  list: '<path d="M8.5 6.5H20"/><path d="M8.5 12H20"/><path d="M8.5 17.5H20"/><circle cx="4.5" cy="6.5" r="1.2" fill="currentColor" stroke="none"/><circle cx="4.5" cy="12" r="1.2" fill="currentColor" stroke="none"/><circle cx="4.5" cy="17.5" r="1.2" fill="currentColor" stroke="none"/>',
  queueadd: '<path d="M9 12h11"/><path d="M14.5 6.5v11"/><path d="M4 7h5"/><path d="M4 11h5"/><path d="M4 15h3"/>',
  heart: '<path d="M12 19.5S4 14.7 4 9.6C4 7 6 5 8.4 5c1.5 0 2.9.8 3.6 2 .7-1.2 2.1-2 3.6-2C18 5 20 7 20 9.6c0 5.1-8 9.9-8 9.9Z"/>',
  star: '<path d="m12 4 2.4 4.9 5.4.8-3.9 3.8.9 5.4-4.8-2.5-4.8 2.5.9-5.4L4.2 9.7l5.4-.8L12 4Z"/>',
  moon: '<path d="M19.5 14A8 8 0 0 1 10 4.5 8 8 0 1 0 19.5 14Z"/>',
  speed: '<path d="M4.5 15.5a8 8 0 1 1 15 0"/><path d="m12 12 4-3.5"/><circle cx="12" cy="12" r="1.4" fill="currentColor" stroke="none"/>',
  mini: '<rect x="4" y="6" width="16" height="12" rx="2"/><rect x="12" y="12" width="6" height="4" rx="1" fill="currentColor" stroke="none"/>',
  maximize: '<rect x="4.5" y="4.5" width="15" height="15" rx="2"/><path d="M4.5 9.5h15"/>',
  minimize: '<rect x="4.5" y="4.5" width="15" height="15" rx="2"/><path d="M9.5 4.5v15"/>',
  minus: '<path d="M5.5 12h13"/>',
  "chevron-down": '<path d="m6 9.5 6 6 6-6"/>',
  "chevron-right": '<path d="m9.5 6 6 6-6 6"/>',
  "chevron-left": '<path d="m14.5 6-6 6 6 6"/>',
  download: '<path d="M12 4v10"/><path d="m7.5 10.5 4.5 4.5 4.5-4.5"/><path d="M5 19.5h14"/>',
  stream: '<circle cx="12" cy="12" r="2.2"/><path d="M7.8 7.8a6 6 0 0 0 0 8.4"/><path d="M16.2 16.2a6 6 0 0 0 0-8.4"/><path d="M5 5a10 10 0 0 0 0 14"/><path d="M19 19a10 10 0 0 0 0-14"/>',
  folderplus: '<path d="M3.5 7V17a1.5 1.5 0 0 0 1.5 1.5h14A1.5 1.5 0 0 0 20.5 17V9.5A1.5 1.5 0 0 0 19 8h-7L10 5.5H5A1.5 1.5 0 0 0 3.5 7Z"/><path d="M12 11v6"/><path d="M9 14h6"/>',
  trash: '<path d="M5 7h14"/><path d="M8 7V5h8v2"/><path d="M6.5 7 7.5 20h9L17.5 7"/><path d="M10.5 10.5v6"/><path d="M13.5 10.5v6"/>',
  edit: '<path d="M4 20h4L20 8l-4-4L4 16v4Z"/><path d="m13.5 6.5 4 4"/>',
  plus: '<path d="M12 5v14"/><path d="M5 12h14"/>',
  columns: '<rect x="4" y="5" width="16" height="14" rx="2"/><path d="M10 5v14"/><path d="M15 5v14"/>',
  radio: '<circle cx="12" cy="12" r="2"/><path d="M8.5 15.5a5 5 0 0 1 0-7"/><path d="M15.5 8.5a5 5 0 0 1 0 7"/><path d="M5.7 18.3a9 9 0 0 1 0-12.6"/><path d="M18.3 5.7a9 9 0 0 1 0 12.6"/>',
  check: '<path d="m5 12.5 4.5 4.5L19 7.5"/>',
  sort: '<path d="M7 5v14"/><path d="m4 16 3 3 3-3"/><path d="M17 19V5"/><path d="m14 8 3-3 3 3"/>',
  info: '<circle cx="12" cy="12" r="8.5"/><path d="M12 11v5"/><circle cx="12" cy="8" r="1" fill="currentColor" stroke="none"/>',
  warning: '<path d="M12 4 21 19.5H3L12 4Z"/><path d="M12 10v4.5"/><circle cx="12" cy="17" r=".9" fill="currentColor" stroke="none"/>',
  clock: '<circle cx="12" cy="12" r="8.5"/><path d="M12 7v5.5l3.5 2"/>',
};
function icon(name, cls) {
  return `<span class="ic ${cls || ""}"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${ICONS[name] || ICONS.info}</svg></span>`;
}
/* bind every element carrying data-ic */
function hydrateIcons(root) {
  (root || document).querySelectorAll("[data-ic]").forEach((n) => {
    n.innerHTML = icon(n.dataset.ic);
  });
}

/* ---------- IPC ---------- */
const IPC = (() => {
  let seq = 1;
  const pending = new Map();
  const listeners = new Map();
  function send(msg) {
    if (window.ipc && typeof window.ipc.postMessage === "function") {
      window.ipc.postMessage(JSON.stringify(msg));
    }
  }
  function call(cmd, args) {
    return new Promise((resolve) => {
      const id = seq++;
      pending.set(id, resolve);
      send({ id, cmd, args: args || {} });
      setTimeout(() => {
        if (pending.has(id)) { pending.delete(id); resolve(null); }
      }, 20000);
    });
  }
  function fire(name, payload) {
    (listeners.get(name) || []).forEach((fn) => {
      try { fn(payload); } catch (err) { console.error(err); }
    });
  }
  return {
    call,
    on(name, fn) {
      if (!listeners.has(name)) listeners.set(name, []);
      listeners.get(name).push(fn);
    },
    _reply(id, ok, data) {
      const res = pending.get(id);
      if (res) { pending.delete(id); res(ok ? data : null); }
    },
    _event: fire,
  };
})();
window.__smp = {
  reply: (id, ok, data) => IPC._reply(id, ok, data),
  event: (name, payload) => IPC._event(name, payload),
};
/* artwork base: the real app uses the smp:// scheme, tests override it */
const artSrc = (id) => (id ? (window.__ART_BASE || "smp://art/") + id : null);

/* ---------- state ---------- */
const S = {
  settings: null,
  overview: null,
  page: "home",
  params: {},
  navStack: [],
  songsSort: "title",
  songsDir: "asc",
  songsCols: null,
  selection: new Set(),
  anchor: null,
  queue: { items: [], index: -1 },
  player: { playing: false, position: 0, duration: 0, volume: 0.64, shuffle: false, loop: "off", sleepTimer: null, speed: 1, track: null },
  online: { open: false, busy: false, q: "", soulseek: [], youtube: [] },
  downloads: new Map(),
  queueOpen: false,
  mini: false,
  np: false,
  lyrics: { trackId: null, data: null },
  scan: null,
};

/* ---------- toasts ---------- */
function toast(kind, title, text) {
  const box = $("#toasts");
  const t = el("div", "toast pop-panel anim-in-top" + (kind === "error" ? " error" : kind === "warn" ? " warn" : ""));
  t.innerHTML = `${icon(kind === "error" ? "warning" : kind === "warn" ? "warning" : "check")}<div><b>${esc(title)}</b><p>${esc(text || "")}</p></div>`;
  box.appendChild(t);
  while (box.children.length > 3) box.removeChild(box.firstChild);
  setTimeout(() => closePop(t, "anim-out-top", true), 3400);
}

/* ---------- open and close motion for pop-ups ---------- */
function openPop(node, anim) {
  node.classList.remove("hidden", "closing");
  node.classList.remove("anim-in-top", "anim-in-right", "anim-in-centre");
  if (S.settings && S.settings.motion && S.settings.motion.enabled !== false) {
    void node.offsetWidth;
    node.classList.add("anim-in-" + anim);
  }
}
function closePop(node, anim, remove) {
  if (node.classList.contains("hidden")) return;
  const finish = () => { if (remove) node.remove(); else node.classList.add("hidden"); node.classList.remove("closing"); };
  if (S.settings && S.settings.motion && S.settings.motion.enabled !== false) {
    node.classList.add("closing");
    node.classList.remove("anim-in-top", "anim-in-right", "anim-in-centre");
    node.classList.add("anim-out-" + anim);
    setTimeout(finish, 160);
  } else finish();
}

/* ---------- popups: context menus, dropdown lists, dialogs ---------- */
const MENU_LAYER = () => $("#menuLayer");
function showMenu(items, x, y) {
  closeMenus();
  const layer = MENU_LAYER();
  layer.classList.remove("hidden");
  const m = el("div", "menu pop-panel");
  items.forEach((it) => {
    if (it === "sep") { m.appendChild(el("div", "menu-sep")); return; }
    const b = el("button", "menu-item");
    b.innerHTML = `${icon(it.ic || "info")}<span>${esc(it.label)}</span>${it.check ? icon("check", "chk") : ""}`;
    b.addEventListener("click", () => { closeMenus(); if (it.fn) it.fn(); });
    m.appendChild(b);
  });
  layer.appendChild(m);
  openPop(m, "top");
  const r = m.getBoundingClientRect();
  m.style.left = Math.min(Math.max(6, x), window.innerWidth - r.width - 6) + "px";
  m.style.top = Math.min(Math.max(6, y), window.innerHeight - r.height - 6) + "px";
  if (m.style.left !== Math.max(6, x) + "px") m.style.left = Math.max(6, x) + "px";
  return m;
}
function closeMenus() {
  const layer = MENU_LAYER();
  if (layer.classList.contains("hidden")) return;
  [...layer.children].forEach((m) => closePop(m, "top", true));
  setTimeout(() => layer.classList.add("hidden"), 180);
}
MENU_LAYER().addEventListener("mousedown", (e) => { if (e.target === MENU_LAYER()) closeMenus(); });

function showListbox(fieldNode, options, current, onPick) {
  closeListbox();
  const lb = el("div", "listbox pop-panel");
  options.forEach((opt) => {
    const o = el("div", "opt" + (opt === current ? " on" : ""));
    o.textContent = typeof opt === "string" ? opt : opt.label;
    o.addEventListener("click", () => { closeListbox(); onPick(opt); });
    lb.appendChild(o);
  });
  document.body.appendChild(lb);
  openPop(lb, "top");
  const fr = fieldNode.getBoundingClientRect();
  const lr = lb.getBoundingClientRect();
  let left = Math.min(fr.left, window.innerWidth - lr.width - 8);
  lb.style.left = Math.max(6, left) + "px";
  lb.style.top = Math.min(fr.bottom + 5, window.innerHeight - lr.height - 8) + "px";
  lb.dataset.owner = "1";
}
function closeListbox() {
  document.querySelectorAll(".listbox").forEach((lb) => closePop(lb, "top", true));
  document.querySelectorAll(".combo.open").forEach((c) => c.classList.remove("open"));
}
document.addEventListener("mousedown", (e) => {
  if (!e.target.closest(".listbox") && !e.target.closest(".combo")) closeListbox();
  if (!e.target.closest(".menu") && !e.target.closest(".menu-layer") === false) { /* handled by layer */ }
}, true);

/* dialog */
function showDialog(title, bodyHTML, actions) {
  closeDialog();
  const layer = $("#dialogLayer");
  layer.classList.remove("hidden");
  const d = el("div", "dialog pop-panel");
  d.innerHTML = `<h3>${esc(title)}</h3><div class="dlg-body">${bodyHTML}</div><div class="dlg-actions"></div>`;
  const act = d.querySelector(".dlg-actions");
  (actions || []).forEach((a) => {
    const b = el("button", "btn" + (a.primary ? " primary" : ""));
    b.textContent = a.label;
    b.addEventListener("click", () => { if (!a.fn || a.fn(d) !== false) closeDialog(); });
    act.appendChild(b);
  });
  layer.appendChild(d);
  openPop(d, "centre");
  return d;
}
function closeDialog() {
  const layer = $("#dialogLayer");
  if (layer.classList.contains("hidden")) return;
  [...layer.children].forEach((d) => closePop(d, "centre", true));
  setTimeout(() => layer.classList.add("hidden"), 180);
}
$("#dialogLayer").addEventListener("mousedown", (e) => { if (e.target === e.currentTarget) closeDialog(); });

/* ---------- shared builders ---------- */
function artImg(src, cls, iconCls) {
  if (src) {
    const im = el("img", cls);
    im.loading = "lazy";
    im.alt = "";
    im.src = src;
    im.addEventListener("error", () => { const d = el("div", cls + " ph", icon(iconCls || "songs")); im.replaceWith(d); });
    return im;
  }
  return el("div", cls + " ph", icon(iconCls || "songs"));
}
function trackRow(t, opts) {
  opts = opts || {};
  const r = el("div", "row");
  r.dataset.id = t.id;
  const num = el("div", "r-num", String(opts.num != null ? opts.num : ""));
  const art = artImg(artSrc(t.artworkId), "r-art", "songs");
  const titles = el("div", "r-titles");
  const name = el("div", "r-name", esc(t.title));
  const sub = el("div", "r-sub");
  sub.innerHTML = (t.artists || []).map((a) => `<span class="lnk" data-artist="${esc(a)}">${esc(a)}</span>`).join('<span class="sep">·</span>')
    + ((t.album && !opts.noAlbum) ? `<span class="sep">·</span><span class="lnk" data-album="${esc(t.albumKey)}">${esc(t.album)}</span>` : "");
  titles.append(name, sub);
  const meta = el("div", "r-meta", fmtTime(t.duration));
  r.append(opts.noNum ? art : num, art, titles, meta);
  if (t.love) {
    const lv = el("span", "badge-love");
    lv.innerHTML = icon("heart");
    r.appendChild(lv);
  }
  r.classList.toggle("playing", S.player.track && S.player.track.id === t.id);
  r.addEventListener("dblclick", () => playFromList(opts.listIds || [t.id], (opts.listIds || [t.id]).indexOf(t.id)));
  r.addEventListener("contextmenu", (e) => { e.preventDefault(); trackMenu([t.id], e.clientX, e.clientY, opts.ctx); });
  return r;
}
function tile(o) {
  const t = el("div", "tile eframe");
  const wrap = el("div", "art-wrap");
  const a = artImg(artSrc(o.art), "art", o.icon || "albums");
  wrap.appendChild(a);
  const play = el("button", "tileplay");
  play.innerHTML = icon("play");
  play.title = "Play " + o.name;
  play.setAttribute("aria-label", "Play " + o.name);
  play.addEventListener("click", (e) => { e.stopPropagation(); o.onPlay && o.onPlay(); });
  wrap.appendChild(play);
  t.appendChild(wrap);
  t.appendChild(el("div", "tile-name", esc(o.name)));
  t.appendChild(el("div", "tile-sub", esc(o.sub || "")));
  t.addEventListener("click", o.onOpen);
  t.addEventListener("contextmenu", (e) => { e.preventDefault(); o.onMenu && o.onMenu(e.clientX, e.clientY); });
  return t;
}
function stateBox(kind, iconName, title, text, actionLabel, onAction) {
  const box = el("div", "state-box" + (kind === "error" ? " error" : ""));
  box.innerHTML = `<div class="big">${icon(iconName)}</div><b>${esc(title)}</b><p>${esc(text)}</p>`;
  if (actionLabel) {
    const b = el("button", "btn primary", esc(actionLabel));
    b.addEventListener("click", onAction);
    box.appendChild(b);
  }
  return box;
}

/* ---------- play helpers ---------- */
async function playFromList(trackIds, startIndex) {
  await IPC.call("player.play", { trackIds, startIndex: startIndex || 0 });
}
async function playTracksOf(list) {
  await playFromList(list.map((t) => t.id), 0);
}
function trackMenu(ids, x, y, ctx) {
  const multi = ids.length > 1;
  const items = [
    { label: multi ? "Play now" : "Play now", ic: "play", fn: async () => { await playFromList(ids, 0); } },
    { label: "Play next", ic: "next", fn: () => IPC.call("player.queueAdd", { trackIds: ids, next: true }) },
    { label: "Add to queue", ic: "queueadd", fn: () => IPC.call("player.queueAdd", { trackIds: ids }) },
    "sep",
    { label: "Add to playlist", ic: "plus", fn: () => addToPlaylistMenu(ids, x, y) },
    { label: "Love", ic: "heart", fn: () => ids.forEach((id) => IPC.call("track.setLove", { id, love: true })) },
    { label: "Rate", ic: "star", fn: () => rateMenu(ids, x, y) },
  ];
  if (ctx && ctx.playlist) {
    items.push("sep", { label: "Remove from this playlist", ic: "x", fn: async () => {
      const pl = S.playlistsState && S.playlistsState.current;
      if (!pl) return;
      const idxs = ctx.indexes || [];
      await IPC.call("playlists.remove", { path: pl.path, indexes: idxs });
      refreshPlaylist(pl.path);
    } });
  }
  items.push("sep",
    { label: "Show in folder", ic: "folders", fn: () => IPC.call("track.showInFolder", { id: ids[0] }) },
    { label: "Delete", ic: "trash", fn: () => confirmDelete(ids) });
  showMenu(items, x, y);
}
function rateMenu(ids, x, y) {
  const items = [0, 1, 2, 3, 4, 5].map((n) => ({
    label: n === 0 ? "No rating" : n + (n === 1 ? " star" : " stars"), ic: "star",
    fn: () => ids.forEach((id) => IPC.call("track.setRating", { id, rating: n })),
  }));
  showMenu(items, x, y);
}
function addToPlaylistMenu(ids, x, y) {
  const items = (S.playlists || []).map((p) => ({
    label: p.name, ic: "playlists",
    fn: async () => { await IPC.call("playlists.add", { path: p.path, trackIds: ids }); toast("ok", "Added to playlist", p.name); },
  }));
  items.push("sep", { label: "New playlist", ic: "plus", fn: () => newPlaylistDialog(ids) });
  showMenu(items, x, y);
}
function confirmDelete(ids) {
  showDialog("Delete from library",
    `<p>Delete ${ids.length === 1 ? "this song" : ids.length + " songs"} from your library? The files move to the Recycle Bin.</p>`,
    [
      { label: "Cancel" },
      { label: "Delete", primary: true, fn: async () => { await IPC.call("track.delete", { ids }); S.selection.clear(); renderPage(); } },
    ]);
}
async function newPlaylistDialog(addIds) {
  const d = showDialog("New playlist",
    `<input class="txtbox" type="text" placeholder="Playlist name" spellcheck="false">`,
    [
      { label: "Cancel" },
      { label: "Create", primary: true, fn: async (dlg) => {
        const name = dlg.querySelector("input").value.trim();
        if (!name) return false;
        const res = await IPC.call("playlists.create", { name });
        await loadPlaylists();
        const p = res && res.path ? res : (S.playlists || []).find((p) => p.name === name);
        if (p && addIds && addIds.length) await IPC.call("playlists.add", { path: p.path, trackIds: addIds });
        nav("playlists", { path: p && p.path });
      } },
    ]);
  const inp = d.querySelector("input");
  inp.focus();
  inp.addEventListener("keydown", (e) => { if (e.key === "Enter") d.querySelector(".dlg-actions .btn.primary").click(); });
}

/* ---------- settings application ---------- */
function applySettings() {
  const st = S.settings || {};
  const r = document.documentElement;
  const ap = st.appearance || {};
  const fam = (ap.fontFamily || "FiraCode Nerd Font").replace(/"/g, "");
  r.style.setProperty("--font", `"${fam}", monospace`);
  r.style.setProperty("--enh-font-weight", String(ap.fontWeight || 500));
  r.style.setProperty("--blur", (ap.blur || 22) + "px");
  const mo = st.motion || {};
  r.dataset.motion = mo.enabled === false ? "off" : "on";
  r.dataset.speed = mo.speed && mo.speed !== "normal" ? mo.speed : "";
}

/* ---------- tabs and router ---------- */
const TABS = [
  ["home", "Home", "home"], ["artists", "Artists", "artists"], ["genres", "Genres", "genres"],
  ["albums", "Albums", "albums"], ["songs", "Songs", "songs"], ["playlists", "Playlists", "playlists"],
  ["folders", "Folders", "folders"], ["settings", "Settings", "settings"],
];
function buildTabs() {
  const tabs = $("#tabs");
  tabs.innerHTML = "";
  TABS.forEach(([id, label, ic]) => {
    const b = el("button", "tab");
    b.dataset.page = id;
    b.innerHTML = `${icon(ic)}<span>${label}</span>`;
    b.addEventListener("click", () => nav(id));
    tabs.appendChild(b);
  });
}
function markTabs() {
  document.querySelectorAll("#tabs .tab").forEach((b) => b.classList.toggle("on", b.dataset.page === S.page));
}
function nav(page, params, replace) {
  if (!replace) S.navStack.push({ page: S.page, params: S.params });
  S.page = page; S.params = params || {};
  S.renderToken = (S.renderToken || 0) + 1;
  markTabs();
  renderPage();
}
function goBack() {
  const prev = S.navStack.pop() || { page: "home", params: {} };
  S.page = prev.page; S.params = prev.params;
  S.renderToken = (S.renderToken || 0) + 1;
  markTabs();
  renderPage();
}
function renderPage() {
  const root = $("#page");
  root.className = "page";
  root.innerHTML = "";
  const token = S.renderToken;
  const fn = {
    home: screenHome, artists: screenArtists, genres: screenGenres, albums: screenAlbums,
    songs: screenSongs, playlists: screenPlaylists, folders: screenFolders, settings: screenSettings,
    search: screenSearch, artist: screenArtist, album: screenAlbum, genre: screenGenre,
    playlist: screenPlaylist,
  }[S.page] || screenHome;
  fn(root, token).catch((err) => {
    if (token !== S.renderToken) return;
    root.innerHTML = "";
    root.appendChild(stateBox("error", "warning", "This page did not load", String(err && err.message || err), "Try again", () => renderPage()));
  });
}
function pageHead(title, sub, tools) {
  const h = el("div", "page-head");
  if (S.navStack.length && ["artist", "album", "genre", "playlist"].includes(S.page)) {
    const back = el("button", "backbtn", icon("chevron-left"));
    back.title = "Back";
    back.setAttribute("aria-label", "Back");
    back.addEventListener("click", goBack);
    h.appendChild(back);
  }
  const t = el("div", "page-head-text");
  t.appendChild(el("div", "page-title", esc(title)));
  if (sub) t.appendChild(el("div", "page-sub", sub));
  h.appendChild(t);
  if (tools) { const w = el("div", "page-head-tools"); tools.forEach((n) => w.appendChild(n)); h.appendChild(w); }
  return h;
}

/* ---------- library data ---------- */
async function loadPlaylists() { const r = await IPC.call("playlists.list"); S.playlists = r || []; return S.playlists; }

/* ---------- home ---------- */
async function screenHome(root, token) {
  const ov = S.overview || await IPC.call("library.overview");
  if (token !== S.renderToken) return;
  S.overview = ov;
  root.appendChild(pageHead("Home", ov && ov.tracks ? fmtCount(ov.tracks) + " songs ready" : ""));
  if (!ov || !ov.tracks) {
    root.appendChild(stateBox("info", "home", "Your library is empty",
      "No songs yet. Add the folder where your music lives and it fills up on its own.",
      "Add a music folder", () => nav("folders")));
    return;
  }
  const stats = el("div", "stats");
  [[ov.tracks, "songs"], [ov.artists, "artists"], [ov.albums, "albums"], [ov.playlists, "playlists"]].forEach(([n, l]) => {
    const s = el("div", "stat");
    s.appendChild(el("div", "stat-value", fmtCount(n)));
    s.appendChild(el("div", "stat-label", l));
    stats.appendChild(s);
  });
  root.appendChild(stats);

  const radio = el("div", "radio-line block");
  radio.innerHTML = `${icon("radio")}<span>Start a radio from anything you like, it plays songs close to what you listen to.</span>`;
  const rb = el("button", "btn small", "Start radio");
  rb.addEventListener("click", () => IPC.call("player.radio", {}));
  radio.appendChild(rb);
  root.appendChild(radio);

  const albums = await IPC.call("library.albums", { sort: "dateAdded" });
  if (token !== S.renderToken) return;
  if (albums && albums.length) {
    const blk = el("div", "block");
    const row = el("div", "h2-row");
    const h = el("div", "h2 lnk", "Recently added");
    h.addEventListener("click", () => nav("albums"));
    const more = el("button", "btn small", "All albums");
    more.addEventListener("click", () => nav("albums"));
    row.append(h, more);
    blk.appendChild(row);
    blk.appendChild(albumGrid(albums.slice(0, 12), token));
    root.appendChild(blk);
  }
  const top = await IPC.call("library.tracks", { sort: "playCount", offset: 0, limit: 8 });
  if (token !== S.renderToken) return;
  if (top && top.items && top.items.length) {
    const blk = el("div", "block");
    blk.appendChild(el("div", "h2", "Most played"));
    const rows = el("div", "rows");
    const ids = top.items.map((t) => t.id);
    top.items.forEach((t, i) => rows.appendChild(trackRow(t, { num: i + 1, listIds: ids })));
    blk.appendChild(rows);
    root.appendChild(blk);
  }
}
function albumGrid(albums, token) {
  const g = el("div", "grid");
  albums.forEach((a) => {
    g.appendChild(tile({
      name: a.title || a.name || "?", sub: (a.artists || a.artist || [""])[0] || (a.year ? String(a.year) : ""),
      art: a.artworkId, icon: "albums",
      onOpen: () => nav("album", { albumKey: a.albumKey || a.key || a.title }),
      onPlay: () => playFromList([a.albumKey], 0),
      onMenu: (x, y) => showMenu([
        { label: "Play now", ic: "play", fn: () => IPC.call("player.play", { albumKey: a.albumKey || a.key }) },
        { label: "Play next", ic: "next", fn: () => IPC.call("player.queueAdd", { trackIds: [a.albumKey], next: true }) },
        { label: "Show artist", ic: "artists", fn: () => a.artist && nav("artist", { name: a.artist }) },
      ], x, y),
    }));
  });
  return g;
}

/* ---------- artists / genres / albums screens ---------- */
async function screenArtists(root, token) {
  const list = await IPC.call("library.artists", { kind: "track" });
  if (token !== S.renderToken) return;
  root.appendChild(pageHead("Artists", list && list.length ? fmtCount(list.length) + " artists" : ""));
  if (!list || !list.length) return root.appendChild(emptyLibraryBox());
  const g = el("div", "grid");
  list.forEach((a) => g.appendChild(tile({
    name: a.name, sub: a.albumCount ? a.albumCount + " albums" : "",
    art: a.imageId || a.artworkId, icon: "artists",
    onOpen: () => nav("artist", { name: a.name }),
    onPlay: () => IPC.call("player.play", { artist: a.name }),
    onMenu: (x, y) => showMenu([
      { label: "Play now", ic: "play", fn: () => IPC.call("player.play", { artist: a.name }) },
      { label: "Play next", ic: "next", fn: () => IPC.call("player.queueAdd", { trackIds: [a.name], next: true }) },
    ], x, y),
  })));
  root.appendChild(g);
}
async function screenGenres(root, token) {
  const list = await IPC.call("library.genres");
  if (token !== S.renderToken) return;
  root.appendChild(pageHead("Genres", list && list.length ? fmtCount(list.length) + " genres" : ""));
  if (!list || !list.length) return root.appendChild(emptyLibraryBox());
  const g = el("div", "grid");
  list.forEach((gn) => g.appendChild(tile({
    name: gn.name, sub: gn.trackCount ? gn.trackCount + " songs" : "", art: gn.artworkId, icon: "genres",
    onOpen: () => nav("genre", { name: gn.name }),
    onPlay: () => IPC.call("player.play", { genre: gn.name }),
    onMenu: (x, y) => showMenu([
      { label: "Play now", ic: "play", fn: () => IPC.call("player.play", { genre: gn.name }) },
      { label: "Play next", ic: "next", fn: () => IPC.call("player.queueAdd", { trackIds: [gn.name], next: true }) },
    ], x, y),
  })));
  root.appendChild(g);
}
async function screenAlbums(root, token) {
  const list = await IPC.call("library.albums", { sort: "artist" });
  if (token !== S.renderToken) return;
  root.appendChild(pageHead("Albums", list && list.length ? fmtCount(list.length) + " albums" : ""));
  if (!list || !list.length) return root.appendChild(emptyLibraryBox());
  root.appendChild(albumGrid(list, token));
}
function emptyLibraryBox() {
  return stateBox("info", "songs", "Nothing here yet",
    "Your music folders are still being read, or none are set. Add a folder on the Folders page.",
    "Open Folders", () => nav("folders"));
}

/* ---------- songs table (virtualised) ---------- */
const SONG_COLS = [
  ["num", "#"], ["art", ""], ["title", "Title"], ["artist", "Artists"], ["album", "Album"],
  ["genres", "Genres"], ["year", "Year"], ["dur", "Time"], ["rating", "Rating"], ["plays", "Plays"], ["love", "Loved"],
];
const SORTABLE = { title: "Title", artist: "Artists", album: "Album", year: "Year", duration: "Time", rating: "Rating", playCount: "Plays", dateAdded: "Date added" };
const ROW_H = 30, PAGE_N = 200;

async function screenSongs(root, token) {
  S.selection.clear(); S.anchor = null;
  if (!S.songsCols) S.songsCols = ["num", "art", "title", "artist", "album", "genres", "year", "dur", "rating", "plays", "love"];
  S.songs = { total: 0, cache: new Map(), fetched: new Set() };

  const tools = el("div", "songs-tools");
  const shuffle = el("button", "btn", icon("shuffle") + "Shuffle all");
  shuffle.addEventListener("click", async () => {
    const all = await IPC.call("library.tracks", { sort: S.songsSort, offset: 0, limit: 50000 });
    if (all && all.items) playFromList(all.items.map((t) => t.id), Math.floor(Math.random() * all.items.length));
  });
  const colsBtn = el("button", "btn", icon("columns") + "Columns");
  colsBtn.addEventListener("click", columnPicker);
  const hint = el("span", "hint", "");
  tools.append(shuffle, colsBtn, hint);
  if (S.scan) hint.textContent = `Scanning: ${S.scan.done} of ${S.scan.total}`;

  const header = el("div", "tbl-head");
  const headMap = {};
  S.songsCols.forEach((c) => {
    const d = el("div");
    if (c === "art") { d.innerHTML = "&nbsp;"; d.style.flex = "0 0 34px"; }
    else {
      const key = { num: "title", artist: "artist", album: "album", genres: "title", year: "year", dur: "duration", rating: "rating", plays: "playCount", love: "title", title: "title" }[c];
      const label = (SONG_COLS.find(([k]) => k === c) || [, c])[1] || "Title";
      d.innerHTML = `${esc(label)}${S.songsSort === key && key !== "title" || (key === "title" && (S.songsSort === "title" || c === "num")) ? '<span class="arrow">' + (S.songsDir === "asc" ? "▲" : "▼") + "</span>" : ""}`;
      if (SORTABLE[key] && c !== "art" && c !== "love") {
        headMap[key] = d;
        d.title = "Sort by " + SORTABLE[key];
        d.addEventListener("click", () => {
          if (S.songsSort === key) S.songsDir = S.songsDir === "asc" ? "desc" : "asc";
          else { S.songsSort = key; S.songsDir = "asc"; }
          renderPage();
        });
        if (S.songsSort === key) d.classList.add("sort");
      }
    }
    header.appendChild(d);
  });

  const scroll = el("div", "tbl-scroll");
  const spacer = el("div", "tbl-spacer");
  const layer = el("div");
  scroll.append(spacer, layer);
  root.className = "page noscroll";
  root.append(tools, header, scroll);

  const first = await ensureSongs(0, 80, token);
  if (token !== S.renderToken) return;
  S.songs.total = first.total;
  spacer.style.height = (first.total * ROW_H) + "px";

  const render = () => {
    const st = scroll.scrollTop, vh = scroll.clientHeight || 600;
    const i0 = Math.max(0, Math.floor(st / ROW_H) - 10);
    const i1 = Math.floor((st + vh) / ROW_H) + 10;
    layer.style.transform = `translateY(${i0 * ROW_H}px)`;
    layer.innerHTML = "";
    for (let i = i0; i <= Math.min(i1, S.songs.total - 1); i++) {
      const t = S.songs.cache.get(i);
      if (!t) { ensureSongs(i, i + 1, token); break; }
      layer.appendChild(songRow(t, i));
    }
  };
  let raf = 0;
  scroll.addEventListener("scroll", () => { if (!raf) raf = requestAnimationFrame(() => { raf = 0; render(); }); });
  render();
  S.songsRender = render;
}
function songRow(t, i) {
  const r = el("div", "tbl-row");
  r.dataset.id = t.id; r.dataset.i = i;
  if (S.selection.has(t.id)) r.classList.add("sel");
  r.classList.toggle("playing", S.player.track && S.player.track.id === t.id);
  for (const c of S.songsCols) {
    const d = el("div");
    switch (c) {
      case "num": d.className = "c-num"; d.textContent = i + 1; break;
      case "art": {
        d.style.flex = "0 0 34px";
        if (t.artworkId) { const im = el("img", "art-mini"); im.loading = "lazy"; im.alt = ""; im.src = artSrc(t.artworkId); d.appendChild(im); }
        else d.innerHTML = icon("songs");
        break;
      }
      case "title": d.className = "c-title"; d.textContent = t.title; break;
      case "artist": d.textContent = (t.artists || []).join(", "); break;
      case "album": d.textContent = t.album || ""; break;
      case "genres": d.textContent = (t.genres || []).join(", "); break;
      case "year": d.textContent = t.year || ""; break;
      case "dur": d.className = "c-dur"; d.textContent = fmtTime(t.duration); break;
      case "rating": {
        d.className = "c-rate";
        d.innerHTML = ratingStars(t.rating, true);
        break;
      }
      case "plays": d.className = "c-plays"; d.textContent = fmtPlays(t.playCount); break;
      case "love": d.className = "c-love"; d.innerHTML = t.love ? icon("heart", "badge-love") : ""; break;
    }
    r.appendChild(d);
  }
  r.addEventListener("click", (e) => songsClick(e, i, t.id));
  r.addEventListener("dblclick", () => playFromList(null, i));
  r.addEventListener("contextmenu", (e) => {
    e.preventDefault();
    if (!S.selection.has(t.id)) { S.selection.clear(); S.selection.add(t.id); S.anchor = i; S.songsRender && S.songsRender(); }
    const ctx = { playlist: false };
    trackMenu([...S.selection], e.clientX, e.clientY, ctx);
  });
  return r;
}
function ratingStars(n, dim) {
  let out = "";
  for (let k = 1; k <= 5; k++) out += `<span class="star ${k <= n ? "on" : ""}" style="${dim ? "width:11px;height:11px" : ""}">${icon("star")}</span>`;
  return out;
}
async function ensureSongs(from, to, token) {
  const firstPage = Math.floor(from / PAGE_N), lastPage = Math.floor(to / PAGE_N);
  let total = S.songs.total;
  for (let p = firstPage; p <= lastPage; p++) {
    if (S.songs.fetched.has(p)) continue;
    S.songs.fetched.add(p);
    const res = await IPC.call("library.tracks", { sort: S.songsSort + ":" + S.songsDir, offset: p * PAGE_N, limit: PAGE_N });
    if (token !== undefined && token !== S.renderToken) return { total: 0 };
    if (!res) { S.songs.fetched.delete(p); continue; }
    total = res.total || total;
    (res.items || []).forEach((t, k) => S.songs.cache.set(p * PAGE_N + k, t));
    if (S.songsRender) S.songsRender();
  }
  S.songs.total = total || S.songs.total;
  return { total: S.songs.total };
}
function songsClick(e, i, id) {
  if (e.shiftKey && S.anchor != null) {
    const a = Math.min(S.anchor, i), b = Math.max(S.anchor, i);
    S.selection.clear();
    for (let k = a; k <= b; k++) { const t = S.songs.cache.get(k); if (t) S.selection.add(t.id); }
  } else if (e.ctrlKey || e.metaKey) {
    if (S.selection.has(id)) S.selection.delete(id); else S.selection.add(id);
    S.anchor = i;
  } else {
    S.selection.clear(); S.selection.add(id); S.anchor = i;
  }
  S.songsRender && S.songsRender();
}
function playFromList(trackIds, startIndex) {
  if (trackIds == null) {
    const ids = [];
    for (let k = 0; k < S.songs.total; k++) { const t = S.songs.cache.get(k); if (t) ids.push(t.id); }
    trackIds = ids;
  }
  return IPC.call("player.play", { trackIds, startIndex: startIndex || 0 });
}
function columnPicker() {
  const opts = SONG_COLS.filter(([k]) => k !== "num");
  const body = `<div class="col-list">${opts.map(([k, label]) => `
    <div class="col-opt ${S.songsCols.includes(k) ? "on" : ""}" data-col="${k}">
      <span class="box">${S.songsCols.includes(k) ? icon("check") : ""}</span><span>${esc(label || "Artwork")}</span>
    </div>`).join("")}</div>`;
  showDialog("Columns in the songs table", body, [{ label: "Done", primary: true }]);
  $("#dialogLayer").addEventListener("click", (e) => {
    const opt = e.target.closest(".col-opt");
    if (!opt) return;
    const k = opt.dataset.col;
    const i = S.songsCols.indexOf(k);
    if (i >= 0) S.songsCols.splice(i, 1); else S.songsCols.push(k);
    opt.classList.toggle("on");
    opt.querySelector(".box").innerHTML = opt.classList.contains("on") ? icon("check") : "";
  }, { once: false });
  const h = () => { closeDialog(); $("#dialogLayer").removeEventListener("mousedown", h); };
  $("#dialogLayer").addEventListener("mousedown", (e) => { if (e.target === e.currentTarget) h(); });
}

/* ---------- artist / album / genre pages ---------- */
async function screenArtist(root, token) {
  const name = S.params.name;
  root.appendChild(pageHead(name || "Artist", "Loading…"));
  const res = await IPC.call("library.artist", { name });
  if (token !== S.renderToken) return;
  root.innerHTML = "";
  if (!res) return root.appendChild(stateBox("error", "artists", "Artist not found", "The library moved on since you opened this page.", "Back", goBack));
  const banner = el("div", "banner");
  const art = artImg(artSrc(res.imageId || (res.albums && res.albums[0] && res.albums[0].artworkId)), "banner-art", "artists");
  banner.appendChild(art);
  const text = el("div", "banner-text");
  text.appendChild(el("span", "banner-tag", "Artist"));
  text.appendChild(el("div", "banner-name", esc(name)));
  const sub = el("div", "banner-sub");
  sub.innerHTML = `<span>${(res.albums || []).length} albums</span><span class="sep">·</span><span>${(res.tracks || []).length} songs</span>`;
  text.appendChild(sub);
  const actions = el("div", "banner-actions");
  const play = el("button", "btn primary", icon("play") + "Play");
  play.addEventListener("click", () => IPC.call("player.play", { artist: name }));
  const shuffle = el("button", "btn", icon("shuffle") + "Shuffle");
  shuffle.addEventListener("click", async () => {
    const ids = (res.tracks || []).map((t) => t.id);
    playFromList(ids, Math.floor(Math.random() * Math.max(1, ids.length)));
  });
  const radio = el("button", "btn", icon("radio") + "Radio");
  radio.addEventListener("click", () => IPC.call("player.radio", { artist: name }));
  actions.append(play, shuffle, radio);
  text.appendChild(actions);
  banner.appendChild(text);
  root.appendChild(banner);

  if (res.albums && res.albums.length) {
    const blk = el("div", "block");
    blk.appendChild(el("div", "h2", "Albums"));
    blk.appendChild(albumGrid(res.albums, token));
    root.appendChild(blk);
  }
  if (res.tracks && res.tracks.length) {
    const blk = el("div", "block");
    blk.appendChild(el("div", "h2", "Songs"));
    const rows = el("div", "rows");
    const ids = res.tracks.map((t) => t.id);
    res.tracks.forEach((t, i) => rows.appendChild(trackRow(t, { num: i + 1, listIds: ids })));
    blk.appendChild(rows);
    root.appendChild(blk);
  }
}
async function screenAlbum(root, token) {
  const key = S.params.albumKey;
  const res = await IPC.call("library.album", { albumKey: key });
  if (token !== S.renderToken) return;
  if (!res || !res.album) return root.appendChild(stateBox("error", "albums", "Album not found", "The library moved on since you opened this page.", "Back", goBack));
  const a = res.album;
  root.innerHTML = "";
  const banner = el("div", "banner");
  banner.appendChild(artImg(artSrc(a.artworkId), "banner-art", "albums"));
  const text = el("div", "banner-text");
  text.appendChild(el("span", "banner-tag", "Album"));
  text.appendChild(el("div", "banner-name", esc(a.title || key)));
  const artist = (a.artists || a.albumArtists || [""])[0] || "";
  const sub = el("div", "banner-sub");
  sub.innerHTML = `<span class="lnk" data-artist="${esc(artist)}">${esc(artist)}</span>` +
    (a.year ? `<span class="sep">·</span><span>${esc(a.year)}</span>` : "") +
    `<span class="sep">·</span><span>${(res.tracks || []).length} songs</span>`;
  sub.querySelector(".lnk").addEventListener("click", () => artist && nav("artist", { name: artist }));
  text.appendChild(sub);
  const actions = el("div", "banner-actions");
  const play = el("button", "btn primary", icon("play") + "Play");
  play.addEventListener("click", () => playTracksOf(res.tracks || []));
  const shuffle = el("button", "btn", icon("shuffle") + "Shuffle");
  shuffle.addEventListener("click", () => {
    const ids = (res.tracks || []).map((t) => t.id);
    playFromList(ids, Math.floor(Math.random() * Math.max(1, ids.length)));
  });
  actions.append(play, shuffle);
  text.appendChild(actions);
  banner.appendChild(text);
  root.appendChild(banner);

  const rows = el("div", "rows block");
  const ids = (res.tracks || []).map((t) => t.id);
  (res.tracks || []).forEach((t, i) => rows.appendChild(trackRow(t, { num: i + 1, listIds: ids, noAlbum: true })));
  root.appendChild(rows);
}
async function screenGenre(root, token) {
  const name = S.params.name;
  const albums = await IPC.call("library.albums", { sort: "artist", genre: name });
  const tracks = await IPC.call("library.tracks", { sort: "playCount:desc", offset: 0, limit: 10, filter: { genre: name } });
  if (token !== S.renderToken) return;
  root.innerHTML = "";
  const banner = el("div", "banner");
  const text = el("div", "banner-text");
  text.appendChild(el("span", "banner-tag", "Genre"));
  text.appendChild(el("div", "banner-name", esc(name)));
  const actions = el("div", "banner-actions");
  const play = el("button", "btn primary", icon("play") + "Play");
  play.addEventListener("click", () => IPC.call("player.play", { genre: name }));
  const radio = el("button", "btn", icon("radio") + "Radio");
  radio.addEventListener("click", () => IPC.call("player.radio", { genre: name }));
  actions.append(play, radio);
  text.appendChild(actions);
  banner.appendChild(text);
  root.appendChild(banner);
  if (albums && albums.length) {
    const blk = el("div", "block");
    blk.appendChild(el("div", "h2", "Albums"));
    blk.appendChild(albumGrid(albums, token));
    root.appendChild(blk);
  }
  if (tracks && tracks.items && tracks.items.length) {
    const blk = el("div", "block");
    blk.appendChild(el("div", "h2", "Top songs"));
    const rows = el("div", "rows");
    const ids = tracks.items.map((t) => t.id);
    tracks.items.forEach((t, i) => rows.appendChild(trackRow(t, { num: i + 1, listIds: ids })));
    blk.appendChild(rows);
    root.appendChild(blk);
  }
}

/* ---------- playlists ---------- */
async function screenPlaylists(root, token) {
  const list = await loadPlaylists();
  if (token !== S.renderToken) return;
  root.appendChild(pageHead("Playlists", list.length ? list.length + " playlists" : ""));
  if (!list.length) {
    root.appendChild(stateBox("info", "playlists", "No playlists yet",
      "Make one, then right click any song to add it.", "New playlist", () => newPlaylistDialog()));
    return;
  }
  const wrap = el("div", "top-split");
  const left = el("div", "block--half");
  left.appendChild(el("div", "h2", "Your playlists"));
  const rows = el("div", "rows");
  list.forEach((p) => {
    const r = el("div", "row");
    r.style.cursor = "pointer";
    r.innerHTML = `${icon("playlists")}<div class="r-titles"><div class="r-name">${esc(p.name)}</div><div class="r-sub">${p.trackCount || 0} songs</div></div>`;
    r.addEventListener("click", () => nav("playlist", { path: p.path }));
    r.addEventListener("contextmenu", (e) => {
      e.preventDefault();
      showMenu([
        { label: "Open", ic: "chevron-right", fn: () => nav("playlist", { path: p.path }) },
        { label: "Rename", ic: "edit", fn: () => renamePlaylist(p) },
        { label: "Delete", ic: "trash", fn: () => confirmPlaylistDelete(p) },
      ], e.clientX, e.clientY);
    });
    rows.appendChild(r);
  });
  left.appendChild(rows);
  const newBtn = el("button", "btn", icon("plus") + "New playlist");
  newBtn.addEventListener("click", () => newPlaylistDialog());
  left.appendChild(newBtn);
  wrap.appendChild(left);
  root.appendChild(wrap);
}
async function screenPlaylist(root, token) {
  const path = S.params.path;
  const list = S.playlists || [];
  const p = list.find((x) => x.path === path) || { name: path, path, trackCount: 0 };
  root.appendChild(pageHead(p.name, "Loading…"));
  const res = await IPC.call("playlists.tracks", { path });
  if (token !== S.renderToken) return;
  root.innerHTML = "";
  S.playlistsState = { current: p, tracks: res || [] };
  const tracks = res || [];
  const banner = el("div", "banner");
  const text = el("div", "banner-text");
  text.appendChild(el("span", "banner-tag", "Playlist"));
  text.appendChild(el("div", "banner-name", esc(p.name)));
  text.appendChild(el("div", "banner-sub", `${tracks.length} songs`));
  const actions = el("div", "banner-actions");
  const play = el("button", "btn primary", icon("play") + "Play");
  play.addEventListener("click", () => playTracksOf(tracks));
  const ren = el("button", "btn", icon("edit") + "Rename");
  ren.addEventListener("click", () => renamePlaylist(p));
  const del = el("button", "btn", icon("trash") + "Delete");
  del.addEventListener("click", () => confirmPlaylistDelete(p));
  actions.append(play, ren, del);
  text.appendChild(actions);
  banner.appendChild(text);
  root.appendChild(banner);
  if (!tracks.length) {
    root.appendChild(stateBox("info", "playlists", "This playlist is empty",
      "Right click songs in your library and choose Add to playlist.", "Back to playlists", () => nav("playlists")));
    return;
  }
  const rows = el("div", "rows");
  const ids = tracks.map((t) => t.id);
  tracks.forEach((t, i) => {
    const r = trackRow(t, { num: i + 1, listIds: ids, noAlbum: false, ctx: { playlist: true, indexes: [i] } });
    r.draggable = true;
    r.addEventListener("dragstart", (e) => { e.dataTransfer.setData("text/plain", String(i)); r.classList.add("dragging"); });
    r.addEventListener("dragend", () => r.classList.remove("dragging"));
    r.addEventListener("dragover", (e) => { e.preventDefault(); r.classList.add("sel"); });
    r.addEventListener("dragleave", () => r.classList.remove("sel"));
    r.addEventListener("drop", async (e) => {
      e.preventDefault();
      r.classList.remove("sel");
      const from = parseInt(e.dataTransfer.getData("text/plain"), 10);
      const to = i;
      if (isNaN(from) || from === to) return;
      await IPC.call("playlists.reorder", { path, from, to });
      refreshPlaylist(path);
    });
    rows.appendChild(r);
  });
  root.appendChild(rows);
}
function refreshPlaylist(path) { nav("playlist", { path }, true); }
async function renamePlaylist(p) {
  const d = showDialog("Rename playlist", `<input class="txtbox" type="text" value="${esc(p.name)}" spellcheck="false">`,
    [{ label: "Cancel" }, { label: "Rename", primary: true, fn: async (dlg) => {
      const name = dlg.querySelector("input").value.trim();
      if (!name) return false;
      await IPC.call("playlists.rename", { path: p.path, name });
      await loadPlaylists(); renderPage();
    } }]);
  const inp = d.querySelector("input"); inp.focus(); inp.select();
  inp.addEventListener("keydown", (e) => { if (e.key === "Enter") d.querySelector(".primary").click(); });
}
function confirmPlaylistDelete(p) {
  showDialog("Delete playlist", `<p>Delete "${esc(p.name)}"? Your songs stay in the library.</p>`,
    [{ label: "Cancel" }, { label: "Delete", primary: true, fn: async () => {
      await IPC.call("playlists.delete", { path: p.path }); await loadPlaylists(); nav("playlists");
    } }]);
}

/* ---------- folders ---------- */
async function screenFolders(root, token) {
  const folders = await IPC.call("library.folders");
  if (token !== S.renderToken) return;
  root.appendChild(pageHead("Folders", "The folders your music comes from"));
  const path = S.params.path;
  if (path) {
    const res = await IPC.call("library.folder", { path });
    if (token !== S.renderToken) return;
    const crumbs = el("div", "songs-tools");
    const up = el("button", "btn small", icon("chevron-left") + esc(path.split("\\").slice(-1)[0] || path));
    up.addEventListener("click", () => nav("folders"));
    crumbs.appendChild(up);
    root.appendChild(crumbs);
    if (res && res.subfolders && res.subfolders.length) {
      const rows = el("div", "folder-list");
      res.subfolders.forEach((sf) => {
        const r = el("div", "folder-row");
        const full = path.endsWith("\\") ? path + sf : path + "\\" + sf;
        r.innerHTML = `${icon("folders")}<span class="folder-path">${esc(sf)}</span>`;
        r.addEventListener("click", () => nav("folders", { path: full }, true));
        rows.appendChild(r);
      });
      root.appendChild(rows);
    }
    if (res && res.tracks && res.tracks.length) {
      const rows = el("div", "rows");
      const ids = res.tracks.map((t) => t.id);
      res.tracks.forEach((t, i) => rows.appendChild(trackRow(t, { num: i + 1, listIds: ids })));
      root.appendChild(rows);
    }
    if ((!res || !res.subfolders || !res.subfolders.length) && (!res || !res.tracks || !res.tracks.length)) {
      root.appendChild(stateBox("info", "folders", "Empty folder", "No songs in this folder.", "Back", () => nav("folders")));
    }
    return;
  }
  const tools = el("div", "songs-tools");
  const add = el("button", "btn primary", icon("folderplus") + "Add a folder");
  add.addEventListener("click", async () => {
    const res = await IPC.call("library.addFolder", {});
    if (res && res.path) toast("ok", "Folder added", res.path);
    renderPage();
  });
  const scan = el("button", "btn", icon("sort") + "Rescan now");
  scan.addEventListener("click", async () => { await IPC.call("library.rescan", {}); toast("ok", "Rescan started", "New songs show up as they are found."); });
  tools.append(add, scan);
  root.appendChild(tools);
  if (!folders || !folders.length) {
    root.appendChild(stateBox("info", "folderplus", "No folders yet",
      "Add the folder where your music lives. Files there show up in the library, and new ones are picked up on their own.",
      "Add a folder", () => add.click()));
    return;
  }
  const rows = el("div", "folder-list");
  folders.forEach((f) => {
    const r = el("div", "folder-row");
    r.innerHTML = `${icon("folders")}<span class="folder-path">${esc(f.path || f.name)}</span>`;
    const tools2 = el("div", "folder-tools");
    const open = el("button", "iconbtn tiny", icon("chevron-right"));
    open.title = "Browse this folder";
    open.addEventListener("click", (e) => { e.stopPropagation(); nav("folders", { path: f.path }, true); });
    const rm = el("button", "iconbtn tiny", icon("trash"));
    rm.title = "Stop watching this folder";
    rm.addEventListener("click", async (e) => {
      e.stopPropagation();
      await IPC.call("library.removeFolder", { id: f.id });
      toast("ok", "Folder removed", "Its songs stay out of the library after the next scan.");
      renderPage();
    });
    tools2.append(open, rm);
    r.appendChild(tools2);
    rows.appendChild(r);
  });
  root.appendChild(rows);
}

/* ---------- search screen and online panel ---------- */
async function screenSearch(root, token) {
  const q = S.params.q || "";
  root.appendChild(pageHead(q ? `Results for "${q}"` : "Search", q ? "From your library" : "Type in the search box above"));
  if (!q) return;
  const res = await IPC.call("library.search", { q });
  if (token !== S.renderToken) return;
  if (!res || (!res.tracks.length && !res.albums.length && !res.artists.length)) {
    root.appendChild(stateBox("info", "search", "Nothing in your library",
      "No song, album or artist matches. The Online panel on the right looks further.", "Clear search", () => clearSearch()));
    return;
  }
  if (res.tracks.length) {
    const blk = el("div", "block");
    blk.appendChild(el("div", "h2", "Songs"));
    const rows = el("div", "rows");
    const ids = res.tracks.map((t) => t.id);
    res.tracks.slice(0, 50).forEach((t, i) => rows.appendChild(trackRow(t, { num: i + 1, listIds: ids })));
    blk.appendChild(rows);
    root.appendChild(blk);
  }
  if (res.albums.length) {
    const blk = el("div", "block");
    blk.appendChild(el("div", "h2", "Albums"));
    blk.appendChild(albumGrid(res.albums, token));
    root.appendChild(blk);
  }
  if (res.artists.length) {
    const blk = el("div", "block");
    blk.appendChild(el("div", "h2", "Artists"));
    const g = el("div", "grid");
    res.artists.forEach((a) => g.appendChild(tile({
      name: a.name, sub: "", art: a.imageId || a.artworkId, icon: "artists",
      onOpen: () => nav("artist", { name: a.name }), onPlay: () => IPC.call("player.play", { artist: a.name }),
    })));
    blk.appendChild(g);
    root.appendChild(blk);
  }
}

let onlineTimer = 0;
function onSearchInput() {
  const inp = $("#searchInput");
  const q = inp.value.trim();
  $("#searchClear").classList.toggle("hidden", !q);
  $("#searchBox").classList.toggle("has-text", !!q);
  if (!q) { clearSearchResults(); return; }
  clearTimeout(onlineTimer);
  onlineTimer = setTimeout(() => runOnlineSearch(q), 350);
  if (S.page !== "search") nav("search", { q }, true);
  else { S.params.q = q; }
}
function clearSearchResults() {
  S.online.soulseek = []; S.online.youtube = [];
  if (S.page === "search") { S.params.q = ""; renderPage(); }
  closeOnline();
}
function clearSearch() {
  $("#searchInput").value = "";
  $("#searchClear").classList.add("hidden");
  $("#searchBox").classList.remove("has-text");
  clearSearchResults();
  goBack();
}
async function runOnlineSearch(q) {
  S.online.q = q;
  S.online.busy = true;
  $("#onlineSpin").classList.remove("hidden");
  openOnline();
  $("#onlineBody").innerHTML = "";
  $("#onlineBody").appendChild(el("div", "online-note", "Looking online…"));
  const res = await IPC.call("online.search", { q });
  if (res === null && S.online.busy) {
    $("#onlineBody").innerHTML = "";
    $("#onlineBody").appendChild(stateBox("error", "warning", "Could not reach the online sources",
      "slskd or the video search did not answer. Check they are running, then search again.", "Try again", () => runOnlineSearch(q)));
  }
  S.online.busy = false;
  $("#onlineSpin").classList.add("hidden");
}
function openOnline() {
  S.online.open = true;
  ensureUnderlay("#onlinePanel", ".online-underlay");
  openPop($("#onlinePanel"), "right");
}
function closeOnline() {
  if (!S.online.open) return;
  S.online.open = false;
  closePop($("#onlinePanel"), "right");
  const u = document.querySelector(".online-underlay");
  if (u) closePop(u, "right", true);
}
function ensureUnderlay(panelSel, cls) {
  const panel = $(panelSel);
  if (document.querySelector(cls)) return;
  const u = el("div", cls.slice(1) + " anim-in-right");
  const pr = () => { const a = panel.getBoundingClientRect(), b = u.getBoundingClientRect(); return Math.abs(a.top - b.top) < 2 && Math.abs(a.width - b.width) < 2; };
  document.body.appendChild(u);
  u.getBoundingClientRect(); pr();
}
function renderOnline() {
  const body = $("#onlineBody");
  if (!S.online.open) return;
  body.innerHTML = "";
  const groups = [["Soulseek", S.online.soulseek], ["YouTube", S.online.youtube]];
  let any = false;
  groups.forEach(([label, items]) => {
    if (!items || !items.length) return;
    any = true;
    body.appendChild(el("div", "online-section", label));
    const rows = el("div", "rows");
    items.forEach((it) => rows.appendChild(onlineRow(it)));
    body.appendChild(rows);
  });
  if (!any) {
    body.appendChild(stateBox("info", "stream", "Nothing found yet",
      S.online.busy ? "Still looking." : "No online results for this search. Try different words."));
  }
}
function onlineRow(it) {
  const r = el("div", "row online-row");
  const art = artImg(artSrc(it.artworkId), "r-art", it.kind === "album" ? "albums" : "songs");
  const titles = el("div", "r-titles");
  titles.appendChild(el("div", "r-name", esc(it.title || it.name || "?")));
  const sub = el("div", "r-sub");
  sub.innerHTML = [it.artist, it.album, it.user, it.duration ? fmtTime(it.duration) : ""].filter(Boolean).map(esc).join('<span class="sep">·</span>');
  titles.appendChild(sub);
  const dl = S.downloads.get(it.resultId);
  if (it.bitrate || it.lossless) {
    const chip = el("span", "qchip" + (it.lossless ? " lossless" : ""), esc(it.lossless ? "lossless" : it.bitrate + " kbps"));
    r.append(art, titles, chip);
  } else r.append(art, titles);
  const icons = el("div", "r-icons");
  const play = el("button", "iconbtn", icon("stream"));
  play.title = "Stream now";
  play.addEventListener("click", () => IPC.call("online.stream", { resultId: it.resultId }));
  const get = el("button", "iconbtn", icon("download"));
  get.title = "Download";
  if (dl) {
    if (dl.pct != null && dl.pct >= 0) {
      get.innerHTML = `<span class="percent">${dl.pct}%</span>`;
      get.classList.add("on");
    } else {
      get.innerHTML = icon("check");
      get.classList.add("on");
    }
  }
  get.addEventListener("click", async () => {
    if (S.downloads.get(it.resultId)) return;
    get.innerHTML = "";
    get.appendChild(el("span", "spin small"));
    const res = await IPC.call("online.download", { resultId: it.resultId });
    if (res && res.jobId) S.downloads.set(it.resultId, { jobId: res.jobId, pct: -1 });
    else if (res === null) toast("error", "Download did not start", "The source did not answer.");
  });
  icons.append(play, get);
  r.appendChild(icons);
  r.addEventListener("dblclick", () => IPC.call("online.stream", { resultId: it.resultId }));
  return r;
}

/* ---------- queue drawer ---------- */
async function toggleQueue(force) {
  const want = force != null ? force : !S.queueOpen;
  if (want === S.queueOpen) return;
  S.queueOpen = want;
  $("#queueBtn").classList.toggle("on", S.queueOpen);
  if (S.queueOpen) {
    await renderQueue();
    ensureUnderlay("#queueDrawer", ".drawer-underlay");
    openPop($("#queueDrawer"), "right");
  } else {
    closePop($("#queueDrawer"), "right");
    const u = document.querySelector(".drawer-underlay");
    if (u) closePop(u, "right", true);
  }
}
async function renderQueue() {
  const res = await IPC.call("player.queue", {});
  if (!res) return;
  S.queue = res;
  $("#queueCount").textContent = res.items.length ? res.items.length + " songs" : "";
  const body = $("#queueBody");
  body.innerHTML = "";
  if (!res.items.length) {
    body.appendChild(stateBox("info", "list", "The queue is empty", "Songs you play next land here.", null));
    return;
  }
  res.items.forEach((t, i) => {
    const r = el("div", "row");
    r.classList.toggle("playing", i === res.index);
    r.draggable = true;
    r.innerHTML = `<div class="r-num">${i + 1}</div>
      <div class="r-titles"><div class="r-name">${esc(t.title)}</div><div class="r-sub">${esc((t.artists || []).join(", "))}</div></div>
      <div class="r-meta">${fmtTime(t.duration)}</div>`;
    r.addEventListener("dblclick", () => IPC.call("player.play", { trackIds: res.items.map((x) => x.id), startIndex: i }));
    r.addEventListener("contextmenu", (e) => {
      e.preventDefault();
      showMenu([
        { label: "Play now", ic: "play", fn: () => IPC.call("player.play", { trackIds: res.items.map((x) => x.id), startIndex: i }) },
        { label: "Remove from queue", ic: "x", fn: async () => { await IPC.call("player.queueRemove", { indexes: [i] }); renderQueue(); } },
      ], e.clientX, e.clientY);
    });
    r.addEventListener("dragstart", (e) => e.dataTransfer.setData("text/plain", String(i)));
    r.addEventListener("dragover", (e) => e.preventDefault());
    r.addEventListener("drop", async (e) => {
      e.preventDefault();
      const from = parseInt(e.dataTransfer.getData("text/plain"), 10);
      if (!isNaN(from) && from !== i) { await IPC.call("player.queueMove", { from, to: i }); renderQueue(); }
    });
    body.appendChild(r);
  });
}

/* ---------- settings screen ---------- */
function setSetting(key, value) {
  const keys = key.split(".");
  let o = S.settings;
  for (let i = 0; i < keys.length - 1; i++) { o[keys[i]] = o[keys[i]] || {}; o = o[keys[i]]; }
  o[keys[keys.length - 1]] = value;
  IPC.call("settings.set", { key, value });
  applySettings();
}
function mkToggle(get, set) {
  const t = el("button", "tog" + (get() ? " on" : ""));
  t.setAttribute("role", "switch");
  t.setAttribute("aria-checked", String(!!get()));
  t.title = get() ? "On" : "Off";
  t.addEventListener("click", () => { const v = !get(); set(v); t.classList.toggle("on", v); t.setAttribute("aria-checked", String(v)); t.title = v ? "On" : "Off"; });
  return t;
}
function mkCombo(options, get, set, width) {
  const c = el("button", "combo");
  c.style.minWidth = (width || 150) + "px";
  const label = () => { const g = get(); const o = options.find((o) => o.value === g); c.textContent = o ? o.label : g; };
  label();
  c.addEventListener("click", () => {
    showListbox(c, options.map((o) => o.label), c.textContent, (picked) => {
      const o = options.find((o) => o.label === picked);
      if (o) { set(o.value); label(); }
    });
    c.classList.add("open");
  });
  return c;
}
function mkSlider(min, max, step, get, set, suffix) {
  const s = el("input", "sliderbox");
  s.type = "range"; s.min = min; s.max = max; s.step = step; s.value = get();
  const out = el("span", "r-meta", get() + (suffix || ""));
  s.addEventListener("input", () => { out.textContent = s.value + (suffix || ""); });
  s.addEventListener("change", () => set(parseFloat(s.value)));
  const wrap = el("span", "sctl");
  wrap.append(s, out);
  return wrap;
}
function srow(name, why, ctl) {
  const r = el("div", "srow");
  const l = el("div", "slabel");
  l.appendChild(el("div", "name", esc(name)));
  if (why) l.appendChild(el("div", "why", esc(why)));
  r.append(l, ctl);
  return r;
}
async function screenSettings(root, token) {
  if (!S.settings) S.settings = (await IPC.call("settings.get")) || {};
  if (token !== S.renderToken) return;
  const st = S.settings;
  const wrap = el("div", "settings-wrap");
  wrap.appendChild(pageHead("Settings", "Everything stays on this computer"));
  const sec = (title, desc) => {
    const s = el("div", "settings-section");
    s.appendChild(el("h3", "", esc(title)));
    if (desc) s.appendChild(el("p", "desc", esc(desc)));
    wrap.appendChild(s);
    return s;
  };

  /* appearance */
  const ap = sec("Appearance", "Dark only, the way the app is meant to look.");
  ap.appendChild(srow("Font", "Used across the whole app", mkCombo(
    ["FiraCode Nerd Font", "Fira Code", "Segoe UI", "Consolas", "Cascadia Code"].map((f) => ({ label: f, value: f })),
    () => (st.appearance || {}).fontFamily || "FiraCode Nerd Font",
    (v) => setSetting("appearance.fontFamily", v))));
  ap.appendChild(srow("Font weight", "How thick the letters are", mkCombo(
    [400, 500, 600, 700].map((w) => ({ label: String(w), value: w })),
    () => (st.appearance || {}).fontWeight || 500,
    (v) => setSetting("appearance.fontWeight", v), 100)));
  ap.appendChild(srow("Blur amount", "How strong the frosted glass is", mkSlider(16, 30, 1,
    () => (st.appearance || {}).blur || 22, (v) => setSetting("appearance.blur", v), " px")));

  /* motion */
  const mo = sec("Open and close animation", "Every panel, menu and dialog fades and slides when it opens and closes.");
  const m = st.motion || {};
  mo.appendChild(srow("Animation", "Turn all open and close motion on or off", mkToggle(
    () => m.enabled !== false, (v) => setSetting("motion.enabled", v))));
  mo.appendChild(srow("Speed", "Follows the speed the rest of your apps use", mkCombo(
    [["follow", "Follow the global speed"], ["veryquick", "Very quick"], ["quick", "Quick"], ["normal", "Normal"], ["relaxed", "Relaxed"], ["slow", "Slow"]]
      .map(([v, l]) => ({ label: l, value: v })),
    () => m.speed || "follow", (v) => setSetting("motion.speed", v))));
  mo.appendChild(el("div", "sdiv"));
  [["menus", "Menus and context menus"], ["tooltips", "Tooltips"], ["toasts", "Toasts"], ["dialogs", "Dialogs"], ["panels", "Side panels and drawers"], ["pages", "Page changes"]].forEach(([k, label]) => {
    mo.appendChild(srow(label, "", mkToggle(
      () => (m.surfaces || {})[k] !== false, (v) => {
        const s = Object.assign({}, m.surfaces || {}); s[k] = v;
        setSetting("motion.surfaces", s);
      })));
  });

  /* behaviour */
  const be = sec("Behaviour", "");
  const b = st.behaviour || {};
  be.appendChild(srow("Enqueue by default", "Double click adds to the queue instead of playing now", mkToggle(() => b.enqueueByDefault, (v) => setSetting("behaviour.enqueueByDefault", v))));
  be.appendChild(srow("Gapless", "No silence between songs in an album", mkToggle(() => b.gapless !== false, (v) => setSetting("behaviour.gapless", v))));
  be.appendChild(srow("Crossfade", "Blend the end of one song into the next", mkSlider(0, 12, 1, () => b.crossfade || 0, (v) => setSetting("behaviour.crossfade", v), " s")));
  be.appendChild(srow("Show artwork", "Cover art in lists and rows", mkToggle(() => b.showArtwork !== false, (v) => setSetting("behaviour.showArtwork", v))));
  be.appendChild(srow("Remember playback", "Keep the queue and position after a restart", mkToggle(() => b.rememberPlayback !== false, (v) => setSetting("behaviour.rememberPlayback", v))));
  be.appendChild(srow("Send listens to Last.fm and ListenBrainz", "Uses the sessions saved on this computer", mkToggle(() => b.scrobble, (v) => setSetting("behaviour.scrobble", v))));

  /* online */
  const on = sec("Online", "Search further than your library and bring songs in.");
  const o = st.online || {};
  on.appendChild(srow("Soulseek search", "Needs slskd running on this computer", mkToggle(() => o.slskd !== false, (v) => setSetting("online.slskd", v))));
  on.appendChild(srow("slskd address", "", mkCombo(["127.0.0.1:5030"].map((v) => ({ label: v, value: v })), () => o.slskdAddress || "127.0.0.1:5030", (v) => setSetting("online.slskdAddress", v))));
  on.appendChild(srow("Video search", "Stream or download from video sites", mkToggle(() => o.youtube !== false, (v) => setSetting("online.youtube", v))));
  on.appendChild(srow("Downloads folder", "Where downloads land", (() => {
    const w = el("span", "sctl");
    const t = el("span", "folder-path", esc(o.downloadsFolder || "Music\\Seamless Music Player\\Downloads"));
    const pick = el("button", "btn small", "Change");
    pick.addEventListener("click", async () => {
      const res = await IPC.call("library.addFolder", {});
      if (res && res.path) { setSetting("online.downloadsFolder", res.path); t.textContent = res.path; }
    });
    w.append(t, pick);
    return w;
  })()));
  on.appendChild(srow("Save online songs to the library", "Songs you play from online stay out of the library unless this is on", mkToggle(() => o.saveToLibrary, (v) => setSetting("online.saveToLibrary", v))));

  /* advanced */
  const adv = sec("Advanced", "");
  const devices = await IPC.call("devices.list");
  if (token !== S.renderToken) return;
  const a = st.advanced || {};
  adv.appendChild(srow("Output device", "", mkCombo(
    (devices || []).map((d) => ({ label: d.name, value: d.id })),
    () => a.outputDevice || (devices && devices[0] && devices[0].id),
    (v) => { setSetting("advanced.outputDevice", v); IPC.call("devices.set", { id: v }); }, 220)));
  const eq = st.eq || {};
  adv.appendChild(srow("Equaliser", "Ten bands, from deep to shimmer", mkToggle(() => eq.enabled, (v) => {
    setSetting("eq.enabled", v);
    IPC.call("eq.set", { enabled: v, preset: eq.preset || "flat", gains: eq.gains || new Array(10).fill(0) });
    renderPage();
  })));
  if (eq.enabled) {
    const bandRow = el("div", "srow");
    bandRow.style.flexWrap = "wrap";
    const bands = ["31", "62", "125", "250", "500", "1k", "2k", "4k", "8k", "16k"];
    const gains = eq.gains || new Array(10).fill(0);
    bands.forEach((label, i) => {
      const cell = el("div");
      cell.style.cssText = "display:flex;flex-direction:column;align-items:center;gap:4px;font-size:10px;color:var(--ov1)";
      const s = el("input", "sliderbox");
      s.style.width = "76px";
      s.type = "range"; s.min = -12; s.max = 12; s.step = 1; s.value = gains[i] || 0;
      const out = el("span", "", (gains[i] || 0) + " dB");
      s.addEventListener("change", () => {
        gains[i] = parseFloat(s.value);
        out.textContent = gains[i] + " dB";
        IPC.call("eq.set", { enabled: true, preset: eq.preset || "custom", gains });
      });
      cell.append(s, out, el("span", "", label));
      bandRow.appendChild(cell);
    });
    adv.appendChild(bandRow);
  }
  const info = await IPC.call("app.info");
  if (token !== S.renderToken) return;
  adv.appendChild(srow("About", info ? `Version ${info.version}, using about ${info.memoryMb} MB of memory right now` : "", (() => {
    const w = el("span", "sctl");
    const rescan = el("button", "btn small", "Rescan library");
    rescan.addEventListener("click", async () => { await IPC.call("library.rescan", {}); toast("ok", "Rescan started", "New songs show up as they are found."); });
    w.appendChild(rescan);
    return w;
  })()));
  root.appendChild(wrap);
}

/* ---------- playbar ---------- */
function updatePlaybar() {
  const p = S.player;
  const t = p.track;
  $("#npTitle").textContent = t ? t.title : "Nothing playing";
  $("#npArtist").textContent = t ? (t.artists || []).join(", ") : "Pick a song and press play";
  $("#npTitle").classList.toggle("clickable", !!t);
  $("#npArtist").classList.toggle("clickable", !!t);
  const artBox = $("#npArt");
  if (t && t.artworkId) {
    if (artBox.dataset.id !== String(t.artworkId)) {
      artBox.dataset.id = String(t.artworkId);
      artBox.innerHTML = "";
      const im = el("img");
      im.alt = ""; im.loading = "lazy"; im.style.cssText = "width:100%;height:100%;object-fit:cover;border-radius:inherit";
      im.src = artSrc(t.artworkId);
      artBox.appendChild(im);
    }
  } else { artBox.dataset.id = ""; artBox.innerHTML = icon("songs"); }
  $("#loveBtn").classList.toggle("love-on", !!(t && t.love));
  $("#rating").innerHTML = t ? ratingStars(t.rating || 0, true) : "";
  $("#playBtn").innerHTML = icon(p.playing ? "pause" : "play");
  $("#shuffleBtn").classList.toggle("on", !!p.shuffle);
  $("#loopBtn").innerHTML = icon(p.loop === "one" ? "repeat-one" : "repeat");
  $("#loopBtn").classList.toggle("on", p.loop !== "off");
  $("#volBtn").innerHTML = icon(p.volume === 0 ? "mute" : "volume");
  if (document.activeElement !== $("#volSlider")) $("#volSlider").value = Math.round((p.volume || 0) * 100);
  $("#posCur").textContent = fmtTime(p.position);
  $("#posTotal").textContent = fmtTime(p.duration);
  const sb = $("#sleepBtn");
  const st = p.sleepTimer;
  const badge = sb.querySelector(".sleep-badge") || (() => { const b = el("span", "sleep-badge"); sb.appendChild(b); return b; })();
  if (st && st.minutes != null && st.minutes > 0) { badge.textContent = st.minutes >= 60 ? Math.round(st.minutes / 60) + "h" : st.minutes + "m"; badge.style.display = ""; }
  else if (st && st.endOfSong) { badge.textContent = "1"; badge.style.display = ""; }
  else badge.style.display = "none";
  const sp = $("#speedBtn");
  sp.innerHTML = icon("speed");
  sp.classList.toggle("on", (p.speed || 1) !== 1);
  updateWave();
  if (S.mini) updateMini();
}
let waveSeed = 1;
function waveBars(trackId, n) {
  let seed = 0;
  const s = String(trackId || "");
  for (let i = 0; i < s.length; i++) seed = (seed * 31 + s.charCodeAt(i)) >>> 0;
  if (!seed) seed = 1;
  const out = [];
  for (let i = 0; i < n; i++) {
    seed = (seed * 1103515245 + 12345) >>> 0;
    const v = 0.18 + (seed / 4294967296) * 0.82;
    out.push(v);
  }
  return out;
}
function updateWave() {
  const p = S.player;
  const ratio = p.duration ? Math.min(1, p.position / p.duration) : 0;
  const flat = S.settings && S.settings.appearance && S.settings.appearance.waveProgress === false;
  $("#waveFlatBg").style.display = flat ? "" : "none";
  $("#waveFlatFg").style.display = flat ? "" : "none";
  $("#waveBg").style.display = flat ? "none" : "";
  $("#waveFg").style.display = flat ? "none" : "";
  if (flat) {
    $("#waveFlatFg").setAttribute("x2", String(ratio * 1000));
  } else {
    const key = p.track ? p.track.id : "none";
    if (waveSeed !== key) {
      waveSeed = key;
      const bars = waveBars(key, 60);
      let d = "";
      bars.forEach((v, i) => {
        const x = 5 + (i * 990) / (bars.length - 1);
        d += (i ? " L" : "M") + x.toFixed(1) + " " + (7 - v * 6).toFixed(2);
      });
      const d2 = [];
      bars.forEach((v, i) => { const x = 5 + (i * 990) / (bars.length - 1); d2.unshift(`${x.toFixed(1)} ${(7 + v * 6).toFixed(2)}`); });
      $("#waveBg").setAttribute("d", d + " " + d2.map((s) => "L" + s).join(" "));
      $("#waveFg").setAttribute("d", d + " " + d2.map((s) => "L" + s).join(" "));
    }
    $("#wave-clip-rect").setAttribute("width", String(ratio * 1000));
  }
  $("#waveThumb").style.left = (ratio * 100) + "%";
}
function waveSeek(e) {
  const wave = $("#wave");
  const r = wave.getBoundingClientRect();
  const ratio = Math.min(1, Math.max(0, (e.clientX - r.left) / r.width));
  if (S.player.duration) IPC.call("player.seek", { seconds: ratio * S.player.duration });
  return ratio;
}
function initWave() {
  const wave = $("#wave");
  let dragging = false;
  wave.addEventListener("pointerdown", (e) => {
    dragging = true; wave.classList.add("dragging"); wave.setPointerCapture(e.pointerId);
    const ratio = waveSeek(e);
    $("#waveThumb").style.left = (ratio * 100) + "%";
  });
  wave.addEventListener("pointermove", (e) => {
    if (!dragging) return;
    const r = wave.getBoundingClientRect();
    const ratio = Math.min(1, Math.max(0, (e.clientX - r.left) / r.width));
    $("#wave-clip-rect").setAttribute("width", String(ratio * 1000));
    $("#waveThumb").style.left = (ratio * 100) + "%";
    $("#posCur").textContent = fmtTime(ratio * S.player.duration);
  });
  wave.addEventListener("pointerup", (e) => { dragging = false; wave.classList.remove("dragging"); waveSeek(e); });
}

/* sleep timer menu */
function sleepMenu(x, y) {
  const cur = S.player.sleepTimer;
  const items = [
    { label: "Off", ic: "moon", check: !cur, fn: () => IPC.call("player.sleepTimer", { minutes: null }) },
    "sep",
    ...[15, 30, 45, 60, 90].map((m) => ({
      label: m + " minutes", ic: "clock", check: cur && cur.minutes === m,
      fn: () => IPC.call("player.sleepTimer", { minutes: m }),
    })),
    "sep",
    { label: "At the end of this song", ic: "songs", check: !!(cur && cur.endOfSong), fn: () => IPC.call("player.sleepTimer", { minutes: null, endOfSong: true }) },
  ];
  showMenu(items, x, y);
}
function speedMenu(x, y) {
  const cur = S.player.speed || 1;
  showMenu([0.75, 1, 1.25, 1.5, 2].map((r) => ({
    label: r === 1 ? "Normal speed" : r + "x", ic: "speed", check: cur === r,
    fn: () => IPC.call("player.speed", { rate: r }),
  })), x, y);
}

/* ---------- now playing page ---------- */
async function openNp() {
  S.np = true;
  const pg = $("#npPage");
  updateNpInfo();
  openPop(pg, "centre");
  loadLyrics();
  startViz();
}
function closeNp() {
  S.np = false;
  stopViz();
  closePop($("#npPage"), "centre");
}
function updateNpInfo() {
  const t = S.player.track;
  $("#npTitle2").textContent = t ? t.title : "Nothing playing";
  $("#npArtist2").textContent = t ? (t.artists || []).join(", ") : "";
  $("#npAlbum2").textContent = t ? t.album || "" : "";
  $("#npArtist2").classList.toggle("lnk", !!t);
  if (t && t.artworkId) {
    $("#npArt").src = artSrc(t.artworkId);
    $("#npBgArt").src = artSrc(t.artworkId);
  } else {
    $("#npArt").removeAttribute("src");
    $("#npBgArt").removeAttribute("src");
  }
  $("#npArt").style.display = t && t.artworkId ? "" : "none";
  if (t) $("#npAlbum2").innerHTML = `<span class="lnk">${esc(t.album || "")}</span>`;
}
async function loadLyrics() {
  const t = S.player.track;
  const box = $("#lyricsScroll");
  if (!t) { box.innerHTML = ""; $("#lyricsSource").textContent = ""; return; }
  box.innerHTML = `<div class="online-note">Looking for lyrics…</div>`;
  const res = await IPC.call("lyrics.get", { trackId: t.id });
  if (S.player.track !== t) return;
  S.lyrics = { trackId: t.id, data: res };
  box.innerHTML = "";
  if (!res || (!res.synced && !res.plain)) {
    box.appendChild(stateBox("info", "songs", "No lyrics for this song", "Nothing came back from the tags or the lyrics service."));
    $("#lyricsSource").textContent = "";
    return;
  }
  $("#lyricsSource").textContent = res.source ? "Lyrics: " + res.source : "";
  if (res.synced && res.synced.length) {
    S.lyrics.lines = res.synced.map((l) => ({ t: l.t, text: l.text, node: null }));
    res.synced.forEach((l, i) => {
      const d = el("div", "lyric-line upcoming");
      d.textContent = l.text;
      box.appendChild(d);
      S.lyrics.lines[i].node = d;
    });
  } else {
    const d = el("div", "lyrics-plain", esc(res.plain));
    box.appendChild(d);
  }
}
let lyricTimer = 0;
function updateLyrics() {
  if (!S.np || !S.lyrics || !S.lyrics.lines) return;
  const pos = S.player.position;
  let now = -1;
  S.lyrics.lines.forEach((l, i) => { if (l.t <= pos + 0.15) now = i; });
  if (now === S.lyrics.now) return;
  S.lyrics.now = now;
  S.lyrics.lines.forEach((l, i) => {
    l.node.classList.toggle("now", i === now);
    l.node.classList.toggle("past", i < now);
    l.node.classList.toggle("upcoming", i > now);
  });
  if (now >= 0) {
    const node = S.lyrics.lines[now].node;
    const box = $("#lyricsScroll");
    const top = node.offsetTop - box.clientHeight * 0.38;
    box.scrollTo({ top, behavior: "smooth" });
  }
}
function startLyricTimer() {
  stopLyricTimer();
  lyricTimer = setInterval(updateLyrics, 250);
}
function stopLyricTimer() { if (lyricTimer) { clearInterval(lyricTimer); lyricTimer = 0; } }

/* visualizer: one canvas, bars from viz events, only while visible */
let vizOn = false, vizBars = null, vizRaf = 0;
const vizCanvas = () => $("#vizCanvas");
function startViz() { vizOn = true; }
function stopViz() { vizOn = false; }
function drawViz() {
  if (!vizOn || !S.np) return;
  const c = vizCanvas();
  const dpr = window.devicePixelRatio || 1;
  const w = Math.max(60, Math.round(c.clientWidth * dpr)), h = Math.round(c.clientHeight * dpr);
  if (c.width !== w) c.width = w;
  if (c.height !== h) c.height = h;
  const ctx = c.getContext("2d");
  ctx.clearRect(0, 0, w, h);
  const bars = vizBars || [];
  if (!bars.length) return;
  const n = bars.length;
  const gap = 2 * dpr;
  const bw = (w - gap * (n - 1)) / n;
  const grad = ctx.createLinearGradient(0, 0, w, 0);
  grad.addColorStop(0, "#89B4FA");
  grad.addColorStop(1, "#CBA6F7");
  ctx.fillStyle = grad;
  for (let i = 0; i < n; i++) {
    const v = Math.max(0.03, Math.min(1, bars[i]));
    const bh = v * (h - 2);
    ctx.fillRect(i * (bw + gap), h - bh, bw, bh);
  }
}

/* ---------- mini player ---------- */
function toggleMini(force) {
  const want = force != null ? force : !S.mini;
  S.mini = want;
  const m = $("#miniPlayer");
  if (want) { updateMini(); openPop(m, "centre"); }
  else closePop(m, "centre");
  $("#miniBtn").classList.toggle("on", want);
}
function updateMini() {
  const t = S.player.track;
  $("#miniTitle").textContent = t ? t.title : "Nothing playing";
  $("#miniArtist").textContent = t ? (t.artists || []).join(", ") : "";
  const a = $("#miniArt");
  if (t && t.artworkId) {
    if (a.dataset.id !== String(t.artworkId)) { a.dataset.id = String(t.artworkId); a.innerHTML = ""; const im = el("img"); im.alt = ""; im.style.cssText = "width:100%;height:100%;object-fit:cover;border-radius:inherit"; im.src = artSrc(t.artworkId); a.appendChild(im); }
  } else { a.dataset.id = ""; a.innerHTML = icon("songs"); }
  $("#miniPlay").innerHTML = icon(S.player.playing ? "pause" : "play");
  $("#miniFill").style.width = (S.player.duration ? (S.player.position / S.player.duration) * 100 : 0) + "%";
}

/* ---------- events and boot ---------- */
function onPlayerState(st) {
  if (!st) return;
  const trackChanged = !S.player.track || !st.track || S.player.track.id !== st.track.id;
  S.player = Object.assign({}, S.player, st, { speed: st.speed || S.player.speed || 1 });
  updatePlaybar();
  if (trackChanged) {
    S.lyrics = { trackId: null, data: null, lines: null, now: -1 };
    if (S.np) { updateNpInfo(); loadLyrics(); }
    document.querySelectorAll(".rows .row, .tbl-row").forEach(() => {});
    renderPage();
  }
  updateLyrics();
}
function wireEvents() {
  IPC.on("player.state", onPlayerState);
  IPC.on("viz", (p) => { vizBars = p && p.bars ? p.bars : null; if (S.np) requestAnimationFrame(drawViz); });
  IPC.on("library.changed", async () => {
    S.overview = await IPC.call("library.overview");
    S.songs && S.songs.cache && (S.songs = { total: 0, cache: new Map(), fetched: new Set() });
    renderPage();
  });
  IPC.on("scan.progress", (p) => {
    S.scan = p;
    const hint = document.querySelector(".songs-tools .hint");
    if (hint && S.page === "songs") hint.textContent = `Scanning: ${p.done} of ${p.total}`;
    if (p.done >= p.total && p.total > 0) S.scan = null;
  });
  IPC.on("online.results", (p) => {
    if (!p) return;
    S.online.soulseek = p.soulseek || [];
    S.online.youtube = p.youtube || [];
    renderOnline();
  });
  IPC.on("download.progress", (p) => {
    if (!p) return;
    for (const [rid, d] of S.downloads) {
      if (d.jobId === p.jobId) {
        d.pct = p.pct;
        d.state = p.state;
        if (p.state === "done") { toast("ok", "Download finished", "It is in your Downloads folder."); S.downloads.delete(rid); }
        else if (p.state === "error") { toast("error", "Download failed", "The source stopped answering."); S.downloads.delete(rid); }
        break;
      }
    }
    renderOnline();
  });
  IPC.on("toast", (p) => { if (p) toast(p.kind || "ok", "", p.text || ""); });
}
function wireControls() {
  $("#playBtn").addEventListener("click", () => IPC.call("player.toggle", {}));
  $("#nextBtn").addEventListener("click", () => IPC.call("player.next", {}));
  $("#prevBtn").addEventListener("click", () => IPC.call("player.prev", {}));
  $("#shuffleBtn").addEventListener("click", () => IPC.call("player.shuffle", { on: !S.player.shuffle }));
  $("#loopBtn").addEventListener("click", () => {
    const mode = S.player.loop === "off" ? "all" : S.player.loop === "all" ? "one" : "off";
    IPC.call("player.loop", { mode });
  });
  $("#loveBtn").addEventListener("click", () => {
    const t = S.player.track;
    if (t) IPC.call("track.setLove", { id: t.id, love: !t.love });
  });
  $("#rating").addEventListener("click", (e) => {
    const t = S.player.track;
    const star = e.target.closest(".star");
    if (!t || !star) return;
    const stars = [...$("#rating").querySelectorAll(".star")];
    IPC.call("track.setRating", { id: t.id, rating: stars.indexOf(star) + 1 });
  });
  $("#volSlider").addEventListener("input", (e) => {
    S.lastVol = S.lastVol == null ? S.player.volume : S.lastVol;
    IPC.call("player.volume", { value: e.target.value / 100 });
  });
  $("#volBtn").addEventListener("click", () => {
    if (S.player.volume === 0) IPC.call("player.volume", { value: S.lastVol == null ? 0.64 : S.lastVol });
    else { S.lastVol = S.player.volume; IPC.call("player.volume", { value: 0 }); }
  });
  $("#sleepBtn").addEventListener("click", (e) => { const r = e.currentTarget.getBoundingClientRect(); sleepMenu(r.left, r.top - 8); });
  $("#speedBtn").addEventListener("click", (e) => { const r = e.currentTarget.getBoundingClientRect(); speedMenu(r.left, r.top - 8); });
  $("#queueBtn").addEventListener("click", () => toggleQueue());
  $("#miniBtn").addEventListener("click", () => toggleMini(true));
  $("#npArt").addEventListener("click", () => { if (S.player.track) openNp(); });
  $("#npTitle").addEventListener("click", () => { const t = S.player.track; if (t) nav("artist", { name: (t.artists || [])[0] }); });
  $("#npArtist").addEventListener("click", () => { const t = S.player.track; if (t) nav("artist", { name: (t.artists || [])[0] }); });
  initWave();

  $("#npBack").addEventListener("click", closeNp);
  $("#npQueue").addEventListener("click", () => toggleQueue());
  $("#npAlbum2").addEventListener("click", (e) => {
    const t = S.player.track;
    if (e.target.classList.contains("lnk") && t) nav("album", { albumKey: t.albumKey });
  });
  $("#npArtist2").addEventListener("click", () => { const t = S.player.track; if (t && (t.artists || []).length) nav("artist", { name: t.artists[0] }); });

  const inp = $("#searchInput");
  inp.addEventListener("input", onSearchInput);
  inp.addEventListener("keydown", (e) => { if (e.key === "Enter") { if (S.page !== "search") nav("search", { q: inp.value.trim() }, true); } });
  $("#searchClear").addEventListener("click", clearSearch);
  $("#onlineClose").addEventListener("click", () => { clearSearchResults(); closeOnline(); });
  $("#queueClose").addEventListener("click", () => toggleQueue(false));

  $("#miniPrev").addEventListener("click", () => IPC.call("player.prev", {}));
  $("#miniPlay").addEventListener("click", () => IPC.call("player.toggle", {}));
  $("#miniNext").addEventListener("click", () => IPC.call("player.next", {}));
  $("#miniExpand").addEventListener("click", () => { toggleMini(false); if (S.player.track) openNp(); });
  $("#miniMin").addEventListener("click", () => IPC.call("window.minimize", {}));
  $("#miniClose").addEventListener("click", () => { toggleMini(false); IPC.call("window.close", {}); });

  document.addEventListener("keydown", (e) => {
    if (e.ctrlKey && e.key.toLowerCase() === "f") { e.preventDefault(); $("#searchInput").focus(); }
    else if (e.key === "Escape") {
      if (!$("#menuLayer").classList.contains("hidden")) closeMenus();
      else if (!$("#dialogLayer").classList.contains("hidden")) closeDialog();
      else if (S.mini) toggleMini(false);
      else if (S.np) closeNp();
      else if (S.queueOpen) toggleQueue(false);
      else if (S.online.open) { clearSearchResults(); closeOnline(); }
    }
  });
  document.addEventListener("mousedown", (e) => {
    if (S.online.open && !e.target.closest("#onlinePanel") && !e.target.closest("#searchBox")) { S.online.open = false; closePop($("#onlinePanel"), "right"); const u = document.querySelector(".online-underlay"); if (u) closePop(u, "right", true); }
    if (S.queueOpen && !e.target.closest("#queueDrawer") && !e.target.closest("#queueBtn")) toggleQueue(false);
  }, true);

  setInterval(() => { if (S.np && S.player.playing) drawViz(); }, 100);
  setInterval(updateLyrics, 500);
}
async function boot() {
  hydrateIcons(document);
  S.settings = (await IPC.call("settings.get")) || {};
  applySettings();
  buildTabs();
  wireEvents();
  wireControls();
  S.overview = await IPC.call("library.overview");
  const st = await IPC.call("player.state", {});
  if (st) onPlayerState(Object.assign({ track: null, playing: false, position: 0, duration: 0, volume: 0.64, shuffle: false, loop: "off", sleepTimer: null }, st));
  else updatePlaybar();
  markTabs();
  renderPage();
}
if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", boot);
else boot();
