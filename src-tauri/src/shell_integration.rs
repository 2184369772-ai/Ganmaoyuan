use crate::storage::{now_string, path_to_string, read_json, write_json_atomic};
use serde::{Deserialize, Serialize};
use std::{
    env, fs,
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
        cleanup_stale_sendto_shortcuts()?;
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
    let shortcut_path = sendto_dir.join(SENDTO_SHORTCUT_NAMES[0]);
    create_shortcut(
        &shortcut_path,
        &current_exe,
        SENDTO_ARGUMENTS,
        current_exe.parent().unwrap_or_else(|| Path::new(".")),
        &current_exe,
    )?;
    for stale_name in SENDTO_SHORTCUT_NAMES.iter().skip(1) {
        let stale_path = sendto_dir.join(stale_name);
        if stale_path.exists() {
            let _ = fs::remove_file(&stale_path);
        }
    }
    let mut settings = read_shell_settings()?;
    settings.sendto_enabled = true;
    settings.updated_at = now_string();
    write_shell_settings(&settings)?;
    Ok(shortcut_path)
}

pub fn unregister_sendto_shortcut() -> Result<(), String> {
    let sendto_dir = sendto_dir()?;
    for shortcut_name in SENDTO_SHORTCUT_NAMES {
        let path = sendto_dir.join(shortcut_name);
        if path.exists() {
            fs::remove_file(&path)
                .map_err(|err| format!("删除 SendTo 入口失败：{}：{err}", path_to_string(&path)))?;
        }
    }
    let mut settings = read_shell_settings()?;
    settings.sendto_enabled = false;
    settings.updated_at = now_string();
    write_shell_settings(&settings)?;
    Ok(())
}

fn cleanup_stale_sendto_shortcuts() -> Result<(), String> {
    let sendto_dir = sendto_dir()?;
    for shortcut_name in SENDTO_SHORTCUT_NAMES.iter().skip(1) {
        let path = sendto_dir.join(shortcut_name);
        if path.exists() {
            let _ = fs::remove_file(&path);
        }
    }
    Ok(())
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
    #[test]
    fn placeholder() {
        assert!(true);
    }
}
