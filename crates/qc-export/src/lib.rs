use qc_core::CoreError;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

struct TempCleanup {
    path: std::path::PathBuf,
    enabled: bool,
}

impl Drop for TempCleanup {
    fn drop(&mut self) {
        if self.enabled {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

fn write_progress_bar(produced: u64, total: u64) {
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

#[allow(clippy::too_many_arguments)]
pub fn export_video(
    project: &qc_core::Project,
    base: &Path,
    timeline: &qc_core::Timeline,
    out: &Path,
    fps: u32,
    width: u32,
    height: u32,
    preset: &str,
    ffmpeg: Option<&Path>,
    range: Option<(u64, u64)>,
    no_audio_norm: bool,
    keep_temp: bool,
    json_progress: bool,
) -> Result<(), CoreError> {
    let (out_w, out_h, ffmpeg_preset, crf) = match preset {
        "draft" => (360u32, 640u32, "ultrafast", 28i32),
        _ => (width, height, "medium", 18i32),
    };

    if json_progress {
        println!(r#"{{"stage":"probe","frame":0,"total":0,"eta_sec":0}}"#);
    } else {
        let _ = writeln!(std::io::stderr(), "probe...");
    }

    // Audio mix to a sibling WAV
    let wav_path = out.with_file_name("mix.wav");
    let mut wav_cleanup = TempCleanup {
        path: wav_path.clone(),
        enabled: !keep_temp,
    };
    qc_audio::mix_project(project, base, timeline, &wav_path, !no_audio_norm)?;

    if json_progress {
        println!(r#"{{"stage":"audio","frame":0,"total":0,"eta_sec":0}}"#);
    } else {
        let _ = writeln!(std::io::stderr(), "audio mix...");
    }

    let ffmpeg_bin = ffmpeg
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| Path::new("ffmpeg").to_path_buf());

    // Version check
    let version_ok = Command::new(&ffmpeg_bin)
        .arg("-version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !version_ok {
        return Err(CoreError::FfmpegFailed {
            log: format!(
                "ffmpeg not found or not runnable at {}; install FFmpeg or use --ffmpeg",
                ffmpeg_bin.display()
            ),
        });
    }

    let mut child = Command::new(&ffmpeg_bin)
        .args([
            "-y",
            "-f",
            "rawvideo",
            "-pix_fmt",
            "rgba",
            "-s",
            &format!("{}x{}", out_w, out_h),
            "-r",
            &fps.to_string(),
            "-i",
            "pipe:0",
            "-i",
            wav_path.to_str().unwrap_or("mix.wav"),
            "-c:v",
            "libx264",
            "-preset",
            ffmpeg_preset,
            "-crf",
            &crf.to_string(),
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
            "-b:a",
            "192k",
            "-movflags",
            "+faststart",
            "-shortest",
        ])
        .arg(out)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| CoreError::FfmpegFailed {
            log: format!("spawn ffmpeg: {e}"),
        })?;

    let mut stdin = child.stdin.take().ok_or_else(|| CoreError::FfmpegFailed {
        log: "no stdin".to_string(),
    })?;

    let stderr = child.stderr.take();
    let stderr_handle = stderr.map(|mut s| {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = std::io::Read::read_to_end(&mut s, &mut buf);
            String::from_utf8_lossy(&buf).to_string()
        })
    });

    let full_total = timeline.total.to_frame(fps);
    let (start, end) = range.unwrap_or((0, full_total));
    let total_frames = end.saturating_sub(start).min(full_total);
    if json_progress {
        println!(
            r#"{{"stage":"render","frame":0,"total":{},"eta_sec":0}}"#,
            total_frames
        );
    } else {
        write_progress_bar(0, total_frames);
    }

    let mut produced = 0u64;
    for frame_index in start..end {
        if frame_index >= full_total {
            break;
        }
        match qc_render::render_frame_rgba(project, base, timeline, frame_index, fps, out_w, out_h)
        {
            Ok(rgba) => {
                if stdin.write_all(&rgba).is_err() {
                    break;
                }
                produced += 1;
                if !json_progress {
                    write_progress_bar(produced, total_frames);
                }
                if json_progress && (produced.is_multiple_of(30) || produced == total_frames) {
                    // rough eta assuming 1 frame per 0.1s unknown; report 0
                    println!(
                        r#"{{"stage":"render","frame":{},"total":{},"eta_sec":0}}"#,
                        produced, total_frames
                    );
                }
            }
            Err(_) => break,
        }
    }

    if json_progress {
        println!(
            r#"{{"stage":"encode","frame":{},"total":{},"eta_sec":0}}"#,
            produced, total_frames
        );
    }
    drop(stdin);

    let status = child
        .wait()
        .map_err(|e| CoreError::FfmpegFailed { log: e.to_string() })?;
    let log = stderr_handle
        .and_then(|h| h.join().ok())
        .unwrap_or_default();
    let tail: String = log
        .lines()
        .rev()
        .take(10)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join("\n");
    if !status.success() {
        return Err(CoreError::FfmpegFailed { log: tail });
    }

    if keep_temp {
        wav_cleanup.enabled = false;
    }

    if json_progress {
        println!(
            r#"{{"stage":"done","frame":{},"total":{},"eta_sec":0}}"#,
            produced, total_frames
        );
    }
    Ok(())
}
