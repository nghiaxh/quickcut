use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::CoreError;

fn default_schema_version() -> u32 {
    1
}
fn d_fps() -> u32 {
    30
}
fn d_width() -> u32 {
    1080
}
fn d_height() -> u32 {
    1920
}
fn d_blur() -> f64 {
    4.0
}
fn d_seed() -> u64 {
    0
}
fn d_margin() -> u32 {
    60
}
fn d_radius() -> u32 {
    28
}
fn d_shadow_blur() -> u32 {
    30
}
fn d_shadow_dy() -> i32 {
    12
}
fn d_shadow_alpha() -> f64 {
    0.35
}
fn d_pad_before() -> f64 {
    0.25
}
fn d_pad_after() -> f64 {
    0.35
}
fn d_hold() -> f64 {
    2.5
}
fn d_transition() -> f64 {
    0.30
}
fn d_voice_gain() -> f64 {
    0.0
}
fn d_sfx_gain() -> f64 {
    -8.0
}
fn d_bgm_gain() -> f64 {
    -18.0
}
fn d_duck() -> f64 {
    -10.0
}
fn d_fade_in() -> f64 {
    1.0
}
fn d_fade_out() -> f64 {
    2.0
}
fn d_target_lufs() -> f64 {
    -14.0
}
fn d_true_peak() -> f64 {
    -1.0
}
fn d_anim_in() -> String {
    "pop".to_string()
}
fn d_anim_out() -> String {
    "slide_left".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct OutputConfig {
    #[serde(default = "d_fps")]
    pub fps: u32,
    #[serde(default = "d_width")]
    pub width: u32,
    #[serde(default = "d_height")]
    pub height: u32,
}

impl Default for OutputConfig {
    fn default() -> Self {
        Self {
            fps: d_fps(),
            width: d_width(),
            height: d_height(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BackgroundConfig {
    pub kind: String,
    /// One video, or a directory of clips to pick from when `kind = "video"`.
    #[serde(default)]
    pub file: Option<String>,
    #[serde(default = "d_blur")]
    pub blur_sigma: f64,
    /// Seeds the clip-to-scene assignment so renders stay reproducible.
    #[serde(default = "d_seed")]
    pub seed: u64,
    #[serde(default = "default_true")]
    pub r#loop: bool,
}

impl Default for BackgroundConfig {
    fn default() -> Self {
        Self {
            kind: "blur_from_image".to_string(),
            file: None,
            blur_sigma: d_blur(),
            seed: d_seed(),
            r#loop: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ShadowConfig {
    #[serde(default = "d_shadow_blur")]
    pub blur: u32,
    #[serde(default = "d_shadow_dy")]
    pub dy: i32,
    #[serde(default = "d_shadow_alpha")]
    pub alpha: f64,
}

impl Default for ShadowConfig {
    fn default() -> Self {
        Self {
            blur: d_shadow_blur(),
            dy: d_shadow_dy(),
            alpha: d_shadow_alpha(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LayoutConfig {
    #[serde(default = "d_margin")]
    pub margin_x: u32,
    #[serde(default = "d_radius")]
    pub corner_radius: u32,
    #[serde(default)]
    pub shadow: ShadowConfig,
    pub long_image: String,
}

impl Default for LayoutConfig {
    fn default() -> Self {
        Self {
            margin_x: d_margin(),
            corner_radius: d_radius(),
            shadow: ShadowConfig::default(),
            long_image: "auto_scroll".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TimingConfig {
    #[serde(default = "d_pad_before")]
    pub pad_before: f64,
    #[serde(default = "d_pad_after")]
    pub pad_after: f64,
    #[serde(default = "d_hold")]
    pub default_hold: f64,
    #[serde(default = "d_transition")]
    pub transition: f64,
}

impl Default for TimingConfig {
    fn default() -> Self {
        Self {
            pad_before: d_pad_before(),
            pad_after: d_pad_after(),
            default_hold: d_hold(),
            transition: d_transition(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BgmConfig {
    pub file: String,
    #[serde(default = "d_bgm_gain")]
    pub gain_db: f64,
    #[serde(default = "d_duck")]
    pub duck_db: f64,
    #[serde(default = "d_fade_in")]
    pub fade_in: f64,
    #[serde(default = "d_fade_out")]
    pub fade_out: f64,
    #[serde(default = "default_true")]
    pub r#loop: bool,
}

fn default_true() -> bool {
    true
}

impl Default for BgmConfig {
    fn default() -> Self {
        Self {
            file: String::new(),
            gain_db: d_bgm_gain(),
            duck_db: d_duck(),
            fade_in: d_fade_in(),
            fade_out: d_fade_out(),
            r#loop: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct MasterConfig {
    #[serde(default = "d_target_lufs")]
    pub target_lufs: f64,
    #[serde(default = "d_true_peak")]
    pub true_peak_db: f64,
}

impl Default for MasterConfig {
    fn default() -> Self {
        Self {
            target_lufs: d_target_lufs(),
            true_peak_db: d_true_peak(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioConfig {
    #[serde(default = "d_voice_gain")]
    pub voice_gain_db: f64,
    #[serde(default = "d_sfx_gain")]
    pub sfx_gain_db: f64,
    #[serde(default)]
    pub bgm: Option<BgmConfig>,
    #[serde(default)]
    pub master: MasterConfig,
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            voice_gain_db: d_voice_gain(),
            sfx_gain_db: d_sfx_gain(),
            bgm: None,
            master: MasterConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Cue {
    pub sfx: String,
    #[serde(default)]
    pub at: f64,
    #[serde(default)]
    pub gain_db: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Scene {
    pub image: String,
    #[serde(default)]
    pub voice: Option<String>,
    #[serde(default = "d_anim_in")]
    pub anim_in: String,
    #[serde(default = "d_anim_out")]
    pub anim_out: String,
    #[serde(default)]
    pub transition_sfx: Option<String>,
    #[serde(default)]
    pub cues: Vec<Cue>,
}

impl Default for Scene {
    fn default() -> Self {
        Self {
            image: String::new(),
            voice: None,
            anim_in: d_anim_in(),
            anim_out: d_anim_out(),
            transition_sfx: None,
            cues: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Project {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub output: OutputConfig,
    #[serde(default)]
    pub background: BackgroundConfig,
    #[serde(default)]
    pub layout: LayoutConfig,
    #[serde(default)]
    pub timing: TimingConfig,
    #[serde(default)]
    pub audio: AudioConfig,
    #[serde(rename = "scene", default)]
    pub scenes: Vec<Scene>,
}

impl Default for Project {
    fn default() -> Self {
        Self {
            schema_version: default_schema_version(),
            output: OutputConfig::default(),
            background: BackgroundConfig::default(),
            layout: LayoutConfig::default(),
            timing: TimingConfig::default(),
            audio: AudioConfig::default(),
            scenes: Vec::new(),
        }
    }
}

pub fn load_project(path: &Path) -> Result<Project, CoreError> {
    let text = std::fs::read_to_string(path)?;
    let project: Project = toml::from_str(&text).map_err(|e| CoreError::InvalidProject {
        field: "project".to_string(),
        reason: e.to_string(),
    })?;
    project.validate()?;
    Ok(project)
}

impl Project {
    pub fn validate(&self) -> Result<(), CoreError> {
        if self.schema_version != 1 {
            return Err(CoreError::InvalidProject {
                field: "schema_version".to_string(),
                reason: "expected 1".to_string(),
            });
        }
        match self.output.fps {
            24 | 25 | 30 | 50 | 60 => {}
            _ => {
                return Err(CoreError::InvalidProject {
                    field: "output.fps".to_string(),
                    reason: "supported 24/25/30/50/60".to_string(),
                })
            }
        }
        if self.scenes.is_empty() {
            return Err(CoreError::InvalidProject {
                field: "scene".to_string(),
                reason: "at least one scene required".to_string(),
            });
        }
        if self.background.kind == "video"
            && self
                .background
                .file
                .as_deref()
                .unwrap_or("")
                .trim()
                .is_empty()
        {
            return Err(CoreError::InvalidProject {
                field: "background.file".to_string(),
                reason: "required when background.kind is video".to_string(),
            });
        }
        for (i, s) in self.scenes.iter().enumerate() {
            if s.image.trim().is_empty() {
                return Err(CoreError::InvalidProject {
                    field: format!("scene[{}].image", i),
                    reason: "image path is empty".to_string(),
                });
            }
        }
        Ok(())
    }
}

/// Sorted pool of background clips. `file` is either one video or a directory of them.
pub fn background_clips(
    base: &Path,
    background: &BackgroundConfig,
) -> Result<Vec<std::path::PathBuf>, CoreError> {
    let rel = background.file.as_deref().unwrap_or("");
    if rel.trim().is_empty() {
        return Err(CoreError::InvalidProject {
            field: "background.file".to_string(),
            reason: "missing".to_string(),
        });
    }
    let path = base.join(rel);
    if path.is_dir() {
        let mut clips: Vec<std::path::PathBuf> = std::fs::read_dir(&path)?
            .filter_map(|entry| entry.ok().map(|e| e.path()))
            .filter(|p| p.is_file() && is_video_ext(p))
            .collect();
        clips.sort();
        if clips.is_empty() {
            return Err(CoreError::MissingFile {
                path: rel.to_string(),
                reason: "no video clips in directory".to_string(),
            });
        }
        Ok(clips)
    } else if path.is_file() {
        Ok(vec![path])
    } else {
        Err(CoreError::MissingFile {
            path: rel.to_string(),
            reason: "background video not found".to_string(),
        })
    }
}

fn is_video_ext(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .as_deref(),
        Some("mp4" | "mov" | "webm" | "mkv")
    )
}

/// One clip per scene: the pool shuffled deterministically by `seed`, wrapping when
/// there are more scenes than clips. Depending only on `(seed, index)` keeps a
/// `--range` render identical to the same stretch of a full render.
pub fn assign_background_clips(pool_len: usize, scene_count: usize, seed: u64) -> Vec<usize> {
    if pool_len == 0 {
        return Vec::new();
    }
    let mut order: Vec<usize> = (0..pool_len).collect();
    let mut state = seed;
    for i in (1..pool_len).rev() {
        let j = (splitmix64(&mut state) % (i as u64 + 1)) as usize;
        order.swap(i, j);
    }
    (0..scene_count).map(|s| order[s % pool_len]).collect()
}

fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clip_assignment_is_stable_and_covers_the_pool_once() {
        let a = assign_background_clips(4, 4, 7);
        assert_eq!(a, assign_background_clips(4, 4, 7));
        let mut sorted = a;
        sorted.sort();
        assert_eq!(sorted, vec![0, 1, 2, 3]);
    }

    #[test]
    fn clip_assignment_wraps_and_follows_the_seed() {
        assert_eq!(assign_background_clips(2, 5, 0).len(), 5);
        assert_eq!(assign_background_clips(0, 3, 0), Vec::<usize>::new());
        assert_ne!(
            assign_background_clips(4, 4, 1),
            assign_background_clips(4, 4, 2)
        );
    }
}
