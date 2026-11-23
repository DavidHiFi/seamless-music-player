# Rust migration plan: dopamine-fork (TypeScript/JavaScript)

1 package, 604 source files (37048 lines), 315 test files, 0 UI files, grouped into 224 units.

| Ready | Caution | Blocked | Keep in source language |
| ---: | ---: | ---: | ---: |
| 73 | 149 | 2 | 0 |

## Start here

Best first ports: safe to port (readiness) and worth porting (loops and string work, tests, size).

| # | Unit | Lines | Priority | Readiness | Tests | Bridge | Main risks |
| ---: | --- | ---: | ---: | ---: | ---: | --- | --- |
| 1 | `src/app/services/playback-indication/` | 61 | 84 | 95 | 5 | napi-rs | - |
| 2 | `src/app/services/file/` | 112 | 78 | 88 | 2 | napi-rs | String offsets and slicing (2); Async and promises (5); Exceptions (1) |
| 3 | `src/app/ui/pipes/` | 340 | 77 | 78 | 16 | napi-rs | String offsets and slicing (16); Number semantics (2) |
| 4 | `src/app/services/genre/` | 64 | 74 | 89 | 9 | napi-rs | Class inheritance (1) |
| 5 | `src/app/services/playlist-folder/` | 95 | 73 | 92 | 3 | napi-rs | Async and promises (2); Exceptions (1) |
| 6 | `src/app/common/validation/` | 58 | 72 | 99 | 3 | napi-rs | - |
| 7 | `src/app/services/album-artwork-cache/` | 83 | 71 | 92 | 4 | napi-rs | Async and promises (5); Exceptions (4) |
| 8 | `src/app/services/track-columns/` | 409 | 71 | 91 | 5 | napi-rs | - |
| 9 | `src/app/services/artist-artwork-cache/` | 101 | 70 | 90 | 5 | napi-rs | Async and promises (5); Exceptions (4) |
| 10 | `src/app/services/album/` | 178 | 70 | 87 | 15 | napi-rs | - |

### `src/app/services/playback-indication/`

61 lines in 2 files, wave 5, priority 84, readiness 95, bridge **napi-rs**.

- 1 npm dependencies not in the crate table: check them by hand.
- 5 test files import it directly.

### `src/app/services/file/`

112 lines in 2 files, wave 5, priority 78, readiness 88, bridge **napi-rs**.

- 1 npm dependencies not in the crate table: check them by hand.
- 2 test files import it directly.
- String offsets and slicing x2: `src/app/services/file/file.service.ts:127`, `src/app/services/file/file.service.ts:127`
- Async and promises x5: `src/app/services/file/file.service.ts:53`, `src/app/services/file/file.service.ts:56`, `src/app/services/file/file.service.ts:71`
- Exceptions x1: `src/app/services/file/file.service.ts:74`

### `src/app/ui/pipes/`

340 lines in 16 files, wave 5, priority 77, readiness 78, bridge **napi-rs**.

- 1 npm dependencies not in the crate table: check them by hand.
- 16 test files import it directly.
- String offsets and slicing x16: `src/app/ui/pipes/format-playback-time.ts:20`, `src/app/ui/pipes/format-playback-time.ts:20`, `src/app/ui/pipes/format-playback-time.ts:20`
- Number semantics x2: `src/app/ui/pipes/format-total-duration.pipe.ts:31`, `src/app/ui/pipes/format-total-duration.pipe.ts:32`

### `src/app/services/genre/`

64 lines in 3 files, wave 4, priority 74, readiness 89, bridge **napi-rs**.

- 1 npm dependencies not in the crate table: check them by hand.
- Import cycle with src/app/ui/components/dialogs/input-dialog/, src/app/ui/components/dialogs/info-dialog/, src/app/ui/components/dialogs/error-dialog/, src/app/ui/components/dialogs/equalizer-dialog/, src/app/ui/components/dialogs/edit-tracks-dialog/, src/app/ui/components/dialogs/edit-smart-playlist-dialog/, src/app/ui/components/dialogs/edit-playlist-dialog/, src/app/services/track-columns/, src/app/ui/components/dialogs/edit-columns-dialog/, src/app/ui/components/dialogs/edit-artist-dialog/, src/app/ui/components/dialogs/edit-album-dialog/, src/app/ui/components/dialogs/confirmation-dialog/, src/app/services/equalizer/, src/app/services/playback/audio-player/, src/app/services/playback-information/, src/app/services/media-session/, src/app/services/music-search/, src/app/services/catalog/, src/app/services/rating-backup/, src/app/services/metadata/, src/app/services/album-accent-color/, src/app/services/appearance/, src/app/common/sorting/, src/app/services/playback/, src/app/services/folder/, src/app/services/indexing/, src/app/services/track/, src/app/services/playlist-folder/, src/app/services/playlist/, src/app/services/album/, src/app/services/dialog/, src/app/services/playlist-session/, src/app/data/repositories/, src/app/services/artist/, src/app/data/migrations/, src/app/data/: port them together.
- 9 test files import it directly.
- Class inheritance x1: `src/app/services/genre/genre-model.ts:7`

### `src/app/services/playlist-folder/`

95 lines in 4 files, wave 4, priority 73, readiness 92, bridge **napi-rs**.

- 1 npm dependencies not in the crate table: check them by hand.
- Import cycle with src/app/ui/components/dialogs/input-dialog/, src/app/ui/components/dialogs/info-dialog/, src/app/ui/components/dialogs/error-dialog/, src/app/ui/components/dialogs/equalizer-dialog/, src/app/ui/components/dialogs/edit-tracks-dialog/, src/app/ui/components/dialogs/edit-smart-playlist-dialog/, src/app/ui/components/dialogs/edit-playlist-dialog/, src/app/services/track-columns/, src/app/ui/components/dialogs/edit-columns-dialog/, src/app/ui/components/dialogs/edit-artist-dialog/, src/app/ui/components/dialogs/edit-album-dialog/, src/app/ui/components/dialogs/confirmation-dialog/, src/app/services/equalizer/, src/app/services/playback/audio-player/, src/app/services/playback-information/, src/app/services/media-session/, src/app/services/music-search/, src/app/services/catalog/, src/app/services/rating-backup/, src/app/services/metadata/, src/app/services/album-accent-color/, src/app/services/appearance/, src/app/common/sorting/, src/app/services/playback/, src/app/services/folder/, src/app/services/indexing/, src/app/services/track/, src/app/services/genre/, src/app/services/playlist/, src/app/services/album/, src/app/services/dialog/, src/app/services/playlist-session/, src/app/data/repositories/, src/app/services/artist/, src/app/data/migrations/, src/app/data/: port them together.
- 3 test files import it directly.
- Async and promises x2: `src/app/services/playlist-folder/playlist-folder.service.ts:24`, `src/app/services/playlist-folder/playlist-folder.service.ts:25`
- Exceptions x1: `src/app/services/playlist-folder/playlist-folder.service.ts:55`

## Order by package

Packages come after the workspace packages they use. Inside a package, port by wave: a unit's imports are in earlier waves.

### 1. Dopamine (`.`, node, 315 test files)

| Wave | Unit | Status | Lines | Readiness | Bridge | Main risks |
| ---: | --- | --- | ---: | ---: | --- | --- |
| 0 | `angular.webpack.js` | ready | 28 | 100 | napi-rs | - |
| 0 | `get-package-information.js` | ready | 18 | 100 | napi-rs | - |
| 0 | `jest.setup.js` | ready | 5 | 100 | napi-rs | - |
| 0 | `main/background-work/` | ready | 16 | 100 | napi-rs | - |
| 0 | `main/background-work/common/` | ready | 49 | 100 | napi-rs | - |
| 0 | `main/background-work/common/application/` | ready | 65 | 100 | napi-rs | - |
| 0 | `main/background-work/common/scheduling/` | ready | 15 | 100 | napi-rs | - |
| 0 | `main/background-work/common/utils/` | ready | 25 | 100 | napi-rs | Exceptions (1); Number semantics (1) |
| 0 | `main/background-work/data/entities/` | ready | 73 | 100 | napi-rs | - |
| 0 | `main/background-work/indexing/messages/` | ready | 38 | 100 | napi-rs | - |
| 0 | `main/common/application/` | caution | 3 | 100 | napi-rs | - |
| 0 | `patch-discord-rpc.js` | ready | 23 | 100 | napi-rs | Exceptions (1) |
| 0 | `patch-taglib-sharp.js` | ready | 24 | 100 | napi-rs | Exceptions (1) |
| 0 | `src/app/services/appearance/theme/` | caution | 63 | 100 | napi-rs | - |
| 0 | `src/app/services/event-listener/` | caution | 37 | 100 | napi-rs | - |
| 0 | `src/app/services/indexing/messages/` | caution | 9 | 100 | napi-rs | - |
| 0 | `src/app/services/welcome/` | caution | 9 | 100 | napi-rs | - |
| 0 | `src/app/ui/components/collection/collection-tracks/collection-tracks-table/collection-tracks-table-header/` | caution | 14 | 100 | napi-rs | - |
| 0 | `src/app/ui/components/controls/accent-button/` | ready | 10 | 100 | napi-rs | - |
| 0 | `src/app/ui/components/controls/big-icon-button/` | caution | 9 | 100 | napi-rs | - |
| 0 | `src/app/ui/components/controls/icon-button/` | caution | 9 | 100 | napi-rs | - |
| 0 | `src/app/ui/components/controls/icon-text-button/` | caution | 12 | 100 | napi-rs | - |
| 0 | `src/app/ui/components/controls/text-icon-secondary-button/` | caution | 12 | 100 | napi-rs | - |
| 0 | `src/app/ui/components/controls/transparent-button/` | caution | 13 | 100 | napi-rs | - |
| 0 | `src/app/ui/components/cover-size-control/` | caution | 38 | 100 | napi-rs | - |
| 0 | `src/app/ui/components/dialogs/dialog-header/` | caution | 11 | 100 | napi-rs | - |
| 0 | `src/app/ui/components/dialogs/license-dialog/` | ready | 8 | 100 | napi-rs | - |
| 0 | `src/app/ui/components/logo-small/` | ready | 9 | 100 | napi-rs | - |
| 0 | `src/app/ui/components/manage-collection/manage-music/` | ready | 9 | 100 | napi-rs | - |
| 0 | `src/app/ui/components/metro-page-container/` | ready | 8 | 100 | napi-rs | - |
| 0 | `src/app/ui/components/mini-players/cover-player/cover-player-volume-control/` | ready | 11 | 100 | napi-rs | - |
| 0 | `src/app/ui/components/step-indicator/` | caution | 20 | 100 | napi-rs | - |
| 0 | `src/app/ui/components/sub-menu/` | ready | 8 | 100 | napi-rs | - |
| 0 | `src/app/ui/components/sub-menu/sub-menu-item/` | caution | 17 | 100 | napi-rs | - |
| 0 | `src/app/ui/components/welcome/welcome-done/` | ready | 8 | 100 | napi-rs | - |
| 0 | `src/app/ui/components/welcome/welcome-music/` | ready | 8 | 100 | napi-rs | - |
| 0 | `src/environments/` | ready | 16 | 100 | napi-rs | - |
| 0 | `electron-jest.js` | ready | 15 | 99 | napi-rs | String offsets and slicing (1) |
| 0 | `src/app/ui/components/mini-players/cover-player/cover-player-playback-queue/` | ready | 10 | 99 | napi-rs | - |
| 0 | `src/app/services/application/` | caution | 23 | 98 | napi-rs | - |
| 0 | `main/background-work/data/` | ready | 454 | 97 | napi-rs | - |
| 0 | `src/polyfills.ts` | ready | 48 | 97 | napi-rs | - |
| 0 | `src/polyfills-test.ts` | ready | 2 | 96 | napi-rs | - |
| 0 | `src/app/services/semantic-zoom/` | caution | 23 | 95 | napi-rs | - |
| 0 | `src/app/ui/components/common/` | caution | 14 | 95 | napi-rs | Callback-shaped APIs (2) |
| 0 | `src/test.ts` | ready | 21 | 95 | napi-rs | Reflection and prototypes (4) |
| 0 | `src/app/services/` | caution | 27 | 94 | napi-rs | - |
| 0 | `src/app/ui/interfaces/` | ready | 3 | 92 | napi-rs | - |
| 0 | `src/app/data/entities/` | caution | 147 | 91 | napi-rs | - |
| 0 | `src/app/common/api/git-hub/` | caution | 33 | 89 | napi-rs | String offsets and slicing (1); Async and promises (2) |
| 0 | `main/api/discord/` | ready | 120 | 88 | napi-rs | Async and promises (2); Exceptions (1) |
| 0 | `main/background-work/common/io/` | ready | 105 | 86 | napi-rs | Async and promises (8); Exceptions (4) |
| 0 | `src/app/common/scheduling/` | ready | 37 | 85 | napi-rs | Async and promises (4) |
| 0 | `main/common/settings/` | ready | 198 | 79 | napi-rs | Dynamic typing (8); Insertion-ordered maps and object keys (1) |
| 0 | `main/` | ready | 472 | 54 | napi-rs | String offsets and slicing (8); Callback-shaped APIs (15) |
| 0 | `src/app/common/settings/` | caution | 1074 | 49 | napi-rs | Async and promises (4); Insertion-ordered maps and object keys (2) |
| 1 | `src/app/services/collection-navigation/` | caution | 73 | 100 | napi-rs | - |
| 1 | `src/app/ui/components/collection/semantic-zoom/semantic-zoom-button/` | caution | 19 | 100 | napi-rs | - |
| 1 | `src/app/ui/components/collection/totals/` | caution | 13 | 100 | napi-rs | - |
| 1 | `src/app/ui/components/controls/toggle-switch/` | caution | 23 | 100 | napi-rs | - |
| 1 | `src/app/ui/components/welcome/welcome-greeting/` | caution | 14 | 100 | napi-rs | - |
| 1 | `src/app/ui/components/welcome/welcome-online/` | caution | 11 | 100 | napi-rs | - |
| 1 | `main/background-work/common/api/` | ready | 92 | 92 | napi-rs | Async and promises (7); Insertion-ordered maps and object keys (1) |
| 1 | `src/app/services/audio-visualizer/` | caution | 65 | 91 | napi-rs | String offsets and slicing (1) |
| 1 | `main/background-work/mocks/` | ready | 490 | 86 | napi-rs | Async and promises (27); Exceptions (3) |
| 1 | `src/app/services/appearance/themes/` | ready | 642 | 86 | napi-rs | - |
| 1 | `src/app/common/application/` | ready | 355 | 82 | napi-rs | - |
| 1 | `src/app/common/utils/` | caution | 202 | 69 | napi-rs | String offsets and slicing (7); Callback-shaped APIs (2) |
| 1 | `main/background-work/indexing/` | caution | 1184 | 49 | napi-rs | Regex features the `regex` crate lacks (1); Async and promises (32) |
| 1 | `main.ts` | blocked | 822 | 44 | napi-rs | String offsets and slicing (3); Dynamic code (eval, new Function, computed import) (1) |
| 1 | `src/app/common/` | caution | 361 | 29 | napi-rs | String offsets and slicing (4); Async and promises (19) |
| 1 | `src/app/common/io/` | caution | 660 | 29 | napi-rs | Regex features the `regex` crate lacks (2); String offsets and slicing (1) |
| 2 | `src/app/services/audio-output-device/` | caution | 27 | 100 | napi-rs | Async and promises (2); Exceptions (1) |
| 2 | `src/app/ui/components/information/components/` | caution | 18 | 100 | napi-rs | Async and promises (2) |
| 2 | `src/app/ui/components/logo-full/` | caution | 14 | 100 | napi-rs | String offsets and slicing (1) |
| 2 | `src/app/common/validation/` | ready | 58 | 99 | napi-rs | - |
| 2 | `src/app/ui/components/metro-page-container/metro-page/` | caution | 69 | 99 | napi-rs | - |
| 2 | `src/app/ui/components/window-controls/` | caution | 44 | 99 | napi-rs | - |
| 2 | `src/app/ui/components/slider/` | caution | 201 | 98 | napi-rs | Exceptions (2); Number semantics (1) |
| 2 | `main/background-work/ioc/` | ready | 129 | 97 | napi-rs | Insertion-ordered maps and object keys (1) |
| 2 | `src/app/services/album-artwork-cache/` | ready | 83 | 92 | napi-rs | Async and promises (5); Exceptions (4) |
| 2 | `src/app/common/api/fanart/` | caution | 27 | 90 | napi-rs | Async and promises (4); Dynamic typing (1) |
| 2 | `src/app/common/api/musicbrainz/` | caution | 52 | 90 | napi-rs | Dynamic typing (3); Async and promises (2) |
| 2 | `src/app/services/artist-artwork-cache/` | ready | 101 | 90 | napi-rs | Async and promises (5); Exceptions (4) |
| 2 | `src/app/services/search/` | caution | 51 | 90 | napi-rs | - |
| 2 | `src/app/services/update/` | caution | 149 | 89 | napi-rs | Async and promises (4); Number semantics (4) |
| 2 | `src/app/ui/animations/` | ready | 14 | 88 | napi-rs | - |
| 2 | `tools/` | blocked | 110 | 88 | napi-rs | Dynamic code (eval, new Function, computed import) (1); Async and promises (13) |
| 2 | `src/app/ui/components/collection/semantic-zoom/` | caution | 58 | 87 | napi-rs | String offsets and slicing (2); Async and promises (2) |
| 2 | `src/app/common/api/listenbrainz/` | caution | 104 | 83 | napi-rs | Dynamic typing (17); Async and promises (12) |
| 2 | `src/app/common/api/lyrics/` | caution | 87 | 78 | napi-rs | String offsets and slicing (2); Async and promises (4) |
| 2 | `src/app/common/metadata/` | ready | 535 | 69 | napi-rs | Async and promises (3); Callback-shaped APIs (2) |
| 2 | `src/app/services/translator/` | caution | 147 | 68 | napi-rs | Async and promises (4); Exceptions (2) |
| 2 | `src/app/common/api/lastfm/` | caution | 320 | 62 | napi-rs | Async and promises (23); Dynamic typing (15) |
| 3 | `src/app/services/tray/` | caution | 48 | 100 | napi-rs | - |
| 3 | `src/app/ui/components/welcome/welcome-language/` | caution | 11 | 100 | napi-rs | - |
| 3 | `main/background-work/workers/` | ready | 25 | 95 | napi-rs | Async and promises (3); Exceptions (1) |
| 3 | `src/app/services/scroll-position/` | ready | 24 | 88 | napi-rs | Insertion-ordered maps and object keys (1) |
| 3 | `src/app/services/notification/` | caution | 136 | 81 | napi-rs | Async and promises (35) |
| 4 | `src/app/ui/components/dialogs/edit-columns-dialog/` | caution | 18 | 100 | napi-rs | - |
| 4 | `src/app/ui/components/dialogs/confirmation-dialog/` | caution | 17 | 99 | napi-rs | - |
| 4 | `src/app/ui/components/dialogs/info-dialog/` | caution | 18 | 99 | napi-rs | - |
| 4 | `src/app/ui/components/dialogs/error-dialog/` | caution | 22 | 98 | napi-rs | - |
| 4 | `src/app/ui/components/dialogs/input-dialog/` | caution | 26 | 98 | napi-rs | - |
| 4 | `src/app/services/media-session/` | caution | 54 | 97 | napi-rs | Async and promises (2) |
| 4 | `src/app/ui/components/notification-bar/` | caution | 77 | 97 | napi-rs | Clock and random numbers (2) |
| 4 | `src/app/services/album-accent-color/` | caution | 21 | 96 | napi-rs | Async and promises (2); Exceptions (1) |
| 4 | `src/app/services/playlist-folder/` | ready | 95 | 92 | napi-rs | Async and promises (2); Exceptions (1) |
| 4 | `src/app/ui/components/dialogs/edit-playlist-dialog/` | caution | 68 | 92 | napi-rs | Async and promises (4); Exceptions (1) |
| 4 | `src/app/services/track-columns/` | ready | 409 | 91 | napi-rs | - |
| 4 | `src/app/ui/components/dialogs/equalizer-dialog/` | caution | 208 | 91 | napi-rs | Number semantics (2) |
| 4 | `src/app/data/migrations/` | caution | 209 | 89 | napi-rs | Class inheritance (14) |
| 4 | `src/app/services/genre/` | ready | 64 | 89 | napi-rs | Class inheritance (1) |
| 4 | `src/app/services/playback-information/` | caution | 72 | 89 | napi-rs | Async and promises (8) |
| 4 | `src/app/ui/components/dialogs/edit-artist-dialog/` | caution | 95 | 89 | napi-rs | Async and promises (10); Exceptions (1) |
| 4 | `src/app/services/album/` | ready | 178 | 87 | napi-rs | - |
| 4 | `src/app/services/equalizer/` | caution | 152 | 87 | napi-rs | Insertion-ordered maps and object keys (2); Exceptions (1) |
| 4 | `src/app/services/folder/` | caution | 228 | 84 | napi-rs | Async and promises (8); Exceptions (2) |
| 4 | `src/app/ui/components/dialogs/edit-album-dialog/` | caution | 207 | 82 | napi-rs | Async and promises (25); Exceptions (6) |
| 4 | `src/app/services/playlist-session/` | caution | 248 | 79 | napi-rs | Async and promises (11); Insertion-ordered maps and object keys (6) |
| 4 | `src/app/ui/components/dialogs/edit-smart-playlist-dialog/` | caution | 359 | 79 | napi-rs | Insertion-ordered maps and object keys (6); Async and promises (2) |
| 4 | `src/app/ui/components/dialogs/edit-tracks-dialog/` | caution | 320 | 79 | napi-rs | Async and promises (26); Exceptions (5) |
| 4 | `src/app/data/` | ready | 351 | 77 | napi-rs | Dynamic typing (3); Exceptions (1) |
| 4 | `src/app/services/metadata/` | ready | 212 | 75 | napi-rs | Async and promises (16); Exceptions (10) |
| 4 | `src/app/services/dialog/` | ready | 203 | 72 | napi-rs | Async and promises (27) |
| 4 | `src/app/common/sorting/` | ready | 425 | 71 | napi-rs | String offsets and slicing (3); Callback-shaped APIs (2) |
| 4 | `src/app/services/rating-backup/` | caution | 646 | 71 | napi-rs | Async and promises (15); Exceptions (10) |
| 4 | `src/app/services/artist/` | ready | 253 | 69 | napi-rs | Regexes built at runtime (2); Insertion-ordered maps and object keys (2) |
| 4 | `src/app/services/track/` | ready | 449 | 68 | napi-rs | Async and promises (5); Insertion-ordered maps and object keys (1) |
| 4 | `src/app/services/playback/audio-player/` | ready | 846 | 56 | napi-rs | Async and promises (48); Callback-shaped APIs (11) |
| 4 | `src/app/services/appearance/` | ready | 594 | 52 | napi-rs | String offsets and slicing (2); Async and promises (7) |
| 4 | `src/app/services/playlist/` | caution | 823 | 50 | napi-rs | Async and promises (42); Insertion-ordered maps and object keys (5) |
| 4 | `src/app/services/indexing/` | caution | 1262 | 41 | napi-rs | Async and promises (104); Exceptions (29) |
| 4 | `src/app/data/repositories/` | caution | 852 | 35 | napi-rs | String offsets and slicing (12); Dynamic typing (62) |
| 4 | `src/app/services/playback/` | caution | 1476 | 24 | napi-rs | String offsets and slicing (6); Async and promises (64) |
| 4 | `src/app/services/catalog/` | caution | 1093 | 8 | napi-rs | String offsets and slicing (18); Regex features the `regex` crate lacks (1) |
| 4 | `src/app/services/music-search/` | caution | 1343 | 0 | napi-rs | String offsets and slicing (17); Regex features the `regex` crate lacks (4) |
| 5 | `src/app/services/lifetime/` | caution | 31 | 100 | napi-rs | Async and promises (3) |
| 5 | `src/app/ui/components/collection/collection-artists/artist/` | caution | 22 | 100 | napi-rs | - |
| 5 | `src/app/ui/components/collection/collection-genres/genre/` | caution | 20 | 100 | napi-rs | - |
| 5 | `src/app/ui/components/collection/track/` | caution | 18 | 100 | napi-rs | - |
| 5 | `src/app/ui/components/information/about/` | caution | 31 | 100 | napi-rs | Async and promises (2) |
| 5 | `src/app/ui/components/manage-collection/manage-refresh/` | caution | 22 | 100 | napi-rs | Async and promises (2) |
| 5 | `src/app/ui/components/playback-controls/` | caution | 21 | 100 | napi-rs | - |
| 5 | `src/app/ui/components/playback-indicator/` | caution | 14 | 100 | napi-rs | - |
| 5 | `src/app/ui/components/playback-time/` | caution | 12 | 100 | napi-rs | - |
| 5 | `src/app/ui/components/search-box/` | caution | 68 | 100 | napi-rs | - |
| 5 | `src/app/ui/components/theme-switcher/` | caution | 16 | 100 | napi-rs | - |
| 5 | `src/app/ui/components/volume-control/` | caution | 20 | 100 | napi-rs | - |
| 5 | `src/app/ui/components/volume-icon/` | caution | 17 | 100 | napi-rs | - |
| 5 | `src/app/ui/components/welcome/welcome-appearance/` | caution | 11 | 100 | napi-rs | - |
| 5 | `src/app/services/discord/` | caution | 150 | 99 | napi-rs | - |
| 5 | `src/app/ui/components/now-playing/now-playing-nothing-playing/` | caution | 26 | 99 | napi-rs | Async and promises (4) |
| 5 | `src/app/ui/components/settings/appearance-settings/` | caution | 46 | 99 | napi-rs | Async and promises (4) |
| 5 | `src/app/ui/components/collection/collection-playlists/playlist-browser/playlist/` | caution | 20 | 98 | napi-rs | - |
| 5 | `src/app/ui/components/playback-speed-control/` | caution | 54 | 98 | napi-rs | Number semantics (1) |
| 5 | `src/app/services/now-playing-navigation/` | caution | 72 | 96 | napi-rs | Async and promises (2) |
| 5 | `src/app/ui/components/manage-collection/manage-albums/` | caution | 81 | 96 | napi-rs | Async and promises (8) |
| 5 | `src/app/ui/components/rating/` | caution | 78 | 96 | napi-rs | Async and promises (5); Exceptions (1) |
| 5 | `src/app/services/playback-indication/` | ready | 61 | 95 | napi-rs | - |
| 5 | `src/app/ui/components/online-results/` | caution | 64 | 95 | napi-rs | Dynamic typing (1) |
| 5 | `src/app/ui/components/now-playing/now-playing-showcase/` | caution | 109 | 94 | napi-rs | Async and promises (2) |
| 5 | `src/app/ui/components/add-folder/` | caution | 98 | 92 | napi-rs | Async and promises (16); Exceptions (5) |
| 5 | `src/app/ui/components/track-artwork/` | caution | 56 | 91 | napi-rs | Async and promises (2); Insertion-ordered maps and object keys (1) |
| 5 | `src/app/ui/components/playback-cover-art/` | caution | 139 | 89 | napi-rs | Async and promises (12) |
| 5 | `src/app/ui/components/sleep-timer-control/` | caution | 49 | 89 | napi-rs | String offsets and slicing (1) |
| 5 | `src/app/services/file/` | ready | 112 | 88 | napi-rs | String offsets and slicing (2); Async and promises (5) |
| 5 | `src/app/ui/components/playback-progress/` | caution | 386 | 88 | napi-rs | Async and promises (4); Exceptions (5) |
| 5 | `src/app/services/dock/` | caution | 175 | 87 | napi-rs | Async and promises (8); Exceptions (1) |
| 5 | `src/app/ui/components/playback-information/` | caution | 231 | 87 | napi-rs | Async and promises (12) |
| 5 | `src/app/ui/directives/` | ready | 183 | 86 | napi-rs | Number semantics (4) |
| 5 | `src/app/services/artist-information/` | ready | 187 | 85 | napi-rs | Async and promises (16); Exceptions (3) |
| 5 | `src/app/services/collection/` | caution | 71 | 85 | napi-rs | Async and promises (7); Exceptions (3) |
| 5 | `src/app/ui/pipes/` | ready | 340 | 78 | napi-rs | String offsets and slicing (16); Number semantics (2) |
| 5 | `src/app/services/navigation/` | caution | 69 | 76 | napi-rs | Async and promises (18) |
| 5 | `src/app/services/scrobbling/` | caution | 418 | 70 | napi-rs | Async and promises (44); Callback-shaped APIs (6) |
| 5 | `src/app/services/lyrics/` | ready | 347 | 68 | napi-rs | String offsets and slicing (4); Async and promises (16) |
| 5 | `src/app/ui/components/` | caution | 189 | 56 | napi-rs | String offsets and slicing (5); Async and promises (13) |
| 5 | `src/app/ui/components/enh-browse/` | caution | 791 | 37 | napi-rs | String offsets and slicing (14); Regex features the `regex` crate lacks (1) |
| 6 | `src/app/services/player-switcher/` | caution | 30 | 100 | napi-rs | Async and promises (3) |
| 6 | `src/app/ui/components/back-button/` | caution | 20 | 100 | napi-rs | Async and promises (3) |
| 6 | `src/app/ui/components/highlights/highlights-playback-pane/` | ready | 15 | 100 | napi-rs | - |
| 6 | `src/app/ui/components/information/` | caution | 17 | 100 | napi-rs | Class inheritance (1) |
| 6 | `src/app/ui/components/manage-collection/` | caution | 16 | 100 | napi-rs | Class inheritance (1) |
| 6 | `src/app/ui/components/now-playing/now-playing-artist-info/similar-artist/` | caution | 15 | 100 | napi-rs | - |
| 6 | `src/app/ui/components/now-playing/now-playing-lyrics/plain-lyrics/` | caution | 16 | 100 | napi-rs | - |
| 6 | `src/app/ui/components/now-playing/now-playing-lyrics/rich-lyrics/` | caution | 197 | 100 | napi-rs | - |
| 6 | `src/app/ui/components/now-playing/now-playing-playback-pane/` | caution | 33 | 100 | napi-rs | - |
| 6 | `src/app/ui/components/settings/` | caution | 17 | 100 | napi-rs | Class inheritance (1) |
| 6 | `src/app/ui/components/welcome/` | caution | 31 | 100 | napi-rs | Class inheritance (1) |
| 6 | `src/app/ui/components/welcome/welcome-navigation-buttons/` | caution | 46 | 100 | napi-rs | Async and promises (2) |
| 6 | `src/app/ui/components/settings/advanced-settings/` | caution | 35 | 99 | napi-rs | Async and promises (4) |
| 6 | `src/app/ui/components/collection/collection-playback-pane/` | caution | 32 | 98 | napi-rs | Async and promises (6) |
| 6 | `src/app/ui/components/love/` | caution | 66 | 98 | napi-rs | Async and promises (3); Exceptions (1) |
| 6 | `src/app/ui/components/now-playing/now-playing-artist-info/` | caution | 65 | 98 | napi-rs | Async and promises (4) |
| 6 | `src/app/ui/components/collection/collection-tracks/` | caution | 63 | 97 | napi-rs | Async and promises (5); Exceptions (1) |
| 6 | `src/app/ui/components/main-menu/` | caution | 41 | 96 | napi-rs | Async and promises (12) |
| 6 | `src/app/ui/components/now-playing/now-playing-lyrics/` | caution | 112 | 96 | napi-rs | Async and promises (5) |
| 6 | `src/app/ui/components/settings/online-settings/` | caution | 141 | 96 | napi-rs | Async and promises (4) |
| 6 | `src/app/ui/components/loading/` | caution | 58 | 95 | napi-rs | Async and promises (13) |
| 6 | `src/app/ui/components/mini-players/cover-player/` | caution | 129 | 93 | napi-rs | - |
| 6 | `src/app/ui/components/highlights/` | caution | 160 | 89 | napi-rs | Async and promises (4); Class inheritance (1) |
| 6 | `src/app/ui/components/playback-queue/` | caution | 138 | 89 | napi-rs | String offsets and slicing (1) |
| 6 | `src/app/ui/components/now-playing/` | caution | 201 | 87 | napi-rs | Async and promises (7); Class inheritance (1) |
| 6 | `src/app/testing/` | caution | 199 | 85 | napi-rs | Async and promises (9); Dynamic typing (2) |
| 6 | `src/app/ui/components/settings/behavior-settings/` | caution | 181 | 81 | napi-rs | String offsets and slicing (2); Async and promises (31) |
| 6 | `src/app/ui/components/collection/` | ready | 343 | 75 | napi-rs | Class inheritance (1); Exceptions (4) |
| 7 | `src/app/ui/components/switch-player-button/` | caution | 17 | 100 | napi-rs | Async and promises (2) |
| 7 | `src/app/ui/components/collection/collection-albums/` | caution | 158 | 87 | napi-rs | Async and promises (6); Class inheritance (2) |
| 7 | `src/app/ui/components/collection/collection-genres/` | caution | 284 | 81 | napi-rs | Async and promises (7); Class inheritance (2) |
| 7 | `src/app/ui/components/collection/collection-artists/` | caution | 358 | 79 | napi-rs | Async and promises (7); Class inheritance (2) |
| 7 | `src/app/ui/components/collection/collection-genres/genre-browser/` | ready | 222 | 78 | napi-rs | Async and promises (8); Insertion-ordered maps and object keys (1) |
| 7 | `src/app/ui/components/collection/collection-playlists/` | ready | 304 | 77 | napi-rs | Async and promises (21); Class inheritance (1) |
| 7 | `src/app/ui/components/collection/album-browser/` | ready | 354 | 75 | napi-rs | Async and promises (8); Insertion-ordered maps and object keys (1) |
| 7 | `src/app/ui/components/collection/collection-artists/artist-browser/` | ready | 246 | 75 | napi-rs | Async and promises (12); Insertion-ordered maps and object keys (2) |
| 7 | `src/app/ui/components/collection/track-browser/` | caution | 331 | 74 | napi-rs | Async and promises (12); Class inheritance (2) |
| 7 | `src/app/ui/components/collection/collection-folders/` | ready | 289 | 72 | napi-rs | Async and promises (15); Class inheritance (2) |
| 7 | `src/app/ui/components/collection/collection-playlists/playlist-track-browser/` | caution | 178 | 72 | napi-rs | String offsets and slicing (1); Async and promises (10) |
| 8 | `src/app/ui/components/collection/album-browser/album/` | caution | 29 | 100 | napi-rs | - |
| 8 | `src/app/ui/components/collection/collection-tracks/collection-tracks-table/` | caution | 319 | 86 | napi-rs | Async and promises (4); Class inheritance (1) |
| 8 | `src/app/ui/components/collection/collection-playlists/playlist-folder-browser/` | ready | 227 | 78 | napi-rs | Async and promises (13); Class inheritance (1) |
| 8 | `src/app/ui/components/collection/collection-playlists/playlist-browser/` | caution | 230 | 76 | napi-rs | Async and promises (14); Insertion-ordered maps and object keys (1) |
| 9 | `src/app/` | caution | 911 | 22 | napi-rs | Async and promises (15); Dynamic typing (3) |
| 10 | `src/main.ts` | ready | 12 | 96 | napi-rs | - |

## npm dependencies

| Package | Units | Rust | Fit | Note |
| --- | ---: | --- | --- | --- |
| `@angular/core` | 185 | ? | unknown |  |
| `@angular/material` | 24 | ? | unknown |  |
| `@angular/cdk` | 18 | ? | unknown |  |
| `@angular/animations` | 10 | ? | unknown |  |
| `electron` | 10 | ? | unknown |  |
| `@angular/common` | 6 | ? | unknown |  |
| `angular-split` | 6 | ? | unknown |  |
| `electron-log` | 6 | ? | unknown |  |
| `fs-extra` | 4 | ? | unknown |  |
| `@electron/remote` | 3 | ? | unknown |  |
| `@ngx-translate/core` | 3 | ? | unknown |  |
| `@angular/router` | 2 | ? | unknown |  |
| `@digimezzo/node-taglib-sharp` | 2 | ? | unknown |  |
| `jimp` | 2 | ? | unknown |  |
| `zone.js` | 2 | ? | unknown |  |
| `@angular/forms` | 1 | ? | unknown |  |
| `@angular/platform-browser` | 1 | ? | unknown |  |
| `@angular/platform-browser-dynamic` | 1 | ? | unknown |  |
| `@jimp/core` | 1 | ? | unknown |  |
| `@jsquash/webp` | 1 | ? | unknown |  |
| `@ngx-translate/http-loader` | 1 | ? | unknown |  |
| `core-js` | 1 | ? | unknown |  |
| `discord-rpc` | 1 | ? | unknown |  |
| `electron-reload` | 1 | ? | unknown |  |
| `fast-sort` | 1 | ? | unknown |  |
| `jest-preset-angular` | 1 | ? | unknown |  |
| `koffi` | 1 | ? | unknown |  |
| `md5-typescript` | 1 | ? | unknown |  |
| `node-vibrant` | 1 | ? | unknown |  |
| `reflect-metadata` | 1 | ? | unknown |  |
| `sanitize-filename` | 1 | ? | unknown |  |
| `tinycolor2` | 1 | ? | unknown |  |
| `typemoq` | 1 | ? | unknown |  |
| `rxjs` | 58 | futures, tokio-stream | partial | operators become stream combinators |
| `cheerio` | 1 | scraper | partial |  |
| `typescript` | 1 | oxc | partial | Parsing is covered. Type checking has no Rust equivalent. |
| `better-sqlite3` | 2 | rusqlite | good |  |
| `fast-xml-parser` | 2 | quick-xml | good |  |
| `moment` | 2 | jiff | good |  |
| `node-fetch` | 2 | reqwest | good |  |
| `uuid` | 2 | uuid, nanoid | good |  |

## Blocked

- `main.ts`: Uses dynamic code (eval, new Function or computed imports).
- `tools/`: Uses dynamic code (eval, new Function or computed imports).

## Risk guide

- **String offsets and slicing** (High): JS indexes strings in UTF-16 code units, Rust in UTF-8 bytes. Offsets that cross the boundary must be converted, or non-ASCII text gets wrong ranges silently.
- **Regex features the `regex` crate lacks** (High): Lookaround, backreferences and the `y` flag are not in the `regex` crate. Use `fancy-regex` for those patterns, or rewrite them, and test them on non-ASCII text: `\w`, `\b` and case-insensitive matching differ between engines even where a pattern compiles. JS `/i` without `u` never matches a non-ASCII letter to an ASCII one, Rust `(?i)` does (`ſ` matches `s`, the Kelvin sign `K` matches `k`): write `(?i-u)` or compare with `eq_ignore_ascii_case`.
- **Regexes built at runtime** (High): Patterns come from data or config in JS syntax. Rust regex syntax differs, so user-visible behavior can change. Pin it with tests on real patterns.
- **Dynamic code (eval, new Function, computed import)** (High): eval, new Function and computed imports have no Rust equivalent. Keep this part in JS or replace it with an explicit registry.
- **Dynamic typing** (Medium): Values whose shape is checked at runtime need an explicit Rust enum or `serde_json::Value`.
- **Insertion-ordered maps and object keys** (Medium): JS Map, Set and object keys keep insertion order. `HashMap` does not: use `indexmap` wherever order reaches output. With serde_json's `preserve_order`, `Map::remove` swaps the last key into the hole: use `shift_remove` for JS `delete`. Sort with `js::sort` when a comparator can return NaN: the order then depends on V8's algorithm.
- **Exceptions** (Low): Thrown errors become `Result`. Errors crossing the bridge must map back to the same JS error shape.
- **Async and promises** (Medium): Promises become futures. Bridge with napi-rs `async fn` or `wasm-bindgen-futures`, or keep the async shell in JS and port the sync core.
- **Class inheritance** (Medium): Class hierarchies become traits plus composition; `instanceof` checks become enums.
- **Reflection and prototypes** (Medium): Proxy, Reflect, Symbol and prototype tricks need a redesign, not a translation.
- **Callback-shaped APIs** (Medium): Function-typed parameters and methods cross the bridge on every call. Keep boundaries coarse: pass data in, get data out.
- **Number semantics** (Low): JS numbers are f64; bitwise ops truncate to i32; parseInt is lenient. Pick Rust integer types deliberately. Printing differs: `${n}` gives `1e+21`, `1e-7` and `0` for -0 where Rust's `{}` does not, so use `js::number_to_string`. `Math.round` rounds halves up (-2.5 gives -2), Rust's `round` away from zero: use `js::math_round`. `toFixed` rounds exact halves up ((2.5).toFixed(0) is "3"), Rust's `{:.0}` to even.
- **JSON round-trips** (Low): JSON.stringify key order and number formatting must match `serde_json` output if the result is user-visible.
- **Clock and random numbers** (Low): Results that depend on the time or on random numbers differ on every run, so a plain recording cannot pin them down. Record with `record --pin-clock` (TS/JS with the napi bridge, Python): `Date.now()`, `new Date()` and `Math.random` (`time.time`, `datetime.now` and `random`) then return fixed values during record, diff and gate, and the port reads the same ones from `src/port/pinned.rs` (`now_ms()`, `random()`, a documented mulberry32). Or pass the time and the seed in as parameters.

