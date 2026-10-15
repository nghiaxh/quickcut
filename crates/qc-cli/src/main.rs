mod commands;

use clap::{Parser, Subcommand};
use qc_core::CoreError;

#[derive(Parser)]
#[command(name = "quickcut", version, about = "QuickCut media renderer")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Init(commands::init::Args),
    Check(commands::check::Args),
    Render(commands::render::Args),
    Probe(commands::probe::Args),
}

fn main() {
    // Logging level handled by global flag count when present in subcommands is not available here;
    // default to warn. Subcommands may raise verbosity in future.
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::WARN)
        .try_init();

    let cli = Cli::parse();
    match dispatch(cli.command) {
        Ok(()) => std::process::exit(0),
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(exit_code(&e));
        }
    }
}

fn dispatch(command: Commands) -> Result<(), CoreError> {
    match command {
        Commands::Init(a) => commands::init::run(a),
        Commands::Check(a) => commands::check::run(a),
        Commands::Render(a) => commands::render::run(a),
        Commands::Probe(a) => commands::probe::run(a),
    }
}

fn exit_code(err: &CoreError) -> i32 {
    match err {
        CoreError::MissingFile { .. }
        | CoreError::ImageTooSmall { .. }
        | CoreError::InvalidProject { .. } => 1,
        CoreError::UnsupportedAudio { .. } => 2,
        CoreError::FfmpegFailed { .. } => 3,
        _ => 4,
    }
}
