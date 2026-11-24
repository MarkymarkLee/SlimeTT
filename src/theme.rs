//! Catppuccin Mocha (https://catppuccin.com/palette) for the popup, Settings and onboarding.
use eframe::egui::{self, Color32, Stroke};

const fn hex(c: u32) -> Color32 {
    Color32::from_rgb((c >> 16) as u8, (c >> 8) as u8, c as u8)
}
pub const BASE: Color32 = hex(0x1e1e2e);
pub const MANTLE: Color32 = hex(0x181825);
pub const CRUST: Color32 = hex(0x11111b);
pub const SURFACE0: Color32 = hex(0x313244);
pub const SURFACE1: Color32 = hex(0x45475a);
pub const SURFACE2: Color32 = hex(0x585b70);
pub const OVERLAY: Color32 = hex(0x7f849c); // overlay1: text that isn't final yet
pub const SUBTEXT: Color32 = hex(0xa6adc8); // subtext0: help text
pub const TEXT: Color32 = hex(0xcdd6f4);
pub const BLUE: Color32 = hex(0x89b4fa);
pub const GREEN: Color32 = hex(0xa6e3a1);
pub const RED: Color32 = hex(0xf38ba8);
pub const PEACH: Color32 = hex(0xfab387);

pub fn apply(ctx: &egui::Context) {
    ctx.set_theme(egui::Theme::Dark); // not the system's light/dark: the palette is one dark flavour
    let mut v = egui::Visuals::dark();
    (
        v.panel_fill,
        v.window_fill,
        v.faint_bg_color,
        v.extreme_bg_color,
        v.code_bg_color,
    ) = (BASE, BASE, MANTLE, CRUST, MANTLE);
    (v.hyperlink_color, v.error_fg_color, v.warn_fg_color) = (BLUE, RED, PEACH);
    v.window_stroke = Stroke::new(1.0, SURFACE1);
    v.selection.bg_fill = SURFACE2;
    v.selection.stroke = Stroke::new(1.0, BLUE);
    let w = &mut v.widgets;
    w.noninteractive.bg_fill = BASE;
    w.noninteractive.bg_stroke = Stroke::new(1.0, SURFACE1); // separators
    for (s, fill) in [
        (&mut w.inactive, SURFACE0),
        (&mut w.hovered, SURFACE1),
        (&mut w.active, SURFACE2),
        (&mut w.open, SURFACE0),
    ] {
        (s.bg_fill, s.weak_bg_fill) = (fill, fill);
        s.bg_stroke = Stroke::new(s.bg_stroke.width, BLUE);
        s.fg_stroke.color = TEXT;
    }
    w.noninteractive.fg_stroke.color = TEXT;
    ctx.set_visuals(v);
}
