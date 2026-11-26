use super::*;
use crate::config::ModLoader;
use crate::modrinth::{self, ContentKind, InstalledItem, DH_SLUG, FPS_PACK, IRIS_SLUG};
use crate::{fmt_count, ContentView};
use eframe::egui::{self, Align, Layout, RichText, Rounding, Stroke, Vec2};
use std::fs;
use std::sync::atomic::Ordering;

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
}
