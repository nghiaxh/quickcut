use thiserror::Error;

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("missing file {path}: {reason}")]
    MissingFile { path: String, reason: String },
    #[error("unsupported audio {path}: {reason}")]
    UnsupportedAudio { path: String, reason: String },
    #[error("image too small {path}: {width}x{height}")]
    ImageTooSmall {
        path: String,
        width: u32,
        height: u32,
    },
    #[error("ffmpeg failed: {log}")]
    FfmpegFailed { log: String },
    #[error("invalid project field {field}: {reason}")]
    InvalidProject { field: String, reason: String },
    #[error("not implemented: {feature}")]
    NotImplemented { feature: String },
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}
