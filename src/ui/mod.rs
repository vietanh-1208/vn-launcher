pub mod accounts;
pub mod content;
pub mod dialogs;
pub mod optimizer_ui;
pub mod settings_logs;
pub mod worlds_servers;

use crate::auth::Account;
use eframe::egui::{
    self, Align2, Color32, FontData, FontDefinitions, FontFamily, FontId, IconData, Pos2,
    RichText, Rounding, Stroke, Vec2,
};
use std::fs;

// ---------- White & green palette ----------
pub const GREEN: Color32 = Color32::from_rgb(22, 163, 74); // #16A34A
pub const GREEN_DARK: Color32 = Color32::from_rgb(21, 128, 61); // #15803D
pub const GREEN_DEEP: Color32 = Color32::from_rgb(20, 83, 45); // #14532D
pub const GREEN_BRIGHT: Color32 = Color32::from_rgb(74, 222, 128); // #4ADE80
pub const GREEN_SOFT: Color32 = Color32::from_rgb(220, 252, 231); // #DCFCE7
pub const GREEN_MIST: Color32 = Color32::from_rgb(240, 253, 244); // #F0FDF4
pub const BG: Color32 = Color32::from_rgb(244, 250, 246);
pub const WHITE: Color32 = Color32::WHITE;
pub const BORDER: Color32 = Color32::from_rgb(209, 231, 219);
pub const TEXT: Color32 = Color32::from_rgb(17, 38, 29);
pub const MUTED: Color32 = Color32::from_rgb(91, 112, 101);
pub const RED: Color32 = Color32::from_rgb(220, 38, 38);
pub const RED_SOFT: Color32 = Color32::from_rgb(254, 226, 226);
pub const AMBER_SOFT: Color32 = Color32::from_rgb(254, 243, 199);
pub const AMBER_TEXT: Color32 = Color32::from_rgb(146, 64, 14);

pub fn card<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::none()
        .fill(WHITE)
        .rounding(Rounding::same(14.0))
        .inner_margin(egui::Margin::same(18.0))
        .stroke(Stroke::new(1.0_f32, BORDER))
        .shadow(egui::epaint::Shadow {
            offset: Vec2::new(0.0, 2.0),
            blur: 8.0,
            spread: 0.0,
            color: Color32::from_black_alpha(10),
        })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        })
        .inner
}

pub fn title(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).size(17.0).strong().color(TEXT));
}

pub fn hint(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).size(12.5).color(MUTED));
}

pub fn btn_primary(text: &str) -> egui::Button<'static> {
    egui::Button::new(RichText::new(text.to_string()).strong().color(WHITE))
        .fill(GREEN)
        .stroke(Stroke::NONE)
        .rounding(Rounding::same(8.0))
        .min_size(Vec2::new(0.0, 32.0))
}

pub fn btn_soft(text: &str) -> egui::Button<'static> {
    egui::Button::new(RichText::new(text.to_string()).color(GREEN_DARK))
        .fill(GREEN_SOFT)
        .stroke(Stroke::NONE)
        .rounding(Rounding::same(8.0))
        .min_size(Vec2::new(0.0, 30.0))
}

pub fn btn_danger(text: &str) -> egui::Button<'static> {
    egui::Button::new(RichText::new(text.to_string()).color(RED))
        .fill(RED_SOFT)
        .stroke(Stroke::NONE)
        .rounding(Rounding::same(8.0))
        .min_size(Vec2::new(0.0, 30.0))
}

pub fn notice(ui: &mut egui::Ui, text: &str) {
    egui::Frame::none()
        .fill(AMBER_SOFT)
        .rounding(Rounding::same(8.0))
        .inner_margin(egui::Margin::symmetric(12.0, 8.0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(text).size(12.5).color(AMBER_TEXT));
        });
}

pub fn pill(ui: &mut egui::Ui, text: &str) {
    egui::Frame::none()
        .fill(GREEN_SOFT)
        .rounding(Rounding::same(20.0))
        .inner_margin(egui::Margin::symmetric(10.0, 3.0))
        .show(ui, |ui| {
            ui.label(RichText::new(text).size(12.0).color(GREEN_DARK));
        });
}

pub fn draw_logo(ui: &mut egui::Ui, size: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), egui::Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, Rounding::same(size * 0.26), GREEN_DEEP);
    p.rect_stroke(
        rect.shrink(2.5),
        Rounding::same(size * 0.22),
        Stroke::new(1.0_f32, Color32::from_white_alpha(70)),
    );
    let r = size * 0.05;
    p.circle_filled(
        Pos2::new(rect.right() - size * 0.17, rect.top() + size * 0.17),
        r,
        GREEN_BRIGHT,
    );
    p.circle_filled(
        Pos2::new(rect.left() + size * 0.17, rect.bottom() - size * 0.17),
        r,
        WHITE,
    );
    let c = rect.center();
    let f = FontId::proportional(size * 0.46);
    p.text(Pos2::new(c.x - size * 0.14, c.y), Align2::CENTER_CENTER, "V", f.clone(), WHITE);
    p.text(Pos2::new(c.x + size * 0.15, c.y), Align2::CENTER_CENTER, "N", f, GREEN_BRIGHT);
}

pub fn setup_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    for path in [
        "C:\\Windows\\Fonts\\segoeui.ttf",
        "C:\\Windows\\Fonts\\tahoma.ttf",
        "C:\\Windows\\Fonts\\arial.ttf",
    ] {
        if let Ok(bytes) = fs::read(path) {
            fonts.font_data.insert("vn".into(), FontData::from_owned(bytes));
            if let Some(f) = fonts.families.get_mut(&FontFamily::Proportional) {
                f.insert(0, "vn".into());
            }
            if let Some(f) = fonts.families.get_mut(&FontFamily::Monospace) {
                f.push("vn".into());
            }
            break;
        }
    }
    if let Ok(bytes) = fs::read("C:\\Windows\\Fonts\\segoeuib.ttf") {
        fonts.font_data.insert("vn_bold".into(), FontData::from_owned(bytes));
    }
    ctx.set_fonts(fonts);
}

pub fn setup_theme(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = Vec2::new(10.0, 8.0);
    style.spacing.button_padding = Vec2::new(14.0, 6.0);
    style.spacing.interact_size.y = 28.0;
    style.spacing.slider_width = 260.0;

    let mut v = egui::Visuals::light();
    v.panel_fill = BG;
    v.window_fill = WHITE;
    v.window_stroke = Stroke::new(1.0_f32, BORDER);
    v.extreme_bg_color = WHITE;
    v.faint_bg_color = GREEN_MIST;
    v.override_text_color = None;
    v.hyperlink_color = GREEN_DARK;

    v.widgets.noninteractive.bg_fill = WHITE;
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, BORDER);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, TEXT);

    v.widgets.inactive.bg_fill = GREEN_MIST;
    v.widgets.inactive.weak_bg_fill = GREEN_MIST;
    v.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, BORDER);
    v.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, TEXT);
    v.widgets.inactive.rounding = Rounding::same(8.0);

    v.widgets.hovered.bg_fill = GREEN_SOFT;
    v.widgets.hovered.weak_bg_fill = GREEN_SOFT;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, GREEN);
    v.widgets.hovered.fg_stroke = Stroke::new(1.5_f32, GREEN_DEEP);
    v.widgets.hovered.rounding = Rounding::same(8.0);

    v.widgets.active.bg_fill = GREEN_SOFT;
    v.widgets.active.weak_bg_fill = GREEN_SOFT;
    v.widgets.active.bg_stroke = Stroke::new(1.5_f32, GREEN_DARK);
    v.widgets.active.fg_stroke = Stroke::new(1.5_f32, GREEN_DEEP);
    v.widgets.active.rounding = Rounding::same(8.0);

    v.widgets.open.bg_fill = GREEN_SOFT;
    v.widgets.open.weak_bg_fill = GREEN_SOFT;
    v.widgets.open.fg_stroke = Stroke::new(1.0_f32, TEXT);

    v.selection.bg_fill = GREEN;
    v.selection.stroke = Stroke::new(1.0_f32, WHITE);
    v.popup_shadow.color = Color32::from_black_alpha(25);
    v.window_shadow.color = Color32::from_black_alpha(35);
    style.visuals = v;
    ctx.set_style(style);
}

/// High-contrast 64x64 "VN" Emerald-Block icon for the window & taskbar
pub fn make_icon() -> IconData {
    let (w, h) = (64u32, 64u32);
    let mut rgba = vec![0u8; (w * h * 4) as usize];
    let mut blend = |x: i32, y: i32, c: [u8; 4], alpha: f32| {
        if x >= 0 && y >= 0 && (x as u32) < w && (y as u32) < h && alpha > 0.0 {
            let a = (alpha.clamp(0.0, 1.0) * (c[3] as f32 / 255.0)).clamp(0.0, 1.0);
            let i = ((y as u32 * w + x as u32) * 4) as usize;
            let dst_a = rgba[i + 3] as f32 / 255.0;
            let out_a = a + dst_a * (1.0 - a);
            if out_a > 0.0 {
                for ch in 0..3 {
                    let src_c = c[ch] as f32 / 255.0;
                    let dst_c = rgba[i + ch] as f32 / 255.0;
                    let out_c = (src_c * a + dst_c * dst_a * (1.0 - a)) / out_a;
                    rgba[i + ch] = (out_c * 255.0).round() as u8;
                }
                rgba[i + 3] = (out_a * 255.0).round() as u8;
            }
        }
    };
    let r = 15.0_f32;
    for y in 0..h as i32 {
        for x in 0..w as i32 {
            let fx = x as f32 + 0.5;
            let fy = y as f32 + 0.5;
            let cx = fx.clamp(r, w as f32 - r);
            let cy = fy.clamp(r, h as f32 - r);
            let d = ((fx - cx).powi(2) + (fy - cy).powi(2)).sqrt();
            if d <= r + 0.8 {
                let cov = (r + 0.5 - d).clamp(0.0, 1.0);
                let t = fy / h as f32;
                let bg = [
                    (18.0 + 10.0 * (1.0 - t)) as u8,
                    (95.0 + 68.0 * (1.0 - t)) as u8,
                    (48.0 + 34.0 * (1.0 - t)) as u8,
                    255,
                ];
                blend(x, y, bg, cov);
                let grass_h = 12.0 + ((x / 4) % 3) as f32 * 2.5;
                if fy < grass_h && d <= r - 2.0 {
                    blend(x, y, [74, 222, 128, 95], cov);
                }
                let edge = (r - d).min(fx).min(fy).min(w as f32 - fx).min(h as f32 - fy);
                if edge < 3.2 {
                    let rim = if fy < 24.0 { [134, 239, 172, 255] } else { [34, 197, 94, 255] };
                    blend(x, y, rim, cov * (1.0 - (edge / 3.2).clamp(0.0, 1.0) * 0.4));
                }
            }
        }
    }
    let mut stroke = |x0: f32, y0: f32, x1: f32, y1: f32, t: f32, c: [u8; 4]| {
        let min_x = (x0.min(x1) - t).floor() as i32;
        let max_x = (x0.max(x1) + t).ceil() as i32;
        let min_y = (y0.min(y1) - t).floor() as i32;
        let max_y = (y0.max(y1) + t).ceil() as i32;
        let dx = x1 - x0;
        let dy = y1 - y0;
        let len2 = (dx * dx + dy * dy).max(0.001);
        let rad = t * 0.5;
        for py in min_y..=max_y {
            for px in min_x..=max_x {
                let fx = px as f32 + 0.5;
                let fy = py as f32 + 0.5;
                let proj = (((fx - x0) * dx + (fy - y0) * dy) / len2).clamp(0.0, 1.0);
                let nx = x0 + proj * dx;
                let ny = y0 + proj * dy;
                let dist = ((fx - nx).powi(2) + (fy - ny).powi(2)).sqrt();
                if dist <= rad + 0.8 {
                    blend(px, py, c, (rad + 0.5 - dist).clamp(0.0, 1.0));
                }
            }
        }
    };
    let shadow = [10, 42, 22, 190];
    let white = [255, 255, 255, 255];
    let mint = [110, 245, 158, 255];
    for (ax, ay, bx, by) in [
        (11.0, 19.0, 19.5, 47.0),
        (19.5, 47.0, 28.0, 19.0),
        (35.0, 47.0, 35.0, 19.0),
        (35.0, 19.0, 51.0, 47.0),
        (51.0, 47.0, 51.0, 19.0),
    ] {
        stroke(ax + 1.5, ay + 2.0, bx + 1.5, by + 2.0, 6.2, shadow);
    }
    stroke(11.0, 19.0, 19.5, 47.0, 6.0, white);
    stroke(19.5, 47.0, 28.0, 19.0, 6.0, white);
    stroke(35.0, 47.0, 35.0, 19.0, 6.0, mint);
    stroke(35.0, 19.0, 51.0, 47.0, 5.8, white);
    stroke(51.0, 47.0, 51.0, 19.0, 6.0, mint);
    IconData { rgba, width: w, height: h }
}

pub fn ensure_avatar_texture(
    cache: &mut std::collections::HashMap<String, egui::TextureHandle>,
    ctx: &egui::Context,
    acc: &Account,
) -> egui::TextureId {
    if let Some(tex) = cache.get(&acc.id) {
        return tex.id();
    }
    let mut pixels = vec![0u8; 8 * 8 * 4];
    let loaded = if !acc.offline_skin.trim().is_empty() {
        image::open(acc.offline_skin.trim()).ok().map(|im| im.to_rgba8())
    } else {
        None
    };
    if let Some(rgba) = loaded {
        let (w, h) = rgba.dimensions();
        for y in 0..8u32 {
            for x in 0..8u32 {
                let base = if 8 + x < w && 8 + y < h {
                    rgba.get_pixel(8 + x, 8 + y).0
                } else {
                    [180, 140, 110, 255]
                };
                let hat = if 40 + x < w && 8 + y < h {
                    rgba.get_pixel(40 + x, 8 + y).0
                } else {
                    [0, 0, 0, 0]
                };
                let a = hat[3] as f32 / 255.0;
                let idx = ((y * 8 + x) * 4) as usize;
                for c in 0..3 {
                    pixels[idx + c] =
                        ((hat[c] as f32 * a) + (base[c] as f32 * (1.0 - a))).round() as u8;
                }
                pixels[idx + 3] = 255;
            }
        }
    } else {
        for y in 0..8usize {
            for x in 0..8usize {
                let idx = (y * 8 + x) * 4;
                let col = if y < 2 {
                    [21, 128, 61, 255]
                } else if (y == 4) && (x == 1 || x == 2 || x == 5 || x == 6) {
                    if x == 2 || x == 5 {
                        [22, 163, 74, 255]
                    } else {
                        [255, 255, 255, 255]
                    }
                } else if y == 6 && (2..=5).contains(&x) {
                    [160, 102, 74, 255]
                } else {
                    [235, 190, 155, 255]
                };
                pixels[idx..idx + 4].copy_from_slice(&col);
            }
        }
    }
    let img = egui::ColorImage::from_rgba_unmultiplied([8, 8], &pixels);
    let handle = ctx.load_texture(format!("avatar_{}", acc.id), img, egui::TextureOptions::NEAREST);
    let id = handle.id();
    cache.insert(acc.id.clone(), handle);
    id
}
