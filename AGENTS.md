# QuickCut repository

Rust workspace implementing a CLI-first video renderer for "react thread post" projects.

## Build and checks

- Build CLI: `cargo build -p qc-cli` (release: `cargo build -p qc-cli --release`)
- Run CLI during development: `cargo run -p qc-cli -- <COMMAND>`
- Format check: `cargo fmt --check`
- Lint: `cargo clippy --workspace --all-targets -- -D warnings`
- Tests: `cargo test --workspace`
- `apps/desktop` is its own workspace: run cargo from inside it, root commands do not build it and CI does not test it
- Rust edition 2021, toolchain not pinned (CI uses stable). FFmpeg must be in `PATH` for `render`.

## Workspace boundaries

- `qc-core`: time model (`Samples`), `Project`, TOML parsing/validation, timeline solver, file probing, `Fit`/`fit_image`, `Progress`/`Stage`
- `qc-audio`: decode/resample/mix/duck WAV generation
- `qc-render`: frame composition built on the `image` crate (not tiny-skia): animation, cross-fade, rounded corners, shadow
- `qc-export`: FFmpeg pipe orchestration, owns the frame loop and reports progress through a callback
- `qc-cli`: clap entrypoint and commands
- `apps/desktop`: egui desktop app using the same crates
- Dependency direction: `cli, desktop -> export -> render,audio -> core`

## Error handling

- Each stage owns its error type: `CoreError`, `RenderError`, `AudioError`, `ExportError`, plus `ArgError` in `qc-cli`
- Every error type exposes `exit_code()`; `qc-cli` downcasts from `anyhow::Error` to map to the contract below
- Libraries use `thiserror`; only `qc-cli` uses `anyhow`
- An error that already embeds an inner message in its `Display` must not also mark that field as `source`, or anyhow prints the message twice

## Commands

- `quickcut init <DIR> [--out project.toml] [--bgm FILE] [--force]`
- `quickcut check <PROJECT|DIR>`
- `quickcut render <PROJECT|DIR> -o out.mp4 [--preset draft|final] [--fps N] [--size WxH] [--range A:B] [--threads N] [--ffmpeg PATH] [--no-audio-norm] [--keep-temp] [--json-progress]`
- `quickcut probe <FILE>`
- `-v` is global: default warn, `-v` info, `-vv` debug

Exit codes: `0` success, `1` input error, `2` decode error, `3` FFmpeg error, `4` other.

## Notes

- Project state lives in `project.toml`; relative paths resolve from that file.
- `--fps` and `--size` fall back to `project.toml` when omitted. Malformed values are errors, never silently corrected.
- Timeline times use integer `Samples` at 48 kHz; do not introduce float seconds accumulation. Voice durations are converted to 48 kHz when probed, so a 44.1 kHz file does not stretch the timeline.
- `render` writes `mix.wav` next to the output and deletes it unless `--keep-temp`.
- Images narrower than `qc_core::MIN_IMAGE_WIDTH` (480) are warned about by `check` and refused by `render`.
- `background.kind` supports `solid` and `blur_from_image`; anything else fails with `NotImplemented`.
- Progress JSON (`--json-progress`) format is part of the CLI contract and must remain stable: keys `stage`, `frame`, `total`, `eta_sec`, stages `probe`, `audio`, `render`, `encode`, `done`.
- `qc-export` never prints; it hands `Progress` to a callback and the CLI picks JSON lines or a progress bar.