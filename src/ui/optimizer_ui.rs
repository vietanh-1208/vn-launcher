use super::*;
use crate::config::{max_ram_mb, total_ram_mb, InstanceConfig, ModLoader, OptimizationProfile};
use crate::optimizer::{
    recommended_ram_for_profile, resolve_profile, EffectiveProfile, HardwareInfo,
};
use eframe::egui::{self, RichText};

pub struct PerformanceActions {
    pub save: bool,
    pub optimize_gfx: bool,
    pub install_fps: bool,
}

pub fn render_performance_card(
    ui: &mut egui::Ui,
    inst: &mut InstanceConfig,
    hw: &HardwareInfo,
    busy: bool,
    has_sodium: bool,
    has_bobby: bool,
    has_dh: bool,
) -> PerformanceActions {
    let mut actions = PerformanceActions {
        save: false,
        optimize_gfx: false,
        install_fps: false,
    };

    let total_ram = if hw.ram_mb > 0 { hw.ram_mb as u32 } else { total_ram_mb() as u32 };
    let max_ram = max_ram_mb();
    let eff = resolve_profile(inst.optimization_profile, hw);
    let rec_ram = recommended_ram_for_profile(eff, hw).min(max_ram);

    card(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            title(ui, "Hiệu năng & Tối ưu phần cứng");
            pill(ui, &format!("Hồ sơ: {}", eff.label()));
        });

        if hw.ram_mb > 0 {
            let gpu_tag = if hw.has_dedicated_gpu { "Card rời" } else { "Card onboard" };
            let cpu_disp = if hw.cpu_name.trim().is_empty() {
                format!("{} luồng", hw.cpu_threads)
            } else {
                format!("{} ({} luồng)", hw.cpu_name.trim(), hw.cpu_threads)
            };
            let gpu_disp = if hw.gpu_name.trim().is_empty() {
                gpu_tag.to_string()
            } else {
                format!("{} ({})", hw.gpu_name.trim(), gpu_tag)
            };
            hint(
                ui,
                &format!(
                    "Phần cứng: {} · RAM {:.1} GB · GPU: {} · Khuyên dùng: {:.1} GB RAM",
                    cpu_disp,
                    total_ram as f32 / 1024.0,
                    gpu_disp,
                    rec_ram as f32 / 1024.0
                ),
            );
        } else {
            hint(
                ui,
                &format!(
                    "Đang quét CPU/GPU... Máy có {:.1} GB RAM. Mức khuyên dùng: {:.1} GB.",
                    total_ram as f32 / 1024.0,
                    rec_ram as f32 / 1024.0
                ),
            );
        }

        ui.add_space(8.0);
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("Chế độ tối ưu:").color(TEXT));
            for mode in [
                OptimizationProfile::Auto,
                OptimizationProfile::Low,
                OptimizationProfile::Balanced,
                OptimizationProfile::High,
            ] {
                if ui
                    .selectable_value(&mut inst.optimization_profile, mode, mode.label())
                    .changed()
                {
                    actions.save = true;
                }
            }
        });

        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new(format!(
                    "RAM cho game: {:.1} GB (tối đa {:.1} GB)",
                    inst.ram_mb as f32 / 1024.0,
                    max_ram as f32 / 1024.0
                ))
                .color(TEXT),
            );
            if ui
                .add(
                    egui::Slider::new(&mut inst.ram_mb, 1024..=max_ram)
                        .step_by(512.0)
                        .show_value(false),
                )
                .changed()
            {
                actions.save = true;
            }
            if inst.ram_mb != rec_ram && ui.add(btn_soft("Dùng mức khuyên dùng")).clicked() {
                inst.ram_mb = rec_ram;
                actions.save = true;
            }
        });

        if inst.ram_mb > rec_ram {
            notice(
                ui,
                "Cấp quá nhiều RAM khiến Windows thiếu bộ nhớ và game dễ giật (GC spike). Hãy dùng mức khuyên dùng.",
            );
        }

        if has_bobby && has_dh {
            ui.add_space(4.0);
            notice(
                ui,
                "⚠ Phát hiện cả Bobby và Distant Horizons cùng lúc! Nên chỉ giữ 1 mod mở rộng tầm nhìn để tránh xung đột render và ngốn tài nguyên kép.",
            );
        }

        if eff == EffectiveProfile::Low && (has_bobby || has_dh) {
            ui.add_space(4.0);
            notice(
                ui,
                "⚠ Máy yếu đang chạy hồ sơ Tiết kiệm: bật gói 1024 đoạn (Bobby / Distant Horizons) có thể gây tụt FPS hoặc tràn RAM.",
            );
        }

        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new("Cửa sổ game").color(TEXT));
            if ui.checkbox(&mut inst.fullscreen, "Toàn màn hình").changed() {
                actions.save = true;
            }
            if !inst.fullscreen {
                if ui
                    .add(
                        egui::DragValue::new(&mut inst.window_width)
                            .range(640..=7680)
                            .suffix(" px"),
                    )
                    .changed()
                {
                    actions.save = true;
                }
                ui.label("×");
                if ui
                    .add(
                        egui::DragValue::new(&mut inst.window_height)
                            .range(480..=4320)
                            .suffix(" px"),
                    )
                    .changed()
                {
                    actions.save = true;
                }
            }
        });

        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(!busy, btn_primary("⚡ Tự động tối ưu đồ hoạ"))
                .clicked()
            {
                actions.optimize_gfx = true;
            }
            hint(
                ui,
                "Áp dụng cấu hình theo hồ sơ phần cứng (không ghi đè giảm tầm nhìn nếu đang bật Bobby / Distant Horizons).",
            );
        });

        if inst.loader == ModLoader::Fabric && !has_sodium {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                hint(
                    ui,
                    "Muốn FPS cao hơn? Cài gói tăng FPS (Sodium, Lithium, FerriteCore...). Không bắt buộc.",
                );
                if ui.add_enabled(!busy, btn_soft("Cài gói tăng FPS")).clicked() {
                    actions.install_fps = true;
                }
            });
        }
    });

    actions
}
