use super::*;
use crate::auth::{self};
use crate::config::ServerEntry;
use crate::optimizer;
use eframe::egui::{self, Align, Layout, RichText};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

impl crate::App {
    pub(crate) fn worlds_tab(&mut self, ui: &mut egui::Ui) {
        let Some(inst) = self.selected() else {
            hint(ui, "Hãy tạo một phiên bản game trước.");
            return;
        };
        let gd = self.game_dir(&inst.id);
        let saves_dir = gd.join("saves");
        let backups_dir = gd.join("backups");
        let busy = self.busy.load(Ordering::SeqCst);

        card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    title(ui, &format!("Thế giới & Sao lưu — {}", inst.name));
                    hint(ui, "Sao lưu nhanh thế giới thành file .zip để tránh mất map khi cài hoặc thử mod mới.");
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.add(btn_soft("📁 Mở thư mục Backups")).clicked() {
                        self.open_folder(backups_dir.clone());
                    }
                    if ui.add(btn_soft("📂 Mở thư mục saves")).clicked() {
                        self.open_folder(saves_dir.clone());
                    }
                });
            });
        });
        ui.add_space(12.0);

        let mut worlds: Vec<(String, PathBuf, u64)> = Vec::new();
        if let Ok(entries) = fs::read_dir(&saves_dir) {
            for e in entries.flatten() {
                let p = e.path();
                if p.is_dir() && p.join("level.dat").exists() {
                    let name = e.file_name().to_string_lossy().to_string();
                    let bytes = optimizer::dir_size_bytes(&p);
                    worlds.push((name, p, bytes));
                }
            }
        }
        worlds.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));

        if worlds.is_empty() {
            card(ui, |ui| {
                hint(ui, "Chưa có thế giới nào trong phiên bản này. Vào game tạo một thế giới chơi đơn rồi quay lại đây để sao lưu 1 chạm!");
            });
            return;
        }

        let mut backup_req: Option<(String, PathBuf)> = None;
        let mut open_req: Option<PathBuf> = None;
        for (name, path, bytes) in worlds {
            let mb = bytes as f64 / (1024.0 * 1024.0);
            card(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(RichText::new(&name).size(15.5).strong().color(TEXT));
                        ui.label(RichText::new(format!("Dung lượng: {:.1} MB", mb)).size(12.0).color(MUTED));
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.add(btn_soft("📂 Mở thư mục")).clicked() {
                            open_req = Some(path.clone());
                        }
                        if ui.add_enabled(!busy, btn_primary("💾 Sao lưu (.zip)")).clicked() {
                            backup_req = Some((name.clone(), path.clone()));
                        }
                    });
                });
            });
            ui.add_space(8.0);
        }

        if let Some(p) = open_req {
            self.open_folder(p);
        }
        if let Some((name, world_path)) = backup_req {
            let ts = auth::now_secs();
            let safe_name: String = name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
            let dest_zip = backups_dir.join(format!("{}_{}.zip", safe_name, ts));
            let busy_flag = Arc::clone(&self.busy);
            let status = Arc::clone(&self.status);
            busy_flag.store(true, Ordering::SeqCst);
            if let Ok(mut s) = status.lock() {
                *s = format!("Đang nén sao lưu thế giới {}...", name);
            }
            thread::spawn(move || {
                let msg = match optimizer::backup_world_zip(&world_path, &dest_zip) {
                    Ok(_) => format!("✓ Đã sao lưu thế giới \"{}\" vào backups/{}", name, dest_zip.file_name().unwrap_or_default().to_string_lossy()),
                    Err(e) => format!("Lỗi sao lưu {}: {}", name, e),
                };
                if let Ok(mut s) = status.lock() {
                    *s = msg;
                }
                busy_flag.store(false, Ordering::SeqCst);
            });
        }
    }

    // ---------------- SERVERS ----------------
    pub(crate) fn servers_tab(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        card(ui, |ui| {
            ui.horizontal(|ui| {
                title(ui, "Thêm máy chủ");
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.add(btn_soft("🔄 Kiểm tra Online / Ping")).clicked() {
                        let list = self.config.saved_servers.clone();
                        let pings = Arc::clone(&self.server_pings);
                        thread::spawn(move || {
                            use std::net::{TcpStream, ToSocketAddrs};
                            use std::time::Instant;
                            for srv in list {
                                let addr_str = if srv.address.contains(':') {
                                    srv.address.clone()
                                } else {
                                    format!("{}:25565", srv.address)
                                };
                                let ms = addr_str
                                    .to_socket_addrs()
                                    .ok()
                                    .and_then(|mut it| it.next())
                                    .and_then(|sa| {
                                        let t0 = Instant::now();
                                        TcpStream::connect_timeout(&sa, Duration::from_secs(2))
                                            .ok()
                                            .map(|_| t0.elapsed().as_millis())
                                    });
                                if let Ok(mut map) = pings.lock() {
                                    map.insert(srv.address, ms);
                                }
                            }
                        });
                    }
                });
            });
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.new_server_name).desired_width(180.0).hint_text("Tên gợi nhớ"));
                ui.add(egui::TextEdit::singleline(&mut self.new_server_addr).desired_width(240.0).hint_text("Địa chỉ IP, vd: play.server.vn"));
                if ui.add(btn_primary("Lưu")).clicked() && !self.new_server_addr.trim().is_empty() {
                    let addr = self.new_server_addr.trim().to_string();
                    let name = if self.new_server_name.trim().is_empty() { addr.clone() } else { self.new_server_name.trim().to_string() };
                    self.config.saved_servers.push(ServerEntry { name, address: addr });
                    self.new_server_name.clear();
                    self.new_server_addr.clear();
                    self.save();
                }
            });
            ui.add_space(4.0);
            hint(ui, "Tài khoản Offline chỉ vào được server cho phép chơi không bản quyền (online-mode=false). Tài khoản Microsoft vào được mọi server.");
        });
        ui.add_space(12.0);

        let running = self.game_running.load(Ordering::SeqCst) || self.busy.load(Ordering::SeqCst);
        let ping_map = self.server_pings.lock().map(|m| m.clone()).unwrap_or_default();
        let mut join: Option<String> = None;
        let mut remove: Option<usize> = None;
        if self.config.saved_servers.is_empty() {
            hint(ui, "Chưa lưu máy chủ nào.");
        }
        for (i, s) in self.config.saved_servers.iter().enumerate() {
            card(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(&s.name).size(15.0).strong().color(TEXT));
                            if let Some(ping_opt) = ping_map.get(&s.address) {
                                match ping_opt {
                                    Some(ms) => pill(ui, &format!("🟢 Online · {} ms", ms)),
                                    None => {
                                        ui.label(RichText::new("🔴 Offline / Không phản hồi").size(12.0).color(RED));
                                    }
                                }
                            }
                        });
                        ui.label(RichText::new(&s.address).size(12.5).color(GREEN_DARK));
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.add(btn_danger("Xoá")).clicked() {
                            remove = Some(i);
                        }
                        if ui.add_enabled(!running, btn_primary("Vào server")).clicked() {
                            join = Some(s.address.clone());
                        }
                    });
                });
            });
            ui.add_space(8.0);
        }
        if let Some(i) = remove {
            self.config.saved_servers.remove(i);
            self.save();
        }
        if let Some(addr) = join {
            self.launch(ctx, Some(addr));
        }
    }

    // ---------------- ACCOUNTS ----------------
}
