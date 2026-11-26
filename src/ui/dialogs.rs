use super::*;
use crate::config::{InstanceConfig, ModLoader};
use crate::modrinth::FPS_PACK;
use crate::optimizer;
use crate::{remove_dir_retry, Tab};
use eframe::egui::{self, Align2, RichText, Rounding, Vec2};
use std::sync::atomic::Ordering;

impl crate::App {
    pub(crate) fn create_dialog(&mut self, ctx: &egui::Context) {
        if !self.show_create {
            return;
        }
        let versions = self.version_list();
        let mut open = true;
        let mut create = false;
        let mut create_with_mods = false;
        egui::Window::new(RichText::new("Tạo phiên bản mới").strong().color(TEXT))
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
            .open(&mut open)
            .frame(egui::Frame::window(&ctx.style()).fill(WHITE).rounding(Rounding::same(14.0)).inner_margin(egui::Margin::same(20.0)))
            .show(ctx, |ui| {
                egui::Grid::new("create_grid").num_columns(2).spacing([16.0, 12.0]).show(ui, |ui| {
                    ui.label(RichText::new("Tên").color(MUTED));
                    ui.add(egui::TextEdit::singleline(&mut self.new_name).desired_width(240.0).hint_text(format!("Minecraft {}", self.new_version)));
                    ui.end_row();
                    ui.label(RichText::new("Phiên bản").color(MUTED));
                    ui.horizontal(|ui| {
                        egui::ComboBox::from_id_salt("new_ver").selected_text(&self.new_version).width(130.0).height(320.0).show_ui(ui, |ui| {
                            for v in &versions {
                                ui.selectable_value(&mut self.new_version, v.clone(), v);
                            }
                        });
                        if ui.checkbox(&mut self.config.show_snapshots, "Snapshot").changed() {
                            let _ = self.config.save();
                        }
                    });
                    ui.end_row();
                    ui.label(RichText::new("Bộ nạp mod").color(MUTED));
                    ui.horizontal(|ui| {
                        ui.selectable_value(&mut self.new_loader, ModLoader::Fabric, "Fabric");
                        if ui.selectable_value(&mut self.new_loader, ModLoader::NeoForge, "NeoForge").clicked() {
                            self.new_install_fps = false;
                        }
                        if ui.selectable_value(&mut self.new_loader, ModLoader::Forge, "Forge").clicked() {
                            self.new_install_fps = false;
                        }
                        if ui.selectable_value(&mut self.new_loader, ModLoader::Vanilla, "Vanilla").clicked() {
                            self.new_install_fps = false;
                        }
                    });
                    ui.end_row();
                    ui.label(RichText::new("Mod tối ưu").color(MUTED));
                    let opt_btn = if self.new_install_fps {
                        btn_primary(&format!("✓ Đã bật: Tự cài Mod Tối Ưu cho bản {}", self.new_version))
                    } else {
                        btn_soft(&format!("⚡ Nhấn để Tự cài Mod Tối Ưu cho bản {}", self.new_version))
                    };
                    if ui.add(opt_btn).clicked() {
                        self.new_install_fps = !self.new_install_fps;
                        if self.new_install_fps {
                            self.new_loader = ModLoader::Fabric;
                        }
                    }
                    ui.end_row();
                });
                ui.add_space(6.0);
                if self.new_install_fps {
                    hint(
                        ui,
                        &format!(
                            "Đã bật: Sẽ tự động tải & cài Bộ {} Mod Tối Ưu đúng cho phiên bản {} ngay khi bấm Tạo.",
                            FPS_PACK.len(),
                            self.new_version
                        ),
                    );
                } else {
                    hint(ui, "Chưa bật Mod tối ưu: Bấm \"Tạo phiên bản\" để tạo bản trống, hoặc bấm \"⚡ Tạo & Tải ngay Mod Tối Ưu\".");
                }
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui.add(btn_primary("+ Tạo phiên bản")).clicked() {
                        create = true;
                    }
                    if ui
                        .add(btn_soft(&format!("⚡ Tạo & Tải ngay {} Mod Tối Ưu", FPS_PACK.len())))
                        .clicked()
                    {
                        self.new_loader = ModLoader::Fabric;
                        self.new_install_fps = true;
                        create = true;
                        create_with_mods = true;
                    }
                });
            });
        if create {
            let name = if self.new_name.trim().is_empty() { format!("Minecraft {}", self.new_version) } else { self.new_name.trim().to_string() };
            let mut inst = InstanceConfig::new(&name, &self.new_version, self.new_loader);
            inst.id = self.config.unique_folder_id(&name, &self.new_version, self.new_loader, None);
            inst.lod_render_distance = 1024;
            let should_install_pack = create_with_mods || (self.new_loader == ModLoader::Fabric && self.new_install_fps);
            self.config.selected_instance_id = inst.id.clone();
            let gd = self.config.instance_dir(&inst.id).join(".minecraft");
            let _ = optimizer::prepare_game_dir(&gd);
            self.config.instances.push(inst);
            self.save();
            self.needs_refresh = true;
            self.tab = Tab::Home;
            open = false;
            if should_install_pack {
                self.install_pack(FPS_PACK, "Bộ 22 Mod Tối Ưu Ultra FPS");
            }
        }
        self.show_create = open;
    }

    pub(crate) fn delete_dialog(&mut self, ctx: &egui::Context) {
        let Some(id) = self.confirm_delete.clone() else { return };
        let name = self.config.instances.iter().find(|i| i.id == id).map(|i| i.name.clone()).unwrap_or_default();
        let mut close = false;
        let mut confirm = false;
        egui::Window::new(RichText::new("Xoá phiên bản?").strong().color(RED))
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
            .frame(egui::Frame::window(&ctx.style()).fill(WHITE).rounding(Rounding::same(14.0)).inner_margin(egui::Margin::same(20.0)))
            .show(ctx, |ui| {
                ui.label(RichText::new(format!("Bạn chắc chắn muốn xoá \"{}\"?", name)).color(TEXT));
                ui.add_space(6.0);
                ui.label(RichText::new("Sẽ bị XOÁ vĩnh viễn (chỉ của phiên bản này):").strong().color(RED));
                hint(ui, &format!("• Thư mục {}", self.config.instance_dir(&id).display()));
                hint(ui, "• Thế giới (saves), mod, resource pack, shader, ảnh chụp, cài đặt game của phiên bản này");
                ui.add_space(6.0);
                ui.label(RichText::new("Được GIỮ LẠI:").strong().color(GREEN));
                hint(ui, "• Dữ liệu Minecraft dùng chung (shared: thư viện, assets, bản game gốc)");
                hint(ui, "• Java đã tải (runtimes), tài khoản đăng nhập, các phiên bản khác");
                hint(ui, "Mẹo: muốn giữ thế giới, hãy sao chép thư mục saves ra ngoài trước khi xoá.");
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if ui.add(btn_soft("Huỷ")).clicked() {
                        close = true;
                    }
                    let del = egui::Button::new(RichText::new("Xoá vĩnh viễn").strong().color(WHITE)).fill(RED).rounding(Rounding::same(8.0)).min_size(Vec2::new(0.0, 30.0));
                    if ui.add(del).clicked() {
                        confirm = true;
                    }
                });
            });
        if confirm {
            if self.game_running.load(Ordering::SeqCst) || self.busy.load(Ordering::SeqCst) {
                self.set_status("Hãy đóng game (hoặc chờ tải xong) trước khi xoá phiên bản.");
            } else if id.trim().is_empty() || id.contains("..") || id.contains('/') || id.contains('\\') {
                self.set_status("Mã phiên bản không hợp lệ, không xoá.");
            } else if let Err(e) = remove_dir_retry(&self.config.instance_dir(&id)) {
                self.set_status(format!("Không xoá được thư mục (có thể đang bị mở): {}", e));
            } else {
                self.config.instances.retain(|i| i.id != id);
                if self.config.selected_instance_id == id {
                    self.config.selected_instance_id = self.config.instances.first().map(|i| i.id.clone()).unwrap_or_default();
                }
                self.save();
                self.needs_refresh = true;
                self.set_status(format!("Đã xoá phiên bản \"{}\".", name));
            }
            close = true;
        }
        if close {
            self.confirm_delete = None;
        }
    }
}
