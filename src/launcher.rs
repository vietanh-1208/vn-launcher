use crate::config::{InstanceConfig, LauncherConfig, ModLoader};
use crate::fabric::ensure_fabric_loader;
use crate::java::{ensure_java_runtime, recommended_java_major};
use crate::modloader::ensure_forge_or_neoforge;
use crate::modrinth::{has_mod, install_project, ContentKind};
use crate::mojang::{build_http_client, ensure_vanilla_version};
use crate::optimizer::{
    apply_offline_skin, build_jvm_args, ensure_high_perf_gpu, patch_bobby, patch_distant_horizons, prepare_game_dir,
    HardwareInfo,
};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};

pub struct LaunchAccount {
    pub username: String,
    pub uuid: String,
    pub access_token: String,
    pub user_type: String,
    pub offline_skin: String,
}

/// `--quickPlayMultiplayer` exists since 1.20; older versions use `--server/--port`.
fn supports_quick_play(mc_version: &str) -> bool {
    if let Some(rest) = mc_version.strip_prefix("1.") {
        let minor: u32 = rest.split('.').next().and_then(|m| m.parse().ok()).unwrap_or(0);
        minor >= 20
    } else {
        true // snapshots & new-style versions (e.g. 26.1)
    }
}

/// Downloads everything needed and starts Minecraft. Returns the running game process
/// (stdout/stderr piped so the launcher can show the game log).
pub fn prepare_and_launch(
    config: &LauncherConfig,
    instance: &InstanceConfig,
    account: &LaunchAccount,
    override_server: Option<String>,
    hw: &HardwareInfo,
    status: &Arc<Mutex<String>>,
) -> Result<Child, String> {
    let client = build_http_client(120)?;

    let shared_dir = config.shared_dir();
    let game_dir = config.instance_game_dir(&instance.id);

    let vanilla = ensure_vanilla_version(&client, &shared_dir, &instance.mc_version, status)?;

    let json_java = vanilla.version_info.java_version.as_ref().map(|j| j.major_version);
    let java_major = recommended_java_major(&instance.mc_version, json_java);
    let java_override = if !instance.java_path.trim().is_empty() {
        instance.java_path.clone()
    } else {
        config.custom_java_path.clone()
    };
    let java_exe = ensure_java_runtime(&client, &config.runtimes_dir(), java_major, &java_override, status)?;
    ensure_high_perf_gpu(&java_exe);

    let mut classpath: Vec<PathBuf> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut extra_jvm_args: Vec<String> = Vec::new();
    let mut extra_game_args: Vec<String> = Vec::new();
    let main_class = match instance.loader {
        ModLoader::Fabric => {
            let fabric = ensure_fabric_loader(
                &client,
                &shared_dir,
                &instance.mc_version,
                instance.loader_version.as_deref(),
                status,
            )?;
            for (ga, jar) in fabric.fabric_jars {
                seen.insert(ga);
                classpath.push(jar);
            }
            fabric.main_class
        }
        ModLoader::NeoForge | ModLoader::Forge => {
            let spec = ensure_forge_or_neoforge(
                &client,
                &java_exe,
                &shared_dir,
                &instance.mc_version,
                instance.loader,
                status,
            )?;
            for (ga, jar) in spec.jars {
                if !ga.is_empty() {
                    seen.insert(ga);
                }
                if !classpath.contains(&jar) {
                    classpath.push(jar);
                }
            }
            extra_jvm_args = spec.extra_jvm_args;
            extra_game_args = spec.extra_game_args;
            spec.main_class
        }
        ModLoader::Vanilla => vanilla.version_info.main_class.clone(),
    };
    for (ga, jar) in &vanilla.classpath_jars {
        if seen.contains(ga) || !jar.exists() || classpath.contains(jar) {
            continue;
        }
        seen.insert(ga.clone());
        classpath.push(jar.clone());
    }
    classpath.push(vanilla.client_jar.clone());

    prepare_game_dir(&game_dir)?;
    if instance.loader == ModLoader::Fabric {
        if has_mod(&game_dir, "bobby") {
            patch_bobby(&game_dir, instance.lod_render_distance);
        }
        if has_mod(&game_dir, "distanthorizons") {
            patch_distant_horizons(&game_dir, instance.lod_render_distance);
        }
        if !account.offline_skin.trim().is_empty() {
            let skin_p = Path::new(account.offline_skin.trim());
            if skin_p.exists() {
                if !has_mod(&game_dir, "customskinloader") {
                    let _ = install_project(
                        &client,
                        ContentKind::Mod,
                        "customskinloader",
                        &instance.mc_version,
                        &game_dir,
                        status,
                    );
                }
                let _ = apply_offline_skin(&game_dir, &account.username, skin_p);
            }
        }
    }

    let mut args = build_jvm_args(instance, java_major, &vanilla.natives_dir, hw);
    args.extend(extra_jvm_args);
    args.push("-cp".into());
    let cp_sep = if cfg!(windows) { ";" } else { ":" };
    args.push(
        classpath
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect::<Vec<_>>()
            .join(cp_sep),
    );
    args.push(main_class);
    args.extend([
        "--username".into(),
        account.username.clone(),
        "--version".into(),
        instance.mc_version.clone(),
        "--gameDir".into(),
        game_dir.to_string_lossy().to_string(),
        "--assetsDir".into(),
        vanilla.assets_dir.to_string_lossy().to_string(),
        "--assetIndex".into(),
        vanilla.asset_index_id.clone(),
        "--uuid".into(),
        account.uuid.clone(),
        "--accessToken".into(),
        account.access_token.clone(),
        "--userType".into(),
        account.user_type.clone(),
        "--versionType".into(),
        "VN Launcher by Vanh".into(),
    ]);
    args.extend(extra_game_args);
    if instance.fullscreen {
        args.push("--fullscreen".into());
    } else {
        args.extend([
            "--width".into(),
            instance.window_width.max(640).to_string(),
            "--height".into(),
            instance.window_height.max(480).to_string(),
        ]);
    }

    let server = override_server
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| instance.quick_join_server.trim().to_string());
    if !server.is_empty() {
        if supports_quick_play(&instance.mc_version) {
            args.push("--quickPlayMultiplayer".into());
            args.push(server);
        } else {
            let mut parts = server.splitn(2, ':');
            let host = parts.next().unwrap_or_default().to_string();
            let port = parts.next().unwrap_or("25565").to_string();
            args.extend(["--server".into(), host, "--port".into(), port]);
        }
    }

    if let Ok(mut s) = status.lock() {
        *s = format!("Đang mở Minecraft {}...", instance.mc_version);
    }

    let mut cmd = Command::new(&java_exe);
    cmd.current_dir(&game_dir)
        .args(&args)
        .env("__GL_THREADED_OPTIMIZATIONS", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        const ABOVE_NORMAL_PRIORITY_CLASS: u32 = 0x0000_8000;
        cmd.creation_flags(CREATE_NO_WINDOW | ABOVE_NORMAL_PRIORITY_CLASS); // no black console + smooth CPU priority
    }
    cmd.spawn()
        .map_err(|e| format!("Không chạy được Java ({}): {}", java_exe.display(), e))
}
