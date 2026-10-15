use qc_core::CoreError;
use std::path::{Path, PathBuf};

pub fn is_image_ext(ext: &str) -> bool {
    matches!(ext.to_lowercase().as_str(), "png" | "jpg" | "jpeg" | "webp")
}

pub fn is_audio_ext(ext: &str) -> bool {
    matches!(ext.to_lowercase().as_str(), "mp3" | "wav" | "m4a" | "ogg")
}

pub fn numeric_prefix(s: &str) -> u64 {
    s.chars()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>()
        .parse()
        .unwrap_or(u64::MAX)
}

pub fn posix(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

pub fn project_from_directory(input: &Path, base: &Path) -> Result<qc_core::Project, CoreError> {
    use std::collections::BTreeMap;

    let mut images: Vec<(String, PathBuf)> = Vec::new();
    let mut audios: BTreeMap<String, PathBuf> = BTreeMap::new();
    let mut bgm_candidate: Option<PathBuf> = None;

    for entry in std::fs::read_dir(input)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        let ext = path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        if stem.eq_ignore_ascii_case("bgm") {
            bgm_candidate = Some(path);
            continue;
        }
        if is_image_ext(&ext) {
            images.push((stem.clone(), path));
        } else if is_audio_ext(&ext) {
            audios.insert(stem, path);
        }
    }

    // Audio without matching image is an error.
    for (stem, path) in &audios {
        let has_image = images.iter().any(|(s, _)| s == stem);
        if !has_image {
            return Err(CoreError::MissingFile {
                path: posix(path),
                reason: "voice has no matching image (expected same numeric prefix)".to_string(),
            });
        }
    }

    images.sort_by_key(|(s, _)| (numeric_prefix(s), s.clone()));

    let mut scenes = Vec::new();
    let mut image_stems = Vec::new();
    for (stem, path) in &images {
        image_stems.push(stem.clone());
        let voice = audios
            .get(stem)
            .map(|p| posix(p.strip_prefix(base).unwrap_or(p)));
        let image_rel = posix(path.strip_prefix(base).unwrap_or(path));
        scenes.push(qc_core::Scene {
            image: image_rel,
            voice,
            ..qc_core::Scene::default()
        });
    }

    let audio = if let Some(path) = bgm_candidate {
        qc_core::AudioConfig {
            bgm: Some(qc_core::BgmConfig {
                file: posix(path.strip_prefix(base).unwrap_or(&path)),
                ..qc_core::BgmConfig::default()
            }),
            ..qc_core::AudioConfig::default()
        }
    } else {
        qc_core::AudioConfig::default()
    };

    let project = qc_core::Project {
        scenes,
        audio,
        ..Default::default()
    };
    project.validate()?;
    Ok(project)
}

pub fn load_or_scan(target: &Path) -> Result<(qc_core::Project, PathBuf), CoreError> {
    if target.is_dir() {
        let project_file = target.join("project.toml");
        if project_file.exists() {
            let project = qc_core::load_project(&project_file)?;
            let base = project_file.parent().unwrap_or(target).to_path_buf();
            return Ok((project, base));
        }
        let project = project_from_directory(target, target)?;
        Ok((project, target.to_path_buf()))
    } else {
        let project = qc_core::load_project(target)?;
        let base = target
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf();
        Ok((project, base))
    }
}

/// Probe every scene's media. Decoding dominates this step, so scenes run in parallel;
/// results keep scene order.
pub fn probe_scenes(
    project: &qc_core::Project,
    base: &Path,
) -> Result<Vec<qc_core::SceneProbe>, CoreError> {
    use rayon::prelude::*;
    project
        .scenes
        .par_iter()
        .map(|scene| qc_core::probe_scene(base, scene))
        .collect()
}
