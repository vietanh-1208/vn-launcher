use reqwest::blocking::Client;
use serde::Deserialize;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Debug, Deserialize)]
struct AdoptiumAsset {
    binary: AdoptiumBinary,
}

#[derive(Debug, Deserialize)]
struct AdoptiumBinary {
    package: AdoptiumPackage,
}

#[derive(Debug, Deserialize)]
struct AdoptiumPackage {
    link: String,
    name: String,
    size: Option<u64>,
}

/// Determines the best Java version for a given Minecraft version string if version JSON isn't loaded yet.
/// Note: Java 21 is backward-compatible with MC 1.18+ (Fabric) and required for 1.20.5+ / 1.21+.
/// Furthermore, Java 21 supports Generational ZGC (-XX:+UseZGC -XX:+ZGenerational), which is a huge boost for 1024-chunk Distant Horizons!
pub fn recommended_java_major(mc_version: &str, json_major: Option<u32>) -> u32 {
    if let Some(m) = json_major {
        // Use exactly what Mojang requires (Java 16 is no longer published -> 17 runs those versions fine)
        return if m == 16 { 17 } else { m };
    }
    if mc_version.starts_with("26.") || mc_version.starts_with("25.") {
        25
    } else if mc_version.starts_with("1.21")
        || mc_version.starts_with("1.20")
        || mc_version.starts_with("1.19")
        || mc_version.starts_with("1.18")
    {
        21
    } else if mc_version.starts_with("1.17") {
        17
    } else {
        8
    }
}

fn java_bin_name() -> &'static str {
    if cfg!(windows) {
        "java.exe"
    } else {
        "java"
    }
}

/// Searches `runtimes_dir/java-<major>` for `bin/java` (or `bin/java.exe` on Windows)
pub fn find_installed_java(runtimes_dir: &Path, major: u32) -> Option<PathBuf> {
    let target_dir = runtimes_dir.join(format!("java-{}", major));
    if !target_dir.exists() {
        return None;
    }
    find_java_exe_recursive(&target_dir)
}

fn find_java_exe_recursive(dir: &Path) -> Option<PathBuf> {
    let bin = java_bin_name();
    let direct = dir.join("bin").join(bin);
    if direct.exists() {
        return Some(direct);
    }
    let mac_direct = dir.join("Contents").join("Home").join("bin").join(bin);
    if mac_direct.exists() {
        return Some(mac_direct);
    }
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let candidate = path.join("bin").join(bin);
                if candidate.exists() {
                    return Some(candidate);
                }
                let mac_candidate = path.join("Contents").join("Home").join("bin").join(bin);
                if mac_candidate.exists() {
                    return Some(mac_candidate);
                }
            }
        }
    }
    None
}

/// Ensures the requested Java major version (e.g. 21, 17, 8) is available locally in `runtimes_dir`.
/// Downloads Eclipse Adoptium Temurin JRE automatically if missing.
pub fn ensure_java_runtime(
    client: &Client,
    runtimes_dir: &Path,
    major: u32,
    custom_java_path: &str,
    status_log: &Arc<Mutex<String>>,
) -> Result<PathBuf, String> {
    if !custom_java_path.trim().is_empty() {
        let custom = PathBuf::from(custom_java_path.trim());
        if custom.exists() {
            return Ok(custom);
        }
    }

    if let Some(existing) = find_installed_java(runtimes_dir, major) {
        return Ok(existing);
    }

    set_status(
        status_log,
        &format!(
            "☕ Đang tìm bản Java {} tối ưu từ Eclipse Adoptium...",
            major
        ),
    );

    let adoptium_os = if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "mac"
    } else {
        "linux"
    };
    let adoptium_arch = if cfg!(target_arch = "aarch64") {
        "aarch64"
    } else {
        "x64"
    };

    let api_url = format!(
        "https://api.adoptium.net/v3/assets/latest/{}/hotspot?architecture={}&image_type=jre&os={}&vendor=eclipse",
        major, adoptium_arch, adoptium_os
    );

    let resp = client
        .get(&api_url)
        .header("User-Agent", "VN-Launcher/1.0 (Open-Source Rust MC Launcher)")
        .send()
        .map_err(|e| format!("Lỗi kết nối API Adoptium Java: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!(
            "Adoptium trả về mã lỗi HTTP {} khi tìm Java {}",
            resp.status(),
            major
        ));
    }

    let assets: Vec<AdoptiumAsset> = resp
        .json()
        .map_err(|e| format!("Lỗi đọc dữ liệu JSON từ Adoptium: {}", e))?;

    let first = assets
        .first()
        .ok_or_else(|| format!("Không tìm thấy gói cài đặt Java {} cho {} {}", major, adoptium_os, adoptium_arch))?;

    let download_url = &first.binary.package.link;
    let pkg_name = &first.binary.package.name;
    let total_size = first.binary.package.size.unwrap_or(45_000_000);

    let target_dir = runtimes_dir.join(format!("java-{}", major));
    fs::create_dir_all(&target_dir)
        .map_err(|e| format!("Không thể tạo thư mục lưu Java: {}", e))?;

    let archive_path = target_dir.join(pkg_name);

    set_status(
        status_log,
        &format!("☕ Đang tải {} ({:.1} MB)...", pkg_name, total_size as f64 / 1_048_576.0),
    );

    download_file_with_progress(client, download_url, &archive_path, total_size, status_log, &format!("Java {}", major))?;

    set_status(
        status_log,
        &format!("📦 Đang giải nén Java {} vào thư mục Portable...", major),
    );

    if pkg_name.ends_with(".tar.gz") || pkg_name.ends_with(".tgz") {
        let out = std::process::Command::new("tar")
            .arg("-xzf")
            .arg(&archive_path)
            .arg("-C")
            .arg(&target_dir)
            .output()
            .map_err(|e| format!("Lỗi chạy tar giải nén Java: {}", e))?;
        if !out.status.success() {
            return Err(format!(
                "Lỗi giải nén Java (.tar.gz): {}",
                String::from_utf8_lossy(&out.stderr)
            ));
        }
    } else {
        extract_zip(&archive_path, &target_dir)?;
    }
    let _ = fs::remove_file(&archive_path);

    find_installed_java(runtimes_dir, major).ok_or_else(|| {
        format!(
            "Đã giải nén Java {} nhưng không tìm thấy bin/{}",
            major,
            java_bin_name()
        )
    })
}

fn download_file_with_progress(
    client: &Client,
    url: &str,
    dest: &Path,
    expected_size: u64,
    status_log: &Arc<Mutex<String>>,
    label: &str,
) -> Result<(), String> {
    let mut resp = client
        .get(url)
        .header("User-Agent", "VN-Launcher/1.0")
        .timeout(Duration::from_secs(300))
        .send()
        .map_err(|e| format!("Lỗi tải {}: {}", label, e))?;

    if !resp.status().is_success() {
        return Err(format!("Tải {} thất bại (HTTP {})", label, resp.status()));
    }

    let total = resp.content_length().unwrap_or(expected_size).max(1);
    let mut out = File::create(dest).map_err(|e| format!("Không tạo được file {}: {}", dest.display(), e))?;

    let mut downloaded: u64 = 0;
    let mut buf = [0u8; 65536];
    let mut last_percent = 0;

    loop {
        let n = resp
            .read(&mut buf)
            .map_err(|e| format!("Lỗi đọc luồng tải {}: {}", label, e))?;
        if n == 0 {
            break;
        }
        out.write_all(&buf[..n])
            .map_err(|e| format!("Lỗi ghi file {}: {}", label, e))?;
        downloaded += n as u64;
        let percent = ((downloaded * 100) / total).min(100);
        if percent >= last_percent + 5 || percent == 100 {
            last_percent = percent;
            set_status(
                status_log,
                &format!(
                    "☕ Đang tải {}: {}% ({:.1} / {:.1} MB)",
                    label,
                    percent,
                    downloaded as f64 / 1_048_576.0,
                    total as f64 / 1_048_576.0
                ),
            );
        }
    }

    Ok(())
}

pub fn extract_zip(zip_path: &Path, dest_dir: &Path) -> Result<(), String> {
    let file = File::open(zip_path).map_err(|e| format!("Không mở được file zip: {}", e))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("File zip lỗi: {}", e))?;

    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| format!("Lỗi đọc mục trong zip: {}", e))?;
        let Some(enclosed) = entry.enclosed_name() else {
            continue;
        };
        let out_path = dest_dir.join(enclosed);

        if entry.is_dir() {
            let _ = fs::create_dir_all(&out_path);
        } else {
            if let Some(parent) = out_path.parent() {
                let _ = fs::create_dir_all(parent);
            }
            let mut outfile = File::create(&out_path)
                .map_err(|e| format!("Không thể ghi file {}: {}", out_path.display(), e))?;
            std::io::copy(&mut entry, &mut outfile)
                .map_err(|e| format!("Lỗi giải nén file {}: {}", out_path.display(), e))?;
        }
    }
    Ok(())
}

fn set_status(status_log: &Arc<Mutex<String>>, msg: &str) {
    if let Ok(mut guard) = status_log.lock() {
        *guard = msg.to_string();
    }
}
