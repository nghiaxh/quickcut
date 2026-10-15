use qc_audio::AudioError;
use qc_core::{Progress, Stage};
use qc_render::RenderError;
use rayon::prelude::*;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Instant;
use thiserror::Error;

/// Errors owned by the export stage.
#[derive(Debug, Error)]
pub enum ExportError {
    #[error("ffmpeg not found or not runnable at {0}; install FFmpeg or pass --ffmpeg")]
    Missing(String),
    #[error("ffmpeg failed:\n{log}")]
    Ffmpeg { log: String },
    // Inner errors are written into the message, not chained, so printing the
    // anyhow chain does not repeat them.
    #[error("cannot start ffmpeg: {0}")]
    Spawn(std::io::Error),
    #[error("ffmpeg stdin is unavailable")]
    NoStdin,
    #[error("cannot wait for ffmpeg: {0}")]
    Wait(std::io::Error),
    #[error("frame {frame} failed to render: {cause}")]
    Frame { frame: u64, cause: RenderError },
    #[error(transparent)]
    Audio(#[from] AudioError),
}

impl ExportError {
    /// Process exit code. Part of the CLI contract.
    pub fn exit_code(&self) -> i32 {
        match self {
            ExportError::Missing(_) | ExportError::Ffmpeg { .. } => 3,
            ExportError::Frame { cause, .. } => cause.exit_code(),
            ExportError::Audio(e) => e.exit_code(),
            ExportError::Spawn(_) | ExportError::NoStdin | ExportError::Wait(_) => 4,
        }
    }
}

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

fn eta_sec(done: u64, total: u64, started: Instant) -> u64 {
    if done == 0 || total <= done {
        return 0;
    }
    (started.elapsed().as_secs_f64() * (total - done) as f64 / done as f64) as u64
}

/// How many frames render at once. At 1080p one frame is 8 MB, so the round is
/// capped by memory as well as by the worker count.
fn block_size(width: u32, height: u32) -> usize {
    let frame_bytes = (width as usize * height as usize * 4).max(1);
    let memory_cap = (128 * 1024 * 1024) / frame_bytes;
    memory_cap.clamp(2, rayon::current_num_threads().max(2))
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
    on_progress: &mut dyn FnMut(Progress),
) -> Result<(), ExportError> {
    let (out_w, out_h, ffmpeg_preset, crf) = match preset {
        "draft" => (360u32, 640u32, "ultrafast", 28i32),
        _ => (width, height, "medium", 18i32),
    };

    let full_total = timeline.total.to_frame(fps);
    let (start, end) = range.unwrap_or((0, full_total));
    let total_frames = end.saturating_sub(start).min(full_total);

    on_progress(Progress {
        stage: Stage::Probe,
        frame: 0,
        total: full_total,
        eta_sec: 0,
    });
    on_progress(Progress {
        stage: Stage::Audio,
        frame: 0,
        total: full_total,
        eta_sec: 0,
    });

    // Audio mix to a sibling WAV
    let wav_path = out.with_file_name("mix.wav");
    let mut wav_cleanup = TempCleanup {
        path: wav_path.clone(),
        enabled: !keep_temp,
    };
    qc_audio::mix_project(project, base, timeline, &wav_path, !no_audio_norm)?;

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
        return Err(ExportError::Missing(ffmpeg_bin.display().to_string()));
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
        .map_err(ExportError::Spawn)?;

    let mut stdin = child.stdin.take().ok_or(ExportError::NoStdin)?;

    let stderr = child.stderr.take();
    let stderr_handle = stderr.map(|mut s| {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = std::io::Read::read_to_end(&mut s, &mut buf);
            String::from_utf8_lossy(&buf).to_string()
        })
    });

    on_progress(Progress {
        stage: Stage::Render,
        frame: 0,
        total: total_frames,
        eta_sec: 0,
    });

    let started = Instant::now();
    let mut produced = 0u64;
    let mut pipe_closed = false;
    let frame_bytes = out_w as usize * out_h as usize * 4;
    let frames: Vec<u64> = (start..end).take_while(|f| *f < full_total).collect();
    let block = block_size(out_w, out_h);

    'rounds: for round in frames.chunks(block) {
        // Frames render in parallel, then go to ffmpeg strictly in order so the
        // output stays byte for byte the same as a single threaded render.
        let rendered: Result<Vec<Vec<u8>>, ExportError> = round
            .par_iter()
            .map(|&frame_index| {
                let mut rgba = Vec::with_capacity(frame_bytes);
                qc_render::render_frame_into(
                    &mut rgba,
                    project,
                    base,
                    timeline,
                    frame_index,
                    fps,
                    out_w,
                    out_h,
                )
                .map_err(|cause| ExportError::Frame {
                    frame: frame_index,
                    cause,
                })?;
                Ok(rgba)
            })
            .collect();

        for rgba in rendered? {
            if stdin.write_all(&rgba).is_err() {
                pipe_closed = true;
                break 'rounds;
            }
            produced += 1;
            if produced.is_multiple_of(30) || produced == total_frames {
                on_progress(Progress {
                    stage: Stage::Render,
                    frame: produced,
                    total: total_frames,
                    eta_sec: eta_sec(produced, total_frames, started),
                });
            }
        }
    }

    on_progress(Progress {
        stage: Stage::Encode,
        frame: produced,
        total: total_frames,
        eta_sec: 0,
    });
    drop(stdin);

    let status = child.wait().map_err(ExportError::Wait)?;
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
        return Err(ExportError::Ffmpeg { log: tail });
    }
    if pipe_closed {
        return Err(ExportError::Ffmpeg {
            log: format!(
                "ffmpeg closed the frame pipe after {produced}/{total_frames} frames\n{tail}"
            ),
        });
    }

    if keep_temp {
        wav_cleanup.enabled = false;
    }

    on_progress(Progress {
        stage: Stage::Done,
        frame: produced,
        total: total_frames,
        eta_sec: 0,
    });
    Ok(())
}
