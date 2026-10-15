use qc_core::CoreError;
use std::path::{Path, PathBuf};

const SAMPLE_RATE: u32 = qc_core::SAMPLE_RATE;

#[derive(Debug, Clone)]
pub struct DecodedAudio {
    pub sample_rate: u32,
    pub channels: usize,
    pub samples: Vec<f32>, // interleaved
}

pub fn db_to_amp(db: f64) -> f32 {
    10f64.powf(db / 20.0) as f32
}

pub fn mix_project(
    project: &qc_core::Project,
    base: &Path,
    timeline: &qc_core::Timeline,
    out: &Path,
    normalize: bool,
) -> Result<(), CoreError> {
    let total_frames = timeline.total.0 as usize;
    let channels = 2usize;
    let mut voice_bus = vec![0f32; total_frames * channels];
    let mut sfx_bus = vec![0f32; total_frames * channels];
    let mut bgm_bus = vec![0f32; total_frames * channels];

    let voice_gain = db_to_amp(project.audio.voice_gain_db);
    let sfx_gain = db_to_amp(project.audio.sfx_gain_db);

    // Voice
    for (i, scene) in project.scenes.iter().enumerate() {
        let Some(rel) = scene.voice.as_ref() else {
            continue;
        };
        let path = base.join(rel);
        let decoded = decode_audio(&path)?;
        let mut resampled = resample(
            &decoded.samples,
            decoded.sample_rate as usize,
            decoded.channels,
            channels,
        )?;
        for s in &mut resampled {
            *s *= voice_gain;
        }
        let span = &timeline.scenes[i];
        let start = span.voice_start.0 as usize;
        let end = span.voice_end.0 as usize;
        let src_frames = resampled.len() / channels;
        for f in 0..src_frames {
            let dst = start + f;
            if dst >= end || dst >= total_frames {
                break;
            }
            for c in 0..channels {
                voice_bus[dst * channels + c] += resampled[f * channels + c];
            }
        }
    }

    // Cues / SFX
    for cue in &timeline.cues {
        let path = resolve_audio(base, &cue.sfx);
        let decoded = decode_audio(&path)?;
        let mut resampled = resample(
            &decoded.samples,
            decoded.sample_rate as usize,
            decoded.channels,
            channels,
        )?;
        let gain = db_to_amp(project.audio.sfx_gain_db + cue.gain_db);
        for s in &mut resampled {
            *s *= gain * sfx_gain;
        }
        let start = cue.at.0 as usize;
        let src_frames = resampled.len() / channels;
        for f in 0..src_frames {
            let dst = start + f;
            if dst >= total_frames {
                break;
            }
            for c in 0..channels {
                sfx_bus[dst * channels + c] += resampled[f * channels + c];
            }
        }
    }

    // BGM
    if let Some(bgm) = &project.audio.bgm {
        if !bgm.file.trim().is_empty() {
            let path = base.join(&bgm.file);
            let decoded = decode_audio(&path)?;
            let mut resampled = resample(
                &decoded.samples,
                decoded.sample_rate as usize,
                decoded.channels,
                channels,
            )?;
            let base_gain = db_to_amp(bgm.gain_db);
            for s in &mut resampled {
                *s *= base_gain;
            }

            // loop
            let bgm_frames = resampled.len() / channels;
            if bgm_frames > 0 {
                // fade helpers (linear)
                let fade_in_frames = (bgm.fade_in * SAMPLE_RATE as f64).round() as usize;
                let fade_out_frames = (bgm.fade_out * SAMPLE_RATE as f64).round() as usize;

                for f in 0..total_frames {
                    let src_f = f % bgm_frames;
                    for c in 0..channels {
                        let mut v = resampled[src_f * channels + c];
                        // fade in
                        if f < fade_in_frames {
                            v *= f as f32 / fade_in_frames.max(1) as f32;
                        }
                        // fade out
                        if f + fade_out_frames > total_frames {
                            let rem = total_frames - f;
                            v *= rem as f32 / fade_out_frames.max(1) as f32;
                        }
                        bgm_bus[f * channels + c] = v;
                    }
                }
            }
        }
    }

    // Ducking envelope from voice bus
    let env = duck_envelope(&voice_bus, total_frames, channels);
    if let Some(bgm) = &project.audio.bgm {
        for f in 0..total_frames {
            let g = db_to_amp(bgm.duck_db * env[f] as f64);
            for c in 0..channels {
                bgm_bus[f * channels + c] *= g;
            }
        }
    }

    // Sum buses -> master
    let mut master = vec![0f32; total_frames * channels];
    for i in 0..master.len() {
        master[i] = voice_bus[i] + sfx_bus[i] + bgm_bus[i];
    }

    // Loudness normalization
    if normalize {
        let measured = measure_lufs(&master, channels, SAMPLE_RATE);
        if measured.is_finite() {
            let target = project.audio.master.target_lufs;
            let gain_db = target - measured;
            let gain = db_to_amp(gain_db);
            for s in &mut master {
                *s *= gain;
            }
        }
    }

    // Limiter (sample peak fallback)
    let peak_target = db_to_amp(project.audio.master.true_peak_db);
    let peak = master.iter().cloned().fold(0.0f32, f32::max);
    if peak > peak_target && peak > 0.0 {
        let gain = peak_target / peak;
        for s in &mut master {
            *s *= gain;
        }
    }

    write_wav(out, &master, channels, SAMPLE_RATE)?;
    Ok(())
}

fn resolve_audio(base: &Path, rel: &str) -> PathBuf {
    let direct = base.join(rel);
    if direct.exists() {
        return direct;
    }
    let in_sfx = base.join("sfx").join(rel);
    if in_sfx.exists() {
        return in_sfx;
    }

    // SFX override: allow callers to refer to a file in sfx/ by stem only.
    let path = std::path::Path::new(rel);
    if path.extension().is_none() {
        for ext in ["wav", "mp3", "ogg", "m4a", "flac"] {
            let base_candidate = base.join(format!("{rel}.{ext}"));
            if base_candidate.exists() {
                return base_candidate;
            }
            let sfx_candidate = base.join("sfx").join(format!("{rel}.{ext}"));
            if sfx_candidate.exists() {
                return sfx_candidate;
            }
        }
    } else if let Some(name) = path.file_name() {
        let sfx_override = base.join("sfx").join(name);
        if sfx_override.exists() {
            return sfx_override;
        }
    }

    direct
}

fn decode_audio(path: &Path) -> Result<DecodedAudio, CoreError> {
    use symphonia::core::formats::FormatOptions;
    use symphonia::core::io::MediaSourceStream;
    use symphonia::core::meta::MetadataOptions;
    use symphonia::core::probe::Hint;

    let file = std::fs::File::open(path).map_err(|e| CoreError::MissingFile {
        path: path.display().to_string(),
        reason: e.to_string(),
    })?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());

    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }

    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|e| CoreError::UnsupportedAudio {
            path: path.display().to_string(),
            reason: e.to_string(),
        })?;

    let mut format = probed.format;
    let track = format
        .default_track()
        .ok_or_else(|| CoreError::UnsupportedAudio {
            path: path.display().to_string(),
            reason: "no default track".to_string(),
        })?;
    let track_id = track.id;
    let mut decoder = symphonia::default::get_codecs()
        .make(
            &track.codec_params,
            &symphonia::core::codecs::DecoderOptions::default(),
        )
        .map_err(|e| CoreError::UnsupportedAudio {
            path: path.display().to_string(),
            reason: e.to_string(),
        })?;

    let mut sample_rate = track.codec_params.sample_rate.unwrap_or(SAMPLE_RATE);
    let mut channels = track.codec_params.channels.map(|c| c.count()).unwrap_or(1);

    let mut samples: Vec<f32> = Vec::new();

    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(symphonia::core::errors::Error::IoError(e))
                if e.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break;
            }
            Err(_) => break,
        };
        if packet.track_id() != track_id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(d) => d,
            Err(_) => continue,
        };

        let spec = *decoded.spec();
        sample_rate = spec.rate;
        channels = spec.channels.count();

        let mut sample_buf = symphonia::core::audio::SampleBuffer::<f32>::new(
            decoded.frames() as u64,
            *decoded.spec(),
        );
        sample_buf.copy_interleaved_ref(decoded);
        samples.extend_from_slice(sample_buf.samples());
    }

    Ok(DecodedAudio {
        sample_rate,
        channels,
        samples,
    })
}

fn resample(
    input: &[f32],
    in_rate: usize,
    in_channels: usize,
    out_channels: usize,
) -> Result<Vec<f32>, CoreError> {
    if input.is_empty() {
        return Ok(Vec::new());
    }
    let in_frames = input.len() / in_channels.max(1);
    if in_rate == SAMPLE_RATE as usize {
        // channel adjust only
        if in_channels == out_channels {
            return Ok(input.to_vec());
        }
        let mut out = Vec::with_capacity(in_frames * out_channels);
        for f in 0..in_frames {
            for c in 0..out_channels {
                let src_c = c.min(in_channels.saturating_sub(1));
                out.push(input[f * in_channels + src_c]);
            }
        }
        return Ok(out);
    }

    // planar input
    let mut planar = vec![Vec::with_capacity(in_frames); in_channels];
    for f in 0..in_frames {
        for c in 0..in_channels {
            planar[c].push(input[f * in_channels + c] as f64);
        }
    }

    use rubato::{FftFixedInOut, Resampler};
    let mut resampler =
        FftFixedInOut::<f64>::new(in_rate, SAMPLE_RATE as usize, in_frames, in_channels).map_err(
            |e| CoreError::UnsupportedAudio {
                path: "resample".to_string(),
                reason: e.to_string(),
            },
        )?;
    let out_planar = resampler
        .process(&planar, None)
        .map_err(|e| CoreError::UnsupportedAudio {
            path: "resample".to_string(),
            reason: e.to_string(),
        })?;

    let out_frames = if out_planar.is_empty() {
        0
    } else {
        out_planar[0].len()
    };
    let mut out = vec![0f32; out_frames * out_channels];
    for f in 0..out_frames {
        for c in 0..out_channels {
            let src_c = c.min(in_channels.saturating_sub(1));
            out[f * out_channels + c] = out_planar[src_c][f] as f32;
        }
    }
    Ok(out)
}

fn duck_envelope(voice: &[f32], total_frames: usize, channels: usize) -> Vec<f32> {
    let window = (SAMPLE_RATE as usize / 50).max(1); // 20ms
    let threshold = 10f32.powf(-45.0 / 20.0);
    let mut raw = vec![0f32; total_frames];
    for start in (0..total_frames).step_by(window) {
        let end = (start + window).min(total_frames);
        let mut sumsq = 0.0f32;
        let mut count = 0usize;
        for f in start..end {
            for c in 0..channels {
                let x = voice[f * channels + c];
                sumsq += x * x;
                count += 1;
            }
        }
        let rms = if count > 0 {
            (sumsq / count as f32).sqrt()
        } else {
            0.0
        };
        let val = if rms > threshold { 1.0 } else { 0.0 };
        raw[start..end].fill(val);
    }

    let attack_coeff = 1.0 - (-1.0f32 / (0.05f32 * SAMPLE_RATE as f32)).exp();
    let release_coeff = 1.0 - (-1.0f32 / (0.3f32 * SAMPLE_RATE as f32)).exp();
    let mut env = vec![0f32; total_frames];
    let mut current = 0.0f32;
    for f in 0..total_frames {
        let target = raw[f];
        let coeff = if target > current {
            attack_coeff
        } else {
            release_coeff
        };
        current += (target - current) * coeff;
        env[f] = current;
    }
    env
}

fn measure_lufs(master: &[f32], channels: usize, sample_rate: u32) -> f64 {
    use ebur128::{EbuR128, Mode};
    let mut ebur = match EbuR128::new(channels as u32, sample_rate, Mode::I | Mode::TRUE_PEAK) {
        Ok(e) => e,
        Err(_) => return f64::NAN, // NaN
    };
    // ebur128 expects interleaved f32 frames
    if ebur.add_frames_f32(master).is_err() {
        return f64::NAN;
    }
    ebur.loudness_global().unwrap_or(f64::NAN)
}

fn write_wav(
    path: &Path,
    samples: &[f32],
    channels: usize,
    sample_rate: u32,
) -> Result<(), CoreError> {
    let spec = hound::WavSpec {
        channels: channels as u16,
        sample_rate,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut writer =
        hound::WavWriter::create(path, spec).map_err(|e| CoreError::InvalidProject {
            field: "wav".to_string(),
            reason: e.to_string(),
        })?;
    for s in samples {
        writer
            .write_sample(*s)
            .map_err(|e| CoreError::InvalidProject {
                field: "wav".to_string(),
                reason: e.to_string(),
            })?;
    }
    writer.finalize().map_err(|e| CoreError::InvalidProject {
        field: "wav".to_string(),
        reason: e.to_string(),
    })?;
    Ok(())
}
