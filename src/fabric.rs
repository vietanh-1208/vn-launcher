use crate::mojang::{download_file_verified, fetch_text_with_fallback, maven_coord_to_path};
use rayon::prelude::*;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FabricLoaderEntry {
    pub loader: FabricLoaderVersion,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FabricLoaderVersion {
    pub version: String,
    pub stable: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FabricProfileJson {
    pub id: String,
    #[serde(rename = "mainClass")]
    pub main_class: String,
    pub libraries: Vec<FabricLibrary>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FabricLibrary {
    pub name: String,
    pub url: Option<String>,
    pub sha1: Option<String>,
}

pub struct PreparedFabric {
    #[allow(dead_code)]
    pub loader_version: String,
    pub main_class: String,
    pub fabric_jars: Vec<(String, PathBuf)>, // (group:artifact, jar_path)
}

pub fn ensure_fabric_loader(
    client: &Client,
    shared_dir: &Path,
    mc_version: &str,
    requested_loader: Option<&str>,
    status_log: &Arc<Mutex<String>>,
) -> Result<PreparedFabric, String> {
    let loader_version = match requested_loader {
        Some(v) if !v.trim().is_empty() => v.trim().to_string(),
        _ => fetch_latest_fabric_loader(client, mc_version, status_log)?,
    };

    let fabric_dir = shared_dir
        .join("fabric")
        .join(format!("{}-{}", mc_version, loader_version));
    let libraries_dir = shared_dir.join("libraries");
    fs::create_dir_all(&fabric_dir).map_err(|e| e.to_string())?;
    fs::create_dir_all(&libraries_dir).map_err(|e| e.to_string())?;

    let profile_path = fabric_dir.join("fabric-profile.json");
    let profile: FabricProfileJson = if profile_path.exists() {
        let raw = fs::read_to_string(&profile_path).map_err(|e| e.to_string())?;
        serde_json::from_str(&raw).map_err(|e| format!("Lỗi đọc fabric-profile.json: {}", e))?
    } else {
        set_status(
            status_log,
            &format!(
                "🧵 Đang tải cấu hình Fabric Loader {} cho MC {}...",
                loader_version, mc_version
            ),
        );
        let url = format!(
            "https://meta.fabricmc.net/v2/versions/loader/{}/{}/profile/json",
            mc_version, loader_version
        );
        let raw = fetch_text_with_fallback(client, &url)
            .map_err(|e| format!("Lỗi kết nối Fabric Meta API: {}", e))?;

        fs::write(&profile_path, &raw).map_err(|e| e.to_string())?;
        serde_json::from_str(&raw).map_err(|e| format!("Lỗi đọc JSON từ Fabric Meta: {}", e))?
    };

    #[derive(Clone)]
    struct FabricTask {
        url: String,
        dest: PathBuf,
        sha1: Option<String>,
    }

    let mut tasks = Vec::new();
    let mut fabric_jars = Vec::new();

    for lib in &profile.libraries {
        let Some(rel_path) = maven_coord_to_path(&lib.name) else {
            continue;
        };
        let dest = libraries_dir.join(&rel_path);
        let ga = extract_group_artifact(&lib.name);
        fabric_jars.push((ga, dest.clone()));

        if !dest.exists() {
            let base = lib
                .url
                .as_deref()
                .unwrap_or("https://maven.fabricmc.net/")
                .trim_end_matches('/');
            let full_url = format!("{}/{}", base, rel_path);
            tasks.push(FabricTask {
                url: full_url,
                dest,
                sha1: lib.sha1.clone(),
            });
        }
    }

    if !tasks.is_empty() {
        set_status(
            status_log,
            &format!("🧵 Đang tải {} thư viện Fabric Loader...", tasks.len()),
        );
        let errors: Vec<String> = tasks
            .par_iter()
            .filter_map(|t| download_file_verified(client, &t.url, &t.dest, t.sha1.as_deref()).err())
            .collect();

        if let Some(err) = errors.first() {
            return Err(format!("Lỗi tải thư viện Fabric: {}", err));
        }
    }

    Ok(PreparedFabric {
        loader_version,
        main_class: profile.main_class,
        fabric_jars,
    })
}

fn fetch_latest_fabric_loader(
    client: &Client,
    mc_version: &str,
    status_log: &Arc<Mutex<String>>,
) -> Result<String, String> {
    set_status(
        status_log,
        &format!("🧵 Đang kiểm tra Fabric Loader mới nhất cho MC {}...", mc_version),
    );
    let url = format!("https://meta.fabricmc.net/v2/versions/loader/{}", mc_version);
    let raw = fetch_text_with_fallback(client, &url)
        .map_err(|e| format!("Không thể kết nối Fabric Meta: {}", e))?;

    let entries: Vec<FabricLoaderEntry> = serde_json::from_str(&raw)
        .map_err(|e| format!("Lỗi đọc danh sách Fabric Loader: {}", e))?;

    if let Some(stable) = entries.iter().find(|e| e.loader.stable) {
        return Ok(stable.loader.version.clone());
    }
    entries
        .first()
        .map(|e| e.loader.version.clone())
        .ok_or_else(|| format!("Không tìm thấy bản Fabric Loader nào cho MC {}", mc_version))
}

pub fn extract_group_artifact(maven_coord: &str) -> String {
    let parts: Vec<&str> = maven_coord.split(':').collect();
    if parts.len() >= 4 {
        format!("{}:{}:{}", parts[0], parts[1], parts[3])
    } else if parts.len() >= 2 {
        format!("{}:{}", parts[0], parts[1])
    } else {
        maven_coord.to_string()
    }
}

fn set_status(status_log: &Arc<Mutex<String>>, msg: &str) {
    if let Ok(mut guard) = status_log.lock() {
        *guard = msg.to_string();
    }
}
