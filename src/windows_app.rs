#[cfg(windows)]
use std::fs;
use std::path::PathBuf;
#[cfg(windows)]
use std::process::Command;
#[cfg(windows)]
use std::thread;

#[allow(dead_code)]
pub fn installed_app_exe_path() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Programs")
        .join("VN Launcher")
        .join("VN Launcher.exe")
}

#[allow(dead_code)]
pub fn start_menu_shortcut_path() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Microsoft")
        .join("Windows")
        .join("Start Menu")
        .join("Programs")
        .join("VN Launcher.lnk")
}

/// Automatically installs VN Launcher into `%LOCALAPPDATA%\Programs\VN Launcher\VN Launcher.exe`,
/// creates Start Menu & Desktop shortcuts, and registers in Windows Installed Apps & App Paths.
#[cfg(windows)]
pub fn install_windows_app(app_version: &str, force_shortcuts: bool) {
    let Ok(cur_exe) = std::env::current_exe() else { return };
    let target_exe = installed_app_exe_path();
    let Some(target_dir) = target_exe.parent() else { return };
    let _ = fs::create_dir_all(target_dir);

    if cur_exe != target_exe {
        let should_copy = match (fs::metadata(&cur_exe), fs::metadata(&target_exe)) {
            (Ok(src_m), Ok(dst_m)) => src_m.len() != dst_m.len() || force_shortcuts,
            (Ok(_), Err(_)) => true,
            _ => false,
        };
        if should_copy {
            let _ = fs::copy(&cur_exe, &target_exe);
        }
    }

    let start_lnk = start_menu_shortcut_path();
    if !force_shortcuts && start_lnk.exists() {
        return;
    }

    let exe_str = target_exe.to_string_lossy().to_string();
    let dir_str = target_dir.to_string_lossy().to_string();
    let start_str = start_lnk.to_string_lossy().to_string();
    let ver_str = app_version.to_string();

    thread::spawn(move || {
        let ps_script = format!(
            r#"$exe = '{exe}'; $dir = '{dir}'; $startLnk = '{start}'; $ver = '{ver}';
$deskLnk = Join-Path ([Environment]::GetFolderPath('Desktop')) 'VN Launcher.lnk';
$wsh = New-Object -ComObject WScript.Shell;
foreach ($lnkPath in @($startLnk, $deskLnk)) {{
    $sc = $wsh.CreateShortcut($lnkPath);
    $sc.TargetPath = $exe;
    $sc.WorkingDirectory = $dir;
    $sc.IconLocation = "$exe,0";
    $sc.Description = 'VN Launcher - Minecraft: Java Edition Launcher';
    $sc.Save();
}}
$uninst = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\VNLauncher';
New-Item -Path $uninst -Force | Out-Null;
Set-ItemProperty -Path $uninst -Name 'DisplayName' -Value 'VN Launcher';
Set-ItemProperty -Path $uninst -Name 'DisplayIcon' -Value "$exe,0";
Set-ItemProperty -Path $uninst -Name 'DisplayVersion' -Value $ver;
Set-ItemProperty -Path $uninst -Name 'Publisher' -Value 'Nguyễn Trọng Việt Anh (Vanh)';
Set-ItemProperty -Path $uninst -Name 'InstallLocation' -Value $dir;
Set-ItemProperty -Path $uninst -Name 'UninstallString' -Value "`"$exe`" --uninstall";
Set-ItemProperty -Path $uninst -Name 'NoModify' -Value 1 -Type DWord;
Set-ItemProperty -Path $uninst -Name 'NoRepair' -Value 1 -Type DWord;
foreach ($ap in @('VN Launcher.exe','vn-launcher.exe')) {{
    $apKey = "HKCU:\Software\Microsoft\Windows\CurrentVersion\App Paths\$ap";
    New-Item -Path $apKey -Force | Out-Null;
    Set-ItemProperty -Path $apKey -Name '(Default)' -Value $exe;
    Set-ItemProperty -Path $apKey -Name 'Path' -Value $dir;
}}"#,
            exe = exe_str.replace('\'', "''"),
            dir = dir_str.replace('\'', "''"),
            start = start_str.replace('\'', "''"),
            ver = ver_str.replace('\'', "''"),
        );
        let mut cmd = Command::new("powershell");
        cmd.args(["-NoProfile", "-WindowStyle", "Hidden", "-Command", &ps_script]);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000);
        }
        let _ = cmd.status();
    });
}

#[cfg(not(windows))]
pub fn install_windows_app(_app_version: &str, _force_shortcuts: bool) {}

#[cfg(windows)]
pub fn uninstall_windows_app() {
    let start_lnk = start_menu_shortcut_path();
    let _ = fs::remove_file(start_lnk);
    let ps_script = r#"
$deskLnk = Join-Path ([Environment]::GetFolderPath('Desktop')) 'VN Launcher.lnk';
if (Test-Path $deskLnk) { Remove-Item $deskLnk -Force -ErrorAction SilentlyContinue }
Remove-Item 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\VNLauncher' -Recurse -Force -ErrorAction SilentlyContinue;
Remove-Item 'HKCU:\Software\Microsoft\Windows\CurrentVersion\App Paths\VN Launcher.exe' -Recurse -Force -ErrorAction SilentlyContinue;
Remove-Item 'HKCU:\Software\Microsoft\Windows\CurrentVersion\App Paths\vn-launcher.exe' -Recurse -Force -ErrorAction SilentlyContinue;
"#;
    let mut cmd = Command::new("powershell");
    cmd.args(["-NoProfile", "-WindowStyle", "Hidden", "-Command", ps_script]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let _ = cmd.status();
}

#[cfg(not(windows))]
pub fn uninstall_windows_app() {}
