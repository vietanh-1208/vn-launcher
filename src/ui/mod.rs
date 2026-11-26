pub mod optimizer_ui;

use eframe::egui::{
    self, Align2, Color32, FontData, FontDefinitions, FontFamily, FontId, Pos2, RichText,
    Rounding, Stroke, Vec2,
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
