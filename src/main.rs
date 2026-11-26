#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod auth;
mod config;
mod fabric;
mod java;
mod launcher;
mod modloader;
mod modrinth;
mod mojang;
mod optimizer;
mod ui;
mod updater;
mod windows_app;

use auth::{Account, AccountKind, AccountStore, PollResult};
use config::{InstanceConfig, LauncherConfig, ModLoader, ServerEntry};
use eframe::egui::{
    self, Align, Color32, Layout, RichText, Rounding, Stroke, Vec2,
};
use java::ensure_java_runtime;
use launcher::{prepare_and_launch, LaunchAccount};
use modrinth::{ContentKind, FamousMod, InstalledItem, SearchHit, DH_SLUG, FPS_PACK};
use std::collections::VecDeque;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::Child;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use ui::*;
use updater::*;
use windows_app::*;

const POPULAR_VERSIONS: &[&str] = &[
    "26.3", "26.2", "1.21.11", "1.21.4", "1.21.3", "1.21.1", "1.20.6", "1.20.4", "1.20.1", "1.19.4", "1.19.2",
    "1.18.2", "1.17.1", "1.16.5", "1.12.2", "1.8.9",
];

#[derive(PartialEq, Eq, Clone, Copy)]
pub(crate) enum Tab {
    Home,
    Content,
    Worlds,
    Servers,
    Accounts,
    Logs,
    Settings,
}

#[derive(PartialEq, Eq, Clone, Copy)]
pub(crate) enum ContentView {
    Browse,
    Installed,
}

#[derive(Clone, serde::Deserialize)]
struct NewsInfo {
    title: String,
    message: String,
    #[serde(default = "default_discord_url")]
    discord_url: String,
    #[serde(default = "default_srv_name")]
    featured_server_name: String,
    #[serde(default = "default_srv_ip")]
    featured_server_ip: String,
}

fn default_discord_url() -> String {
    "https://discord.gg/gmRfdF342s".into()
}
fn default_srv_name() -> String {
    "NGV SMP".into()
}
fn default_srv_ip() -> String {
    "ngvsmp.asia:25616".into()
}

impl Default for NewsInfo {
    fn default() -> Self {
        Self {
            title: "Chào mừng đến với VN Launcher 2.1!".into(),
            message: "Tham gia cộng đồng Discord & chiến cùng anh em tại server NGV SMP (ngvsmp.asia:25616) — hỗ trợ cả Offline & Bản quyền!".into(),
            discord_url: default_discord_url(),
            featured_server_name: default_srv_name(),
            featured_server_ip: default_srv_ip(),
        }
    }
}

#[derive(Clone)]
pub(crate) enum LoginState {
    Idle,
    Requesting,
    ShowCode { user_code: String, uri: String },
    Error(String),
    Done(String),
}

pub(crate) struct App {
    pub(crate) config: LauncherConfig,
    pub(crate) accounts: AccountStore,
    pub(crate) tab: Tab,

    pub(crate) busy: Arc<AtomicBool>,
    pub(crate) status: Arc<Mutex<String>>,
    pub(crate) versions: Arc<Mutex<Vec<(String, String)>>>,

    pub(crate) game_child: Arc<Mutex<Option<Child>>>,
    pub(crate) stop_requested: Arc<AtomicBool>,
    pub(crate) game_running: Arc<AtomicBool>,
    pub(crate) game_logs: Arc<Mutex<VecDeque<String>>>,

    pub(crate) show_create: bool,
    pub(crate) new_name: String,
    pub(crate) new_version: String,
    pub(crate) new_loader: ModLoader,
    pub(crate) new_install_fps: bool,
    pub(crate) confirm_delete: Option<String>,

    pub(crate) content_kind: ContentKind,
    pub(crate) content_view: ContentView,
    pub(crate) search_query: String,
    pub(crate) search_results: Arc<Mutex<Vec<SearchHit>>>,
    pub(crate) searching: Arc<AtomicBool>,
    pub(crate) installed: Vec<InstalledItem>,
    pub(crate) needs_refresh: bool,

    pub(crate) new_server_name: String,
    pub(crate) new_server_addr: String,

    pub(crate) new_offline_name: String,
    pub(crate) login_state: Arc<Mutex<LoginState>>,
    pub(crate) pending_accounts: Arc<Mutex<Vec<Account>>>,
    pub(crate) client_id_input: String,
    pub(crate) update_state: Arc<Mutex<UpdateState>>,
    pub(crate) famous_mods: Arc<Mutex<Vec<FamousMod>>>,
    pub(crate) famous_packs: Arc<Mutex<Vec<FamousMod>>>,
    pub(crate) famous_shaders: Arc<Mutex<Vec<FamousMod>>>,
    pub(crate) server_pings: Arc<Mutex<std::collections::HashMap<String, Option<u128>>>>,
    pub(crate) news: Arc<Mutex<NewsInfo>>,
    pub(crate) hardware: Arc<Mutex<optimizer::HardwareInfo>>,
    pub(crate) avatar_cache: std::collections::HashMap<String, egui::TextureHandle>,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        setup_fonts(&cc.egui_ctx);
        setup_theme(&cc.egui_ctx);

        let config = {
            let mut c = LauncherConfig::load();
            if c.instances.iter().all(|i| i.id != c.selected_instance_id) {
                c.selected_instance_id = c.instances.first().map(|i| i.id.clone()).unwrap_or_default();
            }
            if c.saved_servers.iter().all(|s| !s.address.eq_ignore_ascii_case("ngvsmp.asia:25616")) {
                c.saved_servers.insert(
                    0,
                    ServerEntry {
                        name: "NGV SMP".into(),
                        address: "ngvsmp.asia:25616".into(),
                    },
                );
                let _ = c.save();
            }
            c
        };
        let mut accounts = AccountStore::load();
        if accounts.accounts.is_empty() && !config.username.trim().is_empty() {
            accounts.upsert(Account::offline(config.username.trim()), true);
            let _ = accounts.save();
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
        let update_state = Arc::new(Mutex::new(UpdateState::Idle));
        spawn_update_check(Arc::clone(&update_state), false);
        let famous_mods = Arc::new(Mutex::new(modrinth::default_famous_mods()));
        let famous_packs = Arc::new(Mutex::new(modrinth::default_famous_packs()));
        let famous_shaders = Arc::new(Mutex::new(modrinth::default_famous_shaders()));
        {
            let fm = Arc::clone(&famous_mods);
            let fp = Arc::clone(&famous_packs);
            let fs_list = Arc::clone(&famous_shaders);
            thread::spawn(move || {
                if let Ok(client) = mojang::build_http_client(10) {
                    if let Some(remote) = modrinth::fetch_remote_famous_list(&client, "famous_mods.json") {
                        if let Ok(mut g) = fm.lock() { *g = remote; }
                    }
                    if let Some(remote) = modrinth::fetch_remote_famous_list(&client, "famous_packs.json") {
                        if let Ok(mut g) = fp.lock() { *g = remote; }
                    }
                    if let Some(remote) = modrinth::fetch_remote_famous_list(&client, "famous_shaders.json") {
                        if let Ok(mut g) = fs_list.lock() { *g = remote; }
                    }
                }
            });
        }
        let news = Arc::new(Mutex::new(NewsInfo::default()));
        {
            let news_arc = Arc::clone(&news);
            thread::spawn(move || {
                if let Ok(client) = mojang::build_http_client(10) {
                    if let Ok(resp) = client
                        .get("https://raw.githubusercontent.com/vietanh-1208/vn-launcher/main/news.json")
                        .header("User-Agent", "VN-Launcher/2.1")
                        .send()
                    {
                        if resp.status().is_success() {
                            if let Ok(n) = resp.json::<NewsInfo>() {
                                if let Ok(mut g) = news_arc.lock() {
                                    *g = n;
                                }
                            }
                        }
                    }
                }
            });
        }
        let hardware = Arc::new(Mutex::new(optimizer::HardwareInfo::default()));
        {
            let hardware = Arc::clone(&hardware);
            thread::spawn(move || {
                let hw = optimizer::detect_hardware();
                if let Ok(mut guard) = hardware.lock() {
                    *guard = hw;
                }
            });
        }
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
            update_state,
            famous_mods,
            famous_packs,
            famous_shaders,
            server_pings: Arc::new(Mutex::new(std::collections::HashMap::new())),
            news,
            hardware,
            avatar_cache: std::collections::HashMap::new(),
        }
    }

    // ---------- helpers ----------
    pub(crate) fn selected(&self) -> Option<InstanceConfig> {
        self.config.instances.iter().find(|i| i.id == self.config.selected_instance_id).cloned()
    }

    pub(crate) fn set_status(&self, msg: impl Into<String>) {
        if let Ok(mut s) = self.status.lock() {
            *s = msg.into();
        }
    }

    pub(crate) fn version_list(&self) -> Vec<String> {
        let show_snap = self.config.show_snapshots;
        self.versions
            .lock()
            .map(|g| g.iter().filter(|(_, t)| show_snap || t == "release").map(|(id, _)| id.clone()).collect())
            .unwrap_or_default()
    }

    pub(crate) fn game_dir(&self, id: &str) -> PathBuf {
        self.config.instance_game_dir(id)
    }

    pub(crate) fn open_folder(&self, path: PathBuf) {
        open_path_in_file_manager(&path);
    }

    pub(crate) fn save(&self) {
        let _ = self.config.save();
    }

    // ---------- actions ----------
    pub(crate) fn launch(&mut self, ctx: &egui::Context, server: Option<String>) {
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
        let hw = self.hardware.lock().map(|h| h.clone()).unwrap_or_default();
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
                        offline_skin: acc.offline_skin.clone(),
                    },
                    AccountKind::Microsoft => LaunchAccount {
                        username: acc.username.clone(),
                        uuid: acc.uuid.clone(),
                        access_token: acc.access_token.clone(),
                        user_type: "msa".into(),
                        offline_skin: String::new(),
                    },
                };
                prepare_and_launch(&cfg, &instance, &la, server, &hw, &status)
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

    pub(crate) fn search(&mut self) {
        let Some(inst) = self.selected() else { return };
        let kind = self.content_kind;
        let query = self.search_query.trim().to_string();
        let loader_slug = inst.loader.modrinth_slug().to_string();
        let results = Arc::clone(&self.search_results);
        let searching = Arc::clone(&self.searching);
        let status = Arc::clone(&self.status);
        searching.store(true, Ordering::SeqCst);
        thread::spawn(move || {
            if let Ok(client) = mojang::build_http_client(20) {
                match modrinth::search(&client, kind, &query, &inst.mc_version, &loader_slug) {
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

    pub(crate) fn install_content(&mut self, kind: ContentKind, slug: String, title: String) {
        if self.busy.load(Ordering::SeqCst) {
            return;
        }
        let Some(inst) = self.selected() else { return };
        let game_dir = self.game_dir(&inst.id);
        let loader_slug = inst.loader.modrinth_slug().to_string();
        let busy = Arc::clone(&self.busy);
        let status = Arc::clone(&self.status);
        busy.store(true, Ordering::SeqCst);
        thread::spawn(move || {
            if let Ok(client) = mojang::build_http_client(120) {
                let msg = if kind == ContentKind::Modpack {
                    match modrinth::install_mrpack_from_modrinth(&client, &slug, &inst.mc_version, &loader_slug, &game_dir, &status) {
                        Ok((pack_name, _mc, _loader, count)) => {
                            optimizer::ensure_modernfix_compat(&game_dir);
                            format!("Đã cài Modpack {} ({} file).", pack_name, count)
                        }
                        Err(e) => format!("Không cài được Modpack {}: {}", title, e),
                    }
                } else {
                    match modrinth::install_project_for_loader(&client, kind, &slug, &inst.mc_version, &loader_slug, &game_dir, &status) {
                        Ok(_) => {
                            if slug == "modernfix" {
                                optimizer::ensure_modernfix_compat(&game_dir);
                            } else if slug == "bobby" {
                                optimizer::patch_bobby(&game_dir, 1024);
                            }
                            format!("Đã cài {}.", title)
                        }
                        Err(e) => format!("Không cài được {}: {}", title, e),
                    }
                };
                if let Ok(mut s) = status.lock() {
                    *s = msg;
                }
            }
            busy.store(false, Ordering::SeqCst);
        });
        self.needs_refresh = true;
    }

    pub(crate) fn install_local_mrpack(&mut self, mrpack_path: PathBuf) {
        if self.busy.load(Ordering::SeqCst) {
            return;
        }
        let Some(inst) = self.selected() else { return };
        let game_dir = self.game_dir(&inst.id);
        let busy = Arc::clone(&self.busy);
        let status = Arc::clone(&self.status);
        busy.store(true, Ordering::SeqCst);
        thread::spawn(move || {
            if let Ok(client) = mojang::build_http_client(120) {
                let msg = match modrinth::install_mrpack_file(&client, &mrpack_path, &game_dir, &status) {
                    Ok((pack_name, mc, _loader, count)) => {
                        optimizer::ensure_modernfix_compat(&game_dir);
                        format!("Đã cài xong Modpack {} (MC {}, {} file).", pack_name, mc, count)
                    }
                    Err(e) => format!("Lỗi cài Modpack: {}", e),
                };
                if let Ok(mut s) = status.lock() {
                    *s = msg;
                }
            }
            busy.store(false, Ordering::SeqCst);
        });
        self.needs_refresh = true;
    }

    pub(crate) fn install_pack(&mut self, slugs: &'static [(&'static str, &'static str)], name: &'static str) {
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

    pub(crate) fn download_java(&mut self, major: u32) {
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

    pub(crate) fn start_microsoft_login(&mut self) {
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
            open_url_in_browser(&link);
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
                    Tab::Worlds => self.worlds_tab(ui),
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
            .min_height(82.0)
            .frame(
                egui::Frame::none()
                    .fill(WHITE)
                    .inner_margin(egui::Margin::symmetric(20.0, 12.0))
                    .stroke(Stroke::new(1.0_f32, BORDER)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
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
                            .width(210.0)
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

                    // Status (strictly bounded width so it wraps cleanly and never gets covered by the Play/Stop button)
                    let reserve_right = if running { 265.0 } else { 210.0 };
                    let status_w = (ui.available_width() - reserve_right).max(160.0);
                    ui.allocate_ui_with_layout(Vec2::new(status_w, 58.0), Layout::top_down(Align::LEFT), |ui| {
                        ui.set_max_width(status_w);
                        let inst_line = self
                            .selected()
                            .map(|i| format!("{}  ·  Minecraft {}  ·  {}", i.name, i.mc_version, i.loader.label()))
                            .unwrap_or_else(|| "Chưa chọn phiên bản".into());
                        ui.add(egui::Label::new(RichText::new(inst_line).size(13.0).strong().color(TEXT)).wrap());
                        ui.horizontal(|ui| {
                            if busy {
                                ui.spinner();
                            }
                            let msg = self.status.lock().map(|s| s.clone()).unwrap_or_default();
                            let color = if msg.starts_with("Lỗi") || msg.contains("bị lỗi") { RED } else { MUTED };
                            ui.add(egui::Label::new(RichText::new(msg).size(12.5).color(color)).wrap());
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
                    (Tab::Content, "Mod · Gói · Shader · Modpack"),
                    (Tab::Worlds, "Thế giới & Sao lưu"),
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
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(format!("VN Launcher {}", APP_VERSION)).size(11.5).strong().color(GREEN_DARK));
                        ui.hyperlink_to(RichText::new("💬 Discord").size(11.5).strong(), "https://discord.gg/gmRfdF342s");
                    });
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
        let mut optimize_gfx = false;
        let mut ask_delete = false;
        let mut quick_join_srv: Option<String> = None;
        let hw = self.hardware.lock().map(|h| h.clone()).unwrap_or_default();
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

        // Update available banner
        let upd_snap = self.update_state.lock().map(|u| u.clone()).unwrap_or(UpdateState::Idle);
        if let UpdateState::Available { version, download_url } = upd_snap {
            egui::Frame::none()
                .fill(GREEN_SOFT)
                .stroke(Stroke::new(1.5_f32, GREEN))
                .rounding(Rounding::same(12.0))
                .inner_margin(egui::Margin::symmetric(16.0, 10.0))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(format!("🎉 Có bản cập nhật mới v{} trên GitHub!", version)).size(14.0).strong().color(GREEN_DEEP));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui.add(btn_primary(&format!("⬇ Cập nhật ngay (v{})", version))).clicked() {
                                start_self_update(download_url, Arc::clone(&self.update_state), Arc::clone(&self.status));
                            }
                        });
                    });
                });
            ui.add_space(12.0);
        } else if matches!(upd_snap, UpdateState::Downloading) {
            egui::Frame::none()
                .fill(GREEN_SOFT)
                .stroke(Stroke::new(1.5_f32, GREEN))
                .rounding(Rounding::same(12.0))
                .inner_margin(egui::Margin::symmetric(16.0, 10.0))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(RichText::new("Đang tải bản cập nhật mới nhất và khởi động lại...").size(14.0).strong().color(GREEN_DEEP));
                    });
                });
            ui.add_space(12.0);
        }

        let news_snap = self.news.lock().map(|n| n.clone()).unwrap_or_default();
        card(ui, |ui| {
            ui.label(RichText::new(format!("📢 {}", news_snap.title)).size(15.0).strong().color(GREEN_DEEP));
            ui.add_space(2.0);
            ui.add(egui::Label::new(RichText::new(&news_snap.message).size(12.5).color(TEXT)).wrap());
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if !news_snap.featured_server_ip.trim().is_empty()
                    && ui
                        .add_enabled(!busy, btn_primary(&format!("🎮 Vào {}", news_snap.featured_server_name)))
                        .clicked()
                {
                    quick_join_srv = Some(news_snap.featured_server_ip.clone());
                }
                ui.add_space(6.0);
                ui.hyperlink_to(RichText::new("💬 Tham gia Discord").size(13.0).strong(), &news_snap.discord_url);
            });
        });
        ui.add_space(12.0);
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
                    for (ldr, lbl) in [
                        (ModLoader::Fabric, "Fabric (Nhanh nhất)"),
                        (ModLoader::NeoForge, "NeoForge"),
                        (ModLoader::Forge, "Forge"),
                        (ModLoader::Vanilla, "Vanilla (bản gốc)"),
                    ] {
                        if ui.selectable_value(&mut inst.loader, ldr, lbl).changed() {
                            save = true;
                        }
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
        let perf = ui::optimizer_ui::render_performance_card(
            ui,
            inst,
            &hw,
            busy,
            has_sodium,
            has_bobby,
            has_dh,
        );
        if perf.save {
            save = true;
        }
        if perf.optimize_gfx {
            optimize_gfx = true;
        }
        if perf.install_fps {
            install_fps = true;
        }
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
                                let dialog = if cfg!(windows) {
                                    rfd::FileDialog::new().add_filter("java.exe", &["exe"])
                                } else {
                                    rfd::FileDialog::new()
                                };
                                if let Some(p) = dialog.pick_file() {
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
        if optimize_gfx {
            let profile = self
                .config
                .instances
                .iter()
                .find(|i| i.id == id)
                .map(|i| i.optimization_profile)
                .unwrap_or_default();
            match optimizer::optimize_graphics_options(&gd, profile, &hw) {
                Ok(eff) => self.set_status(format!(
                    "Đã tự động tối ưu đồ hoạ theo hồ sơ {}!",
                    eff.label()
                )),
                Err(e) => self.set_status(format!("Không chỉnh được options.txt: {}", e)),
            }
        }
        if ask_delete {
            self.confirm_delete = Some(id);
        }
        if let Some(addr) = quick_join_srv {
            self.launch(_ctx, Some(addr));
        }
    }
}

pub(crate) fn remove_dir_retry(dir: &Path) -> std::io::Result<()> {
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

pub(crate) fn fmt_count(n: u64) -> String {
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

/// High-contrast 64x64 "VN" Emerald-Block icon for the window & taskbar
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
    use crate::config::{max_ram_mb, recommended_ram_mb, total_ram_mb};
    use std::process::Command;

    fn setup_test_data_junctions(cfg: &LauncherConfig) {
        let base = cfg.base_data_dir();
        let _ = fs::create_dir_all(base.join("instances"));
        let candidates = [
            PathBuf::from(r"C:\Users\Vanh\Downloads\vn_launcher_data"),
            dirs::data_dir().unwrap_or_default().join(".vn_launcher"),
        ];
        for sub in ["shared", "runtimes"] {
            let link = base.join(sub);
            if !link.exists() {
                if let Some(target) = candidates.iter().map(|c| c.join(sub)).find(|p| p.exists()) {
                    let _ = Command::new("cmd")
                        .args(["/c", "mklink", "/J"])
                        .arg(&link)
                        .arg(&target)
                        .status();
                } else {
                    let _ = fs::create_dir_all(&link);
                }
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
            offline_skin: String::new(),
        };
        let mut child = prepare_and_launch(&cfg, &vanilla_inst, &la, None, &optimizer::HardwareInfo::default(), &status)
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
        let mut fchild = prepare_and_launch(&cfg, &fabric_inst, &la, None, &optimizer::HardwareInfo::default(), &status)
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
