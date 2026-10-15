use qc_core::CoreError;
use std::path::PathBuf;

#[derive(clap::Args, Debug)]
pub struct Args {
    #[arg(value_name = "PROJECT")]
    pub project: PathBuf,
    #[arg(short = 'o', long = "output", default_value = "out.mp4")]
    pub output: PathBuf,
    #[arg(long, default_value = "final")]
    pub preset: String,
    #[arg(long, default_value_t = 30)]
    pub fps: u32,
    #[arg(long, default_value = "1080x1920")]
    pub size: String,
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
    #[arg(short, long, action = clap::ArgAction::Count)]
    pub verbose: u8,
    #[arg(long)]
    pub json_progress: bool,
}

pub fn run(args: Args) -> Result<(), CoreError> {
    let (project, base) = crate::commands::util::load_or_scan(&args.project)?;
    project.validate()?;

    let mut inputs = Vec::new();
    for scene in &project.scenes {
        let img = qc_core::probe_image(&base.join(&scene.image))?;
        let voice_samples = if let Some(v) = &scene.voice {
            Some(qc_core::probe_audio(&base.join(v))?.duration_samples)
        } else {
            None
        };
        inputs.push(qc_core::SceneInputs {
            voice_samples,
            image_width: img.width,
            image_height: img.height,
        });
    }
    let timeline = qc_core::solve_timeline(&project, &inputs)?;

    let (width, height) =
        parse_size(&args.size).unwrap_or((project.output.width, project.output.height));
    let fps = if args.fps == 0 {
        project.output.fps
    } else {
        args.fps
    };

    if let Some(n) = args.threads {
        let _ = rayon::ThreadPoolBuilder::new()
            .num_threads(n)
            .build_global();
    }

    // Convert scene range (1-based) into frame bounds.
    let range = if let Some(r) = &args.range {
        let mut parts = r.split(':');
        let a: usize = parts.next().and_then(|s| s.parse().ok()).unwrap_or(1);
        let b: usize = parts
            .next()
            .and_then(|s| s.parse().ok())
            .unwrap_or(project.scenes.len());
        let a = a
            .saturating_sub(1)
            .min(timeline.scenes.len().saturating_sub(1));
        let b = b
            .saturating_sub(1)
            .min(timeline.scenes.len().saturating_sub(1));
        let start_frame = timeline.scenes[a].start.to_frame(fps);
        let end_frame = timeline.scenes[b].end.to_frame(fps);
        Some((start_frame, end_frame))
    } else {
        None
    };

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
        args.json_progress,
    )?;

    println!("wrote {}", args.output.display());
    Ok(())
}

fn parse_size(s: &str) -> Option<(u32, u32)> {
    let (w, h) = s.split_once('x')?;
    Some((w.parse().ok()?, h.parse().ok()?))
}
