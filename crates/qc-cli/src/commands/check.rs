use crate::commands::util;
use anyhow::Context;
use qc_core::{CoreError, Fit, SceneInputs};
use std::path::PathBuf;

#[derive(clap::Args, Debug)]
pub struct Args {
    pub target: PathBuf,
}

pub fn run(args: Args) -> anyhow::Result<()> {
    let (project, base) = util::load_or_scan(&args.target)
        .with_context(|| format!("loading project from {}", args.target.display()))?;
    project.validate().context("invalid project.toml")?;

    if project.background.kind == "video" {
        let clips = qc_core::background_clips(&base, &project.background)
            .context("resolving background video")?;
        let assign = qc_core::assign_background_clips(
            clips.len(),
            project.scenes.len(),
            project.background.seed,
        );
        println!("background video: {} clip(s)", clips.len());
        for (scene, clip) in assign.iter().enumerate() {
            let name = clips[*clip]
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("?");
            println!("  scene {} -> {name}", scene + 1);
        }
    }

    for scene in &project.scenes {
        let image_path = base.join(&scene.image);
        if !image_path.exists() {
            return Err(CoreError::MissingFile {
                path: util::posix(&image_path),
                reason: "scene image missing".to_string(),
            }
            .into());
        }
        if let Some(voice) = &scene.voice {
            let voice_path = base.join(voice);
            if !voice_path.exists() {
                return Err(CoreError::MissingFile {
                    path: util::posix(&voice_path),
                    reason: "scene voice missing".to_string(),
                }
                .into());
            }
        }
    }

    tracing::debug!(
        "probing {} scenes resolved from {}",
        project.scenes.len(),
        base.display()
    );
    let probes = util::probe_scenes(&project, &base).context("probing scene media")?;
    let inputs: Vec<SceneInputs> = probes.iter().map(|p| p.inputs.clone()).collect();
    let timeline = qc_core::solve_timeline(&project, &inputs).context("solving timeline")?;

    println!(
        "{:<4} {:<20} {:<16} {:>8} {:>8} {:>8}",
        "idx", "image", "voice", "dur", "start", "end"
    );
    for (span, scene) in timeline.scenes.iter().zip(project.scenes.iter()) {
        println!(
            "{:<4} {:<20} {:<16} {:>8.2} {:>8.2} {:>8.2}",
            span.index + 1,
            scene.image,
            scene.voice.as_deref().unwrap_or("-"),
            (span.voice_end.0 - span.voice_start.0) as f64 / qc_core::SAMPLE_RATE as f64,
            span.start.0 as f64 / qc_core::SAMPLE_RATE as f64,
            span.end.0 as f64 / qc_core::SAMPLE_RATE as f64
        );
    }
    println!(
        "total: {:.2}s",
        timeline.total.0 as f64 / qc_core::SAMPLE_RATE as f64
    );

    for warning in warnings(&project, &probes) {
        println!("WARN: {warning}");
    }

    Ok(())
}

fn warnings(project: &qc_core::Project, probes: &[qc_core::SceneProbe]) -> Vec<String> {
    let mut warnings = Vec::new();
    for (i, probe) in probes.iter().enumerate() {
        if probe.inputs.image_width < qc_core::MIN_IMAGE_WIDTH {
            warnings.push(format!(
                "scene {}: image is {}px wide, below the {}px render minimum",
                i + 1,
                probe.inputs.image_width,
                qc_core::MIN_IMAGE_WIDTH
            ));
        }
        if let Some(rms_db) = probe.voice_rms_db {
            if rms_db < -45.0 {
                warnings.push(format!(
                    "scene {}: voice is nearly silent ({:.1} dBFS)",
                    i + 1,
                    rms_db
                ));
            }
        }
        let Some(voice_samples) = probe.inputs.voice_samples else {
            continue;
        };
        let fit = qc_core::fit_image(
            probe.inputs.image_width,
            probe.inputs.image_height,
            project.output.width,
            project.output.height,
            project.layout.margin_x,
        );
        if let Fit::Scroll { max_offset_px } = fit {
            let secs = voice_samples as f64 / qc_core::SAMPLE_RATE as f64;
            let speed = max_offset_px as f64 / secs.max(0.01);
            if speed > 600.0 {
                warnings.push(format!(
                    "scene {}: scroll speed {:.0} px/s exceeds 600 px/s",
                    i + 1,
                    speed
                ));
            }
        }
    }
    warnings
}
