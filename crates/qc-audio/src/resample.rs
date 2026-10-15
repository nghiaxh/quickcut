use crate::AudioError;
use qc_core::SAMPLE_RATE;

/// Resample `input` (at `in_rate`) to 48 kHz, returning exactly the sample count
/// a same-duration clip should have.
pub fn resample(
    input: &[f32],
    in_rate: usize,
    in_channels: usize,
    out_channels: usize,
) -> Result<Vec<f32>, AudioError> {
    if input.is_empty() {
        return Ok(Vec::new());
    }
    let in_channels = in_channels.max(1);
    let in_frames = input.len() / in_channels;
    if in_rate == SAMPLE_RATE as usize {
        if in_channels == out_channels {
            return Ok(input.to_vec());
        }
        return Ok(map_channels(input, in_frames, in_channels, out_channels));
    }

    // planar input
    let mut planar = vec![Vec::with_capacity(in_frames); in_channels];
    for f in 0..in_frames {
        for c in 0..in_channels {
            planar[c].push(input[f * in_channels + c] as f64);
        }
    }

    let out_frames = expected_out_frames(in_frames, in_rate);
    let resampled = resample_to(&planar, in_rate, out_frames)?;
    let out = interleave(&resampled, out_frames, out_channels);
    Ok(out)
}

/// Where the audio must land: 48 kHz frames for the same wall clock duration.
fn expected_out_frames(in_frames: usize, in_rate: usize) -> usize {
    ((in_frames as u64 * SAMPLE_RATE as u64 + (in_rate as u64 / 2)) / in_rate as u64) as usize
}

/// FftFixedInOut rounds its chunk size up, so a whole-file chunk only works when
/// the length happens to line up. Resampling in fixed small chunks and feeding
/// the tail with `process_partial` always works.
fn resample_to(
    planar: &[Vec<f64>],
    in_rate: usize,
    out_frames_wanted: usize,
) -> Result<Vec<Vec<f64>>, AudioError> {
    use rubato::{FftFixedInOut, Resampler};

    const CHUNK: usize = 1024;
    let in_frames = planar[0].len();
    let nbr_channels = planar.len();

    let mut resampler =
        FftFixedInOut::<f64>::new(in_rate, SAMPLE_RATE as usize, CHUNK, nbr_channels).map_err(
            |e| AudioError::Resample {
                reason: e.to_string(),
            },
        )?;
    let chunk_in = resampler.input_frames_next();

    let mut out: Vec<Vec<f64>> = vec![Vec::with_capacity(out_frames_wanted); nbr_channels];

    let mut pos = 0;
    while pos < in_frames {
        let end = (pos + chunk_in).min(in_frames);
        let complete = end == pos + chunk_in;
        let block: Vec<Vec<f64>> = planar
            .iter()
            .map(|channel| {
                let mut slice = channel[pos..end].to_vec();
                if !complete {
                    // pad the tail so the last, partial chunk still runs
                    slice.resize(chunk_in, 0.0);
                }
                slice
            })
            .collect();
        let produced = if complete {
            resampler.process(&block, None)
        } else {
            resampler.process_partial(Some(&block), None).map(|mut b| {
                for channel in b.iter_mut() {
                    // drop the silence the partial flush appends
                    let keep = channel.len().min(out_frames_wanted.saturating_sub(0));
                    channel.truncate(keep);
                }
                b
            })
        }
        .map_err(|e| AudioError::Resample {
            reason: e.to_string(),
        })?;

        for (dst, src) in out.iter_mut().zip(produced.iter()) {
            dst.extend_from_slice(src);
        }
        pos = end;
    }

    // The FFT resampler delays by half a filter length. Left in place that pushes
    // the voice out of sync with the video, so drop it.
    let latency = resampler_delay(in_rate);
    for channel in out.iter_mut() {
        if latency > 0 && latency < channel.len() {
            channel.drain(..latency);
        }
        channel.truncate(out_frames_wanted);
        if channel.len() < out_frames_wanted {
            channel.resize(out_frames_wanted, 0.0);
        }
    }

    Ok(out)
}

fn resampler_delay(in_rate: usize) -> usize {
    // rubato's FftFixedInOut introduces fft_size_in / 2 samples of delay.
    const FFT_SIZE: usize = 1024;
    (FFT_SIZE / 2) * SAMPLE_RATE as usize / in_rate
}

fn map_channels(
    input: &[f32],
    in_frames: usize,
    in_channels: usize,
    out_channels: usize,
) -> Vec<f32> {
    let mut out = Vec::with_capacity(in_frames * out_channels);
    for f in 0..in_frames {
        for c in 0..out_channels {
            let src_c = c.min(in_channels - 1);
            out.push(input[f * in_channels + src_c]);
        }
    }
    out
}

fn interleave(planar: &[Vec<f64>], out_frames: usize, out_channels: usize) -> Vec<f32> {
    let mut out = Vec::with_capacity(out_frames * out_channels);
    for f in 0..out_frames {
        for c in 0..out_channels {
            let src_c = c.min(planar.len() - 1);
            let sample = planar[src_c].get(f).copied().unwrap_or(0.0);
            out.push(sample as f32);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: usize = 44_100;

    /// One second of a 1 kHz tone, loudest at the very start.
    fn impulse_leading_tone() -> Vec<f32> {
        (0..RATE)
            .map(|i| {
                let t = i as f64 / RATE as f64;
                (t * 1000.0 * 2.0 * std::f64::consts::PI).sin() as f32
            })
            .collect()
    }

    #[test]
    fn output_length_is_the_same_duration_at_48k() {
        let input = impulse_leading_tone();
        let out = resample(&input, RATE, 1, 2).expect("resample");
        assert_eq!(out.len(), 2 * SAMPLE_RATE as usize);
    }

    #[test]
    fn audio_starts_at_the_first_sample() {
        let input = impulse_leading_tone();
        let out = resample(&input, RATE, 1, 1).expect("resample");

        // A resampler that delays by a filter length pushes the onset late, so
        // look for where the signal actually starts.
        let onset = out
            .iter()
            .position(|s| s.abs() > 0.05)
            .unwrap_or_else(|| panic!("no signal found"));
        let onset_ms = onset as f64 * 1000.0 / SAMPLE_RATE as f64;
        assert!(
            onset_ms < 10.0,
            "onset at {onset_ms:.1} ms means the resampler delayed the voice"
        );
    }

    #[test]
    fn tone_frequency_survives_the_round_trip() {
        let input = impulse_leading_tone();
        let out = resample(&input, RATE, 1, 1).expect("resample");

        // Skip the first 100 ms so edge transients do not skew the estimate.
        let skip = SAMPLE_RATE as usize / 10;
        let window = &out[skip..skip + SAMPLE_RATE as usize / 2];
        let mut crossings = 0usize;
        for pair in window.windows(2) {
            if pair[0] <= 0.0 && pair[1] > 0.0 {
                crossings += 1;
            }
        }
        let hz = crossings as f64 * (SAMPLE_RATE as f64 / window.len() as f64);
        assert!(
            (hz - 1000.0).abs() < 20.0,
            "expected about 1000 Hz, zero crossings suggest {hz:.0} Hz"
        );
    }

    #[test]
    fn odd_lengths_do_not_crash() {
        // 44100 / gcd(44100, 48000) = 147, so most frame counts are not a whole
        // number of resampler chunks. Every one of these used to fail.
        for frames in [1usize, 146, 147, 148, 1000, 4321] {
            let input: Vec<f32> = (0..frames).map(|i| (i % 17) as f32 / 17.0).collect();
            let out = resample(&input, RATE, 1, 2).unwrap_or_else(|e| {
                panic!("{frames} frames failed: {e}");
            });
            assert!(
                out.len().is_multiple_of(2),
                "output must be interleaved stereo"
            );
        }
    }

    #[test]
    fn stereo_is_kept_and_mono_is_widened() {
        let mono: Vec<f32> = (0..1000).map(|i| (i % 13) as f32 / 13.0).collect();
        let stereo = resample(&mono, RATE, 1, 2).expect("mono to stereo");
        for pair in stereo.chunks(2) {
            assert_eq!(pair[0], pair[1], "mono must be duplicated, not offset");
        }

        let interleaved: Vec<f32> = (0..2000)
            .map(|i| if i % 2 == 0 { 1.0 } else { 0.0 })
            .collect();
        let out = resample(&interleaved, RATE, 2, 2).expect("stereo to stereo");
        assert!(out.len().is_multiple_of(2));
    }
}
