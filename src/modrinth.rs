use crate::mojang::download_file_verified;
use rayon::prelude::*;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Read;
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
    Modpack,
}

impl ContentKind {
    pub fn label(&self) -> &'static str {
        match self {
            ContentKind::Mod => "Mod",
            ContentKind::ResourcePack => "Gói tài nguyên",
            ContentKind::Shader => "Shader",
            ContentKind::Modpack => "Modpack",
        }
    }
    pub fn project_type(&self) -> &'static str {
        match self {
            ContentKind::Mod => "mod",
            ContentKind::ResourcePack => "resourcepack",
            ContentKind::Shader => "shader",
            ContentKind::Modpack => "modpack",
        }
    }
    pub fn folder(&self) -> &'static str {
        match self {
            ContentKind::Mod => "mods",
            ContentKind::ResourcePack => "resourcepacks",
            ContentKind::Shader => "shaderpacks",
            ContentKind::Modpack => "modpacks",
        }
    }
    pub fn file_ext(&self) -> &'static str {
        match self {
            ContentKind::Mod => "jar",
            ContentKind::Modpack => "mrpack",
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
    ("simple-voice-chat", "Simple Voice Chat – Nói chuyện bằng Mic trực tiếp trong game (phím V)"),
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
pub struct FamousMod {
    pub slug: String,
    pub name: String,
    pub desc: String,
}

pub const FAMOUS_MODS_FALLBACK: &[(&str, &str, &str)] = &[
    ("jei", "Just Enough Items (JEI)", "Xem công thức chế tạo, nấu chảy & cách kiếm mọi vật phẩm"),
    ("xaeros-minimap", "Xaero's Minimap", "Bản đồ nhỏ góc màn hình, đánh dấu toạ độ nhà & chỗ chết"),
    ("xaeros-world-map", "Xaero's World Map", "Bản đồ thế giới toàn cảnh (bấm M) đồng bộ với Minimap"),
    ("jade", "Jade (WAILA)", "Chỉ tâm vào khối, rương hoặc quái là hiện tên & lượng máu"),
    ("zoomify", "Zoomify", "Phóng to tầm nhìn mượt mà (phím C) thay thế OptiFine Zoom"),
    ("continuity", "Continuity", "Nối liền viền kính & khối giống hệt OptiFine"),
    ("lambdynamiclights", "LambDynamicLights", "Cầm đuốc trên tay hoặc ném ra đất là tự phát sáng xung quanh"),
    ("mouse-tweaks", "Mouse Tweaks", "Giữ & kéo chuột chuyển đồ siêu nhanh trong rương/túi đồ"),
    ("item-scroller", "Item Scroller", "Lăn con lăn chuột để chuyển hoặc chế tạo hàng loạt vật phẩm"),
    ("3dskinlayers", "3D Skin Layers", "Biến lớp áo khoác, mũ, tóc của Skin thành khối nổi 3D cực đẹp"),
    ("not-enough-animations", "Not Enough Animations", "Thêm hoạt ảnh ăn uống, chèo thuyền, cầm bản đồ thật hơn"),
    ("visuality", "Visuality", "Hiệu ứng hạt lấp lánh khi đánh quái, đào quặng & đom đóm ban đêm"),
    ("sound-physics-remastered", "Sound Physics Remastered", "Hiệu ứng âm thanh vang dội trong hang động & dưới nước cực thật"),
    ("presence-footsteps", "Presence Footsteps", "Âm thanh bước chân chi tiết theo từng loại cỏ, đá, gỗ, tuyết"),
    ("chat-heads", "Chat Heads", "Hiện đầu nhân vật bên cạnh tên người chơi trong khung chat"),
    ("betterf3", "BetterF3", "Làm đẹp & tô màu bảng thông số F3 giúp dễ đọc FPS, toạ độ"),
    ("controlling", "Controlling", "Tìm kiếm phím tắt & kiểm tra trùng phím trong cài đặt điều khiển"),
    ("gamma-utils", "Gamma Utils (Fullbright)", "Bật sáng ban đêm & trong hang tối chỉ với 1 phím bấm"),
    ("clumps", "Clumps", "Gộp hàng trăm hạt kinh nghiệm XP thành 1 cục chống lag máy farm"),
    ("fallingtree", "FallingTree", "Chặt 1 khúc gỗ dưới gốc là đổ cả cây (tiện chơi sinh tồn)"),
    ("shulkerboxtooltip", "Shulker Box Tooltip", "Di chuột vào hộp Shulker là xem ngay đồ bên trong"),
    ("waystones", "Waystones", "Đặt cột đá dịch chuyển nhanh giữa các căn cứ trong sinh tồn"),
    ("travelersbackpack", "Traveler's Backpack", "Ba lô đeo lưng mở rộng túi đồ kèm bình chứa & túi ngủ"),
    ("litematica", "Litematica", "Hiện bản vẽ mẫu (Schematic) giúp xây công trình lớn chuẩn từng khối"),
    ("tweakeroo", "Tweakeroo", "Bộ công cụ hỗ trợ đặt khối, Redstone & sinh tồn chuyên nghiệp"),
    ("minihud", "MiniHUD", "Hiện gọn toạ độ, FPS, ranh giới Chunk & vùng quái spawn"),
    ("freecam", "Freecam", "Tách camera bay tự do quan sát công trình khi nhân vật đứng yên"),
    ("replaymod", "Replay Mod", "Ghi lại toàn bộ quá trình chơi để quay video góc nhìn điện ảnh"),
    ("worldedit", "WorldEdit", "Xây dựng, sao chép & tạo địa hình hàng loạt bằng lệnh"),
    ("axiom", "Axiom", "Công cụ xây dựng 3D & điêu khắc địa hình hiện đại cho Builder"),
    ("simple-voice-chat", "Simple Voice Chat", "Trò chuyện bằng mic trực tiếp trong game theo khoảng cách gần xa"),
    ("emotecraft", "Emotecraft", "Nhảy múa, vẫy tay & thực hiện các động tác biểu cảm nhân vật"),
];

pub const FAMOUS_PACKS_FALLBACK: &[(&str, &str, &str)] = &[
    ("fresh-animations", "Fresh Animations", "Hoạt ảnh quái, dân làng & động vật sống động như trailer Minecraft"),
    ("faithful-32x", "Faithful 32x", "Nâng độ phân giải khối & vật phẩm lên 32x sắc nét, giữ đúng chất gốc"),
    ("faithful-64x", "Faithful 64x", "Phiên bản 64x siêu chi tiết & sắc nét cho máy tầm trung - cao"),
    ("bare-bones", "Bare Bones", "Đồ hoạ phẳng tối giản giống hệt trailer chính thức, cực nhẹ & tăng FPS"),
    ("better-leaves", "Motschen's Better Leaves", "Làm tán lá cây xum xuê, tự nhiên mà vẫn giữ FPS cao"),
    ("xalis-enhanced-vanilla", "xali's Enhanced Vanilla", "Tăng chi tiết gạch đá, hoa cỏ, đèn lồng cực đẹp theo phong cách Vanilla"),
    ("default-dark-mode", "Default Dark Mode", "Đổi toàn bộ menu, rương, túi đồ sang nền tối (Dark Mode) dịu mắt"),
    ("clean-connected-glass", "Clean Connected Glass", "Kính trong suốt & nối liền viền gọn đẹp"),
    ("even-better-enchants", "Even Better Enchants", "Mỗi cuốn sách phù phép có hình vẽ riêng biệt cực dễ phân biệt"),
    ("icons", "Icons", "Thêm các biểu tượng nhỏ tinh tế vào nút bấm và menu trong game"),
];

pub const FAMOUS_SHADERS_FALLBACK: &[(&str, &str, &str)] = &[
    ("makeup-ultra-fast-shaders", "MakeUp - Ultra Fast", "Shader siêu nhẹ FPS cao nhất, phù hợp cả máy yếu & laptop văn phòng"),
    ("mellow", "Mellow", "Shader nhẹ nhàng, ánh nắng ấm áp và mặt nước mượt mà"),
    ("complementary-reimagined", "Complementary Reimagined", "Shader nổi tiếng nhất: giữ chất khối Minecraft nhưng ánh sáng & nước cực đẹp"),
    ("complementary-unbound", "Complementary Unbound", "Mây thể tích, nước phản chiếu chân thật & hiệu ứng bầu trời đỉnh cao"),
    ("bsl-shaders", "BSL Shaders", "Màu sắc rực rỡ, ánh nắng ấm & sương mù kinh điển"),
    ("sildurs-vibrant-shaders", "Sildur's Vibrant Shaders", "Màu sắc sống động, có sẵn nhiều mức từ Lite đến Extreme"),
    ("photon-shader", "Photon Shaders", "Ánh sáng bầu trời thực tế & mây nhiều tầng cực chân thật"),
    ("solas-shader", "Solas Shader", "Ánh sáng ban đêm, cực quang & hang động huyền ảo"),
    ("bliss-shader", "Bliss Shaders", "Hiệu ứng sương mù & thời tiết thay đổi theo vùng cực tự nhiên"),
    ("super-duper-vanilla", "Super Duper Vanilla", "Phong cách ánh sáng sắc nét giống trailer Minecraft"),
    ("rethinking-voxels", "Rethinking Voxels", "Ánh sáng dò tia (Path-tracing) bóng đổ siêu thực cho máy cấu hình mạnh"),
    ("insanity-shader", "Insanity Shader", "Không khí u ám, sương mù rùng rợn chuyên chơi map kinh dị"),
];

fn to_famous_vec(slice: &[(&str, &str, &str)]) -> Vec<FamousMod> {
    slice
        .iter()
        .map(|(slug, name, desc)| FamousMod {
            slug: (*slug).to_string(),
            name: (*name).to_string(),
            desc: (*desc).to_string(),
        })
        .collect()
}

pub fn default_famous_mods() -> Vec<FamousMod> {
    to_famous_vec(FAMOUS_MODS_FALLBACK)
}

pub fn default_famous_packs() -> Vec<FamousMod> {
    to_famous_vec(FAMOUS_PACKS_FALLBACK)
}

pub fn default_famous_shaders() -> Vec<FamousMod> {
    to_famous_vec(FAMOUS_SHADERS_FALLBACK)
}

/// Fetches the latest curated famous list (`famous_mods.json` / `famous_packs.json` / `famous_shaders.json`) from GitHub.
pub fn fetch_remote_famous_list(client: &Client, file_name: &str) -> Option<Vec<FamousMod>> {
    let url = format!("https://raw.githubusercontent.com/vietanh-1208/vn-launcher/main/{}", file_name);
    let resp = client
        .get(&url)
        .header("User-Agent", UA)
        .timeout(Duration::from_secs(10))
        .send()
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let list: Vec<FamousMod> = resp.json().ok()?;
    if list.is_empty() { None } else { Some(list) }
}

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

pub fn search(client: &Client, kind: ContentKind, query: &str, mc_version: &str, loader_slug: &str) -> Result<Vec<SearchHit>, String> {
    let loader = if loader_slug.is_empty() || loader_slug == "vanilla" { "fabric" } else { loader_slug };
    let facets = match kind {
        ContentKind::Mod | ContentKind::Modpack => format!(
            "[[\"project_type:{}\"],[\"versions:{}\"],[\"categories:{}\"]]",
            kind.project_type(),
            mc_version,
            loader
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
    install_project_for_loader(client, kind, project, mc_version, "fabric", game_dir, status)
}

pub fn install_project_for_loader(
    client: &Client,
    kind: ContentKind,
    project: &str,
    mc_version: &str,
    loader_slug: &str,
    game_dir: &Path,
    status: &Arc<Mutex<String>>,
) -> Result<String, String> {
    let target = game_dir.join(kind.folder());
    fs::create_dir_all(&target).map_err(|e| e.to_string())?;
    let mut visited = HashSet::new();
    let l = if loader_slug.is_empty() || loader_slug == "vanilla" { "fabric" } else { loader_slug };
    install_recursive(client, kind, project, mc_version, l, &target, status, &mut visited)
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
    loader_slug: &str,
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
        ContentKind::Mod | ContentKind::Modpack => format!("[\"{}\"]", loader_slug),
        ContentKind::ResourcePack => "[\"minecraft\"]".to_string(),
        ContentKind::Shader => "[\"iris\"]".to_string(),
    };
    let mut versions = fetch_versions(client, project, Some(&loaders), Some(mc_version)).unwrap_or_default();
    if versions.is_empty() && kind != ContentKind::Mod {
        versions = fetch_versions(client, project, Some(&loaders), None).unwrap_or_default();
        if versions.is_empty() {
            versions = fetch_versions(client, project, None, None).unwrap_or_default();
        }
    }
    if versions.is_empty() && kind == ContentKind::Mod {
        for fb in compatible_mc_fallbacks(mc_version) {
            versions = fetch_versions(client, project, Some(&loaders), Some(fb)).unwrap_or_default();
            if !versions.is_empty() {
                break;
            }
        }
    }
    if versions.is_empty() && kind == ContentKind::Mod && project == "modernfix" {
        // ModernFix-mVUS provides officially maintained builds for 1.21.4..1.21.11 & 26.x
        versions = fetch_versions(client, "modernfix-mvus", Some(&loaders), Some(mc_version)).unwrap_or_default();
        if versions.is_empty() {
            for fb in compatible_mc_fallbacks(mc_version) {
                versions = fetch_versions(client, "modernfix-mvus", Some(&loaders), Some(fb)).unwrap_or_default();
                if !versions.is_empty() {
                    break;
                }
            }
        }
    }
    if versions.is_empty() && kind == ContentKind::Shader {
        // Many shader packs work on every version even if not tagged
        versions = fetch_versions(client, project, Some(&loaders), None)?;
    }
    let Some(ver) = versions.first() else {
        return Err(format!("'{}' chưa có bản cho Minecraft {}", project, mc_version));
    };

    if kind == ContentKind::Mod {
        for dep in &ver.dependencies {
            if dep.dependency_type == "required" {
                if let Some(id) = &dep.project_id {
                    let _ = install_recursive(client, kind, id, mc_version, loader_slug, target, status, visited);
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
    for (slug, _name, desc) in FAMOUS_MODS_FALLBACK {
        if matches_mod_slug(filename, slug) {
            return Some(*desc);
        }
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

#[derive(Debug, Clone, Deserialize)]
struct MrpackIndex {
    #[serde(default)]
    name: String,
    #[serde(default)]
    dependencies: HashMap<String, String>,
    #[serde(default)]
    files: Vec<MrpackFile>,
}

#[derive(Debug, Clone, Deserialize)]
struct MrpackFile {
    path: String,
    #[serde(default)]
    downloads: Vec<String>,
    hashes: Option<Hashes>,
    env: Option<MrpackEnv>,
}

#[derive(Debug, Clone, Deserialize)]
struct MrpackEnv {
    client: Option<String>,
}

/// Installs a `.mrpack` Modrinth modpack file into `game_dir`.
/// Returns `(pack_name, mc_version, fabric_loader_version, installed_file_count)`.
pub fn install_mrpack_file(
    client: &Client,
    mrpack_path: &Path,
    game_dir: &Path,
    status: &Arc<Mutex<String>>,
) -> Result<(String, String, Option<String>, usize), String> {
    let f = fs::File::open(mrpack_path).map_err(|e| format!("Không mở được file .mrpack: {}", e))?;
    let mut archive = zip::ZipArchive::new(f).map_err(|e| format!("File .mrpack không hợp lệ: {}", e))?;

    let index: MrpackIndex = {
        let mut entry = archive
            .by_name("modrinth.index.json")
            .map_err(|_| "File .mrpack thiếu modrinth.index.json.".to_string())?;
        let mut raw = String::new();
        entry.read_to_string(&mut raw).map_err(|e| e.to_string())?;
        serde_json::from_str(&raw).map_err(|e| format!("Lỗi đọc modrinth.index.json: {}", e))?
    };

    let mc_version = index
        .dependencies
        .get("minecraft")
        .cloned()
        .unwrap_or_default();
    let fabric_loader = index.dependencies.get("fabric-loader").cloned();
    let pack_name = if index.name.trim().is_empty() {
        "Modpack".to_string()
    } else {
        index.name.clone()
    };

    fs::create_dir_all(game_dir).map_err(|e| e.to_string())?;
    let client_files: Vec<MrpackFile> = index
        .files
        .into_iter()
        .filter(|mf| {
            mf.env
                .as_ref()
                .and_then(|e| e.client.as_deref())
                .map(|c| c != "unsupported")
                .unwrap_or(true)
        })
        .collect();

    let total = client_files.len();
    set_status(status, &format!("Đang tải {} file của {}...", total, pack_name));

    let done_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let errs: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));

    client_files.par_iter().for_each(|mf| {
        let rel = mf.path.trim_start_matches(['/', '\\']);
        if rel.contains("..") {
            return;
        }
        let dest = game_dir.join(rel);
        if let Some(parent) = dest.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let sha1 = mf.hashes.as_ref().and_then(|h| h.sha1.as_deref());
        let mut ok = false;
        for url in &mf.downloads {
            if download_file_verified(client, url, &dest, sha1).is_ok() {
                ok = true;
                break;
            }
        }
        if !ok {
            if let Ok(mut e) = errs.lock() {
                e.push(rel.to_string());
            }
        }
        let n = done_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
        set_status(status, &format!("Đang cài {} ({}/{})...", pack_name, n, total));
    });

    // Extract overrides/ and client-overrides/
    set_status(status, &format!("Đang giải nén cấu hình {}...", pack_name));
    for i in 0..archive.len() {
        let Ok(mut entry) = archive.by_index(i) else { continue };
        let name = entry.name().replace('\\', "/");
        let rel_opt = name
            .strip_prefix("overrides/")
            .or_else(|| name.strip_prefix("client-overrides/"));
        let Some(rel) = rel_opt else { continue };
        if rel.is_empty() || rel.contains("..") {
            continue;
        }
        let dest = game_dir.join(rel);
        if entry.is_dir() {
            let _ = fs::create_dir_all(&dest);
        } else {
            if let Some(parent) = dest.parent() {
                let _ = fs::create_dir_all(parent);
            }
            if let Ok(mut out) = fs::File::create(&dest) {
                let _ = std::io::copy(&mut entry, &mut out);
            }
        }
    }

    let failed = errs.lock().map(|e| e.len()).unwrap_or(0);
    Ok((pack_name, mc_version, fabric_loader, total.saturating_sub(failed)))
}

pub fn install_mrpack_from_modrinth(
    client: &Client,
    project: &str,
    mc_version: &str,
    loader_slug: &str,
    game_dir: &Path,
    status: &Arc<Mutex<String>>,
) -> Result<(String, String, Option<String>, usize), String> {
    let l = format!("[\"{}\"]", if loader_slug.is_empty() || loader_slug == "vanilla" { "fabric" } else { loader_slug });
    let mut versions = fetch_versions(client, project, Some(&l), Some(mc_version)).unwrap_or_default();
    if versions.is_empty() {
        versions = fetch_versions(client, project, None, Some(mc_version)).unwrap_or_default();
    }
    if versions.is_empty() {
        versions = fetch_versions(client, project, None, None).unwrap_or_default();
    }
    let ver = versions
        .into_iter()
        .next()
        .ok_or_else(|| format!("Không tìm thấy bản tải của Modpack '{}'", project))?;
    let file = ver
        .files
        .iter()
        .find(|f| f.primary.unwrap_or(false))
        .or_else(|| ver.files.first())
        .ok_or_else(|| "Modpack không có file tải.".to_string())?;
    let tmp = std::env::temp_dir().join(&file.filename);
    set_status(status, &format!("Đang tải {}...", file.filename));
    download_file_verified(client, &file.url, &tmp, file.hashes.as_ref().and_then(|h| h.sha1.as_deref()))?;
    let res = install_mrpack_file(client, &tmp, game_dir, status);
    let _ = fs::remove_file(&tmp);
    res
}
