mod commands;

use clap::{Parser, Subcommand};
use commands::render::ArgError;
use qc_core::CoreError;
use qc_export::ExportError;

#[derive(Parser)]
#[command(name = "quickcut", version, about = "QuickCut media renderer")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
    /// Log more: -v for info, -vv for debug.
    #[arg(short, long, global = true, action = clap::ArgAction::Count)]
    verbose: u8,
}

#[derive(Subcommand)]
enum Commands {
    Init(commands::init::Args),
    Check(commands::check::Args),
    Render(commands::render::Args),
    Probe(commands::probe::Args),
}

fn main() {
    let cli = Cli::parse();
    init_tracing(cli.verbose);

    if let Err(e) = run(cli.command) {
        eprintln!("error: {e:#}");
        std::process::exit(exit_code(&e));
    }
}

fn init_tracing(verbose: u8) {
    let level = match verbose {
        0 => tracing::Level::WARN,
        1 => tracing::Level::INFO,
        _ => tracing::Level::DEBUG,
    };
    let _ = tracing_subscriber::fmt()
        .with_max_level(level)
        .with_writer(std::io::stderr)
        .try_init();
}

fn run(command: Commands) -> anyhow::Result<()> {
    match command {
        Commands::Init(a) => commands::init::run(a),
        Commands::Check(a) => commands::check::run(a),
        Commands::Render(a) => commands::render::run(a),
        Commands::Probe(a) => commands::probe::run(a),
    }
}

/// Maps the error to the exit codes fixed by the CLI contract: 1 input, 2 decode,
/// 3 ffmpeg, 4 anything else.
fn exit_code(err: &anyhow::Error) -> i32 {
    if let Some(e) = err.downcast_ref::<ExportError>() {
        return e.exit_code();
    }
    if let Some(_e) = err.downcast_ref::<ArgError>() {
        return 1;
    }
    if let Some(e) = err.downcast_ref::<CoreError>() {
        return e.exit_code();
    }
    4
}
