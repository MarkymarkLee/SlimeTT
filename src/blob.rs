//! Streamer-slime icon (assets/slime.svg) for the menu bar and the popup, plus the "…" speech bubble.
use crate::theme::{BLUE, CRUST, TEXT};
use eframe::egui;

const SVG: &[u8] = include_bytes!("../assets/slime.svg");

fn tree() -> resvg::usvg::Tree {
    resvg::usvg::Tree::from_data(SVG, &Default::default()).expect("assets/slime.svg is valid")
}

/// Rasterize SVG units `(x0, y0)` + `w`×`h` into a `pw`×`ph` straight-alpha RGBA buffer.
fn render_area(x0: f32, y0: f32, w: f32, h: f32, pw: u32, ph: u32) -> Vec<u8> {
    let mut pm = resvg::tiny_skia::Pixmap::new(pw, ph).expect("non-zero size");
    let t = resvg::tiny_skia::Transform::from_translate(-x0, -y0).post_scale(pw as f32 / w, ph as f32 / h);
    resvg::render(&tree(), t, &mut pm.as_mut());
    pm.pixels()
        .iter()
        .flat_map(|c| {
            let c = c.demultiply();
            [c.red(), c.green(), c.blue(), c.alpha()]
        })
        .collect()
}

/// Whole SVG square at `px`×`px`.
fn render(px: u32) -> Vec<u8> {
    let s = tree().size().width();
    render_area(0.0, 0.0, s, s, px, px)
}

/// The drawn part of the SVG (x, y, w, h) as fractions of its square: the file has transparent margins,
/// which would make the slime look smaller and lower than whatever it sits next to.
fn bounds() -> (f32, f32, f32, f32) {
    static B: std::sync::OnceLock<(f32, f32, f32, f32)> = std::sync::OnceLock::new();
    *B.get_or_init(|| {
        const PROBE: usize = 400;
        let (mut x0, mut y0, mut x1, mut y1) = (PROBE, PROBE, 0, 0);
        for (i, px) in render(PROBE as u32).chunks(4).enumerate() {
            if px[3] > 0 {
                let (x, y) = (i % PROBE, i / PROBE);
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x + 1);
                y1 = y1.max(y + 1);
            }
        }
        let p = PROBE as f32;
        (x0 as f32 / p, y0 as f32 / p, (x1 - x0) as f32 / p, (y1 - y0) as f32 / p)
    })
}

/// Width / height of the trimmed slime `show` draws.
pub fn aspect() -> f32 {
    let (.., w, h) = bounds();
    w / h
}

/// Menu-bar icon: trimmed to the drawn pixels and rendered at the 22pt max height @2x, so it fills the bar.
/// Template image, so macOS tints it for light/dark menu bars.
pub fn tray_icon() -> Option<tray_icon::Icon> {
    let s = tree().size().width();
    let (x, y, w, h) = bounds();
    let pad = 0.08 * h; // transparent margin so it sits a bit smaller than the 22pt max
    let (w, h) = ((w + 2.0 * pad) * s, (h + 2.0 * pad) * s);
    let ph = 44;
    let pw = (ph as f32 * w / h).round() as u32;
    let rgba = render_area((x - pad) * s, (y - pad) * s, w, h, pw, ph);
    tray_icon::Icon::from_rgba(rgba, pw, ph)
        .map_err(|e| log::error!("tray icon: {e}"))
        .ok()
}

#[derive(Clone, Copy, PartialEq)]
pub enum Mood {
    Idle,
    Talking,
    Writing,
}

/// Static slime left of the transcript, trimmed so it is exactly `height` tall, with a "…" bubble while talking
/// or a scribbling pencil while writing.
pub fn show(ui: &mut egui::Ui, tex: &mut Option<egui::TextureHandle>, height: f32, time: f32, mood: Mood) {
    let (fx, fy, fw, fh) = bounds();
    let handle = tex.get_or_insert_with(|| {
        let ph = (96.0 * ui.ctx().pixels_per_point()) as u32; // fixed, above any size it is shown at
        let pw = (ph as f32 * fw / fh) as u32;
        let s = tree().size().width();
        let img = egui::ColorImage::from_rgba_unmultiplied(
            [pw as usize, ph as usize],
            &render_area(fx * s, fy * s, fw * s, fh * s, pw, ph),
        );
        ui.ctx().load_texture("slime", img, egui::TextureOptions::LINEAR)
    });
    let side = height / fh; // the untrimmed SVG square at this scale; bubble and pencil are placed against it
    let img = ui
        .add(egui::Image::new((handle.id(), egui::vec2(side * fw, height))).tint(BLUE))
        .rect; // the SVG is white: tint = colour
    let rect = egui::Rect::from_min_size(img.min - egui::vec2(fx, fy) * side, egui::Vec2::splat(side));
    let paint = ui.painter();
    let k = (side / 96.0).max(0.6); // designed at 96pt; don't let the dots get unreadably small
    if mood == Mood::Writing {
        // a ground line level with the slime's bottom, and a pencil writing along it on its sharpened point:
        // slow travel plus a small quick hand shake (short strokes, a wobbling lean). Big fast swings smeared
        // the whole pencil into one grey bar, so the shake stays within a couple of points.
        let ground = img.bottom() - 1.0 * k;
        // the line starts touching the bottom of the slime's round body and runs right through its empty bottom-right
        // corner; the pencil above it reaches the right ear cup
        let x0 = rect.right() - 40.0 * k;
        paint.line_segment(
            [egui::pos2(x0, ground), egui::pos2(x0 + 26.0 * k, ground)],
            egui::Stroke::new(2.0 * k, TEXT),
        );
        let travel = 13.0 + 10.0 * (time * 3.5).sin(); // glides across the whole line and back, ~1 s each way
        let stroke = egui::vec2(1.8 * (time * 19.0).sin(), -1.0 - 1.6 * (time * 13.0).sin().abs()); // little up-strokes off the line
        let tip = egui::pos2(x0 + (travel + stroke.x) * k, ground + stroke.y * k);
        let k = 0.8 * k; // the pencil itself a little smaller, to fit that gap
        let lean = (-0.8f32).atan2(0.6) + 0.13 * (time * 13.0).sin(); // up-right, wobbling
        let d = egui::vec2(lean.cos(), lean.sin());
        let n = egui::vec2(-d.y, d.x) * 4.0 * k; // half the width
        let slab = |a: f32, b: f32| {
            vec![
                tip + d * a * k - n,
                tip + d * a * k + n,
                tip + d * b * k + n,
                tip + d * b * k - n,
            ]
        };
        // no outlines: the parts are told apart by the gaps between them
        paint.add(egui::Shape::convex_polygon(
            vec![tip, tip + d * 9.0 * k + n, tip + d * 9.0 * k - n],
            TEXT,
            egui::Stroke::NONE,
        )); // sharpened point
        paint.add(egui::Shape::convex_polygon(slab(10.5, 26.0), TEXT, egui::Stroke::NONE)); // body
        paint.add(egui::Shape::convex_polygon(slab(27.5, 33.0), TEXT, egui::Stroke::NONE)); // eraser
        return;
    }
    if mood != Mood::Talking {
        return;
    }
    // bubble up and to the right of the head; three dots hop in sequence
    let b = egui::Rect::from_center_size(
        rect.right_top() + egui::vec2(-26.0, 26.0) * k,
        egui::vec2(34.0, 20.0) * k,
    ); // over the head, not past the slime's right edge: the text starts right there
    paint.rect_filled(b, 10.0 * k, TEXT);
    paint.circle_filled(b.left_bottom() + egui::vec2(4.0, 1.0) * k, 4.0 * k, TEXT);
    paint.circle_filled(b.left_bottom() + egui::vec2(-1.0, 6.0) * k, 2.0 * k, TEXT);
    for i in 0..3 {
        let hop = ((time * 6.0 - i as f32 * 0.9).sin().max(0.0)) * 3.0 * k;
        paint.circle_filled(
            b.center() + egui::vec2((i as f32 - 1.0) * 8.0 * k, -hop),
            2.5 * k,
            CRUST,
        );
    }
}

#[cfg(test)]
#[path = "../tests/blob.rs"]
mod tests;
