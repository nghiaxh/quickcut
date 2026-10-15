use crate::commands::util;
use qc_core::CoreError;
use std::path::PathBuf;

#[derive(clap::Args, Debug)]
pub struct Args {
    pub file: PathBuf,
}

pub fn run(args: Args) -> Result<(), CoreError> {
    let ext = args
        .file
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase();

    if util::is_image_ext(&ext) {
        let info = qc_core::probe_image(&args.file)?;
        println!("image: {}x{}", info.width, info.height);
    } else if util::is_audio_ext(&ext) {
        let info = qc_core::probe_audio(&args.file)?;
        println!(
            "audio: {} Hz, {} ch, {:.3}s ({} samples)",
            info.sample_rate, info.channels, info.duration_secs, info.duration_samples
        );
    } else {
        return Err(CoreError::UnsupportedAudio {
            path: util::posix(&args.file),
            reason: "not a supported image or audio file".to_string(),
        });
    }
    Ok(())
}
