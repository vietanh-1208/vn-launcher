use crate::config::InstanceConfig;
use std::fs;
use std::path::Path;

/// JVM arguments tuned for a smooth client (Aikar-style G1GC; optional ZGC on Java 21+).
/// No AlwaysPreTouch / huge -Xms: on 8 GB laptops these force Windows to swap and cause stutter.
pub fn build_jvm_args(instance: &InstanceConfig, java_major: u32, natives_dir: &Path) -> Vec<String> {
    let ram = instance.ram_mb.max(1024);
    let xms = ram.min(1024);
    let mut args = vec![format!("-Xmx{}M", ram), format!("-Xms{}M", xms)];

    if instance.use_zgc && java_major >= 21 {
        args.push("-XX:+UseZGC".into());
        if java_major < 23 {
            args.push("-XX:+ZGenerational".into());
        }
    } else {
        for a in [
            "-XX:+UseG1GC",
            "-XX:+ParallelRefProcEnabled",
            "-XX:MaxGCPauseMillis=200",
            "-XX:+UnlockExperimentalVMOptions",
            "-XX:+DisableExplicitGC",
            "-XX:G1NewSizePercent=30",
            "-XX:G1MaxNewSizePercent=40",
            "-XX:G1HeapRegionSize=8M",
            "-XX:G1ReservePercent=20",
            "-XX:G1HeapWastePercent=5",
            "-XX:G1MixedGCCountTarget=4",
            "-XX:InitiatingHeapOccupancyPercent=15",
            "-XX:G1MixedGCLiveThresholdPercent=90",
            "-XX:G1RSetUpdatingPauseTimePercent=5",
            "-XX:SurvivorRatio=32",
            "-XX:+PerfDisableSharedMem",
            "-XX:MaxTenuringThreshold=1",
            "-XX:+AlwaysActAsServerClassMachine",
            "-XX:+UseStringDeduplication",
        ] {
            args.push(a.into());
        }
    }
    if java_major >= 21 {
        args.push("--enable-native-access=ALL-UNNAMED".into());
    }

    let natives = natives_dir.to_string_lossy();
    args.push(format!("-Djava.library.path={}", natives));
    args.push(format!("-Dorg.lwjgl.librarypath={}", natives));
    args.push("-Dminecraft.launcher.brand=VN-Launcher-by-Vanh".into());
    args.push("-Dminecraft.launcher.version=2.0.0".into());

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
        let _ = fs::write(
            &options,
            "lang:vi_vn\nrenderDistance:12\nsimulationDistance:8\nenableVsync:false\nmaxFps:260\ngraphicsMode:1\nrenderClouds:\"false\"\nentityDistanceScaling:0.75\nbiomeBlendRadius:1\nparticles:1\n",
        );
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
