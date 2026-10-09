use crate::storage::{now_string, path_to_string, read_json, write_json_atomic};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    env, fs,
    io::Write,
    path::{Path, PathBuf},
};
#[cfg(target_os = "windows")]
use windows::{
    core::{Interface, HSTRING},
    Win32::{
        System::Com::{
            CoCreateInstance, CoInitializeEx, CoUninitialize, IPersistFile, CLSCTX_INPROC_SERVER,
            COINIT_APARTMENTTHREADED,
        },
        UI::Shell::{IShellLinkW, ShellLink},
    },
};

const SENDTO_SHORTCUT_NAMES: [&str; 2] = ["感冒院.lnk", "Ganmaoyuan.lnk"];
const SENDTO_ARGUMENTS: &str = "--shell-source windowsSendTo";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct ShellIntegrationSettings {
    #[serde(default = "default_true")]
    sendto_enabled: bool,
    #[serde(default)]
    updated_at: String,
    #[serde(default)]
    sendto_shortcut_path: Option<String>,
    #[serde(default)]
    sendto_shortcut_sha256: Option<String>,
}

fn default_true() -> bool {
    true
}

pub fn maybe_register_sendto_shortcut(current_exe: &Path) -> Result<(), String> {
    #[cfg(not(target_os = "windows"))]
    {
        let _ = current_exe;
        return Ok(());
    }
    #[cfg(target_os = "windows")]
    {
        if !read_shell_settings()?.sendto_enabled {
            return Ok(());
        }
        register_sendto_shortcut(current_exe)?;
        Ok(())
    }
}

pub fn register_sendto_shortcut(current_exe: &Path) -> Result<PathBuf, String> {
    let current_exe = current_exe.canonicalize().map_err(|err| {
        format!(
            "规范化感冒院程序路径失败：{}：{err}",
            path_to_string(current_exe)
        )
    })?;
    if !current_exe.exists() {
        return Err(format!(
            "感冒院程序不存在：{}",
            path_to_string(&current_exe)
        ));
    }
    let sendto_dir = sendto_dir()?;
    fs::create_dir_all(&sendto_dir).map_err(|err| {
        format!(
            "创建 Windows SendTo 目录失败：{}：{err}",
            path_to_string(&sendto_dir)
        )
    })?;
    let mut settings = read_shell_settings()?;
    let (shortcut_path, reserved_placeholder) =
        if let Some(owned_path) = recorded_owned_shortcut(&sendto_dir, &settings) {
            (owned_path, None)
        } else {
            let (path, placeholder) = reserve_sendto_shortcut_path(&sendto_dir)?;
            (path, Some(placeholder))
        };
    if let Err(error) = create_shortcut(
        &shortcut_path,
        &current_exe,
        SENDTO_ARGUMENTS,
        current_exe.parent().unwrap_or_else(|| Path::new(".")),
        &current_exe,
    ) {
        if let Some(placeholder) = reserved_placeholder {
            remove_if_placeholder(&shortcut_path, &placeholder);
        }
        return Err(error);
    }
    settings.sendto_shortcut_path = Some(path_to_string(&shortcut_path));
    settings.sendto_shortcut_sha256 = Some(file_sha256(&shortcut_path)?);
    settings.sendto_enabled = true;
    settings.updated_at = now_string();
    write_shell_settings(&settings)?;
    Ok(shortcut_path)
}

pub fn unregister_sendto_shortcut() -> Result<(), String> {
    let sendto_dir = sendto_dir()?;
    let mut settings = read_shell_settings()?;
    if let Some(path) = recorded_owned_shortcut(&sendto_dir, &settings) {
        if path.exists() {
            fs::remove_file(&path).map_err(|err| {
                format!(
                    "删除感冒院创建的 SendTo 入口失败：{}：{err}",
                    path_to_string(&path)
                )
            })?;
        }
    }
    settings.sendto_enabled = false;
    settings.sendto_shortcut_path = None;
    settings.sendto_shortcut_sha256 = None;
    settings.updated_at = now_string();
    write_shell_settings(&settings)?;
    Ok(())
}

fn recorded_owned_shortcut(
    sendto_dir: &Path,
    settings: &ShellIntegrationSettings,
) -> Option<PathBuf> {
    let path = PathBuf::from(settings.sendto_shortcut_path.as_ref()?);
    let recorded_hash = settings.sendto_shortcut_sha256.as_deref()?;
    if path.parent() != Some(sendto_dir)
        || path.extension().and_then(|ext| ext.to_str()) != Some("lnk")
    {
        return None;
    }
    let actual_hash = file_sha256(&path).ok()?;
    if should_remove_owned_shortcut(
        &path,
        sendto_dir,
        Some(&path),
        Some(recorded_hash),
        Some(&actual_hash),
    ) {
        Some(path)
    } else {
        None
    }
}

fn should_remove_owned_shortcut(
    candidate: &Path,
    sendto_dir: &Path,
    recorded_path: Option<&Path>,
    recorded_hash: Option<&str>,
    actual_hash: Option<&str>,
) -> bool {
    candidate.parent() == Some(sendto_dir)
        && candidate.extension().and_then(|ext| ext.to_str()) == Some("lnk")
        && recorded_path == Some(candidate)
        && recorded_hash.is_some()
        && recorded_hash == actual_hash
}

fn reserve_sendto_shortcut_path(sendto_dir: &Path) -> Result<(PathBuf, Vec<u8>), String> {
    let mut candidates = SENDTO_SHORTCUT_NAMES
        .iter()
        .map(|name| sendto_dir.join(name))
        .chain((2_u32..).map(|index| sendto_dir.join(format!("感冒院 (Ganmaoyuan {index}).lnk"))));
    loop {
        let candidate = candidates
            .next()
            .expect("numbered SendTo names are unbounded");
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(mut file) => {
                let placeholder = format!("Ganmaoyuan SendTo reservation {}", uuid::Uuid::new_v4());
                let placeholder = placeholder.into_bytes();
                if let Err(err) = file.write_all(&placeholder) {
                    drop(file);
                    remove_if_placeholder(&candidate, &placeholder);
                    return Err(format!(
                        "预留 SendTo 名称失败：{}：{err}",
                        path_to_string(&candidate)
                    ));
                }
                return Ok((candidate, placeholder));
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(format!(
                    "预留 SendTo 名称失败：{}：{error}",
                    path_to_string(&candidate)
                ));
            }
        }
    }
}

fn remove_if_placeholder(path: &Path, placeholder: &[u8]) {
    if fs::read(path).ok().as_deref() == Some(placeholder) {
        let _ = fs::remove_file(path);
    }
}

fn file_sha256(path: &Path) -> Result<String, String> {
    let bytes = fs::read(path)
        .map_err(|err| format!("读取 SendTo 快捷方式失败：{}：{err}", path_to_string(path)))?;
    let digest = Sha256::digest(bytes);
    Ok(digest.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn create_shortcut(
    shortcut_path: &Path,
    target_path: &Path,
    arguments: &str,
    working_directory: &Path,
    icon_path: &Path,
) -> Result<(), String> {
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (
            shortcut_path,
            target_path,
            arguments,
            working_directory,
            icon_path,
        );
        return Ok(());
    }
    #[cfg(target_os = "windows")]
    unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED)
            .ok()
            .map_err(|err| format!("初始化 Windows Shell COM 失败：{err}"))?;
        let result = (|| -> Result<(), String> {
            let shell_link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)
                .map_err(|err| format!("创建 Windows 快捷方式对象失败：{err}"))?;
            shell_link
                .SetPath(&HSTRING::from(path_to_string(target_path)))
                .map_err(|err| format!("写入快捷方式目标失败：{err}"))?;
            shell_link
                .SetArguments(&HSTRING::from(arguments))
                .map_err(|err| format!("写入快捷方式参数失败：{err}"))?;
            shell_link
                .SetWorkingDirectory(&HSTRING::from(path_to_string(working_directory)))
                .map_err(|err| format!("写入快捷方式工作目录失败：{err}"))?;
            shell_link
                .SetIconLocation(&HSTRING::from(path_to_string(icon_path)), 0)
                .map_err(|err| format!("写入快捷方式图标失败：{err}"))?;
            let persist: IPersistFile = shell_link
                .cast()
                .map_err(|err| format!("获取快捷方式持久化接口失败：{err}"))?;
            persist
                .Save(&HSTRING::from(path_to_string(shortcut_path)), true)
                .map_err(|err| format!("保存快捷方式失败：{err}"))?;
            Ok(())
        })();
        CoUninitialize();
        result
    }
}

fn sendto_dir() -> Result<PathBuf, String> {
    let appdata = env::var_os("APPDATA")
        .map(PathBuf::from)
        .ok_or_else(|| "无法获取 APPDATA，不能创建 Windows SendTo 入口。".to_string())?;
    Ok(appdata.join("Microsoft").join("Windows").join("SendTo"))
}

fn shell_settings_path() -> Result<PathBuf, String> {
    Ok(global_data_dir_path()?.join("shell-integrations.json"))
}

fn global_data_dir_path() -> Result<PathBuf, String> {
    if let Some(configured) = env::var_os("GANMAOYUAN_DATA_DIR") {
        let path = PathBuf::from(configured);
        fs::create_dir_all(&path)
            .map_err(|err| format!("创建感冒院数据目录失败：{}：{err}", path_to_string(&path)))?;
        return Ok(path);
    }
    let preferred = PathBuf::from(r"D:\GanMaoYuan\AppData");
    let fallback = env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("com.ganmaoyuan.desktop");
    // Keep existing installations together while avoiding a new hard dependency on D:.
    let path = if preferred.exists() {
        preferred
    } else {
        fallback
    };
    fs::create_dir_all(&path)
        .map_err(|err| format!("创建感冒院数据目录失败：{}：{err}", path_to_string(&path)))?;
    Ok(path)
}

fn read_shell_settings() -> Result<ShellIntegrationSettings, String> {
    let path = shell_settings_path()?;
    if !path.exists() {
        return Ok(ShellIntegrationSettings::default());
    }
    read_json(&path)
}

fn write_shell_settings(settings: &ShellIntegrationSettings) -> Result<(), String> {
    write_json_atomic(&shell_settings_path()?, settings)
}

#[cfg(test)]
mod tests {
    use super::should_remove_owned_shortcut;
    use std::path::Path;

    #[test]
    fn unregister_only_removes_exact_recorded_shortcut_bytes() {
        let directory = Path::new(r"C:\Users\Example\AppData\Roaming\Microsoft\Windows\SendTo");
        let owned = directory.join("感冒院.lnk");
        assert!(should_remove_owned_shortcut(
            &owned,
            directory,
            Some(&owned),
            Some("owned-hash"),
            Some("owned-hash")
        ));
        assert!(!should_remove_owned_shortcut(
            &owned,
            directory,
            Some(&owned),
            Some("owned-hash"),
            Some("user-replaced-hash")
        ));
        assert!(!should_remove_owned_shortcut(
            &owned,
            directory,
            None,
            None,
            Some("owned-hash")
        ));
        let outside = Path::new(
            r"C:\Users\Example\AppData\Roaming\Microsoft\Windows\SendTo\..\Documents\user.lnk",
        );
        assert!(!should_remove_owned_shortcut(
            outside,
            directory,
            Some(outside),
            Some("same"),
            Some("same")
        ));
    }
}
