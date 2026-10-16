use qc_core::{CoreError, Fit};
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use thiserror::Error;

/// Errors owned by the render stage.
#[derive(Debug, Error)]
pub enum RenderError {
    #[error("cannot read image {path}: {reason}")]
    Decode { path: String, reason: String },
    #[error(
        "image {path} is {width}x{height}, narrower than the {}px minimum",
        qc_core::MIN_IMAGE_WIDTH
    )]
    TooSmall {
        path: String,
        width: u32,
        height: u32,
    },
    #[error(transparent)]
    Project(#[from] CoreError),
}

impl RenderError {
    /// Process exit code. Part of the CLI contract.
    pub fn exit_code(&self) -> i32 {
        match self {
            RenderError::Project(e) => e.exit_code(),
            _ => 1,
        }
    }
}

// Per-thread scratch so the hot path allocates nothing. Rayon gives each worker
// its own, and the desktop preview only ever runs on the UI thread.
thread_local! {
    static POST: RefCell<image::RgbaImage> = RefCell::new(image::RgbaImage::new(0, 0));
    static SHADOW: RefCell<image::RgbaImage> = RefCell::new(image::RgbaImage::new(0, 0));
    static FADE: RefCell<image::RgbaImage> = RefCell::new(image::RgbaImage::new(0, 0));
}

fn fit_scratch(buf: &mut image::RgbaImage, width: u32, height: u32) {
    if buf.dimensions() != (width, height) {
        *buf = image::RgbaImage::new(width, height);
    }
}

/// Plain pixel copy between equally sized buffers, without touching the trait
/// `image` exposes under the same name, and without reallocating the scratch.
fn copy_pixels(dst: &mut image::RgbaImage, src: &image::RgbaImage) {
    debug_assert_eq!(dst.dimensions(), src.dimensions());
    let (width, height) = dst.dimensions();
    let mut data = std::mem::take(dst).into_raw();
    data.copy_from_slice(src.as_raw());
    *dst = image::RgbaImage::from_raw(width, height, data)
        .unwrap_or_else(|| image::RgbaImage::new(width, height));
}

/// Render one frame into `out`, reusing its allocation across frames.
///
/// `background` is the decoded video-background frame for this index, when the
/// project uses `background.kind = "video"`; it is `None` for the solid and
/// blur-from-image backgrounds, which are baked per scene instead.
#[allow(clippy::too_many_arguments)]
pub fn render_frame_into(
    out: &mut Vec<u8>,
    project: &qc_core::Project,
    base: &Path,
    timeline: &qc_core::Timeline,
    frame_index: u64,
    fps: u32,
    width: u32,
    height: u32,
    background: Option<&image::RgbaImage>,
) -> Result<(), RenderError> {
    let frame_sample = frame_index * qc_core::SAMPLE_RATE as u64 / fps as u64;

    let scene_index = timeline.scene_index(qc_core::Samples(frame_sample));
    // `from_raw` adopts the caller's buffer, so the finished frame is handed back
    // without a copy and without a fresh allocation.
    let mut canvas = match image::RgbaImage::from_raw(width, height, std::mem::take(out)) {
        Some(canvas) => canvas,
        None => image::RgbaImage::new(width, height),
    };

    match fade_source(timeline, scene_index, frame_sample) {
        Some((previous_index, t)) => {
            draw_scene(
                &mut canvas,
                project,
                base,
                timeline,
                scene_index,
                frame_index,
                fps,
                width,
                height,
                background,
            )?;
            FADE.with(|slot| -> Result<(), RenderError> {
                let mut previous = slot.borrow_mut();
                fit_scratch(&mut previous, width, height);
                draw_scene(
                    &mut previous,
                    project,
                    base,
                    timeline,
                    previous_index,
                    frame_index,
                    fps,
                    width,
                    height,
                    background,
                )?;
                for (dst, previous_px) in canvas.pixels_mut().zip(previous.pixels()) {
                    for c in 0..4 {
                        dst[c] = blend_channel(dst[c], previous_px[c], t);
                    }
                }
                Ok(())
            })?;
        }
        None => draw_scene(
            &mut canvas,
            project,
            base,
            timeline,
            scene_index,
            frame_index,
            fps,
            width,
            height,
            background,
        )?,
    }

    *out = canvas.into_raw();
    Ok(())
}

/// Drop all cached assets and settled frames. Call after project edits.
pub fn reset_caches() {
    RENDER_CACHE.lock().unwrap().clear();
    STATIC_SCENE.lock().unwrap().clear();
}

/// Render one frame into a freshly allocated buffer.
#[allow(clippy::too_many_arguments)]
pub fn render_frame_rgba(
    project: &qc_core::Project,
    base: &Path,
    timeline: &qc_core::Timeline,
    frame_index: u64,
    fps: u32,
    width: u32,
    height: u32,
    background: Option<&image::RgbaImage>,
) -> Result<Vec<u8>, RenderError> {
    let mut out = Vec::with_capacity(width as usize * height as usize * 4);
    render_frame_into(
        &mut out,
        project,
        base,
        timeline,
        frame_index,
        fps,
        width,
        height,
        background,
    )?;
    Ok(out)
}

/// Previous scene still visible during a cross-fade, plus how far the fade has run.
fn fade_source(
    timeline: &qc_core::Timeline,
    scene_index: usize,
    frame_sample: u64,
) -> Option<(usize, f32)> {
    if scene_index == 0 {
        return None;
    }
    let previous = &timeline.scenes[scene_index - 1];
    let current = &timeline.scenes[scene_index];
    if frame_sample < current.start.0 || frame_sample >= previous.end.0 {
        return None;
    }
    let span = (previous.end.0 - current.start.0).max(1) as f32;
    let t = ((frame_sample - current.start.0) as f32 / span).clamp(0.0, 1.0);
    Some((scene_index - 1, t))
}

#[allow(clippy::too_many_arguments)]
fn draw_scene(
    canvas: &mut image::RgbaImage,
    project: &qc_core::Project,
    base: &Path,
    timeline: &qc_core::Timeline,
    scene_index: usize,
    frame_index: u64,
    fps: u32,
    width: u32,
    height: u32,
    background: Option<&image::RgbaImage>,
) -> Result<(), RenderError> {
    let span = &timeline.scenes[scene_index];
    let margin = project.layout.margin_x;
    let avail_width = width.saturating_sub(margin * 2);

    let image_path = base.join(&span.image);
    let assets = cached_assets(project, &image_path, width, height, avail_width)?;

    // Animation progress.
    let scene_start_frame = span.start.to_frame(fps);
    let scene_end_frame = span.end.to_frame(fps);
    let in_duration = (0.4 * fps as f32).max(1.0);
    let out_duration = (0.35 * fps as f32).max(1.0);
    let t_in = ((frame_index as f32 - scene_start_frame as f32) / in_duration).clamp(0.0, 1.0);
    let t_out = ((scene_end_frame as f32 - frame_index as f32) / out_duration).clamp(0.0, 1.0);

    // Once both easings have settled and nothing scrolls, every remaining frame of
    // the scene is pixel identical. Compositing a long static video is pure waste,
    // so render it once and hand out copies. A video background changes every frame,
    // so those scenes never settle.
    let video_background = project.background.kind == "video";
    let settled =
        t_in >= 1.0 && t_out >= 1.0 && matches!(assets.fit, Fit::Centered) && !video_background;
    let static_key = format!(
        "{scene_index}|{}",
        assets_key(project, &image_path, width, height)
    );
    if settled {
        if let Some(ready) = STATIC_SCENE.lock().unwrap().get(&static_key).cloned() {
            copy_pixels(canvas, &ready);
            return Ok(());
        }
    }

    draw_scene_now(
        canvas,
        project,
        span,
        scene_index,
        &assets,
        t_in,
        t_out,
        frame_index,
        fps,
        width,
        height,
        background,
    )?;

    if settled {
        STATIC_SCENE
            .lock()
            .unwrap()
            .insert(static_key, canvas.clone());
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn draw_scene_now(
    canvas: &mut image::RgbaImage,
    project: &qc_core::Project,
    span: &qc_core::SceneSpan,
    scene_index: usize,
    assets: &RenderAssets,
    t_in: f32,
    t_out: f32,
    frame_index: u64,
    fps: u32,
    width: u32,
    height: u32,
    background: Option<&image::RgbaImage>,
) -> Result<(), RenderError> {
    let ease_in = ease_in_out_cubic(t_in);
    let ease_out = ease_in_out_cubic(t_out);
    let opacity = (ease_in * (0.5 + 0.5 * ease_out)).clamp(0.0, 1.0);

    let margin = project.layout.margin_x;
    let scaled_w = assets.scaled.width();
    let scaled_h = assets.scaled.height();
    let mut offset_x = margin as i32;

    POST.with(|slot| {
        let mut post = slot.borrow_mut();

        match project.scenes[scene_index].anim_in.as_str() {
            "pop" => {
                let scale = 0.82 + 0.18 * ease_in;
                let new_w = ((scaled_w as f32) * scale).round().max(1.0) as u32;
                let new_h = ((scaled_h as f32) * scale).round().max(1.0) as u32;
                // Bilinear on purpose: this runs per frame on a 0.82 to 1.0 zoom,
                // where the cost of Lanczos buys nothing visible.
                *post = image::imageops::resize(
                    &assets.scaled,
                    new_w,
                    new_h,
                    image::imageops::FilterType::Triangle,
                );
                offset_x += (scaled_w as i32 - new_w as i32) / 2;
            }
            "slide_left" => {
                offset_x += ((1.0 - ease_in) * width as f32 * 0.28).round() as i32;
            }
            _ => {
                if post.dimensions() != assets.scaled.dimensions() {
                    *post = assets.scaled.clone();
                } else {
                    copy_pixels(&mut post, &assets.scaled);
                }
            }
        }

        if project.scenes[scene_index].anim_out == "slide_left" {
            offset_x -= ((1.0 - ease_out) * width as f32 * 0.28).round() as i32;
        }

        round_corners(&mut post, project.layout.corner_radius);

        if opacity < 1.0 {
            for px in post.pixels_mut() {
                px[3] = (px[3] as f32 * opacity).round() as u8;
            }
        }

        // Background is fully opaque, so a straight copy replaces the old clone.
        // A video background is supplied per frame; otherwise it is the cached one.
        match background {
            Some(frame) => copy_pixels(canvas, frame),
            None => copy_pixels(canvas, &assets.background),
        }

        let y = match assets.fit {
            Fit::Centered => ((height - scaled_h) / 2) as i32,
            Fit::Scroll { max_offset_px } => {
                -(scroll_offset(span, frame_index, fps, max_offset_px) as i32)
            }
        };

        let shadow = &project.layout.shadow;
        if shadow.alpha > 0.0 && opacity > 0.0 {
            if opacity >= 1.0 {
                image::imageops::overlay(
                    canvas,
                    &assets.shadow,
                    offset_x as i64,
                    (y + shadow.dy) as i64,
                );
            } else {
                SHADOW.with(|slot| {
                    let mut faded = slot.borrow_mut();
                    if faded.dimensions() != assets.shadow.dimensions() {
                        *faded = assets.shadow.clone();
                    } else {
                        copy_pixels(&mut faded, &assets.shadow);
                    }
                    let opacity_byte = (opacity * 255.0).round() as u8;
                    for px in faded.pixels_mut() {
                        px[3] = px[3].saturating_mul(opacity_byte) / 255;
                    }
                    image::imageops::overlay(
                        canvas,
                        &*faded,
                        offset_x as i64,
                        (y + shadow.dy) as i64,
                    );
                });
            }
        }

        image::imageops::overlay(canvas, &*post, offset_x as i64, y as i64);
    });

    Ok(())
}

/// Scroll position of a tall screenshot, eased and held still at both ends.
fn scroll_offset(span: &qc_core::SceneSpan, frame_index: u64, fps: u32, max_offset_px: u32) -> u32 {
    let voice_start_frame = span.voice_start.to_frame(fps);
    let voice_end_frame = span.voice_end.to_frame(fps);
    let mut progress = 0.0f32;
    if voice_end_frame > voice_start_frame {
        let delay_frames = (0.4 * fps as f32).round() as u64;
        let s = voice_start_frame + delay_frames;
        let e = voice_end_frame.saturating_sub(delay_frames);
        if frame_index <= s {
            progress = 0.0;
        } else if frame_index >= e {
            progress = 1.0;
        } else if e > s {
            progress = (frame_index - s) as f32 / (e - s) as f32;
        }
    }
    (max_offset_px as f32 * ease_in_out_cubic(progress)).round() as u32
}

#[derive(Clone)]
struct RenderAssets {
    background: image::RgbaImage,
    scaled: image::RgbaImage,
    shadow: image::RgbaImage,
    fit: Fit,
}

static RENDER_CACHE: std::sync::LazyLock<Mutex<HashMap<String, Arc<RenderAssets>>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

/// Fully composited frames of scenes whose animation has settled.
static STATIC_SCENE: std::sync::LazyLock<Mutex<HashMap<String, image::RgbaImage>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

fn assets_key(project: &qc_core::Project, image_path: &Path, width: u32, height: u32) -> String {
    format!(
        "{}|{}x{}|{}|{}|{}|{}|{}|{}",
        image_path.display(),
        width,
        height,
        project.background.kind,
        project.background.blur_sigma,
        project.layout.corner_radius,
        project.layout.shadow.blur,
        project.layout.shadow.dy,
        project.layout.shadow.alpha,
    )
}

fn cached_assets(
    project: &qc_core::Project,
    image_path: &Path,
    width: u32,
    height: u32,
    avail_width: u32,
) -> Result<Arc<RenderAssets>, RenderError> {
    let key = assets_key(project, image_path, width, height);

    if let Some(cached) = RENDER_CACHE.lock().unwrap().get(&key).cloned() {
        return Ok(cached);
    }

    let scene_image = image::open(image_path)
        .map_err(|e| RenderError::Decode {
            path: image_path.display().to_string(),
            reason: e.to_string(),
        })?
        .to_rgba8();

    let (orig_w, orig_h) = scene_image.dimensions();
    if orig_w < qc_core::MIN_IMAGE_WIDTH {
        return Err(RenderError::TooSmall {
            path: image_path.display().to_string(),
            width: orig_w,
            height: orig_h,
        });
    }
    let fit = qc_core::fit_image(orig_w, orig_h, width, height, project.layout.margin_x);
    let scale = avail_width as f32 / orig_w as f32;
    let scaled_h = (orig_h as f32 * scale).round() as u32;
    let scaled = image::imageops::resize(
        &scene_image,
        avail_width,
        scaled_h,
        image::imageops::FilterType::Lanczos3,
    );

    let mut background = image::RgbaImage::new(width, height);
    fill_background(&mut background, project, &scene_image, width, height)?;

    // Shadow base: rounded-ish silhouette from scaled alpha, blurred.
    let mut shadow_mask = image::RgbaImage::new(scaled.dimensions().0, scaled.dimensions().1);
    for (dst, src) in shadow_mask.pixels_mut().zip(scaled.pixels()) {
        if src[3] > 0 {
            *dst = image::Rgba([0, 0, 0, 255]);
        } else {
            *dst = image::Rgba([0, 0, 0, 0]);
        }
    }
    let mut shadow = fast_blur(&shadow_mask, project.layout.shadow.blur as f32);
    if project.layout.shadow.alpha < 1.0 {
        let shadow_alpha = (project.layout.shadow.alpha * 255.0).round() as u8;
        for px in shadow.pixels_mut() {
            px[3] = px[3].saturating_mul(shadow_alpha) / 255;
        }
    }

    let assets = Arc::new(RenderAssets {
        background,
        scaled,
        shadow,
        fit,
    });

    RENDER_CACHE.lock().unwrap().insert(key, assets.clone());

    Ok(assets)
}

fn fast_blur(src: &image::RgbaImage, sigma: f32) -> image::RgbaImage {
    if sigma <= 0.0 {
        return src.clone();
    }
    let down = ((sigma / 8.0).ceil() as u32).clamp(1, 8);
    let small_w = (src.width() / down).max(1);
    let small_h = (src.height() / down).max(1);
    let small =
        image::imageops::resize(src, small_w, small_h, image::imageops::FilterType::Triangle);
    let blurred_small = image::imageops::blur(&small, sigma / down as f32);
    image::imageops::resize(
        &blurred_small,
        src.width(),
        src.height(),
        image::imageops::FilterType::Triangle,
    )
}

fn fill_background(
    canvas: &mut image::RgbaImage,
    project: &qc_core::Project,
    src: &image::RgbaImage,
    width: u32,
    height: u32,
) -> Result<(), RenderError> {
    match project.background.kind.as_str() {
        "solid" => {
            for px in canvas.pixels_mut() {
                *px = image::Rgba([0, 0, 0, 255]);
            }
        }
        "blur_from_image" => {
            let (w, h) = src.dimensions();
            let sx = width as f32 / w as f32;
            let sy = height as f32 / h as f32;
            let s = sx.max(sy);
            let nw = (w as f32 * s).round() as u32;
            let nh = (h as f32 * s).round() as u32;
            let resized =
                image::imageops::resize(src, nw, nh, image::imageops::FilterType::Triangle);
            let xo = (nw.saturating_sub(width)) / 2;
            let yo = (nh.saturating_sub(height)) / 2;
            let cropped = image::imageops::crop_imm(&resized, xo, yo, width, height).to_image();
            let blurred = fast_blur(&cropped, project.background.blur_sigma as f32);
            for (dst, srcpx) in canvas.pixels_mut().zip(blurred.pixels()) {
                *dst = image::Rgba([srcpx[0], srcpx[1], srcpx[2], 255]);
            }
        }
        "video" => {
            // The clip is decoded per frame by the exporter and handed to
            // `render_frame_into`; this filler is only a placeholder for previews.
            for px in canvas.pixels_mut() {
                *px = image::Rgba([0, 0, 0, 255]);
            }
        }
        other => {
            return Err(RenderError::Project(CoreError::NotImplemented {
                feature: format!(
                    "background kind {other:?} (supported: solid, blur_from_image, video)"
                ),
            }));
        }
    }
    Ok(())
}

fn round_corners(img: &mut image::RgbaImage, radius: u32) {
    if radius == 0 {
        return;
    }
    let (w, h) = img.dimensions();
    let r = radius.min(w.min(h) / 2) as i32;
    if r == 0 {
        return;
    }
    let r2 = r * r;
    let w = w as i32;
    let h = h as i32;

    for y in 0..r {
        for x in 0..r {
            if (x - r + 1).pow(2) + (y - r + 1).pow(2) > r2 {
                img.get_pixel_mut(x as u32, y as u32)[3] = 0;
            }
        }
        for x in (w - r)..w {
            if (x - (w - r)).pow(2) + (y - r + 1).pow(2) > r2 {
                img.get_pixel_mut(x as u32, y as u32)[3] = 0;
            }
        }
    }
    for y in (h - r)..h {
        for x in 0..r {
            if (x - r + 1).pow(2) + (y - (h - r)).pow(2) > r2 {
                img.get_pixel_mut(x as u32, y as u32)[3] = 0;
            }
        }
        for x in (w - r)..w {
            if (x - (w - r)).pow(2) + (y - (h - r)).pow(2) > r2 {
                img.get_pixel_mut(x as u32, y as u32)[3] = 0;
            }
        }
    }
}

fn ease_in_out_cubic(t: f32) -> f32 {
    if t < 0.5 {
        4.0 * t * t * t
    } else {
        1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
    }
}

fn blend_channel(a: u8, b: u8, t: f32) -> u8 {
    ((a as f32) * (1.0 - t) + (b as f32) * t).round() as u8
}
