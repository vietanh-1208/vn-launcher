use crate::config::{InstanceConfig, LauncherConfig, ModLoader};
use crate::fabric::ensure_fabric_loader;
use crate::java::{ensure_java_runtime, recommended_java_major};
use crate::modrinth::has_mod;
use crate::mojang::{build_http_client, ensure_vanilla_version};
use crate::optimizer::{build_jvm_args, patch_bobby, patch_distant_horizons, prepare_game_dir};
use std::collections::HashSet;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};

pub struct LaunchAccount {
    pub username: String,
    pub uuid: String,
    pub access_token: String,
    pub user_type: String,
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

    let mut classpath: Vec<PathBuf> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
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
    }

    let mut args = build_jvm_args(instance, java_major, &vanilla.natives_dir);
    args.push("-cp".into());
    args.push(
        classpath
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect::<Vec<_>>()
            .join(";"),
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
        cmd.creation_flags(CREATE_NO_WINDOW); // no black console window behind the game
    }
    cmd.spawn()
        .map_err(|e| format!("Không chạy được Java ({}): {}", java_exe.display(), e))
}
