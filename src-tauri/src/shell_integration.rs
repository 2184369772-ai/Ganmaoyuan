use crate::storage::{
    global_data_dir_from_environment, now_string, path_to_string, read_json, write_json_atomic,
};
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
            COINIT_APARTMENTTHREADED, STGM_READ,
        },
        UI::Shell::{IShellLinkW, ShellLink},
    },
};

const SENDTO_SHORTCUT_NAME: &str = "Ganmaoyuan - SendTo.lnk";
const SENDTO_ARGUMENTS: &str = "--shell-source windowsSendTo";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct ShellIntegrationSettings {
    #[serde(default = "default_true")]
    sendto_enabled: bool,
    #[serde(default)]
    managed_sendto_shortcut_path: String,
    #[serde(default)]
    managed_sendto_target_path: String,
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
        if register_sendto_shortcut(current_exe).is_err() {
            // A name conflict must not prevent the main application from starting.
            log::warn!("SendTo registration skipped; existing user shortcuts were preserved.");
        }
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
    let shortcut_path = sendto_dir.join(SENDTO_SHORTCUT_NAME);
    let mut settings = read_shell_settings()?;
    let previously_managed = same_file_path(
        Path::new(&settings.managed_sendto_shortcut_path),
        &shortcut_path,
    ) && same_file_path(
        Path::new(&settings.managed_sendto_target_path),
        &current_exe,
    );
    let existing = match fs::symlink_metadata(&shortcut_path) {
        Ok(metadata) => {
            if is_link_or_reparse(&metadata) || !metadata.is_file() {
                return Err("SendTo 入口同名项目不属于当前安装，已保留且未覆盖。".to_string());
            }
            Some(shortcut_matches(
                &shortcut_path,
                &current_exe,
                SENDTO_ARGUMENTS,
            )?)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(format!("检查 SendTo 入口失败：{error}")),
    };
    let created_by_this_call =
        match shortcut_disposition(existing.is_some(), existing.unwrap_or(false)) {
            ShortcutDisposition::Create => {
                create_shortcut(
                    &shortcut_path,
                    &current_exe,
                    SENDTO_ARGUMENTS,
                    current_exe.parent().unwrap_or_else(|| Path::new(".")),
                    &current_exe,
                )?;
                true
            }
            ShortcutDisposition::ReuseVerified => false,
            ShortcutDisposition::PreserveConflict => {
                return Err("SendTo 入口同名项目不属于当前安装，已保留且未覆盖。".to_string());
            }
        };
    settings.sendto_enabled = true;
    if created_by_this_call || previously_managed {
        settings.managed_sendto_shortcut_path = path_to_string(&shortcut_path);
        settings.managed_sendto_target_path = path_to_string(&current_exe);
    }
    settings.updated_at = now_string();
    if let Err(error) = write_shell_settings(&settings) {
        if created_by_this_call {
            if let Ok(metadata) = fs::symlink_metadata(&shortcut_path) {
                if metadata.is_file()
                    && !is_link_or_reparse(&metadata)
                    && shortcut_matches(&shortcut_path, &current_exe, SENDTO_ARGUMENTS)
                        .unwrap_or(false)
                {
                    let _ = fs::remove_file(&shortcut_path);
                }
            }
        }
        return Err(error);
    }
    Ok(shortcut_path)
}

pub fn unregister_sendto_shortcut(current_exe: &Path) -> Result<(), String> {
    let sendto_dir = sendto_dir()?;
    let mut settings = read_shell_settings()?;
    let recorded_path = PathBuf::from(&settings.managed_sendto_shortcut_path);
    let current_exe = current_exe
        .canonicalize()
        .unwrap_or_else(|_| current_exe.to_path_buf());
    let allowed_path = recorded_path.file_name().and_then(|name| name.to_str())
        == Some(SENDTO_SHORTCUT_NAME)
        && recorded_path
            .parent()
            .and_then(|parent| parent.canonicalize().ok())
            == sendto_dir.canonicalize().ok()
        && same_file_path(
            Path::new(&settings.managed_sendto_target_path),
            &current_exe,
        );
    if allowed_path {
        match fs::symlink_metadata(&recorded_path) {
            Ok(metadata)
                if metadata.is_file()
                    && !is_link_or_reparse(&metadata)
                    && should_remove_managed_shortcut(
                        true,
                        shortcut_matches(&recorded_path, &current_exe, SENDTO_ARGUMENTS)?,
                    ) =>
            {
                fs::remove_file(&recorded_path)
                    .map_err(|err| format!("删除已核验的 SendTo 入口失败：{err}"))?;
            }
            // The shortcut was replaced or its target cannot be verified. Keep it.
            Ok(_) | Err(_) => {}
        }
    }
    settings.sendto_enabled = false;
    settings.managed_sendto_shortcut_path.clear();
    settings.managed_sendto_target_path.clear();
    settings.updated_at = now_string();
    write_shell_settings(&settings)?;
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShortcutDisposition {
    Create,
    ReuseVerified,
    PreserveConflict,
}

fn shortcut_disposition(exists: bool, target_and_args_match: bool) -> ShortcutDisposition {
    match (exists, target_and_args_match) {
        (false, _) => ShortcutDisposition::Create,
        (true, true) => ShortcutDisposition::ReuseVerified,
        (true, false) => ShortcutDisposition::PreserveConflict,
    }
}

fn should_remove_managed_shortcut(
    recorded_for_this_install: bool,
    target_and_args_match: bool,
) -> bool {
    recorded_for_this_install && target_and_args_match
}

fn is_link_or_reparse(metadata: &fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        return metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0;
    }
    #[cfg(not(windows))]
    false
}

fn same_file_path(left: &Path, right: &Path) -> bool {
    let left = left.canonicalize().unwrap_or_else(|_| left.to_path_buf());
    let right = right.canonicalize().unwrap_or_else(|_| right.to_path_buf());
    path_to_string(&left).eq_ignore_ascii_case(&path_to_string(&right))
}

fn shortcut_matches(path: &Path, target: &Path, arguments: &str) -> Result<bool, String> {
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (path, target, arguments);
        Ok(false)
    }
    #[cfg(target_os = "windows")]
    {
        let Some((actual_target, actual_arguments)) = read_shortcut(path)? else {
            return Ok(false);
        };
        Ok(actual_arguments.trim() == arguments && same_file_path(&actual_target, target))
    }
}

#[cfg(target_os = "windows")]
fn read_shortcut(path: &Path) -> Result<Option<(PathBuf, String)>, String> {
    use std::ptr::null_mut;

    unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED)
            .ok()
            .map_err(|err| format!("初始化 Windows Shell COM 失败：{err}"))?;
        let result = (|| -> Result<Option<(PathBuf, String)>, String> {
            let shell_link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)
                .map_err(|err| format!("读取 Windows 快捷方式失败：{err}"))?;
            let persist: IPersistFile = shell_link
                .cast()
                .map_err(|err| format!("读取快捷方式持久化接口失败：{err}"))?;
            persist
                .Load(&HSTRING::from(path_to_string(path)), STGM_READ)
                .map_err(|err| format!("加载快捷方式失败：{err}"))?;
            let mut target_buffer = vec![0_u16; 32_768];
            shell_link
                .GetPath(&mut target_buffer, null_mut(), 0)
                .map_err(|err| format!("读取快捷方式目标失败：{err}"))?;
            let mut arguments_buffer = vec![0_u16; 8_192];
            shell_link
                .GetArguments(&mut arguments_buffer)
                .map_err(|err| format!("读取快捷方式参数失败：{err}"))?;
            let target = String::from_utf16_lossy(
                &target_buffer[..target_buffer
                    .iter()
                    .position(|value| *value == 0)
                    .unwrap_or(target_buffer.len())],
            );
            let arguments = String::from_utf16_lossy(
                &arguments_buffer[..arguments_buffer
                    .iter()
                    .position(|value| *value == 0)
                    .unwrap_or(arguments_buffer.len())],
            );
            if target.trim().is_empty() {
                return Ok(None);
            }
            Ok(Some((PathBuf::from(target), arguments)))
        })();
        CoUninitialize();
        result
    }
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
    global_data_dir_from_environment()
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
    fn sendto_name_collision_preserves_unowned_shortcut() {
        use super::{shortcut_disposition, ShortcutDisposition};
        assert_eq!(
            shortcut_disposition(false, false),
            ShortcutDisposition::Create
        );
        assert_eq!(
            shortcut_disposition(true, true),
            ShortcutDisposition::ReuseVerified
        );
        assert_eq!(
            shortcut_disposition(true, false),
            ShortcutDisposition::PreserveConflict
        );
    }

    #[test]
    fn only_a_recorded_shortcut_with_matching_target_and_arguments_is_removable() {
        use super::should_remove_managed_shortcut;
        assert!(!should_remove_managed_shortcut(false, true));
        assert!(!should_remove_managed_shortcut(true, false));
        assert!(should_remove_managed_shortcut(true, true));
    }

    #[test]
    fn nsis_never_deletes_sendto_shortcuts_by_legacy_filename() {
        let script = include_str!("../installer/sendto-installer.nsi");
        assert!(!script.contains("Delete \"$SENDTO\\感冒院.lnk\""));
        assert!(!script.contains("Delete \"$SENDTO\\Ganmaoyuan.lnk\""));
        assert!(script.contains("IsShortcutTarget \"$SENDTO\\Ganmaoyuan - SendTo.lnk\""));
        assert!(script.contains("WriteRegStr SHCTX \"${UNINSTKEY}\" \"ManagedSendToShortcut\""));
        assert!(script.contains("ReadRegStr $1 SHCTX \"${UNINSTKEY}\" \"ManagedSendToShortcut\""));
        assert!(
            script.contains("ReadRegStr $R0 SHCTX \"${UNINSTKEY}\" \"ManagedStartMenuShortcut\"")
        );
        assert!(script.contains("ReadRegStr $R0 SHCTX \"${UNINSTKEY}\" \"ManagedDesktopShortcut\""));
        assert!(script.contains("WriteRegStr SHCTX \"${UNINSTKEY}\" \"ManagedDesktopShortcut\" \"$DESKTOP\\${PRODUCTNAME}.lnk\""));
        assert!(script.contains("Preserving an existing desktop shortcut with a different target."));
        assert!(script.contains("$OldMainBinaryName != \"${PRODUCTNAME}.exe\""));
        assert!(script.contains("Delete \"$INSTDIR\\${PRODUCTNAME}.exe\""));
        assert!(!script.contains("Delete \"$INSTDIR\\$OldMainBinaryName\""));
        assert!(!script.contains("\"$INSTDIR\\$OldMainBinaryName\""));
    }

    #[test]
    fn nsis_user_data_cleanup_does_not_follow_reparse_points() {
        let script = include_str!("../installer/sendto-installer.nsi");
        let cleanup = script
            .split("Function un.RemoveUserDataTree")
            .nth(1)
            .and_then(|body| body.split("FunctionEnd").next())
            .expect("user data cleanup function exists");
        let attribute_check = cleanup
            .find("GetFileAttributesW(w \"$R0\")")
            .expect("cleanup checks attributes before traversing");
        let reparse_guard = cleanup
            .find("IntOp $R5 $R4 & 0x400")
            .expect("cleanup detects reparse points");
        let recursive_call = cleanup
            .find("Call un.RemoveUserDataTree")
            .expect("cleanup recursively handles ordinary child directories");
        assert!(attribute_check < reparse_guard);
        assert!(reparse_guard < recursive_call);
        assert!(!cleanup.contains("RMDir /r"));
        assert!(script.contains("StrCpy $R0 \"$APPDATA\\${BUNDLEID}\""));
        assert!(script.contains("StrCpy $R0 \"$LOCALAPPDATA\\${BUNDLEID}\""));
        assert!(!script.contains("RmDir /r \"$APPDATA\\${BUNDLEID}\""));
        assert!(!script.contains("RmDir /r \"$LOCALAPPDATA\\${BUNDLEID}\""));
        assert!(script.contains("${If} $DeleteAppDataCheckboxState = 1"));
        assert!(script.contains("${AndIf} $UpdateMode <> 1"));
    }
}
