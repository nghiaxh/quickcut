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
    Io(std::io::Error),
}

impl From<std::io::Error> for CoreError {
    // Hand-written, not `#[from]`: the Display already carries the io message, so
    // marking it as a source too would print it twice once anyhow walks the chain.
    fn from(e: std::io::Error) -> Self {
        CoreError::Io(e)
    }
}

impl CoreError {
    /// Process exit code. Part of the CLI contract.
    pub fn exit_code(&self) -> i32 {
        match self {
            CoreError::MissingFile { .. }
            | CoreError::ImageTooSmall { .. }
            | CoreError::InvalidProject { .. } => 1,
            CoreError::UnsupportedAudio { .. } => 2,
            CoreError::FfmpegFailed { .. } => 3,
            _ => 4,
        }
    }
}
