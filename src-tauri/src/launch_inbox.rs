use crate::{
    models::MaterialInboxItem,
    project_service,
    storage::{global_data_dir, now_string, path_to_string, read_json, write_json_atomic},
};
use serde::{Deserialize, Serialize};
use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    sync::OnceLock,
};
use tauri::Runtime;
#[cfg(target_os = "windows")]
use windows::{
    core::HSTRING,
    Win32::{
        Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, HWND},
        System::Threading::CreateMutexW,
        UI::WindowsAndMessaging::{FindWindowW, SetForegroundWindow, ShowWindow, SW_RESTORE},
    },
};

#[cfg(target_os = "windows")]
static PRIMARY_INSTANCE_MUTEX: OnceLock<isize> = OnceLock::new();

#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct LaunchInboxQueueDocument {
    #[serde(default)]
    entries: Vec<LaunchInboxEntry>,
    #[serde(default)]
    updated_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
struct LaunchInboxEntry {
    #[serde(default)]
    file_path: String,
    #[serde(default)]
    received_from: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LaunchRequest {
    pub file_paths: Vec<String>,
    pub received_from: String,
}

pub fn collect_launch_request(args: impl IntoIterator<Item = OsString>) -> LaunchRequest {
    let mut received_from = "appImport".to_string();
    let mut values = Vec::new();

    let mut iter = args.into_iter();
    while let Some(value) = iter.next() {
        let text = value.to_string_lossy().to_string();
        if text == "--shell-source" {
            if let Some(source) = iter.next() {
                let source_value = source.to_string_lossy().trim().to_string();
                if !source_value.is_empty() {
                    received_from = source_value;
                }
            }
            continue;
        }
        if text == "--register-sendto" || text == "--unregister-sendto" {
            continue;
        }
        values.push(OsString::from(text));
    }

    let mut file_paths = Vec::new();
    let mut index = 0;

    while index < values.len() {
        if let Some((next_index, file_path)) = match_existing_file_path(&values, index) {
            file_paths.push(file_path);
            index = next_index;
        } else {
            index += 1;
        }
    }

    LaunchRequest {
        file_paths,
        received_from,
    }
}

#[cfg(target_os = "windows")]
pub fn acquire_primary_instance() -> Result<bool, String> {
    if PRIMARY_INSTANCE_MUTEX.get().is_some() {
        return Ok(true);
    }
    let name = HSTRING::from("GanmaoyuanPrimaryInstance");
    let handle = unsafe { CreateMutexW(None, false, &name) }
        .map_err(|err| format!("创建感冒院单实例互斥体失败：{err}"))?;
    let raw = handle.0 as isize;
    let already_exists =
        unsafe { windows::Win32::Foundation::GetLastError() } == ERROR_ALREADY_EXISTS;
    if already_exists {
        unsafe {
            let _ = CloseHandle(handle);
        }
        return Ok(false);
    }
    let _ = PRIMARY_INSTANCE_MUTEX.set(raw);
    Ok(true)
}

#[cfg(not(target_os = "windows"))]
pub fn acquire_primary_instance() -> Result<bool, String> {
    Ok(true)
}

#[cfg(target_os = "windows")]
pub fn focus_existing_window(window_title: &str) {
    let title = HSTRING::from(window_title);
    let hwnd = unsafe { FindWindowW(None, &title) };
    if let Ok(hwnd) = hwnd {
        if hwnd == HWND(std::ptr::null_mut()) {
            return;
        }
        unsafe {
            let _ = ShowWindow(hwnd, SW_RESTORE);
            let _ = SetForegroundWindow(hwnd);
        }
    }
}

#[cfg(not(target_os = "windows"))]
pub fn focus_existing_window(_window_title: &str) {}

pub fn enqueue_launch_file_paths<R: Runtime>(
    app: &tauri::AppHandle<R>,
    request: &LaunchRequest,
) -> Result<(), String> {
    if request.file_paths.is_empty() {
        return Ok(());
    }
    let path = launch_inbox_queue_path(app)?;
    let mut document: LaunchInboxQueueDocument = if path.exists() {
        read_json(&path)?
    } else {
        LaunchInboxQueueDocument::default()
    };
    for file_path in &request.file_paths {
        if !document
            .entries
            .iter()
            .any(|existing| normalize_key(&existing.file_path) == normalize_key(file_path))
        {
            document.entries.push(LaunchInboxEntry {
                file_path: file_path.clone(),
                received_from: request.received_from.clone(),
            });
        }
    }
    document.updated_at = now_string();
    write_json_atomic(&path, &document)
}

fn consume_launch_entries<R: Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<Vec<LaunchInboxEntry>, String> {
    consume_launch_entries_from_path(&launch_inbox_queue_path(app)?)
}

fn consume_launch_entries_from_path(path: &Path) -> Result<Vec<LaunchInboxEntry>, String> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let mut document: LaunchInboxQueueDocument = read_json(&path)?;
    // Polling an empty handoff queue must be read-only. Rewriting it continuously
    // causes needless disk churn while the desktop app is idle.
    if document.entries.is_empty() {
        return Ok(Vec::new());
    }
    let entries = document.entries.drain(..).collect::<Vec<_>>();
    document.updated_at = now_string();
    write_json_atomic(&path, &document)?;
    Ok(entries)
}

pub fn consume_launch_inbox_entries<R: Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<Vec<MaterialInboxItem>, String> {
    let entries = consume_launch_entries(app)?;
    if entries.is_empty() {
        return Ok(Vec::new());
    }
    let mut by_source = std::collections::BTreeMap::<String, Vec<String>>::new();
    for entry in entries {
        by_source
            .entry(entry.received_from)
            .or_default()
            .push(entry.file_path);
    }
    let mut results = Vec::new();
    for (received_from, file_paths) in by_source {
        results.extend(project_service::receive_inbox_files_with_source(
            app,
            file_paths,
            &received_from,
        )?);
    }
    Ok(results)
}

fn launch_inbox_queue_path<R: Runtime>(app: &tauri::AppHandle<R>) -> Result<PathBuf, String> {
    Ok(global_data_dir(app)?.join("launch-inbox-queue.json"))
}

fn normalize_key(value: &str) -> String {
    value.replace('/', "\\").to_lowercase()
}

fn match_existing_file_path(values: &[OsString], start: usize) -> Option<(usize, String)> {
    let mut candidate = OsString::new();
    let mut matched = None;

    for index in start..values.len() {
        if index > start {
            candidate.push(" ");
        }
        candidate.push(&values[index]);

        let path = PathBuf::from(&candidate);
        if !path.exists() || !path.is_file() {
            continue;
        }
        if let Ok(canonical) = fs::canonicalize(&path) {
            matched = Some((index + 1, path_to_string(&canonical)));
        }
    }

    matched
}

#[cfg(test)]
mod tests {
    use super::{
        collect_launch_request, consume_launch_entries_from_path, LaunchInboxQueueDocument,
    };
    use crate::storage::write_json_atomic;
    use std::{ffi::OsString, fs, path::PathBuf};

    #[test]
    fn collects_existing_file_args_only() {
        let base = std::env::temp_dir().join("ganmaoyuan-launch-args-test");
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        let file = base.join("测试 文件.txt");
        fs::write(&file, "hello").unwrap();
        let dir = base.join("folder");
        fs::create_dir_all(&dir).unwrap();

        let request = collect_launch_request(vec![
            OsString::from(file.as_os_str()),
            OsString::from(dir.as_os_str()),
            OsString::from(PathBuf::from("not-exists.txt").as_os_str()),
        ]);

        assert_eq!(request.file_paths.len(), 1);
        assert!(request.file_paths[0].ends_with("测试 文件.txt"));
    }

    #[test]
    fn rejoins_space_split_windows_style_arguments() {
        let base = std::env::temp_dir().join("ganmaoyuan-launch-split-args-test");
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        let file = base.join("中文 空格").join("长 文件 名.txt");
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, "hello").unwrap();
        let split_args = file
            .to_string_lossy()
            .split(' ')
            .map(OsString::from)
            .collect::<Vec<_>>();

        let request = collect_launch_request(split_args);

        assert_eq!(request.file_paths.len(), 1);
        assert!(request.file_paths[0].ends_with("长 文件 名.txt"));
    }

    #[test]
    fn collects_multiple_split_file_arguments() {
        let base = std::env::temp_dir().join("ganmaoyuan-launch-multi-args-test");
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        let first = base.join("第一个 文件.txt");
        let second = base.join("第二个 文件.pdf");
        fs::write(&first, "first").unwrap();
        fs::write(&second, "second").unwrap();

        let split_args = format!("{} {}", first.to_string_lossy(), second.to_string_lossy())
            .split(' ')
            .map(OsString::from)
            .collect::<Vec<_>>();

        let request = collect_launch_request(split_args);

        assert_eq!(request.file_paths.len(), 2);
        assert!(request.file_paths[0].ends_with("第一个 文件.txt"));
        assert!(request.file_paths[1].ends_with("第二个 文件.pdf"));
    }

    #[test]
    fn parses_shell_source_argument() {
        let request = collect_launch_request(vec![
            OsString::from("--shell-source"),
            OsString::from("windowsSendTo"),
            OsString::from("C:\\not-a-file.txt"),
        ]);

        assert_eq!(request.received_from, "windowsSendTo");
    }

    #[test]
    fn empty_launch_queue_is_not_rewritten_by_idle_polling() {
        let base = std::env::temp_dir().join("ganmaoyuan-launch-empty-queue-test");
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        let queue = base.join("launch-inbox-queue.json");
        let document = LaunchInboxQueueDocument {
            updated_at: "stable".to_string(),
            ..LaunchInboxQueueDocument::default()
        };
        write_json_atomic(&queue, &document).unwrap();
        let before = fs::read(&queue).unwrap();

        assert!(consume_launch_entries_from_path(&queue).unwrap().is_empty());
        assert_eq!(fs::read(&queue).unwrap(), before);

        fs::remove_dir_all(&base).unwrap();
    }
}
