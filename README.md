# QuickCut

[![Rust](https://img.shields.io/badge/Rust-1.75%2B-orange.svg)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](LICENSE)
[![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-lightgrey.svg)]()
[![Status](https://img.shields.io/badge/status-pre--alpha-red.svg)]()

CLI-first Rust tool for rendering "react thread post" videos from a folder of images and voice audio. Desktop UI is intentionally deferred until the shared core and CLI are stable.

## Features

- Convention-based input scanning: `01.png` pairs with `01.mp3`
- TOML project files shared between CLI and future desktop app
- Timeline solving from voice durations
- Audio mixing: voice, SFX, BGM, ducking, loudness normalization
- Frame rendering with backgrounds, scaling, auto-scroll, encodes to H.264 MP4 via FFmpeg

## Quick Start

```sh
quickcut init ./my_video
quickcut check ./my_video
quickcut render ./my_video -o out.mp4
```

Run the bundled short sample:

```sh
quickcut check examples/basic
quickcut render examples/basic -o examples/basic/out.mp4
```

Inspect media metadata:

```sh
quickcut probe ./my_video/01.png
```

## Build from source

Requires the Rust toolchain and FFmpeg in `PATH`.

```sh
cargo build -p qc-cli --release
```

Binary output:

```text
target/release/quickcut
```

## Commands

| Command | Purpose |
| --- | --- |
| `init` | Create `project.toml` from an input folder |
| `check` | Validate input and print the planned timeline |
| `render` | Render an MP4 |
| `probe` | Show audio/image metadata |

## Exit Codes

| Code | Meaning |
| --- | --- |
| `0` | Success |
| `1` | Input or validation error |
| `2` | Audio decode error |
| `3` | FFmpeg error |
| `4` | Other error |

## Project Layout

```text
quickcut/
  crates/
    qc-core/     Shared model, parser, timeline solver
    qc-audio/    Decode, mix, duck, loudness
    qc-render/   Frame compositor
    qc-export/   FFmpeg pipe export
    qc-cli/      Command-line interface
  .github/       CI workflows
```

## Status

QuickCut is under active development. The core API is designed as a reusable library for the planned desktop application.