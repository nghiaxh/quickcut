use serde::Serialize;

/// Stage of the render pipeline. Names are part of the `--json-progress` CLI contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Stage {
    Probe,
    Audio,
    Render,
    Encode,
    Done,
}

impl Stage {
    pub fn as_str(self) -> &'static str {
        match self {
            Stage::Probe => "probe",
            Stage::Audio => "audio",
            Stage::Render => "render",
            Stage::Encode => "encode",
            Stage::Done => "done",
        }
    }
}

/// Progress report emitted while exporting. The desktop app consumes the same shape.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Progress {
    pub stage: Stage,
    pub frame: u64,
    pub total: u64,
    pub eta_sec: u64,
}
