use crate::{CoreError, Samples, SceneInputs};

#[derive(Debug, Clone)]
pub struct SceneSpan {
    pub index: usize,
    pub image: String,
    pub start: Samples,
    pub end: Samples,
    pub voice_start: Samples,
    pub voice_end: Samples,
}

/// How a scene image maps into the frame once scaled to the available width.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fit {
    /// Scaled image is shorter than the frame, so it is centered vertically.
    Centered,
    /// Scaled image is taller than the frame, so it scrolls by `max_offset_px`.
    Scroll { max_offset_px: u32 },
}

/// Scale a scene image to `frame_width - 2 * margin_x` and report how it fits.
/// The renderer calls this with the size it is actually rendering, so `--preset draft`
/// and `--size` stay correct; `check` calls it with the project's canonical output size.
pub fn fit_image(
    image_width: u32,
    image_height: u32,
    frame_width: u32,
    frame_height: u32,
    margin_x: u32,
) -> Fit {
    if image_width == 0 {
        return Fit::Centered;
    }
    let avail = frame_width.saturating_sub(margin_x * 2);
    let scaled_h = (image_height as u64 * avail as u64 / image_width as u64) as u32;
    if scaled_h > frame_height {
        Fit::Scroll {
            max_offset_px: scaled_h - frame_height,
        }
    } else {
        Fit::Centered
    }
}

#[derive(Debug, Clone)]
pub struct AudioCue {
    pub scene: usize,
    pub at: Samples,
    pub sfx: String,
    pub gain_db: f64,
}

#[derive(Debug, Clone)]
pub struct Timeline {
    pub total: Samples,
    pub scenes: Vec<SceneSpan>,
    pub cues: Vec<AudioCue>,
}

impl Timeline {
    /// Index of the scene visible at `sample`. A frame inside a transition belongs to
    /// the earlier span, so the renderer and the background decoder agree.
    pub fn scene_index(&self, sample: Samples) -> usize {
        for (i, span) in self.scenes.iter().enumerate() {
            if sample.0 >= span.start.0 && sample.0 < span.end.0 {
                return i;
            }
        }
        self.scenes.len().saturating_sub(1)
    }
}

pub fn solve(project: &crate::Project, inputs: &[SceneInputs]) -> Result<Timeline, CoreError> {
    if inputs.len() != project.scenes.len() {
        return Err(CoreError::InvalidProject {
            field: "scenes".to_string(),
            reason: format!(
                "expected {} inputs, got {}",
                project.scenes.len(),
                inputs.len()
            ),
        });
    }

    let pad_before = Samples::from_secs(project.timing.pad_before);
    let pad_after = Samples::from_secs(project.timing.pad_after);
    let transition = Samples::from_secs(project.timing.transition);

    let mut scenes = Vec::with_capacity(project.scenes.len());
    let mut cues = Vec::new();
    let mut prev_end = Samples(0);

    for (i, (scene, input)) in project.scenes.iter().zip(inputs.iter()).enumerate() {
        let hold = scene.hold_sec.unwrap_or(project.timing.default_hold);
        let voice_len = match input.voice_samples {
            Some(v) => Samples(v),
            None => Samples::from_secs(hold),
        };

        let scene_len = pad_before.0 + voice_len.0 + pad_after.0;
        let start = if i == 0 {
            Samples(0)
        } else {
            Samples(prev_end.0.saturating_sub(transition.0))
        };

        let voice_start = Samples(start.0 + pad_before.0);
        let voice_end = Samples(voice_start.0 + voice_len.0);
        let end = Samples(start.0 + scene_len);

        scenes.push(SceneSpan {
            index: i,
            image: scene.image.clone(),
            start,
            end,
            voice_start,
            voice_end,
        });

        if i > 0 {
            if let Some(sfx) = &scene.transition_sfx {
                cues.push(AudioCue {
                    scene: i,
                    at: start,
                    sfx: sfx.clone(),
                    gain_db: 0.0,
                });
            }
        }
        for c in &scene.cues {
            cues.push(AudioCue {
                scene: i,
                at: Samples(start.0 + Samples::from_secs(c.at).0),
                sfx: c.sfx.clone(),
                gain_db: c.gain_db,
            });
        }

        prev_end = end;
    }

    let total = scenes.last().map(|s| s.end).unwrap_or(Samples(0));
    Ok(Timeline {
        total,
        scenes,
        cues,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Project, Scene, SceneInputs};

    #[test]
    #[allow(clippy::field_reassign_with_default)]
    fn solver_respects_transition() {
        let mut project = Project::default();
        project.scenes = vec![
            Scene {
                image: "a.png".into(),
                voice: Some("a.mp3".into()),
                ..Default::default()
            },
            Scene {
                image: "b.png".into(),
                voice: Some("b.mp3".into()),
                ..Default::default()
            },
        ];
        project.timing.transition = 0.3;
        let inputs = vec![
            SceneInputs {
                voice_samples: Some(48000),
                image_width: 100,
                image_height: 100,
            },
            SceneInputs {
                voice_samples: Some(48000),
                image_width: 100,
                image_height: 100,
            },
        ];
        let tl = solve(&project, &inputs).unwrap();
        assert!(tl.scenes[1].start.0 < tl.scenes[0].end.0);
    }

    #[test]
    fn short_image_is_centered() {
        // 960x640 scaled to 960 wide is shorter than the 1920 frame.
        assert_eq!(fit_image(960, 640, 1080, 1920, 60), Fit::Centered);
    }

    #[test]
    fn tall_image_scrolls_by_the_overflow() {
        // 960x2400 scaled to 960 wide overflows the 1920 frame by 480 px.
        assert_eq!(
            fit_image(960, 2400, 1080, 1920, 60),
            Fit::Scroll { max_offset_px: 480 }
        );
    }

    #[test]
    fn fit_follows_the_frame_it_renders_into() {
        // Same image, smaller frame: no longer a scroll.
        assert_eq!(fit_image(960, 2400, 360, 640, 60), Fit::Centered);
    }
}
