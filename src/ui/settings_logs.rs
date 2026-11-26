use super::*;
use crate::java::find_installed_java;
use crate::updater::{spawn_update_check, start_self_update, UpdateState, APP_VERSION};
use crate::windows_app::{install_windows_app, installed_app_exe_path};
use eframe::egui::{self, Align, Layout, RichText, Rounding};
use std::sync::atomic::Ordering;
use std::sync::Arc;

impl crate::App {
    pub(crate) fn logs_tab(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let lines: Vec<String> = self.game_logs.lock().map(|l| l.iter().cloned().collect()).unwrap_or_default();
        card(ui, |ui| {
            ui.horizontal(|ui| {
                title(ui, "Nhật ký game");
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.add(btn_soft("Xoá")).clicked() {
                        self.game_logs.lock().map(|mut l| l.clear()).ok();
                    }
                    if ui.add(btn_soft("Sao chép tất cả")).clicked() {
                        ctx.output_mut(|o| o.copied_text = lines.join("\n"));
                    }
                    if let Some(inst) = self.selected() {
                        if ui.add(btn_soft("Mở thư mục logs")).clicked() {
                            self.open_folder(self.game_dir(&inst.id).join("logs"));
                        }
                    }
                });
            });
            hint(ui, "Khi game bị lỗi, hãy sao chép nhật ký này để nhờ người khác hỗ trợ.");
            ui.add_space(8.0);
            egui::Frame::none().fill(GREEN_MIST).rounding(Rounding::same(8.0)).inner_margin(egui::Margin::same(10.0)).show(ui, |ui| {
                egui::ScrollArea::vertical().id_salt("log_scroll").max_height(440.0).stick_to_bottom(true).auto_shrink([false, false]).show(ui, |ui| {
                    if lines.is_empty() {
                        hint(ui, "Chưa có nhật ký. Bấm CHƠI NGAY để bắt đầu.");
                    }
                    for line in lines.iter().rev().take(800).rev() {
                        let color = if line.contains("ERROR") || line.contains("Exception") {
                            RED
                        } else if line.contains("WARN") {
                            AMBER_TEXT
                        } else {
                            TEXT
                        };
                        ui.label(RichText::new(line).monospace().size(11.5).color(color));
                    }
                });
            });
        });
    }

    // ---------------- SETTINGS ----------------
    pub(crate) fn settings_tab(&mut self, ui: &mut egui::Ui) {
        let busy = self.busy.load(Ordering::SeqCst);
        let runtimes = self.config.runtimes_dir();
        card(ui, |ui| {
            title(ui, "Java");
            hint(ui, "Launcher tự tải đúng bản Java khi bấm CHƠI NGAY. Bạn cũng có thể tải trước tại đây.");
            ui.add_space(8.0);
            for (major, desc) in [
                (25u32, "Minecraft 26.x trở lên"),
                (21, "Minecraft 1.20.5 – 1.21.11"),
                (17, "Minecraft 1.17 – 1.20.4"),
                (8, "Minecraft 1.16.5 trở xuống"),
            ] {
                let installed = find_installed_java(&runtimes, major).is_some();
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("Java {}", major)).strong().color(TEXT));
                    hint(ui, desc);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if installed {
                            pill(ui, "Đã có");
                        } else if ui.add_enabled(!busy, btn_soft(&format!("Tải Java {}", major))).clicked() {
                            self.download_java(major);
                        }
                    });
                });
                ui.add_space(4.0);
            }
        });
        ui.add_space(12.0);
        card(ui, |ui| {
            title(ui, "Thư mục lưu dữ liệu & chạy game");
            ui.add_space(6.0);
            hint(ui, &format!("Nơi lưu dữ liệu game hiện tại: {}", self.config.base_data_dir().display()));
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                if ui.add(btn_primary("📁 Chọn chỗ lưu dữ liệu game...")).clicked() {
                    if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                        self.config.custom_data_dir = dir.to_string_lossy().to_string();
                        self.config.portable_mode = false;
                        self.save();
                        self.needs_refresh = true;
                    }
                }
                if ui.add(btn_soft("Mở thư mục dữ liệu")).clicked() {
                    self.open_folder(self.config.base_data_dir());
                }
                if !self.config.custom_data_dir.trim().is_empty() && ui.add(btn_soft("Đặt lại mặc định")).clicked() {
                    self.config.custom_data_dir.clear();
                    self.save();
                    self.needs_refresh = true;
                }
            });
            ui.add_space(8.0);
            if ui.checkbox(&mut self.config.minimize_on_play, "Thu nhỏ launcher khi vào game").changed() {
                self.save();
            }
            if ui
                .checkbox(&mut self.config.portable_mode, "Chế độ Portable – lưu game cạnh file vn-launcher.exe (khi không chọn thư mục riêng)")
                .changed()
            {
                if self.config.portable_mode {
                    self.config.custom_data_dir.clear();
                }
                self.save();
                self.needs_refresh = true;
            }
            hint(ui, "Tài khoản và cấu hình được lưu trong %APPDATA%\\VNLauncher nên thư mục chứa file vn-launcher.exe luôn gọn sạch.");
        });
        ui.add_space(12.0);
        card(ui, |ui| {
            title(ui, "Ứng dụng Windows (Start Menu & Desktop)");
            let app_exe = installed_app_exe_path();
            hint(ui, &format!("Vị trí cài đặt App: {}", app_exe.display()));
            hint(ui, "Đã tích hợp vào Windows Start Menu, Windows Search (gõ \"VN Launcher\"), Desktop và danh sách Ứng dụng đã cài đặt.");
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                if ui.add(btn_primary("📌 Tạo lại lối tắt Start Menu & Desktop")).clicked() {
                    install_windows_app(APP_VERSION, true);
                    self.set_status("Đã cài đặt App & tạo lối tắt VN Launcher trên Start Menu và Desktop.");
                }
                if ui.add(btn_soft("Mở thư mục cài App")).clicked() {
                    if let Some(p) = app_exe.parent() {
                        self.open_folder(p.to_path_buf());
                    }
                }
            });
        });
        ui.add_space(12.0);
        card(ui, |ui| {
            title(ui, "Giới thiệu & Cập nhật");
            ui.label(RichText::new(format!("VN Launcher {} · Phát triển bởi Nguyễn Trọng Việt Anh (Vanh)", APP_VERSION)).size(13.5).strong().color(TEXT));
            hint(ui, "Mã nguồn mở (MIT) · Viết bằng Rust · Kho Mod, Resource Pack & Shader từ Modrinth.");
            ui.add_space(6.0);
            let upd = self.update_state.lock().map(|u| u.clone()).unwrap_or(UpdateState::Idle);
            ui.horizontal_wrapped(|ui| {
                match upd {
                    UpdateState::Checking => {
                        ui.spinner();
                        ui.label(RichText::new("Đang kiểm tra bản mới trên GitHub...").color(MUTED));
                    }
                    UpdateState::Downloading => {
                        ui.spinner();
                        ui.label(RichText::new("Đang tải bản cập nhật và tự cài lại...").color(GREEN_DARK));
                    }
                    UpdateState::Available { version, download_url } => {
                        if ui.add(btn_primary(&format!("⬇ Cập nhật ngay lên v{}", version))).clicked() {
                            start_self_update(download_url, Arc::clone(&self.update_state), Arc::clone(&self.status));
                        }
                        if ui.add(btn_soft("🔄 Kiểm tra lại")).clicked() {
                            spawn_update_check(Arc::clone(&self.update_state), true);
                        }
                    }
                    UpdateState::UpToDate => {
                        if ui.add(btn_soft("🔄 Kiểm tra cập nhật")).clicked() {
                            spawn_update_check(Arc::clone(&self.update_state), true);
                        }
                        pill(ui, &format!("✓ Đang dùng bản mới nhất (v{})", APP_VERSION));
                    }
                    UpdateState::Error(e) => {
                        if ui.add(btn_soft("🔄 Thử kiểm tra lại")).clicked() {
                            spawn_update_check(Arc::clone(&self.update_state), true);
                        }
                        hint(ui, &e);
                    }
                    UpdateState::Idle => {
                        if ui.add(btn_soft("🔄 Kiểm tra & Cập nhật bản mới")).clicked() {
                            spawn_update_check(Arc::clone(&self.update_state), true);
                        }
                    }
                }
            });
            ui.add_space(6.0);
            ui.hyperlink_to(RichText::new("🌐 GitHub chính chủ: github.com/vietanh-1208/vn-launcher").size(13.0).strong(), "https://github.com/vietanh-1208/vn-launcher");
        });
    }

    // ---------------- DIALOGS ----------------
}
