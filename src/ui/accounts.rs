use super::*;
use crate::auth::{self, Account, AccountKind};
use crate::LoginState;
use eframe::egui::{self, Align, Layout, RichText, Rounding, Stroke, Vec2};
use std::fs;
use std::path::Path;

impl crate::App {
    pub(crate) fn accounts_tab(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        card(ui, |ui| {
            title(ui, "Tài khoản của bạn");
            ui.add_space(8.0);
            let mut make_active: Option<String> = None;
            let mut remove: Option<String> = None;
            let mut pick_skin_for: Option<(String, String)> = None;
            let mut clear_skin_for: Option<String> = None;
            if self.accounts.accounts.is_empty() {
                hint(ui, "Chưa có tài khoản nào.");
            }
            for a in &self.accounts.accounts {
                let active = self.accounts.active_id.as_deref() == Some(a.id.as_str());
                let has_skin = a.kind == AccountKind::Offline && !a.offline_skin.trim().is_empty() && Path::new(&a.offline_skin).exists();
                let tex_id = ensure_avatar_texture(&mut self.avatar_cache, ctx, a);
                egui::Frame::none()
                    .fill(if active { GREEN_MIST } else { WHITE })
                    .rounding(Rounding::same(10.0))
                    .inner_margin(egui::Margin::symmetric(12.0, 10.0))
                    .stroke(Stroke::new(1.0_f32, if active { GREEN } else { BORDER }))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            ui.image((tex_id, Vec2::splat(32.0)));
                            ui.add_space(6.0);
                            ui.vertical(|ui| {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(&a.username).size(15.0).strong().color(TEXT));
                                    if has_skin {
                                        pill(ui, "✓ Có Skin riêng");
                                    }
                                });
                                ui.label(RichText::new(a.kind_label()).size(12.0).color(if a.kind == AccountKind::Microsoft { GREEN_DARK } else { MUTED }));
                            });
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if ui.add(btn_danger("Xoá")).clicked() {
                                    remove = Some(a.id.clone());
                                }
                                if a.kind == AccountKind::Offline {
                                    if has_skin && ui.add(btn_soft("Gỡ Skin")).clicked() {
                                        clear_skin_for = Some(a.id.clone());
                                    }
                                    if ui.add(btn_soft("🎨 Đổi Skin (.png)")).clicked() {
                                        pick_skin_for = Some((a.id.clone(), a.username.clone()));
                                    }
                                }
                                if active {
                                    pill(ui, "Đang dùng");
                                } else if ui.add(btn_soft("Dùng tài khoản này")).clicked() {
                                    make_active = Some(a.id.clone());
                                }
                            });
                        });
                    });
                ui.add_space(6.0);
            }
            if let Some((acc_id, uname)) = pick_skin_for {
                if let Some(src) = rfd::FileDialog::new().add_filter("Ảnh Skin Minecraft", &["png"]).pick_file() {
                    let dest = auth::skins_dir().join(format!("{}.png", uname));
                    if fs::copy(&src, &dest).is_ok() {
                        if let Some(acc) = self.accounts.accounts.iter_mut().find(|x| x.id == acc_id) {
                            acc.offline_skin = dest.to_string_lossy().to_string();
                            self.avatar_cache.remove(&acc.id);
                        }
                        let _ = self.accounts.save();
                        self.set_status(format!("Đã gắn Skin riêng cho {} (tự động nạp khi mở Fabric)!", uname));
                    }
                }
            }
            if let Some(acc_id) = clear_skin_for {
                if let Some(acc) = self.accounts.accounts.iter_mut().find(|x| x.id == acc_id) {
                    acc.offline_skin.clear();
                    self.avatar_cache.remove(&acc.id);
                }
                let _ = self.accounts.save();
                self.set_status("Đã gỡ Skin riêng.");
            }
            if let Some(id) = make_active {
                self.accounts.active_id = Some(id);
                let _ = self.accounts.save();
            }
            if let Some(id) = remove {
                self.accounts.remove(&id);
                let _ = self.accounts.save();
            }
        });
        ui.add_space(12.0);

        card(ui, |ui| {
            title(ui, "Thêm tài khoản Offline (không cần mua game)");
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.new_offline_name).desired_width(220.0).hint_text("Tên nhân vật (3–16 ký tự)"));
                let name = self.new_offline_name.trim().to_string();
                let valid = (3..=16).contains(&name.chars().count()) && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
                if ui.add_enabled(valid, btn_primary("Thêm")).clicked() {
                    self.accounts.upsert(Account::offline(&name), true);
                    let _ = self.accounts.save();
                    self.new_offline_name.clear();
                }
            });
            hint(ui, "Tên chỉ gồm chữ không dấu, số và dấu _ (giống quy định của Minecraft).");
        });
        ui.add_space(12.0);

        card(ui, |ui| {
            title(ui, "Đăng nhập Microsoft (tài khoản Premium)");
            hint(ui, "Dùng luồng đăng nhập chính thức của Microsoft. Launcher không bao giờ thấy mật khẩu của bạn.");
            ui.add_space(8.0);

            if self.config.ms_client_id.trim().is_empty() {
                notice(
                    ui,
                    "Microsoft yêu cầu mỗi launcher có một \"Azure Client ID\" riêng và được Mojang duyệt. Cách làm:\n\
                     1. Vào portal.azure.com → App registrations → New registration (chọn \"Personal Microsoft accounts\").\n\
                     2. Mục Authentication → bật \"Allow public client flows\".\n\
                     3. Gửi Client ID cho Mojang duyệt tại aka.ms/mce-reviewappid (có thể mất vài tuần).\n\
                     4. Dán Client ID vào ô bên dưới.",
                );
                ui.add_space(8.0);
            }
            ui.horizontal(|ui| {
                ui.label(RichText::new("Azure Client ID").color(MUTED));
                ui.add(egui::TextEdit::singleline(&mut self.client_id_input).desired_width(320.0).hint_text("xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx"));
                if ui.add(btn_soft("Lưu")).clicked() {
                    self.config.ms_client_id = self.client_id_input.trim().to_string();
                    self.save();
                }
            });
            ui.add_space(10.0);

            let state = self.login_state.lock().map(|s| s.clone()).unwrap_or(LoginState::Idle);
            let can_login = !self.config.ms_client_id.trim().is_empty() && matches!(state, LoginState::Idle | LoginState::Error(_) | LoginState::Done(_));
            if ui.add_enabled(can_login, btn_primary("Đăng nhập bằng Microsoft")).clicked() {
                self.start_microsoft_login();
            }
            ui.add_space(8.0);
            match state {
                LoginState::Idle => {}
                LoginState::Requesting => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        hint(ui, "Đang kết nối Microsoft...");
                    });
                }
                LoginState::ShowCode { user_code, uri } => {
                    egui::Frame::none().fill(GREEN_MIST).rounding(Rounding::same(10.0)).inner_margin(egui::Margin::same(14.0)).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.label(RichText::new("Trình duyệt đã mở trang đăng nhập Microsoft. Nếu được hỏi mã, hãy nhập:").color(TEXT));
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(&user_code).size(26.0).strong().monospace().color(GREEN_DEEP));
                            if ui.add(btn_soft("Sao chép mã")).clicked() {
                                ctx.output_mut(|o| o.copied_text = user_code.clone());
                            }
                            if ui.add(btn_soft("Mở lại trang")).clicked() {
                                open_url_in_browser(&uri);
                            }
                        });
                        ui.horizontal(|ui| {
                            ui.spinner();
                            hint(ui, "Đang chờ bạn đăng nhập trên trình duyệt...");
                        });
                    });
                }
                LoginState::Error(e) => {
                    ui.label(RichText::new(e).color(RED));
                }
                LoginState::Done(name) => {
                    ui.label(RichText::new(format!("Đăng nhập thành công: {}", name)).strong().color(GREEN_DARK));
                }
            }
        });
    }

    // ---------------- LOGS ----------------
}
