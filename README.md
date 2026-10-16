# QuickCut

[![Rust](https://img.shields.io/badge/Rust-stable-orange.svg)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](LICENSE)
[![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-lightgrey.svg)]()
[![Status](https://img.shields.io/badge/status-pre--alpha-red.svg)]()

A desktop-first Rust editor that turns a folder of images and voice audio into a "react thread post" short video. The crates behind the app are a reusable library, and a secondary `quickcut-cli` binary covers scripting and headless renders.

## Requirements

- **FFmpeg** with `libx264` and the native `aac` encoder. Only `render` needs it; `init`, `check`, and `probe` run without it.
- FFmpeg must be on `PATH`, or passed explicitly with `--ffmpeg PATH`. A missing or unusable binary fails `render` with exit code 3.
- **No Rust install is needed to run a prebuilt binary.** Grab the Windows, macOS, or Linux build from the GitHub releases page.
- **Rust toolchain** only when building from source; the pinned version is in `rust-toolchain.toml`.

FFmpeg is deliberately not bundled: `libx264` is GPL-licensed, so shipping it would place GPL obligations on everyone who downloads QuickCut.

## Features

- Convention-based input scanning: `01.png` pairs with `01.mp3`, and `bgm.*` is picked up automatically. Effects live in `sfx/` and are referenced by name from `project.toml`
- Voice, SFX and BGM in mp3, wav, m4a or ogg, at any sample rate; everything is resampled to 48 kHz
- TOML project files shared between CLI and desktop app
- Timeline solved from voice durations, integer samples at 48 kHz
- Audio mixing: voice, SFX, BGM looping, ducking, EBU R128 loudness normalization
- Frame rendering: blurred, solid, or video background, pop and slide animations, cross-fade between scenes, rounded corners, drop shadow
- Auto-scroll for tall screenshots, timed to the voice track
- H.264 MP4 through FFmpeg, with a fast `draft` preset for quick previews
- Stable JSON progress output for tooling

## Quick Start

```sh
cargo run
```

The app opens with no project. Choose **New Project**, pick a folder that holds your images and voice files, and QuickCut scans them into `project.toml`. Name images and voice files with the same stem (`01.png` pairs with `01.mp3`), and a file named `bgm.*` is picked up as background music.

Then:

1. Drag scenes in the list, edit image/voice per scene, and tune animation and timing in the inspector.
2. Scrub the preview, then flip to final resolution for a last look.
3. Export an MP4 from the export dialog.

The repository ships no sample media. A minimal project folder is just:

```text
my_video/
  01.png
  01.mp3
```

The same project renders from the command line:

```sh
cargo run -p qc-cli -- init ./my_video
cargo run -p qc-cli -- check ./my_video
cargo run -p qc-cli -- render ./my_video -o out.mp4
```

`init` accepts `--bgm FILE` for background music, `--bg-video FILE|DIR` to switch to a video background (one clip or a directory to pick from per scene), `--out PATH` to write the project somewhere else, and `--force` to overwrite an existing file.

## Command line

The GUI is the product. `quickcut-cli` exists for scripting and headless machines.

| Command | Purpose |
| --- | --- |
| `init` | Create `project.toml` from an input folder |
| `check` | Validate input and print the planned timeline |
| `render` | Render an MP4 |
| `probe` | Show audio/image metadata |

Useful `render` flags:

| Flag | Purpose |
| --- | --- |
| `-o, --output PATH` | Output file, defaults to `out.mp4` |
| `--preset draft\|final` | Fast preview or final quality, defaults to `final` |
| `--fps N` | Frame rate override, falls back to `project.toml` |
| `--size WIDTHxHEIGHT` | Resolution override, falls back to `project.toml` |
| `--range FIRST:LAST` | Render scenes `FIRST` through `LAST` inclusive |
| `--threads N` | Cap the render pool; the FFmpeg encoder also needs cores |
| `--ffmpeg PATH` | Use a specific FFmpeg binary |
| `--no-audio-norm` | Skip EBU R128 loudness normalization |
| `--keep-temp` | Keep the generated `mix.wav` next to the output |
| `--json-progress` | Emit stable JSON progress lines |

`-v` and `-vv` are global and raise logging to info and debug. Malformed `--fps`, `--size` or `--range` values are errors, never silently corrected.

## Project file

`project.toml` is the shared source of truth for the CLI and the desktop app. Relative paths resolve from the file's own directory, so the same project renders the same regardless of where you invoke it.

```toml
schema_version = 1

[output]
fps = 30
width = 1080
height = 1920

[background]
kind = "blur_from_image" # solid, blur_from_image, or video
file = "clips"           # required when kind = "video"; a file or a directory
blur_sigma = 4.0
seed = 0                 # seeds the per-scene clip assignment
loop = true

[layout]
margin_x = 60
corner_radius = 28
long_image = "auto_scroll"

[timing]
pad_before = 0.25
pad_after = 0.35
default_hold = 2.5
transition = 0.30

[audio]
voice_gain_db = 0.0
sfx_gain_db = -8.0

[audio.bgm]
file = "bgm.mp3"
gain_db = -18.0
duck_db = -10.0
fade_in = 1.0
fade_out = 2.0
loop = true

[audio.master]
target_lufs = -14.0
true_peak_db = -1.0

[[scene]]
image = "01.png"
voice = "01.mp3"
anim_in = "pop"         # pop, or anything else for no entrance animation
anim_out = "slide_left" # slide_left, or anything else for no exit animation
transition_sfx = "whoosh.wav" # resolved from sfx/ by name

[[scene]]
image = "02.png"
voice = "02.mp3"
```

## Exit Codes

| Code | Meaning |
| --- | --- |
| `0` | Success |
| `1` | Input or validation error |
| `2` | Audio decode error |
| `3` | FFmpeg error |
| `4` | Other error |

## Build from source

Requires the Rust toolchain pinned in `rust-toolchain.toml`, plus FFmpeg as described under [Requirements](#requirements). On Linux the GUI build also needs the usual `eframe`/`egui` system packages (`libgtk-3-dev`, `libxkbcommon-dev`, `libgl1-mesa-dev`, `libwayland-dev`).

Build the desktop app, which is the workspace default member:

```sh
cargo build --release
```

Build the secondary CLI:

```sh
cargo build -p qc-cli --release
```

Binary output:

```text
target/release/quickcut       # desktop app
target/release/quickcut-cli   # command line
```

Prebuilt desktop and CLI binaries for Linux, macOS (Intel and Apple Silicon) and Windows are attached to each GitHub release.

Development checks:

```sh
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

## Performance

Measured on an 8-core machine at 1080x1920, 30 fps:

| Workload | Before | Now |
| --- | --- | --- |
| 1 minute of video, mixed-length scenes | ~3.7 min | ~1.2 min |

Three optimizations account for the gain: settled scenes are composited once and cached, the frame loop renders into a reused buffer instead of allocating per frame, and frames render in parallel while still reaching FFmpeg in order. Output is byte-for-byte identical regardless of thread count.

Use `--threads N` to cap the render pool. On a loaded machine, four threads often outperform eight because the FFmpeg encoder also needs cores.

## Current limits

- Scene images must be at least 480 px wide; `check` warns, `render` refuses
- `background.kind` supports `solid`, `blur_from_image`, and `video` (one clip, or a directory of clips picked per scene)
- The peak limiter trims the entire mix to the true-peak ceiling instead of applying look-ahead gain reduction, so quiet passages are not lifted

## Project Layout

```text
quickcut/
  src/           egui desktop app, the root package (the product)
  crates/
    qc-core/     Shared model, parser, timeline solver, probing
    qc-audio/    Decode, mix, duck, loudness
    qc-render/   Frame compositor
    qc-export/   FFmpeg pipe export
    qc-cli/      Secondary command-line interface
  .github/       CI workflows
```

The root package is the desktop app, so root cargo commands and CI build it by default. Sample media is kept out of the repository: `examples/` and `assets/` are git ignored.

## Status

QuickCut is under active development and pre-alpha. The desktop app is the primary product; the crates are the shared library behind it, and `quickcut-cli` is the secondary scripting tool. Expect breaking changes to `Project` fields and the progress JSON until the schema settles.