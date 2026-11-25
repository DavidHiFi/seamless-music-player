# Contributing to Seamless Music Player

Thanks for helping. Bug reports, ideas and pull requests are all welcome.

## Reporting a bug

[Open an issue](https://github.com/DavidHiFi/seamless-music-player/issues/new) and include:

- what you did, what you expected and what happened instead
- your Windows version and the player version (Settings, then About at the bottom)
- a screenshot if it is something you can see

## Suggesting a feature

Open an issue that describes what you want to do and why. A screenshot or a mock-up helps.

## Sending a pull request

1. Fork the repo and create a branch from `main`.
2. Install Rust (stable, MSVC) and the WebView2 runtime, then run `cargo build --release`.
3. Before you push, run `cargo test --release` and `target\release\smp.exe --selftest`. Both must pass.
4. Keep each pull request to one change, and say in the description how you tested it.

The backend is Rust in `src\`. The window content is plain HTML, CSS and JavaScript in `ui\`,
built into the exe. Keep the look: Catppuccin Mocha colours, the blue to mauve accent and
FiraCode Nerd Font. Every window, menu and toast opens and closes with a short fade, and
each one can be switched off in Settings.

## License

By contributing, you agree that your work is released under this repo's [GPL-3.0 license](LICENSE).
