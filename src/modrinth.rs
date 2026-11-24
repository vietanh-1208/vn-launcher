use crate::mojang::download_file_verified;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const API: &str = "https://api.modrinth.com/v2";
const UA: &str = "VN-Launcher/2.0 (open-source Minecraft launcher)";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentKind {
    Mod,
    ResourcePack,
    Shader,
}

impl ContentKind {
    pub fn label(&self) -> &'static str {
        match self {
            ContentKind::Mod => "Mod",
            ContentKind::ResourcePack => "Gói tài nguyên",
            ContentKind::Shader => "Shader",
        }
    }
    pub fn project_type(&self) -> &'static str {
        match self {
            ContentKind::Mod => "mod",
            ContentKind::ResourcePack => "resourcepack",
            ContentKind::Shader => "shader",
        }
    }
    pub fn folder(&self) -> &'static str {
        match self {
            ContentKind::Mod => "mods",
            ContentKind::ResourcePack => "resourcepacks",
            ContentKind::Shader => "shaderpacks",
        }
    }
    pub fn file_ext(&self) -> &'static str {
        match self {
            ContentKind::Mod => "jar",
            _ => "zip",
        }
    }
}

/// Full 22-mod Ultra-FPS suite for all Minecraft versions (1.16.5 -> 26.3+)
/// (Bobby / Distant Horizons 1024-chunk mods are NOT auto-installed here; they are optional 1-click downloads on the Home tab)
pub const FPS_PACK: &[(&str, &str)] = &[
    ("fabric-api", "Fabric API – Thư viện nền tảng bắt buộc"),
    ("sodium", "Sodium – Tăng 300–500% FPS & tối ưu render GPU"),
    ("c2me-fabric", "C2ME – Đa luồng hoá tạo & nạp chunk trên toàn bộ nhân CPU"),
    ("lithium", "Lithium – Tối ưu vật lý, AI quái & chunk tick"),
    ("ferrite-core", "FerriteCore – Giảm ~45% lượng RAM tiêu thụ"),
    ("modernfix", "ModernFix – Tăng tốc mở game, giảm RAM sâu & chống tràn bộ nhớ"),
    ("immediatelyfast", "ImmediatelyFast – Tối ưu render GUI, HUD, Entity & Block Entity"),
    ("exordium", "Exordium – Đẩy tối đa FPS bằng cách đệm khung hình giao diện 2D/HUD"),
    ("threadtweak", "ThreadTweak – Tối ưu độ ưu tiên luồng Render để giữ FPS cao ổn định"),
    ("entityculling", "EntityCulling – Ẩn thực thể/khối bị tường che khuất"),
    ("moreculling", "More Culling – Ẩn mặt khối, lá cây & khung vật phẩm bị khuất"),
    ("badoptimizations", "BadOptimizations – Tối ưu lightmap, bầu trời & các tác vụ thừa"),
    ("noisium", "Noisium – Tăng tốc 30% thuật toán tạo địa hình (Worldgen)"),
    ("krypton", "Krypton – Tối ưu nén gói tin mạng, giảm trễ khi chơi server"),
    ("fast-ip-ping", "Fast IP Ping – Tối ưu kết nối & ping máy chủ tức thì"),
    ("sodium-extra", "Sodium Extra – Tuỳ chỉnh chi tiết sương mù, hạt & hiệu ứng"),
    ("reeses-sodium-options", "Reese's Sodium Options – Giao diện cài đặt đồ hoạ gọn đẹp"),
    ("dynamic-fps", "Dynamic FPS – Tự giảm tải GPU/CPU khi thu nhỏ cửa sổ"),
    ("iris", "Iris Shaders – Hỗ trợ chạy Shader mượt kèm Sodium"),
    ("appleskin", "AppleSkin – Hiển thị độ no & hồi máu của thức ăn"),
    ("inventoryhud", "Inventory HUD+ – Hiển thị giáp, túi đồ & độ bền trên màn hình"),
    ("modmenu", "Mod Menu – Danh sách & chỉnh cài đặt mod trong game"),
];
/// Extra legacy optimization mods automatically included when installing on older MC versions (1.16.5..1.20.4)
pub const LEGACY_EXTRA_MODS: &[(&str, &str)] = &[
    ("indium", "Indium – Tương thích Fabric Rendering API cho Sodium bản cũ"),
    ("lazydfu", "LazyDFU – Tăng tốc khởi động cho Minecraft 1.16–1.19"),
    ("starlight", "Starlight – Tối ưu hệ thống ánh sáng cho Minecraft 1.16–1.20.4"),
];
pub const DH_SLUG: &str = "distanthorizons";
pub const IRIS_SLUG: &str = "iris";

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SearchResponse {
    pub hits: Vec<SearchHit>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SearchHit {
    pub project_id: String,
    pub slug: String,
    pub title: String,
    pub description: String,
    pub author: String,
    pub downloads: u64,
}

#[derive(Debug, Clone, Deserialize)]
struct ProjectVersion {
    files: Vec<VersionFile>,
    #[serde(default)]
    dependencies: Vec<Dependency>,
}

#[derive(Debug, Clone, Deserialize)]
struct VersionFile {
    url: String,
    filename: String,
    primary: Option<bool>,
    hashes: Option<Hashes>,
}

#[derive(Debug, Clone, Deserialize)]
struct Hashes {
    sha1: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct Dependency {
    project_id: Option<String>,
    dependency_type: String,
}

#[derive(Debug, Clone)]
pub struct InstalledItem {
    pub name: String,
    pub path: PathBuf,
    pub enabled: bool,
    pub size_kb: u64,
    pub is_dir: bool,
}

pub fn search(client: &Client, kind: ContentKind, query: &str, mc_version: &str) -> Result<Vec<SearchHit>, String> {
    let facets = match kind {
        ContentKind::Mod => format!(
            "[[\"project_type:mod\"],[\"versions:{}\"],[\"categories:fabric\"]]",
            mc_version
        ),
        _ => format!(
            "[[\"project_type:{}\"],[\"versions:{}\"]]",
            kind.project_type(),
            mc_version
        ),
    };
    let index = if query.trim().is_empty() { "downloads" } else { "relevance" };
    let resp = client
        .get(format!("{}/search", API))
        .header("User-Agent", UA)
        .query(&[("query", query), ("facets", facets.as_str()), ("index", index), ("limit", "30")])
        .timeout(Duration::from_secs(20))
        .send()
        .map_err(|e| format!("Không kết nối được Modrinth: {}", e))?;
    if !resp.status().is_success() {
        return Err(format!("Modrinth trả về lỗi HTTP {}", resp.status()));
    }
    resp.json::<SearchResponse>()
        .map(|r| r.hits)
        .map_err(|e| format!("Lỗi đọc kết quả Modrinth: {}", e))
}

fn fetch_versions(
    client: &Client,
    project: &str,
    loaders: Option<&str>,
    mc_version: Option<&str>,
) -> Result<Vec<ProjectVersion>, String> {
    let mut query: Vec<(&str, String)> = Vec::new();
    if let Some(l) = loaders {
        query.push(("loaders", l.to_string()));
    }
    if let Some(v) = mc_version {
        query.push(("game_versions", format!("[\"{}\"]", v)));
    }
    let resp = client
        .get(format!("{}/project/{}/version", API, project))
        .header("User-Agent", UA)
        .query(&query)
        .timeout(Duration::from_secs(20))
        .send()
        .map_err(|e| format!("Không kết nối được Modrinth: {}", e))?;
    if !resp.status().is_success() {
        return Err(format!("Không tìm thấy '{}' trên Modrinth (HTTP {})", project, resp.status()));
    }
    resp.json().map_err(|e| format!("Lỗi đọc phiên bản '{}': {}", project, e))
}

/// Installs a mod / resource pack / shader for the given Minecraft version.
/// Mods also pull their required dependencies automatically.
pub fn install_project(
    client: &Client,
    kind: ContentKind,
    project: &str,
    mc_version: &str,
    game_dir: &Path,
    status: &Arc<Mutex<String>>,
) -> Result<String, String> {
    let target = game_dir.join(kind.folder());
    fs::create_dir_all(&target).map_err(|e| e.to_string())?;
    let mut visited = HashSet::new();
    install_recursive(client, kind, project, mc_version, &target, status, &mut visited)
}

fn compatible_mc_fallbacks(mc_version: &str) -> Vec<&'static str> {
    match mc_version {
        "1.21.1" => vec!["1.21"],
        "1.21.3" => vec!["1.21.2"],
        "1.20.6" => vec!["1.20.5"],
        "1.20.4" => vec!["1.20.3", "1.20.2"],
        "1.20.1" => vec!["1.20"],
        "1.19.4" => vec!["1.19.3"],
        "1.19.2" => vec!["1.19.1", "1.19"],
        "1.18.2" => vec!["1.18.1"],
        "1.16.5" => vec!["1.16.4"],
        _ => Vec::new(),
    }
}

fn shared_mod_cache_dir(target_mods_dir: &Path, mc_version: &str) -> Option<PathBuf> {
    // target_mods_dir is `<data_dir>/instances/<id>/.minecraft/mods`
    let data_dir = target_mods_dir.parent()?.parent()?.parent()?.parent()?;
    Some(data_dir.join("shared").join("mod_cache").join(mc_version))
}

fn install_recursive(
    client: &Client,
    kind: ContentKind,
    project: &str,
    mc_version: &str,
    target: &Path,
    status: &Arc<Mutex<String>>,
    visited: &mut HashSet<String>,
) -> Result<String, String> {
    if !visited.insert(project.to_string()) {
        return Ok(String::new());
    }
    if kind == ContentKind::Mod && project == "inventoryhud" {
        return install_inventory_hud(client, mc_version, target, status);
    }
    let loaders = match kind {
        ContentKind::Mod => "[\"fabric\"]",
        ContentKind::ResourcePack => "[\"minecraft\"]",
        ContentKind::Shader => "[\"iris\"]",
    };
    let mut versions = fetch_versions(client, project, Some(loaders), Some(mc_version)).unwrap_or_default();
    if versions.is_empty() && kind == ContentKind::Mod {
        for fb in compatible_mc_fallbacks(mc_version) {
            versions = fetch_versions(client, project, Some(loaders), Some(fb)).unwrap_or_default();
            if !versions.is_empty() {
                break;
            }
        }
    }
    if versions.is_empty() && kind == ContentKind::Mod && project == "modernfix" {
        // ModernFix-mVUS provides officially maintained builds for 1.21.4..1.21.11 & 26.x
        versions = fetch_versions(client, "modernfix-mvus", Some(loaders), Some(mc_version)).unwrap_or_default();
        if versions.is_empty() {
            for fb in compatible_mc_fallbacks(mc_version) {
                versions = fetch_versions(client, "modernfix-mvus", Some(loaders), Some(fb)).unwrap_or_default();
                if !versions.is_empty() {
                    break;
                }
            }
        }
    }
    if versions.is_empty() && kind == ContentKind::Shader {
        // Many shader packs work on every version even if not tagged
        versions = fetch_versions(client, project, Some(loaders), None)?;
    }
    let Some(ver) = versions.first() else {
        return Err(format!("'{}' chưa có bản cho Minecraft {}", project, mc_version));
    };

    if kind == ContentKind::Mod {
        for dep in &ver.dependencies {
            if dep.dependency_type == "required" {
                if let Some(id) = &dep.project_id {
                    let _ = install_recursive(client, kind, id, mc_version, target, status, visited);
                }
            }
        }
    }

    let file = ver
        .files
        .iter()
        .find(|f| f.primary.unwrap_or(false))
        .or_else(|| ver.files.first())
        .ok_or_else(|| format!("'{}' không có file tải về", project))?;

    let dest = target.join(&file.filename);
    if !dest.exists() {
        let cache_file = if kind == ContentKind::Mod {
            shared_mod_cache_dir(target, mc_version).map(|d| d.join(&file.filename))
        } else {
            None
        };
        if let Some(ref cf) = cache_file {
            if cf.exists() {
                let _ = fs::copy(cf, &dest);
            }
        }
        if !dest.exists() {
            set_status(status, &format!("Đang tải {}...", file.filename));
            let sha1 = file.hashes.as_ref().and_then(|h| h.sha1.as_deref());
            download_file_verified(client, &file.url, &dest, sha1)?;
            if let Some(ref cf) = cache_file {
                if let Some(parent) = cf.parent() {
                    let _ = fs::create_dir_all(parent);
                }
                let _ = fs::copy(&dest, cf);
            }
        }
    }
    Ok(file.filename.clone())
}

/// Installs a list of mods; returns (installed, failed) names
pub fn install_mod_list(
    client: &Client,
    slugs: &[(&str, &str)],
    mc_version: &str,
    game_dir: &Path,
    status: &Arc<Mutex<String>>,
) -> (Vec<String>, Vec<String>) {
    let mut ok = Vec::new();
    let mut failed = Vec::new();
    for (slug, label) in slugs {
        set_status(status, &format!("Đang cài {}...", label));
        match install_project(client, ContentKind::Mod, slug, mc_version, game_dir, status) {
            Ok(_) => ok.push(label.to_string()),
            Err(_) => failed.push(label.to_string()),
        }
    }
    // For older Minecraft versions (1.16..1.20.4), automatically pull Indium/LazyDFU/Starlight if available
    if mc_version.starts_with("1.16")
        || mc_version.starts_with("1.17")
        || mc_version.starts_with("1.18")
        || mc_version.starts_with("1.19")
        || mc_version.starts_with("1.20.1")
        || mc_version.starts_with("1.20.2")
        || mc_version.starts_with("1.20.4")
    {
        for (slug, label) in LEGACY_EXTRA_MODS {
            if install_project(client, ContentKind::Mod, slug, mc_version, game_dir, status).is_ok() {
                ok.push(label.to_string());
            }
        }
    }
    (ok, failed)
}

pub fn list_installed(kind: ContentKind, game_dir: &Path) -> Vec<InstalledItem> {
    let dir = game_dir.join(kind.folder());
    let mut list = Vec::new();
    let Ok(entries) = fs::read_dir(&dir) else {
        return list;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()).map(|s| s.to_string()) else {
            continue;
        };
        let is_dir = path.is_dir();
        let enabled = match kind {
            ContentKind::Mod => {
                if name.ends_with(".jar") {
                    true
                } else if name.ends_with(".jar.disabled") {
                    false
                } else {
                    continue;
                }
            }
            _ => {
                if !(is_dir || name.ends_with(".zip")) {
                    continue;
                }
                true
            }
        };
        let size_kb = if is_dir { 0 } else { entry.metadata().map(|m| m.len() / 1024).unwrap_or(0) };
        list.push(InstalledItem { name, path, enabled, size_kb, is_dir });
    }
    list.sort_by_key(|a| a.name.to_lowercase());
    list
}

pub fn matches_mod_slug(filename: &str, slug: &str) -> bool {
    let lower = filename.to_lowercase();
    let s = slug.to_lowercase();
    if s == "sodium" {
        return lower.starts_with("sodium-") && !lower.starts_with("sodium-extra");
    }
    let clean_file = lower.replace(['-', '_', '.'], "");
    let clean_slug = s.replace(['-', '_', '.'], "");
    clean_file.contains(&clean_slug)
}

pub fn has_mod(game_dir: &Path, needle: &str) -> bool {
    list_installed(ContentKind::Mod, game_dir)
        .iter()
        .any(|m| m.enabled && matches_mod_slug(&m.name, needle))
}

/// Returns a friendly Vietnamese title + description for known optimization/utility mod JARs
pub fn describe_mod_file(filename: &str) -> Option<&'static str> {
    for (slug, label) in FPS_PACK.iter().chain(LEGACY_EXTRA_MODS.iter()) {
        if matches_mod_slug(filename, slug) {
            return Some(*label);
        }
    }
    if matches_mod_slug(filename, DH_SLUG) {
        return Some("Distant Horizons – Vẽ địa hình giả lập LOD tầm xa (tới 1024 chunk)");
    }
    None
}

fn install_inventory_hud(
    client: &Client,
    mc_version: &str,
    target: &Path,
    status: &Arc<Mutex<String>>,
) -> Result<String, String> {
    #[derive(Deserialize)]
    struct CfWidget {
        files: Vec<CfFile>,
    }
    #[derive(Deserialize)]
    struct CfFile {
        id: u64,
        name: String,
        #[serde(default)]
        versions: Vec<String>,
    }
    let resp = client
        .get("https://api.cfwidget.com/357540")
        .header("User-Agent", UA)
        .timeout(Duration::from_secs(20))
        .send()
        .map_err(|e| format!("Không kết nối được kho Inventory HUD+: {}", e))?;
    let mut files = resp
        .json::<CfWidget>()
        .map_err(|e| format!("Lỗi đọc phiên bản Inventory HUD+: {}", e))?
        .files;
    files.sort_by(|a, b| b.id.cmp(&a.id));
    let needle = format!(".{}-", mc_version);
    let file = files
        .into_iter()
        .find(|f| {
            let n = f.name.to_lowercase();
            n.contains("fabric") && (n.contains(&needle) || f.versions.iter().any(|v| v == mc_version))
        })
        .ok_or_else(|| format!("Inventory HUD+ chưa có bản cho Minecraft {}", mc_version))?;
    let dest = target.join(&file.name);
    if !dest.exists() {
        set_status(status, &format!("Đang tải {}...", file.name));
        let url = format!(
            "https://edge.forgecdn.net/files/{}/{}/{}",
            file.id / 1000,
            file.id % 1000,
            file.name
        );
        download_file_verified(client, &url, &dest, None)?;
    }
    Ok(file.name)
}

pub fn toggle_mod(item: &InstalledItem) -> Result<(), String> {
    let new_name = if item.enabled {
        format!("{}.disabled", item.name)
    } else {
        item.name.trim_end_matches(".disabled").to_string()
    };
    let new_path = item.path.parent().unwrap_or_else(|| Path::new(".")).join(new_name);
    fs::rename(&item.path, new_path).map_err(|e| format!("Không đổi được trạng thái: {}", e))
}

pub fn delete_item(item: &InstalledItem) -> Result<(), String> {
    if item.is_dir {
        fs::remove_dir_all(&item.path).map_err(|e| e.to_string())
    } else {
        fs::remove_file(&item.path).map_err(|e| e.to_string())
    }
}

fn set_status(status: &Arc<Mutex<String>>, msg: &str) {
    if let Ok(mut g) = status.lock() {
        *g = msg.to_string();
    }
}
