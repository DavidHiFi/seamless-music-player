# Music Search for the phone

A small page for adding music from the phone, the same way the PC player does it:
search, tap Add, and the song shows up in the library a minute later.

- Searches people on Soulseek first, and YouTube at the same time.
- Songs come first, then albums with an "Add album" button, then YouTube results.
- Add tries the best few people at once and keeps whichever starts sending. If nobody
  starts within about 12 seconds it gets the song from YouTube instead.
- When the library has picked the song up, the page says so and Amperfy can play it.
- If the tower is asleep it gets woken, and the search runs again by itself.

## How it runs

- One small program on the always-on laptop, listening only on itself (port 8332).
  Tailscale serve puts it on the private network at the Music Search address.
- Soulseek runs on the tower. YouTube downloads run on the tower too, over ssh, and
  land in the same music folder the library reads.
- Logins are read at run time from the laptop's key folder and never reach the page.

## Build

```
cargo build --release
```

Run the build on the tower, then copy `music-search-phone.exe` to the laptop.
