use md5::{Digest, Md5};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum OptimizationProfile {
    #[default]
    Auto,
    Low,
    Balanced,
    High,
}

impl OptimizationProfile {
    pub fn label(&self) -> &'static str {
        match self {
            OptimizationProfile::Auto => "Tự động theo máy",
            OptimizationProfile::Low => "Máy yếu (Potato PC)",
            OptimizationProfile::Balanced => "Tầm trung (Balanced)",
            OptimizationProfile::High => "Máy khỏe (High-End)",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ModLoader {
    Vanilla,
    Fabric,
    NeoForge,
    Forge,
}

impl ModLoader {
    pub fn label(&self) -> &'static str {
        match self {
            ModLoader::Vanilla => "Vanilla",
            ModLoader::Fabric => "Fabric",
            ModLoader::NeoForge => "NeoForge",
            ModLoader::Forge => "Forge",
        }
    }

    pub fn modrinth_slug(&self) -> &'static str {
        match self {
            ModLoader::Fabric => "fabric",
            ModLoader::NeoForge => "neoforge",
            ModLoader::Forge => "forge",
            ModLoader::Vanilla => "vanilla",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerEntry {
    pub name: String,
    pub address: String,
}

fn default_ram() -> u32 {
    recommended_ram_mb()
}
fn default_lod() -> u32 {
    256
}
fn default_width() -> u32 {
    1280
}
fn default_height() -> u32 {
    720
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceConfig {
    pub id: String,
    pub name: String,
    pub mc_version: String,
    pub loader: ModLoader,
    #[serde(default)]
    pub loader_version: Option<String>,
    #[serde(default = "default_ram")]
    pub ram_mb: u32,
    /// Distant Horizons LOD radius (only used when Distant Horizons is installed)
    #[serde(default = "default_lod")]
    pub lod_render_distance: u32,
    #[serde(default)]
    pub use_zgc: bool,
    #[serde(default)]
    pub custom_jvm_args: String,
    #[serde(default)]
    pub java_path: String,
    #[serde(default)]
    pub quick_join_server: String,
    #[serde(default = "default_width")]
    pub window_width: u32,
    #[serde(default = "default_height")]
    pub window_height: u32,
    #[serde(default)]
    pub fullscreen: bool,
    /// Optional custom directory where this version's game files are stored
    #[serde(default)]
    pub custom_game_dir: String,
    #[serde(default)]
    pub optimization_profile: OptimizationProfile,
}

pub fn make_readable_folder_id(name: &str, mc_version: &str, loader: ModLoader) -> String {
    let clean_name: String = name
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '-',
            _ if c.is_control() => ' ',
            _ => c,
        })
        .collect::<String>()
        .trim()
        .trim_matches('.')
        .to_string();
    let base = if clean_name.is_empty() {
        format!("Minecraft {}", mc_version)
    } else {
        clean_name
    };
    let loader_str = loader.label();
    if base.contains(mc_version) && base.to_lowercase().contains(&loader_str.to_lowercase()) {
        base
    } else if base.contains(mc_version) {
        format!("{} ({})", base, loader_str)
    } else {
        format!("{} ({} - {})", base, mc_version, loader_str)
    }
}

impl InstanceConfig {
    pub fn new(name: &str, mc_version: &str, loader: ModLoader) -> Self {
        Self {
            id: make_readable_folder_id(name, mc_version, loader),
            name: name.to_string(),
            mc_version: mc_version.to_string(),
            loader,
            loader_version: None,
            ram_mb: recommended_ram_mb(),
            lod_render_distance: default_lod(),
            use_zgc: false,
            custom_jvm_args: String::new(),
            java_path: String::new(),
            quick_join_server: String::new(),
            window_width: default_width(),
            window_height: default_height(),
            fullscreen: false,
            custom_game_dir: String::new(),
            optimization_profile: OptimizationProfile::Auto,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LauncherConfig {
    #[serde(default)]
    pub config_version: u32,
    /// Legacy field (migrated into the account store)
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub portable_mode: bool,
    /// User-chosen root folder for game files (instances, shared, runtimes)
    #[serde(default)]
    pub custom_data_dir: String,
    #[serde(default)]
    pub custom_java_path: String,
    #[serde(default)]
    pub selected_instance_id: String,
    #[serde(default)]
    pub instances: Vec<InstanceConfig>,
    #[serde(default)]
    pub saved_servers: Vec<ServerEntry>,
    #[serde(default)]
    pub minimize_on_play: bool,
    #[serde(default)]
    pub show_snapshots: bool,
    /// Azure application (client) ID used for Microsoft sign-in
    #[serde(default)]
    pub ms_client_id: String,
}

pub const CONFIG_VERSION: u32 = 2;

impl Default for LauncherConfig {
    fn default() -> Self {
        Self {
            config_version: CONFIG_VERSION,
            username: String::new(),
            portable_mode: false,
            custom_data_dir: String::new(),
            custom_java_path: String::new(),
            selected_instance_id: String::new(),
            instances: Vec::new(),
            saved_servers: vec![ServerEntry {
                name: "NGV SMP".into(),
                address: "ngvsmp.asia:25616".into(),
            }],
            minimize_on_play: false,
            show_snapshots: false,
            ms_client_id: String::new(),
        }
    }
}

impl LauncherConfig {
    pub fn exe_dir() -> PathBuf {
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()))
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("."))
    }

    pub fn default_appdata_data_dir() -> PathBuf {
        dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".vn_launcher")
    }

    pub fn base_data_dir(&self) -> PathBuf {
        if !self.custom_data_dir.trim().is_empty() {
            PathBuf::from(self.custom_data_dir.trim())
        } else if self.portable_mode {
            Self::exe_dir().join("vn_launcher_data")
        } else {
            Self::default_appdata_data_dir()
        }
    }

    pub fn config_file_path() -> PathBuf {
        let portable_cfg = Self::exe_dir().join("vn_launcher_config.json");
        if portable_cfg.exists() {
            return portable_cfg;
        }
        let dir = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("VNLauncher");
        let _ = fs::create_dir_all(&dir);
        dir.join("vn_launcher_config.json")
    }

    pub fn load() -> Self {
        let path = Self::config_file_path();
        let raw = fs::read_to_string(&path).ok();
        let parsed = raw.as_deref().and_then(|c| serde_json::from_str::<LauncherConfig>(c).ok());
        if raw.is_some() && parsed.is_none() {
            // Never silently lose the user's config: keep a copy before writing defaults
            let _ = fs::copy(&path, path.with_extension("json.broken"));
        }
        let mut cfg = parsed.unwrap_or_default();

        if cfg.config_version < CONFIG_VERSION {
            // Migrate v1 configs: friendlier names & safe RAM for the user's PC
            let safe_ram = recommended_ram_mb();
            for inst in &mut cfg.instances {
                if inst.name.contains("VN Ultra") || inst.name.starts_with('⚡') {
                    inst.name = format!("Minecraft {} ({})", inst.mc_version, inst.loader.label());
                }
                if inst.ram_mb > safe_ram {
                    inst.ram_mb = safe_ram;
                }
                if inst.lod_render_distance == 0 || (safe_ram <= 4096 && inst.lod_render_distance > default_lod()) {
                    inst.lod_render_distance = default_lod();
                }
            }
            cfg.config_version = CONFIG_VERSION;
        }
        cfg.migrate_readable_folder_names();
        let _ = cfg.save();
        cfg
    }

    pub fn unique_folder_id(&self, name: &str, mc_version: &str, loader: ModLoader, ignore_id: Option<&str>) -> String {
        let base = make_readable_folder_id(name, mc_version, loader);
        let mut candidate = base.clone();
        let mut idx = 2u32;
        loop {
            let taken_in_cfg = self
                .instances
                .iter()
                .any(|i| Some(i.id.as_str()) != ignore_id && i.id.eq_ignore_ascii_case(&candidate));
            let dir_exists = Some(candidate.as_str()) != ignore_id && self.instance_dir(&candidate).exists();
            if !taken_in_cfg && !dir_exists {
                return candidate;
            }
            candidate = format!("{} - {}", base, idx);
            idx += 1;
        }
    }

    pub fn migrate_readable_folder_names(&mut self) {
        let count = self.instances.len();
        for i in 0..count {
            let old_id = self.instances[i].id.clone();
            let desired = self.unique_folder_id(
                &self.instances[i].name,
                &self.instances[i].mc_version,
                self.instances[i].loader,
                Some(&old_id),
            );
            if old_id == desired {
                continue;
            }
            // Automatically rename timestamp-style `inst-...` folders (or whenever desired differs and rename succeeds)
            let old_dir = self.instance_dir(&old_id);
            let new_dir = self.instance_dir(&desired);
            let renamed_ok = if old_dir.exists() {
                !new_dir.exists() && fs::rename(&old_dir, &new_dir).is_ok()
            } else {
                true
            };
            if renamed_ok {
                if self.selected_instance_id == old_id {
                    self.selected_instance_id = desired.clone();
                }
                self.instances[i].id = desired;
            }
        }
    }

    pub fn save(&self) -> Result<(), String> {
        let json = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        fs::write(Self::config_file_path(), json).map_err(|e| e.to_string())
    }

    pub fn shared_dir(&self) -> PathBuf {
        self.base_data_dir().join("shared")
    }

    pub fn runtimes_dir(&self) -> PathBuf {
        self.base_data_dir().join("runtimes")
    }

    pub fn instances_dir(&self) -> PathBuf {
        self.base_data_dir().join("instances")
    }

    pub fn instance_dir(&self, instance_id: &str) -> PathBuf {
        if let Some(inst) = self.instances.iter().find(|i| i.id == instance_id) {
            if !inst.custom_game_dir.trim().is_empty() {
                return PathBuf::from(inst.custom_game_dir.trim());
            }
        }
        self.instances_dir().join(instance_id)
    }

    pub fn instance_game_dir(&self, instance_id: &str) -> PathBuf {
        if let Some(inst) = self.instances.iter().find(|i| i.id == instance_id) {
            if !inst.custom_game_dir.trim().is_empty() {
                let custom = PathBuf::from(inst.custom_game_dir.trim());
                return if custom.file_name().and_then(|n| n.to_str()) == Some(".minecraft") {
                    custom
                } else {
                    custom.join(".minecraft")
                };
            }
        }
        self.instance_dir(instance_id).join(".minecraft")
    }
}

/// Standard Minecraft offline-mode UUID (v3 of "OfflinePlayer:<name>")
pub fn generate_offline_uuid(username: &str) -> String {
    let mut hasher = Md5::new();
    hasher.update(format!("OfflinePlayer:{}", username).as_bytes());
    let mut b: [u8; 16] = hasher.finalize().into();
    b[6] = (b[6] & 0x0f) | 0x30;
    b[8] = (b[8] & 0x3f) | 0x80;
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7], b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15]
    )
}

/// Total physical RAM of this PC in MB
pub fn total_ram_mb() -> u64 {
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
        let mut m: MEMORYSTATUSEX = std::mem::zeroed();
        m.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
        if GlobalMemoryStatusEx(&mut m) != 0 {
            return m.ullTotalPhys / (1024 * 1024);
        }
    }
    8192
}

/// Safe RAM for Minecraft: leaves enough memory for Windows so the game never swaps to disk
pub fn recommended_ram_mb() -> u32 {
    let total = total_ram_mb();
    if total <= 4_600 {
        1536
    } else if total <= 6_600 {
        2048
    } else if total <= 8_700 {
        3072
    } else if total <= 12_700 {
        4096
    } else if total <= 16_700 {
        6144
    } else {
        8192
    }
}

pub fn max_ram_mb() -> u32 {
    let total = total_ram_mb() as u32;
    total.saturating_sub(2560).max(1536)
}
