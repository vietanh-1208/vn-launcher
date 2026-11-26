use crate::config::{total_ram_mb, InstanceConfig, OptimizationProfile};
use std::fs;
use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone)]
pub struct HardwareInfo {
    pub ram_mb: u64,
    pub cpu_name: String,
    pub cpu_threads: usize,
    pub gpu_name: String,
    pub has_dedicated_gpu: bool,
}

impl Default for HardwareInfo {
    fn default() -> Self {
        let ram_mb = total_ram_mb();
        let cpu_threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
        Self {
            ram_mb,
            cpu_name: "Đang nhận diện CPU...".into(),
            cpu_threads,
            gpu_name: "Đang nhận diện GPU...".into(),
            has_dedicated_gpu: false,
        }
    }
}

/// Detects CPU, RAM, and GPU on a background thread without blocking the UI.
pub fn detect_hardware() -> HardwareInfo {
    let mut sys = sysinfo::System::new();
    sys.refresh_memory();
    sys.refresh_cpu_list(sysinfo::CpuRefreshKind::everything());

    let ram_mb = (sys.total_memory() / (1024 * 1024)).max(total_ram_mb());
    let cpu_threads = sys.cpus().len().max(1);
    let cpu_name = sys
        .cpus()
        .first()
        .map(|c| c.brand().trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "CPU x64".into());

    let mut gpu_name = "Integrated Graphics".to_string();
    let mut has_dedicated_gpu = false;
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        if let Ok(out) = Command::new("powershell")
            .args([
                "-NoProfile",
                "-WindowStyle",
                "Hidden",
                "-Command",
                "(Get-CimInstance Win32_VideoController | Select-Object -ExpandProperty Name) -join '|'",
            ])
            .creation_flags(0x0800_0000)
            .output()
        {
            let raw = String::from_utf8_lossy(&out.stdout).trim().to_string();
            let gpus: Vec<&str> = raw.split('|').map(|s| s.trim()).filter(|s| !s.is_empty()).collect();
            if let Some(dgpu) = gpus.iter().find(|g| {
                let l = g.to_lowercase();
                l.contains("nvidia") || l.contains("geforce") || l.contains("rtx") || l.contains("gtx") || l.contains("radeon rx")
            }) {
                gpu_name = (*dgpu).to_string();
                has_dedicated_gpu = true;
            } else if let Some(first) = gpus.first() {
                gpu_name = (*first).to_string();
            }
        }
    }
    #[cfg(target_os = "linux")]
    {
        if let Ok(out) = Command::new("sh")
            .args(["-c", "lspci 2>/dev/null | grep -iE 'vga|3d|display'"])
            .output()
        {
            let raw = String::from_utf8_lossy(&out.stdout);
            for line in raw.lines() {
                let desc = line.split(':').last().unwrap_or(line).trim();
                if !desc.is_empty() {
                    let l = desc.to_lowercase();
                    if l.contains("nvidia") || l.contains("geforce") || l.contains("rtx") || l.contains("gtx") || l.contains("radeon rx") {
                        gpu_name = desc.to_string();
                        has_dedicated_gpu = true;
                        break;
                    } else if gpu_name.is_empty() {
                        gpu_name = desc.to_string();
                    }
                }
            }
        }
    }
    #[cfg(target_os = "macos")]
    {
        if let Ok(out) = Command::new("system_profiler")
            .arg("SPDisplaysDataType")
            .output()
        {
            let raw = String::from_utf8_lossy(&out.stdout);
            for line in raw.lines() {
                let t = line.trim();
                if let Some(val) = t.strip_prefix("Chipset Model:") {
                    gpu_name = val.trim().to_string();
                    let l = gpu_name.to_lowercase();
                    has_dedicated_gpu = l.contains("apple m") || l.contains("radeon") || l.contains("pro");
                    break;
                }
            }
        }
    }

    HardwareInfo {
        ram_mb,
        cpu_name,
        cpu_threads,
        gpu_name,
        has_dedicated_gpu,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectiveProfile {
    Low,
    Balanced,
    High,
}

impl EffectiveProfile {
    pub fn label(&self) -> &'static str {
        match self {
            EffectiveProfile::Low => "Máy yếu (Potato PC)",
            EffectiveProfile::Balanced => "Tầm trung (Balanced)",
            EffectiveProfile::High => "Máy khỏe (High-End)",
        }
    }
}

pub fn resolve_profile(profile: OptimizationProfile, hw: &HardwareInfo) -> EffectiveProfile {
    match profile {
        OptimizationProfile::Low => EffectiveProfile::Low,
        OptimizationProfile::Balanced => EffectiveProfile::Balanced,
        OptimizationProfile::High => EffectiveProfile::High,
        OptimizationProfile::Auto => {
            if hw.ram_mb <= 6_400 || hw.cpu_threads <= 4 {
                EffectiveProfile::Low
            } else if hw.ram_mb >= 15_360 && hw.has_dedicated_gpu {
                EffectiveProfile::High
            } else {
                EffectiveProfile::Balanced
            }
        }
    }
}

pub fn recommended_ram_for_profile(eff: EffectiveProfile, hw: &HardwareInfo) -> u32 {
    let max_safe = (hw.ram_mb as u32).saturating_sub(2560).max(1536);
    let target = match eff {
        EffectiveProfile::Low => 2048.min(max_safe),
        EffectiveProfile::Balanced => 3584.min(max_safe),
        EffectiveProfile::High => 6144.min(max_safe),
    };
    target.max(1536)
}

/// JVM arguments tuned per hardware tier (Low / Balanced / High).
pub fn build_jvm_args(
    instance: &InstanceConfig,
    java_major: u32,
    natives_dir: &Path,
    hw: &HardwareInfo,
) -> Vec<String> {
    let ram = instance.ram_mb.max(1024);
    let eff = resolve_profile(instance.optimization_profile, hw);
    let xms = match eff {
        EffectiveProfile::Low => ram.min(512),
        EffectiveProfile::Balanced => ram.min(1024),
        EffectiveProfile::High => ram.min(2048),
    };
    let mut args = vec![format!("-Xmx{}M", ram), format!("-Xms{}M", xms)];
    #[cfg(target_os = "macos")]
    {
        args.push("-XstartOnFirstThread".into());
    }

    if instance.use_zgc && java_major >= 21 {
        args.push("-XX:+UseZGC".into());
        if java_major < 23 {
            args.push("-XX:+ZGenerational".into());
        }
    } else {
        let tier_flags: &[&str] = match eff {
            EffectiveProfile::Low => &[
                "-XX:+UseG1GC",
                "-XX:MaxGCPauseMillis=50",
                "-XX:+UnlockExperimentalVMOptions",
                "-XX:+DisableExplicitGC",
                "-XX:G1HeapWastePercent=5",
                "-XX:+AlwaysActAsServerClassMachine",
                "-XX:+UseStringDeduplication",
            ],
            EffectiveProfile::Balanced => &[
                "-XX:+UseG1GC",
                "-XX:+UnlockExperimentalVMOptions",
                "-XX:+DisableExplicitGC",
                "-XX:G1NewSizePercent=20",
                "-XX:G1ReservePercent=20",
                "-XX:G1HeapRegionSize=8M",
                "-XX:MaxGCPauseMillis=100",
                "-XX:+PerfDisableSharedMem",
                "-XX:+AlwaysActAsServerClassMachine",
                "-XX:+UseStringDeduplication",
            ],
            EffectiveProfile::High => &[
                "-XX:+UseG1GC",
                "-XX:+ParallelRefProcEnabled",
                "-XX:MaxGCPauseMillis=30",
                "-XX:+UnlockExperimentalVMOptions",
                "-XX:+DisableExplicitGC",
                "-XX:G1NewSizePercent=30",
                "-XX:G1MaxNewSizePercent=40",
                "-XX:G1HeapRegionSize=8M",
                "-XX:G1ReservePercent=20",
                "-XX:G1HeapWastePercent=5",
                "-XX:+PerfDisableSharedMem",
                "-XX:+AlwaysActAsServerClassMachine",
                "-XX:+UseStringDeduplication",
            ],
        };
        for a in tier_flags {
            args.push((*a).into());
        }
        if eff == EffectiveProfile::High && hw.ram_mb >= 15_360 {
            args.push("-XX:+AlwaysPreTouch".into());
        }
    }
    if java_major >= 21 {
        args.push("--enable-native-access=ALL-UNNAMED".into());
    }

    let natives = natives_dir.to_string_lossy();
    args.push(format!("-Djava.library.path={}", natives));
    args.push(format!("-Dorg.lwjgl.librarypath={}", natives));
    args.push("-Dminecraft.launcher.brand=VN-Launcher-by-Vanh".into());
    args.push("-Dminecraft.launcher.version=2.2.0".into());

    for part in instance.custom_jvm_args.split_whitespace() {
        args.push(part.to_string());
    }
    args
}

/// Creates the standard folders and a sensible options.txt the FIRST time only.
/// After that the player's in-game settings are never overwritten.
pub fn prepare_game_dir(game_dir: &Path) -> Result<(), String> {
    for sub in ["mods", "resourcepacks", "shaderpacks", "saves", "screenshots", "config"] {
        fs::create_dir_all(game_dir.join(sub)).map_err(|e| e.to_string())?;
    }
    let options = game_dir.join("options.txt");
    if !options.exists() {
        let initial = if total_ram_mb() <= 4_600 {
            "lang:vi_vn\nrenderDistance:6\nsimulationDistance:5\nenableVsync:false\nmaxFps:260\ngraphicsMode:0\nrenderClouds:\"false\"\nentityDistanceScaling:0.5\nentityShadows:false\nbiomeBlendRadius:0\nao:0\nmipmapLevels:0\nparticles:2\n"
        } else {
            "lang:vi_vn\nrenderDistance:12\nsimulationDistance:8\nenableVsync:false\nmaxFps:260\ngraphicsMode:1\nrenderClouds:\"false\"\nentityDistanceScaling:0.75\nbiomeBlendRadius:1\nparticles:1\n"
        };
        let _ = fs::write(&options, initial);
    }
    // Prevent Lithium + ModernFix getTemperature mixin conflict when ModernFix is installed
    let has_modernfix = fs::read_dir(game_dir.join("mods"))
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .any(|e| e.file_name().to_string_lossy().to_lowercase().contains("modernfix"));
    if has_modernfix {
        ensure_modernfix_compat(game_dir);
    }
    Ok(())
}

/// Applies tiered graphics settings (`Low` / `Balanced` / `High`) to `options.txt` in-place.
/// Non-destructive: if the user installed Bobby or Distant Horizons (or set renderDistance higher on High-End),
/// existing `renderDistance` is preserved and never lowered.
pub fn optimize_graphics_options(
    game_dir: &Path,
    profile: OptimizationProfile,
    hw: &HardwareInfo,
) -> Result<EffectiveProfile, String> {
    fs::create_dir_all(game_dir).map_err(|e| e.to_string())?;
    let eff = resolve_profile(profile, hw);
    let options = game_dir.join("options.txt");
    let existing = fs::read_to_string(&options).unwrap_or_default();

    let has_view_mod = fs::read_dir(game_dir.join("mods"))
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .any(|e| {
            let n = e.file_name().to_string_lossy().to_lowercase();
            n.contains("bobby") || n.contains("distanthorizons") || n.contains("distant-horizons")
        });

    let presets: &[(&str, &str)] = match eff {
        EffectiveProfile::Low => &[
            ("renderDistance", "6"),
            ("simulationDistance", "5"),
            ("enableVsync", "false"),
            ("maxFps", "260"),
            ("graphicsMode", "0"),
            ("renderClouds", "\"false\""),
            ("entityDistanceScaling", "0.5"),
            ("entityShadows", "false"),
            ("biomeBlendRadius", "0"),
            ("ao", "0"),
            ("mipmapLevels", "0"),
            ("particles", "2"),
        ],
        EffectiveProfile::Balanced => &[
            ("renderDistance", "12"),
            ("simulationDistance", "8"),
            ("enableVsync", "false"),
            ("maxFps", "260"),
            ("graphicsMode", "1"),
            ("renderClouds", "\"fast\""),
            ("entityDistanceScaling", "0.75"),
            ("entityShadows", "true"),
            ("biomeBlendRadius", "1"),
            ("mipmapLevels", "2"),
            ("particles", "1"),
        ],
        EffectiveProfile::High => &[
            ("renderDistance", "16"),
            ("simulationDistance", "12"),
            ("enableVsync", "false"),
            ("maxFps", "260"),
            ("graphicsMode", "1"),
            ("renderClouds", "\"fancy\""),
            ("entityDistanceScaling", "1.0"),
            ("entityShadows", "true"),
            ("biomeBlendRadius", "2"),
            ("mipmapLevels", "4"),
            ("particles", "0"),
        ],
    };

    let mut seen = vec![false; presets.len()];
    let mut lines: Vec<String> = Vec::new();
    for raw_line in existing.lines() {
        let mut replaced = false;
        if let Some((k, v)) = raw_line.split_once(':') {
            for (idx, (pk, pv)) in presets.iter().enumerate() {
                if k == *pk {
                    if k == "renderDistance" {
                        let cur_rd = v.trim().parse::<u32>().unwrap_or(0);
                        let target_rd = pv.parse::<u32>().unwrap_or(12);
                        if (has_view_mod && cur_rd >= 12) || (eff == EffectiveProfile::High && cur_rd > target_rd) {
                            lines.push(raw_line.to_string());
                            seen[idx] = true;
                            replaced = true;
                            break;
                        }
                    }
                    lines.push(format!("{}:{}", pk, pv));
                    seen[idx] = true;
                    replaced = true;
                    break;
                }
            }
        }
        if !replaced {
            lines.push(raw_line.to_string());
        }
    }
    if lines.is_empty() {
        lines.push("lang:vi_vn".to_string());
    }
    for (idx, (pk, pv)) in presets.iter().enumerate() {
        if !seen[idx] {
            lines.push(format!("{}:{}", pk, pv));
        }
    }
    fs::write(&options, lines.join("\n") + "\n").map_err(|e| e.to_string())?;
    Ok(eff)
}

/// Registers the Java executable to use High Performance GPU in Windows Graphics Preferences.
pub fn ensure_high_perf_gpu(java_exe: &Path) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let exe_path = java_exe.to_string_lossy().replace('\'', "''");
        let ps = format!(
            "$p='HKCU:\\Software\\Microsoft\\DirectX\\UserGpuPreferences'; if(!(Test-Path $p)){{New-Item -Path $p -Force|Out-Null}}; Set-ItemProperty -Path $p -Name '{}' -Value 'GpuPreference=2;' -Force",
            exe_path
        );
        let _ = Command::new("powershell")
            .args(["-NoProfile", "-WindowStyle", "Hidden", "-Command", &ps])
            .creation_flags(0x0800_0000)
            .status();
    }
    #[cfg(not(windows))]
    let _ = java_exe;
}

pub fn ensure_modernfix_compat(game_dir: &Path) {
    let cfg_dir = game_dir.join("config");
    let _ = fs::create_dir_all(&cfg_dir);
    let mf = cfg_dir.join("modernfix-mixins.properties");
    let existing = fs::read_to_string(&mf).unwrap_or_default();
    if !existing.contains("mixin.perf.remove_biome_temperature_cache") {
        let mut out = existing.trim_end().to_string();
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str("mixin.perf.remove_biome_temperature_cache=false\n");
        let _ = fs::write(&mf, out);
    }
}

/// Configures Bobby so the in-game Render Distance ("Kh.cách kết xuất") slider
/// unlocks past 32 chunks (up to max_distance, e.g. 1024) while keeping all other
/// Bobby settings at default.
pub fn patch_bobby(game_dir: &Path, max_distance: u32) {
    let cfg_dir = game_dir.join("config");
    let _ = fs::create_dir_all(&cfg_dir);
    let max_d = max_distance.clamp(64, 1024);
    let content = format!(
        "enabled=true\nno-block-entities=true\nmax-render-distance={}\n",
        max_d
    );
    let _ = fs::write(cfg_dir.join("bobby.conf"), content);
}

/// Adjusts or creates the Distant Horizons v5 config tuned for high FPS + far horizon.
pub fn patch_distant_horizons(game_dir: &Path, lod_radius: u32) -> bool {
    let cfg_dir = game_dir.join("config");
    let _ = fs::create_dir_all(&cfg_dir);
    let path = cfg_dir.join("DistantHorizons.toml");
    let cpus = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
    let dh_threads = (cpus / 4).clamp(1, 3);
    let radius = lod_radius.clamp(32, 1024);

    let Ok(content) = fs::read_to_string(&path) else {
        // Pre-seed a valid v5 config before first launch so DH immediately uses fast settings
        let initial = format!(
            "_version = 5\n\n[common.multiThreading]\n\tnumberOfThreads = {}\n\tthreadRunTimeRatio = \"0.65\"\n\n[client.advanced.graphics]\n\tenableSsao = false\n\n[client.advanced.graphics.quality]\n\tlodChunkRenderDistanceRadius = {}\n\tenableCaveCulling = false\n",
            dh_threads, radius
        );
        let _ = fs::write(&path, initial);
        return true;
    };

    let replacements: [(&str, String); 5] = [
        ("lodChunkRenderDistanceRadius", radius.to_string()),
        ("numberOfThreads", dh_threads.to_string()),
        ("threadRunTimeRatio", "\"0.65\"".to_string()),
        ("enableSsao", "false".to_string()),
        ("enableCaveCulling", "false".to_string()),
    ];

    let mut changed = false;
    let mut out = String::with_capacity(content.len());
    for line in content.lines() {
        let trimmed = line.trim_start();
        let mut replaced = None;
        for (key, value) in &replacements {
            if trimmed.starts_with(&format!("{} =", key)) {
                let indent = &line[..line.len() - trimmed.len()];
                let new_line = format!("{}{} = {}", indent, key, value);
                if new_line != line {
                    changed = true;
                }
                replaced = Some(new_line);
                break;
            }
        }
        out.push_str(replaced.as_deref().unwrap_or(line));
        out.push('\n');
    }
    if changed {
        let _ = fs::write(&path, out);
    }
    true
}

/// Copies a local `.png` skin into `CustomSkinLoader/LocalSkin/skins/<username>.png`
/// and configures `CustomSkinLoader.json` so Offline accounts display their custom skin.
pub fn apply_offline_skin(game_dir: &Path, username: &str, skin_src: &Path) -> Result<(), String> {
    if !skin_src.exists() {
        return Err("Không tìm thấy file ảnh Skin đã chọn.".into());
    }
    let csl_dir = game_dir.join("CustomSkinLoader");
    let skins_dir = csl_dir.join("LocalSkin").join("skins");
    fs::create_dir_all(&skins_dir).map_err(|e| e.to_string())?;
    fs::copy(skin_src, skins_dir.join(format!("{}.png", username))).map_err(|e| e.to_string())?;

    let cfg_path = csl_dir.join("CustomSkinLoader.json");
    let should_write = fs::read_to_string(&cfg_path)
        .map(|s| !s.contains("\"LocalSkin\""))
        .unwrap_or(true);
    if should_write {
        let cfg = serde_json::json!({
            "enable": true,
            "enableDynamicSkull": true,
            "enableTransparentSkin": true,
            "forceLoadAllTextures": true,
            "loadlist": [
                { "name": "LocalSkin", "type": "Legacy", "checkPNG": false, "skin": "LocalSkin/skins/{USERNAME}.png", "model": "auto" },
                { "name": "Mojang", "type": "MojangAPI" },
                { "name": "LittleSkin", "type": "CustomSkinAPI", "root": "https://littleskin.cn/csl/" }
            ]
        });
        let _ = fs::write(&cfg_path, serde_json::to_string_pretty(&cfg).unwrap_or_default());
    }
    Ok(())
}

pub fn dir_size_bytes(path: &Path) -> u64 {
    let Ok(entries) = fs::read_dir(path) else { return 0 };
    let mut total = 0u64;
    for entry in entries.flatten() {
        let p = entry.path();
        if let Ok(meta) = entry.metadata() {
            if meta.is_dir() {
                total += dir_size_bytes(&p);
            } else {
                total += meta.len();
            }
        }
    }
    total
}

/// Zips a Minecraft world directory into `dest_zip`.
pub fn backup_world_zip(world_dir: &Path, dest_zip: &Path) -> Result<(), String> {
    if let Some(parent) = dest_zip.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let file = fs::File::create(dest_zip).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    fn add_dir(
        root: &Path,
        cur: &Path,
        zip: &mut zip::ZipWriter<fs::File>,
        options: zip::write::SimpleFileOptions,
    ) -> Result<(), String> {
        let entries = fs::read_dir(cur).map_err(|e| e.to_string())?;
        for entry in entries.flatten() {
            let p = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            // Skip Windows/Java session lock file so backup succeeds cleanly
            if name == "session.lock" {
                continue;
            }
            let rel = p.strip_prefix(root).unwrap_or(&p).to_string_lossy().replace('\\', "/");
            if p.is_dir() {
                let _ = zip.add_directory(format!("{}/", rel), options);
                add_dir(root, &p, zip, options)?;
            } else if let Ok(mut f) = fs::File::open(&p) {
                zip.start_file(&rel, options).map_err(|e| e.to_string())?;
                std::io::copy(&mut f, zip).map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    }

    add_dir(world_dir, world_dir, &mut zip, options)?;
    zip.finish().map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{InstanceConfig, ModLoader, OptimizationProfile};

    #[test]
    fn test_resolve_profile_tiers() {
        let low_hw = HardwareInfo {
            ram_mb: 4096,
            cpu_name: "Intel Celeron".into(),
            cpu_threads: 4,
            gpu_name: "Intel UHD Graphics".into(),
            has_dedicated_gpu: false,
        };
        let mid_hw = HardwareInfo {
            ram_mb: 8192,
            cpu_name: "Intel Core 5 205H".into(),
            cpu_threads: 12,
            gpu_name: "NVIDIA GeForce RTX 3050 Laptop GPU".into(),
            has_dedicated_gpu: true,
        };
        let high_hw = HardwareInfo {
            ram_mb: 16384,
            cpu_name: "Ryzen 7".into(),
            cpu_threads: 16,
            gpu_name: "NVIDIA GeForce RTX 4070".into(),
            has_dedicated_gpu: true,
        };

        assert_eq!(resolve_profile(OptimizationProfile::Auto, &low_hw), EffectiveProfile::Low);
        assert_eq!(resolve_profile(OptimizationProfile::Auto, &mid_hw), EffectiveProfile::Balanced);
        assert_eq!(resolve_profile(OptimizationProfile::Auto, &high_hw), EffectiveProfile::High);
        assert_eq!(resolve_profile(OptimizationProfile::High, &low_hw), EffectiveProfile::High);
        assert_eq!(resolve_profile(OptimizationProfile::Low, &high_hw), EffectiveProfile::Low);
    }

    #[test]
    fn test_build_jvm_args_profiles() {
        let natives = std::env::temp_dir().join("vn_test_natives");
        let mut inst = InstanceConfig::new("Test", "1.21.4", ModLoader::Fabric);

        inst.optimization_profile = OptimizationProfile::Low;
        let low_hw = HardwareInfo { ram_mb: 4096, ..Default::default() };
        let low_args = build_jvm_args(&inst, 21, &natives, &low_hw);
        assert!(low_args.iter().any(|a| a == "-XX:MaxGCPauseMillis=50"));
        assert!(!low_args.iter().any(|a| a == "-XX:+AlwaysPreTouch"));

        inst.optimization_profile = OptimizationProfile::High;
        let high_hw = HardwareInfo { ram_mb: 16384, has_dedicated_gpu: true, ..Default::default() };
        let high_args = build_jvm_args(&inst, 21, &natives, &high_hw);
        assert!(high_args.iter().any(|a| a == "-XX:MaxGCPauseMillis=30"));
        assert!(high_args.iter().any(|a| a == "-XX:+AlwaysPreTouch"));
    }

    #[test]
    fn test_non_destructive_bobby_override() {
        let dir = std::env::temp_dir().join("vn_test_bobby_preserve");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("mods")).unwrap();
        fs::write(dir.join("mods").join("bobby-5.2.0.jar"), b"dummy").unwrap();
        fs::write(dir.join("options.txt"), "renderDistance:64\nsimulationDistance:12\n").unwrap();

        let low_hw = HardwareInfo { ram_mb: 4096, ..Default::default() };
        let eff = optimize_graphics_options(&dir, OptimizationProfile::Low, &low_hw).unwrap();
        assert_eq!(eff, EffectiveProfile::Low);

        let updated = fs::read_to_string(dir.join("options.txt")).unwrap();
        assert!(updated.contains("renderDistance:64"), "Bobby renderDistance must not be lowered");
        assert!(updated.contains("particles:2"));
        assert!(updated.contains("entityShadows:false"));

        let _ = fs::remove_dir_all(&dir);
    }
}
