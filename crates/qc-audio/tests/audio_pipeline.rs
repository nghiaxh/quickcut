//! Audio checks that need no committed media: every asset is generated here.

use std::path::{Path, PathBuf};

use qc_audio::mix_project;
use qc_core::{Project, Samples, SceneInputs, SAMPLE_RATE};

/// Scratch directory for one test, removed when the guard drops.
struct TempProject {
    dir: PathBuf,
}

impl TempProject {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("qc-audio-test-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp project");
        Self { dir }
    }

    fn path(&self) -> &Path {
        &self.dir
    }
}

impl Drop for TempProject {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Mono 16-bit WAV holding `freq_hz` for `secs`, so ducking has something to bite on.
fn write_tone(path: &Path, secs: f64, freq_hz: f64) {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, spec).expect("create wav");
    let frames = (secs * SAMPLE_RATE as f64) as u32;
    for i in 0..frames {
        let t = i as f64 / SAMPLE_RATE as f64;
        let value = (t * freq_hz * 2.0 * std::f64::consts::PI).sin() * 0.5;
        writer
            .write_sample((value * i16::MAX as f64) as i16)
            .expect("write sample");
    }
    writer.finalize().expect("finalize wav");
}

/// A project with `scenes` silent-voice scenes plus optional looping bgm and sfx.
fn build_project(
    dir: &Path,
    scenes: usize,
    voice_secs: f64,
    bgm: bool,
) -> (Project, Vec<SceneInputs>) {
    for i in 0..scenes {
        let name = format!("{:02}", i + 1);
        write_tone(&dir.join(format!("{name}.wav")), voice_secs, 440.0);
    }
    if bgm {
        write_tone(&dir.join("bgm.wav"), 0.5, 220.0);
    }

    let mut project = Project::default();
    let mut inputs = Vec::new();
    for i in 0..scenes {
        let name = format!("{:02}", i + 1);
        project.scenes.push(qc_core::Scene {
            image: format!("{name}.png"),
            voice: Some(format!("{name}.wav")),
            ..Default::default()
        });
        inputs.push(SceneInputs {
            voice_samples: Some(Samples::from_secs(voice_secs).0),
            image_width: 1080,
            image_height: 1920,
        });
    }
    if bgm {
        project.audio.bgm = Some(qc_core::BgmConfig {
            file: "bgm.wav".to_string(),
            ..Default::default()
        });
    }
    (project, inputs)
}

/// Read a 32-bit float WAV and return `(interleaved samples, channels)`.
fn read_wav(path: &Path) -> (Vec<f32>, u16) {
    let mut reader = hound::WavReader::open(path).expect("open wav");
    let spec = reader.spec();
    let samples: Vec<f32> = reader
        .samples::<f32>()
        .map(|s| s.expect("read sample"))
        .collect();
    (samples, spec.channels)
}

#[test]
fn mix_length_matches_the_solved_timeline() {
    let temp = TempProject::new("length");
    let (project, inputs) = build_project(temp.path(), 3, 0.5, false);
    let timeline = qc_core::solve_timeline(&project, &inputs).expect("solve");

    let out = temp.path().join("mix.wav");
    mix_project(&project, temp.path(), &timeline, &out, true).expect("mix");

    let (samples, channels) = read_wav(&out);
    assert_eq!(
        samples.len() / channels as usize,
        timeline.total.0 as usize,
        "mix.wav must be exactly Timeline.total samples"
    );
}

#[test]
fn silent_voice_does_not_silence_the_mix() {
    let temp = TempProject::new("tone");
    let (project, inputs) = build_project(temp.path(), 2, 0.4, true);
    let timeline = qc_core::solve_timeline(&project, &inputs).expect("solve");

    let out = temp.path().join("mix.wav");
    mix_project(&project, temp.path(), &timeline, &out, true).expect("mix");

    let (samples, _) = read_wav(&out);
    let peak = samples.iter().fold(0.0f32, |acc, s| acc.max(s.abs()));
    assert!(peak > 0.01, "mix is silent, peak was {peak}");
}

#[test]
fn true_peak_stays_under_the_configured_ceiling() {
    let temp = TempProject::new("peak");
    let (mut project, inputs) = build_project(temp.path(), 2, 0.4, true);
    // A hot ceiling with loudness normalisation on, so the limiter has to work.
    project.audio.master.true_peak_db = -6.0;
    project.audio.master.target_lufs = -8.0;
    let timeline = qc_core::solve_timeline(&project, &inputs).expect("solve");

    let out = temp.path().join("mix.wav");
    mix_project(&project, temp.path(), &timeline, &out, true).expect("mix");

    let (samples, channels) = read_wav(&out);
    let ceiling = 10f64.powf(project.audio.master.true_peak_db / 20.0);
    // Sample peak is an upper bound on nothing in particular, but it can never
    // exceed the limiter gain, so it must sit under the ceiling too.
    let peak = samples.iter().fold(0.0f32, |acc, s| acc.max(s.abs())) as f64;
    assert!(
        peak <= ceiling * 1.001,
        "sample peak {peak:.4} above ceiling {ceiling:.4}"
    );
    assert_eq!(channels, 2, "mix must be stereo");
}

#[test]
fn bgm_is_quieter_under_voice_than_in_a_silent_scene() {
    // pad_after is shorter than transition + pad_before, so two voiced scenes never
    // leave a gap. A scene without voice does, and the duck envelope must release.
    let temp = TempProject::new("duck");
    write_tone(&temp.path().join("01.wav"), 1.0, 440.0);
    write_tone(&temp.path().join("bgm.wav"), 0.5, 220.0);

    let mut project = Project::default();
    project.scenes.push(qc_core::Scene {
        image: "01.png".into(),
        voice: Some("01.wav".into()),
        ..Default::default()
    });
    project.scenes.push(qc_core::Scene {
        image: "02.png".into(),
        voice: None,
        ..Default::default()
    });
    project.audio.bgm = Some(qc_core::BgmConfig {
        file: "bgm.wav".to_string(),
        gain_db: 0.0,
        ..Default::default()
    });
    // Voice has to sit far below the bgm, otherwise its own energy hides the
    // ducking gain we are trying to measure.
    project.audio.voice_gain_db = -30.0;

    let inputs = vec![
        SceneInputs {
            voice_samples: Some(Samples::from_secs(1.0).0),
            image_width: 1080,
            image_height: 1920,
        },
        SceneInputs {
            voice_samples: None,
            image_width: 1080,
            image_height: 1920,
        },
    ];
    let timeline = qc_core::solve_timeline(&project, &inputs).expect("solve");
    assert!(
        timeline.total.0 as f64 / SAMPLE_RATE as f64 > 3.0,
        "the voiceless scene needs to give the envelope room to release"
    );

    let out = temp.path().join("mix.wav");
    // Normalisation off: it rescales the whole mix and would hide the duck depth.
    mix_project(&project, temp.path(), &timeline, &out, false).expect("mix");

    let (samples, channels) = read_wav(&out);
    let rms_db = |from: Samples, to: Samples| {
        let (from, to) = (from.0 as usize, to.0 as usize);
        let slice = &samples[from * channels as usize..to * channels as usize];
        let rms =
            (slice.iter().map(|s| (*s as f64).powi(2)).sum::<f64>() / slice.len() as f64).sqrt();
        20.0 * rms.max(1e-9).log10()
    };

    let voiced = &timeline.scenes[0];
    let under_voice = rms_db(
        Samples(voiced.voice_start.0 + SAMPLE_RATE as u64 / 4),
        Samples(voiced.voice_end.0 - SAMPLE_RATE as u64 / 4),
    );
    let silence = Samples(timeline.scenes[1].start.0 + SAMPLE_RATE as u64);
    let in_silence = rms_db(silence, Samples(silence.0 + SAMPLE_RATE as u64 / 2));

    assert!(
        in_silence > under_voice + 6.0,
        "bgm should recover in a voiceless scene: under voice {under_voice:.1} dB, in silence {in_silence:.1} dB"
    );
}
