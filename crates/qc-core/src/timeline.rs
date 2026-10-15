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
        let voice_len = match input.voice_samples {
            Some(v) => Samples(v),
            None => Samples::from_secs(project.timing.default_hold),
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
}
