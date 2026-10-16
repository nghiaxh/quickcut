# QuickCut repository

Rust workspace implementing a desktop-first video editor for "react thread post" projects. The desktop app is the product; the CLI is a secondary scripting and headless tool.

## Build and checks

- Run the app: `cargo run` (desktop is the workspace default member)
- Build the CLI: `cargo build -p qc-cli` (release: `cargo build -p qc-cli --release`); the binary is `quickcut-cli`
- Format check: `cargo fmt --check`
- Lint: `cargo clippy --workspace --all-targets -- -D warnings`
- Tests: `cargo test --workspace`
- The desktop app is a normal workspace member under `apps/desktop`; root cargo commands and CI cover it
- Rust edition 2021, toolchain pinned in `rust-toolchain.toml`. FFmpeg must be in `PATH` for `render`.

## Workspace boundaries

- `qc-core`: time model (`Samples`), `Project`, TOML parsing/validation, timeline solver, file probing, `Fit`/`fit_image`, `Progress`/`Stage`
- `qc-audio`: decode/resample/mix/duck WAV generation
- `qc-render`: frame composition built on the `image` crate (not tiny-skia): animation, cross-fade, rounded corners, shadow. `render_frame_into` writes into a caller-owned buffer and uses per-thread scratch, so the frame loop allocates nothing. Scenes whose animation has settled are composited once and cached.
- `qc-export`: FFmpeg pipe orchestration, owns the frame loop, reports progress through a callback, and renders frames in parallel rounds that reach ffmpeg in strict order
- `qc-cli`: clap entrypoint and commands, packaged as the secondary `quickcut-cli` binary
- `apps/desktop`: egui desktop app, packaged as `quickcut`, the primary product
- Dependency direction: `cli, desktop -> export -> render,audio -> core`

## Error handling

- Each stage owns its error type: `CoreError`, `RenderError`, `AudioError`, `ExportError`, plus `ArgError` in `qc-cli`
- Every error type exposes `exit_code()`; `qc-cli` downcasts from `anyhow::Error` to map to the contract below
- Libraries use `thiserror`; only `qc-cli` uses `anyhow`
- An error that already embeds an inner message in its `Display` must not also mark that field as `source`, or anyhow prints the message twice

## Performance

- Profile before re-measuring. Render ablation on a 13 s project showed the shadow overlay and the animation resample, not the background copy, dominated the frame cost.
- The frame loop renders in parallel rounds sized by both the worker count and a 128 MB in-flight cap. Frames must still reach ffmpeg in order; `--threads` sizes the rayon pool and has to be called before any other rayon work.
- Output must stay byte for byte deterministic for the same input regardless of thread count. Verify with a hash comparison after touching the frame loop.

## Commands

- `quickcut-cli init <DIR> [--out project.toml] [--bgm FILE] [--force]`
- `quickcut-cli check <PROJECT|DIR>`
- `quickcut-cli render <PROJECT|DIR> -o out.mp4 [--preset draft|final] [--fps N] [--size WxH] [--range A:B] [--threads N] [--ffmpeg PATH] [--no-audio-norm] [--keep-temp] [--json-progress]`
- `quickcut-cli probe <FILE>`
- `-v` is global: default warn, `-v` info, `-vv` debug

Exit codes: `0` success, `1` input error, `2` decode error, `3` FFmpeg error, `4` other.

## Notes

- Project state lives in `project.toml`; relative paths resolve from that file.
- `--fps` and `--size` fall back to `project.toml` when omitted. Malformed values are errors, never silently corrected.
- Timeline times use integer `Samples` at 48 kHz; do not introduce float seconds accumulation. Voice durations are converted to 48 kHz when probed, so a 44.1 kHz file does not stretch the timeline.
- `render` writes `mix.wav` next to the output and deletes it unless `--keep-temp`.
- The limiter trims the whole mix to `true_peak_db` measured as EBU R128 true peak. It is not a look-ahead dynamics processor, so quiet passages are not lifted.
- `qc-core` and `qc-audio` must request the same symphonia features. Otherwise `check` accepts formats that `render` cannot mix, which is worse than rejecting the format outright.
- Resampling to 48 kHz goes through `FftFixedInOut` in fixed chunks with a partial flush, and the half-filter-length delay is dropped. Do not feed it a whole-file buffer: it rounds the chunk size up and then rejects the input.
- Images narrower than `qc_core::MIN_IMAGE_WIDTH` (480) are warned about by `check` and refused by `render`.
- `background.kind` supports `solid`, `blur_from_image`, and `video`; anything else fails with `NotImplemented`. A `video` background names one clip or a directory of clips; the clip is cover-scaled, blurred in ffmpeg, decoded in `qc-export`, and picked per scene by `assign_background_clips` (seeded, so `--range` matches a full render).
- The `--json-progress` output is part of the CLI contract and must remain stable: keys `stage`, `frame`, `total`, `eta_sec`; stages `probe`, `audio`, `render`, `encode`, `done`.
- `qc-export` never prints; it hands `Progress` to a callback and the CLI picks JSON lines or a progress bar.