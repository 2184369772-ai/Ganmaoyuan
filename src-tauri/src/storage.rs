use serde::{de::DeserializeOwned, Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    env,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::{Manager, Runtime};
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MigrationRecord {
    source_dir: String,
    destination_dir: String,
    copied_files: Vec<String>,
    verified_at: String,
    source_retained: bool,
}

pub fn now_string() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .to_string()
}

pub fn new_id() -> String {
    Uuid::new_v4().to_string()
}

pub fn path_to_string(path: &Path) -> String {
    normalize_windows_display_path(&path.to_string_lossy())
}

fn normalize_windows_display_path(value: &str) -> String {
    if let Some(rest) = value.strip_prefix(r"\\?\UNC\") {
        return format!(r"\\{rest}");
    }
    value.strip_prefix(r"\\?\").unwrap_or(value).to_string()
}

pub fn canonical_existing_file(path: &Path) -> Result<PathBuf, String> {
    if !path.exists() {
        return Err(format!("文件不存在：{}", path_to_string(path)));
    }
    if !path.is_file() {
        return Err(format!("路径不是文件：{}", path_to_string(path)));
    }
    path.canonicalize()
        .map_err(|err| format!("规范化文件路径失败：{}：{err}", path_to_string(path)))
}

pub fn canonical_project_root(path: &Path) -> Result<PathBuf, String> {
    fs::create_dir_all(path)
        .map_err(|err| format!("创建项目根目录失败：{}：{err}", path_to_string(path)))?;
    path.canonicalize()
        .map_err(|err| format!("规范化项目根目录失败：{}：{err}", path_to_string(path)))
}

pub fn hash_file(path: &Path) -> Result<String, String> {
    let mut file = File::open(path)
        .map_err(|err| format!("读取文件哈希失败：{}：{err}", path_to_string(path)))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|err| format!("读取文件哈希失败：{}：{err}", path_to_string(path)))?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

pub fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T, String> {
    match read_json_exact(path) {
        Ok(value) => Ok(value),
        Err(primary_error) => {
            let backup = backup_path(path);
            if !backup.exists() {
                return Err(primary_error);
            }
            read_json_exact(&backup).map_err(|backup_error| {
                format!(
                    "{primary_error}；备份也无法读取：{}",
                    backup_error.trim_start_matches("读取 JSON 失败：")
                )
            })
        }
    }
}

fn read_json_exact<T: DeserializeOwned>(path: &Path) -> Result<T, String> {
    let text = fs::read_to_string(path)
        .map_err(|err| format!("读取 JSON 失败：{}：{err}", path_to_string(path)))?;
    serde_json::from_str(&text)
        .map_err(|err| format!("解析 JSON 失败：{}：{err}", path_to_string(path)))
}

pub fn write_json_atomic<T: Serialize + DeserializeOwned>(
    path: &Path,
    value: &T,
) -> Result<(), String> {
    let bytes =
        serde_json::to_vec_pretty(value).map_err(|err| format!("序列化 JSON 失败：{err}"))?;
    serde_json::from_slice::<T>(&bytes).map_err(|err| format!("JSON 写入前校验失败：{err}"))?;
    write_bytes_atomic(path, &bytes)?;
    read_json_exact::<T>(path)
        .map(|_| ())
        .map_err(|err| format!("JSON 写入后校验失败：{err}"))
}

pub fn append_json_line<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("创建事件目录失败：{}：{err}", path_to_string(parent)))?;
    }
    let mut line = serde_json::to_vec(value).map_err(|err| format!("序列化事件失败：{err}"))?;
    line.push(b'\n');
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|err| format!("打开事件记录失败：{}：{err}", path_to_string(path)))?;
    file.write_all(&line)
        .and_then(|_| file.sync_data())
        .map_err(|err| format!("写入事件记录失败：{}：{err}", path_to_string(path)))
}

pub fn write_text_atomic(path: &Path, text: &str) -> Result<(), String> {
    write_bytes_atomic(path, text.as_bytes())
}

fn write_bytes_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("无法确定文件目录：{}", path_to_string(path)))?;
    fs::create_dir_all(parent)
        .map_err(|err| format!("创建数据目录失败：{}：{err}", path_to_string(parent)))?;

    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("data.json");
    let temporary = parent.join(format!(".{file_name}.{}.tmp", new_id()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|err| {
            format!(
                "创建临时数据文件失败：{}：{err}",
                path_to_string(&temporary)
            )
        })?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|err| {
            let _ = fs::remove_file(&temporary);
            format!("写入临时数据文件失败：{}：{err}", path_to_string(path))
        })?;
    drop(file);

    let replace_result = if path.exists() {
        replace_existing_file(path, &temporary, &backup_path(path))
    } else {
        fs::rename(&temporary, path)
            .map_err(|err| format!("提交数据文件失败：{}：{err}", path_to_string(path)))
    };
    if replace_result.is_err() && temporary.exists() {
        let _ = fs::remove_file(&temporary);
    }
    replace_result
}

fn backup_path(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("data.json");
    path.with_file_name(format!("{name}.bak"))
}

#[cfg(target_os = "windows")]
fn replace_existing_file(path: &Path, temporary: &Path, backup: &Path) -> Result<(), String> {
    use windows::{
        core::HSTRING,
        Win32::Storage::FileSystem::{ReplaceFileW, REPLACEFILE_WRITE_THROUGH},
    };

    if backup.exists() {
        fs::remove_file(backup)
            .map_err(|err| format!("更新数据备份失败：{}：{err}", path_to_string(backup)))?;
    }
    unsafe {
        ReplaceFileW(
            &HSTRING::from(path_to_string(path)),
            &HSTRING::from(path_to_string(temporary)),
            &HSTRING::from(path_to_string(backup)),
            REPLACEFILE_WRITE_THROUGH,
            None,
            None,
        )
    }
    .map_err(|err| format!("原子替换数据文件失败：{}：{err}", path_to_string(path)))
}

#[cfg(not(target_os = "windows"))]
fn replace_existing_file(path: &Path, temporary: &Path, backup: &Path) -> Result<(), String> {
    fs::copy(path, backup)
        .map_err(|err| format!("更新数据备份失败：{}：{err}", path_to_string(backup)))?;
    fs::rename(temporary, path)
        .map_err(|err| format!("提交数据文件失败：{}：{err}", path_to_string(path)))
}

pub fn global_data_dir<R: Runtime>(app: &tauri::AppHandle<R>) -> Result<PathBuf, String> {
    let legacy_dir = app
        .path()
        .app_data_dir()
        .map_err(|err| format!("无法获取应用数据目录：{err}"))?;
    let destination = env::var_os("GANMAOYUAN_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let preferred = PathBuf::from(r"D:\GanMaoYuan\AppData");
            // Existing installs keep their historical data location. Fresh installs use
            // Tauri's per-user directory instead of assuming every machine has a D: drive.
            if preferred.exists() {
                preferred
            } else {
                legacy_dir.clone()
            }
        });
    fs::create_dir_all(&destination).map_err(|err| {
        format!(
            "创建感冒院数据目录失败：{}：{err}",
            path_to_string(&destination)
        )
    })?;
    migrate_legacy_data(&legacy_dir, &destination)?;
    Ok(destination)
}

fn migrate_legacy_data(source: &Path, destination: &Path) -> Result<(), String> {
    if same_path(source, destination) {
        return Ok(());
    }

    let mut copied = Vec::new();
    for name in ["projects.json", "memos.json"] {
        let source_file = source.join(name);
        let destination_file = destination.join(name);
        if !source_file.exists() || destination_file.exists() {
            continue;
        }
        let bytes = fs::read(&source_file)
            .map_err(|err| format!("读取旧数据失败：{}：{err}", path_to_string(&source_file)))?;
        write_bytes_atomic(&destination_file, &bytes)?;
        let source_hash = hash_file(&source_file)?;
        let destination_hash = hash_file(&destination_file)?;
        if source_hash != destination_hash {
            return Err(format!("迁移数据校验失败：{name}"));
        }
        copied.push(name.to_string());
    }

    if copied.is_empty() {
        return Ok(());
    }
    let record = MigrationRecord {
        source_dir: path_to_string(source),
        destination_dir: path_to_string(destination),
        copied_files: copied,
        verified_at: now_string(),
        source_retained: true,
    };
    write_json_atomic(&destination.join("migration-from-legacy.json"), &record)
}

fn same_path(left: &Path, right: &Path) -> bool {
    path_to_string(left)
        .trim_end_matches(['\\', '/'])
        .eq_ignore_ascii_case(path_to_string(right).trim_end_matches(['\\', '/']))
}

#[cfg(test)]
pub fn test_root(label: &str) -> PathBuf {
    let base = env::var_os("GANMAOYUAN_TEST_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/test-data"));
    base.join(format!("{label}-{}", new_id()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_migration_copies_verifies_and_retains_source() {
        let root = test_root("migration");
        let source = root.join("legacy");
        let destination = root.join("current");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("projects.json"), br#"{"projects":[]}"#).unwrap();
        fs::write(source.join("memos.json"), b"[]").unwrap();

        migrate_legacy_data(&source, &destination).unwrap();

        assert_eq!(
            hash_file(&source.join("projects.json")).unwrap(),
            hash_file(&destination.join("projects.json")).unwrap()
        );
        assert_eq!(
            hash_file(&source.join("memos.json")).unwrap(),
            hash_file(&destination.join("memos.json")).unwrap()
        );
        assert!(source.join("projects.json").exists());
        let record: MigrationRecord =
            read_json(&destination.join("migration-from-legacy.json")).unwrap();
        assert!(record.source_retained);
        assert_eq!(record.copied_files.len(), 2);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn read_json_uses_valid_backup_after_current_file_corruption() {
        let root = test_root("recovery");
        fs::create_dir_all(&root).unwrap();
        let path = root.join("state.json");
        write_json_atomic(&path, &vec!["first".to_string()]).unwrap();
        write_json_atomic(&path, &vec!["second".to_string()]).unwrap();
        fs::write(&path, b"{broken").unwrap();

        let recovered: Vec<String> = read_json(&path).unwrap();

        assert_eq!(recovered, vec!["first"]);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn extended_windows_paths_are_stored_as_regular_absolute_paths() {
        assert_eq!(
            normalize_windows_display_path(r"\\?\D:\项目\资料.txt"),
            r"D:\项目\资料.txt"
        );
        assert_eq!(
            normalize_windows_display_path(r"\\?\UNC\server\share\资料.txt"),
            r"\\server\share\资料.txt"
        );
        assert_eq!(
            normalize_windows_display_path(r"D:\项目\资料.txt"),
            r"D:\项目\资料.txt"
        );
    }
}
