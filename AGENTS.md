# QuickCut repository

Rust workspace implementing a CLI-first video renderer for "react thread post" projects.

## Build and checks

- Build CLI: `cargo build -p qc-cli` (release: `cargo build -p qc-cli --release`)
- Run CLI during development: `cargo run -p qc-cli -- <COMMAND>`
- Format check: `cargo fmt --check`
- Lint: `cargo clippy -- -D warnings`
- Tests: `cargo test` (currently no test suites beyond scaffold)
- Rust edition 2021. FFmpeg must be installed and in `PATH` for `render`.

## Workspace boundaries

- `qc-core`: time model (`Samples`), `Project`, TOML parsing/validation, timeline solver, file probing
- `qc-audio`: decode/resample/mix/duck WAV generation
- `qc-render`: frame composition (currently image-based)
- `qc-export`: FFmpeg pipe orchestration
- `qc-cli`: clap entrypoint and commands
- `apps/desktop`: pure Rust egui desktop app using the same crates
- Dependency direction: `cli, desktop -> export -> render,audio -> core`

## Commands

- `quickcut init <DIR> [--out project.toml] [--bgm FILE] [--force]`
- `quickcut check <PROJECT|DIR>`
- `quickcut render <PROJECT|DIR> -o out.mp4 [--preset draft|final] [--fps 30] [--size 1080x1920]`
- `quickcut probe <FILE>`

Exit codes: `0` success, `1` input error, `2` decode error, `3` FFmpeg error, `4` other.

## Notes

- Project state lives in `project.toml`; relative paths resolve from that file.
- Timeline times use integer `Samples` at 48 kHz; do not introduce float seconds accumulation.
- `render` audio mix writes `mix.wav` next to the output unless later changed.
- Progress JSON (`--json-progress`) format is part of the CLI contract and must remain stable.