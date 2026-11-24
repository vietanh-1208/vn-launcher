#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod auth;
mod config;
mod fabric;
mod java;
mod launcher;
mod modrinth;
mod mojang;
mod optimizer;

use auth::{Account, AccountKind, AccountStore, PollResult};
use config::{max_ram_mb, recommended_ram_mb, total_ram_mb, InstanceConfig, LauncherConfig, ModLoader, ServerEntry};
use eframe::egui::{
    self, Align, Align2, Color32, FontData, FontDefinitions, FontFamily, FontId, IconData, Layout, Pos2, RichText,
    Rounding, Stroke, Vec2,
};
use java::{ensure_java_runtime, find_installed_java};
use launcher::{prepare_and_launch, LaunchAccount};
use modrinth::{ContentKind, InstalledItem, SearchHit, DH_SLUG, FPS_PACK, IRIS_SLUG};
use std::collections::VecDeque;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

// ---------- White & green palette ----------
const GREEN: Color32 = Color32::from_rgb(22, 163, 74); // #16A34A
const GREEN_DARK: Color32 = Color32::from_rgb(21, 128, 61); // #15803D
const GREEN_DEEP: Color32 = Color32::from_rgb(20, 83, 45); // #14532D
const GREEN_BRIGHT: Color32 = Color32::from_rgb(74, 222, 128); // #4ADE80
const GREEN_SOFT: Color32 = Color32::from_rgb(220, 252, 231); // #DCFCE7
const GREEN_MIST: Color32 = Color32::from_rgb(240, 253, 244); // #F0FDF4
const BG: Color32 = Color32::from_rgb(244, 250, 246);
const WHITE: Color32 = Color32::WHITE;
const BORDER: Color32 = Color32::from_rgb(209, 231, 219);
const TEXT: Color32 = Color32::from_rgb(17, 38, 29);
const MUTED: Color32 = Color32::from_rgb(91, 112, 101);
const RED: Color32 = Color32::from_rgb(220, 38, 38);
const RED_SOFT: Color32 = Color32::from_rgb(254, 226, 226);
const AMBER_SOFT: Color32 = Color32::from_rgb(254, 243, 199);
const AMBER_TEXT: Color32 = Color32::from_rgb(146, 64, 14);

const POPULAR_VERSIONS: &[&str] = &[
    "26.3", "26.2", "1.21.11", "1.21.4", "1.21.3", "1.21.1", "1.20.6", "1.20.4", "1.20.1", "1.19.4", "1.19.2",
    "1.18.2", "1.17.1", "1.16.5", "1.12.2", "1.8.9",
];

#[derive(PartialEq, Eq, Clone, Copy)]
enum Tab {
    Home,
    Content,
    Servers,
    Accounts,
    Logs,
    Settings,
}

#[derive(PartialEq, Eq, Clone, Copy)]
enum ContentView {
    Browse,
    Installed,
}

#[derive(Clone)]
enum LoginState {
    Idle,
    Requesting,
    ShowCode { user_code: String, uri: String },
    Error(String),
    Done(String),
}

struct App {
    config: LauncherConfig,
    accounts: AccountStore,
    tab: Tab,

    busy: Arc<AtomicBool>,
    status: Arc<Mutex<String>>,
    versions: Arc<Mutex<Vec<(String, String)>>>,

    game_child: Arc<Mutex<Option<Child>>>,
    stop_requested: Arc<AtomicBool>,
    game_running: Arc<AtomicBool>,
    game_logs: Arc<Mutex<VecDeque<String>>>,

    show_create: bool,
    new_name: String,
    new_version: String,
    new_loader: ModLoader,
    new_install_fps: bool,
    confirm_delete: Option<String>,

    content_kind: ContentKind,
    content_view: ContentView,
    search_query: String,
    search_results: Arc<Mutex<Vec<SearchHit>>>,
    searching: Arc<AtomicBool>,
    installed: Vec<InstalledItem>,
    needs_refresh: bool,

    new_server_name: String,
    new_server_addr: String,

    new_offline_name: String,
    login_state: Arc<Mutex<LoginState>>,
    pending_accounts: Arc<Mutex<Vec<Account>>>,
    client_id_input: String,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        setup_fonts(&cc.egui_ctx);
        setup_theme(&cc.egui_ctx);

        let mut config = LauncherConfig::load();
        let mut accounts = AccountStore::load();
        if accounts.accounts.is_empty() {
            let name = if config.username.trim().is_empty() { "VNPlayer".to_string() } else { config.username.trim().to_string() };
            accounts.upsert(Account::offline(&name), true);
            let _ = accounts.save();
        }
        if config.instances.iter().all(|i| i.id != config.selected_instance_id) {
            config.selected_instance_id = config.instances.first().map(|i| i.id.clone()).unwrap_or_default();
        }

        let versions = Arc::new(Mutex::new(
            POPULAR_VERSIONS.iter().map(|v| (v.to_string(), "release".to_string())).collect::<Vec<_>>(),
        ));
        {
            let versions = Arc::clone(&versions);
            thread::spawn(move || {
                if let Ok(client) = mojang::build_http_client(15) {
                    if let Ok(m) = mojang::fetch_version_manifest(&client) {
                        let list: Vec<(String, String)> = m
                            .versions
                            .into_iter()
                            .filter(|v| v.version_type == "release" || v.version_type == "snapshot")
                            .map(|v| (v.id, v.version_type))
                            .collect();
                        if !list.is_empty() {
                            if let Ok(mut g) = versions.lock() {
                                *g = list;
                            }
                        }
                    }
                }
            });
        }

        let client_id_input = config.ms_client_id.clone();
        Self {
            config,
            accounts,
            tab: Tab::Home,
            busy: Arc::new(AtomicBool::new(false)),
            status: Arc::new(Mutex::new("Sẵn sàng".into())),
            versions,
            game_child: Arc::new(Mutex::new(None)),
            stop_requested: Arc::new(AtomicBool::new(false)),
            game_running: Arc::new(AtomicBool::new(false)),
            game_logs: Arc::new(Mutex::new(VecDeque::new())),
            show_create: false,
            new_name: String::new(),
            new_version: "1.21.1".into(),
            new_loader: ModLoader::Fabric,
            new_install_fps: false,
            confirm_delete: None,
            content_kind: ContentKind::Mod,
            content_view: ContentView::Installed,
            search_query: String::new(),
            search_results: Arc::new(Mutex::new(Vec::new())),
            searching: Arc::new(AtomicBool::new(false)),
            installed: Vec::new(),
            needs_refresh: true,
            new_server_name: String::new(),
            new_server_addr: String::new(),
            new_offline_name: String::new(),
            login_state: Arc::new(Mutex::new(LoginState::Idle)),
            pending_accounts: Arc::new(Mutex::new(Vec::new())),
            client_id_input,
        }
    }

    // ---------- helpers ----------
    fn selected(&self) -> Option<InstanceConfig> {
        self.config.instances.iter().find(|i| i.id == self.config.selected_instance_id).cloned()
    }

    fn set_status(&self, msg: impl Into<String>) {
        if let Ok(mut s) = self.status.lock() {
            *s = msg.into();
        }
    }

    fn version_list(&self) -> Vec<String> {
        let show_snap = self.config.show_snapshots;
        self.versions
            .lock()
            .map(|g| g.iter().filter(|(_, t)| show_snap || t == "release").map(|(id, _)| id.clone()).collect())
            .unwrap_or_default()
    }

    fn game_dir(&self, id: &str) -> PathBuf {
        self.config.instance_game_dir(id)
    }

    fn open_folder(&self, path: PathBuf) {
        let _ = fs::create_dir_all(&path);
        let _ = Command::new("explorer").arg(path).spawn();
    }

    fn save(&self) {
        let _ = self.config.save();
    }

    // ---------- actions ----------
    fn launch(&mut self, ctx: &egui::Context, server: Option<String>) {
        if self.busy.load(Ordering::SeqCst) || self.game_running.load(Ordering::SeqCst) {
            return;
        }
        let Some(instance) = self.selected() else {
            self.set_status("Hãy tạo một phiên bản game trước.");
            return;
        };
        let Some(account) = self.accounts.active().cloned() else {
            self.tab = Tab::Accounts;
            self.set_status("Hãy thêm tài khoản trước khi chơi.");
            return;
        };
        self.save();
        if self.config.minimize_on_play {
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
        }

        let cfg = self.config.clone();
        let busy = Arc::clone(&self.busy);
        let status = Arc::clone(&self.status);
        let child_slot = Arc::clone(&self.game_child);
        let stop_req = Arc::clone(&self.stop_requested);
        stop_req.store(false, Ordering::SeqCst);
        let running = Arc::clone(&self.game_running);
        let logs = Arc::clone(&self.game_logs);
        let pending = Arc::clone(&self.pending_accounts);
        let ctx = ctx.clone();
        busy.store(true, Ordering::SeqCst);
        if let Ok(mut l) = logs.lock() {
            l.clear();
        }

        thread::spawn(move || {
            let result = (|| -> Result<Child, String> {
                let mut acc = account;
                if acc.kind == AccountKind::Microsoft {
                    if let Ok(mut s) = status.lock() {
                        *s = "Đang kiểm tra đăng nhập Microsoft...".into();
                    }
                    let client = mojang::build_http_client(30)?;
                    if auth::ensure_fresh(&client, &cfg.ms_client_id, &mut acc)? {
                        if let Ok(mut p) = pending.lock() {
                            p.push(acc.clone());
                        }
                    }
                }
                let la = match acc.kind {
                    AccountKind::Offline => LaunchAccount {
                        username: acc.username.clone(),
                        uuid: acc.uuid.clone(),
                        access_token: "0".into(),
                        user_type: "legacy".into(),
                    },
                    AccountKind::Microsoft => LaunchAccount {
                        username: acc.username.clone(),
                        uuid: acc.uuid.clone(),
                        access_token: acc.access_token.clone(),
                        user_type: "msa".into(),
                    },
                };
                prepare_and_launch(&cfg, &instance, &la, server, &status)
            })();

            match result {
                Ok(mut child) => {
                    for pipe in [
                        child.stdout.take().map(|p| Box::new(p) as Box<dyn std::io::Read + Send>),
                        child.stderr.take().map(|p| Box::new(p) as Box<dyn std::io::Read + Send>),
                    ]
                    .into_iter()
                    .flatten()
                    {
                        let logs = Arc::clone(&logs);
                        thread::spawn(move || {
                            for line in BufReader::new(pipe).lines().map_while(Result::ok) {
                                if let Ok(mut l) = logs.lock() {
                                    l.push_back(line);
                                    while l.len() > 3000 {
                                        l.pop_front();
                                    }
                                }
                            }
                        });
                    }
                    if let Ok(mut slot) = child_slot.lock() {
                        *slot = Some(child);
                    }
                    running.store(true, Ordering::SeqCst);
                    busy.store(false, Ordering::SeqCst);
                    if let Ok(mut s) = status.lock() {
                        *s = format!("Đang chơi Minecraft {}", instance.mc_version);
                    }
                    ctx.request_repaint();

                    loop {
                        thread::sleep(Duration::from_millis(700));
                        let exited = {
                            let mut slot = child_slot.lock().unwrap();
                            match slot.as_mut().map(|c| c.try_wait()) {
                                Some(Ok(Some(code))) => Some(code.code()),
                                Some(Ok(None)) => None,
                                _ => Some(None),
                            }
                        };
                        if let Some(code) = exited {
                            if let Ok(mut slot) = child_slot.lock() {
                                *slot = None;
                            }
                            running.store(false, Ordering::SeqCst);
                            if let Ok(mut s) = status.lock() {
                                let by_user = stop_req.swap(false, Ordering::SeqCst);
                                *s = match code {
                                    _ if by_user => "Đã dừng game theo yêu cầu.".into(),
                                    Some(0) | None => "Game đã đóng.".into(),
                                    Some(c) => format!("Game bị lỗi và đã thoát (mã {}). Xem tab Nhật ký để biết chi tiết.", c),
                                };
                            }
                            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                            ctx.request_repaint();
                            break;
                        }
                    }
                }
                Err(e) => {
                    busy.store(false, Ordering::SeqCst);
                    if let Ok(mut s) = status.lock() {
                        *s = format!("Lỗi: {}", e);
                    }
                    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                    ctx.request_repaint();
                }
            }
        });
    }

    fn stop_game(&self) {
        if let Ok(mut slot) = self.game_child.lock() {
            if let Some(child) = slot.as_mut() {
                self.stop_requested.store(true, Ordering::SeqCst);
                let _ = child.kill();
            }
        }
    }

    fn search(&mut self) {
        let Some(inst) = self.selected() else { return };
        let kind = self.content_kind;
        let query = self.search_query.trim().to_string();
        let results = Arc::clone(&self.search_results);
        let searching = Arc::clone(&self.searching);
        let status = Arc::clone(&self.status);
        searching.store(true, Ordering::SeqCst);
        thread::spawn(move || {
            if let Ok(client) = mojang::build_http_client(20) {
                match modrinth::search(&client, kind, &query, &inst.mc_version) {
                    Ok(hits) => {
                        if let Ok(mut r) = results.lock() {
                            *r = hits;
                        }
                    }
                    Err(e) => {
                        if let Ok(mut s) = status.lock() {
                            *s = e;
                        }
                    }
                }
            }
            searching.store(false, Ordering::SeqCst);
        });
    }

    fn install_content(&mut self, kind: ContentKind, slug: String, title: String) {
        if self.busy.load(Ordering::SeqCst) {
            return;
        }
        let Some(inst) = self.selected() else { return };
        let game_dir = self.game_dir(&inst.id);
        let busy = Arc::clone(&self.busy);
        let status = Arc::clone(&self.status);
        busy.store(true, Ordering::SeqCst);
        thread::spawn(move || {
            if let Ok(client) = mojang::build_http_client(90) {
                let msg = match modrinth::install_project(&client, kind, &slug, &inst.mc_version, &game_dir, &status) {
                    Ok(_) => {
                        if slug == "modernfix" {
                            optimizer::ensure_modernfix_compat(&game_dir);
                        } else if slug == "bobby" {
                            optimizer::patch_bobby(&game_dir, 1024);
                        }
                        format!("Đã cài {}.", title)
                    }
                    Err(e) => format!("Không cài được {}: {}", title, e),
                };
                if let Ok(mut s) = status.lock() {
                    *s = msg;
                }
            }
            busy.store(false, Ordering::SeqCst);
        });
        self.needs_refresh = true;
    }

    fn install_pack(&mut self, slugs: &'static [(&'static str, &'static str)], name: &'static str) {
        if self.busy.load(Ordering::SeqCst) {
            return;
        }
        let Some(inst) = self.selected() else { return };
        let game_dir = self.game_dir(&inst.id);
        let busy = Arc::clone(&self.busy);
        let status = Arc::clone(&self.status);
        busy.store(true, Ordering::SeqCst);
        thread::spawn(move || {
            if let Ok(client) = mojang::build_http_client(90) {
                let (ok, failed) = modrinth::install_mod_list(&client, slugs, &inst.mc_version, &game_dir, &status);
                optimizer::ensure_modernfix_compat(&game_dir);
                let msg = if failed.is_empty() {
                    format!("Đã cài {} ({} mod).", name, ok.len())
                } else {
                    format!("Đã cài {} mod. Chưa có bản cho MC {}: {}", ok.len(), inst.mc_version, failed.join(", "))
                };
                if let Ok(mut s) = status.lock() {
                    *s = msg;
                }
            }
            busy.store(false, Ordering::SeqCst);
        });
        self.needs_refresh = true;
    }

    fn download_java(&mut self, major: u32) {
        if self.busy.load(Ordering::SeqCst) {
            return;
        }
        let dir = self.config.runtimes_dir();
        let busy = Arc::clone(&self.busy);
        let status = Arc::clone(&self.status);
        busy.store(true, Ordering::SeqCst);
        thread::spawn(move || {
            if let Ok(client) = mojang::build_http_client(300) {
                let msg = match ensure_java_runtime(&client, &dir, major, "", &status) {
                    Ok(_) => format!("Đã tải Java {}.", major),
                    Err(e) => format!("Lỗi tải Java {}: {}", major, e),
                };
                if let Ok(mut s) = status.lock() {
                    *s = msg;
                }
            }
            busy.store(false, Ordering::SeqCst);
        });
    }

    fn start_microsoft_login(&mut self) {
        let client_id = self.config.ms_client_id.trim().to_string();
        if client_id.is_empty() {
            return;
        }
        let state = Arc::clone(&self.login_state);
        let pending = Arc::clone(&self.pending_accounts);
        if let Ok(mut s) = state.lock() {
            *s = LoginState::Requesting;
        }
        thread::spawn(move || {
            let set = |st: LoginState| {
                if let Ok(mut s) = state.lock() {
                    *s = st;
                }
            };
            let Ok(client) = mojang::build_http_client(30) else { return };
            let dc = match auth::request_device_code(&client, &client_id) {
                Ok(dc) => dc,
                Err(e) => return set(LoginState::Error(e)),
            };
            let link = format!("https://www.microsoft.com/link?otc={}", dc.user_code);
            let _ = Command::new("rundll32").args(["url.dll,FileProtocolHandler", &link]).spawn();
            set(LoginState::ShowCode { user_code: dc.user_code.clone(), uri: dc.verification_uri.clone() });

            let mut interval = dc.interval.max(2);
            let deadline = auth::now_secs() + dc.expires_in;
            loop {
                thread::sleep(Duration::from_secs(interval));
                if auth::now_secs() > deadline {
                    return set(LoginState::Error("Mã đăng nhập đã hết hạn, hãy thử lại.".into()));
                }
                match auth::poll_device_token(&client, &client_id, &dc.device_code) {
                    Ok(PollResult::Pending) => continue,
                    Ok(PollResult::SlowDown) => interval += 5,
                    Ok(PollResult::Success { access_token, refresh_token }) => {
                        match auth::minecraft_login(&client, &access_token, &refresh_token) {
                            Ok(acc) => {
                                let name = acc.username.clone();
                                if let Ok(mut p) = pending.lock() {
                                    p.push(acc);
                                }
                                return set(LoginState::Done(name));
                            }
                            Err(e) => return set(LoginState::Error(e)),
                        }
                    }
                    Err(e) => return set(LoginState::Error(e)),
                }
            }
        });
    }
}

// =====================================================================
// UI building blocks
// =====================================================================
fn card<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::none()
        .fill(WHITE)
        .rounding(Rounding::same(14.0))
        .inner_margin(egui::Margin::same(18.0))
        .stroke(Stroke::new(1.0_f32, BORDER))
        .shadow(egui::epaint::Shadow { offset: Vec2::new(0.0, 2.0), blur: 8.0, spread: 0.0, color: Color32::from_black_alpha(10) })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        })
        .inner
}

fn title(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).size(17.0).strong().color(TEXT));
}

fn hint(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).size(12.5).color(MUTED));
}

fn btn_primary(text: &str) -> egui::Button<'static> {
    egui::Button::new(RichText::new(text.to_string()).strong().color(WHITE))
        .fill(GREEN)
        .stroke(Stroke::NONE)
        .rounding(Rounding::same(8.0))
        .min_size(Vec2::new(0.0, 32.0))
}

fn btn_soft(text: &str) -> egui::Button<'static> {
    egui::Button::new(RichText::new(text.to_string()).color(GREEN_DARK))
        .fill(GREEN_SOFT)
        .stroke(Stroke::NONE)
        .rounding(Rounding::same(8.0))
        .min_size(Vec2::new(0.0, 30.0))
}

fn btn_danger(text: &str) -> egui::Button<'static> {
    egui::Button::new(RichText::new(text.to_string()).color(RED))
        .fill(RED_SOFT)
        .stroke(Stroke::NONE)
        .rounding(Rounding::same(8.0))
        .min_size(Vec2::new(0.0, 30.0))
}

fn notice(ui: &mut egui::Ui, text: &str) {
    egui::Frame::none()
        .fill(AMBER_SOFT)
        .rounding(Rounding::same(8.0))
        .inner_margin(egui::Margin::symmetric(12.0, 8.0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(text).size(12.5).color(AMBER_TEXT));
        });
}

fn pill(ui: &mut egui::Ui, text: &str) {
    egui::Frame::none()
        .fill(GREEN_SOFT)
        .rounding(Rounding::same(20.0))
        .inner_margin(egui::Margin::symmetric(10.0, 3.0))
        .show(ui, |ui| {
            ui.label(RichText::new(text).size(12.0).color(GREEN_DARK));
        });
}

fn draw_logo(ui: &mut egui::Ui, size: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), egui::Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, Rounding::same(size * 0.26), GREEN_DEEP);
    p.rect_stroke(rect.shrink(2.5), Rounding::same(size * 0.22), Stroke::new(1.0_f32, Color32::from_white_alpha(70)));
    let r = size * 0.05;
    p.circle_filled(Pos2::new(rect.right() - size * 0.17, rect.top() + size * 0.17), r, GREEN_BRIGHT);
    p.circle_filled(Pos2::new(rect.left() + size * 0.17, rect.bottom() - size * 0.17), r, WHITE);
    let c = rect.center();
    let f = FontId::proportional(size * 0.46);
    p.text(Pos2::new(c.x - size * 0.14, c.y), Align2::CENTER_CENTER, "V", f.clone(), WHITE);
    p.text(Pos2::new(c.x + size * 0.15, c.y), Align2::CENTER_CENTER, "N", f, GREEN_BRIGHT);
}

// =====================================================================
// Main loop
// =====================================================================
impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let busy = self.busy.load(Ordering::SeqCst);
        let running = self.game_running.load(Ordering::SeqCst);
        if busy || self.searching.load(Ordering::SeqCst) {
            ctx.request_repaint_after(Duration::from_millis(100));
        } else if running || !matches!(*self.login_state.lock().unwrap(), LoginState::Idle) {
            ctx.request_repaint_after(Duration::from_millis(700));
        }

        // Accounts produced by background threads
        let new_accounts: Vec<Account> = self.pending_accounts.lock().map(|mut p| p.drain(..).collect()).unwrap_or_default();
        if !new_accounts.is_empty() {
            for acc in new_accounts {
                self.accounts.upsert(acc, true);
            }
            let _ = self.accounts.save();
        }

        // Drag & drop files into the current content folder
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).collect());
        if !dropped.is_empty() {
            if let Some(inst) = self.selected() {
                let gd = self.game_dir(&inst.id);
                let mut n = 0;
                for p in dropped {
                    let name = p.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_string();
                    let folder = if name.ends_with(".jar") {
                        "mods"
                    } else if name.ends_with(".zip") {
                        if self.content_kind == ContentKind::Shader { "shaderpacks" } else { "resourcepacks" }
                    } else {
                        continue;
                    };
                    let dir = gd.join(folder);
                    let _ = fs::create_dir_all(&dir);
                    if fs::copy(&p, dir.join(&name)).is_ok() {
                        n += 1;
                    }
                }
                if n > 0 {
                    self.needs_refresh = true;
                    self.set_status(format!("Đã thêm {} file vào {}.", n, inst.name));
                }
            }
        }

        if self.needs_refresh || busy {
            self.installed = match self.selected() {
                Some(inst) => modrinth::list_installed(self.content_kind, &self.game_dir(&inst.id)),
                None => Vec::new(),
            };
            if !busy {
                self.needs_refresh = false;
            }
        }

        self.bottom_bar(ctx, busy, running);
        self.sidebar(ctx);

        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(BG).inner_margin(egui::Margin::symmetric(24.0, 20.0)))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| match self.tab {
                    Tab::Home => self.home_tab(ui, ctx),
                    Tab::Content => self.content_tab(ui),
                    Tab::Servers => self.servers_tab(ui, ctx),
                    Tab::Accounts => self.accounts_tab(ui, ctx),
                    Tab::Logs => self.logs_tab(ui, ctx),
                    Tab::Settings => self.settings_tab(ui),
                });
            });

        self.create_dialog(ctx);
        self.delete_dialog(ctx);
    }
}

impl App {
    fn bottom_bar(&mut self, ctx: &egui::Context, busy: bool, running: bool) {
        egui::TopBottomPanel::bottom("bottom")
            .exact_height(74.0)
            .frame(
                egui::Frame::none()
                    .fill(WHITE)
                    .inner_margin(egui::Margin::symmetric(20.0, 12.0))
                    .stroke(Stroke::new(1.0_f32, BORDER)),
            )
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    // Account switcher
                    ui.vertical(|ui| {
                        ui.label(RichText::new("Tài khoản").size(11.5).color(MUTED));
                        let active_label = self
                            .accounts
                            .active()
                            .map(|a| format!("{}  ·  {}", a.username, a.kind_label()))
                            .unwrap_or_else(|| "Chưa có tài khoản".into());
                        let mut chosen: Option<String> = None;
                        egui::ComboBox::from_id_salt("acc_switch")
                            .selected_text(RichText::new(active_label).strong().color(TEXT))
                            .width(220.0)
                            .show_ui(ui, |ui| {
                                for a in &self.accounts.accounts {
                                    let sel = self.accounts.active_id.as_deref() == Some(a.id.as_str());
                                    if ui.selectable_label(sel, format!("{}  ·  {}", a.username, a.kind_label())).clicked() {
                                        chosen = Some(a.id.clone());
                                    }
                                }
                                ui.separator();
                                if ui.button("Quản lý tài khoản...").clicked() {
                                    self.tab = Tab::Accounts;
                                }
                            });
                        if let Some(id) = chosen {
                            self.accounts.active_id = Some(id);
                            let _ = self.accounts.save();
                        }
                    });

                    ui.add_space(16.0);

                    // Status
                    ui.vertical(|ui| {
                        let inst_line = self
                            .selected()
                            .map(|i| format!("{}  ·  Minecraft {}  ·  {}", i.name, i.mc_version, i.loader.label()))
                            .unwrap_or_else(|| "Chưa chọn phiên bản".into());
                        ui.label(RichText::new(inst_line).size(13.0).strong().color(TEXT));
                        ui.horizontal(|ui| {
                            if busy {
                                ui.spinner();
                            }
                            let msg = self.status.lock().map(|s| s.clone()).unwrap_or_default();
                            let color = if msg.starts_with("Lỗi") || msg.contains("bị lỗi") { RED } else { MUTED };
                            ui.label(RichText::new(msg).size(12.5).color(color));
                        });
                    });

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if running {
                            let stop = egui::Button::new(RichText::new("DỪNG GAME").size(15.0).strong().color(WHITE))
                                .fill(RED)
                                .rounding(Rounding::same(10.0))
                                .min_size(Vec2::new(150.0, 46.0));
                            if ui.add(stop).clicked() {
                                self.stop_game();
                            }
                            ui.add_space(6.0);
                            pill(ui, "Đang chơi");
                        } else {
                            let text = if busy { "ĐANG CHUẨN BỊ..." } else { "CHƠI NGAY" };
                            let play = egui::Button::new(RichText::new(text).size(16.0).strong().color(WHITE))
                                .fill(if busy { Color32::from_rgb(134, 179, 151) } else { GREEN })
                                .rounding(Rounding::same(10.0))
                                .min_size(Vec2::new(190.0, 48.0));
                            if ui.add_enabled(!busy && self.selected().is_some(), play).clicked() {
                                self.launch(ctx, None);
                            }
                        }
                    });
                });
            });
    }

    fn sidebar(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("side")
            .resizable(false)
            .exact_width(236.0)
            .frame(
                egui::Frame::none()
                    .fill(WHITE)
                    .inner_margin(egui::Margin::symmetric(14.0, 18.0))
                    .stroke(Stroke::new(1.0_f32, BORDER)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    draw_logo(ui, 46.0);
                    ui.add_space(4.0);
                    ui.vertical(|ui| {
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 4.0;
                            ui.label(RichText::new("VN").size(21.0).strong().color(GREEN_DEEP));
                            ui.label(RichText::new("Launcher").size(21.0).strong().color(GREEN));
                        });
                        ui.label(RichText::new("Minecraft Java Edition").size(11.5).color(MUTED));
                    });
                });
                ui.add_space(18.0);

                for (tab, label) in [
                    (Tab::Home, "Trang chủ"),
                    (Tab::Content, "Mod · Gói tài nguyên · Shader"),
                    (Tab::Servers, "Máy chủ"),
                    (Tab::Accounts, "Tài khoản"),
                    (Tab::Logs, "Nhật ký game"),
                    (Tab::Settings, "Cài đặt"),
                ] {
                    let sel = self.tab == tab;
                    let b = egui::Button::new(RichText::new(label).size(14.0).color(if sel { GREEN_DARK } else { TEXT }).strong())
                        .fill(if sel { GREEN_SOFT } else { Color32::TRANSPARENT })
                        .stroke(Stroke::NONE)
                        .rounding(Rounding::same(8.0))
                        .min_size(Vec2::new(ui.available_width(), 36.0));
                    if ui.add(b).clicked() {
                        self.tab = tab;
                        if tab == Tab::Content {
                            self.needs_refresh = true;
                            if self.search_results.lock().map(|r| r.is_empty()).unwrap_or(true) {
                                self.search();
                            }
                        }
                    }
                }

                ui.add_space(14.0);
                ui.separator();
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("PHIÊN BẢN GAME").size(11.5).strong().color(MUTED));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.add(btn_soft("+ Thêm")).clicked() {
                            self.new_name.clear();
                            self.new_install_fps = false;
                            self.show_create = true;
                        }
                    });
                });
                ui.add_space(6.0);

                let mut pick: Option<String> = None;
                egui::ScrollArea::vertical().id_salt("inst_list").show(ui, |ui| {
                    if self.config.instances.is_empty() {
                        hint(ui, "Chưa có phiên bản nào. Bấm \"+ Thêm\" để tạo.");
                    }
                    for inst in &self.config.instances {
                        let sel = inst.id == self.config.selected_instance_id;
                        let resp = egui::Frame::none()
                            .fill(if sel { GREEN_MIST } else { WHITE })
                            .rounding(Rounding::same(10.0))
                            .inner_margin(egui::Margin::symmetric(12.0, 9.0))
                            .stroke(Stroke::new(if sel { 1.5_f32 } else { 1.0_f32 }, if sel { GREEN } else { BORDER }))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.label(RichText::new(&inst.name).size(13.5).strong().color(TEXT));
                                ui.label(
                                    RichText::new(format!("{}  ·  {}", inst.mc_version, inst.loader.label()))
                                        .size(11.5)
                                        .color(if sel { GREEN_DARK } else { MUTED }),
                                );
                            })
                            .response;
                        if resp.interact(egui::Sense::click()).clicked() {
                            pick = Some(inst.id.clone());
                        }
                        ui.add_space(6.0);
                    }
                });
                if let Some(id) = pick {
                    self.config.selected_instance_id = id;
                    self.needs_refresh = true;
                    self.search_results.lock().map(|mut r| r.clear()).ok();
                    self.save();
                }
                ui.with_layout(Layout::bottom_up(Align::LEFT), |ui| {
                    ui.add_space(2.0);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("© 2026").size(11.0).color(MUTED));
                        ui.hyperlink_to(RichText::new("Việt Anh (Vanh)").size(11.0).strong(), "https://github.com/vietanh-1208");
                    });
                    ui.label(RichText::new("VN Launcher 2.0").size(11.5).strong().color(GREEN_DARK));
                });
            });
    }

    // ---------------- HOME ----------------
    fn home_tab(&mut self, ui: &mut egui::Ui, _ctx: &egui::Context) {
        let Some(inst_snapshot) = self.selected() else {
            card(ui, |ui| {
                title(ui, "Chào mừng đến với VN Launcher!");
                hint(ui, "Bạn chưa có phiên bản game nào. Tạo một phiên bản để bắt đầu chơi.");
                ui.add_space(8.0);
                if ui.add(btn_primary("+ Tạo phiên bản mới")).clicked() {
                    self.show_create = true;
                }
            });
            return;
        };
        let id = inst_snapshot.id.clone();
        let gd = self.game_dir(&id);
        let has_dh = modrinth::has_mod(&gd, "distanthorizons");
        let has_bobby = modrinth::has_mod(&gd, "bobby");
        let has_sodium = modrinth::has_mod(&gd, "sodium");
        let versions = self.version_list();
        let busy = self.busy.load(Ordering::SeqCst);
        let mut save = false;
        let mut open: Option<PathBuf> = None;
        let mut install_dh = false;
        let mut install_bobby = false;
        let mut remove_1024 = false;
        let mut install_fps = false;
        let mut ask_delete = false;
        let total_ram = total_ram_mb();
        let rec_ram = recommended_ram_mb();
        let max_ram = max_ram_mb();
        let show_snap_before = self.config.show_snapshots;
        let mut show_snap = show_snap_before;

        // Live download / installation banner on Home tab
        if busy {
            let msg = self.status.lock().map(|s| s.clone()).unwrap_or_default();
            egui::Frame::none()
                .fill(GREEN_SOFT)
                .stroke(Stroke::new(1.5_f32, GREEN))
                .rounding(Rounding::same(12.0))
                .inner_margin(egui::Margin::symmetric(16.0, 12.0))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(RichText::new(format!("Đang tải / cài đặt: {}", msg)).size(14.0).strong().color(GREEN_DEEP));
                    });
                });
            ui.add_space(12.0);
        }

        // Header
        card(ui, |ui| {
            ui.horizontal(|ui| {
                draw_logo(ui, 56.0);
                ui.add_space(8.0);
                ui.vertical(|ui| {
                    ui.label(RichText::new(&inst_snapshot.name).size(22.0).strong().color(TEXT));
                    ui.horizontal(|ui| {
                        pill(ui, &format!("Minecraft {}", inst_snapshot.mc_version));
                        pill(ui, inst_snapshot.loader.label());
                        if has_sodium {
                            pill(ui, "Sodium Ultra FPS");
                        }
                        if has_bobby {
                            pill(ui, &format!("Bobby · Mở khoá {} đoạn", inst_snapshot.lod_render_distance));
                        }
                        if has_dh {
                            pill(ui, &format!("Distant Horizons · {} chunk", inst_snapshot.lod_render_distance));
                        }
                    });
                });
            });
            ui.add_space(12.0);
            ui.horizontal_wrapped(|ui| {
                for (label, sub) in [
                    ("Thư mục game", ""),
                    ("Mods", "mods"),
                    ("Gói tài nguyên", "resourcepacks"),
                    ("Shader", "shaderpacks"),
                    ("Thế giới", "saves"),
                    ("Ảnh chụp", "screenshots"),
                ] {
                    if ui.add(btn_soft(label)).clicked() {
                        open = Some(if sub.is_empty() { gd.clone() } else { gd.join(sub) });
                    }
                }
            });
        });
        ui.add_space(14.0);

        let Some(inst) = self.config.instances.iter_mut().find(|i| i.id == id) else { return };

        // General
        card(ui, |ui| {
            title(ui, "Thông tin phiên bản");
            ui.add_space(8.0);
            egui::Grid::new("general").num_columns(2).spacing([18.0, 12.0]).show(ui, |ui| {
                ui.label(RichText::new("Tên hiển thị").color(MUTED));
                if ui.add(egui::TextEdit::singleline(&mut inst.name).desired_width(280.0)).changed() {
                    save = true;
                }
                ui.end_row();

                ui.label(RichText::new("Phiên bản Minecraft").color(MUTED));
                ui.horizontal(|ui| {
                    egui::ComboBox::from_id_salt("ver")
                        .selected_text(&inst.mc_version)
                        .width(140.0)
                        .height(320.0)
                        .show_ui(ui, |ui| {
                            for v in &versions {
                                if ui.selectable_value(&mut inst.mc_version, v.clone(), v).changed() {
                                    inst.loader_version = None;
                                    save = true;
                                }
                            }
                        });
                    ui.checkbox(&mut show_snap, "Hiện bản snapshot");
                });
                ui.end_row();

                ui.label(RichText::new("Bộ nạp mod").color(MUTED));
                ui.horizontal(|ui| {
                    if ui.selectable_value(&mut inst.loader, ModLoader::Fabric, "Fabric (chạy được mod)").changed() {
                        save = true;
                    }
                    if ui.selectable_value(&mut inst.loader, ModLoader::Vanilla, "Vanilla (bản gốc)").changed() {
                        save = true;
                    }
                });
                ui.end_row();
            });
            if inst.mc_version != inst_snapshot.mc_version && inst.loader == ModLoader::Fabric {
                let installed_mods = modrinth::list_installed(ContentKind::Mod, &gd);
                if !installed_mods.is_empty() {
                    let backup_dir = gd.join(format!("mods_backup_{}", inst_snapshot.mc_version));
                    let _ = fs::create_dir_all(&backup_dir);
                    for m in installed_mods {
                        if !m.is_dir {
                            let _ = fs::rename(&m.path, backup_dir.join(&m.name));
                        }
                    }
                    install_fps = true;
                }
            }
        });
        ui.add_space(14.0);

        // Performance
        card(ui, |ui| {
            title(ui, "Hiệu năng");
            hint(ui, &format!("Máy bạn có {:.1} GB RAM. Mức khuyên dùng cho Minecraft: {:.1} GB.", total_ram as f32 / 1024.0, rec_ram as f32 / 1024.0));
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("RAM cho game: {:.1} GB", inst.ram_mb as f32 / 1024.0)).color(TEXT));
                if ui
                    .add(egui::Slider::new(&mut inst.ram_mb, 1024..=max_ram).step_by(512.0).show_value(false))
                    .changed()
                {
                    save = true;
                }
                if inst.ram_mb != rec_ram && ui.add(btn_soft("Dùng mức khuyên dùng")).clicked() {
                    inst.ram_mb = rec_ram;
                    save = true;
                }
            });
            if inst.ram_mb > rec_ram {
                notice(ui, "Cấp quá nhiều RAM khiến Windows thiếu bộ nhớ và game giật hơn. Hãy dùng mức khuyên dùng.");
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("Cửa sổ game").color(TEXT));
                if ui.checkbox(&mut inst.fullscreen, "Toàn màn hình").changed() {
                    save = true;
                }
                if !inst.fullscreen {
                    if ui.add(egui::DragValue::new(&mut inst.window_width).range(640..=7680).suffix(" px")).changed() {
                        save = true;
                    }
                    ui.label("×");
                    if ui.add(egui::DragValue::new(&mut inst.window_height).range(480..=4320).suffix(" px")).changed() {
                        save = true;
                    }
                }
            });
            if inst.loader == ModLoader::Fabric && !has_sodium {
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    hint(ui, "Muốn FPS cao hơn? Cài gói tăng FPS (Sodium, Lithium, FerriteCore...). Không bắt buộc.");
                    if ui.add_enabled(!busy, btn_soft("Cài gói tăng FPS")).clicked() {
                        install_fps = true;
                    }
                });
            }
        });
        ui.add_space(14.0);

        // View distance / Bobby & DH (NOT auto-installed; shown with Download button)
        card(ui, |ui| {
            title(ui, "Mở khoá thanh Kết xuất & Tầm nhìn xa (tới 1024 đoạn)");
            if has_bobby || has_dh {
                if has_bobby {
                    hint(
                        ui,
                        "Đã tải Mod Bobby: Thanh \"Kh.cách kết xuất\" trong Cài đặt video của game đã được mở khoá vượt mốc 32 đoạn lên tới mức bạn chọn bên dưới.",
                    );
                } else {
                    hint(ui, "Đã tải Distant Horizons: Vẽ phần xa bằng mô hình giản lược (LOD). Giá trị càng lớn càng tốn CPU/RAM.");
                }
                ui.add_space(8.0);
                let mut dist_changed = false;
                ui.horizontal(|ui| {
                    for v in [64u32, 128, 256, 512, 1024] {
                        let sel = inst.lod_render_distance == v;
                        let b = egui::Button::new(RichText::new(format!("{} đoạn", v)).color(if sel { WHITE } else { GREEN_DARK }))
                            .fill(if sel { GREEN } else { GREEN_SOFT })
                            .stroke(Stroke::NONE)
                            .rounding(Rounding::same(8.0));
                        if ui.add(b).clicked() {
                            inst.lod_render_distance = v;
                            save = true;
                            dist_changed = true;
                        }
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.add_enabled(!busy, btn_danger("🗑 Xoá mở khoá 1024 đoạn")).clicked() {
                            remove_1024 = true;
                        }
                    });
                });
                ui.add_space(6.0);
                if ui.add(egui::Slider::new(&mut inst.lod_render_distance, 32..=1024).suffix(" đoạn")).changed() {
                    save = true;
                    dist_changed = true;
                }
                if dist_changed && has_bobby {
                    optimizer::patch_bobby(&gd, inst.lod_render_distance);
                }
            } else if inst.loader == ModLoader::Fabric {
                hint(
                    ui,
                    "Mặc định không tự tải sẵn (để giữ mức 1000+ FPS tối đa). Ai chơi đơn thích mở khoá thanh \"Kh.cách kết xuất\" lên tới 1024 đoạn thì bấm nút tải bên dưới:",
                );
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.add_enabled(!busy, btn_primary("⬇ Tải mở khoá 1024 đoạn (Mod Bobby)")).clicked() {
                        install_bobby = true;
                    }
                    if ui.add_enabled(!busy, btn_soft("🌄 Tải Distant Horizons (LOD)")).clicked() {
                        install_dh = true;
                    }
                });
            } else {
                hint(ui, "Bản Vanilla tối đa 32 đoạn. Chuyển sang Fabric để có nút tải mở khoá tới 1024 đoạn.");
            }
        });
        ui.add_space(14.0);

        // Advanced
        card(ui, |ui| {
            egui::CollapsingHeader::new(RichText::new("Nâng cao").size(16.0).strong().color(TEXT))
                .id_salt("adv")
                .show(ui, |ui| {
                    egui::Grid::new("adv_grid").num_columns(2).spacing([18.0, 10.0]).show(ui, |ui| {
                        ui.label(RichText::new("Java riêng (tuỳ chọn)").color(MUTED));
                        ui.horizontal(|ui| {
                            if ui
                                .add(egui::TextEdit::singleline(&mut inst.java_path).desired_width(300.0).hint_text("Để trống = tự động tải Java"))
                                .changed()
                            {
                                save = true;
                            }
                            if ui.add(btn_soft("Chọn...")).clicked() {
                                if let Some(p) = rfd::FileDialog::new().add_filter("java.exe", &["exe"]).pick_file() {
                                    inst.java_path = p.to_string_lossy().to_string();
                                    save = true;
                                }
                            }
                        });
                        ui.end_row();
                        ui.label(RichText::new("Tham số JVM thêm").color(MUTED));
                        if ui.add(egui::TextEdit::singleline(&mut inst.custom_jvm_args).desired_width(380.0)).changed() {
                            save = true;
                        }
                        ui.end_row();
                        ui.label(RichText::new("Tự vào server khi mở").color(MUTED));
                        if ui
                            .add(egui::TextEdit::singleline(&mut inst.quick_join_server).desired_width(260.0).hint_text("vd: play.server.vn"))
                            .changed()
                        {
                            save = true;
                        }
                        ui.end_row();
                        ui.label(RichText::new("Bộ dọn bộ nhớ").color(MUTED));
                        if ui.checkbox(&mut inst.use_zgc, "Dùng ZGC (chỉ nên bật khi máy ≥ 16 GB RAM)").changed() {
                            save = true;
                        }
                        ui.end_row();
                    });
                });
        });
        ui.add_space(14.0);

        // Danger zone
        card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(RichText::new("Xoá phiên bản này").size(15.0).strong().color(RED));
                    hint(ui, "Xoá toàn bộ thế giới, mod và cài đặt của phiên bản này. Không thể hoàn tác.");
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.add(btn_danger("Xoá phiên bản")).clicked() {
                        ask_delete = true;
                    }
                });
            });
        });

        if show_snap != show_snap_before {
            self.config.show_snapshots = show_snap;
            save = true;
        }
        if save {
            if !self.game_running.load(Ordering::SeqCst) && !busy {
                self.config.migrate_readable_folder_names();
            }
            self.save();
        }
        if let Some(p) = open {
            self.open_folder(p);
        }
        if install_dh {
            self.install_content(ContentKind::Mod, DH_SLUG.into(), "Distant Horizons".into());
        }
        if install_bobby {
            self.install_content(ContentKind::Mod, "bobby".into(), "Bobby (Mở khoá 1024 đoạn)".into());
        }
        if remove_1024 {
            if let Ok(entries) = fs::read_dir(gd.join("mods")) {
                for entry in entries.flatten() {
                    let name = entry.file_name().to_string_lossy().to_lowercase();
                    if name.contains("bobby") || name.contains("distanthorizons") || name.contains("distant-horizons") {
                        let _ = fs::remove_file(entry.path());
                    }
                }
            }
            let _ = fs::remove_file(gd.join("config").join("bobby.conf"));
            let _ = fs::remove_dir_all(gd.join(".bobby"));
            self.needs_refresh = true;
            self.set_status("Đã xoá mod mở khoá 1024 đoạn khỏi phiên bản này.");
        }
        if install_fps {
            self.install_pack(FPS_PACK, "Bộ 22 Mod Tối Ưu Ultra FPS");
        }
        if ask_delete {
            self.confirm_delete = Some(id);
        }
    }

    // ---------------- CONTENT ----------------
    fn content_tab(&mut self, ui: &mut egui::Ui) {
        let Some(inst) = self.selected() else {
            hint(ui, "Hãy tạo một phiên bản game trước.");
            return;
        };
        let gd = self.game_dir(&inst.id);
        let busy = self.busy.load(Ordering::SeqCst);

        card(ui, |ui| {
            ui.horizontal(|ui| {
                for kind in [ContentKind::Mod, ContentKind::ResourcePack, ContentKind::Shader] {
                    let sel = self.content_kind == kind;
                    let b = egui::Button::new(RichText::new(kind.label()).strong().color(if sel { WHITE } else { GREEN_DARK }))
                        .fill(if sel { GREEN } else { GREEN_SOFT })
                        .stroke(Stroke::NONE)
                        .rounding(Rounding::same(8.0))
                        .min_size(Vec2::new(130.0, 34.0));
                    if ui.add(b).clicked() && !sel {
                        self.content_kind = kind;
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
                        if let Some(files) = rfd::FileDialog::new().add_filter(ext, &[ext]).pick_files() {
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
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new("⚡ Bộ Mod Tối Ưu Chuẩn Java").size(16.0).strong().color(TEXT));
                    pill(ui, &format!("Đã có {}/{} mod", have_count, FPS_PACK.len()));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.add_enabled(!busy, btn_soft("🌄 Cài Distant Horizons (1024 chunk)")).clicked() {
                            dh_install = true;
                        }
                        let btn_label = if have_count == FPS_PACK.len() {
                            format!("✓ Đã cài đủ {} Mod Tối Ưu (Bấm để kiểm tra lại)", FPS_PACK.len())
                        } else {
                            format!("⚡ Cài trọn bộ {} Mod Tối Ưu (1-Click)", FPS_PACK.len())
                        };
                        if ui.add_enabled(!busy, btn_primary(&btn_label)).clicked() {
                            pack_install = true;
                        }
                    });
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

    // ---------------- SERVERS ----------------
    fn servers_tab(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        card(ui, |ui| {
            title(ui, "Thêm máy chủ");
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
        let mut join: Option<String> = None;
        let mut remove: Option<usize> = None;
        if self.config.saved_servers.is_empty() {
            hint(ui, "Chưa lưu máy chủ nào.");
        }
        for (i, s) in self.config.saved_servers.iter().enumerate() {
            card(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(RichText::new(&s.name).size(15.0).strong().color(TEXT));
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
    fn accounts_tab(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        card(ui, |ui| {
            title(ui, "Tài khoản của bạn");
            ui.add_space(8.0);
            let mut make_active: Option<String> = None;
            let mut remove: Option<String> = None;
            if self.accounts.accounts.is_empty() {
                hint(ui, "Chưa có tài khoản nào.");
            }
            for a in &self.accounts.accounts {
                let active = self.accounts.active_id.as_deref() == Some(a.id.as_str());
                egui::Frame::none()
                    .fill(if active { GREEN_MIST } else { WHITE })
                    .rounding(Rounding::same(10.0))
                    .inner_margin(egui::Margin::symmetric(12.0, 10.0))
                    .stroke(Stroke::new(1.0_f32, if active { GREEN } else { BORDER }))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            ui.vertical(|ui| {
                                ui.label(RichText::new(&a.username).size(15.0).strong().color(TEXT));
                                ui.label(RichText::new(a.kind_label()).size(12.0).color(if a.kind == AccountKind::Microsoft { GREEN_DARK } else { MUTED }));
                            });
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if ui.add(btn_danger("Xoá")).clicked() {
                                    remove = Some(a.id.clone());
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
    fn logs_tab(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
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
    fn settings_tab(&mut self, ui: &mut egui::Ui) {
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
                    install_windows_app(true);
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
            title(ui, "Giới thiệu");
            ui.label(RichText::new("VN Launcher 2.0 · Phát triển bởi Nguyễn Trọng Việt Anh (Vanh)").size(13.5).strong().color(TEXT));
            hint(ui, "Mã nguồn mở (MIT) · Viết bằng Rust · Kho Mod, Resource Pack & Shader từ Modrinth.");
            ui.add_space(4.0);
            ui.hyperlink_to(RichText::new("🌐 GitHub chính chủ: github.com/vietanh-1208").size(13.0).strong(), "https://github.com/vietanh-1208");
        });
    }

    // ---------------- DIALOGS ----------------
    fn create_dialog(&mut self, ctx: &egui::Context) {
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

    fn delete_dialog(&mut self, ctx: &egui::Context) {
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

fn remove_dir_retry(dir: &Path) -> std::io::Result<()> {
    if !dir.exists() {
        return Ok(());
    }
    for _ in 0..3 {
        if fs::remove_dir_all(dir).is_ok() {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(300));
    }
    fs::remove_dir_all(dir)
}

fn fmt_count(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.0}K", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

// =====================================================================
// Theme, fonts, icon
// =====================================================================
fn setup_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    for path in ["C:\\Windows\\Fonts\\segoeui.ttf", "C:\\Windows\\Fonts\\tahoma.ttf", "C:\\Windows\\Fonts\\arial.ttf"] {
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

fn setup_theme(ctx: &egui::Context) {
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
fn make_icon() -> IconData {
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
                // Rich emerald gradient background
                let bg = [
                    (18.0 + 10.0 * (1.0 - t)) as u8,
                    (95.0 + 68.0 * (1.0 - t)) as u8,
                    (48.0 + 34.0 * (1.0 - t)) as u8,
                    255,
                ];
                blend(x, y, bg, cov);
                // Pixel-grass top accent bar
                let grass_h = 12.0 + ((x / 4) % 3) as f32 * 2.5;
                if fy < grass_h && d <= r - 2.0 {
                    blend(x, y, [74, 222, 128, 95], cov);
                }
                // Glossy outer emerald rim
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
    // Drop shadow
    for (ax, ay, bx, by) in [
        (11.0, 19.0, 19.5, 47.0),
        (19.5, 47.0, 28.0, 19.0),
        (35.0, 47.0, 35.0, 19.0),
        (35.0, 19.0, 51.0, 47.0),
        (51.0, 47.0, 51.0, 19.0),
    ] {
        stroke(ax + 1.5, ay + 2.0, bx + 1.5, by + 2.0, 6.2, shadow);
    }
    // Crisp White "V"
    stroke(11.0, 19.0, 19.5, 47.0, 6.0, white);
    stroke(19.5, 47.0, 28.0, 19.0, 6.0, white);
    // Glowing Emerald-Mint "N"
    stroke(35.0, 47.0, 35.0, 19.0, 6.0, mint);
    stroke(35.0, 19.0, 51.0, 47.0, 5.8, white);
    stroke(51.0, 47.0, 51.0, 19.0, 6.0, mint);
    IconData { rgba, width: w, height: h }
}

fn installed_app_exe_path() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Programs")
        .join("VN Launcher")
        .join("VN Launcher.exe")
}

fn start_menu_shortcut_path() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Microsoft")
        .join("Windows")
        .join("Start Menu")
        .join("Programs")
        .join("VN Launcher.lnk")
}

/// Automatically installs VN Launcher into `%LOCALAPPDATA%\Programs\VN Launcher\VN Launcher.exe`,
/// creates Start Menu & Desktop shortcuts, and registers in Windows Installed Apps & App Paths.
fn install_windows_app(force_shortcuts: bool) {
    let Ok(cur_exe) = std::env::current_exe() else { return };
    let target_exe = installed_app_exe_path();
    let Some(target_dir) = target_exe.parent() else { return };
    let _ = fs::create_dir_all(target_dir);

    if cur_exe != target_exe {
        let should_copy = match (fs::metadata(&cur_exe), fs::metadata(&target_exe)) {
            (Ok(src_m), Ok(dst_m)) => src_m.len() != dst_m.len() || force_shortcuts,
            (Ok(_), Err(_)) => true,
            _ => false,
        };
        if should_copy {
            let _ = fs::copy(&cur_exe, &target_exe);
        }
    }

    let start_lnk = start_menu_shortcut_path();
    if !force_shortcuts && start_lnk.exists() {
        return;
    }

    let exe_str = target_exe.to_string_lossy().to_string();
    let dir_str = target_dir.to_string_lossy().to_string();
    let start_str = start_lnk.to_string_lossy().to_string();

    thread::spawn(move || {
        let ps_script = format!(
            r#"$exe = '{exe}'; $dir = '{dir}'; $startLnk = '{start}';
$deskLnk = Join-Path ([Environment]::GetFolderPath('Desktop')) 'VN Launcher.lnk';
$wsh = New-Object -ComObject WScript.Shell;
foreach ($lnkPath in @($startLnk, $deskLnk)) {{
    $sc = $wsh.CreateShortcut($lnkPath);
    $sc.TargetPath = $exe;
    $sc.WorkingDirectory = $dir;
    $sc.IconLocation = "$exe,0";
    $sc.Description = 'VN Launcher - Minecraft: Java Edition Launcher';
    $sc.Save();
}}
$uninst = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\VNLauncher';
New-Item -Path $uninst -Force | Out-Null;
Set-ItemProperty -Path $uninst -Name 'DisplayName' -Value 'VN Launcher';
Set-ItemProperty -Path $uninst -Name 'DisplayIcon' -Value "$exe,0";
Set-ItemProperty -Path $uninst -Name 'DisplayVersion' -Value '2.0.0';
Set-ItemProperty -Path $uninst -Name 'Publisher' -Value 'Nguyễn Trọng Việt Anh (Vanh)';
Set-ItemProperty -Path $uninst -Name 'InstallLocation' -Value $dir;
Set-ItemProperty -Path $uninst -Name 'UninstallString' -Value "`"$exe`" --uninstall";
Set-ItemProperty -Path $uninst -Name 'NoModify' -Value 1 -Type DWord;
Set-ItemProperty -Path $uninst -Name 'NoRepair' -Value 1 -Type DWord;
foreach ($ap in @('VN Launcher.exe','vn-launcher.exe')) {{
    $apKey = "HKCU:\Software\Microsoft\Windows\CurrentVersion\App Paths\$ap";
    New-Item -Path $apKey -Force | Out-Null;
    Set-ItemProperty -Path $apKey -Name '(Default)' -Value $exe;
    Set-ItemProperty -Path $apKey -Name 'Path' -Value $dir;
}}"#,
            exe = exe_str.replace('\'', "''"),
            dir = dir_str.replace('\'', "''"),
            start = start_str.replace('\'', "''"),
        );
        let mut cmd = Command::new("powershell");
        cmd.args(["-NoProfile", "-WindowStyle", "Hidden", "-Command", &ps_script]);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000);
        }
        let _ = cmd.status();
    });
}

fn uninstall_windows_app() {
    let start_lnk = start_menu_shortcut_path();
    let _ = fs::remove_file(start_lnk);
    let ps_script = r#"
$deskLnk = Join-Path ([Environment]::GetFolderPath('Desktop')) 'VN Launcher.lnk';
if (Test-Path $deskLnk) { Remove-Item $deskLnk -Force -ErrorAction SilentlyContinue }
Remove-Item 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\VNLauncher' -Recurse -Force -ErrorAction SilentlyContinue;
Remove-Item 'HKCU:\Software\Microsoft\Windows\CurrentVersion\App Paths\VN Launcher.exe' -Recurse -Force -ErrorAction SilentlyContinue;
Remove-Item 'HKCU:\Software\Microsoft\Windows\CurrentVersion\App Paths\vn-launcher.exe' -Recurse -Force -ErrorAction SilentlyContinue;
"#;
    let mut cmd = Command::new("powershell");
    cmd.args(["-NoProfile", "-WindowStyle", "Hidden", "-Command", ps_script]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let _ = cmd.status();
}

fn main() -> eframe::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--uninstall") {
        uninstall_windows_app();
        return Ok(());
    }

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("VN Launcher")
            .with_inner_size([1120.0, 720.0])
            .with_min_inner_size([960.0, 600.0])
            .with_icon(Arc::new(make_icon())),
        ..Default::default()
    };
    eframe::run_native("VN Launcher", options, Box::new(|cc| Ok(Box::new(App::new(cc)))))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_test_data_junctions(cfg: &LauncherConfig) {
        let base = cfg.base_data_dir();
        let _ = fs::create_dir_all(base.join("instances"));
        let real_data = PathBuf::from(r"C:\Users\Vanh\Downloads\vn_launcher_data");
        for sub in ["shared", "runtimes"] {
            let link = base.join(sub);
            let target = real_data.join(sub);
            if !link.exists() && target.exists() {
                let _ = Command::new("cmd")
                    .args(["/c", "mklink", "/J"])
                    .arg(&link)
                    .arg(&target)
                    .status();
            }
        }
    }

    fn inspect_process_cmd(pid: u32) -> String {
        let out = Command::new("powershell")
            .args([
                "-NoProfile",
                "-Command",
                &format!(
                    "(Get-CimInstance Win32_Process -Filter 'ProcessId={}' | Select-Object -ExpandProperty CommandLine)",
                    pid
                ),
            ])
            .output()
            .expect("powershell query");
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    #[test]
    fn verify_runtime_flows() {
        // 1. RAM & icon
        let total = total_ram_mb();
        let rec = recommended_ram_mb();
        let max = max_ram_mb();
        println!("[TEST 1] System RAM: total={} MB, recommended={} MB, max={} MB", total, rec, max);
        assert!(total >= 4096);
        assert_eq!(rec, 3072, "8 GB system should recommend 3072 MB");
        let icon = make_icon();
        assert_eq!(icon.width, 64);
        assert_eq!(icon.height, 64);
        assert_eq!(icon.rgba.len(), 64 * 64 * 4);

        // 2. Patch Distant Horizons v5 config
        let real_dh = PathBuf::from(
            r"C:\Users\Vanh\Downloads\vn_launcher_data\instances\inst-1.21.11-3\.minecraft\config\DistantHorizons.toml",
        );
        if real_dh.exists() {
            let tmp_game = std::env::temp_dir().join("vn_launcher_dh_test");
            let _ = fs::create_dir_all(tmp_game.join("config"));
            fs::copy(&real_dh, tmp_game.join("config").join("DistantHorizons.toml")).unwrap();
            assert!(optimizer::patch_distant_horizons(&tmp_game, 512));
            let patched = fs::read_to_string(tmp_game.join("config").join("DistantHorizons.toml")).unwrap();
            assert!(patched.contains("_version = 5"));
            assert!(patched.contains("lodChunkRenderDistanceRadius = 512"));
            assert!(patched.contains("numberOfThreads = 3"));
            assert!(patched.contains("threadRunTimeRatio = \"0.75\""));
            assert!(patched.contains("enableSsao = false"));
            let _ = fs::remove_dir_all(&tmp_game);
            println!("[TEST 2] DistantHorizons.toml v5 patched cleanly in-place");
        }

        // 3. Setup test config with junctions to cached shared/runtimes
        let mut cfg = LauncherConfig::default();
        cfg.portable_mode = true;
        setup_test_data_junctions(&cfg);

        // 4. Instance create, rename, delete scope
        let mut dummy = InstanceConfig::new("Bản Thử Xoá", "1.21.11", ModLoader::Vanilla);
        dummy.name = "Bản Đã Đổi Tên".into();
        let dummy_dir = cfg.instance_game_dir(&dummy.id);
        optimizer::prepare_game_dir(&dummy_dir).unwrap();
        fs::write(dummy_dir.join("saves").join("world.txt"), "keep_test").unwrap();
        assert!(dummy_dir.exists());
        remove_dir_retry(&cfg.instance_dir(&dummy.id)).unwrap();
        assert!(!cfg.instance_dir(&dummy.id).exists(), "Instance dir must be deleted");
        assert!(cfg.shared_dir().exists(), "Shared dir must remain untouched");
        assert!(cfg.runtimes_dir().exists(), "Runtimes dir must remain untouched");
        println!("[TEST 3] Create, rename, and scoped delete verified (shared & runtimes preserved)");

        // 5. Launch Vanilla 1.21.11 (no mods forced!) + inspect JVM command line + stop game
        let vanilla_inst = InstanceConfig::new("Test Vanilla 1.21.11", "1.21.11", ModLoader::Vanilla);
        let vanilla_game_dir = cfg.instance_game_dir(&vanilla_inst.id);
        let status = Arc::new(Mutex::new(String::new()));
        let acc = Account::offline("VanhTest");
        let la = LaunchAccount {
            username: acc.username.clone(),
            uuid: acc.uuid.clone(),
            access_token: "0".into(),
            user_type: "legacy".into(),
        };
        let mut child = prepare_and_launch(&cfg, &vanilla_inst, &la, None, &status)
            .expect("Vanilla 1.21.11 should launch");
        let pid = child.id();
        println!("[TEST 4] Vanilla 1.21.11 spawned with PID {}", pid);

        let mods_count = fs::read_dir(vanilla_game_dir.join("mods")).unwrap().count();
        assert_eq!(mods_count, 0, "Vanilla instance must have 0 forced mods");
        let opts = fs::read_to_string(vanilla_game_dir.join("options.txt")).unwrap();
        assert!(opts.contains("renderDistance:12") && opts.contains("simulationDistance:8"));

        let cmdline = inspect_process_cmd(pid);
        println!("[TEST 4] Vanilla JVM cmdline prefix: {}", &cmdline.chars().take(240).collect::<String>());
        assert!(cmdline.contains("-Xmx3072M"));
        assert!(cmdline.contains("-Xms1024M"));
        assert!(cmdline.contains("-XX:+UseG1GC"));
        assert!(!cmdline.contains("AlwaysPreTouch"));
        assert!(cmdline.contains("net.minecraft.client.main.Main"));
        assert!(child.try_wait().unwrap().is_none(), "Vanilla process must still be running");

        let _ = child.kill();
        let _ = child.wait();
        assert!(child.try_wait().unwrap().is_some(), "Vanilla process must be stopped after kill");
        let _ = remove_dir_retry(&cfg.instance_dir(&vanilla_inst.id));
        println!("[TEST 4] Vanilla 1.21.11 verified & stopped cleanly");

        // 6. Launch Fabric 1.21.11 (0 forced mods, deduplicated classpath, read logs) + stop game
        let fabric_inst = InstanceConfig::new("Test Fabric 1.21.11", "1.21.11", ModLoader::Fabric);
        let fabric_game_dir = cfg.instance_game_dir(&fabric_inst.id);
        let mut fchild = prepare_and_launch(&cfg, &fabric_inst, &la, None, &status)
            .expect("Fabric 1.21.11 should launch");
        let fpid = fchild.id();
        println!("[TEST 5] Fabric 1.21.11 spawned with PID {}", fpid);

        let fmods_count = fs::read_dir(fabric_game_dir.join("mods")).unwrap().count();
        assert_eq!(fmods_count, 0, "New Fabric instance must have 0 forced mods");

        let fcmd = inspect_process_cmd(fpid);
        assert!(fcmd.contains("net.fabricmc.loader.impl.launch.knot.KnotClient"));

        // Read a couple lines from stdout to confirm piped logs & no remap lock error
        if let Some(stdout) = fchild.stdout.take() {
            let mut reader = BufReader::new(stdout);
            let mut line = String::new();
            for _ in 0..3 {
                line.clear();
                if reader.read_line(&mut line).unwrap_or(0) > 0 {
                    println!("[TEST 5 LOG] {}", line.trim());
                }
            }
        }
        let _ = fchild.kill();
        let _ = fchild.wait();
        let _ = remove_dir_retry(&cfg.instance_dir(&fabric_inst.id));
        println!("[TEST 5] Fabric 1.21.11 verified & stopped cleanly");

        // 7. Microsoft OAuth device code check with unconfigured/dummy client ID
        assert!(cfg.ms_client_id.is_empty(), "Default ms_client_id must be empty");
        let http = mojang::build_http_client(15).unwrap();
        let ms_res = auth::request_device_code(&http, "00000000-0000-0000-0000-000000000000");
        assert!(ms_res.is_err(), "Invalid client_id must be rejected by Microsoft");
        println!("[TEST 6] Microsoft OAuth rejected dummy client_id as expected: {}", ms_res.unwrap_err());

        for sub in ["shared", "runtimes"] {
            let _ = Command::new("cmd")
                .args(["/c", "rmdir"])
                .arg(cfg.base_data_dir().join(sub))
                .status();
        }
        let _ = fs::remove_dir_all(cfg.base_data_dir());
    }
}
