use crate::commands::util;
use anyhow::Context;
use qc_core::{Progress, SceneInputs, Stage, Timeline};
use std::io::Write;
use std::path::PathBuf;
use thiserror::Error;

#[derive(clap::Args, Debug)]
pub struct Args {
    #[arg(value_name = "PROJECT")]
    pub project: PathBuf,
    #[arg(short = 'o', long = "output", default_value = "out.mp4")]
    pub output: PathBuf,
    #[arg(long, default_value = "final")]
    pub preset: String,
    #[arg(long)]
    pub fps: Option<u32>,
    #[arg(long)]
    pub size: Option<String>,
    #[arg(long)]
    pub range: Option<String>,
    #[arg(long)]
    pub threads: Option<usize>,
    #[arg(long)]
    pub ffmpeg: Option<PathBuf>,
    #[arg(long)]
    pub no_audio_norm: bool,
    #[arg(long)]
    pub keep_temp: bool,
    #[arg(long)]
    pub json_progress: bool,
}

/// A flag the user typed that could not be parsed. Never guessed at silently.
#[derive(Debug, Error)]
pub enum ArgError {
    #[error("invalid --size {0:?}: expected WIDTHxHEIGHT, for example 1080x1920")]
    Size(String),
    #[error("invalid --range {0:?}: expected FIRST:LAST, for example 1:3")]
    Range(String),
    #[error("unknown --preset {0:?}: expected draft or final")]
    Preset(String),
}

pub fn run(args: Args) -> anyhow::Result<()> {
    let (project, base) = util::load_or_scan(&args.project)
        .with_context(|| format!("loading project from {}", args.project.display()))?;
    project.validate().context("invalid project.toml")?;

    if !matches!(args.preset.as_str(), "draft" | "final") {
        return Err(ArgError::Preset(args.preset.clone()).into());
    }

    // Must happen before any rayon work: the global pool can only be sized once,
    // and probing scenes is already parallel.
    if let Some(n) = args.threads {
        let _ = rayon::ThreadPoolBuilder::new()
            .num_threads(n)
            .build_global();
    }

    let probes = util::probe_scenes(&project, &base).context("probing scene media")?;
    let inputs: Vec<SceneInputs> = probes.iter().map(|p| p.inputs.clone()).collect();
    let timeline = qc_core::solve_timeline(&project, &inputs).context("solving timeline")?;

    let (width, height) = match &args.size {
        Some(s) => parse_size(s).ok_or_else(|| ArgError::Size(s.to_string()))?,
        None => (project.output.width, project.output.height),
    };
    let fps = args.fps.unwrap_or(project.output.fps);

    let range = match &args.range {
        Some(r) => Some(parse_range(r, &timeline, fps)?),
        None => None,
    };

    let total_frames = timeline.total.to_frame(fps);
    tracing::debug!(
        "{} scenes, {} frames at {} fps, {}x{}",
        timeline.scenes.len(),
        total_frames,
        fps,
        width,
        height
    );
    tracing::info!(
        "rendering {} to {} with preset {}",
        args.project.display(),
        args.output.display(),
        args.preset
    );

    let mut progress = ProgressPrinter::new(args.json_progress);
    qc_export::export_video(
        &project,
        &base,
        &timeline,
        &args.output,
        fps,
        width,
        height,
        &args.preset,
        args.ffmpeg.as_deref(),
        range,
        args.no_audio_norm,
        args.keep_temp,
        None,
        &mut |p| progress.update(p),
    )
    .with_context(|| format!("exporting {}", args.output.display()))?;

    println!("wrote {}", args.output.display());
    Ok(())
}

/// Turns pipeline reports into either JSON lines (stable, for the desktop app)
/// or a terminal progress bar.
struct ProgressPrinter {
    json: bool,
    on_bar: bool,
}

impl ProgressPrinter {
    fn new(json: bool) -> Self {
        Self {
            json,
            on_bar: false,
        }
    }

    fn update(&mut self, p: Progress) {
        if self.json {
            if let Ok(line) = serde_json::to_string(&p) {
                println!("{line}");
            }
            return;
        }
        match p.stage {
            Stage::Render => {
                self.on_bar = true;
                draw_bar(p.frame, p.total);
            }
            Stage::Probe | Stage::Audio | Stage::Encode | Stage::Done => {
                if self.on_bar {
                    // Close the bar so the next line does not overwrite it.
                    let _ = writeln!(std::io::stderr());
                    self.on_bar = false;
                }
                let _ = writeln!(std::io::stderr(), "{}...", p.stage.as_str());
            }
        }
    }
}

fn draw_bar(produced: u64, total: u64) {
    const BAR_WIDTH: usize = 24;
    let pct = produced
        .checked_mul(100)
        .and_then(|scaled| scaled.checked_div(total))
        .unwrap_or(100)
        .min(100);
    let filled = ((BAR_WIDTH * pct as usize) / 100).min(BAR_WIDTH);
    let _ = write!(
        std::io::stderr(),
        "\rrendering: [{}{}] {:>3}% ({}/{})",
        "#".repeat(filled),
        "-".repeat(BAR_WIDTH - filled),
        pct,
        produced,
        total
    );
    let _ = std::io::stderr().flush();
}

fn parse_size(s: &str) -> Option<(u32, u32)> {
    let (w, h) = s.split_once('x')?;
    Some((w.parse().ok()?, h.parse().ok()?))
}

/// `1:3` means scenes 1 through 3 inclusive; frames come from the solved timeline.
fn parse_range(s: &str, timeline: &Timeline, fps: u32) -> Result<(u64, u64), ArgError> {
    let bad = || {
        ArgError::Range(format!(
            "{s} (project has {} scenes)",
            timeline.scenes.len()
        ))
    };
    let mut parts = s.split(':');
    let (Some(first), Some(last), None) = (parts.next(), parts.next(), parts.next()) else {
        return Err(bad());
    };
    let (Ok(first), Ok(last)) = (first.trim().parse::<usize>(), last.trim().parse::<usize>())
    else {
        return Err(bad());
    };
    if first == 0 || last == 0 || first > last || last > timeline.scenes.len() {
        return Err(bad());
    }
    Ok((
        timeline.scenes[first - 1].start.to_frame(fps),
        timeline.scenes[last - 1].end.to_frame(fps),
    ))
}
