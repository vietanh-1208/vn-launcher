use super::*;
use crate::auth::{self, Account, AccountKind};
use crate::config::{InstanceConfig, ModLoader, ServerEntry};
use crate::java::find_installed_java;
use crate::modrinth::{self, ContentKind, InstalledItem, DH_SLUG, FPS_PACK, IRIS_SLUG};
use crate::optimizer;
use crate::updater::{spawn_update_check, start_self_update, UpdateState, APP_VERSION};
use crate::windows_app::{install_windows_app, installed_app_exe_path};
use crate::{fmt_count, remove_dir_retry, ContentView, LoginState, Tab};
use eframe::egui::{self, Align, Align2, Layout, RichText, Rounding, Stroke, Vec2};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

impl crate::App {
    pub(crate) fn content_tab(&mut self, ui: &mut egui::Ui) {
        let Some(inst) = self.selected() else {
            hint(ui, "Hãy tạo một phiên bản game trước.");
            return;
        };
        let gd = self.game_dir(&inst.id);
        let busy = self.busy.load(Ordering::SeqCst);

        card(ui, |ui| {
            ui.horizontal(|ui| {
                for kind in [ContentKind::Mod, ContentKind::ResourcePack, ContentKind::Shader, ContentKind::Modpack] {
                    let sel = self.content_kind == kind;
                    let b = egui::Button::new(RichText::new(kind.label()).strong().color(if sel { WHITE } else { GREEN_DARK }))
                        .fill(if sel { GREEN } else { GREEN_SOFT })
                        .stroke(Stroke::NONE)
                        .rounding(Rounding::same(8.0))
                        .min_size(Vec2::new(110.0, 34.0));
                    if ui.add(b).clicked() && !sel {
                        self.content_kind = kind;
                        if kind == ContentKind::Modpack {
                            self.content_view = ContentView::Browse;
                        }
                        self.needs_refresh = true;
                        self.search_results.lock().map(|mut r| r.clear()).ok();
                        self.search();
                    }
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.add(btn_soft("Mở thư mục")).clicked() {
                        self.open_folder(gd.join(self.content_kind.folder()));
                    }
                    if ui.add(btn_soft("+ Thêm file từ máy")).clicked() {
                        let ext = self.content_kind.file_ext();
                        if self.content_kind == ContentKind::Modpack {
                            if let Some(file) = rfd::FileDialog::new().add_filter("Modrinth Modpack", &["mrpack"]).pick_file() {
                                self.install_local_mrpack(file);
                            }
                        } else if let Some(files) = rfd::FileDialog::new().add_filter(ext, &[ext]).pick_files() {
                            let dir = gd.join(self.content_kind.folder());
                            let _ = fs::create_dir_all(&dir);
                            for f in files {
                                if let Some(n) = f.file_name() {
                                    let _ = fs::copy(&f, dir.join(n));
                                }
                            }
                            self.needs_refresh = true;
                            self.content_view = ContentView::Installed;
                        }
                    }
                });
            });
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                for (view, label) in [
                    (ContentView::Installed, format!("Đã cài trong máy ({})", self.installed.len())),
                    (ContentView::Browse, "Tìm thêm trên Modrinth".to_string()),
                ] {
                    if ui.selectable_label(self.content_view == view, RichText::new(label).size(14.0).strong()).clicked() {
                        self.content_view = view;
                        self.needs_refresh = true;
                        if view == ContentView::Browse && self.search_results.lock().map(|r| r.is_empty()).unwrap_or(true) {
                            self.search();
                        }
                    }
                }
            });
        });
        ui.add_space(12.0);

        if self.content_kind == ContentKind::Mod && inst.loader == ModLoader::Vanilla {
            notice(ui, "Phiên bản này đang là Vanilla nên không chạy được mod. Vào Trang chủ → Bộ nạp mod → chọn Fabric.");
            ui.add_space(10.0);
        }
        if self.content_kind == ContentKind::Shader && !modrinth::has_mod(&gd, "iris") {
            card(ui, |ui| {
                ui.horizontal(|ui| {
                    hint(ui, "Shader cần mod Iris (kèm Sodium) để chạy.");
                    if ui.add_enabled(!busy && inst.loader == ModLoader::Fabric, btn_primary("Cài Iris")).clicked() {
                        self.install_content(ContentKind::Mod, IRIS_SLUG.into(), "Iris Shaders".into());
                    }
                });
            });
            ui.add_space(10.0);
        }
        if self.content_kind == ContentKind::ResourcePack || self.content_kind == ContentKind::Shader {
            let installed_snapshot = self.installed.clone();
            let list = if self.content_kind == ContentKind::ResourcePack {
                self.famous_packs.lock().map(|g| g.clone()).unwrap_or_default()
            } else {
                self.famous_shaders.lock().map(|g| g.clone()).unwrap_or_default()
            };
            let (card_title, card_hint, id_salt) = if self.content_kind == ContentKind::ResourcePack {
                (
                    "🎨 Gói Tài Nguyên (Resource Pack) Nổi Tiếng",
                    "Tổng hợp các Resource Pack nổi tiếng nhất (Fresh Animations, Faithful 32x/64x, Bare Bones, Dark Mode...) — bấm + Cài là dùng ngay.",
                    "famous_packs_list",
                )
            } else {
                (
                    "✨ Shader Đẹp & Nhẹ Nổi Tiếng",
                    "Từ MakeUp Ultra Fast (siêu nhẹ cho máy yếu) tới Complementary Reimagined / Unbound / BSL / Photon — tự động tương thích Iris.",
                    "famous_shaders_list",
                )
            };
            let have_count = list
                .iter()
                .filter(|item| installed_snapshot.iter().any(|i| modrinth::matches_mod_slug(&i.name, &item.slug)))
                .count();
            let mut pick_install: Option<(String, String)> = None;
            card(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(card_title).size(16.0).strong().color(TEXT));
                    pill(ui, &format!("Đã cài {}/{}", have_count, list.len()));
                });
                ui.add_space(4.0);
                hint(ui, card_hint);
                ui.add_space(6.0);
                egui::CollapsingHeader::new(
                    RichText::new(format!("Danh sách {} lựa chọn nổi tiếng (bấm + Cài)", list.len()))
                        .size(13.5)
                        .strong()
                        .color(GREEN_DARK),
                )
                .id_salt(id_salt)
                .default_open(true)
                .show(ui, |ui| {
                    ui.add_space(4.0);
                    for entry in &list {
                        let already = installed_snapshot
                            .iter()
                            .any(|i| modrinth::matches_mod_slug(&i.name, &entry.slug));
                        egui::Frame::none()
                            .fill(GREEN_MIST)
                            .stroke(Stroke::new(1.0_f32, BORDER))
                            .rounding(Rounding::same(8.0))
                            .inner_margin(egui::Margin::symmetric(10.0, 6.0))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.vertical(|ui| {
                                        ui.label(RichText::new(&entry.name).size(13.5).strong().color(TEXT));
                                        ui.label(RichText::new(&entry.desc).size(12.0).color(MUTED));
                                    });
                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        if already {
                                            pill(ui, "✓ Đã cài");
                                        } else if ui.add_enabled(!busy, btn_primary("+ Cài")).clicked() {
                                            pick_install = Some((entry.slug.clone(), entry.name.clone()));
                                        }
                                    });
                                });
                            });
                        ui.add_space(4.0);
                    }
                });
            });
            ui.add_space(10.0);
            if let Some((slug, title)) = pick_install {
                self.install_content(self.content_kind, slug, title);
            }
        }
        if self.content_kind == ContentKind::Mod && inst.loader == ModLoader::Fabric {
            let installed_snapshot = self.installed.clone();
            let have_count = FPS_PACK
                .iter()
                .filter(|(slug, _)| installed_snapshot.iter().any(|i| modrinth::matches_mod_slug(&i.name, slug)))
                .count();
            let mut single_install: Option<(String, String)> = None;
            let mut toggle_item: Option<InstalledItem> = None;
            let mut pack_install = false;
            let mut dh_install = false;

            card(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("⚡ Bộ Mod Tối Ưu Chuẩn Java (kèm Mic)").size(16.0).strong().color(TEXT));
                    pill(ui, &format!("Đã có {}/{} mod", have_count, FPS_PACK.len()));
                });
                ui.add_space(6.0);
                ui.horizontal_wrapped(|ui| {
                    let btn_label = if have_count == FPS_PACK.len() {
                        format!("✓ Đã cài đủ {} Mod Tối Ưu (Bấm để kiểm tra lại)", FPS_PACK.len())
                    } else {
                        format!("⚡ Cài trọn bộ {} Mod Tối Ưu + Mic (1-Click)", FPS_PACK.len())
                    };
                    if ui.add_enabled(!busy, btn_primary(&btn_label)).clicked() {
                        pack_install = true;
                    }
                    if ui.add_enabled(!busy, btn_soft("🌄 Cài Distant Horizons (1024 chunk)")).clicked() {
                        dh_install = true;
                    }
                });
                ui.add_space(4.0);
                hint(
                    ui,
                    &format!(
                        "Bộ mod tối ưu FPS, RAM, giảm lag & tiện ích từ bản Java thường (Sodium, Lithium, FerriteCore, ModernFix, BadOptimizations, Iris, AppleSkin, Inventory HUD+...) tương thích chuẩn cho Minecraft {}.",
                        inst.mc_version
                    ),
                );
                ui.add_space(6.0);
                egui::CollapsingHeader::new(
                    RichText::new(format!(
                        "Danh sách {} Mod Tối Ưu Chuẩn Java (bật/tắt hoặc cài lẻ từng mod)",
                        FPS_PACK.len()
                    ))
                    .size(13.5)
                    .strong()
                    .color(GREEN_DARK),
                )
                .id_salt("java_opt_pack_list")
                .default_open(true)
                .show(ui, |ui| {
                    ui.add_space(4.0);
                    for (slug, full_label) in FPS_PACK {
                        let (mod_title, mod_desc) = full_label
                            .split_once(" – ")
                            .unwrap_or((full_label, "Mod tối ưu cho Minecraft Java"));
                        let found = installed_snapshot
                            .iter()
                            .find(|i| modrinth::matches_mod_slug(&i.name, slug))
                            .cloned();
                        egui::Frame::none()
                            .fill(GREEN_MIST)
                            .stroke(Stroke::new(1.0_f32, BORDER))
                            .rounding(Rounding::same(8.0))
                            .inner_margin(egui::Margin::symmetric(12.0, 7.0))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.vertical(|ui| {
                                        ui.set_max_width(ui.available_width() - 160.0);
                                        ui.horizontal(|ui| {
                                            ui.label(RichText::new(mod_title).size(13.5).strong().color(TEXT));
                                            if let Some(ref item) = found {
                                                let st = if item.enabled { "✓ Đang bật" } else { "Đã tắt" };
                                                let col = if item.enabled { GREEN_DARK } else { MUTED };
                                                ui.label(RichText::new(format!("({} · {})", st, item.name)).size(11.5).color(col));
                                            }
                                        });
                                        ui.label(RichText::new(mod_desc).size(12.0).color(MUTED));
                                    });
                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        if let Some(item) = found {
                                            if ui.add(btn_soft(if item.enabled { "Tắt" } else { "Bật" })).clicked() {
                                                toggle_item = Some(item);
                                            }
                                        } else if ui.add_enabled(!busy, btn_primary("+ Cài")).clicked() {
                                            single_install = Some(((*slug).to_string(), mod_title.to_string()));
                                        }
                                    });
                                });
                            });
                        ui.add_space(4.0);
                    }
                });
            });
            ui.add_space(10.0);

            let famous_list = self.famous_mods.lock().map(|g| g.clone()).unwrap_or_default();
            let famous_have = famous_list
                .iter()
                .filter(|fm| installed_snapshot.iter().any(|i| modrinth::matches_mod_slug(&i.name, &fm.slug)))
                .count();
            card(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new("⭐ Mod Nổi Tiếng Được Yêu Thích").size(16.0).strong().color(TEXT));
                    pill(ui, &format!("Đã cài {}/{} mod", famous_have, famous_list.len()));
                });
                ui.add_space(4.0);
                hint(
                    ui,
                    &format!(
                        "Tổng hợp các mod nổi tiếng nhất (JEI, Xaero's Minimap, World Map, Jade, Zoomify, Continuity, 3D Skin, Sound Physics...) — tự đồng bộ danh sách mới từ GitHub & chạy chuẩn trên Minecraft {}.",
                        inst.mc_version
                    ),
                );
                ui.add_space(6.0);
                egui::CollapsingHeader::new(
                    RichText::new(format!(
                        "Danh sách {} Mod Nổi Tiếng (bấm + Cài hoặc bật/tắt)",
                        famous_list.len()
                    ))
                    .size(13.5)
                    .strong()
                    .color(GREEN_DARK),
                )
                .id_salt("famous_mods_list")
                .default_open(false)
                .show(ui, |ui| {
                    ui.add_space(4.0);
                    for fm in &famous_list {
                        let found = installed_snapshot
                            .iter()
                            .find(|i| modrinth::matches_mod_slug(&i.name, &fm.slug))
                            .cloned();
                        egui::Frame::none()
                            .fill(GREEN_MIST)
                            .stroke(Stroke::new(1.0_f32, BORDER))
                            .rounding(Rounding::same(8.0))
                            .inner_margin(egui::Margin::symmetric(10.0, 6.0))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.vertical(|ui| {
                                        ui.label(RichText::new(&fm.name).size(13.5).strong().color(TEXT));
                                        ui.label(RichText::new(&fm.desc).size(12.0).color(MUTED));
                                    });
                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        if let Some(item) = found {
                                            if item.enabled {
                                                pill(ui, "Đang bật");
                                            } else {
                                                pill(ui, "Đang tắt");
                                            }
                                            if ui.add(btn_soft(if item.enabled { "Tắt" } else { "Bật" })).clicked() {
                                                toggle_item = Some(item);
                                            }
                                        } else if ui.add_enabled(!busy, btn_primary("+ Cài")).clicked() {
                                            single_install = Some((fm.slug.clone(), fm.name.clone()));
                                        }
                                    });
                                });
                            });
                        ui.add_space(4.0);
                    }
                });
            });
            ui.add_space(10.0);

            if pack_install {
                self.install_pack(FPS_PACK, "Bộ 16 Mod Tối Ưu Chuẩn Java");
            }
            if dh_install {
                self.install_content(ContentKind::Mod, DH_SLUG.into(), "Distant Horizons".into());
            }
            if let Some((slug, title)) = single_install {
                self.install_content(ContentKind::Mod, slug, title);
            }
            if let Some(item) = toggle_item {
                let _ = modrinth::toggle_mod(&item);
                self.needs_refresh = true;
            }
        }

        match self.content_view {
            ContentView::Browse => {
                card(ui, |ui| {
                    ui.horizontal(|ui| {
                        let edit = ui.add(
                            egui::TextEdit::singleline(&mut self.search_query)
                                .desired_width(ui.available_width() - 120.0)
                                .hint_text(format!("Tìm {} cho Minecraft {}...", self.content_kind.label().to_lowercase(), inst.mc_version)),
                        );
                        let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                        if ui.add(btn_primary("Tìm kiếm")).clicked() || enter {
                            self.search();
                        }
                    });
                });
                ui.add_space(10.0);
                if self.searching.load(Ordering::SeqCst) {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        hint(ui, "Đang tìm trên Modrinth...");
                    });
                }
                let hits = self.search_results.lock().map(|r| r.clone()).unwrap_or_default();
                let mut install: Option<(String, String)> = None;
                for hit in hits {
                    let already = self.installed.iter().any(|i| i.name.to_lowercase().replace('-', "").contains(&hit.slug.to_lowercase().replace('-', "")));
                    card(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.vertical(|ui| {
                                ui.set_max_width(ui.available_width() - 130.0);
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(&hit.title).size(15.0).strong().color(TEXT));
                                    ui.label(RichText::new(format!("bởi {} · {} lượt tải", hit.author, fmt_count(hit.downloads))).size(12.0).color(MUTED));
                                });
                                ui.label(RichText::new(&hit.description).size(12.5).color(MUTED));
                            });
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if already {
                                    pill(ui, "Đã cài");
                                } else if ui.add_enabled(!busy, btn_primary("Cài đặt")).clicked() {
                                    install = Some((hit.slug.clone(), hit.title.clone()));
                                }
                            });
                        });
                    });
                    ui.add_space(8.0);
                }
                if let Some((slug, t)) = install {
                    self.install_content(self.content_kind, slug, t);
                }
            }
            ContentView::Installed => {
                if self.installed.is_empty() {
                    card(ui, |ui| {
                        title(ui, "Chưa có gì ở đây");
                        hint(ui, "Tìm & tải ở mục bên cạnh, hoặc kéo-thả file vào cửa sổ launcher.");
                    });
                }
                let items = self.installed.clone();
                for item in items {
                    let friendly = if self.content_kind == ContentKind::Mod {
                        modrinth::describe_mod_file(&item.name)
                    } else {
                        None
                    };
                    card(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.vertical(|ui| {
                                ui.set_max_width(ui.available_width() - 140.0);
                                if let Some(desc) = friendly {
                                    ui.label(RichText::new(desc).size(14.0).strong().color(if item.enabled { TEXT } else { MUTED }));
                                } else {
                                    ui.label(RichText::new(&item.name).size(14.0).strong().color(if item.enabled { TEXT } else { MUTED }));
                                }
                                let info = if item.is_dir { "Thư mục".to_string() } else { format!("{} KB", item.size_kb) };
                                let state = if item.enabled { "Đang bật" } else { "Đã tắt" };
                                ui.label(
                                    RichText::new(format!("{} · {} · File: {}", state, info, item.name))
                                        .size(12.0)
                                        .color(if item.enabled { GREEN_DARK } else { MUTED }),
                                );
                            });
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if ui.add(btn_danger("Xoá")).clicked() {
                                    let _ = modrinth::delete_item(&item);
                                    self.needs_refresh = true;
                                }
                                if self.content_kind == ContentKind::Mod && ui.add(btn_soft(if item.enabled { "Tắt" } else { "Bật" })).clicked() {
                                    let _ = modrinth::toggle_mod(&item);
                                    self.needs_refresh = true;
                                }
                            });
                        });
                    });
                    ui.add_space(6.0);
                }
            }
        }
    }

    // ---------------- WORLDS & BACKUP ----------------
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
                                let _ = Command::new("rundll32").args(["url.dll,FileProtocolHandler", &uri]).spawn();
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

