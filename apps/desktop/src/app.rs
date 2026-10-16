use qc_core::{load_or_scan, probe_scene, solve_timeline, Project, SceneInputs, Timeline};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

#[derive(Clone)]
pub struct ProjectSession {
    pub path: PathBuf,
    pub base: PathBuf,
    pub project: Project,
    pub timeline: Timeline,
    pub inputs: Vec<SceneInputs>,
    pub dirty: bool,
}

pub struct Studio {
    pub session: Option<ProjectSession>,
    pub selected: Option<usize>,
    pub preview_frame: u64,
    pub preview_bytes: Option<Vec<u8>>,
    pub preview_texture: Option<egui::TextureHandle>,
    pub preview_size: (u32, u32),
    pub render_final: bool,
    pub preview_dirty: bool,
    pub preview_pending: bool,
    pub preview_rx: Option<mpsc::Receiver<PreviewResult>>,
    pub last_edit: Instant,
    pub export_rx: Option<mpsc::Receiver<ExportMsg>>,
    pub export_cancel: Option<Arc<AtomicBool>>,
    pub export_status: String,
    pub status: String,
    pub export_open: bool,
    pub ffmpeg_path: String,
    pub out_path: String,
    pub export_preset: String,
    pub export_threads: String,
    pub ffmpeg_missing: bool,
    pub undo_stack: Vec<Project>,
    pub redo_stack: Vec<Project>,
    pub recent: Vec<String>,
}

pub struct PreviewResult {
    pub w: u32,
    pub h: u32,
    pub bytes: Vec<u8>,
    pub err: Option<String>,
}

pub enum ExportMsg {
    Progress(qc_core::Progress),
    Done(Result<(), String>),
}

impl Studio {
    pub fn new(_cc: &eframe::CreationContext) -> Self {
        let mut s = Self {
            session: None,
            selected: None,
            preview_frame: 0,
            preview_bytes: None,
            preview_texture: None,
            preview_size: (360, 640),
            render_final: false,
            preview_dirty: false,
            preview_pending: false,
            preview_rx: None,
            last_edit: Instant::now(),
            export_rx: None,
            export_cancel: None,
            export_status: String::new(),
            status: "Ready".to_string(),
            export_open: false,
            ffmpeg_path: "ffmpeg".to_string(),
            out_path: "out.mp4".to_string(),
            export_preset: "final".to_string(),
            export_threads: String::new(),
            ffmpeg_missing: false,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            recent: Vec::new(),
        };
        let cfg = load_config();
        s.recent = cfg.recent.clone();
        s.ffmpeg_path = if cfg.ffmpeg_path.is_empty() {
            discover_ffmpeg()
        } else {
            cfg.ffmpeg_path
        };
        s.check_ffmpeg();
        s
    }

    pub fn load_path(&mut self, path: PathBuf, ctx: &egui::Context) {
        let project_path = if path.is_dir() {
            path.join("project.toml")
        } else {
            path.clone()
        };
        match load_or_scan(&path) {
            Ok((project, base)) => self.load_session(project, base, project_path, ctx),
            Err(e) => self.status = e.to_string(),
        }
    }

    fn load_session(
        &mut self,
        project: Project,
        base: PathBuf,
        path: PathBuf,
        _ctx: &egui::Context,
    ) {
        let inputs = match (0..project.scenes.len())
            .map(|i| probe_scene(&base, &project.scenes[i]).map(|p| p.inputs))
            .collect::<Result<Vec<_>, _>>()
        {
            Ok(v) => v,
            Err(e) => {
                self.status = e.to_string();
                return;
            }
        };
        let timeline = match solve_timeline(&project, &inputs) {
            Ok(t) => t,
            Err(e) => {
                self.status = e.to_string();
                return;
            }
        };
        self.session = Some(ProjectSession {
            path,
            base,
            project,
            timeline,
            inputs,
            dirty: false,
        });
        self.selected = Some(0);
        self.preview_frame = 0;
        self.preview_bytes = None;
        self.preview_texture = None;
        self.preview_dirty = true;
        self.last_edit = Instant::now();
        self.status = format!(
            "Loaded {} scenes",
            self.session.as_ref().unwrap().project.scenes.len()
        );
        let loaded = self.session.as_ref().unwrap().path.display().to_string();
        self.recent.retain(|p| p != &loaded);
        self.recent.insert(0, loaded);
        self.recent.truncate(8);
        self.save_config();
    }

    /// Scan a media folder into a new project and write `project.toml` there.
    pub fn new_project(&mut self, dir: PathBuf, ctx: &egui::Context) {
        let manifest = dir.join("project.toml");
        if manifest.exists() {
            self.load_path(manifest, ctx);
            return;
        }
        match qc_core::project_from_directory(&dir, &dir) {
            Ok(project) => {
                self.load_session(project, dir.clone(), manifest, ctx);
                self.save_session();
                self.status = format!("Created project in {}", dir.display());
            }
            Err(e) => self.status = e.to_string(),
        }
    }

    pub fn save_session(&mut self) {
        let Some(sess) = &mut self.session else {
            return;
        };
        let text = match toml::to_string_pretty(&sess.project) {
            Ok(t) => t,
            Err(e) => {
                self.status = format!("save failed: {e}");
                return;
            }
        };
        if let Err(e) = std::fs::write(&sess.path, text) {
            self.status = format!("save failed: {e}");
            return;
        }
        sess.dirty = false;
        self.status = format!("Saved {}", sess.path.display());
    }

    /// Write the current project to a new `project.toml` and rebase the session.
    pub fn save_as(&mut self, path: PathBuf) {
        let Some(sess) = &mut self.session else {
            return;
        };
        if let Some(parent) = path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                self.status = format!("save failed: {e}");
                return;
            }
        }
        sess.path = path;
        if let Some(parent) = sess.path.parent() {
            sess.base = parent.to_path_buf();
        }
        self.save_session();
        self.reload_timeline();
        qc_render::reset_caches();
        self.preview_dirty = true;
        self.last_edit = Instant::now();
    }

    fn reload_timeline(&mut self) {
        let Some(sess) = &mut self.session else {
            return;
        };
        let inputs = match (0..sess.project.scenes.len())
            .map(|i| probe_scene(&sess.base, &sess.project.scenes[i]).map(|p| p.inputs))
            .collect::<Result<Vec<_>, _>>()
        {
            Ok(v) => v,
            Err(e) => {
                self.status = e.to_string();
                return;
            }
        };
        match solve_timeline(&sess.project, &inputs) {
            Ok(tl) => {
                sess.inputs = inputs;
                sess.timeline = tl;
            }
            Err(e) => self.status = e.to_string(),
        }
    }

    pub fn mutate_project<F: FnOnce(&mut Project)>(&mut self, f: F) {
        if let Some(sess) = &self.session {
            let mut stack = self.undo_stack.clone();
            stack.push(sess.project.clone());
            if stack.len() > 50 {
                stack.remove(0);
            }
            self.undo_stack = stack;
            self.redo_stack.clear();
        }
        let Some(sess) = &mut self.session else {
            return;
        };
        f(&mut sess.project);
        sess.dirty = true;
        self.reload_timeline();
        qc_render::reset_caches();
        self.preview_dirty = true;
        self.last_edit = Instant::now();
    }

    pub fn undo_edit(&mut self) {
        if let Some(prev) = self.undo_stack.pop() {
            if let Some(sess) = &mut self.session {
                self.redo_stack.push(sess.project.clone());
                sess.project = prev;
                sess.dirty = true;
            }
            self.reload_timeline();
            qc_render::reset_caches();
            self.preview_dirty = true;
            self.last_edit = Instant::now();
        }
    }

    pub fn redo_edit(&mut self) {
        if let Some(next) = self.redo_stack.pop() {
            if let Some(sess) = &mut self.session {
                self.undo_stack.push(sess.project.clone());
                sess.project = next;
                sess.dirty = true;
            }
            self.reload_timeline();
            qc_render::reset_caches();
            self.preview_dirty = true;
            self.last_edit = Instant::now();
        }
    }

    pub fn add_scene_image(&mut self, image_path: PathBuf) {
        let Some(sess) = &mut self.session else {
            return;
        };
        let base = sess.base.clone();
        let rel = image_path
            .strip_prefix(&base)
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .unwrap_or_else(|_| image_path.to_string_lossy().replace('\\', "/"));
        if image_path.exists() && !base.join(&rel).exists() {
            // copy into project folder for portability
            let dest_dir = base.join("media");
            let _ = std::fs::create_dir_all(&dest_dir);
            let file_name = image_path.file_name().unwrap_or_default();
            let mut dest = dest_dir.join(file_name);
            let mut i = 1;
            while dest.exists() {
                let stem = image_path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("image");
                let ext = image_path
                    .extension()
                    .and_then(|s| s.to_str())
                    .unwrap_or("png");
                dest = dest_dir.join(format!("{} ({}).{}", stem, i, ext));
                i += 1;
            }
            if std::fs::copy(&image_path, &dest).is_ok() {
                let rel2 = dest
                    .strip_prefix(&base)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                self.mutate_project(|p| {
                    p.scenes.push(qc_core::Scene {
                        image: rel2,
                        ..qc_core::Scene::default()
                    });
                });
                return;
            }
        }
        self.mutate_project(|p| {
            p.scenes.push(qc_core::Scene {
                image: rel,
                ..qc_core::Scene::default()
            });
        });
    }

    fn check_ffmpeg(&mut self) {
        self.ffmpeg_missing = std::process::Command::new(&self.ffmpeg_path)
            .arg("-version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| !s.success())
            .unwrap_or(true);
    }

    pub fn validation_issues(&self) -> Vec<String> {
        let mut issues = Vec::new();
        let Some(sess) = &self.session else {
            return issues;
        };
        if let Err(e) = sess.project.validate() {
            issues.push(e.to_string());
        }
        for (i, scene) in sess.project.scenes.iter().enumerate() {
            if scene.image.trim().is_empty() {
                issues.push(format!("scene {}: image path is empty", i + 1));
            }
            if let Some(input) = sess.inputs.get(i) {
                if input.image_width < qc_core::MIN_IMAGE_WIDTH {
                    issues.push(format!(
                        "scene {}: image {}px wide (min {})",
                        i + 1,
                        input.image_width,
                        qc_core::MIN_IMAGE_WIDTH
                    ));
                }
            }
        }
        if sess.project.background.kind == "video"
            && sess
                .project
                .background
                .file
                .as_deref()
                .unwrap_or("")
                .trim()
                .is_empty()
        {
            issues.push("background.file is required for kind = video".to_string());
        }
        issues
    }

    fn save_config(&self) {
        let Some(path) = config_path() else {
            return;
        };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let cfg = AppConfig {
            ffmpeg_path: self.ffmpeg_path.clone(),
            recent: self.recent.clone(),
        };
        if let Ok(text) = toml::to_string_pretty(&cfg) {
            let _ = std::fs::write(path, text);
        }
    }

    fn maybe_render_preview(&mut self, ctx: &egui::Context) {
        if self.preview_pending {
            if let Some(rx) = &self.preview_rx {
                if let Ok(res) = rx.try_recv() {
                    self.preview_pending = false;
                    self.preview_rx = None;
                    if let Some(err) = res.err {
                        self.status = err;
                    } else {
                        let image = egui::ColorImage::from_rgba_unmultiplied(
                            [res.w as usize, res.h as usize],
                            &res.bytes,
                        );
                        self.preview_texture =
                            Some(ctx.load_texture("preview", image, egui::TextureOptions::LINEAR));
                        self.preview_bytes = Some(res.bytes);
                    }
                }
            }
            return;
        }
        if !self.preview_dirty {
            return;
        }
        if self.last_edit.elapsed() < Duration::from_millis(150) {
            ctx.request_repaint_after(Duration::from_millis(150));
            return;
        }
        let Some(sess) = &self.session else {
            return;
        };
        let (w, h) = if self.render_final {
            (sess.project.output.width, sess.project.output.height)
        } else {
            self.preview_size
        };
        let fps = sess.project.output.fps;
        let project = sess.project.clone();
        let base = sess.base.clone();
        let timeline = sess.timeline.clone();
        let frame = self.preview_frame;
        let (tx, rx) = mpsc::channel();
        self.preview_rx = Some(rx);
        self.preview_pending = true;
        self.preview_dirty = false;
        std::thread::spawn(move || {
            let r =
                qc_render::render_frame_rgba(&project, &base, &timeline, frame, fps, w, h, None);
            match r {
                Ok(bytes) => {
                    let _ = tx.send(PreviewResult {
                        w,
                        h,
                        bytes,
                        err: None,
                    });
                }
                Err(e) => {
                    let _ = tx.send(PreviewResult {
                        w,
                        h,
                        bytes: Vec::new(),
                        err: Some(e.to_string()),
                    });
                }
            }
        });
    }

    fn poll_export(&mut self, ctx: &egui::Context) {
        if let Some(rx) = &self.export_rx {
            if let Ok(msg) = rx.try_recv() {
                match msg {
                    ExportMsg::Progress(p) => {
                        self.export_status =
                            format!("{} {}/{}", p.stage.as_str(), p.frame, p.total);
                        if p.stage == qc_core::Stage::Done {
                            self.export_open = false;
                        }
                        ctx.request_repaint();
                    }
                    ExportMsg::Done(res) => {
                        self.export_rx = None;
                        self.export_cancel = None;
                        match res {
                            Ok(()) => {
                                self.status = format!("Exported {}", self.out_path);
                                self.export_status = "Done".to_string();
                            }
                            Err(e) => {
                                self.status = e;
                                self.export_status = "Failed".to_string();
                            }
                        }
                    }
                }
            }
        }
    }

    fn start_export(&mut self, ctx: &egui::Context) {
        self.save_config();
        let Some(sess) = &self.session else { return };
        let project = sess.project.clone();
        let base = sess.base.clone();
        let timeline = sess.timeline.clone();
        let out = PathBuf::from(&self.out_path);
        if !out.is_absolute() {
            // resolve relative to project base for convenience
        }
        let out = if out.is_absolute() {
            out
        } else {
            sess.base.join(&self.out_path)
        };
        let fps = project.output.fps;
        let (w, h) = match self.export_preset.as_str() {
            "draft" => (360u32, 640u32),
            _ => (project.output.width, project.output.height),
        };
        let preset = self.export_preset.clone();
        let ffmpeg = if self.ffmpeg_path == "ffmpeg" {
            None
        } else {
            Some(PathBuf::from(&self.ffmpeg_path))
        };
        let threads: Option<usize> = self.export_threads.trim().parse().ok();
        let cancel = Arc::new(AtomicBool::new(false));
        self.export_cancel = Some(cancel.clone());
        let (tx, rx) = mpsc::channel();
        self.export_rx = Some(rx);
        self.export_status = "Starting...".to_string();
        let txp = tx.clone();
        std::thread::spawn(move || {
            let _ = threads;
            let mut cb = move |p| {
                let _ = txp.send(ExportMsg::Progress(p));
            };
            let res = qc_export::export_video(
                &project,
                &base,
                &timeline,
                &out,
                fps,
                w,
                h,
                &preset,
                ffmpeg.as_deref(),
                None,
                false,
                true,
                Some(cancel),
                &mut cb,
            );
            let res = res.map_err(|e| e.to_string());
            let _ = tx.send(ExportMsg::Done(res));
        });
        let _ = ctx;
    }
}

impl eframe::App for Studio {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_export(ctx);
        self.maybe_render_preview(ctx);

        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.button("New Project").clicked() {
                    if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                        self.new_project(dir, ctx);
                    }
                }
                if ui.button("Open Project").clicked() {
                    if let Some(p) = rfd::FileDialog::new()
                        .add_filter("Project", &["toml"])
                        .pick_file()
                    {
                        self.load_path(p, ctx);
                    }
                }
                if ui.button("Save").clicked() {
                    self.save_session();
                }
                if ui.button("Save As").clicked() {
                    if let Some(p) = rfd::FileDialog::new()
                        .set_file_name("project.toml")
                        .save_file()
                    {
                        self.save_as(p);
                    }
                }
                if ui.button("Undo").clicked() {
                    self.undo_edit();
                }
                if ui.button("Redo").clicked() {
                    self.redo_edit();
                }
                if ui.button("Add Image").clicked() {
                    if let Some(p) = rfd::FileDialog::new()
                        .add_filter("Image", &["png", "jpg", "jpeg", "webp"])
                        .pick_file()
                    {
                        self.add_scene_image(p);
                    }
                }
                if ui.button("Export").clicked() {
                    self.export_open = true;
                }
                if !self.recent.is_empty() {
                    let recent = self.recent.clone();
                    let mut picked: Option<PathBuf> = None;
                    egui::ComboBox::from_id_salt("recent")
                        .selected_text("Recent")
                        .show_ui(ui, |ui| {
                            for r in &recent {
                                if ui.selectable_label(false, r).clicked() {
                                    picked = Some(PathBuf::from(r));
                                }
                            }
                        });
                    if let Some(p) = picked {
                        self.load_path(p, ctx);
                    }
                }
                ui.separator();
                if let Some(sess) = &self.session {
                    if sess.dirty {
                        ui.label("modified");
                    } else {
                        ui.label("saved");
                    }
                    ui.label(format!("scenes: {}", sess.project.scenes.len()));
                }
                ui.label(&self.status);
            });
        });

        egui::SidePanel::left("scenes").show(ctx, |ui| {
            ui.heading("Scenes");
            let mut to_move: Option<(usize, isize)> = None;
            let mut to_delete: Option<usize> = None;
            let mut to_dup: Option<usize> = None;
            if let Some(sess) = &self.session {
                let n = sess.project.scenes.len();
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for i in 0..n {
                        let scene = &sess.project.scenes[i];
                        let selected = self.selected == Some(i);
                        let resp =
                            ui.selectable_label(selected, format!("{} {}", i + 1, scene.image));
                        if resp.clicked() {
                            self.selected = Some(i);
                            if let Some(tl) = sess.timeline.scenes.get(i) {
                                self.preview_frame = tl.start.to_frame(sess.project.output.fps);
                                self.preview_dirty = true;
                                self.last_edit = Instant::now();
                            }
                        }
                        ui.horizontal(|ui| {
                            if ui.small_button("up").clicked() && i > 0 {
                                to_move = Some((i, -1));
                            }
                            if ui.small_button("down").clicked() && i + 1 < n {
                                to_move = Some((i, 1));
                            }
                            if ui.small_button("dup").clicked() {
                                to_dup = Some(i);
                            }
                            if ui.small_button("del").clicked() && n > 1 {
                                to_delete = Some(i);
                            }
                        });
                    }
                });
            } else {
                ui.label("No project loaded");
            }
            // apply queued mutations outside the borrow
            if let Some((i, d)) = to_move {
                self.mutate_project(|p| {
                    let j = (i as isize + d) as usize;
                    p.scenes.swap(i, j);
                });
            }
            if let Some(i) = to_dup {
                self.mutate_project(|p| {
                    if let Some(sc) = p.scenes.get(i).cloned() {
                        p.scenes.insert(i + 1, sc);
                    }
                });
            }
            if let Some(i) = to_delete {
                self.mutate_project(|p| {
                    p.scenes.remove(i);
                });
                if self.selected == Some(i) {
                    self.selected = Some(0);
                }
            }
        });

        egui::SidePanel::right("inspector").show(ctx, |ui| {
            ui.heading("Inspector");
            let mut changed = false;
            if let Some(sess) = &mut self.session {
                if let Some(sel) = self.selected {
                    if let Some(scene) = sess.project.scenes.get_mut(sel) {
                        ui.label("Scene");
                        let mut img = scene.image.clone();
                        if ui.text_edit_singleline(&mut img).changed() {
                            scene.image = img;
                            changed = true;
                        }
                        let mut voice = scene.voice.clone().unwrap_or_default();
                        if ui.text_edit_singleline(&mut voice).changed() {
                            scene.voice = if voice.trim().is_empty() {
                                None
                            } else {
                                Some(voice)
                            };
                            changed = true;
                        }
                        let mut hold = scene.hold_sec.unwrap_or(0.0);
                        if ui
                            .add(egui::Slider::new(&mut hold, 0.0..=10.0))
                            .on_hover_text("hold sec (0 uses default)")
                            .changed()
                        {
                            scene.hold_sec = if hold > 0.0 { Some(hold) } else { None };
                            changed = true;
                        }
                        let mut caption = scene.caption.clone().unwrap_or_default();
                        if ui.text_edit_singleline(&mut caption).changed() {
                            scene.caption = if caption.trim().is_empty() {
                                None
                            } else {
                                Some(caption)
                            };
                            changed = true;
                        }
                        egui::ComboBox::from_label("anim in")
                            .selected_text(&scene.anim_in)
                            .show_ui(ui, |ui| {
                                for v in ["pop", "slide_left", "none"] {
                                    if ui
                                        .selectable_value(&mut scene.anim_in, v.to_string(), v)
                                        .changed()
                                    {
                                        changed = true;
                                    }
                                }
                            });
                        egui::ComboBox::from_label("anim out")
                            .selected_text(&scene.anim_out)
                            .show_ui(ui, |ui| {
                                for v in ["slide_left", "none"] {
                                    if ui
                                        .selectable_value(&mut scene.anim_out, v.to_string(), v)
                                        .changed()
                                    {
                                        changed = true;
                                    }
                                }
                            });
                    }
                    if let Some(input) = sess.inputs.get(sel) {
                        ui.separator();
                        ui.label(format!(
                            "image: {}x{}",
                            input.image_width, input.image_height
                        ));
                        match input.voice_samples {
                            Some(v) => ui.label(format!("voice: {:.2}s", v as f64 / 48000.0)),
                            None => ui.label("voice: none"),
                        };
                    }
                }
                ui.separator();
                ui.collapsing("Project settings", |ui| {
                    // output
                    let mut fps = sess.project.output.fps as i32;
                    if ui.add(egui::Slider::new(&mut fps, 24..=60)).changed() {
                        // snap to allowed handled by validation; store raw
                        sess.project.output.fps = fps as u32;
                        changed = true;
                    }
                    let mut w = sess.project.output.width as i32;
                    let mut h = sess.project.output.height as i32;
                    if ui.add(egui::DragValue::new(&mut w)).changed() {
                        sess.project.output.width = w as u32;
                        changed = true;
                    }
                    if ui.add(egui::DragValue::new(&mut h)).changed() {
                        sess.project.output.height = h as u32;
                        changed = true;
                    }
                    // bg kind
                    let mut kind = sess.project.background.kind.clone();
                    egui::ComboBox::from_label("bg kind")
                        .selected_text(&kind)
                        .show_ui(ui, |ui| {
                            for v in ["solid", "blur_from_image", "video"] {
                                if ui.selectable_value(&mut kind, v.to_string(), v).changed() {
                                    changed = true;
                                }
                            }
                        });
                    if kind != sess.project.background.kind {
                        sess.project.background.kind = kind;
                        changed = true;
                    }
                    let mut blur = sess.project.background.blur_sigma;
                    if ui.add(egui::Slider::new(&mut blur, 0.0..=12.0)).changed() {
                        sess.project.background.blur_sigma = blur;
                        changed = true;
                    }
                    let mut bg_file = sess.project.background.file.clone().unwrap_or_default();
                    if ui.text_edit_singleline(&mut bg_file).changed() {
                        sess.project.background.file = if bg_file.trim().is_empty() {
                            None
                        } else {
                            Some(bg_file)
                        };
                        changed = true;
                    }
                    if ui.button("Pick background clip").clicked() {
                        if let Some(p) = rfd::FileDialog::new()
                            .add_filter("Video", &["mp4", "mov", "webm", "mkv"])
                            .pick_file()
                        {
                            let rel = p
                                .strip_prefix(&sess.base)
                                .map(|r| r.to_string_lossy().replace('\\', "/"))
                                .unwrap_or_else(|_| p.to_string_lossy().replace('\\', "/"));
                            sess.project.background.file = Some(rel);
                            sess.project.background.kind = "video".to_string();
                            changed = true;
                        }
                    }
                    ui.separator();
                    let bgm_label = sess
                        .project
                        .audio
                        .bgm
                        .as_ref()
                        .map(|b| b.file.clone())
                        .unwrap_or_else(|| "none".to_string());
                    ui.label(format!("bgm: {bgm_label}"));
                    if ui.button("Pick BGM").clicked() {
                        if let Some(p) = rfd::FileDialog::new()
                            .add_filter("Audio", &["mp3", "wav", "m4a", "ogg"])
                            .pick_file()
                        {
                            let rel = p
                                .strip_prefix(&sess.base)
                                .map(|r| r.to_string_lossy().replace('\\', "/"))
                                .unwrap_or_else(|_| p.to_string_lossy().replace('\\', "/"));
                            sess.project.audio.bgm = Some(qc_core::BgmConfig {
                                file: rel,
                                ..qc_core::BgmConfig::default()
                            });
                            changed = true;
                        }
                    }
                    // timing
                    let mut hold = sess.project.timing.default_hold;
                    if ui.add(egui::Slider::new(&mut hold, 0.5..=10.0)).changed() {
                        sess.project.timing.default_hold = hold;
                        changed = true;
                    }
                    let mut transition = sess.project.timing.transition;
                    if ui
                        .add(egui::Slider::new(&mut transition, 0.0..=1.0))
                        .changed()
                    {
                        sess.project.timing.transition = transition;
                        changed = true;
                    }
                    // audio gains
                    let mut voice_gain = sess.project.audio.voice_gain_db;
                    if ui
                        .add(egui::Slider::new(&mut voice_gain, -20.0..=20.0))
                        .changed()
                    {
                        sess.project.audio.voice_gain_db = voice_gain;
                        changed = true;
                    }
                    let mut sfx_gain = sess.project.audio.sfx_gain_db;
                    if ui
                        .add(egui::Slider::new(&mut sfx_gain, -30.0..=20.0))
                        .changed()
                    {
                        sess.project.audio.sfx_gain_db = sfx_gain;
                        changed = true;
                    }
                });
                if changed {
                    sess.dirty = true;
                    // reload + caches handled below
                }
            }
            // apply changed after mutable borrow ends
            if self.session.is_some() && changed {
                if let Some(sess) = &mut self.session {
                    sess.dirty = true;
                }
                self.reload_timeline();
                qc_render::reset_caches();
                self.preview_dirty = true;
                self.last_edit = Instant::now();
            }
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Preview");
            if let Some(tex) = &self.preview_texture {
                let img = egui::Image::from_texture(tex).fit_to_original_size(1.0);
                ui.add(img);
            } else {
                ui.label("No preview yet");
            }
            ui.horizontal(|ui| {
                let max = if let Some(sess) = &self.session {
                    sess.timeline
                        .total
                        .to_frame(sess.project.output.fps)
                        .saturating_sub(1)
                } else {
                    0
                };
                if ui.button("Play/Pause").clicked() {
                    // simple: step one frame
                }
                ui.add(egui::Slider::new(&mut self.preview_frame, 0..=max).text("frame"));
                if ui.button("Render preview").clicked() {
                    self.preview_dirty = true;
                    self.last_edit = Instant::now() - Duration::from_secs(10);
                }
                let mut final_toggle = self.render_final;
                if ui.checkbox(&mut final_toggle, "final res").changed() {
                    self.render_final = final_toggle;
                    self.preview_dirty = true;
                    self.last_edit = Instant::now() - Duration::from_secs(10);
                }
            });
            if let Some(sess) = &self.session {
                let sec = self.preview_frame as f64 / sess.project.output.fps as f64;
                ui.label(format!(
                    "t={:.2}s / {:.2}s",
                    sec,
                    sess.timeline.total.0 as f64 / 48000.0
                ));
            }
        });

        if self.export_open {
            egui::Window::new("Export").show(ctx, |ui| {
                ui.label("Output path");
                ui.text_edit_singleline(&mut self.out_path);
                ui.label("Preset");
                egui::ComboBox::from_label("preset")
                    .selected_text(&self.export_preset)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.export_preset, "draft".to_string(), "draft");
                        ui.selectable_value(&mut self.export_preset, "final".to_string(), "final");
                    });
                ui.label("FFmpeg path");
                ui.text_edit_singleline(&mut self.ffmpeg_path);
                ui.label("Threads (optional)");
                ui.text_edit_singleline(&mut self.export_threads);
                ui.label(&self.export_status);
                if self.export_rx.is_some() {
                    if ui.button("Cancel").clicked() {
                        if let Some(c) = &self.export_cancel {
                            c.store(true, Ordering::Relaxed);
                        }
                    }
                } else if ui.button("Start").clicked() {
                    self.check_ffmpeg();
                    if self.ffmpeg_missing {
                        self.export_status = "FFmpeg not found".to_string();
                    } else {
                        self.start_export(ctx);
                    }
                }
                if ui.button("Close").clicked() {
                    self.export_open = false;
                }
            });
        }

        egui::TopBottomPanel::bottom("bottom").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(&self.export_status);
                if !self.ffmpeg_missing {
                    ui.label("ffmpeg ok");
                } else {
                    ui.label("ffmpeg missing");
                }
            });
            let issues = self.validation_issues();
            if !issues.is_empty() {
                ui.colored_label(
                    egui::Color32::from_rgb(220, 180, 60),
                    format!("{} issue(s): {}", issues.len(), issues.join("; ")),
                );
            }
        });

        // dropped files
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .filter_map(|f| f.path.clone())
                .collect()
        });
        for p in dropped {
            if p.is_file() {
                self.add_scene_image(p);
            } else if p.is_dir() {
                self.load_path(p, ctx);
            }
        }
    }
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct AppConfig {
    pub ffmpeg_path: String,
    #[serde(default)]
    pub recent: Vec<String>,
}

fn config_path() -> Option<PathBuf> {
    if cfg!(target_os = "windows") {
        std::env::var("APPDATA")
            .ok()
            .map(|d| PathBuf::from(d).join("QuickCut").join("config.toml"))
    } else if cfg!(target_os = "macos") {
        std::env::var("HOME")
            .ok()
            .map(|h| PathBuf::from(h).join("Library/Application Support/QuickCut/config.toml"))
    } else {
        std::env::var("XDG_CONFIG_HOME")
            .ok()
            .or_else(|| std::env::var("HOME").ok().map(|h| format!("{h}/.config")))
            .map(|d| PathBuf::from(d).join("quickcut").join("config.toml"))
    }
}

fn load_config() -> AppConfig {
    let Some(path) = config_path() else {
        return AppConfig::default();
    };
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| toml::from_str(&text).ok())
        .unwrap_or_default()
}

fn discover_ffmpeg() -> String {
    let candidates = [
        "ffmpeg",
        "/usr/local/bin/ffmpeg",
        "/opt/homebrew/bin/ffmpeg",
        "/usr/bin/ffmpeg",
    ];
    for cand in candidates {
        if std::process::Command::new(cand)
            .arg("-version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
        {
            return cand.to_string();
        }
    }
    #[cfg(target_os = "windows")]
    {
        for cand in [
            r"C:\\Program Files\\FFmpeg\\bin\\ffmpeg.exe",
            r"C:\\ffmpeg\\bin\\ffmpeg.exe",
        ] {
            if std::path::Path::new(cand).exists() {
                return cand.to_string();
            }
        }
    }
    "ffmpeg".to_string()
}
