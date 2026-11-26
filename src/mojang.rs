use rayon::prelude::*;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

const VERSION_MANIFEST_URLS: &[&str] = &[
    "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json",
    "https://launchermeta.mojang.com/mc/game/version_manifest_v2.json",
    "https://bmclapi2.bangbang93.com/mc/game/version_manifest_v2.json",
];

/// Builds an HTTP client bound to IPv4 (`0.0.0.0`) so it never hangs or fails on broken ISP IPv6 (`AAAA`) routes.
pub fn build_http_client(timeout_secs: u64) -> Result<Client, String> {
    let mut builder = Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(timeout_secs))
        .pool_max_idle_per_host(32)
        .pool_idle_timeout(Duration::from_secs(60))
        .tcp_nodelay(true)
        .tcp_keepalive(Duration::from_secs(30))
        .user_agent("VN-Launcher/2.4");
    #[cfg(windows)]
    {
        builder = builder.local_address(Some(IpAddr::V4(Ipv4Addr::UNSPECIFIED)));
    }
    builder
        .build()
        .map_err(|e| format!("Không khởi tạo được kết nối mạng: {}", e))
}

/// Converts an official Mojang/Fabric download URL to a high-speed fallback mirror URL when primary CDN fails.
pub fn mirror_mojang_url(url: &str) -> Option<String> {
    let pairs = [
        ("https://piston-meta.mojang.com", "https://bmclapi2.bangbang93.com"),
        ("https://launchermeta.mojang.com", "https://bmclapi2.bangbang93.com"),
        ("https://piston-data.mojang.com", "https://bmclapi2.bangbang93.com"),
        ("https://launcher.mojang.com", "https://bmclapi2.bangbang93.com"),
        ("https://libraries.minecraft.net", "https://bmclapi2.bangbang93.com/maven"),
        ("https://resources.download.minecraft.net", "https://bmclapi2.bangbang93.com/assets"),
        ("https://meta.fabricmc.net", "https://bmclapi2.bangbang93.com/fabric-meta"),
        ("https://maven.fabricmc.net", "https://bmclapi2.bangbang93.com/maven"),
    ];
    for (from, to) in pairs {
        if let Some(rest) = url.strip_prefix(from) {
            return Some(format!("{}{}", to, rest));
        }
    }
    None
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct VersionManifest {
    pub latest: LatestVersions,
    pub versions: Vec<ManifestVersionEntry>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LatestVersions {
    pub release: String,
    pub snapshot: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ManifestVersionEntry {
    pub id: String,
    #[serde(rename = "type")]
    pub version_type: String,
    pub url: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct VersionInfo {
    pub id: String,
    #[serde(rename = "mainClass")]
    pub main_class: String,
    #[serde(rename = "assetIndex")]
    pub asset_index: AssetIndexRef,
    pub assets: Option<String>,
    pub downloads: VersionDownloads,
    pub libraries: Vec<Library>,
    #[serde(rename = "javaVersion")]
    pub java_version: Option<JavaVersionInfo>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct JavaVersionInfo {
    #[serde(rename = "majorVersion")]
    pub major_version: u32,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AssetIndexRef {
    pub id: String,
    pub sha1: String,
    pub size: u64,
    pub url: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct VersionDownloads {
    pub client: DownloadArtifact,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DownloadArtifact {
    pub sha1: Option<String>,
    pub size: Option<u64>,
    pub url: String,
    pub path: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Library {
    pub name: String,
    pub downloads: Option<LibraryDownloads>,
    pub url: Option<String>,
    pub rules: Option<Vec<OsRule>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LibraryDownloads {
    pub artifact: Option<DownloadArtifact>,
    pub classifiers: Option<HashMap<String, DownloadArtifact>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OsRule {
    pub action: String,
    pub os: Option<OsConstraint>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OsConstraint {
    pub name: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AssetIndexFile {
    pub objects: HashMap<String, AssetObject>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AssetObject {
    pub hash: String,
    pub size: u64,
}

pub struct PreparedVanilla {
    pub version_info: VersionInfo,
    pub client_jar: PathBuf,
    pub classpath_jars: Vec<(String, PathBuf)>,
    pub natives_dir: PathBuf,
    pub assets_dir: PathBuf,
    pub asset_index_id: String,
}

pub fn fetch_version_manifest(client: &Client) -> Result<VersionManifest, String> {
    let mut last_err = String::new();
    for url in VERSION_MANIFEST_URLS {
        for attempt in 1..=2 {
            match client
                .get(*url)
                .header("User-Agent", "VN-Launcher/1.0")
                .timeout(Duration::from_secs(15))
                .send()
            {
                Ok(resp) if resp.status().is_success() => {
                    match resp.json::<VersionManifest>() {
                        Ok(m) if !m.versions.is_empty() => return Ok(m),
                        Ok(_) => last_err = "Danh sách phiên bản trả về trống".to_string(),
                        Err(e) => last_err = format!("Lỗi đọc JSON danh sách phiên bản Mojang: {}", e),
                    }
                }
                Ok(resp) => {
                    last_err = format!("HTTP {} từ {}", resp.status(), url);
                }
                Err(e) => {
                    last_err = format!("Lỗi tải danh sách phiên bản từ Mojang: {}", e);
                }
            }
            if attempt == 1 {
                thread::sleep(Duration::from_millis(250));
            }
        }
    }
    Err(last_err)
}

pub fn fetch_text_with_fallback(client: &Client, url: &str) -> Result<String, String> {
    let mut candidates = vec![url.to_string()];
    if let Some(mirror) = mirror_mojang_url(url) {
        candidates.push(mirror);
    }
    let mut last_err = String::new();
    for candidate in &candidates {
        for attempt in 1..=2 {
            match client
                .get(candidate)
                .header("User-Agent", "VN-Launcher/1.0")
                .timeout(Duration::from_secs(20))
                .send()
            {
                Ok(resp) if resp.status().is_success() => {
                    match resp.text() {
                        Ok(txt) if !txt.trim().is_empty() => return Ok(txt),
                        Ok(_) => last_err = format!("Dữ liệu rỗng từ {}", candidate),
                        Err(e) => last_err = e.to_string(),
                    }
                }
                Ok(resp) => last_err = format!("HTTP {} từ {}", resp.status(), candidate),
                Err(e) => last_err = e.to_string(),
            }
            if attempt == 1 {
                thread::sleep(Duration::from_millis(250));
            }
        }
    }
    Err(last_err)
}

pub fn current_mojang_os() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "osx"
    } else {
        "linux"
    }
}

/// Evaluates Mojang OS rules for the current OS (Windows, Linux, macOS)
pub fn is_library_allowed_on_current_os(rules: &Option<Vec<OsRule>>) -> bool {
    let Some(rule_list) = rules else {
        return true;
    };
    if rule_list.is_empty() {
        return true;
    }
    let cur = current_mojang_os();
    let mut allowed = false;
    for rule in rule_list {
        let matches_os = match &rule.os {
            None => true,
            Some(os) => match &os.name {
                None => true,
                Some(name) => name == cur,
            },
        };
        if matches_os {
            allowed = rule.action == "allow";
        }
    }
    allowed
}

fn classify_native_for_current_os(lib_name: &str) -> (bool, bool) {
    if !lib_name.contains(":natives-") {
        return (true, false);
    }
    if cfg!(target_os = "windows") {
        let ok = lib_name.ends_with(":natives-windows");
        (ok, ok)
    } else if cfg!(target_os = "macos") {
        let is_mac = lib_name.contains(":natives-macos") || lib_name.contains(":natives-osx");
        let is_arm = lib_name.ends_with("-arm64");
        let ok = is_mac && (cfg!(target_arch = "aarch64") || !is_arm);
        (ok, ok)
    } else {
        let is_linux = lib_name.contains(":natives-linux");
        let is_arm = lib_name.ends_with("-arm64");
        let ok = is_linux && (cfg!(target_arch = "aarch64") == is_arm || !is_arm);
        (ok, ok)
    }
}

/// Converts Maven coordinates `group:artifact:version[:classifier]` to a relative path
pub fn maven_coord_to_path(coord: &str) -> Option<String> {
    let parts: Vec<&str> = coord.split(':').collect();
    if parts.len() < 3 {
        return None;
    }
    let group_path = parts[0].replace('.', "/");
    let artifact = parts[1];
    let version = parts[2];
    if parts.len() >= 4 {
        let classifier = parts[3];
        Some(format!(
            "{}/{}/{}/{}-{}-{}.jar",
            group_path, artifact, version, artifact, version, classifier
        ))
    } else {
        Some(format!(
            "{}/{}/{}/{}-{}.jar",
            group_path, artifact, version, artifact, version
        ))
    }
}

/// Downloads and verifies Minecraft Vanilla client, libraries, natives, and assets using parallel threads.
pub fn ensure_vanilla_version(
    client: &Client,
    shared_dir: &Path,
    mc_version: &str,
    status_log: &Arc<Mutex<String>>,
) -> Result<PreparedVanilla, String> {
    let versions_dir = shared_dir.join("versions").join(mc_version);
    let libraries_dir = shared_dir.join("libraries");
    let assets_dir = shared_dir.join("assets");
    let natives_dir = versions_dir.join("natives");

    fs::create_dir_all(&versions_dir).map_err(|e| e.to_string())?;
    fs::create_dir_all(&libraries_dir).map_err(|e| e.to_string())?;
    fs::create_dir_all(&assets_dir).map_err(|e| e.to_string())?;
    fs::create_dir_all(&natives_dir).map_err(|e| e.to_string())?;

    let version_json_path = versions_dir.join(format!("{}.json", mc_version));
    let version_info: VersionInfo = if version_json_path.exists() {
        let raw = fs::read_to_string(&version_json_path).map_err(|e| e.to_string())?;
        serde_json::from_str(&raw).map_err(|e| format!("Lỗi đọc {}.json: {}", mc_version, e))?
    } else {
        set_status(
            status_log,
            &format!("🌐 Đang lấy thông tin phiên bản Minecraft {}...", mc_version),
        );
        let manifest_cache = shared_dir.join("version_manifest_v2.json");
        let manifest = match fetch_version_manifest(client) {
            Ok(m) => {
                if let Ok(json_str) = serde_json::to_string(&m) {
                    let _ = fs::write(&manifest_cache, json_str);
                }
                m
            }
            Err(e) => {
                if let Ok(cached_raw) = fs::read_to_string(&manifest_cache) {
                    serde_json::from_str::<VersionManifest>(&cached_raw).map_err(|_| e)?
                } else {
                    return Err(e);
                }
            }
        };
        let entry = manifest
            .versions
            .iter()
            .find(|v| v.id == mc_version)
            .ok_or_else(|| format!("Không tìm thấy phiên bản Minecraft '{}'", mc_version))?;

        let raw = fetch_text_with_fallback(client, &entry.url)?;

        fs::write(&version_json_path, &raw).map_err(|e| e.to_string())?;
        serde_json::from_str(&raw).map_err(|e| format!("Lỗi phân tích {}.json: {}", mc_version, e))?
    };

    // 1. Download Client JAR
    let client_jar = versions_dir.join(format!("{}.jar", mc_version));
    if !verify_file_sha1(&client_jar, version_info.downloads.client.sha1.as_deref()) {
        set_status(
            status_log,
            &format!("⬇️ Đang tải Minecraft Client {}.jar...", mc_version),
        );
        download_file_verified(
            client,
            &version_info.downloads.client.url,
            &client_jar,
            version_info.downloads.client.sha1.as_deref(),
        )?;
    }

    // 2. Collect Libraries & Natives for Windows
    #[derive(Clone)]
    struct DownloadTask {
        url: String,
        dest: PathBuf,
        sha1: Option<String>,
        is_native_zip: bool,
    }

    let mut tasks = Vec::new();
    let mut classpath_jars = Vec::new();

    for lib in &version_info.libraries {
        if !is_library_allowed_on_current_os(&lib.rules) {
            continue;
        }
        let (native_allowed, is_native) = classify_native_for_current_os(&lib.name);
        if !native_allowed {
            continue;
        }
        let ga = crate::fabric::extract_group_artifact(&lib.name);

        if let Some(downloads) = &lib.downloads {
            if let Some(artifact) = &downloads.artifact {
                let rel_path = artifact
                    .path
                    .clone()
                    .or_else(|| maven_coord_to_path(&lib.name))
                    .unwrap_or_else(|| format!("unknown/{}.jar", lib.name.replace(':', "_")));
                let dest = libraries_dir.join(&rel_path);
                classpath_jars.push((ga.clone(), dest.clone()));
                if !verify_file_sha1(&dest, artifact.sha1.as_deref()) {
                    tasks.push(DownloadTask {
                        url: artifact.url.clone(),
                        dest,
                        sha1: artifact.sha1.clone(),
                        is_native_zip: is_native,
                    });
                } else if is_native {
                    let _ = extract_native_jar(&dest, &natives_dir);
                }
            }
            if let Some(classifiers) = &downloads.classifiers {
                let native_opt = if cfg!(target_os = "windows") {
                    classifiers.get("natives-windows")
                } else if cfg!(target_os = "macos") {
                    classifiers
                        .get("natives-macos")
                        .or_else(|| classifiers.get("natives-osx"))
                } else {
                    classifiers.get("natives-linux")
                };
                if let Some(native_art) = native_opt {
                    let rel_path = native_art
                        .path
                        .clone()
                        .unwrap_or_else(|| format!("natives/{}.jar", lib.name.replace(':', "_")));
                    let dest = libraries_dir.join(&rel_path);
                    if !verify_file_sha1(&dest, native_art.sha1.as_deref()) {
                        tasks.push(DownloadTask {
                            url: native_art.url.clone(),
                            dest,
                            sha1: native_art.sha1.clone(),
                            is_native_zip: true,
                        });
                    } else {
                        let _ = extract_native_jar(&dest, &natives_dir);
                    }
                }
            }
        } else if let Some(base_url) = &lib.url {
            if let Some(rel_path) = maven_coord_to_path(&lib.name) {
                let dest = libraries_dir.join(&rel_path);
                classpath_jars.push((ga, dest.clone()));
                if !dest.exists() {
                    let full_url = format!("{}{}", base_url.trim_end_matches('/'), format!("/{}", rel_path));
                    tasks.push(DownloadTask {
                        url: full_url,
                        dest,
                        sha1: None,
                        is_native_zip: false,
                    });
                }
            }
        }
    }

    let io_pool = rayon::ThreadPoolBuilder::new()
        .num_threads(24)
        .build()
        .ok();

    if !tasks.is_empty() {
        let total_libs = tasks.len();
        let done_libs = AtomicUsize::new(0);
        set_status(
            status_log,
            &format!("📚 Đang tải tốc độ cao {} thư viện Minecraft...", total_libs),
        );

        let run_libs = || -> Vec<String> {
            tasks
                .par_iter()
                .filter_map(|task| {
                    let res = download_file_verified(client, &task.url, &task.dest, task.sha1.as_deref());
                    if res.is_ok() && task.is_native_zip {
                        let _ = extract_native_jar(&task.dest, &natives_dir);
                    }
                    let finished = done_libs.fetch_add(1, Ordering::Relaxed) + 1;
                    if finished % 5 == 0 || finished == total_libs {
                        set_status(
                            status_log,
                            &format!("📚 Đang tải thư viện: {}/{}", finished, total_libs),
                        );
                    }
                    res.err()
                })
                .collect()
        };
        let errors = match &io_pool {
            Some(pool) => pool.install(run_libs),
            None => run_libs(),
        };

        if let Some(first_err) = errors.first() {
            return Err(format!("Lỗi tải thư viện Minecraft: {}", first_err));
        }
    }

    // 3. Download Asset Index & Assets in Parallel
    let indexes_dir = assets_dir.join("indexes");
    let objects_dir = assets_dir.join("objects");
    fs::create_dir_all(&indexes_dir).map_err(|e| e.to_string())?;
    fs::create_dir_all(&objects_dir).map_err(|e| e.to_string())?;

    let asset_index_path = indexes_dir.join(format!("{}.json", version_info.asset_index.id));
    if !verify_file_sha1(&asset_index_path, Some(&version_info.asset_index.sha1)) {
        set_status(
            status_log,
            &format!("🎨 Đang tải chỉ mục tài nguyên ({})...", version_info.asset_index.id),
        );
        download_file_verified(
            client,
            &version_info.asset_index.url,
            &asset_index_path,
            Some(&version_info.asset_index.sha1),
        )?;
    }

    let index_raw = fs::read_to_string(&asset_index_path).map_err(|e| e.to_string())?;
    let asset_index_file: AssetIndexFile =
        serde_json::from_str(&index_raw).map_err(|e| format!("Lỗi đọc asset index: {}", e))?;

    let mut missing_assets: Vec<(String, PathBuf)> = Vec::new();
    for obj in asset_index_file.objects.values() {
        let prefix = &obj.hash[0..2];
        let obj_path = objects_dir.join(prefix).join(&obj.hash);
        if !obj_path.exists() {
            let url = format!(
                "https://resources.download.minecraft.net/{}/{}",
                prefix, obj.hash
            );
            missing_assets.push((url, obj_path));
        }
    }

    // Deduplicate by destination path
    missing_assets.sort_by(|a, b| a.1.cmp(&b.1));
    missing_assets.dedup_by(|a, b| a.1 == b.1);

    if !missing_assets.is_empty() {
        // Pre-create 2-char hex subdirectories once so 24 worker threads skip redundant create_dir_all syscalls
        let mut prefixes: Vec<&str> = missing_assets
            .iter()
            .filter_map(|(_, p)| p.parent().and_then(|d| d.file_name()).and_then(|n| n.to_str()))
            .collect();
        prefixes.sort_unstable();
        prefixes.dedup();
        for p in prefixes {
            let _ = fs::create_dir_all(objects_dir.join(p));
        }

        let total_assets = missing_assets.len();
        let done_assets = AtomicUsize::new(0);
        set_status(
            status_log,
            &format!("🎨 Đang tải tốc độ cao {} tài nguyên âm thanh & hình ảnh...", total_assets),
        );

        let run_assets = || -> Vec<String> {
            missing_assets
                .par_iter()
                .filter_map(|(url, dest)| {
                    let res = download_file_verified(client, url, dest, None);
                    let finished = done_assets.fetch_add(1, Ordering::Relaxed) + 1;
                    if finished % 50 == 0 || finished == total_assets {
                        set_status(
                            status_log,
                            &format!("🎨 Đang tải tài nguyên (Assets): {}/{}", finished, total_assets),
                        );
                    }
                    res.err()
                })
                .collect()
        };
        let asset_errors = match &io_pool {
            Some(pool) => pool.install(run_assets),
            None => run_assets(),
        };

        if let Some(first_err) = asset_errors.first() {
            return Err(format!("Lỗi tải tài nguyên Minecraft: {}", first_err));
        }
    }

    Ok(PreparedVanilla {
        asset_index_id: version_info.asset_index.id.clone(),
        version_info,
        client_jar,
        classpath_jars,
        natives_dir,
        assets_dir,
    })
}

pub fn verify_file_sha1(path: &Path, expected_sha1: Option<&str>) -> bool {
    if !path.exists() {
        return false;
    }
    let Some(expected) = expected_sha1 else {
        return path.metadata().map(|m| m.len() > 0).unwrap_or(false);
    };
    let Ok(mut file) = File::open(path) else {
        return false;
    };
    let mut hasher = Sha1::new();
    let mut buf = [0u8; 32768];
    loop {
        let Ok(n) = file.read(&mut buf) else {
            return false;
        };
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let digest = format!("{:x}", hasher.finalize());
    digest.eq_ignore_ascii_case(expected)
}

pub fn download_file_verified(
    client: &Client,
    url: &str,
    dest: &Path,
    expected_sha1: Option<&str>,
) -> Result<(), String> {
    if let Some(parent) = dest.parent() {
        if !parent.exists() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
    }

    let mirror_url = mirror_mojang_url(url);
    for attempt in 1..=4 {
        let target_url = if attempt % 2 == 0 {
            mirror_url.as_deref().unwrap_or(url)
        } else {
            url
        };
        let resp_res = client
            .get(target_url)
            .header("User-Agent", "VN-Launcher/2.3")
            .timeout(Duration::from_secs(90))
            .send();

        match resp_res {
            Ok(mut resp) if resp.status().is_success() => {
                let tmp_path = dest.with_extension("part");
                let mut file = File::create(&tmp_path).map_err(|e| e.to_string())?;
                let mut buf = [0u8; 65536];
                let mut copy_ok = true;
                loop {
                    match resp.read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => {
                            if file.write_all(&buf[..n]).is_err() {
                                copy_ok = false;
                                break;
                            }
                        }
                        Err(_) => {
                            copy_ok = false;
                            break;
                        }
                    }
                }
                drop(file);
                if copy_ok && verify_file_sha1(&tmp_path, expected_sha1) {
                    let _ = fs::rename(&tmp_path, dest);
                    return Ok(());
                }
                let _ = fs::remove_file(&tmp_path);
            }
            _ => {}
        }

        if attempt == 4 {
            return Err(format!("Không thể tải file từ URL: {}", url));
        }
        thread::sleep(Duration::from_millis(200));
    }
    Ok(())
}

fn extract_native_jar(jar_path: &Path, natives_dir: &Path) -> Result<(), String> {
    let file = File::open(jar_path).map_err(|e| e.to_string())?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        let Some(name) = entry.enclosed_name() else {
            continue;
        };
        let file_name = match name.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };
        if file_name.ends_with(".dll") {
            let out_path = natives_dir.join(&file_name);
            if !out_path.exists() {
                if let Ok(mut out) = File::create(&out_path) {
                    let _ = std::io::copy(&mut entry, &mut out);
                }
            }
        }
    }
    Ok(())
}

fn set_status(status_log: &Arc<Mutex<String>>, msg: &str) {
    if let Ok(mut guard) = status_log.lock() {
        *guard = msg.to_string();
    }
}
