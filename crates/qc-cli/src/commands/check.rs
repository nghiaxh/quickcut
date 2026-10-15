use crate::commands::util;
use qc_core::CoreError;
use std::path::PathBuf;

#[derive(clap::Args, Debug)]
pub struct Args {
    pub target: PathBuf,
}

pub fn run(args: Args) -> Result<(), CoreError> {
    let (project, base) = util::load_or_scan(&args.target)?;
    project.validate()?;

    let mut inputs = Vec::new();
    let mut warnings = Vec::new();

    for (i, scene) in project.scenes.iter().enumerate() {
        let image_path = base.join(&scene.image);
        if !image_path.exists() {
            return Err(CoreError::MissingFile {
                path: util::posix(&image_path),
                reason: "scene image missing".to_string(),
            });
        }
        let img = qc_core::probe_image(&image_path)?;
        if img.width < 480 {
            warnings.push(format!("scene {}: image smaller than 480px wide", i + 1));
        }

        let voice_samples = if let Some(voice) = &scene.voice {
            let voice_path = base.join(voice);
            if !voice_path.exists() {
                return Err(CoreError::MissingFile {
                    path: util::posix(&voice_path),
                    reason: "scene voice missing".to_string(),
                });
            }
            let info = qc_core::probe_audio(&voice_path)?;
            if info.rms_db < -45.0 {
                warnings.push(format!(
                    "scene {}: voice is nearly silent ({:.1} dBFS)",
                    i + 1,
                    info.rms_db
                ));
            }
            // scroll speed warning
            let scaled_w = project
                .output
                .width
                .saturating_sub(project.layout.margin_x * 2);
            if img.width > 0 {
                let scaled_h = img.height as f32 * (scaled_w as f32 / img.width as f32);
                if scaled_h as u32 > project.output.height {
                    let secs = info.duration_samples as f64 / 48000.0;
                    let distance = scaled_h as f64 - project.output.height as f64;
                    let speed = distance / secs.max(0.01);
                    if speed > 600.0 {
                        warnings.push(format!(
                            "scene {}: scroll speed {:.0} px/s exceeds 600 px/s",
                            i + 1,
                            speed
                        ));
                    }
                }
            }
            Some(info.duration_samples)
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
            (span.voice_end.0 - span.voice_start.0) as f64 / 48000.0,
            span.start.0 as f64 / 48000.0,
            span.end.0 as f64 / 48000.0
        );
    }
    println!("total: {:.2}s", timeline.total.0 as f64 / 48000.0);

    for w in warnings {
        println!("WARN: {w}");
    }

    Ok(())
}
