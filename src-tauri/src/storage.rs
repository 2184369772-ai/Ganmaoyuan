use serde::{de::DeserializeOwned, Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    env,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::{Manager, Runtime};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct JsonRecoveryNotice {
    pub file_name: String,
    pub primary_state: String,
    pub backup_age: String,
    pub message: String,
}

#[derive(Default)]
struct JsonRecoveryState {
    pending: Vec<JsonRecoveryNotice>,
    protected_paths: HashSet<String>,
}

static JSON_RECOVERY_STATE: OnceLock<Mutex<JsonRecoveryState>> = OnceLock::new();

fn json_recovery_state() -> &'static Mutex<JsonRecoveryState> {
    JSON_RECOVERY_STATE.get_or_init(|| Mutex::new(JsonRecoveryState::default()))
}

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
    read_json_with_recovery_notice(path).map(|(value, _)| value)
}

pub fn read_json_with_recovery_notice<T: DeserializeOwned>(
    path: &Path,
) -> Result<(T, Option<JsonRecoveryNotice>), String> {
    match read_json_exact_detailed(path) {
        Ok(value) => {
            if let Ok(mut state) = json_recovery_state().lock() {
                state.protected_paths.remove(&recovery_path_key(path));
            }
            Ok((value, None))
        }
        Err(primary_error) => {
            let backup = backup_path(path);
            let file_name = path
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("数据文件");
            let primary_state = primary_error.user_state();
            let metadata = match fs::symlink_metadata(&backup) {
                Ok(metadata) if is_link_or_reparse(&metadata) || !metadata.is_file() => {
                    return Err(format!(
                        "{}；同目录 .bak 不是普通文件，已拒绝回退读取。",
                        primary_error.describe(file_name)
                    ));
                }
                Ok(metadata) => metadata,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    return Err(format!(
                        "{}；没有可用的 .bak 备份。",
                        primary_error.describe(file_name)
                    ));
                }
                Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
                    return Err(format!(
                        "{}；.bak 文件无权限读取。",
                        primary_error.describe(file_name)
                    ));
                }
                Err(error) => {
                    return Err(format!(
                        "{}；检查 .bak 文件失败：{error}。",
                        primary_error.describe(file_name)
                    ));
                }
            };
            let value = read_json_exact_detailed(&backup).map_err(|backup_error| {
                format!(
                    "{}；.bak 也不可用：{}。",
                    primary_error.describe(file_name),
                    backup_error.describe("备份文件")
                )
            })?;
            let backup_age = modified_age_label(metadata.modified().ok());
            let notice = JsonRecoveryNotice {
                file_name: file_name.to_string(),
                primary_state: primary_state.to_string(),
                backup_age: backup_age.clone(),
                message: format!(
                    "主数据文件“{file_name}”{primary_state}，当前读取正在使用同目录 .bak（该备份文件修改于{backup_age}）。此时间之后的更新可能未包含；为保护有效备份，普通保存暂不会覆盖该文件，请核对后通过受控恢复处理。"
                ),
            };
            if let Ok(mut state) = json_recovery_state().lock() {
                if state.protected_paths.insert(recovery_path_key(path)) {
                    state.pending.push(notice.clone());
                }
            }
            Ok((value, Some(notice)))
        }
    }
}

pub fn take_json_recovery_notices() -> Vec<JsonRecoveryNotice> {
    json_recovery_state()
        .lock()
        .map(|mut state| std::mem::take(&mut state.pending))
        .unwrap_or_default()
}

fn recovery_path_key(path: &Path) -> String {
    path_to_string(path).replace('/', "\\").to_ascii_lowercase()
}

fn modified_age_label(modified: Option<SystemTime>) -> String {
    let Some(modified) = modified else {
        return "时间未知".to_string();
    };
    let age = SystemTime::now()
        .duration_since(modified)
        .unwrap_or_default();
    if age.as_secs() < 3600 {
        "不足 1 小时前".to_string()
    } else if age.as_secs() < 86_400 {
        format!("约 {} 小时前", age.as_secs() / 3600)
    } else if age.as_secs() < 31_536_000 {
        format!("约 {} 天前", age.as_secs() / 86_400)
    } else {
        format!("约 {} 年前", age.as_secs() / 31_536_000)
    }
}

#[cfg(windows)]
fn is_link_or_reparse(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    metadata.file_type().is_symlink() || metadata.file_attributes() & 0x400 != 0
}

#[cfg(not(windows))]
fn is_link_or_reparse(metadata: &fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

enum JsonReadFailure {
    Io(std::io::Error),
    Parse(serde_json::Error),
}

impl JsonReadFailure {
    fn user_state(&self) -> &'static str {
        match self {
            Self::Io(error) if error.kind() == std::io::ErrorKind::NotFound => "缺失",
            Self::Io(error) if error.kind() == std::io::ErrorKind::PermissionDenied => "无权限读取",
            Self::Io(_) => "读取失败",
            Self::Parse(_) => "内容损坏",
        }
    }

    fn describe(&self, name: &str) -> String {
        match self {
            Self::Io(error) if error.kind() == std::io::ErrorKind::NotFound => {
                format!("主数据文件“{name}”缺失")
            }
            Self::Io(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
                format!("主数据文件“{name}”无权限读取")
            }
            Self::Io(error) => format!("主数据文件“{name}”读取失败：{error}"),
            Self::Parse(error) => format!("主数据文件“{name}”内容损坏：{error}"),
        }
    }
}

fn read_json_exact_detailed<T: DeserializeOwned>(path: &Path) -> Result<T, JsonReadFailure> {
    let text = fs::read_to_string(path).map_err(JsonReadFailure::Io)?;
    serde_json::from_str(&text).map_err(JsonReadFailure::Parse)
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
    let path_key = recovery_path_key(path);
    if json_recovery_state()
        .lock()
        .map(|state| state.protected_paths.contains(&path_key))
        .unwrap_or(false)
    {
        return Err(format!(
            "拒绝覆盖 {}：本次读取已回退到 .bak，主文件和有效备份均受保护。请先核对数据并通过受控恢复处理。",
            path.file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("数据文件")
        ));
    }
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

    let temporary = parent.join(format!(".gm-{}.tmp", new_id()));
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
    let destination = resolve_global_data_dir(
        configured_data_dir(),
        &legacy_dir,
        &legacy_global_data_dir(),
    );
    fs::create_dir_all(&destination).map_err(|err| {
        format!(
            "创建感冒院数据目录失败：{}：{err}",
            path_to_string(&destination)
        )
    })?;
    migrate_legacy_data(&legacy_dir, &destination)?;
    Ok(destination)
}

pub fn global_data_dir_from_environment() -> Result<PathBuf, String> {
    let portable_default = if let Some(app_data) = env::var_os("APPDATA") {
        PathBuf::from(app_data).join("com.ganmaoyuan.desktop")
    } else if let Some(xdg_data) = env::var_os("XDG_DATA_HOME") {
        PathBuf::from(xdg_data).join("com.ganmaoyuan.desktop")
    } else if let Some(home) = env::var_os("HOME") {
        PathBuf::from(home)
            .join(".local")
            .join("share")
            .join("com.ganmaoyuan.desktop")
    } else {
        return Err("无法解析感冒院的用户数据目录。".to_string());
    };
    let destination = resolve_global_data_dir(
        configured_data_dir(),
        &portable_default,
        &legacy_global_data_dir(),
    );
    fs::create_dir_all(&destination).map_err(|err| {
        format!(
            "创建感冒院数据目录失败：{}：{err}",
            path_to_string(&destination)
        )
    })?;
    Ok(destination)
}

pub fn default_documents_child<R: Runtime>(
    app: &tauri::AppHandle<R>,
    child: &str,
) -> Result<PathBuf, String> {
    let documents = app
        .path()
        .document_dir()
        .map_err(|err| format!("无法获取文档目录：{err}"))?;
    Ok(documents.join("Ganmaoyuan").join(child))
}

fn configured_data_dir() -> Option<PathBuf> {
    env::var_os("GANMAOYUAN_DATA_DIR")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn legacy_global_data_dir() -> PathBuf {
    PathBuf::from(r"D:\GanMaoYuan\AppData")
}

fn resolve_global_data_dir(
    configured: Option<PathBuf>,
    portable_default: &Path,
    legacy_candidate: &Path,
) -> PathBuf {
    if let Some(configured) = configured.filter(|path| !path.as_os_str().is_empty()) {
        return configured;
    }

    if has_legacy_app_data(legacy_candidate) {
        legacy_candidate.to_path_buf()
    } else {
        portable_default.to_path_buf()
    }
}

fn has_legacy_app_data(root: &Path) -> bool {
    if path_contains_reparse_point(root) {
        return false;
    }
    [
        "projects.json",
        "memos.json",
        "material-inbox.json",
        "workspace-config.json",
        "deepseek-settings.json",
        "shell-integrations.json",
    ]
    .iter()
    .any(|name| {
        let path = root.join(name);
        let Ok(metadata) = fs::symlink_metadata(path) else {
            return false;
        };
        metadata.file_type().is_file() && !is_reparse_point(&metadata)
    })
}

fn path_contains_reparse_point(path: &Path) -> bool {
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component.as_os_str());
        if let Ok(metadata) = fs::symlink_metadata(&current) {
            if metadata.file_type().is_symlink() || is_reparse_point(&metadata) {
                return true;
            }
        }
    }
    false
}

#[cfg(windows)]
fn is_reparse_point(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    metadata.file_attributes() & 0x400 != 0
}

#[cfg(not(windows))]
fn is_reparse_point(_: &fs::Metadata) -> bool {
    false
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
        .unwrap_or_else(|| env::temp_dir().join("ganmaoyuan-tests"));
    base.join(format!("{label}-{}", new_id()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_install_uses_portable_user_data_path_even_when_legacy_dir_exists_empty() {
        let root = test_root("portable-data");
        let portable = root.join("user-data");
        let legacy = root.join("legacy");
        fs::create_dir_all(&legacy).unwrap();

        assert_eq!(
            resolve_global_data_dir(None, &portable, &legacy),
            portable,
            "an empty legacy location must not be selected just because its drive exists"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn explicit_data_dir_takes_precedence_over_legacy_compatibility() {
        let root = test_root("configured-data");
        let configured = root.join("configured");
        let portable = root.join("user-data");

        assert_eq!(
            resolve_global_data_dir(Some(configured.clone()), &portable, &root.join("legacy")),
            configured
        );
        fs::remove_dir_all(root).unwrap_or_default();
    }

    #[test]
    fn legacy_data_location_is_used_only_when_recognized_data_exists() {
        let root = test_root("legacy-data");
        let legacy = root.join("legacy");
        let portable = root.join("user-data");
        fs::create_dir_all(&legacy).unwrap();
        fs::write(legacy.join("projects.json"), br#"{"projects":[]}"#).unwrap();

        assert!(has_legacy_app_data(&legacy));
        assert_eq!(resolve_global_data_dir(None, &portable, &legacy), legacy);
        fs::remove_dir_all(root).unwrap();
    }

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

        let (recovered, notice): (Vec<String>, Option<JsonRecoveryNotice>) =
            read_json_with_recovery_notice(&path).unwrap();

        assert_eq!(recovered, vec!["first"]);
        let notice = notice.expect("a backup fallback must be visible");
        assert!(notice.message.contains("内容损坏"));
        assert!(notice.message.contains(".bak"));
        let backup_before = hash_file(&backup_path(&path)).unwrap();
        assert!(write_json_atomic(&path, &vec!["new".to_string()]).is_err());
        assert_eq!(hash_file(&backup_path(&path)).unwrap(), backup_before);

        write_text_atomic(&path, r#"["recovered-and-reviewed"]"#).unwrap();
        assert_eq!(
            read_json::<Vec<String>>(&path).unwrap(),
            vec!["recovered-and-reviewed"]
        );
        write_json_atomic(&path, &vec!["next".to_string()]).unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn missing_primary_can_fall_back_and_is_reported_as_missing() {
        let root = test_root("missing-primary-recovery");
        fs::create_dir_all(&root).unwrap();
        let path = root.join("state.json");
        fs::write(backup_path(&path), br#"["previous"]"#).unwrap();

        let (recovered, notice): (Vec<String>, Option<JsonRecoveryNotice>) =
            read_json_with_recovery_notice(&path).unwrap();

        assert_eq!(recovered, vec!["previous"]);
        assert_eq!(notice.unwrap().primary_state, "缺失");
        assert!(
            !path.exists(),
            "fallback reads must not recreate the primary file"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn inaccessible_primary_can_fall_back_and_is_reported_as_permission_failure() {
        let root = test_root("permission-primary-recovery");
        let path = root.join("state.json");
        fs::create_dir_all(&path).unwrap();
        fs::write(backup_path(&path), br#"["previous"]"#).unwrap();

        let (recovered, notice): (Vec<String>, Option<JsonRecoveryNotice>) =
            read_json_with_recovery_notice(&path).unwrap();

        assert_eq!(recovered, vec!["previous"]);
        assert_eq!(notice.unwrap().primary_state, "无权限读取");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn json_read_failure_classifies_permission_and_corruption_separately() {
        let denied =
            JsonReadFailure::Io(std::io::Error::from(std::io::ErrorKind::PermissionDenied));
        let corrupted =
            JsonReadFailure::Parse(serde_json::from_str::<serde_json::Value>("{").unwrap_err());

        assert_eq!(denied.user_state(), "无权限读取");
        assert_eq!(corrupted.user_state(), "内容损坏");
    }

    #[test]
    fn atomic_json_replace_works_near_legacy_windows_path_limit() {
        let root = test_root("long-atomic-path");
        let file_name = "system-capability-overview.md";
        let mut parent = root.clone();
        let mut index = 0;

        loop {
            let target = parent.join(file_name);
            let old_temporary = parent.join(format!(".{file_name}.{}.tmp", new_id()));
            if path_to_string(&target).len() >= 220 && path_to_string(&old_temporary).len() >= 260 {
                assert!(path_to_string(&target).len() < 260);
                break;
            }
            parent = parent.join(format!("nested-{index:02}-abcdefghij"));
            index += 1;
        }

        fs::create_dir_all(&parent).unwrap();
        let path = parent.join(file_name);
        write_json_atomic(&path, &vec!["first".to_string()]).unwrap();
        write_json_atomic(&path, &vec!["second".to_string()]).unwrap();

        assert_eq!(
            read_json::<Vec<String>>(&path).unwrap(),
            vec!["second".to_string()]
        );
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
