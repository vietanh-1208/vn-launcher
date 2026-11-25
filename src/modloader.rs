use crate::config::ModLoader;
use crate::mojang::download_file_verified;
use reqwest::blocking::Client;
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub struct ModdedSpec {
    pub main_class: String,
    pub jars: Vec<(String, PathBuf)>,
    pub extra_jvm_args: Vec<String>,
    pub extra_game_args: Vec<String>,
}

fn set_status(status: &Arc<Mutex<String>>, msg: &str) {
    if let Ok(mut g) = status.lock() {
        *g = msg.to_string();
    }
}

fn maven_coord_to_rel_path(name: &str) -> Option<String> {
    let parts: Vec<&str> = name.split(':').collect();
    if parts.len() < 3 {
        return None;
    }
    let group = parts[0].replace('.', "/");
    let artifact = parts[1];
    let (ver, ext) = if let Some((v, e)) = parts[2].split_once('@') {
        (v, e)
    } else {
        (parts[2], "jar")
    };
    let (ver_clean, cls_opt) = if parts.len() >= 4 {
        let c = parts[3].split('@').next().unwrap_or("");
        let e = parts[3].split('@').nth(1).unwrap_or(ext);
        (ver, Some((c, e)))
    } else {
        (ver, None)
    };
    let filename = match cls_opt {
        Some((cls, e)) if !cls.is_empty() => format!("{}-{}-{}.{}", artifact, ver_clean, cls, e),
        _ => format!("{}-{}.{}", artifact, ver_clean, ext),
    };
    Some(format!("{}/{}/{}/{}", group, artifact, ver_clean, filename))
}

fn resolve_neoforge_version(client: &Client, mc_version: &str) -> Result<String, String> {
    let prefix = if let Some(rest) = mc_version.strip_prefix("1.") {
        let mut p = rest.split('.');
        let minor = p.next().unwrap_or("21");
        let patch = p.next().unwrap_or("0");
        format!("{}.{}.", minor, patch)
    } else {
        return Err(format!("NeoForge chưa hỗ trợ phiên bản {}", mc_version));
    };

    let xml = client
        .get("https://maven.neoforged.net/releases/net/neoforged/neoforge/maven-metadata.xml")
        .header("User-Agent", "VN-Launcher/2.1")
        .timeout(Duration::from_secs(20))
        .send()
        .map_err(|e| format!("Không tải được danh sách NeoForge: {}", e))?
        .text()
        .map_err(|e| e.to_string())?;

    let mut matched: Vec<String> = Vec::new();
    for line in xml.lines() {
        let t = line.trim();
        if let Some(inner) = t.strip_prefix("<version>").and_then(|s| s.strip_suffix("</version>")) {
            if inner.starts_with(&prefix) && !inner.contains("beta") {
                matched.push(inner.to_string());
            }
        }
    }
    if matched.is_empty() {
        for line in xml.lines() {
            let t = line.trim();
            if let Some(inner) = t.strip_prefix("<version>").and_then(|s| s.strip_suffix("</version>")) {
                if inner.starts_with(&prefix) {
                    matched.push(inner.to_string());
                }
            }
        }
    }
    matched
        .pop()
        .ok_or_else(|| format!("Chưa có bản NeoForge cho Minecraft {}", mc_version))
}

fn resolve_forge_version(client: &Client, mc_version: &str) -> Result<String, String> {
    #[derive(Deserialize)]
    struct PromoResp {
        promos: HashMap<String, String>,
    }
    let resp: PromoResp = client
        .get("https://files.minecraftforge.net/net/minecraftforge/forge/promotions_slim.json")
        .header("User-Agent", "VN-Launcher/2.1")
        .timeout(Duration::from_secs(20))
        .send()
        .map_err(|e| format!("Không tải được danh sách Forge: {}", e))?
        .json()
        .map_err(|e| format!("Lỗi đọc danh sách Forge: {}", e))?;

    resp.promos
        .get(&format!("{}-recommended", mc_version))
        .or_else(|| resp.promos.get(&format!("{}-latest", mc_version)))
        .cloned()
        .ok_or_else(|| format!("Chưa có bản Forge cho Minecraft {}", mc_version))
}

fn find_installed_version_json(shared_dir: &Path, needle1: &str, needle2: &str) -> Option<PathBuf> {
    let vers_dir = shared_dir.join("versions");
    let Ok(entries) = fs::read_dir(&vers_dir) else { return None };
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if name.contains(needle1) && name.contains(needle2) {
            let jf = e.path().join(format!("{}.json", name));
            if jf.exists() {
                return Some(jf);
            }
        }
    }
    None
}

pub fn ensure_forge_or_neoforge(
    client: &Client,
    java_exe: &Path,
    shared_dir: &Path,
    mc_version: &str,
    loader: ModLoader,
    status: &Arc<Mutex<String>>,
) -> Result<ModdedSpec, String> {
    let (loader_name, ver, installer_url, installer_name, needle1, needle2) = match loader {
        ModLoader::NeoForge => {
            set_status(status, &format!("Đang tìm bản NeoForge cho MC {}...", mc_version));
            let v = resolve_neoforge_version(client, mc_version)?;
            let fname = format!("neoforge-{}-installer.jar", v);
            let url = format!("https://maven.neoforged.net/releases/net/neoforged/neoforge/{}/{}", v, fname);
            ("NeoForge", v.clone(), url, fname, "neoforge".to_string(), v)
        }
        ModLoader::Forge => {
            set_status(status, &format!("Đang tìm bản Forge cho MC {}...", mc_version));
            let v = resolve_forge_version(client, mc_version)?;
            let full = format!("{}-{}", mc_version, v);
            let fname = format!("forge-{}-installer.jar", full);
            let url = format!("https://maven.minecraftforge.net/net/minecraftforge/forge/{}/{}", full, fname);
            ("Forge", v, url, fname, mc_version.to_string(), "forge".to_string())
        }
        _ => return Err("Không phải Forge/NeoForge".into()),
    };

    let version_json = match find_installed_version_json(shared_dir, &needle1, &needle2) {
        Some(p) => p,
        None => {
            let inst_dir = shared_dir.join("installers");
            fs::create_dir_all(&inst_dir).map_err(|e| e.to_string())?;
            let inst_jar = inst_dir.join(&installer_name);
            if !inst_jar.exists() {
                set_status(status, &format!("Đang tải trình cài {} {}...", loader_name, ver));
                download_file_verified(client, &installer_url, &inst_jar, None)?;
            }
            let profiles = shared_dir.join("launcher_profiles.json");
            if !profiles.exists() {
                let _ = fs::write(&profiles, "{\"profiles\":{}}");
            }
            set_status(status, &format!("Đang cài đặt {} {} (chờ ~15-30s lần đầu)...", loader_name, ver));
            let mut cmd = Command::new(java_exe);
            cmd.arg("-jar")
                .arg(&inst_jar)
                .arg("--installClient")
                .arg(shared_dir);
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                cmd.creation_flags(0x0800_0000);
            }
            let out = cmd.output().map_err(|e| format!("Không chạy được bộ cài {}: {}", loader_name, e))?;
            if !out.status.success() {
                let err_txt = String::from_utf8_lossy(&out.stderr);
                return Err(format!("Cài đặt {} thất bại: {}", loader_name, err_txt.chars().take(240).collect::<String>()));
            }
            find_installed_version_json(shared_dir, &needle1, &needle2)
                .ok_or_else(|| format!("Không tìm thấy file JSON sau khi cài {} {}", loader_name, ver))?
        }
    };

    let raw = fs::read_to_string(&version_json).map_err(|e| e.to_string())?;
    let val: Value = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    let version_id = val["id"].as_str().unwrap_or("").to_string();
    let main_class = val["mainClass"]
        .as_str()
        .ok_or_else(|| format!("Thiếu mainClass trong {}.json", version_id))?
        .to_string();

    let libs_dir = shared_dir.join("libraries");
    let mut jars: Vec<(String, PathBuf)> = Vec::new();
    if let Some(libs) = val["libraries"].as_array() {
        for lib in libs {
            let name = lib["name"].as_str().unwrap_or("");
            let ga = name.split(':').take(2).collect::<Vec<_>>().join(":");
            let rel_opt = lib["downloads"]["artifact"]["path"]
                .as_str()
                .map(|s| s.to_string())
                .or_else(|| maven_coord_to_rel_path(name));
            let Some(rel) = rel_opt else { continue };
            let p = libs_dir.join(&rel);
            if p.exists() {
                jars.push((ga, p));
            }
        }
    }

    let subst = |s: &str| -> String {
        s.replace("${library_directory}", &libs_dir.to_string_lossy())
            .replace("${classpath_separator}", ";")
            .replace("${version_name}", &version_id)
    };

    let mut extra_jvm_args = Vec::new();
    if let Some(jvm_arr) = val["arguments"]["jvm"].as_array() {
        for item in jvm_arr {
            if let Some(s) = item.as_str() {
                extra_jvm_args.push(subst(s));
            }
        }
    }
    let mut extra_game_args = Vec::new();
    if let Some(game_arr) = val["arguments"]["game"].as_array() {
        for item in game_arr {
            if let Some(s) = item.as_str() {
                extra_game_args.push(subst(s));
            }
        }
    }

    Ok(ModdedSpec {
        main_class,
        jars,
        extra_jvm_args,
        extra_game_args,
    })
}
