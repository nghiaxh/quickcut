use eframe::egui;
use std::path::PathBuf;

struct DesktopApp {
    project_path: String,
    status: String,
    project: Option<qc_core::Project>,
    base: Option<PathBuf>,
    timeline: Option<qc_core::Timeline>,
    preview_frame: u64,
    preview_texture: Option<egui::TextureHandle>,
    preview_size: (u32, u32),
}

impl DesktopApp {
    fn new(_cc: &eframe::CreationContext) -> Self {
        Self {
            project_path: String::new(),
            status: "Ready".to_string(),
            project: None,
            base: None,
            timeline: None,
            preview_frame: 0,
            preview_texture: None,
            preview_size: (1080, 1920),
        }
    }

    fn load_project(&mut self) {
        let path = PathBuf::from(&self.project_path);
        match qc_core::load_project(&path) {
            Ok(project) => {
                let base = path
                    .parent()
                    .unwrap_or_else(|| std::path::Path::new("."))
                    .to_path_buf();
                let inputs: Vec<qc_core::SceneInputs> = match project
                    .scenes
                    .iter()
                    .map(|scene| qc_core::probe_scene(&base, scene).map(|probe| probe.inputs))
                    .collect()
                {
                    Ok(inputs) => inputs,
                    Err(e) => {
                        self.status = e.to_string();
                        return;
                    }
                };
                match qc_core::solve_timeline(&project, &inputs) {
                    Ok(timeline) => {
                        self.status = format!("Loaded {} scenes", project.scenes.len());
                        self.project = Some(project);
                        self.base = Some(base);
                        self.timeline = Some(timeline);
                    }
                    Err(e) => self.status = e.to_string(),
                }
            }
            Err(e) => self.status = e.to_string(),
        }
    }

    fn render_preview(&mut self, ctx: &egui::Context) {
        let (Some(project), Some(base), Some(timeline)) =
            (&self.project, &self.base, &self.timeline)
        else {
            return;
        };
        let (w, h) = self.preview_size;
        match qc_render::render_frame_rgba(
            project,
            base,
            timeline,
            self.preview_frame,
            30,
            w,
            h,
            None,
        ) {
            Ok(rgba) => {
                let image =
                    egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba);
                self.preview_texture =
                    Some(ctx.load_texture("preview", image, egui::TextureOptions::LINEAR));
            }
            Err(e) => self.status = e.to_string(),
        }
    }
}

impl eframe::App for DesktopApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Project:");
                ui.text_edit_singleline(&mut self.project_path);
                if ui.button("Browse").clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("Project", &["toml"])
                        .pick_file()
                    {
                        self.project_path = path.display().to_string();
                    }
                }
                if ui.button("Load").clicked() {
                    self.load_project();
                }
            });
            ui.label(&self.status);
        });

        egui::SidePanel::left("left").show(ctx, |ui| {
            if let Some(timeline) = &self.timeline {
                ui.label(format!("Total: {:.2}s", timeline.total.0 as f64 / 48000.0));
                ui.label(format!("Scenes: {}", timeline.scenes.len()));
                ui.add(
                    egui::Slider::new(
                        &mut self.preview_frame,
                        0..=timeline.total.to_frame(30).saturating_sub(1),
                    )
                    .text("Frame"),
                );
                if ui.button("Render preview").clicked() {
                    self.render_preview(ctx);
                }
                if ui.button("Export mp4").clicked() {
                    if let (Some(project), Some(base), Some(timeline)) =
                        (&self.project, &self.base, &self.timeline)
                    {
                        let out = base.join("out.mp4");
                        match qc_export::export_video(
                            project,
                            base,
                            timeline,
                            &out,
                            30,
                            1080,
                            1920,
                            "final",
                            None,
                            None,
                            false,
                            true,
                            &mut |_| {},
                        ) {
                            Ok(()) => self.status = format!("Exported {}", out.display()),
                            Err(e) => self.status = e.to_string(),
                        }
                    }
                }
            }
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            if let Some(tex) = &self.preview_texture {
                let img = egui::Image::from_texture(tex).fit_to_original_size(1.0);
                ui.add(img);
            } else {
                ui.label("Preview area");
            }
        });
    }
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions::default();
    eframe::run_native(
        "QuickCut",
        options,
        Box::new(|cc| Ok(Box::new(DesktopApp::new(cc)))),
    )
}
