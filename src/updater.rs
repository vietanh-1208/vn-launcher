use crate::mojang;
use crate::windows_app::installed_app_exe_path;
use std::fs;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::thread;

pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
const GITHUB_LATEST_RELEASE_API: &str =
    "https://api.github.com/repos/vietanh-1208/vn-launcher/releases/latest";

#[derive(Clone)]
pub enum UpdateState {
    Idle,
    Checking,
    UpToDate,
    Available { version: String, download_url: String },
    Downloading,
    Error(String),
}

pub fn parse_semver(s: &str) -> (u32, u32, u32) {
    let clean = s.trim().trim_start_matches('v').trim_start_matches('V');
    let mut parts = clean.split('.').map(|p| p.parse::<u32>().unwrap_or(0));
    (
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
    )
}

pub fn spawn_update_check(state: Arc<Mutex<UpdateState>>, manual: bool) {
    if manual {
        if let Ok(mut s) = state.lock() {
            *s = UpdateState::Checking;
        }
    }
    thread::spawn(move || {
        let Ok(client) = mojang::build_http_client(15) else {
            if manual {
                if let Ok(mut s) = state.lock() {
                    *s = UpdateState::Error("Không tạo được kết nối mạng.".into());
                }
            }
            return;
        };
        let resp = client
            .get(GITHUB_LATEST_RELEASE_API)
            .header("User-Agent", "VN-Launcher/2.0")
            .header("Accept", "application/vnd.github+json")
            .send();
        let Ok(r) = resp else {
            if manual {
                if let Ok(mut s) = state.lock() {
                    *s = UpdateState::Error(
                        "Không kết nối được GitHub để kiểm tra bản mới.".into(),
                    );
                }
            }
            return;
        };
        if !r.status().is_success() {
            if manual {
                if let Ok(mut s) = state.lock() {
                    *s = UpdateState::Error(format!("GitHub trả về mã {}", r.status()));
                }
            }
            return;
        }
        let Ok(json) = r.json::<serde_json::Value>() else { return };
        let tag = json["tag_name"]
            .as_str()
            .unwrap_or("")
            .trim_start_matches('v')
            .to_string();
        let dl_url = json["assets"]
            .as_array()
            .and_then(|arr| {
                arr.iter()
                    .find(|a| {
                        a["name"]
                            .as_str()
                            .unwrap_or("")
                            .to_lowercase()
                            .ends_with(".exe")
                    })
                    .and_then(|a| a["browser_download_url"].as_str())
            })
            .unwrap_or("")
            .to_string();
        if !tag.is_empty() && !dl_url.is_empty() && parse_semver(&tag) > parse_semver(APP_VERSION) {
            if let Ok(mut s) = state.lock() {
                *s = UpdateState::Available {
                    version: tag,
                    download_url: dl_url,
                };
            }
        } else if manual {
            if let Ok(mut s) = state.lock() {
                *s = UpdateState::UpToDate;
            }
        }
    });
}

pub fn start_self_update(url: String, state: Arc<Mutex<UpdateState>>, status: Arc<Mutex<String>>) {
    if let Ok(mut s) = state.lock() {
        *s = UpdateState::Downloading;
    }
    if let Ok(mut st) = status.lock() {
        *st = "Đang tải bản cập nhật VN Launcher...".into();
    }
    thread::spawn(move || {
        let res = (|| -> Result<(), String> {
            let client = mojang::build_http_client(120)?;
            let mut resp = client
                .get(&url)
                .header("User-Agent", "VN-Launcher/2.0")
                .send()
                .map_err(|e| e.to_string())?;
            if !resp.status().is_success() {
                return Err(format!("HTTP {}", resp.status()));
            }
            let tmp = std::env::temp_dir().join("vn-launcher-update.exe");
            let mut file = fs::File::create(&tmp).map_err(|e| e.to_string())?;
            std::io::copy(&mut resp, &mut file).map_err(|e| e.to_string())?;
            drop(file);

            let install_dir = std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(|d| d.to_path_buf()))
                .unwrap_or_else(|| installed_app_exe_path().parent().unwrap().to_path_buf());
            Command::new(&tmp)
                .arg("/silent")
                .arg(install_dir.to_string_lossy().to_string())
                .spawn()
                .map_err(|e| e.to_string())?;
            std::process::exit(0);
        })();
        if let Err(e) = res {
            if let Ok(mut s) = state.lock() {
                *s = UpdateState::Error(format!("Lỗi tải cập nhật: {}", e));
            }
            if let Ok(mut st) = status.lock() {
                *st = format!("Lỗi cập nhật: {}", e);
            }
        }
    });
}
