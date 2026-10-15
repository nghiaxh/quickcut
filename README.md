# QuickCut

[![Rust](https://img.shields.io/badge/Rust-stable-orange.svg)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](LICENSE)
[![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-lightgrey.svg)]()
[![Status](https://img.shields.io/badge/status-pre--alpha-red.svg)]()

CLI-first Rust tool for rendering "react thread post" videos from a folder of images and voice audio. The CLI is the product; the crates behind it are a library so a desktop app can ship later on the same core.

## Features

- Convention-based input scanning: `01.png` pairs with `01.mp3`, `bgm.*` and `sfx/` are picked up automatically
- Voice, SFX and BGM in mp3, wav, m4a or ogg, at any sample rate; everything is resampled to 48 kHz
- TOML project files shared between CLI and desktop app
- Timeline solved from voice durations, integer samples at 48 kHz
- Audio mixing: voice, SFX, BGM looping, ducking, EBU R128 loudness normalization
- Frame rendering: blurred or solid background, pop and slide animations, cross-fade between scenes, rounded corners, drop shadow
- Auto-scroll for tall screenshots, timed to the voice track
- H.264 MP4 through FFmpeg, with a fast `draft` preset for quick previews
- Stable JSON progress output for tooling

## Quick Start

```sh
quickcut init ./my_video
quickcut check ./my_video
quickcut render ./my_video -o out.mp4
```

No sample media is committed, so make your own in one step:

```sh
mkdir my_video && cp a-screenshot.png my_video/01.png && cp my-voice.mp3 my_video/01.mp3
quickcut init ./my_video
quickcut check ./my_video
quickcut render ./my_video -o out.mp4
```

Inspect media metadata:

```sh
quickcut probe ./my_video/01.png
```

## Commands

| Command | Purpose |
| --- | --- |
| `init` | Create `project.toml` from an input folder |
| `check` | Validate input and print the planned timeline |
| `render` | Render an MP4 |
| `probe` | Show audio/image metadata |

Useful `render` flags: `--preset draft` for fast previews, `--range 1:3` to render a few scenes, `--size 1080x1350` to override the project, `--json-progress` for machine-readable progress, `-v`/`-vv` for more logs. `--fps` and `--size` fall back to `project.toml` when you leave them out.

## Exit Codes

| Code | Meaning |
| --- | --- |
| `0` | Success |
| `1` | Input or validation error |
| `2` | Audio decode error |
| `3` | FFmpeg error |
| `4` | Other error |

## Build from source

Requires the Rust toolchain (pinned in `rust-toolchain.toml`) and FFmpeg in `PATH`. FFmpeg is not bundled: libx264 is GPL, so shipping it would put GPL obligations on anyone who downloads QuickCut.

```sh
cargo build -p qc-cli --release
```

Binary output:

```text
target/release/quickcut
```

Prebuilt binaries for Linux, macOS (Intel and Apple Silicon) and Windows are attached to each GitHub release.

Development checks:

```sh
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

## Performance

Measured on an 8-core machine, 1080x1920 at 30 fps:

| Workload | Before | Now |
| --- | --- | --- |
| 1 minute of video, mixed-length scenes | ~3.7 min | ~1.2 min |

Three things get it there: scenes whose animation has finished are composited once and reused, the frame loop writes into a reused buffer instead of allocating per frame, and frames render in parallel while still reaching FFmpeg in order. Output is byte for byte identical no matter how many threads you use.

Use `--threads N` to cap the render pool. On a busy machine 4 threads is often faster than 8, because FFmpeg's encoder wants cores too.

## Current limits

- Scene images must be at least 480 px wide; `check` warns, `render` refuses
- `background.kind` supports `solid` and `blur_from_image`
- The peak limiter trims the whole mix to the true-peak ceiling rather than running look-ahead gain reduction, so quiet passages are not lifted

## Project Layout

```text
quickcut/
  crates/
    qc-core/     Shared model, parser, timeline solver, probing
    qc-audio/    Decode, mix, duck, loudness
    qc-render/   Frame compositor
    qc-export/   FFmpeg pipe export
    qc-cli/      Command-line interface
  apps/
    desktop/     Experimental egui app (separate workspace)
  tools/         Scripts for generating sample media
  .github/       CI workflows
```

`apps/desktop` is its own Cargo workspace, so root commands and CI do not build it.

`examples/` and `assets/` are git ignored. Sample media is generated rather than committed, and `tools/make-demo.ps1` rebuilds it.

## Status

QuickCut is under active development and pre-alpha. The core API is treated as the stable contract that the desktop app will depend on; expect breaking changes to `Project` fields and the progress JSON until the schema settles.