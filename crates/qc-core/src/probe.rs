use crate::CoreError;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct AudioInfo {
    pub sample_rate: u32,
    pub channels: usize,
    pub duration_samples: u64,
    pub duration_secs: f64,
    pub rms_db: f64,
}

#[derive(Debug, Clone)]
pub struct ImageInfo {
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone)]
pub struct SceneInputs {
    pub voice_samples: Option<u64>,
    pub image_width: u32,
    pub image_height: u32,
}

pub fn probe_image(path: &Path) -> Result<ImageInfo, CoreError> {
    let (width, height) = image::image_dimensions(path).map_err(|e| CoreError::MissingFile {
        path: path.display().to_string(),
        reason: e.to_string(),
    })?;
    Ok(ImageInfo { width, height })
}

pub fn probe_audio(path: &Path) -> Result<AudioInfo, CoreError> {
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

    let mut sample_rate = track.codec_params.sample_rate.unwrap_or(crate::SAMPLE_RATE);
    let mut channels = track.codec_params.channels.map(|c| c.count()).unwrap_or(1);

    let mut sumsq = 0f64;
    let mut nsamples = 0usize;

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
        for x in sample_buf.samples() {
            sumsq += (*x as f64) * (*x as f64);
            nsamples += 1;
        }
    }

    let duration_samples = nsamples as u64 / channels.max(1) as u64;
    let rms = if nsamples > 0 {
        (sumsq / nsamples as f64).sqrt()
    } else {
        0.0
    };
    let rms_db = if rms > 0.0 {
        20.0 * rms.log10()
    } else {
        -120.0
    };

    Ok(AudioInfo {
        sample_rate,
        channels,
        duration_samples,
        duration_secs: duration_samples as f64 / sample_rate as f64,
        rms_db,
    })
}
