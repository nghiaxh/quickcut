pub const SAMPLE_RATE: u32 = 48_000;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Samples(pub u64);

impl Samples {
    pub fn from_secs(s: f64) -> Self {
        Samples((s * SAMPLE_RATE as f64).round() as u64)
    }

    pub fn to_frame(self, fps: u32) -> u64 {
        self.0 * fps as u64 / SAMPLE_RATE as u64
    }
}
