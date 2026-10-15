use qc_core::CoreError;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

pub fn render_frame_rgba(
    project: &qc_core::Project,
    base: &Path,
    timeline: &qc_core::Timeline,
    frame_index: u64,
    fps: u32,
    width: u32,
    height: u32,
) -> Result<Vec<u8>, CoreError> {
    let frame_sample = frame_index * qc_core::SAMPLE_RATE as u64 / fps as u64;

    // find scene span containing this frame
    let mut scene_index = timeline.scenes.len().saturating_sub(1);
    for (i, span) in timeline.scenes.iter().enumerate() {
        if frame_sample >= span.start.0 && frame_sample < span.end.0 {
            scene_index = i;
            break;
        }
    }

    let current = render_scene_canvas(
        project,
        base,
        timeline,
        scene_index,
        frame_index,
        fps,
        width,
        height,
    )?;

    // Cross-fade between overlapping scenes during a transition.
    if scene_index > 0 {
        let prev_span = &timeline.scenes[scene_index - 1];
        let curr_span = &timeline.scenes[scene_index];
        if frame_sample >= curr_span.start.0 && frame_sample < prev_span.end.0 {
            let denom = (prev_span.end.0 - curr_span.start.0).max(1) as f32;
            let t = ((frame_sample - curr_span.start.0) as f32 / denom).clamp(0.0, 1.0);
            let previous = render_scene_canvas(
                project,
                base,
                timeline,
                scene_index - 1,
                frame_index,
                fps,
                width,
                height,
            )?;
            let mut blended = image::RgbaImage::new(width, height);
            for (dst, (a, b)) in blended
                .pixels_mut()
                .zip(current.pixels().zip(previous.pixels()))
            {
                dst[0] = blend_channel(a[0], b[0], t);
                dst[1] = blend_channel(a[1], b[1], t);
                dst[2] = blend_channel(a[2], b[2], t);
                dst[3] = blend_channel(a[3], b[3], t);
            }
            return Ok(blended.into_raw());
        }
    }

    Ok(current.into_raw())
}

#[allow(clippy::too_many_arguments)]
fn render_scene_canvas(
    project: &qc_core::Project,
    base: &Path,
    timeline: &qc_core::Timeline,
    scene_index: usize,
    frame_index: u64,
    fps: u32,
    width: u32,
    height: u32,
) -> Result<image::RgbaImage, CoreError> {
    let span = &timeline.scenes[scene_index];
    let margin = project.layout.margin_x;
    let avail_width = width.saturating_sub(margin * 2);

    let image_path = base.join(&project.scenes[scene_index].image);
    let assets = cached_assets(project, &image_path, width, height, avail_width)?;

    // Animation progress.
    let scene_start_frame = span.start.to_frame(fps);
    let scene_end_frame = span.end.to_frame(fps);
    let in_duration = (0.4 * fps as f32).max(1.0);
    let out_duration = (0.35 * fps as f32).max(1.0);
    let t_in = ((frame_index as f32 - scene_start_frame as f32) / in_duration).clamp(0.0, 1.0);
    let t_out = ((scene_end_frame as f32 - frame_index as f32) / out_duration).clamp(0.0, 1.0);
    let ease_in = ease_in_out_cubic(t_in);
    let ease_out = ease_in_out_cubic(t_out);

    let anim_in = project.scenes[scene_index].anim_in.as_str();
    let anim_out = project.scenes[scene_index].anim_out.as_str();

    // Base scaled image and placement.
    let mut post = assets.scaled.clone();
    let mut offset_x = margin as i32;
    let scaled_w = post.dimensions().0;
    let scaled_h = post.dimensions().1;

    // Anim in.
    match anim_in {
        "pop" => {
            let scale = 0.82 + 0.18 * ease_in;
            let new_w = ((scaled_w as f32) * scale).round().max(1.0) as u32;
            let new_h = ((scaled_h as f32) * scale).round().max(1.0) as u32;
            post = image::imageops::resize(
                &assets.scaled,
                new_w,
                new_h,
                image::imageops::FilterType::Lanczos3,
            );
            offset_x = margin as i32 + (scaled_w as i32 - new_w as i32) / 2;
        }
        "slide_left" => {
            offset_x = margin as i32 + ((1.0 - ease_in) * width as f32 * 0.28).round() as i32;
        }
        _ => {}
    }

    // Anim out.
    if anim_out == "slide_left" {
        offset_x -= ((1.0 - ease_out) * width as f32 * 0.28).round() as i32;
    }

    // Rounded corners.
    round_corners(&mut post, project.layout.corner_radius);

    // Opacity.
    let opacity = (ease_in * (0.5 + 0.5 * ease_out)).clamp(0.0, 1.0);
    if opacity < 1.0 {
        for px in post.pixels_mut() {
            px[3] = (px[3] as f32 * opacity).round() as u8;
        }
    }

    // Scroll offset for long images.
    let offset_y = if scaled_h > height {
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
        progress = ease_in_out_cubic(progress);
        let max_off = scaled_h.saturating_sub(height);
        (max_off as f32 * progress).round() as u32
    } else {
        0
    };
    let y = if scaled_h > height {
        -(offset_y as i32)
    } else {
        ((height - scaled_h) / 2) as i32
    };

    // Background canvas.
    let mut canvas = assets.background.clone();

    // Shadow.
    let shadow = &project.layout.shadow;
    if shadow.alpha > 0.0 && opacity > 0.0 {
        let mut sh = assets.shadow.clone();
        if opacity < 1.0 {
            let opacity_byte = (opacity * 255.0).round() as u8;
            for px in sh.pixels_mut() {
                px[3] = px[3].saturating_mul(opacity_byte) / 255;
            }
        }
        image::imageops::overlay(&mut canvas, &sh, offset_x as i64, (y + shadow.dy) as i64);
    }

    image::imageops::overlay(&mut canvas, &post, offset_x as i64, y as i64);

    Ok(canvas)
}

#[derive(Clone)]
struct RenderAssets {
    background: image::RgbaImage,
    scaled: image::RgbaImage,
    shadow: image::RgbaImage,
}

static RENDER_CACHE: std::sync::LazyLock<Mutex<HashMap<String, Arc<RenderAssets>>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

fn cached_assets(
    project: &qc_core::Project,
    image_path: &Path,
    width: u32,
    height: u32,
    avail_width: u32,
) -> Result<Arc<RenderAssets>, CoreError> {
    let key = format!(
        "{}|{}x{}|{}|{}|{}|{}|{}|{}|{}",
        image_path.display(),
        width,
        height,
        project.background.kind,
        project.background.blur_sigma,
        project.background.dim,
        project.layout.corner_radius,
        project.layout.shadow.blur,
        project.layout.shadow.dy,
        project.layout.shadow.alpha,
    );

    if let Some(cached) = RENDER_CACHE.lock().unwrap().get(&key).cloned() {
        return Ok(cached);
    }

    let scene_image = image::open(image_path)
        .map_err(|e| CoreError::MissingFile {
            path: image_path.display().to_string(),
            reason: e.to_string(),
        })?
        .to_rgba8();

    let (orig_w, orig_h) = scene_image.dimensions();
    let scale = avail_width as f32 / orig_w as f32;
    let scaled_h = (orig_h as f32 * scale).round() as u32;
    let scaled = image::imageops::resize(
        &scene_image,
        avail_width,
        scaled_h,
        image::imageops::FilterType::Lanczos3,
    );

    let mut background = image::RgbaImage::new(width, height);
    fill_background(&mut background, project, &scene_image, width, height);

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
) {
    match project.background.kind.as_str() {
        "solid" => {
            for px in canvas.pixels_mut() {
                *px = image::Rgba([0, 0, 0, 255]);
            }
        }
        _ => {
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
            let dim = project.background.dim as f32;
            for (dst, srcpx) in canvas.pixels_mut().zip(blurred.pixels()) {
                let r = (srcpx[0] as f32 * (1.0 - dim)) as u8;
                let g = (srcpx[1] as f32 * (1.0 - dim)) as u8;
                let b = (srcpx[2] as f32 * (1.0 - dim)) as u8;
                *dst = image::Rgba([r, g, b, 255]);
            }
        }
    }
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
