use crate::commands::util;
use anyhow::Context;
use qc_core::CoreError;
use std::path::PathBuf;

#[derive(clap::Args, Debug)]
pub struct Args {
    pub dir: PathBuf,
    #[arg(long, default_value = "project.toml")]
    pub out: PathBuf,
    #[arg(long)]
    pub bgm: Option<PathBuf>,
    /// One background video, or a directory of clips to pick from per scene.
    #[arg(long)]
    pub bg_video: Option<PathBuf>,
    #[arg(long)]
    pub force: bool,
}

pub fn run(args: Args) -> anyhow::Result<()> {
    if !args.dir.is_dir() {
        return Err(CoreError::InvalidProject {
            field: "dir".to_string(),
            reason: "not a directory".to_string(),
        }
        .into());
    }

    let out = if args.out.is_absolute() {
        args.out.clone()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(&args.out)
    };
    let base = out
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .to_path_buf();

    if out.exists() && !args.force {
        return Err(CoreError::InvalidProject {
            field: "out".to_string(),
            reason: format!("{} already exists, use --force", out.display()),
        }
        .into());
    }

    let mut project = util::project_from_directory(&args.dir, &base)
        .with_context(|| format!("scanning {}", args.dir.display()))?;

    if let Some(bgm) = &args.bgm {
        let rel = util::posix(bgm.strip_prefix(&base).unwrap_or(bgm));
        project.audio.bgm = Some(qc_core::BgmConfig {
            file: rel,
            ..qc_core::BgmConfig::default()
        });
    }

    if let Some(bg) = &args.bg_video {
        project.background.kind = "video".to_string();
        project.background.file = Some(util::posix(bg.strip_prefix(&base).unwrap_or(bg)));
    }

    let text = toml::to_string_pretty(&project).map_err(|e| CoreError::InvalidProject {
        field: "project".to_string(),
        reason: e.to_string(),
    })?;
    // Ensure parent exists
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    std::fs::write(&out, text).with_context(|| format!("writing {}", out.display()))?;
    println!("wrote {}", out.display());
    Ok(())
}
