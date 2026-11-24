//! Accounts: offline (no account) + Microsoft premium sign-in.
//!
//! Microsoft sign-in uses the official OAuth 2.0 *device code* flow:
//!   Microsoft account -> Xbox Live -> XSTS -> Minecraft Services -> profile.
//! It requires YOUR OWN Azure application (client) ID, approved by Mojang
//! for Minecraft Services (https://aka.ms/mce-reviewappid).

use crate::config::generate_offline_uuid;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const MS_DEVICE_CODE_URL: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/devicecode";
const MS_TOKEN_URL: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/token";
const MS_SCOPE: &str = "XboxLive.signin offline_access";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AccountKind {
    Offline,
    Microsoft,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub id: String,
    pub kind: AccountKind,
    pub username: String,
    pub uuid: String,
    #[serde(default)]
    pub refresh_token: String,
    #[serde(default)]
    pub access_token: String,
    #[serde(default)]
    pub expires_at: u64,
}

impl Account {
    pub fn offline(name: &str) -> Self {
        Self {
            id: format!("offline:{}", name.to_lowercase()),
            kind: AccountKind::Offline,
            username: name.to_string(),
            uuid: generate_offline_uuid(name),
            refresh_token: String::new(),
            access_token: String::new(),
            expires_at: 0,
        }
    }

    pub fn kind_label(&self) -> &'static str {
        match self.kind {
            AccountKind::Offline => "Offline",
            AccountKind::Microsoft => "Microsoft (Premium)",
        }
    }
}

/// Stored in %APPDATA%\VNLauncher\accounts.json (NOT in the portable folder,
/// so sharing the launcher folder never leaks your Microsoft login).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AccountStore {
    pub accounts: Vec<Account>,
    pub active_id: Option<String>,
}

impl AccountStore {
    pub fn path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("VNLauncher")
            .join("accounts.json")
    }

    pub fn load() -> Self {
        fs::read_to_string(Self::path())
            .ok()
            .and_then(|c| serde_json::from_str(&c).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> Result<(), String> {
        let path = Self::path();
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let json = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        fs::write(path, json).map_err(|e| e.to_string())
    }

    pub fn active(&self) -> Option<&Account> {
        let id = self.active_id.as_ref()?;
        self.accounts.iter().find(|a| &a.id == id)
    }

    pub fn upsert(&mut self, acc: Account, make_active: bool) {
        let id = acc.id.clone();
        if let Some(existing) = self.accounts.iter_mut().find(|a| a.id == id) {
            *existing = acc;
        } else {
            self.accounts.push(acc);
        }
        if make_active || self.active_id.is_none() {
            self.active_id = Some(id);
        }
    }

    pub fn remove(&mut self, id: &str) {
        self.accounts.retain(|a| a.id != id);
        if self.active_id.as_deref() == Some(id) {
            self.active_id = self.accounts.first().map(|a| a.id.clone());
        }
    }
}

pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[derive(Debug, Clone)]
pub struct DeviceCode {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub interval: u64,
    pub expires_in: u64,
}

pub fn request_device_code(client: &Client, client_id: &str) -> Result<DeviceCode, String> {
    let resp = client
        .post(MS_DEVICE_CODE_URL)
        .form(&[("client_id", client_id), ("scope", MS_SCOPE)])
        .timeout(Duration::from_secs(20))
        .send()
        .map_err(|e| format!("Không kết nối được Microsoft: {}", e))?;
    let v: Value = resp.json().map_err(|e| e.to_string())?;
    if let Some(err) = v.get("error_description").and_then(|e| e.as_str()) {
        return Err(format!("Microsoft từ chối Client ID: {}", err));
    }
    Ok(DeviceCode {
        device_code: str_field(&v, "device_code")?,
        user_code: str_field(&v, "user_code")?,
        verification_uri: v
            .get("verification_uri")
            .and_then(|x| x.as_str())
            .unwrap_or("https://www.microsoft.com/link")
            .to_string(),
        interval: v.get("interval").and_then(|x| x.as_u64()).unwrap_or(5),
        expires_in: v.get("expires_in").and_then(|x| x.as_u64()).unwrap_or(900),
    })
}

pub enum PollResult {
    Pending,
    SlowDown,
    Success { access_token: String, refresh_token: String },
}

pub fn poll_device_token(client: &Client, client_id: &str, device_code: &str) -> Result<PollResult, String> {
    let resp = client
        .post(MS_TOKEN_URL)
        .form(&[
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ("client_id", client_id),
            ("device_code", device_code),
        ])
        .timeout(Duration::from_secs(20))
        .send()
        .map_err(|e| format!("Mất kết nối Microsoft: {}", e))?;
    let v: Value = resp.json().map_err(|e| e.to_string())?;
    match v.get("error").and_then(|e| e.as_str()) {
        Some("authorization_pending") => Ok(PollResult::Pending),
        Some("slow_down") => Ok(PollResult::SlowDown),
        Some("expired_token") => Err("Mã đăng nhập đã hết hạn, hãy thử lại.".into()),
        Some("authorization_declined") => Err("Bạn đã từ chối đăng nhập.".into()),
        Some(other) => Err(format!("Lỗi đăng nhập Microsoft: {}", other)),
        None => Ok(PollResult::Success {
            access_token: str_field(&v, "access_token")?,
            refresh_token: v
                .get("refresh_token")
                .and_then(|x| x.as_str())
                .unwrap_or_default()
                .to_string(),
        }),
    }
}

fn refresh_ms_token(client: &Client, client_id: &str, refresh_token: &str) -> Result<(String, String), String> {
    let resp = client
        .post(MS_TOKEN_URL)
        .form(&[
            ("grant_type", "refresh_token"),
            ("client_id", client_id),
            ("refresh_token", refresh_token),
            ("scope", MS_SCOPE),
        ])
        .timeout(Duration::from_secs(20))
        .send()
        .map_err(|e| format!("Mất kết nối Microsoft: {}", e))?;
    let v: Value = resp.json().map_err(|e| e.to_string())?;
    if v.get("error").is_some() {
        return Err("Phiên đăng nhập Microsoft đã hết hạn, hãy đăng nhập lại.".into());
    }
    Ok((
        str_field(&v, "access_token")?,
        v.get("refresh_token")
            .and_then(|x| x.as_str())
            .unwrap_or(refresh_token)
            .to_string(),
    ))
}

/// Microsoft token -> Xbox Live -> XSTS -> Minecraft token -> Minecraft profile
pub fn minecraft_login(client: &Client, ms_access: &str, ms_refresh: &str) -> Result<Account, String> {
    // 1. Xbox Live
    let xbl: Value = client
        .post("https://user.auth.xboxlive.com/user/authenticate")
        .header("Accept", "application/json")
        .json(&json!({
            "Properties": {
                "AuthMethod": "RPS",
                "SiteName": "user.auth.xboxlive.com",
                "RpsTicket": format!("d={}", ms_access)
            },
            "RelyingParty": "http://auth.xboxlive.com",
            "TokenType": "JWT"
        }))
        .send()
        .map_err(|e| format!("Lỗi Xbox Live: {}", e))?
        .json()
        .map_err(|e| format!("Lỗi Xbox Live: {}", e))?;
    let xbl_token = str_field(&xbl, "Token")?;

    // 2. XSTS
    let xsts_resp = client
        .post("https://xsts.auth.xboxlive.com/xsts/authorize")
        .header("Accept", "application/json")
        .json(&json!({
            "Properties": { "SandboxId": "RETAIL", "UserTokens": [xbl_token] },
            "RelyingParty": "rp://api.minecraftservices.com/",
            "TokenType": "JWT"
        }))
        .send()
        .map_err(|e| format!("Lỗi XSTS: {}", e))?;
    let xsts_status = xsts_resp.status();
    let xsts: Value = xsts_resp.json().unwrap_or(Value::Null);
    if !xsts_status.is_success() {
        let code = xsts.get("XErr").and_then(|x| x.as_u64()).unwrap_or(0);
        return Err(match code {
            2148916233 => "Tài khoản Microsoft này chưa có hồ sơ Xbox. Hãy vào xbox.com tạo hồ sơ rồi thử lại.".into(),
            2148916235 => "Xbox Live không hỗ trợ ở khu vực của tài khoản này.".into(),
            2148916236 | 2148916237 => "Tài khoản cần xác minh tuổi trên xbox.com.".into(),
            2148916238 => "Tài khoản trẻ em cần được thêm vào Gia đình Microsoft.".into(),
            _ => format!("Xbox từ chối đăng nhập (mã {}).", code),
        });
    }
    let xsts_token = str_field(&xsts, "Token")?;
    let uhs = xsts
        .pointer("/DisplayClaims/xui/0/uhs")
        .and_then(|x| x.as_str())
        .ok_or("Thiếu thông tin Xbox (uhs)")?
        .to_string();

    // 3. Minecraft Services
    let mc_resp = client
        .post("https://api.minecraftservices.com/authentication/login_with_xbox")
        .json(&json!({ "identityToken": format!("XBL3.0 x={};{}", uhs, xsts_token) }))
        .send()
        .map_err(|e| format!("Lỗi Minecraft Services: {}", e))?;
    if mc_resp.status().as_u16() == 403 {
        return Err("Minecraft Services từ chối Client ID này (403). Client ID của bạn cần được Mojang duyệt tại https://aka.ms/mce-reviewappid".into());
    }
    if !mc_resp.status().is_success() {
        return Err(format!("Minecraft Services lỗi HTTP {}", mc_resp.status()));
    }
    let mc: Value = mc_resp.json().map_err(|e| e.to_string())?;
    let mc_token = str_field(&mc, "access_token")?;
    let expires_in = mc.get("expires_in").and_then(|x| x.as_u64()).unwrap_or(86_400);

    // 4. Profile (also verifies game ownership)
    let prof_resp = client
        .get("https://api.minecraftservices.com/minecraft/profile")
        .bearer_auth(&mc_token)
        .send()
        .map_err(|e| format!("Lỗi lấy hồ sơ Minecraft: {}", e))?;
    if prof_resp.status().as_u16() == 404 {
        return Err("Tài khoản này chưa mua Minecraft: Java Edition.".into());
    }
    let prof: Value = prof_resp.json().map_err(|e| e.to_string())?;
    let uuid = str_field(&prof, "id")?;
    let name = str_field(&prof, "name")?;

    Ok(Account {
        id: format!("ms:{}", uuid),
        kind: AccountKind::Microsoft,
        username: name,
        uuid,
        refresh_token: ms_refresh.to_string(),
        access_token: mc_token,
        expires_at: now_secs() + expires_in,
    })
}

/// Refreshes the Minecraft token if it expires within 10 minutes. Returns true if updated.
pub fn ensure_fresh(client: &Client, client_id: &str, acc: &mut Account) -> Result<bool, String> {
    if acc.kind != AccountKind::Microsoft {
        return Ok(false);
    }
    if acc.expires_at > now_secs() + 600 && !acc.access_token.is_empty() {
        return Ok(false);
    }
    if client_id.trim().is_empty() {
        return Err("Thiếu Azure Client ID để làm mới đăng nhập Microsoft (xem tab Tài khoản).".into());
    }
    let (ms_access, ms_refresh) = refresh_ms_token(client, client_id.trim(), &acc.refresh_token)?;
    *acc = minecraft_login(client, &ms_access, &ms_refresh)?;
    Ok(true)
}

fn str_field(v: &Value, key: &str) -> Result<String, String> {
    v.get(key)
        .and_then(|x| x.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| format!("Phản hồi thiếu trường '{}'", key))
}
