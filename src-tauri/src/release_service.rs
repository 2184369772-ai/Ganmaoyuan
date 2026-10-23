use crate::{
    analysis::{analyze_managed_file, extracted_text_path},
    models::{
        AppRegistry, AtlasAssessment, BackupRestoreResult, DeepSeekSettings, FileRecord,
        GlobalFilesDocument, InboxRoutingSettings, LocalBackupResult, MaterialInboxDocument,
        MemoItem, PrivacyArtifactsResult, ProjectAnalysis, ProjectManifest, ProjectMigrationResult,
        SafeExportResult, WorkspaceMessage,
    },
    project_service,
    storage::{
        canonical_project_root, default_documents_child, global_data_dir, hash_file, new_id,
        now_string, path_to_string, read_json, read_json_with_recovery_notice, write_json_atomic,
        write_text_atomic,
    },
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    collections::{BTreeMap, HashSet},
    fs,
    path::{Path, PathBuf},
};
use tauri::Runtime;

const BACKUP_SCHEMA_VERSION: u32 = 3;
const BACKUP_MANIFEST_FILE: &str = "backup-bundle.json";
const BACKUP_FORMAT: &str = "ganmaoyuan-local-backup";
const MIGRATION_RECORD_FILE: &str = ".ganmaoyuan/root-migration.json";
const DEEPSEEK_SETTINGS_FILE: &str = "deepseek-settings.json";
const BACKUP_GLOBAL_FILES: [&str; 5] = [
    "projects.json",
    "memos.json",
    "material-inbox.json",
    "inbox-routing-settings.json",
    DEEPSEEK_SETTINGS_FILE,
];
const MAX_BACKUP_ENTRIES: usize = 100_000;

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
struct BackupBundleManifest {
    #[serde(default)]
    bundle_format: String,
    #[serde(default)]
    bundle_id: String,
    schema_version: u32,
    app_version: String,
    created_at: String,
    #[serde(default)]
    global_files: Vec<BackupFileEntry>,
    #[serde(default)]
    projects: Vec<ProjectBackupEntry>,
    #[serde(default)]
    excluded_entries: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
struct BackupFileEntry {
    relative_path: String,
    checksum: String,
    size_bytes: u64,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
struct ProjectBackupEntry {
    project_id: String,
    project_name: String,
    original_root_dir: String,
    backup_relative_dir: String,
    file_count: usize,
    #[serde(default)]
    files: Vec<BackupFileEntry>,
    #[serde(default)]
    excluded_entries: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
struct ProjectMigrationRecord {
    old_root_dir: String,
    new_root_dir: String,
    migrated_at: String,
    retained_old_root: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
struct SafeExportConversation {
    id: String,
    author: String,
    kind: String,
    created_at: String,
    status: String,
    #[serde(default)]
    related_task: String,
    #[serde(default)]
    source: String,
    #[serde(default)]
    model_id: String,
    text_preview: String,
    text_redacted: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
struct SafeExportFileIndexEntry {
    file_name: String,
    managed_relative_path: String,
    file_type: String,
    category: String,
    recommended_category: String,
    current_version: String,
    created_source: String,
    related_task: String,
    still_exists: bool,
    needs_user_confirmation: bool,
    parse_status: String,
    content_summary: String,
    #[serde(default)]
    main_fields_or_sections: Vec<String>,
    #[serde(default)]
    analysis_warnings: Vec<String>,
}

pub fn create_local_backup<R: Runtime>(
    app: &tauri::AppHandle<R>,
    project_roots: Vec<String>,
    destination_dir: String,
) -> Result<LocalBackupResult, String> {
    let global_dir = global_data_dir(app)?;
    let registry_path = project_service::registry_path(app)?;
    let primary_metadata = fs::symlink_metadata(&registry_path);
    let has_primary = match primary_metadata {
        Ok(metadata) if metadata.is_file() && !is_link_or_reparse(&metadata) => true,
        Ok(_) => return Err("项目注册表不是普通文件，已停止创建备份。".to_string()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(error) => return Err(format!("检查项目注册表失败：{error}")),
    };
    let registry_backup = backup_path_for(&registry_path);
    let has_backup = match fs::symlink_metadata(&registry_backup) {
        Ok(_) => true,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(error) => return Err(format!("检查项目注册表 .bak 失败：{error}")),
    };
    let registry = if has_primary || has_backup {
        let (registry, recovery_notice): (AppRegistry, Option<crate::storage::JsonRecoveryNotice>) =
            read_json_with_recovery_notice(&registry_path)?;
        if let Some(notice) = recovery_notice {
            return Err(format!(
                "项目注册表当前只能从旧 .bak 读取，未创建可能不完整的全量备份：{}",
                notice.message
            ));
        }
        registry
    } else {
        AppRegistry {
            schema_version: crate::models::MANIFEST_SCHEMA_VERSION,
            ..AppRegistry::default()
        }
    };
    let roots = resolve_project_roots(&registry, project_roots)?;
    let destination_parent = if destination_dir.trim().is_empty() {
        default_documents_child(app, "Backups")?
    } else {
        PathBuf::from(&destination_dir)
    };
    create_local_backup_from_paths(
        &global_dir,
        roots,
        &destination_parent,
        env!("CARGO_PKG_VERSION"),
    )
}

pub fn restore_local_backup<R: Runtime>(
    app: &tauri::AppHandle<R>,
    backup_dir: String,
    restore_projects_base_dir: String,
    confirmed: bool,
) -> Result<BackupRestoreResult, String> {
    if !confirmed {
        return Err("恢复操作需要先由用户确认数据覆盖与新建项目目录。".to_string());
    }
    let global_dir = global_data_dir(app)?;
    let backup_root = PathBuf::from(&backup_dir);
    let restore_base = if restore_projects_base_dir.trim().is_empty() {
        default_documents_child(app, "RestoredProjects")?
    } else {
        PathBuf::from(&restore_projects_base_dir)
    };
    restore_local_backup_from_paths(
        &global_dir,
        &backup_root,
        &restore_base,
        &project_service::registry_path(app)?,
        confirmed,
    )
}

pub fn migrate_project_root<R: Runtime>(
    app: &tauri::AppHandle<R>,
    project_root: String,
    new_root_dir: String,
) -> Result<ProjectMigrationResult, String> {
    let root = PathBuf::from(&project_root);
    let new_root = canonical_project_root(Path::new(&new_root_dir))?;
    if same_path(&root, &new_root) {
        return Err("新根目录与当前项目根目录相同，无需迁移。".to_string());
    }
    if project_service::manifest_path(&new_root).exists() {
        return Err(format!(
            "目标目录已经存在感冒院项目：{}",
            path_to_string(&new_root)
        ));
    }
    let _guard = project_service::project_write_lock()
        .lock()
        .map_err(|_| "项目存储锁已损坏，请重试。".to_string())?;
    let manifest = project_service::read_and_repair_manifest(&root)?;
    copy_dir_recursive(&root, &new_root)?;
    let mut migrated = project_service::read_and_repair_manifest(&new_root)?;
    rewrite_manifest_root_paths(&mut migrated, &new_root);
    project_service::persist_project(&new_root, &migrated, None)?;
    write_json_atomic(
        &new_root.join(MIGRATION_RECORD_FILE),
        &ProjectMigrationRecord {
            old_root_dir: path_to_string(&root),
            new_root_dir: path_to_string(&new_root),
            migrated_at: now_string(),
            retained_old_root: true,
        },
    )?;

    let mut registry = project_service::read_registry(app)?;
    registry
        .projects
        .retain(|item| item.id != manifest.project.id);
    registry.projects.insert(0, migrated.project.clone());
    project_service::write_registry(app, &registry)?;

    Ok(ProjectMigrationResult {
        project: migrated.project,
        old_root_dir: path_to_string(&root),
        new_root_dir: path_to_string(&new_root),
        retained_old_root: true,
        copied_entries: count_files_recursive(&new_root)?,
        migrated_at: now_string(),
    })
}

pub fn export_project_safe<R: Runtime>(
    app: &tauri::AppHandle<R>,
    project_root: String,
    destination_dir: String,
) -> Result<SafeExportResult, String> {
    let default_parent = default_documents_child(app, "Exports")?;
    export_project_safe_to(&project_root, &destination_dir, &default_parent)
}

fn export_project_safe_to(
    project_root: &str,
    destination_dir: &str,
    default_parent: &Path,
) -> Result<SafeExportResult, String> {
    let root = PathBuf::from(&project_root);
    let manifest = project_service::read_and_repair_manifest(&root)?;
    let destination_parent = if destination_dir.trim().is_empty() {
        canonical_project_root(default_parent)?
    } else {
        canonical_project_root(Path::new(&destination_dir))?
    };
    let export_dir = unique_child_dir(
        &destination_parent,
        &format!("safe-export-{}", sanitize_slug(&manifest.project.name)),
    )?;
    fs::create_dir_all(&export_dir)
        .map_err(|err| format!("创建导出目录失败：{}；{err}", path_to_string(&export_dir)))?;

    let material_root = export_dir.join("materials");
    let mut included_files = Vec::new();
    let mut seen_managed = HashSet::new();
    for file in &manifest.files {
        if file.managed_path.trim().is_empty() || !seen_managed.insert(file.managed_path.clone()) {
            continue;
        }
        let source = PathBuf::from(&file.managed_path);
        if !source.exists() {
            continue;
        }
        let relative = PathBuf::from(if file.managed_relative_path.trim().is_empty() {
            file.file_name.clone()
        } else {
            file.managed_relative_path.clone()
        });
        let destination = material_root.join(relative);
        copy_file_verified(&source, &destination)?;
        included_files.push(path_to_string(
            destination
                .strip_prefix(&export_dir)
                .unwrap_or(&destination),
        ));
    }

    let file_index = manifest
        .files
        .iter()
        .map(build_safe_export_file_index)
        .collect::<Vec<_>>();
    let conversations = manifest
        .messages
        .iter()
        .map(build_safe_export_conversation)
        .collect::<Vec<_>>();
    let redacted_message_count = conversations
        .iter()
        .filter(|item| item.text_redacted)
        .count();

    write_json_atomic(&export_dir.join("project-summary.json"), &manifest.project)?;
    write_json_atomic(
        &export_dir.join("project-analysis.json"),
        &manifest.project_analysis,
    )?;
    write_json_atomic(&export_dir.join("atlas-assessment.json"), &manifest.atlas)?;
    write_json_atomic(&export_dir.join("file-index.json"), &file_index)?;
    write_json_atomic(&export_dir.join("conversations.json"), &conversations)?;
    write_json_atomic(&export_dir.join("tasks.json"), &manifest.tasks)?;
    write_json_atomic(&export_dir.join("decisions.json"), &manifest.decisions)?;
    write_json_atomic(
        &export_dir.join("location-decisions.json"),
        &manifest.location_decisions,
    )?;
    write_json_atomic(
        &export_dir.join("pending-reviews.json"),
        &manifest.pending_reviews,
    )?;
    write_json_atomic(
        &export_dir.join("export-manifest.json"),
        &json!({
            "schemaVersion": BACKUP_SCHEMA_VERSION,
            "exportedAt": now_string(),
            "projectName": manifest.project.name,
            "includedFiles": included_files,
            "redactedMessageCount": redacted_message_count,
        }),
    )?;

    Ok(SafeExportResult {
        export_dir: path_to_string(&export_dir),
        included_files: included_files.clone(),
        exported_file_count: included_files.len(),
        redacted_message_count,
        exported_at: now_string(),
    })
}

pub fn generate_privacy_artifacts<R: Runtime>(
    app: &tauri::AppHandle<R>,
    destination_dir: String,
    project_root: Option<String>,
) -> Result<PrivacyArtifactsResult, String> {
    let global_dir = global_data_dir(app)?;
    let destination_parent = if destination_dir.trim().is_empty() {
        canonical_project_root(&default_documents_child(app, "Diagnostics")?)?
    } else {
        canonical_project_root(Path::new(&destination_dir))?
    };
    let artifact_dir = unique_child_dir(&destination_parent, "release-artifacts")?;
    fs::create_dir_all(&artifact_dir).map_err(|err| {
        format!(
            "创建隐私与诊断目录失败：{}；{err}",
            path_to_string(&artifact_dir)
        )
    })?;

    let privacy_notice_path = artifact_dir.join("PRIVACY.md");
    let third_party_notices_path = artifact_dir.join("THIRD_PARTY_NOTICES.md");
    let diagnostic_log_path = artifact_dir.join("diagnostic-log.json");

    write_text_atomic(&privacy_notice_path, PRIVACY_NOTICE)?;
    write_text_atomic(&third_party_notices_path, THIRD_PARTY_NOTICES)?;

    let registry = project_service::read_registry(app).unwrap_or(AppRegistry {
        schema_version: crate::models::MANIFEST_SCHEMA_VERSION,
        projects: Vec::new(),
        continue_preferences: Vec::new(),
    });
    let deepseek_settings: DeepSeekSettings = if global_dir.join(DEEPSEEK_SETTINGS_FILE).exists() {
        read_json(&global_dir.join(DEEPSEEK_SETTINGS_FILE)).unwrap_or_default()
    } else {
        DeepSeekSettings::default()
    };
    let project_diagnostics = project_root
        .filter(|value| !value.trim().is_empty())
        .map(|value| PathBuf::from(value))
        .and_then(|root| project_service::read_and_repair_manifest(&root).ok())
        .map(|manifest| build_project_diagnostic(&manifest));

    write_json_atomic(
        &diagnostic_log_path,
        &json!({
            "generatedAt": now_string(),
            "appVersion": env!("CARGO_PKG_VERSION"),
            "globalDataDir": path_to_string(&global_dir),
            "deepSeekSettings": {
                "hasApiKey": deepseek_settings.has_api_key,
                "selectedModelId": deepseek_settings.selected_model_id,
                "lastTestedAt": deepseek_settings.last_tested_at,
            },
            "projects": registry.projects.iter().map(|item| json!({
                "id": item.id,
                "name": item.name,
                "rootDir": item.root_dir,
                "lastOpenedAt": item.last_opened_at,
                "nextStep": item.next_step,
            })).collect::<Vec<_>>(),
            "projectDiagnostic": project_diagnostics,
            "sensitiveFieldsExcluded": [
                "apiKey",
                "credentialManagerSecret",
                "rawExtractedText",
                "messageFullText",
                "originalSourceContent"
            ],
        }),
    )?;

    Ok(PrivacyArtifactsResult {
        privacy_notice_path: path_to_string(&privacy_notice_path),
        third_party_notices_path: path_to_string(&third_party_notices_path),
        diagnostic_log_path: path_to_string(&diagnostic_log_path),
        generated_at: now_string(),
    })
}

fn resolve_project_roots(
    registry: &AppRegistry,
    requested: Vec<String>,
) -> Result<Vec<String>, String> {
    let selected_values = if requested.is_empty() {
        registry
            .projects
            .iter()
            .map(|item| item.root_dir.clone())
            .collect::<Vec<_>>()
    } else {
        requested
    };
    let mut unique = HashSet::new();
    let mut selected = Vec::new();
    for value in selected_values {
        let root = canonical_existing_directory(Path::new(&value), "所选项目目录")?;
        let is_registered = registry.projects.iter().any(|item| {
            canonical_existing_directory(Path::new(&item.root_dir), "已登记项目目录")
                .is_ok_and(|registered| same_path(&registered, &root))
        });
        if !is_registered {
            return Err("备份只能包含当前应用注册表中的项目目录。".to_string());
        }
        if !unique.insert(path_to_string(&root).to_ascii_lowercase()) {
            return Err("备份项目列表包含重复目录。".to_string());
        }
        selected.push(root);
    }
    Ok(selected.iter().map(|path| path_to_string(path)).collect())
}

fn create_local_backup_from_paths(
    global_dir: &Path,
    project_roots: Vec<String>,
    destination_parent: &Path,
    app_version: &str,
) -> Result<LocalBackupResult, String> {
    let global_dir = canonical_existing_directory(global_dir, "应用数据目录")?;
    let mut canonical_roots = Vec::new();
    let mut seen_roots = HashSet::new();
    for root in project_roots {
        let canonical = canonical_existing_directory(Path::new(&root), "已登记项目目录")?;
        if paths_overlap(&global_dir, &canonical) {
            return Err("备份项目目录不能与感冒院应用数据目录重叠。".to_string());
        }
        if !seen_roots.insert(path_to_string(&canonical).to_ascii_lowercase()) {
            return Err("备份项目列表包含重复目录。".to_string());
        }
        canonical_roots.push(canonical);
    }

    // Reject a backup destination nested inside a source project before creating it.
    // Otherwise the newly-created bundle could be copied into itself.
    let destination_candidate = absolute_normalized_path(destination_parent)?;
    ensure_no_reparse_components(&destination_candidate)?;
    if canonical_roots
        .iter()
        .any(|root| path_is_within(&destination_candidate, root))
    {
        return Err("备份输出目录不能位于任何备份项目目录内部。".to_string());
    }

    let backup_dir = unique_child_dir(&destination_candidate, "ganmaoyuan-backup")?;
    let destination_parent = backup_dir
        .parent()
        .ok_or_else(|| "备份输出目录没有有效父目录。".to_string())?
        .canonicalize()
        .map_err(|err| format!("规范化备份输出目录失败：{err}"))?;
    if canonical_roots
        .iter()
        .any(|root| path_is_within(&destination_parent, root))
    {
        let _ = fs::remove_dir(&backup_dir);
        return Err("备份输出目录解析后落入项目目录，已拒绝写入。".to_string());
    }

    let result = (|| -> Result<LocalBackupResult, String> {
        create_local_backup_bundle(&global_dir, &canonical_roots, &backup_dir, app_version)
    })();
    match result {
        Ok(backup) => Ok(backup),
        Err(error) => {
            let cleanup = fs::symlink_metadata(&backup_dir)
                .ok()
                .filter(|metadata| metadata.is_dir() && !is_link_or_reparse(metadata))
                .map(|_| fs::remove_dir_all(&backup_dir));
            if let Some(Err(cleanup_error)) = cleanup {
                return Err(format!("{error}；清理不完整备份目录失败：{cleanup_error}"));
            }
            Err(error)
        }
    }
}

fn create_local_backup_bundle(
    global_dir: &Path,
    project_roots: &[PathBuf],
    backup_dir: &Path,
    app_version: &str,
) -> Result<LocalBackupResult, String> {
    let mut manifest = BackupBundleManifest {
        bundle_format: BACKUP_FORMAT.to_string(),
        bundle_id: crate::storage::new_id(),
        schema_version: BACKUP_SCHEMA_VERSION,
        app_version: app_version.to_string(),
        created_at: now_string(),
        global_files: Vec::new(),
        projects: Vec::new(),
        excluded_entries: vec![
            "每个项目仅备份 .ganmaoyuan 应用数据目录及项目根目录内逐条登记的受管文件；不扫描或复制项目根目录中的未登记内容，包括源码仓库、.git、node_modules、构建产物和临时依赖。".to_string(),
            "global/open-folder-debug.jsonl".to_string(),
            "global/search-index.json (可从登记资料重建)".to_string(),
            "global/managed/workspaceCleanup/** (指向应用数据目录以外的用户工作区，不复制)"
                .to_string(),
            "global/managed/未登记文件 (不作为受管资料备份)".to_string(),
            "project/.ganmaoyuan/analysis/content/**".to_string(),
            "project/.ganmaoyuan/staging/**".to_string(),
        ],
    };
    let mut reported_exclusions = manifest.excluded_entries.clone();

    let global_backup_root = backup_dir.join("global");
    fs::create_dir_all(&global_backup_root).map_err(|err| {
        format!(
            "创建全局备份目录失败：{}；{err}",
            path_to_string(&global_backup_root)
        )
    })?;
    for name in BACKUP_GLOBAL_FILES {
        let source = global_dir.join(name);
        match fs::symlink_metadata(&source) {
            Ok(metadata) if metadata.is_file() && !is_link_or_reparse(&metadata) => {}
            Ok(_) => return Err(format!("全局备份资料不是普通文件：{name}。")),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(format!("检查全局备份数据失败：{name}；{error}")),
        }
        let destination = global_backup_root.join(name);
        copy_file_verified(&source, &destination)?;
        validate_global_json(name, &destination)?;
        manifest
            .global_files
            .push(build_backup_file_entry(&backup_dir, &destination)?);
    }
    copy_global_managed_for_backup(global_dir, &global_backup_root)?;
    let mut global_paths = Vec::new();
    collect_regular_files(&global_backup_root, &mut global_paths)?;
    if global_paths.len() > MAX_BACKUP_ENTRIES {
        return Err("全局受管资料文件数量超过备份安全上限。".to_string());
    }
    manifest.global_files = global_paths
        .into_iter()
        .map(|path| build_backup_file_entry(backup_dir, &path))
        .collect::<Result<Vec<_>, _>>()?;

    let mut total_entries = manifest.global_files.len();
    for root in project_roots {
        let project = project_service::read_and_repair_manifest(&root)?;
        if project.recovered_from_backup {
            return Err(format!(
                "项目“{}”当前只能从 .bak 回退读取；为避免把旧状态当作完整备份，已停止。",
                project.project.name
            ));
        }
        let project_backup_relative = PathBuf::from("projects").join(format!(
            "{}-{}",
            sanitize_slug(&project.project.name),
            project.project.id
        ));
        let project_backup_root = backup_dir.join(&project_backup_relative);
        let excluded = copy_project_for_backup(&root, &project_backup_root, &project)?;
        let project_files = collect_backup_file_entries(&backup_dir, &project_backup_root)?;
        total_entries = total_entries.saturating_add(project_files.len());
        if total_entries > MAX_BACKUP_ENTRIES {
            return Err("全量备份文件总数超过安全上限。".to_string());
        }
        manifest.projects.push(ProjectBackupEntry {
            project_id: project.project.id.clone(),
            project_name: project.project.name.clone(),
            original_root_dir: path_to_string(root),
            backup_relative_dir: path_to_string(&project_backup_relative),
            file_count: project_files.len(),
            files: project_files,
            excluded_entries: excluded.clone(),
        });
        reported_exclusions.extend(
            excluded
                .into_iter()
                .map(|item| format!("项目“{}”：{item}", project.project.name)),
        );
    }

    let checksum_manifest_path = backup_dir.join(BACKUP_MANIFEST_FILE);
    write_json_atomic(&checksum_manifest_path, &manifest)?;

    Ok(LocalBackupResult {
        backup_dir: path_to_string(&backup_dir),
        global_files: manifest
            .global_files
            .iter()
            .map(|item| item.relative_path.clone())
            .collect(),
        project_roots: manifest
            .projects
            .iter()
            .map(|item| item.original_root_dir.clone())
            .collect(),
        excluded_entries: reported_exclusions,
        checksum_manifest_path: path_to_string(&checksum_manifest_path),
        created_at: manifest.created_at,
    })
}

fn restore_local_backup_from_paths(
    global_dir: &Path,
    backup_root: &Path,
    restore_base: &Path,
    registry_path: &Path,
    confirmed: bool,
) -> Result<BackupRestoreResult, String> {
    if !confirmed {
        return Err("恢复操作需要先由用户确认数据覆盖与新建项目目录。".to_string());
    }
    let backup_root = canonical_backup_root(backup_root)?;
    let global_dir = canonical_existing_directory(global_dir, "应用数据目录")?;
    let restore_candidate = absolute_normalized_path(restore_base)?;
    ensure_no_reparse_components(&restore_candidate)?;
    if paths_overlap(&global_dir, &restore_candidate)
        || paths_overlap(&restore_candidate, &backup_root)
    {
        return Err("恢复项目目录不能位于应用数据目录或备份目录内部。".to_string());
    }
    let expected_registry = global_dir.join("projects.json");
    if !same_path(registry_path, &expected_registry) {
        return Err("项目注册表目标不在应用数据目录内，已拒绝恢复。".to_string());
    }
    reject_existing_link(registry_path)?;

    let (manifest, mut warnings) = validate_backup_bundle(&backup_root)?;
    let restore_base = canonical_restore_directory(&restore_candidate)?;
    if paths_overlap(&global_dir, &restore_base) || paths_overlap(&restore_base, &backup_root) {
        return Err("恢复项目目录规范化后与应用数据或备份目录重叠，已拒绝恢复。".to_string());
    }
    let mut registry: AppRegistry = if registry_path.exists() {
        let contents = fs::read_to_string(registry_path)
            .map_err(|err| format!("读取当前项目注册表失败：{err}"))?;
        serde_json::from_str(&contents)
            .map_err(|err| format!("当前项目注册表无效，未开始恢复：{err}"))?
    } else {
        AppRegistry {
            schema_version: crate::models::MANIFEST_SCHEMA_VERSION,
            projects: Vec::new(),
            continue_preferences: Vec::new(),
        }
    };
    registry.schema_version = crate::models::MANIFEST_SCHEMA_VERSION;
    let scratch = unique_child_dir(&global_dir, ".ganmaoyuan-restore-stage")?;
    let mut restored_roots = Vec::new();
    let mut changed_globals = Vec::new();
    let mut managed_directory_snapshot: Option<(PathBuf, PathBuf, bool)> = None;

    let restore_result =
        (|| -> Result<(Vec<crate::models::ProjectSummary>, Vec<String>), String> {
            let mut restored_projects = Vec::new();
            for project in &manifest.projects {
                let source_root = safe_bundle_path(&backup_root, &project.backup_relative_dir)?;
                let target_root =
                    unique_child_dir(&restore_base, &sanitize_slug(&project.project_name))?;
                restored_roots.push(target_root.clone());
                copy_dir_recursive(&source_root, &target_root)?;
                verify_project_copy(&backup_root, project, &target_root)?;
                let mut restored_manifest =
                    project_service::read_and_repair_manifest(&target_root)?;
                if restored_manifest.project.id != project.project_id {
                    return Err("恢复后的项目 ID 与备份清单不一致。".to_string());
                }
                rewrite_manifest_root_paths(&mut restored_manifest, &target_root);
                warnings.extend(rebuild_project_extracted_text_cache(
                    &target_root,
                    &mut restored_manifest,
                ));
                project_service::persist_project(&target_root, &restored_manifest, None)?;
                registry
                    .projects
                    .retain(|item| item.id != restored_manifest.project.id);
                registry
                    .projects
                    .insert(0, restored_manifest.project.clone());
                restored_projects.push(restored_manifest.project);
            }

            let mut restored_global_files = Vec::new();
            for item in &manifest.global_files {
                let relative = validate_global_backup_path(&item.relative_path)?;
                let name = relative
                    .file_name()
                    .and_then(|value| value.to_str())
                    .ok_or_else(|| "备份中的全局文件名无效。".to_string())?;
                let source = safe_bundle_path(&backup_root, &item.relative_path)?;
                let global_relative = relative
                    .strip_prefix("global")
                    .map_err(|_| "备份全局路径缺少 global 前缀。".to_string())?;
                let staged = safe_child_path(&scratch, &path_to_string(global_relative))?;
                copy_file_verified(&source, &staged)?;
                verify_backup_file(&backup_root, item)?;
                validate_global_backup_file(&item.relative_path, &staged)?;

                // The project registry is merged below; restoring an old registry
                // verbatim could hide projects created after the backup.
                if name == "projects.json" {
                    continue;
                }
                if normalize_bundle_path(&item.relative_path).starts_with("global/managed/") {
                    continue;
                }
                let destination = global_dir.join(name);
                reject_existing_link(&destination)?;
                let snapshot = capture_file_snapshot(&destination)?;
                changed_globals.push(snapshot);
                let contents = fs::read_to_string(&staged)
                    .map_err(|err| format!("读取暂存全局数据失败：{name}；{err}"))?;
                write_text_atomic(&destination, &contents)?;
                restored_global_files.push(path_to_string(&destination));
            }

            let has_managed_payload = manifest.schema_version >= 3
                && manifest.global_files.iter().any(|entry| {
                    normalize_bundle_path(&entry.relative_path).starts_with("global/managed/")
                });
            if has_managed_payload {
                let staged_managed = scratch.join("managed");
                let staged_registry = staged_managed.join("files.json");
                if staged_registry.exists() {
                    let mut document: GlobalFilesDocument = read_json(&staged_registry)?;
                    for file in &mut document.files {
                        if file.destination == "workspaceCleanup"
                            || !file.undone_at.trim().is_empty()
                            || matches!(
                                file.lifecycle_status.as_str(),
                                "undone" | "removed" | "cancelled"
                            )
                        {
                            continue;
                        }
                        let relative = validated_relative_path(&file.managed_relative_path)?;
                        let stored_path = safe_child_path(
                            &staged_managed,
                            &format!("{}/{}", file.destination, path_to_string(&relative)),
                        )?;
                        if !stored_path.is_file() {
                            return Err(format!(
                                "暂存恢复中缺少全局受管资料：{}。",
                                file.file_name
                            ));
                        }
                        file.managed_path = path_to_string(
                            &global_dir
                                .join("managed")
                                .join(&file.destination)
                                .join(relative),
                        );
                    }
                    let serialized = serde_json::to_vec_pretty(&document)
                        .map_err(|error| format!("序列化恢复后的全局资料登记失败：{error}"))?;
                    fs::write(&staged_registry, serialized)
                        .map_err(|error| format!("更新恢复后的全局资料路径失败：{error}"))?;
                }
                swap_managed_directory(&global_dir, &scratch, &mut managed_directory_snapshot)?;
                restored_global_files.push(path_to_string(&global_dir.join("managed")));
            }

            let registry_staged = scratch.join("projects.json");
            write_json_atomic(&registry_staged, &registry)?;
            let registry_snapshot = capture_file_snapshot(registry_path)?;
            changed_globals.push(registry_snapshot);
            let registry_contents = fs::read_to_string(&registry_staged)
                .map_err(|err| format!("读取暂存项目注册表失败：{err}"))?;
            write_text_atomic(registry_path, &registry_contents)?;
            restored_global_files.push(path_to_string(registry_path));
            Ok((restored_projects, restored_global_files))
        })();

    match restore_result {
        Ok((restored_projects, restored_global_files)) => {
            let cleanup_result = fs::remove_dir_all(&scratch)
                .map_err(|err| format!("清理本次恢复暂存目录失败：{err}"));
            if let Err(error) = cleanup_result {
                warnings.push(error);
            }
            warnings.push("项目注册表采用合并方式；新版本备份中的 global/managed 会替换本机全局受管资料目录；备份中的 API Key 与 Windows Credential Manager 凭据不会恢复。SHA-256 仅校验内容一致性，感冒院无法认证备份来源；恢复的项目事实仍须由你核对，只恢复你信任的备份。".to_string());
            Ok(BackupRestoreResult {
                backup_dir: path_to_string(&backup_root),
                restored_global_dir: path_to_string(&global_dir),
                restored_projects,
                restored_global_files,
                warnings,
                restored_at: now_string(),
            })
        }
        Err(error) => {
            let rollback_errors = rollback_file_snapshots(&changed_globals);
            let mut rollback_errors = rollback_errors;
            let mut preserve_scratch = false;
            if let Some((managed_dir, previous_dir, had_previous)) =
                managed_directory_snapshot.take()
            {
                if managed_dir.exists() {
                    match fs::symlink_metadata(&managed_dir) {
                        Ok(metadata) if metadata.is_dir() && !is_link_or_reparse(&metadata) => {
                            if let Err(error) = fs::remove_dir_all(&managed_dir) {
                                rollback_errors.push(format!("移除新全局资料目录失败：{error}"));
                            }
                        }
                        Ok(_) => {
                            rollback_errors.push("新全局资料目录类型异常，未尝试删除。".to_string())
                        }
                        Err(error) => {
                            rollback_errors.push(format!("检查新全局资料目录失败：{error}"))
                        }
                    }
                }
                if had_previous && previous_dir.exists() {
                    if let Err(error) = fs::rename(&previous_dir, &managed_dir) {
                        preserve_scratch = true;
                        rollback_errors.push(format!("恢复原全局资料目录失败：{error}"));
                    }
                }
            }
            for root in restored_roots.iter().rev() {
                if path_is_within(root, &restore_base) {
                    let _ = fs::remove_dir_all(root);
                }
            }
            let mut message = error;
            if !rollback_errors.is_empty() {
                message.push_str("；回滚时仍有项目需要检查：");
                message.push_str(&rollback_errors.join("；"));
            }
            if preserve_scratch {
                message.push_str("；为保留原全局资料快照，暂存目录未清理：");
                message.push_str(&path_to_string(&scratch));
            } else if let Err(cleanup_error) = fs::remove_dir_all(&scratch) {
                message.push_str("；清理本次恢复暂存目录失败：");
                message.push_str(&cleanup_error.to_string());
            }
            Err(message)
        }
    }
}

fn build_backup_file_entry(root: &Path, path: &Path) -> Result<BackupFileEntry, String> {
    Ok(BackupFileEntry {
        relative_path: path_to_string(path.strip_prefix(root).unwrap_or(path)),
        checksum: hash_file(path)?,
        size_bytes: fs::metadata(path)
            .map_err(|err| format!("读取备份文件信息失败：{}；{err}", path_to_string(path)))?
            .len(),
    })
}

fn copy_global_managed_for_backup(
    global_dir: &Path,
    backup_global_root: &Path,
) -> Result<(), String> {
    let source_root = global_dir.join("managed");
    match fs::symlink_metadata(&source_root) {
        Ok(metadata) if metadata.is_dir() && !is_link_or_reparse(&metadata) => {}
        Ok(_) => return Err("全局受管资料根目录不是普通目录，已拒绝备份。".to_string()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(format!("读取全局受管资料目录失败：{error}")),
    }
    ensure_no_reparse_components(&source_root)?;
    let source_root = source_root
        .canonicalize()
        .map_err(|error| format!("规范化全局受管资料目录失败：{error}"))?;

    let registry_path = source_root.join("files.json");
    let registry_exists = match fs::symlink_metadata(&registry_path) {
        Ok(metadata) if metadata.is_file() && !is_link_or_reparse(&metadata) => true,
        Ok(_) => return Err("全局受管资料登记不是普通文件，已拒绝备份。".to_string()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(error) => return Err(format!("检查全局受管资料登记失败：{error}")),
    };
    let backup_registry_path = registry_path.with_file_name("files.json.bak");
    let backup_registry_exists = match fs::symlink_metadata(&backup_registry_path) {
        Ok(metadata) if metadata.is_file() && !is_link_or_reparse(&metadata) => true,
        Ok(_) => return Err("全局受管资料 .bak 登记不是普通文件，已拒绝备份。".to_string()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(error) => return Err(format!("检查全局受管资料 .bak 登记失败：{error}")),
    };
    let (document, recovered_notice): (
        GlobalFilesDocument,
        Option<crate::storage::JsonRecoveryNotice>,
    ) = if registry_exists || backup_registry_exists {
        read_json_with_recovery_notice(&registry_path)?
    } else {
        (GlobalFilesDocument::default(), None)
    };
    if let Some(notice) = recovered_notice {
        return Err(format!(
            "全局受管资料登记当前只能从旧 .bak 读取，未生成可能不完整的备份：{}",
            notice.message
        ));
    }
    let registry_target = backup_global_root.join("managed/files.json");
    if registry_exists {
        copy_file_verified(&registry_path, &registry_target)?;
    } else {
        let empty_registry = serde_json::to_vec_pretty(&document)
            .map_err(|error| format!("序列化空的全局资料登记失败：{error}"))?;
        write_bytes_new(&registry_target, &empty_registry)?;
    }
    validate_global_json("files.json", &registry_target)?;

    let audit_path = source_root.join("audit/operations.jsonl");
    match fs::symlink_metadata(&audit_path) {
        Ok(metadata) if metadata.is_file() && !is_link_or_reparse(&metadata) => {
            let audit_target = backup_global_root.join("managed/audit/operations.jsonl");
            copy_file_verified(&audit_path, &audit_target)?;
        }
        Ok(_) => return Err("全局资料操作记录不是普通文件，已拒绝备份。".to_string()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("检查全局资料操作记录失败：{error}")),
    }

    for item in &document.files {
        if item.destination == "workspaceCleanup" {
            continue;
        }
        if item.destination != "general" && item.destination != "temporary" {
            return Err(format!(
                "全局受管资料“{}”的归属类型无法安全纳入备份：{}。",
                item.file_name, item.destination
            ));
        }
        if !item.undone_at.trim().is_empty()
            || matches!(
                item.lifecycle_status.as_str(),
                "undone" | "removed" | "cancelled"
            )
        {
            continue;
        }
        let relative = validated_relative_path(&item.managed_relative_path)
            .map_err(|error| format!("全局受管资料登记路径无效（{}）：{error}", item.file_name))?;
        let relative_text = path_to_string(&relative);
        let source_relative = format!("{}/{}", item.destination, relative_text);
        let source = safe_child_path(&source_root, &source_relative)?;
        let source_metadata = fs::symlink_metadata(&source)
            .map_err(|error| format!("登记的全局受管资料不可读取：{}；{error}", item.file_name))?;
        if !source_metadata.is_file() || is_link_or_reparse(&source_metadata) {
            return Err(format!(
                "登记的全局受管资料不是普通文件：{}。",
                item.file_name
            ));
        }
        let canonical_source = source
            .canonicalize()
            .map_err(|error| format!("规范化全局受管资料失败：{}；{error}", item.file_name))?;
        if !path_is_within(&canonical_source, &source_root) {
            return Err(format!(
                "全局受管资料越出应用管理目录：{}。",
                item.file_name
            ));
        }
        if !item.content_hash.trim().is_empty()
            && hash_file(&canonical_source)? != item.content_hash.to_ascii_lowercase()
        {
            return Err(format!(
                "全局受管资料哈希与登记不一致：{}。",
                item.file_name
            ));
        }
        let backup_relative = format!("managed/{source_relative}");
        let destination = safe_child_path(backup_global_root, &backup_relative)?;
        copy_file_verified(&canonical_source, &destination)?;
    }
    Ok(())
}

fn write_bytes_new(path: &Path, contents: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "全局资料暂存文件缺少父目录。".to_string())?;
    fs::create_dir_all(parent).map_err(|error| format!("创建全局资料暂存目录失败：{error}"))?;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| format!("创建全局资料暂存文件失败：{error}"))?;
    use std::io::Write;
    file.write_all(contents)
        .and_then(|_| file.sync_all())
        .map_err(|error| format!("写入全局资料暂存文件失败：{error}"))
}

fn copy_project_for_backup(
    source: &Path,
    destination: &Path,
    project: &ProjectManifest,
) -> Result<Vec<String>, String> {
    let source = canonical_existing_directory(source, "已登记项目目录")?;
    let app_data = source.join(".ganmaoyuan");
    let app_data_metadata = fs::symlink_metadata(&app_data)
        .map_err(|error| format!("项目应用数据目录不可读取：{error}"))?;
    if !app_data_metadata.is_dir() || is_link_or_reparse(&app_data_metadata) {
        return Err("项目 .ganmaoyuan 应用数据根不是普通目录，已拒绝备份。".to_string());
    }
    ensure_no_reparse_components(&app_data)?;

    let mut excluded = vec![
        "项目根目录中未登记的源码仓库、.git、node_modules、构建产物、临时依赖及其他文件未扫描、未复制。".to_string(),
        ".ganmaoyuan/analysis/content/**（可重建的解析正文缓存）".to_string(),
        ".ganmaoyuan/staging/**（临时暂存数据）".to_string(),
    ];
    copy_dir_recursive_filtered(
        &app_data,
        &destination.join(".ganmaoyuan"),
        &source,
        &mut |relative, _| !is_excluded_project_data_path(relative),
    )?;

    let mut copied_paths = HashSet::new();
    for file in &project.files {
        let display_name = if file.file_name.trim().is_empty() {
            "未命名资料"
        } else {
            file.file_name.as_str()
        };
        if file.managed_relative_path.trim().is_empty() {
            excluded.push(format!(
                "登记资料“{display_name}”缺少项目相对路径，无法安全纳入备份。"
            ));
            continue;
        }
        let relative = validated_relative_path(&file.managed_relative_path).map_err(|error| {
            format!("登记受管资料路径无效（{display_name}），已停止备份：{error}")
        })?;
        if is_excluded_project_data_path(&relative) {
            excluded.push(format!(
                "登记资料“{display_name}”位于可重建缓存或暂存目录，未纳入备份。"
            ));
            continue;
        }
        if is_source_or_generated_directory(&relative) {
            excluded.push(format!(
                "登记资料“{display_name}”位于源码、依赖或构建目录，未纳入备份。"
            ));
            continue;
        }

        let relative_text = path_to_string(&relative);
        let source_path = safe_child_path(&source, &relative_text)?;
        let metadata = match fs::symlink_metadata(&source_path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                excluded.push(format!(
                    "登记资料“{display_name}”在项目目录中缺失；外部原始文件不会被访问或备份。"
                ));
                continue;
            }
            Err(error) => {
                return Err(format!("检查登记资料失败（{display_name}）：{error}"));
            }
        };
        if !metadata.is_file() || is_link_or_reparse(&metadata) {
            return Err(format!(
                "登记受管资料不是普通文件或包含链接，已拒绝备份：{display_name}。"
            ));
        }
        let canonical_file = source_path
            .canonicalize()
            .map_err(|error| format!("规范化登记资料失败（{display_name}）：{error}"))?;
        if !path_is_within(&canonical_file, &source) {
            return Err(format!(
                "登记资料越出项目目录，已拒绝备份：{display_name}。"
            ));
        }

        let key = normalize_bundle_path(&relative_text).to_ascii_lowercase();
        if is_project_data_root_path(&relative) || !copied_paths.insert(key) {
            continue;
        }
        let target = safe_child_path(destination, &relative_text)?;
        copy_file_verified(&canonical_file, &target)?;
    }
    Ok(excluded)
}

fn is_excluded_project_data_path(relative: &Path) -> bool {
    let mut components = relative.components();
    if !components
        .next()
        .and_then(|part| part.as_os_str().to_str())
        .is_some_and(|part| part.eq_ignore_ascii_case(".ganmaoyuan"))
    {
        return false;
    }
    let Some(second) = components.next().and_then(|part| part.as_os_str().to_str()) else {
        return false;
    };
    if second.eq_ignore_ascii_case("staging") {
        return true;
    }
    second.eq_ignore_ascii_case("analysis")
        && components
            .next()
            .and_then(|part| part.as_os_str().to_str())
            .is_some_and(|part| part.eq_ignore_ascii_case("content"))
}

fn is_project_data_root_path(relative: &Path) -> bool {
    relative
        .components()
        .next()
        .and_then(|part| part.as_os_str().to_str())
        .is_some_and(|part| part.eq_ignore_ascii_case(".ganmaoyuan"))
}

fn is_source_or_generated_directory(relative: &Path) -> bool {
    if is_project_data_root_path(relative) {
        return false;
    }
    relative.components().any(|component| {
        component.as_os_str().to_str().is_some_and(|part| {
            [
                ".git",
                "node_modules",
                "target",
                "dist",
                "build",
                "out",
                "coverage",
                ".next",
                ".vite",
                ".turbo",
                "vendor",
            ]
            .iter()
            .any(|excluded| part.eq_ignore_ascii_case(excluded))
        })
    })
}

fn collect_backup_file_entries(
    backup_root: &Path,
    project_backup_root: &Path,
) -> Result<Vec<BackupFileEntry>, String> {
    let mut paths = Vec::new();
    collect_regular_files(project_backup_root, &mut paths)?;
    if paths.len() > MAX_BACKUP_ENTRIES {
        return Err("项目文件数量超过备份安全上限。".to_string());
    }
    paths
        .into_iter()
        .map(|path| build_backup_file_entry(backup_root, &path))
        .collect()
}

fn validate_backup_bundle(
    backup_root: &Path,
) -> Result<(BackupBundleManifest, Vec<String>), String> {
    let manifest_path = safe_bundle_path(backup_root, BACKUP_MANIFEST_FILE)?;
    if !fs::symlink_metadata(&manifest_path)
        .map_err(|err| format!("备份清单不可读取：{err}"))?
        .is_file()
    {
        return Err("备份清单不是普通文件。".to_string());
    }
    let raw =
        fs::read_to_string(&manifest_path).map_err(|err| format!("读取备份清单失败：{err}"))?;
    let manifest: BackupBundleManifest =
        serde_json::from_str(&raw).map_err(|err| format!("备份清单格式无效：{err}"))?;
    if !matches!(manifest.schema_version, 1 | 2 | BACKUP_SCHEMA_VERSION) {
        return Err(format!("不支持备份清单版本 {}。", manifest.schema_version));
    }
    if manifest.app_version.trim().is_empty()
        || manifest.app_version.chars().count() > 64
        || manifest.created_at.trim().is_empty()
        || manifest.created_at.chars().count() > 128
    {
        return Err("备份清单缺少有效的版本或创建时间。".to_string());
    }
    let mut warnings = Vec::new();
    if manifest.schema_version >= 2 {
        if manifest.bundle_format != BACKUP_FORMAT || !looks_like_uuid(&manifest.bundle_id) {
            return Err(
                "备份格式标记无效，无法识别为受支持的感冒院备份；格式标记本身不证明备份来源。"
                    .to_string(),
            );
        }
    } else {
        warnings.push(
            "这是旧版备份：项目文件没有逐文件 SHA-256 清单；只会恢复到新建项目目录，无法验证其备份时完整性。".to_string(),
        );
    }
    if manifest.schema_version < 3 {
        warnings.push(
            "此旧版备份不包含全局受管资料副本；恢复时保留本机现有 managed 目录，不会用空内容覆盖。"
                .to_string(),
        );
    }
    validate_backup_exclusion_descriptions(&manifest.excluded_entries)?;
    warnings.extend(
        manifest
            .excluded_entries
            .iter()
            .map(|item| format!("备份范围说明：{item}")),
    );

    let entry_count = manifest.global_files.len()
        + manifest
            .projects
            .iter()
            .map(|project| project.files.len())
            .sum::<usize>();
    if entry_count > MAX_BACKUP_ENTRIES || manifest.projects.len() > 10_000 {
        return Err("备份清单条目数量超过安全上限。".to_string());
    }

    let mut seen_paths = HashSet::new();
    let mut expected_paths = HashSet::new();
    expected_paths.insert(BACKUP_MANIFEST_FILE.to_ascii_lowercase());
    for item in &manifest.global_files {
        validate_global_backup_path(&item.relative_path)?;
        insert_unique_backup_path(&mut seen_paths, &item.relative_path)?;
        verify_backup_file(backup_root, item)?;
        expected_paths.insert(normalize_bundle_path(&item.relative_path).to_ascii_lowercase());
    }
    let mut seen_project_ids = HashSet::new();
    for project in &manifest.projects {
        validate_backup_exclusion_descriptions(&project.excluded_entries)?;
        warnings.extend(
            project
                .excluded_entries
                .iter()
                .map(|item| format!("项目“{}”未包含：{item}", project.project_name)),
        );
        if !valid_project_id(&project.project_id)
            || project.project_name.trim().is_empty()
            || project.project_name.chars().count() > 200
            || !seen_project_ids.insert(project.project_id.to_ascii_lowercase())
        {
            return Err("備份中有重复或无效的项目身份信息。".to_string());
        }
        let expected_dir = format!(
            "projects/{}-{}",
            sanitize_slug(&project.project_name),
            project.project_id
        );
        if normalize_bundle_path(&project.backup_relative_dir) != expected_dir {
            return Err("项目备份目录与清单中的项目身份不匹配。".to_string());
        }
        let source_root = safe_bundle_path(backup_root, &project.backup_relative_dir)?;
        let metadata = fs::symlink_metadata(&source_root)
            .map_err(|err| format!("项目备份目录不可读取：{err}"))?;
        if !metadata.is_dir() || is_link_or_reparse(&metadata) {
            return Err("项目备份根必须是普通目录，不能是目录链接。".to_string());
        }
        let project_manifest_path = safe_bundle_path(
            backup_root,
            &format!("{expected_dir}/.ganmaoyuan/project-location-manifest.json"),
        )?;
        let project_manifest_text = fs::read_to_string(&project_manifest_path)
            .map_err(|err| format!("项目位置清单不可读取：{err}"))?;
        let project_manifest: ProjectManifest = serde_json::from_str(&project_manifest_text)
            .map_err(|err| format!("项目位置清单无效：{err}"))?;
        if project_manifest.project.id != project.project_id
            || project_manifest.project.name != project.project_name
        {
            return Err("备份清单与项目位置清单的 ID/名称不一致。".to_string());
        }

        if manifest.schema_version >= 2 {
            if project.files.is_empty() {
                return Err(format!(
                    "项目“{}”缺少逐文件完整性清单。",
                    project.project_name
                ));
            }
            let prefix = format!("{expected_dir}/");
            let mut project_expected = HashSet::new();
            for item in &project.files {
                let normalized = normalize_bundle_path(&item.relative_path);
                if !normalized.starts_with(&prefix) {
                    return Err("项目文件校验路径越出其备份目录。".to_string());
                }
                insert_unique_backup_path(&mut seen_paths, &item.relative_path)?;
                verify_backup_file(backup_root, item)?;
                let normalized = normalized.to_ascii_lowercase();
                expected_paths.insert(normalized.clone());
                project_expected.insert(normalized);
            }
            let actual_files = collect_regular_files(&source_root, &mut Vec::new())?;
            let actual_project = actual_files
                .iter()
                .map(|path| {
                    path.strip_prefix(&source_root)
                        .map(|relative| {
                            format!(
                                "{}/{}",
                                expected_dir,
                                normalize_bundle_path(&path_to_string(relative))
                            )
                            .to_ascii_lowercase()
                        })
                        .map_err(|_| "项目备份文件越出项目目录。".to_string())
                })
                .collect::<Result<HashSet<_>, _>>()?;
            if actual_project != project_expected {
                return Err(format!(
                    "项目“{}”的实际文件与校验清单不一致，未执行恢复。",
                    project.project_name
                ));
            }
        } else {
            let mut project_files = Vec::new();
            collect_regular_files(&source_root, &mut project_files)?;
            for path in project_files {
                let relative = path
                    .strip_prefix(backup_root)
                    .map_err(|_| "旧版项目文件越出备份目录。".to_string())?;
                expected_paths
                    .insert(normalize_bundle_path(&path_to_string(relative)).to_ascii_lowercase());
            }
        }
    }

    for name in &manifest.global_files {
        let path = safe_bundle_path(backup_root, &name.relative_path)?;
        validate_global_backup_file(&name.relative_path, &path)?;
    }
    if manifest.schema_version >= 3 {
        let has_managed_entries = manifest.global_files.iter().any(|entry| {
            normalize_bundle_path(&entry.relative_path).starts_with("global/managed/")
        });
        let registry_entry = manifest.global_files.iter().find(|entry| {
            normalize_bundle_path(&entry.relative_path) == "global/managed/files.json"
        });
        if has_managed_entries && registry_entry.is_none() {
            return Err("全局受管资料备份缺少登记清单，未执行恢复。".to_string());
        }
        if let Some(registry_entry) = registry_entry {
            let registry_path = safe_bundle_path(backup_root, &registry_entry.relative_path)?;
            let document: GlobalFilesDocument = read_json(&registry_path)?;
            for item in &document.files {
                if item.destination == "workspaceCleanup" {
                    continue;
                }
                if item.destination != "general" && item.destination != "temporary" {
                    return Err(format!(
                        "全局受管资料登记包含不支持的归属类型：{}。",
                        item.destination
                    ));
                }
                if !item.undone_at.trim().is_empty()
                    || matches!(
                        item.lifecycle_status.as_str(),
                        "undone" | "removed" | "cancelled"
                    )
                {
                    continue;
                }
                let relative = validated_relative_path(&item.managed_relative_path)?;
                let expected = format!(
                    "global/managed/{}/{}",
                    item.destination,
                    normalize_bundle_path(&path_to_string(&relative))
                )
                .to_ascii_lowercase();
                if !seen_paths.contains(&expected) {
                    return Err(format!(
                        "全局受管资料备份缺少已登记文件：{}。",
                        item.file_name
                    ));
                }
            }
            if document
                .files
                .iter()
                .any(|item| item.destination == "workspaceCleanup")
            {
                warnings.push("全局资料登记中有位于用户工作区的 workspaceCleanup 文件；该外部目录未复制，恢复后仍需在原目录存在或由用户重新定位。".to_string());
            }
        }
    }

    let mut actual_paths = Vec::new();
    collect_regular_files(backup_root, &mut actual_paths)?;
    let actual_paths = actual_paths
        .iter()
        .map(|path| {
            let relative = path.strip_prefix(backup_root).unwrap_or(path);
            normalize_bundle_path(&path_to_string(relative)).to_ascii_lowercase()
        })
        .collect::<HashSet<_>>();
    if actual_paths != expected_paths {
        return Err("备份目录含有清单外文件，或缺少清单列出的文件；未执行恢复。".to_string());
    }
    Ok((manifest, warnings))
}

fn validate_backup_exclusion_descriptions(items: &[String]) -> Result<(), String> {
    if items.len() > MAX_BACKUP_ENTRIES
        || items.iter().any(|item| {
            item.trim().is_empty()
                || item.chars().count() > 2_048
                || item.chars().any(char::is_control)
        })
    {
        return Err("备份清单中的排除范围说明无效或超过安全上限。".to_string());
    }
    Ok(())
}

fn verify_project_copy(
    backup_root: &Path,
    project: &ProjectBackupEntry,
    target_root: &Path,
) -> Result<(), String> {
    if project.files.is_empty() {
        return Ok(());
    }
    let prefix = format!("{}/", normalize_bundle_path(&project.backup_relative_dir));
    for entry in &project.files {
        let normalized = normalize_bundle_path(&entry.relative_path);
        let relative = normalized
            .strip_prefix(&prefix)
            .ok_or_else(|| "项目文件校验路径越出备份目录。".to_string())?;
        let copied_path = safe_child_path(target_root, relative)?;
        verify_file_contents(&copied_path, entry)?;
        verify_backup_file(backup_root, entry)?;
    }
    Ok(())
}

fn validate_global_json(name: &str, path: &Path) -> Result<(), String> {
    let text =
        fs::read_to_string(path).map_err(|err| format!("备份全局文件无法读取：{name}；{err}"))?;
    let invalid = |err: serde_json::Error| format!("备份全局文件结构无效：{name}；{err}");
    match name {
        "projects.json" => serde_json::from_str::<AppRegistry>(&text)
            .map(|_| ())
            .map_err(invalid),
        "memos.json" => serde_json::from_str::<Vec<MemoItem>>(&text)
            .map(|_| ())
            .map_err(invalid),
        "material-inbox.json" => serde_json::from_str::<MaterialInboxDocument>(&text)
            .map(|_| ())
            .map_err(invalid),
        "inbox-routing-settings.json" => serde_json::from_str::<InboxRoutingSettings>(&text)
            .map(|_| ())
            .map_err(invalid),
        DEEPSEEK_SETTINGS_FILE => serde_json::from_str::<DeepSeekSettings>(&text)
            .map(|_| ())
            .map_err(invalid),
        "files.json" => serde_json::from_str::<GlobalFilesDocument>(&text)
            .map(|_| ())
            .map_err(invalid),
        _ => Err("备份清单包含不允许恢复的全局文件。".to_string()),
    }
}

fn validate_global_backup_file(relative_path: &str, path: &Path) -> Result<(), String> {
    let normalized = normalize_bundle_path(relative_path);
    if normalized.starts_with("global/managed/") && normalized != "global/managed/files.json" {
        return Ok(());
    }
    let name = Path::new(relative_path)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    validate_global_json(name, path)
}

fn validate_global_backup_path(value: &str) -> Result<PathBuf, String> {
    let relative = validated_relative_path(value)?;
    let parts = value.split(['/', '\\']).collect::<Vec<_>>();
    let fixed_global = parts.len() == 2
        && parts.first() == Some(&"global")
        && BACKUP_GLOBAL_FILES.contains(&parts[1]);
    let managed_path = parts.first() == Some(&"global")
        && parts.get(1) == Some(&"managed")
        && match parts.get(2).copied() {
            Some("files.json") if parts.len() == 3 => true,
            Some("audit") if parts.len() == 4 && parts[3] == "operations.jsonl" => true,
            Some("general" | "temporary") if parts.len() >= 4 => true,
            _ => false,
        };
    if !fixed_global && !managed_path {
        return Err("备份全局文件路径不在允许的固定文件清单内。".to_string());
    }
    Ok(relative)
}

fn verify_backup_file(root: &Path, entry: &BackupFileEntry) -> Result<(), String> {
    if entry.checksum.len() != 64 || !entry.checksum.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("文件校验值无效：{}", entry.relative_path));
    }
    let path = safe_bundle_path(root, &entry.relative_path)?;
    verify_file_contents(&path, entry)
}

fn verify_file_contents(path: &Path, entry: &BackupFileEntry) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|err| format!("备份文件缺失或不可读取：{}；{err}", entry.relative_path))?;
    if !metadata.is_file() || is_link_or_reparse(&metadata) {
        return Err(format!("备份项不是普通文件：{}", entry.relative_path));
    }
    if metadata.len() != entry.size_bytes || hash_file(path)? != entry.checksum.to_ascii_lowercase()
    {
        return Err(format!(
            "备份文件大小或 SHA-256 校验失败：{}",
            entry.relative_path
        ));
    }
    Ok(())
}

fn insert_unique_backup_path(seen: &mut HashSet<String>, path: &str) -> Result<(), String> {
    let normalized = normalize_bundle_path(path).to_ascii_lowercase();
    if !seen.insert(normalized) {
        return Err(format!("备份清单包含重复文件路径：{path}"));
    }
    Ok(())
}

fn safe_bundle_path(root: &Path, value: &str) -> Result<PathBuf, String> {
    let relative = validated_relative_path(value)?;
    let root = root
        .canonicalize()
        .map_err(|err| format!("规范化备份目录失败：{err}"))?;
    let mut current = root.clone();
    for component in relative.components() {
        current.push(component.as_os_str());
        let metadata = fs::symlink_metadata(&current).map_err(|err| {
            format!(
                "备份路径项不存在或不可读取：{}；{err}",
                path_to_string(&current)
            )
        })?;
        if is_link_or_reparse(&metadata) {
            return Err(format!(
                "备份路径包含符号链接或重解析点，已拒绝：{}",
                path_to_string(&current)
            ));
        }
    }
    let canonical = current
        .canonicalize()
        .map_err(|err| format!("规范化备份项失败：{err}"))?;
    if !path_is_within(&canonical, &root) {
        return Err("备份路径越出用户选择的备份目录。".to_string());
    }
    Ok(canonical)
}

fn safe_child_path(root: &Path, value: &str) -> Result<PathBuf, String> {
    let relative = validated_relative_path(value)?;
    let root = root
        .canonicalize()
        .map_err(|err| format!("规范化恢复项目目录失败：{err}"))?;
    let mut current = root.clone();
    for component in relative.components() {
        current.push(component.as_os_str());
        if let Ok(metadata) = fs::symlink_metadata(&current) {
            if is_link_or_reparse(&metadata) {
                return Err("恢复目标包含链接或重解析点。".to_string());
            }
        }
    }
    if current.starts_with(&root) {
        Ok(current)
    } else {
        Err("恢复目标越出项目目录。".to_string())
    }
}

fn validated_relative_path(value: &str) -> Result<PathBuf, String> {
    if value.trim().is_empty()
        || value.starts_with(['/', '\\'])
        || value.contains(':')
        || value.contains('\0')
    {
        return Err(format!("备份路径不是安全的相对路径：{value}"));
    }
    let mut relative = PathBuf::new();
    for part in value.split(['/', '\\']) {
        if part.is_empty() || part == "." || part == ".." || part.chars().any(char::is_control) {
            return Err(format!("备份路径包含非法组件：{value}"));
        }
        relative.push(part);
    }
    if relative.is_absolute() {
        return Err(format!("备份路径不能是绝对路径：{value}"));
    }
    Ok(relative)
}

fn normalize_bundle_path(value: &str) -> String {
    value.replace('\\', "/")
}

fn valid_project_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn looks_like_uuid(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(index, byte)| match index {
            8 | 13 | 18 | 23 => byte == b'-',
            _ => byte.is_ascii_hexdigit(),
        })
}

fn collect_regular_files(root: &Path, files: &mut Vec<PathBuf>) -> Result<Vec<PathBuf>, String> {
    let mut local = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory)
            .map_err(|err| format!("读取备份目录失败：{}；{err}", path_to_string(&directory)))?
        {
            let entry = entry.map_err(|err| format!("遍历备份目录失败：{err}"))?;
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path)
                .map_err(|err| format!("读取备份项失败：{}；{err}", path_to_string(&path)))?;
            if is_link_or_reparse(&metadata) {
                return Err(format!(
                    "备份中存在符号链接或重解析点：{}",
                    path_to_string(&path)
                ));
            }
            if metadata.is_dir() {
                pending.push(path);
            } else if metadata.is_file() {
                local.push(path);
            } else {
                return Err("备份中存在非普通文件系统对象。".to_string());
            }
            if local.len() + pending.len() > MAX_BACKUP_ENTRIES {
                return Err("备份目录项目数量超过安全上限。".to_string());
            }
        }
    }
    files.extend(local.iter().cloned());
    Ok(local)
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

fn reject_existing_link(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if is_link_or_reparse(&metadata) => Err(format!(
            "恢复目标是链接或重解析点，已拒绝：{}",
            path_to_string(path)
        )),
        Ok(metadata) if !metadata.is_file() => {
            Err(format!("恢复目标不是普通文件：{}", path_to_string(path)))
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!(
            "检查恢复目标失败：{}；{error}",
            path_to_string(path)
        )),
    }
}

fn ensure_no_reparse_components(path: &Path) -> Result<(), String> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|err| format!("获取当前目录失败：{err}"))?
            .join(path)
    };
    let mut current = PathBuf::new();
    for component in absolute.components() {
        match component {
            std::path::Component::Prefix(prefix) => current.push(prefix.as_os_str()),
            std::path::Component::RootDir => current.push(component.as_os_str()),
            std::path::Component::CurDir => continue,
            std::path::Component::ParentDir => {
                current.pop();
                continue;
            }
            std::path::Component::Normal(part) => current.push(part),
        }
        if matches!(component, std::path::Component::Prefix(_)) {
            continue;
        }
        match fs::symlink_metadata(&current) {
            Ok(metadata) if is_link_or_reparse(&metadata) => {
                return Err(format!(
                    "路径包含链接或重解析点：{}",
                    path_to_string(&current)
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(error) => return Err(format!("检查路径组件失败：{error}")),
        }
    }
    Ok(())
}

fn absolute_normalized_path(path: &Path) -> Result<PathBuf, String> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|err| format!("获取当前目录失败：{err}"))?
            .join(path)
    };
    let mut normalized = PathBuf::new();
    for component in absolute.components() {
        match component {
            std::path::Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            std::path::Component::RootDir => normalized.push(component.as_os_str()),
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            std::path::Component::Normal(part) => normalized.push(part),
        }
    }
    if !normalized.is_absolute() {
        return Err("目标路径无法解析为绝对路径。".to_string());
    }
    Ok(normalized)
}

fn canonical_backup_root(path: &Path) -> Result<PathBuf, String> {
    ensure_no_reparse_components(path)?;
    let canonical = path
        .canonicalize()
        .map_err(|err| format!("备份目录不存在或不可访问：{err}"))?;
    if !canonical.is_dir() {
        return Err("备份来源必须是包含 backup-bundle.json 的目录。".to_string());
    }
    Ok(canonical)
}

fn canonical_existing_directory(path: &Path, label: &str) -> Result<PathBuf, String> {
    ensure_no_reparse_components(path)?;
    let canonical = path
        .canonicalize()
        .map_err(|err| format!("规范化{label}失败：{err}"))?;
    if !canonical.is_dir() {
        return Err(format!("{label}不是目录。"));
    }
    Ok(canonical)
}

fn canonical_restore_directory(path: &Path) -> Result<PathBuf, String> {
    create_directory_chain_no_reparse(path)?;
    canonical_existing_directory(path, "恢复项目目录")
}

fn paths_overlap(left: &Path, right: &Path) -> bool {
    path_is_within(left, right) || path_is_within(right, left)
}

fn path_is_within(path: &Path, root: &Path) -> bool {
    #[cfg(windows)]
    {
        let path = path_to_string(path)
            .trim_end_matches(['\\', '/'])
            .to_ascii_lowercase();
        let root = path_to_string(root)
            .trim_end_matches(['\\', '/'])
            .to_ascii_lowercase();
        path == root
            || path
                .strip_prefix(&root)
                .is_some_and(|tail| tail.starts_with('\\') || tail.starts_with('/'))
    }
    #[cfg(not(windows))]
    {
        path.starts_with(root)
    }
}

#[derive(Debug)]
struct FileSnapshot {
    path: PathBuf,
    original_contents: Option<Vec<u8>>,
    original_backup_contents: Option<Vec<u8>>,
}

fn capture_file_snapshot(path: &Path) -> Result<FileSnapshot, String> {
    reject_existing_link(path)?;
    let backup = backup_path_for(path);
    reject_existing_link(&backup)?;
    let original_contents = if path.exists() {
        Some(fs::read(path).map_err(|err| format!("读取恢复前文件失败：{err}"))?)
    } else {
        None
    };
    let original_backup_contents = if backup.exists() {
        Some(fs::read(&backup).map_err(|err| format!("读取恢复前备份文件失败：{err}"))?)
    } else {
        None
    };
    Ok(FileSnapshot {
        path: path.to_path_buf(),
        original_contents,
        original_backup_contents,
    })
}

fn rollback_file_snapshots(snapshots: &[FileSnapshot]) -> Vec<String> {
    let mut errors = Vec::new();
    for snapshot in snapshots.iter().rev() {
        match &snapshot.original_contents {
            Some(contents) => match std::str::from_utf8(contents) {
                Ok(text) => {
                    if let Err(error) = write_text_atomic(&snapshot.path, text) {
                        errors.push(format!("{}：{error}", path_to_string(&snapshot.path)));
                    }
                }
                Err(error) => errors.push(format!("回滚文件编码无效：{error}")),
            },
            None => {
                if let Err(error) = fs::remove_file(&snapshot.path) {
                    if error.kind() != std::io::ErrorKind::NotFound {
                        errors.push(format!("{}：{error}", path_to_string(&snapshot.path)));
                    }
                }
            }
        }
        let backup = backup_path_for(&snapshot.path);
        match &snapshot.original_backup_contents {
            Some(contents) => {
                if let Err(error) = fs::write(&backup, contents) {
                    errors.push(format!(
                        "恢复 {} 的旧备份文件失败：{error}",
                        path_to_string(&snapshot.path)
                    ));
                }
            }
            None => {
                if let Err(error) = fs::remove_file(&backup) {
                    if error.kind() != std::io::ErrorKind::NotFound {
                        errors.push(format!(
                            "清理 {} 的临时备份失败：{error}",
                            path_to_string(&snapshot.path)
                        ));
                    }
                }
            }
        }
    }
    errors
}

fn swap_managed_directory(
    global_dir: &Path,
    scratch: &Path,
    snapshot: &mut Option<(PathBuf, PathBuf, bool)>,
) -> Result<(), String> {
    if snapshot.is_some() {
        return Err("本次恢复尝试重复替换全局受管资料目录，已停止。".to_string());
    }
    let staged = scratch.join("managed");
    let staged_metadata = fs::symlink_metadata(&staged)
        .map_err(|error| format!("全局受管资料暂存目录不可用：{error}"))?;
    if !staged_metadata.is_dir() || is_link_or_reparse(&staged_metadata) {
        return Err("全局受管资料暂存项不是普通目录。".to_string());
    }
    let destination = global_dir.join("managed");
    let previous = scratch.join("managed-current");
    if previous.exists() {
        return Err("恢复暂存目录中已存在原全局资料快照，拒绝覆盖。".to_string());
    }
    let had_previous = match fs::symlink_metadata(&destination) {
        Ok(metadata) if metadata.is_dir() && !is_link_or_reparse(&metadata) => {
            ensure_no_reparse_components(&destination)?;
            true
        }
        Ok(_) => return Err("当前全局受管资料不是普通目录，已拒绝替换。".to_string()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(error) => return Err(format!("检查当前全局受管资料目录失败：{error}")),
    };
    if had_previous {
        fs::rename(&destination, &previous)
            .map_err(|error| format!("暂存当前全局受管资料目录失败：{error}"))?;
    }
    *snapshot = Some((destination.clone(), previous.clone(), had_previous));
    if let Err(error) = fs::rename(&staged, &destination) {
        if had_previous {
            match fs::rename(&previous, &destination) {
                Ok(()) => *snapshot = None,
                Err(rollback_error) => {
                    return Err(format!(
                        "安装备份中的全局受管资料失败：{error}；恢复原目录也失败：{rollback_error}；原目录保留在 {}",
                        path_to_string(&previous)
                    ));
                }
            }
        } else {
            *snapshot = None;
        }
        return Err(format!("安装备份中的全局受管资料失败：{error}"));
    }
    Ok(())
}

fn backup_path_for(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("data.json");
    path.with_file_name(format!("{name}.bak"))
}

fn copy_dir_recursive(source: &Path, destination: &Path) -> Result<(), String> {
    copy_dir_recursive_filtered(source, destination, source, &mut |_, _| true)
}

fn copy_dir_recursive_filtered(
    source: &Path,
    destination: &Path,
    root: &Path,
    include: &mut dyn FnMut(&Path, &Path) -> bool,
) -> Result<(), String> {
    fs::create_dir_all(destination)
        .map_err(|err| format!("创建目录失败：{}；{err}", path_to_string(destination)))?;
    for entry in fs::read_dir(source)
        .map_err(|err| format!("读取目录失败：{}；{err}", path_to_string(source)))?
    {
        let entry =
            entry.map_err(|err| format!("遍历目录失败：{}；{err}", path_to_string(source)))?;
        let path = entry.path();
        let relative = path.strip_prefix(root).unwrap_or(&path);
        if !include(relative, &path) {
            continue;
        }
        let metadata = fs::symlink_metadata(&path)
            .map_err(|err| format!("读取文件类型失败：{}；{err}", path_to_string(&path)))?;
        if is_link_or_reparse(&metadata) {
            return Err(format!(
                "复制范围包含符号链接或重解析点：{}",
                path_to_string(&path)
            ));
        }
        let destination_path = destination.join(
            path.file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("item"),
        );
        if metadata.is_dir() {
            copy_dir_recursive_filtered(&path, &destination_path, root, include)?;
        } else if metadata.is_file() {
            copy_file_verified(&path, &destination_path)?;
        } else {
            return Err(format!("复制范围包含非普通文件：{}", path_to_string(&path)));
        }
    }
    Ok(())
}

fn copy_file_verified(source: &Path, destination: &Path) -> Result<(), String> {
    let source_metadata = fs::symlink_metadata(source)
        .map_err(|err| format!("读取源文件失败：{}；{err}", path_to_string(source)))?;
    if !source_metadata.is_file() || is_link_or_reparse(&source_metadata) {
        return Err(format!(
            "拒绝复制链接或非普通源文件：{}",
            path_to_string(source)
        ));
    }
    if let Ok(destination_metadata) = fs::symlink_metadata(destination) {
        if is_link_or_reparse(&destination_metadata) || !destination_metadata.is_file() {
            return Err(format!(
                "拒绝覆盖链接或非普通目标：{}",
                path_to_string(destination)
            ));
        }
    }
    if let Some(parent) = destination.parent() {
        ensure_no_reparse_components(parent)?;
        fs::create_dir_all(parent)
            .map_err(|err| format!("创建目录失败：{}；{err}", path_to_string(parent)))?;
    }
    fs::copy(source, destination).map_err(|err| {
        format!(
            "复制文件失败：{} -> {}；{err}",
            path_to_string(source),
            path_to_string(destination)
        )
    })?;
    let source_hash = hash_file(source)?;
    let destination_hash = hash_file(destination)?;
    if source_hash != destination_hash {
        return Err(format!(
            "复制校验失败：{} -> {}",
            path_to_string(source),
            path_to_string(destination)
        ));
    }
    Ok(())
}

fn unique_child_dir(parent: &Path, prefix: &str) -> Result<PathBuf, String> {
    if prefix.trim().is_empty()
        || prefix.contains(['/', '\\', ':'])
        || prefix.chars().any(char::is_control)
        || matches!(prefix, "." | "..")
    {
        return Err("输出目录名前缀无效。".to_string());
    }
    create_directory_chain_no_reparse(parent).map_err(|err| {
        format!(
            "创建或检查输出父目录失败：{}；{err}",
            path_to_string(parent)
        )
    })?;
    let parent = canonical_existing_directory(parent, "输出父目录")?;
    for _ in 0..16 {
        let candidate = parent.join(format!("{prefix}-{}", new_id()));
        match fs::create_dir(&candidate) {
            Ok(()) => {
                let metadata = fs::symlink_metadata(&candidate)
                    .map_err(|err| format!("检查新建输出目录失败：{err}"))?;
                if !metadata.is_dir() || is_link_or_reparse(&metadata) {
                    return Err("新建输出目录不是普通目录。".to_string());
                }
                let canonical = candidate
                    .canonicalize()
                    .map_err(|err| format!("规范化新建输出目录失败：{err}"))?;
                if !path_is_within(&canonical, &parent) {
                    return Err("新建输出目录越出所选父目录。".to_string());
                }
                return Ok(canonical);
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(format!(
                    "创建输出目录失败：{}；{error}",
                    path_to_string(&candidate)
                ));
            }
        }
    }
    Err("无法分配不冲突的输出目录；未覆盖任何现有目录。".to_string())
}

fn create_directory_chain_no_reparse(path: &Path) -> Result<(), String> {
    let absolute = absolute_normalized_path(path)?;
    let mut current = PathBuf::new();
    for component in absolute.components() {
        match component {
            std::path::Component::Prefix(prefix) => {
                current.push(prefix.as_os_str());
                continue;
            }
            std::path::Component::RootDir => current.push(component.as_os_str()),
            std::path::Component::CurDir => continue,
            std::path::Component::ParentDir => {
                current.pop();
                continue;
            }
            std::path::Component::Normal(part) => current.push(part),
        }
        match fs::symlink_metadata(&current) {
            Ok(metadata) if is_link_or_reparse(&metadata) => {
                return Err(format!(
                    "目标路径包含链接或重解析点：{}",
                    path_to_string(&current)
                ));
            }
            Ok(metadata) if !metadata.is_dir() => {
                return Err(format!(
                    "目标路径组件不是目录：{}",
                    path_to_string(&current)
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                match fs::create_dir(&current) {
                    Ok(()) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                    Err(error) => {
                        return Err(format!(
                            "创建目录失败：{}；{error}",
                            path_to_string(&current)
                        ));
                    }
                }
                let metadata = fs::symlink_metadata(&current)
                    .map_err(|error| format!("检查新建目录失败：{error}"))?;
                if is_link_or_reparse(&metadata) || !metadata.is_dir() {
                    return Err(format!(
                        "新建路径组件不是普通目录：{}",
                        path_to_string(&current)
                    ));
                }
            }
            Err(error) => {
                return Err(format!(
                    "检查目标路径组件失败：{}；{error}",
                    path_to_string(&current)
                ));
            }
        }
    }
    Ok(())
}

fn sanitize_slug(value: &str) -> String {
    let cleaned = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else if ch.is_whitespace() {
                '-'
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim_matches(['-', '_'])
        .to_string();
    if cleaned.is_empty() {
        "project".to_string()
    } else {
        cleaned
    }
}

fn rewrite_manifest_root_paths(manifest: &mut ProjectManifest, new_root: &Path) {
    let old_root = PathBuf::from(&manifest.project.root_dir);
    manifest.project.root_dir = path_to_string(new_root);
    manifest.project.manifest_path = path_to_string(&project_service::manifest_path(new_root));
    for file in &mut manifest.files {
        if !file.managed_relative_path.trim().is_empty() {
            file.managed_path = path_to_string(&new_root.join(&file.managed_relative_path));
        } else if let Ok(relative) = PathBuf::from(&file.managed_path).strip_prefix(&old_root) {
            file.managed_path = path_to_string(&new_root.join(relative));
        }
        if !file.extracted_text_path.trim().is_empty() {
            if let Ok(relative) = PathBuf::from(&file.extracted_text_path).strip_prefix(&old_root) {
                file.extracted_text_path = path_to_string(&new_root.join(relative));
            } else {
                file.extracted_text_path = path_to_string(
                    &new_root
                        .join(".ganmaoyuan/analysis/content")
                        .join(format!("{}.txt", file.id)),
                );
            }
        }
    }
    let managed_by_id = manifest
        .files
        .iter()
        .map(|file| (file.id.clone(), file.managed_path.clone()))
        .collect::<BTreeMap<_, _>>();
    for message in &mut manifest.messages {
        for attachment in &mut message.attachments {
            if let Some(path) = managed_by_id.get(&attachment.file_id) {
                attachment.managed_path = path.clone();
            } else if let Ok(relative) =
                PathBuf::from(&attachment.managed_path).strip_prefix(&old_root)
            {
                attachment.managed_path = path_to_string(&new_root.join(relative));
            }
        }
    }
    for artifact in &mut manifest.artifacts {
        if let Some(path) = managed_by_id.get(&artifact.file_id) {
            artifact.managed_path = path.clone();
        } else if !artifact.managed_path.trim().is_empty() {
            if let Ok(relative) = PathBuf::from(&artifact.managed_path).strip_prefix(&old_root) {
                artifact.managed_path = path_to_string(&new_root.join(relative));
            }
        }
    }
    for decision in &mut manifest.location_decisions {
        if let Ok(relative) = PathBuf::from(&decision.managed_path).strip_prefix(&old_root) {
            decision.managed_path = path_to_string(&new_root.join(relative));
        }
    }
    for report in &mut manifest.codex_reports {
        if let Ok(relative) = PathBuf::from(&report.evidence_managed_path).strip_prefix(&old_root) {
            report.evidence_managed_path = path_to_string(&new_root.join(relative));
        }
    }
}

fn rebuild_project_extracted_text_cache(
    project_root: &Path,
    manifest: &mut ProjectManifest,
) -> Vec<String> {
    let Ok(canonical_root) = project_root.canonicalize() else {
        return vec!["项目正文缓存未重建：恢复后的项目根目录无法规范化。".to_string()];
    };
    let mut warnings = Vec::new();
    for file in &mut manifest.files {
        if file.parse_status != "success" || file.extracted_text_path.trim().is_empty() {
            continue;
        }
        let managed_path = PathBuf::from(&file.managed_path);
        let Ok(canonical_managed) = managed_path.canonicalize() else {
            file.extracted_text_path.clear();
            warnings.push(format!(
                "项目资料“{}”的正文缓存未重建：受管文件不可读取。",
                file.file_name
            ));
            continue;
        };
        if !canonical_managed.starts_with(&canonical_root) || !canonical_managed.is_file() {
            file.extracted_text_path.clear();
            warnings.push(format!(
                "项目资料“{}”的正文缓存未重建：文件路径不在恢复项目内。",
                file.file_name
            ));
            continue;
        }
        let Ok(actual_hash) = hash_file(&canonical_managed) else {
            file.extracted_text_path.clear();
            warnings.push(format!(
                "项目资料“{}”的正文缓存未重建：无法校验受管文件。",
                file.file_name
            ));
            continue;
        };
        if file.content_hash.trim().is_empty()
            || actual_hash != file.content_hash.to_ascii_lowercase()
        {
            file.extracted_text_path.clear();
            warnings.push(format!(
                "项目资料“{}”的正文缓存未重建：资料哈希与登记不匹配。",
                file.file_name
            ));
            continue;
        }
        let file_type = file.file_type.to_ascii_lowercase();
        let (analysis, extracted_text) = analyze_managed_file(
            &file.id,
            &file.file_name,
            &canonical_managed,
            &file_type,
            &actual_hash,
        );
        let Some(extracted_text) = extracted_text.filter(|_| analysis.parse_status == "success")
        else {
            file.extracted_text_path.clear();
            warnings.push(format!(
                "项目资料“{}”的正文缓存未重建：现有解析器未能重新提取正文。",
                file.file_name
            ));
            continue;
        };
        let cache_path = extracted_text_path(project_root, &file.id);
        if let Err(error) = write_text_atomic(&cache_path, &extracted_text) {
            file.extracted_text_path.clear();
            warnings.push(format!(
                "项目资料“{}”的正文缓存未重建：{error}",
                file.file_name
            ));
            continue;
        }
        file.extracted_text_path = path_to_string(&cache_path);
    }
    warnings
}

fn build_safe_export_file_index(file: &FileRecord) -> SafeExportFileIndexEntry {
    SafeExportFileIndexEntry {
        file_name: file.file_name.clone(),
        managed_relative_path: file.managed_relative_path.clone(),
        file_type: file.file_type.clone(),
        category: file.category.clone(),
        recommended_category: file.recommended_category.clone(),
        current_version: file.current_version.clone(),
        created_source: file.created_source.clone(),
        related_task: file.related_task.clone(),
        still_exists: file.still_exists,
        needs_user_confirmation: file.needs_user_confirmation,
        parse_status: file.parse_status.clone(),
        content_summary: redact_preview(&file.content_summary, 180),
        main_fields_or_sections: file.main_fields_or_sections.clone(),
        analysis_warnings: file.analysis_warnings.clone(),
    }
}

fn build_safe_export_conversation(message: &WorkspaceMessage) -> SafeExportConversation {
    SafeExportConversation {
        id: message.id.clone(),
        author: message.author.clone(),
        kind: message.kind.clone(),
        created_at: message.created_at.clone(),
        status: message.status.clone(),
        related_task: message.related_task.clone(),
        source: message.source.clone(),
        model_id: message.model_id.clone(),
        text_preview: redact_preview(&message.text, 160),
        text_redacted: true,
    }
}

fn redact_preview(text: &str, max_chars: usize) -> String {
    let mut value = text.replace('\r', " ").replace('\n', " ");
    if value.contains("sk-") {
        value = value.replace("sk-", "sk-[redacted]-");
    }
    if value.trim().is_empty() {
        return String::new();
    }
    let chars = value.chars().collect::<Vec<_>>();
    let visible = chars.len().min(max_chars).min(48);
    let mut preview = chars[..visible].iter().collect::<String>();
    if chars.len() > visible {
        preview.push_str("...");
    }
    preview.push_str(" [redacted]");
    preview
}

fn build_project_diagnostic(manifest: &ProjectManifest) -> serde_json::Value {
    let category_counts =
        manifest
            .files
            .iter()
            .fold(BTreeMap::<String, usize>::new(), |mut map, file| {
                *map.entry(file.category.clone()).or_insert(0) += 1;
                map
            });
    json!({
        "project": manifest.project,
        "fileCount": manifest.files.len(),
        "pendingReviewCount": manifest.pending_reviews.iter().filter(|item| item.status != "resolved").count(),
        "taskCount": manifest.tasks.len(),
        "decisionCount": manifest.decisions.len(),
        "messageCount": manifest.messages.len(),
        "recoveryPointCount": manifest.recovery_points.len(),
        "monitoring": manifest.monitoring,
        "atlas": build_atlas_diagnostic(&manifest.atlas),
        "projectAnalysis": build_project_analysis_diagnostic(&manifest.project_analysis),
        "categoryCounts": category_counts,
    })
}

fn build_atlas_diagnostic(atlas: &AtlasAssessment) -> serde_json::Value {
    json!({
        "status": atlas.status,
        "atlasVersion": atlas.atlas_version,
        "reviewRequired": atlas.review_required,
        "failureReason": atlas.failure_reason,
    })
}

fn build_project_analysis_diagnostic(analysis: &ProjectAnalysis) -> serde_json::Value {
    json!({
        "status": analysis.status,
        "updatedAt": analysis.updated_at,
        "modelId": analysis.model_id,
        "goalCount": analysis.goals.len(),
        "questionCount": analysis.questions.len(),
        "nextStepCount": analysis.next_steps.len(),
        "failureReason": analysis.failure_reason,
    })
}

fn count_files_recursive(root: &Path) -> Result<usize, String> {
    let mut count = 0_usize;
    for entry in fs::read_dir(root)
        .map_err(|err| format!("读取目录失败：{}；{err}", path_to_string(root)))?
    {
        let entry =
            entry.map_err(|err| format!("遍历目录失败：{}；{err}", path_to_string(root)))?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|err| format!("读取文件类型失败：{}；{err}", path_to_string(&path)))?;
        if file_type.is_dir() {
            count += count_files_recursive(&path)?;
        } else if file_type.is_file() {
            count += 1;
        }
    }
    Ok(count)
}

fn same_path(left: &Path, right: &Path) -> bool {
    path_to_string(left)
        .trim_end_matches(['\\', '/'])
        .eq_ignore_ascii_case(path_to_string(right).trim_end_matches(['\\', '/']))
}

const PRIVACY_NOTICE: &str = r#"# Ganmaoyuan Privacy Notice

- 感冒院 v0.1 仅面向本机项目使用，不做云同步、在线更新或公开远程仓库写回。
- DeepSeek API Key 保存在 Windows Credential Manager，不写入源码、项目 JSON、备份、导出或诊断日志。
- 项目级授权范围版本 2 会立即发送项目名称/说明/下一步、最多 24 份已登记资料的名称/类型/分类/解析摘要/章节、最多 18 条近期消息（每条最多 1,000 字）和当前只读 Atlas 状态/摘要，用于生成项目理解；不含资料正文、完整聊天历史或本地绝对路径。版本 1 授权需要重新确认。
- 后续聊天发送当前问题（最多 8,000 字）和按问题筛选的当前项目记录，包括最多 3 条相关历史消息、已核验决定、最多 5 条 Work Ledger 事件、最多 4 条任务、最多 3 项成果和最多 3 条待确认记录；只有问题涉及项目背景/目标时才附项目说明，只有问题涉及当前工作/下一步时才附已确认下一步及相关状态；提问涉及 Git 或理由时可附当前 Git 摘要或匹配的 Keep the Why 片段。命中资料时最多发送 3 份登记资料的摘要及每份最多 1,800 字的正文片段。最多 4 张本轮图片（单张不超过 8 MB、总计不超过 20 MB）仅发送给支持视觉的模型；不会附带之前对话的图片。
- Inbox AI 匹配只处理最多 3 个已分别授权的候选项目；发送待处理资料的名称/类型/分类/截断摘要/章节，以及候选项目的说明、项目理解、限量资料/任务/决定摘要和匹配理由；不发送原文件或本地来源路径。
- DeepSeek API Key 保存在 Windows Credential Manager，仅作为请求授权头使用，不进入提示内容、备份或诊断日志；非成功响应正文不会回显到错误消息。
- 本机备份只包含每个已登记项目的 .ganmaoyuan 应用数据目录、项目根目录内逐条登记且可安全读取的受管文件，以及应用数据目录内登记的全局 general/temporary 受管资料和操作记录；不会遍历项目根目录中的源码仓库、.git、node_modules、构建产物、临时依赖或未登记文件。项目分析正文缓存和搜索索引可在恢复后重建；staging、调试日志、外部用户工作区中的 workspaceCleanup 文件、项目根外的登记原始文件和 API Key 不包含在备份中，并在适用时列入排除警告。旧版备份不含全局受管资料，恢复时保留本机现有 managed 目录。备份未加密；SHA-256 检查文件完整性，但不证明备份来源；只恢复可信来源。
- Atlas 评估保持只读，不会自动修改 D:\Atlas。
"#;

const THIRD_PARTY_NOTICES: &str = r#"# Third Party Notices

本地发布包当前直接依赖的主要第三方组件如下：

- React 18.3.1 — MIT License
- React DOM 18.3.1 — MIT License
- React Router DOM 7.18.2 — MIT License
- Vite 7.x — MIT License
- Vitest 4.x — MIT License
- Tauri 2.x — MIT OR Apache-2.0
- Tauri Plugin Dialog 2.x — MIT OR Apache-2.0
- Tauri Plugin Log 2.x — MIT OR Apache-2.0
- Reqwest 0.13.x — MIT OR Apache-2.0
- Serde / Serde JSON — MIT OR Apache-2.0
- SHA-2 — MIT OR Apache-2.0
- UUID — MIT OR Apache-2.0
- Calamine — MIT License
- Zip — MIT License
- Quick-XML — MIT License
- Pdf-Extract / LoPdf — MIT OR Apache-2.0
- Windows crate — MIT OR Apache-2.0

本清单用于本地产品化说明，不包含每一个传递依赖的完整版权文本。若后续进入正式分发阶段，建议在发布流程中补充自动化 license 审计与归档。
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        models::{
            DeepSeekAuthorization, GlobalManagedFile, MonitoringState, PendingReviewItem,
            ProjectSummary, TaskRecord, WorkspaceDraft,
        },
        storage::test_root,
    };

    #[test]
    fn backup_rejects_output_inside_project_and_linked_destination_without_side_effects() {
        let root = test_root("release-backup-output-boundary");
        let global_dir = root.join("global");
        let project_root = root.join("project");
        let outside = root.join("outside");
        fs::create_dir_all(&global_dir).unwrap();
        fs::create_dir_all(&project_root).unwrap();
        fs::create_dir_all(&outside).unwrap();

        let nested_output = project_root.join("backups/nested");
        let error = create_local_backup_from_paths(
            &global_dir,
            vec![path_to_string(&project_root)],
            &nested_output,
            "test-version",
        )
        .unwrap_err();
        assert!(error.contains("项目目录内部"));
        assert!(!nested_output.exists());

        #[cfg(windows)]
        {
            let linked_output = root.join("linked-output");
            if std::os::windows::fs::symlink_dir(&outside, &linked_output).is_ok() {
                let error = create_local_backup_from_paths(
                    &global_dir,
                    Vec::new(),
                    &linked_output.join("backups"),
                    "test-version",
                )
                .unwrap_err();
                assert!(error.contains("链接或重解析点"));
                assert!(!outside.join("backups").exists());
                fs::remove_dir(linked_output).unwrap();
            }
        }

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn restore_rejects_overlap_and_invalid_manifest_before_creating_destination() {
        let root = test_root("release-restore-preflight");
        let global_dir = root.join("global");
        let backup_parent = root.join("backups");
        let restore_parent = root.join("restore-target");
        fs::create_dir_all(&global_dir).unwrap();
        let backup =
            create_local_backup_from_paths(&global_dir, Vec::new(), &backup_parent, "test-version")
                .unwrap();

        let overlap = global_dir.join("nested-restore");
        assert!(restore_local_backup_from_paths(
            &global_dir,
            Path::new(&backup.backup_dir),
            &overlap,
            &global_dir.join("projects.json"),
            true,
        )
        .is_err());
        assert!(!overlap.exists());

        fs::write(
            Path::new(&backup.backup_dir).join(BACKUP_MANIFEST_FILE),
            "not a valid backup manifest",
        )
        .unwrap();
        assert!(restore_local_backup_from_paths(
            &global_dir,
            Path::new(&backup.backup_dir),
            &restore_parent,
            &global_dir.join("projects.json"),
            true,
        )
        .is_err());
        assert!(!restore_parent.exists());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn unique_output_directories_are_exclusive_and_reject_linked_parents() {
        let root = test_root("release-unique-output");
        fs::create_dir_all(&root).unwrap();

        let first = unique_child_dir(&root, "backup").unwrap();
        let marker = first.join("keep.txt");
        fs::write(&marker, "pre-existing output").unwrap();
        let second = unique_child_dir(&root, "backup").unwrap();
        assert_ne!(first, second);
        assert!(path_is_within(&first, &root.canonicalize().unwrap()));
        assert!(path_is_within(&second, &root.canonicalize().unwrap()));
        assert_eq!(fs::read_to_string(marker).unwrap(), "pre-existing output");
        assert!(!second.join("keep.txt").exists());
        assert!(unique_child_dir(&root, "../outside").is_err());

        let outside = root.join("outside");
        let linked_parent = root.join("linked-parent");
        fs::create_dir_all(&outside).unwrap();
        #[cfg(windows)]
        let link_result = std::os::windows::fs::symlink_dir(&outside, &linked_parent);
        #[cfg(unix)]
        let link_result = std::os::unix::fs::symlink(&outside, &linked_parent);
        #[cfg(any(windows, unix))]
        if link_result.is_ok() {
            assert!(unique_child_dir(&linked_parent, "must-not-escape").is_err());
            assert_eq!(fs::read_dir(&outside).unwrap().count(), 0);
        }

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn backup_selection_is_limited_to_registered_project_roots() {
        let root = test_root("release-backup-scope");
        let registered_root = root.join("registered");
        let unregistered_root = root.join("unregistered");
        fs::create_dir_all(&registered_root).unwrap();
        fs::create_dir_all(&unregistered_root).unwrap();
        let registry = AppRegistry {
            projects: vec![ProjectSummary {
                id: "registered-id".to_string(),
                root_dir: path_to_string(&registered_root),
                ..ProjectSummary::default()
            }],
            ..AppRegistry::default()
        };
        let canonical_registered = path_to_string(&registered_root.canonicalize().unwrap());
        assert_eq!(
            resolve_project_roots(&registry, vec![path_to_string(&registered_root)]).unwrap(),
            vec![canonical_registered.clone()]
        );
        assert_eq!(
            resolve_project_roots(&registry, Vec::new()).unwrap(),
            vec![canonical_registered]
        );
        assert!(
            resolve_project_roots(&registry, vec![path_to_string(&unregistered_root)]).is_err()
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn backup_uses_registered_project_data_not_source_tree_and_restores_isolated_projects() {
        let root = test_root("release-backup-managed-scope");
        let global_dir = root.join("global");
        let backup_parent = root.join("backups");
        let restore_parent = root.join("restored");
        let project_a_root = root.join("project-a");
        let project_b_root = root.join("project-b");
        let outside = root.join("dependency-target");
        fs::create_dir_all(&global_dir).unwrap();
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("must-not-copy.txt"), "outside dependency").unwrap();

        let a_first = project_a_root.join(".ganmaoyuan/managed/01/shared.txt");
        let a_duplicate = project_a_root.join(".ganmaoyuan/managed/02/shared.txt");
        let b_file = project_b_root.join(".ganmaoyuan/managed/01/shared.txt");
        let a_registered_root_file = project_a_root.join("registered/hand-off.txt");
        for (path, contents) in [
            (&a_first, "same content"),
            (&a_duplicate, "same content"),
            (&b_file, "project b only"),
            (&a_registered_root_file, "registered in-root material"),
        ] {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, contents).unwrap();
        }
        fs::write(
            project_a_root.join("registered/unregistered.txt"),
            "unregistered root material",
        )
        .unwrap();

        let a_ledger = project_a_root.join(".ganmaoyuan/work-ledger.json");
        let b_ledger = project_b_root.join(".ganmaoyuan/work-ledger.json");
        fs::create_dir_all(a_ledger.parent().unwrap()).unwrap();
        fs::create_dir_all(b_ledger.parent().unwrap()).unwrap();
        fs::write(&a_ledger, "{\"events\":[{\"id\":\"ledger-a\"}]}").unwrap();
        fs::write(&b_ledger, "{\"events\":[{\"id\":\"ledger-b\"}]}").unwrap();
        let b_result = project_b_root.join(".ganmaoyuan/codex/results/task-b.json");
        fs::create_dir_all(b_result.parent().unwrap()).unwrap();
        fs::write(&b_result, "{\"taskId\":\"task-b\"}").unwrap();
        let external_registered = outside.join("registered-original.txt");
        fs::write(&external_registered, "external original must not be copied").unwrap();

        let a_manifest = synthetic_backup_project(
            &project_a_root,
            "backup-project-a",
            "Synthetic A",
            vec![
                backup_file_record(
                    &project_a_root,
                    "shared.txt",
                    ".ganmaoyuan/managed/01/shared.txt",
                ),
                backup_file_record(
                    &project_a_root,
                    "shared.txt",
                    ".ganmaoyuan/managed/02/shared.txt",
                ),
                backup_file_record(&project_a_root, "hand-off.txt", "registered/hand-off.txt"),
            ],
        );
        let mut external_record = backup_file_record(
            &project_b_root,
            "registered-original.txt",
            "external/registered-original.txt",
        );
        external_record.managed_path = path_to_string(&external_registered);
        let b_manifest = synthetic_backup_project(
            &project_b_root,
            "backup-project-b",
            "Synthetic B",
            vec![
                backup_file_record(
                    &project_b_root,
                    "shared.txt",
                    ".ganmaoyuan/managed/01/shared.txt",
                ),
                backup_file_record(
                    &project_b_root,
                    "missing.txt",
                    ".ganmaoyuan/managed/missing.txt",
                ),
                external_record,
            ],
        );
        assert_eq!(a_manifest.project.id, "backup-project-a");
        assert_eq!(b_manifest.project.id, "backup-project-b");

        fs::create_dir_all(project_a_root.join("src")).unwrap();
        fs::write(project_a_root.join("src/main.rs"), "source must not copy").unwrap();
        fs::create_dir_all(project_a_root.join(".git")).unwrap();
        fs::write(
            project_a_root.join(".git/config"),
            "git internals must not copy",
        )
        .unwrap();
        fs::create_dir_all(project_a_root.join("tmp/training-ppt/node_modules")).unwrap();
        let dependency_link = project_a_root.join("tmp/training-ppt/node_modules/lucide");
        create_test_directory_link(&outside, &dependency_link)
            .expect("isolated fixture must create a node_modules junction/symlink");

        let registry = AppRegistry {
            projects: vec![a_manifest.project.clone(), b_manifest.project.clone()],
            ..AppRegistry::default()
        };
        write_json_atomic(&global_dir.join("projects.json"), &registry).unwrap();
        let registered_roots = resolve_project_roots(&registry, Vec::new()).unwrap();
        let backup = create_local_backup_from_paths(
            &global_dir,
            registered_roots,
            &backup_parent,
            "test-version",
        )
        .expect("source-tree dependency link must not block a data-only backup");

        assert!(backup
            .excluded_entries
            .iter()
            .any(|item| item.contains("node_modules") && item.contains("未登记")));
        assert!(backup
            .excluded_entries
            .iter()
            .any(|item| item.contains("missing.txt") && item.contains("缺失")));
        assert!(backup
            .excluded_entries
            .iter()
            .any(|item| item.contains("registered-original.txt") && item.contains("缺失")));
        let backup_root = PathBuf::from(&backup.backup_dir);
        let bundle: BackupBundleManifest =
            read_json(Path::new(&backup.checksum_manifest_path)).unwrap();
        assert_eq!(bundle.projects.len(), 2);
        assert!(bundle
            .excluded_entries
            .iter()
            .any(|item| item.contains("node_modules")));
        assert!(bundle.projects.iter().any(|project| project
            .excluded_entries
            .iter()
            .any(|item| item.contains("missing.txt"))));
        assert!(!backup_root
            .join("projects/Synthetic-A-backup-project-a/src/main.rs")
            .exists());
        assert!(!backup_root
            .join("projects/Synthetic-A-backup-project-a/.git/config")
            .exists());
        assert!(!backup_root
            .join("projects/Synthetic-A-backup-project-a/tmp/training-ppt/node_modules/lucide")
            .exists());
        assert!(!backup_root
            .join("projects/Synthetic-A-backup-project-a/tmp/training-ppt/node_modules/lucide/must-not-copy.txt")
            .exists());
        assert!(!backup_root
            .join("projects/Synthetic-B-backup-project-b/external/registered-original.txt")
            .exists());
        assert_eq!(
            fs::read_to_string(
                backup_root.join("projects/Synthetic-A-backup-project-a/registered/hand-off.txt")
            )
            .unwrap(),
            "registered in-root material"
        );
        assert!(!backup_root
            .join("projects/Synthetic-A-backup-project-a/registered/unregistered.txt")
            .exists());
        assert_eq!(
            fs::read_to_string(
                backup_root.join(
                    "projects/Synthetic-A-backup-project-a/.ganmaoyuan/managed/01/shared.txt"
                )
            )
            .unwrap(),
            "same content"
        );
        assert_eq!(
            fs::read_to_string(
                backup_root.join(
                    "projects/Synthetic-A-backup-project-a/.ganmaoyuan/managed/02/shared.txt"
                )
            )
            .unwrap(),
            "same content"
        );
        assert_eq!(
            fs::read_to_string(
                backup_root.join(
                    "projects/Synthetic-B-backup-project-b/.ganmaoyuan/managed/01/shared.txt"
                )
            )
            .unwrap(),
            "project b only"
        );

        let restored = restore_local_backup_from_paths(
            &global_dir,
            &backup_root,
            &restore_parent,
            &global_dir.join("projects.json"),
            true,
        )
        .unwrap();
        assert!(restored
            .warnings
            .iter()
            .any(|warning| warning.contains("node_modules")));
        assert!(restored
            .warnings
            .iter()
            .any(|warning| warning.contains("missing.txt")));
        let restored_a = restored
            .restored_projects
            .iter()
            .find(|project| project.id == "backup-project-a")
            .unwrap();
        let restored_b = restored
            .restored_projects
            .iter()
            .find(|project| project.id == "backup-project-b")
            .unwrap();
        let restored_a_root = PathBuf::from(&restored_a.root_dir);
        let restored_b_root = PathBuf::from(&restored_b.root_dir);
        assert_eq!(
            fs::read_to_string(restored_a_root.join(".ganmaoyuan/work-ledger.json")).unwrap(),
            "{\"events\":[{\"id\":\"ledger-a\"}]}"
        );
        assert_eq!(
            fs::read_to_string(restored_b_root.join(".ganmaoyuan/work-ledger.json")).unwrap(),
            "{\"events\":[{\"id\":\"ledger-b\"}]}"
        );
        assert!(restored_b_root
            .join(".ganmaoyuan/codex/results/task-b.json")
            .exists());
        assert!(!restored_a_root
            .join(".ganmaoyuan/codex/results/task-b.json")
            .exists());
        assert!(!restored_a_root.join("src/main.rs").exists());
        assert_eq!(
            fs::read_to_string(restored_a_root.join("registered/hand-off.txt")).unwrap(),
            "registered in-root material"
        );
        assert!(!restored_a_root.join("registered/unregistered.txt").exists());
        assert_eq!(
            fs::read_to_string(restored_a_root.join(".ganmaoyuan/managed/02/shared.txt")).unwrap(),
            "same content"
        );

        fs::remove_dir(&dependency_link).unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn backup_still_rejects_links_inside_managed_project_data() {
        let root = test_root("release-backup-managed-link");
        let global_dir = root.join("global");
        let backup_parent = root.join("backups");
        let project_root = root.join("project");
        let outside = root.join("outside-managed");
        fs::create_dir_all(&global_dir).unwrap();
        fs::create_dir_all(&outside).unwrap();
        let manifest = synthetic_backup_project(
            &project_root,
            "managed-link-project",
            "Managed Link",
            Vec::new(),
        );
        let link = project_root.join(".ganmaoyuan/managed/link-to-outside");
        create_test_directory_link(&outside, &link).unwrap();
        let error = create_local_backup_from_paths(
            &global_dir,
            vec![path_to_string(&project_root)],
            &backup_parent,
            "test-version",
        )
        .unwrap_err();
        assert!(error.contains("符号链接") || error.contains("重解析点"));
        assert!(fs::read_dir(&backup_parent).unwrap().next().is_none());
        assert_eq!(manifest.project.id, "managed-link-project");
        fs::remove_dir(&link).unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    fn synthetic_backup_project(
        root: &Path,
        id: &str,
        name: &str,
        files: Vec<FileRecord>,
    ) -> ProjectManifest {
        fs::create_dir_all(root.join(".ganmaoyuan/managed")).unwrap();
        let manifest = ProjectManifest {
            schema_version: crate::models::MANIFEST_SCHEMA_VERSION,
            project: ProjectSummary {
                id: id.to_string(),
                name: name.to_string(),
                root_dir: path_to_string(root),
                manifest_path: path_to_string(&project_service::manifest_path(root)),
                ..ProjectSummary::default()
            },
            files,
            ..ProjectManifest::default()
        };
        project_service::persist_project(root, &manifest, None).unwrap();
        project_service::read_and_repair_manifest(root).unwrap()
    }

    fn backup_file_record(root: &Path, name: &str, relative: &str) -> FileRecord {
        FileRecord {
            id: new_id(),
            file_name: name.to_string(),
            managed_path: path_to_string(&root.join(relative)),
            managed_relative_path: relative.to_string(),
            file_type: "txt".to_string(),
            still_exists: true,
            ..FileRecord::default()
        }
    }

    fn create_test_directory_link(target: &Path, link: &Path) -> std::io::Result<()> {
        #[cfg(windows)]
        {
            let quote = |path: &Path| path.to_string_lossy().replace('\'', "''");
            let script = format!(
                "New-Item -ItemType Junction -Path '{}' -Target '{}' -ErrorAction Stop | Out-Null",
                quote(link),
                quote(target)
            );
            let output = std::process::Command::new("powershell.exe")
                .args(["-NoProfile", "-NonInteractive", "-Command"])
                .arg(script)
                .output()?;
            if output.status.success() {
                Ok(())
            } else {
                Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    String::from_utf8_lossy(&output.stderr).into_owned(),
                ))
            }
        }
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(target, link)
        }
        #[cfg(not(any(windows, unix)))]
        {
            let _ = (target, link);
            Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "directory links are unsupported on this test platform",
            ))
        }
    }

    #[test]
    fn backup_excludes_sensitive_analysis_content_and_restores_project() {
        let root = test_root("release-backup");
        let global_dir = root.join("global");
        let backup_parent = root.join("backups");
        let restore_parent = root.join("restored");
        let project_root = root.join("project");
        fs::create_dir_all(&global_dir).unwrap();
        fs::create_dir_all(&project_root.join(".ganmaoyuan/analysis/content")).unwrap();
        fs::create_dir_all(&project_root.join(".ganmaoyuan/managed/02_requirements")).unwrap();

        let managed = project_root.join(".ganmaoyuan/managed/02_requirements/demo.txt");
        fs::write(&managed, "managed").unwrap();
        let extracted_cache = project_root.join(".ganmaoyuan/analysis/content/raw.txt");
        fs::write(&extracted_cache, "stale cache-only secret").unwrap();
        let global_managed = global_dir.join("managed/general/engineering/vave/reference.txt");
        fs::create_dir_all(global_managed.parent().unwrap()).unwrap();
        fs::write(&global_managed, "global managed reference").unwrap();
        fs::create_dir_all(global_dir.join("managed/audit")).unwrap();
        fs::write(
            global_dir.join("managed/audit/operations.jsonl"),
            "{\"operationId\":\"global-audit-1\"}\n",
        )
        .unwrap();
        write_json_atomic(
            &global_dir.join("managed/files.json"),
            &GlobalFilesDocument {
                schema_version: 1,
                files: vec![
                    GlobalManagedFile {
                        id: "global-file-1".to_string(),
                        file_name: "reference.txt".to_string(),
                        managed_path: path_to_string(&global_managed),
                        managed_relative_path: "engineering/vave/reference.txt".to_string(),
                        destination: "general".to_string(),
                        content_hash: hash_file(&global_managed).unwrap(),
                        lifecycle_status: "managed".to_string(),
                        ..GlobalManagedFile::default()
                    },
                    GlobalManagedFile {
                        id: "external-workspace-file".to_string(),
                        file_name: "external.txt".to_string(),
                        managed_path: path_to_string(&root.join("outside/external.txt")),
                        managed_relative_path: "outside/external.txt".to_string(),
                        destination: "workspaceCleanup".to_string(),
                        lifecycle_status: "managed".to_string(),
                        ..GlobalManagedFile::default()
                    },
                ],
                updated_at: now_string(),
            },
        )
        .unwrap();
        let codex_evidence = project_root.join(".ganmaoyuan/codex/evidence/result.json");
        fs::create_dir_all(codex_evidence.parent().unwrap()).unwrap();
        fs::write(&codex_evidence, "isolated evidence snapshot").unwrap();
        let core_records = [
            (
                ".ganmaoyuan/work-ledger.json",
                "{\"events\":[{\"id\":\"ledger-event\"}] }",
            ),
            (
                ".ganmaoyuan/codex/runs.json",
                "{\"runs\":[{\"id\":\"run-1\"}] }",
            ),
            (".ganmaoyuan/codex/tasks/task-1.json", "{\"id\":\"task-1\"}"),
            (
                ".ganmaoyuan/codex/results/task-1-result.json",
                "{\"taskId\":\"task-1\"}",
            ),
            (
                ".ganmaoyuan/workspace/events.jsonl",
                "{\"id\":\"event-1\"}\n",
            ),
            (
                ".ganmaoyuan/audit/operations.jsonl",
                "{\"id\":\"audit-1\"}\n",
            ),
        ];
        for (relative, contents) in core_records {
            let path = project_root.join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, contents).unwrap();
        }
        fs::write(
            project_root.join(".ganmaoyuan/analysis/content/raw.txt"),
            "secret raw content",
        )
        .unwrap();
        let global_memo = MemoItem {
            id: "global-memo-1".to_string(),
            text: "global record snapshot".to_string(),
            created_at: now_string(),
            updated_at: now_string(),
        };
        write_json_atomic(&global_dir.join("memos.json"), &vec![global_memo]).unwrap();
        write_json_atomic(
            &global_dir.join("projects.json"),
            &AppRegistry {
                schema_version: crate::models::MANIFEST_SCHEMA_VERSION,
                projects: vec![
                    ProjectSummary {
                        id: "project-1".to_string(),
                        name: "demo".to_string(),
                        root_dir: path_to_string(&project_root),
                        manifest_path: path_to_string(&project_service::manifest_path(
                            &project_root,
                        )),
                        last_opened_at: now_string(),
                        next_step: String::new(),
                        created_at: now_string(),
                        description: String::new(),
                        repository_path: String::new(),
                    },
                    ProjectSummary {
                        id: "other-existing".to_string(),
                        name: "other project".to_string(),
                        root_dir: path_to_string(&root.join("other-project")),
                        manifest_path: path_to_string(&root.join("other-project/manifest.json")),
                        last_opened_at: now_string(),
                        created_at: now_string(),
                        ..ProjectSummary::default()
                    },
                ],
                continue_preferences: Vec::new(),
            },
        )
        .unwrap();

        let manifest = ProjectManifest {
            schema_version: crate::models::MANIFEST_SCHEMA_VERSION,
            project: ProjectSummary {
                id: "project-1".to_string(),
                name: "demo".to_string(),
                root_dir: path_to_string(&project_root),
                manifest_path: path_to_string(&project_service::manifest_path(&project_root)),
                last_opened_at: now_string(),
                next_step: String::new(),
                created_at: now_string(),
                description: String::new(),
                repository_path: String::new(),
            },
            files: vec![FileRecord {
                id: "file-1".to_string(),
                file_name: "demo.txt".to_string(),
                file_type: "txt".to_string(),
                parse_status: "success".to_string(),
                content_hash: hash_file(&managed).unwrap(),
                managed_path: path_to_string(&managed),
                managed_relative_path: ".ganmaoyuan/managed/02_requirements/demo.txt".to_string(),
                extracted_text_path: path_to_string(&extracted_cache),
                ..FileRecord::default()
            }],
            messages: vec![WorkspaceMessage {
                id: "msg-1".to_string(),
                author: "user".to_string(),
                kind: "requirement".to_string(),
                text: "very sensitive text".to_string(),
                created_at: now_string(),
                ..WorkspaceMessage::default()
            }],
            tasks: vec![TaskRecord {
                id: "task-1".to_string(),
                title: "task".to_string(),
                status: "active".to_string(),
                source_message_id: "msg-1".to_string(),
                created_at: now_string(),
                updated_at: now_string(),
            }],
            decisions: vec![crate::models::DecisionRecord {
                id: "decision-1".to_string(),
                summary: "keep verified task-linked evidence".to_string(),
                source_message_id: "msg-1".to_string(),
                created_at: now_string(),
            }],
            artifacts: Vec::new(),
            codex_prompts: Vec::new(),
            codex_reports: vec![crate::models::CodexReportRecord {
                id: "report-snapshot".to_string(),
                evidence_managed_path: path_to_string(&codex_evidence),
                ..crate::models::CodexReportRecord::default()
            }],
            daily_sessions: Vec::new(),
            location_decisions: Vec::new(),
            pending_reviews: vec![PendingReviewItem::default()],
            monitoring: MonitoringState::default(),
            atlas: AtlasAssessment::default(),
            project_analysis: ProjectAnalysis::default(),
            audit: Vec::new(),
            draft: WorkspaceDraft::default(),
            recovery_points: Vec::new(),
            deepseek_authorization: DeepSeekAuthorization::default(),
            ..ProjectManifest::default()
        };
        project_service::persist_project(&project_root, &manifest, None).unwrap();

        let result = create_local_backup_from_paths(
            &global_dir,
            vec![path_to_string(&project_root)],
            &backup_parent,
            "0.1.1",
        )
        .unwrap();
        assert!(restore_local_backup_from_paths(
            &global_dir,
            Path::new(&result.backup_dir),
            &restore_parent,
            &global_dir.join("projects.json"),
            false,
        )
        .is_err());
        assert!(!restore_parent.exists());
        assert!(!PathBuf::from(&result.backup_dir)
            .join("projects/demo-project-1/.ganmaoyuan/analysis/content/raw.txt")
            .exists());
        assert!(!PathBuf::from(&result.backup_dir)
            .join("global/managed/workspaceCleanup/outside/external.txt")
            .exists());
        assert_eq!(
            fs::read_to_string(
                PathBuf::from(&result.backup_dir)
                    .join("global/managed/general/engineering/vave/reference.txt")
            )
            .unwrap(),
            "global managed reference"
        );
        assert!(PathBuf::from(&result.backup_dir)
            .join("global/managed/audit/operations.jsonl")
            .exists());
        for (relative, contents) in core_records {
            assert_eq!(
                fs::read_to_string(
                    PathBuf::from(&result.backup_dir)
                        .join("projects/demo-project-1")
                        .join(relative)
                )
                .unwrap(),
                contents
            );
        }

        let restore = restore_local_backup_from_paths(
            &global_dir,
            Path::new(&result.backup_dir),
            &restore_parent,
            &global_dir.join("projects.json"),
            true,
        )
        .unwrap();
        assert!(restore
            .warnings
            .iter()
            .any(|warning| warning.contains("无法认证备份来源")));
        assert!(restore
            .warnings
            .iter()
            .any(|warning| warning.contains("workspaceCleanup")));
        assert_eq!(restore.restored_projects.len(), 1);
        assert!(PathBuf::from(&restore.restored_projects[0].root_dir)
            .join(".ganmaoyuan/managed/02_requirements/demo.txt")
            .exists());
        let restored_root = PathBuf::from(&restore.restored_projects[0].root_dir);
        let restored_manifest = project_service::read_and_repair_manifest(&restored_root).unwrap();
        let restored_message = restored_manifest
            .messages
            .iter()
            .find(|message| message.id == "msg-1")
            .unwrap();
        assert_eq!(restored_message.text, "very sensitive text");
        assert_eq!(restored_manifest.decisions[0].source_message_id, "msg-1");
        assert_eq!(restored_manifest.codex_reports[0].id, "report-snapshot");
        let restored_cache = PathBuf::from(&restored_manifest.files[0].extracted_text_path);
        assert!(restored_cache.starts_with(&restored_root));
        assert_eq!(fs::read_to_string(restored_cache).unwrap(), "managed");
        let evidence = PathBuf::from(&restored_manifest.codex_reports[0].evidence_managed_path);
        assert!(evidence.starts_with(&restored_root));
        assert_eq!(
            fs::read_to_string(evidence).unwrap(),
            "isolated evidence snapshot"
        );
        assert_eq!(restored_manifest.decisions[0].id, "decision-1");
        for (relative, contents) in core_records {
            assert_eq!(
                fs::read_to_string(restored_root.join(relative)).unwrap(),
                contents
            );
        }
        let restored_registry: AppRegistry = read_json(&global_dir.join("projects.json")).unwrap();
        assert!(restored_registry
            .projects
            .iter()
            .any(|item| item.id == "other-existing"));
        assert_eq!(
            restored_registry
                .projects
                .iter()
                .find(|item| item.id == "project-1")
                .unwrap()
                .root_dir,
            path_to_string(&restored_root)
        );
        let restored_global: GlobalFilesDocument =
            read_json(&global_dir.join("managed/files.json")).unwrap();
        let restored_global_file = restored_global
            .files
            .iter()
            .find(|item| item.id == "global-file-1")
            .unwrap();
        assert!(Path::new(&restored_global_file.managed_path).is_file());
        assert_eq!(
            fs::read_to_string(&restored_global_file.managed_path).unwrap(),
            "global managed reference"
        );
        let restored_memos: Vec<MemoItem> = read_json(&global_dir.join("memos.json")).unwrap();
        assert_eq!(restored_memos[0].id, "global-memo-1");
        assert_eq!(restored_memos[0].text, "global record snapshot");
        assert_eq!(
            fs::read_to_string(&managed).unwrap(),
            "managed",
            "restoring into an isolated destination must not modify the source project"
        );

        let backup_root = PathBuf::from(&result.backup_dir);
        let bundle_path = backup_root.join(BACKUP_MANIFEST_FILE);
        let original_bundle = fs::read(&bundle_path).unwrap();
        for invalid_relative_dir in ["../escape", r"C:\\outside"] {
            let mut value: serde_json::Value = serde_json::from_slice(&original_bundle).unwrap();
            value["projects"][0]["backupRelativeDir"] = json!(invalid_relative_dir);
            fs::write(&bundle_path, serde_json::to_vec(&value).unwrap()).unwrap();
            assert!(restore_local_backup_from_paths(
                &global_dir,
                &backup_root,
                &restore_parent,
                &global_dir.join("projects.json"),
                true,
            )
            .is_err());
            assert!(!root.join("escape").exists());
        }
        fs::write(&bundle_path, &original_bundle).unwrap();
        let outside_global_marker = root.join("outside-global-marker.json");
        fs::write(&outside_global_marker, "must stay untouched").unwrap();
        let mut malicious_global: serde_json::Value =
            serde_json::from_slice(&fs::read(&bundle_path).unwrap()).unwrap();
        malicious_global["globalFiles"][0]["relativePath"] =
            json!(path_to_string(&outside_global_marker));
        fs::write(&bundle_path, serde_json::to_vec(&malicious_global).unwrap()).unwrap();
        assert!(restore_local_backup_from_paths(
            &global_dir,
            &backup_root,
            &restore_parent,
            &global_dir.join("projects.json"),
            true,
        )
        .is_err());
        assert_eq!(
            fs::read_to_string(&outside_global_marker).unwrap(),
            "must stay untouched"
        );
        fs::write(&bundle_path, &original_bundle).unwrap();
        let managed_backup = backup_root
            .join("projects/demo-project-1/.ganmaoyuan/managed/02_requirements/demo.txt");
        fs::write(&managed_backup, "tampered backup content").unwrap();
        assert!(restore_local_backup_from_paths(
            &global_dir,
            &backup_root,
            &restore_parent,
            &global_dir.join("projects.json"),
            true,
        )
        .is_err());
        assert_eq!(
            read_json::<AppRegistry>(&global_dir.join("projects.json"))
                .unwrap()
                .projects
                .len(),
            2
        );
        fs::write(&managed_backup, "managed").unwrap();
        let external_temp = root.join("outside-backup-root.txt");
        fs::write(&external_temp, "outside data").unwrap();
        let linked_backup_file = backup_root.join("projects/demo-project-1/unlisted-link.txt");
        #[cfg(windows)]
        if std::os::windows::fs::symlink_file(&external_temp, &linked_backup_file).is_ok() {
            assert!(validate_backup_bundle(&backup_root).is_err());
            fs::remove_file(&linked_backup_file).unwrap();
        }
        let linked_backup_dir = backup_root.join("projects/demo-project-1/unlisted-linked-dir");
        #[cfg(windows)]
        let link_dir_result = std::os::windows::fs::symlink_dir(&root, &linked_backup_dir);
        #[cfg(unix)]
        let link_dir_result = std::os::unix::fs::symlink(&root, &linked_backup_dir);
        #[cfg(any(windows, unix))]
        if link_dir_result.is_ok() {
            assert!(validate_backup_bundle(&backup_root).is_err());
            fs::remove_dir(&linked_backup_dir).unwrap();
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn legacy_backup_preserves_existing_global_managed_files() {
        let root = test_root("release-legacy-backup-managed");
        let global_dir = root.join("global");
        let backup_parent = root.join("backups");
        let restore_parent = root.join("restored");
        let managed_file = global_dir.join("managed/general/template.txt");
        fs::create_dir_all(managed_file.parent().unwrap()).unwrap();
        fs::write(&managed_file, "keep-current-global-managed-data").unwrap();
        write_json_atomic(
            &global_dir.join("managed/files.json"),
            &GlobalFilesDocument {
                schema_version: 1,
                files: vec![GlobalManagedFile {
                    id: "current-managed-file".to_string(),
                    file_name: "template.txt".to_string(),
                    managed_path: path_to_string(&managed_file),
                    managed_relative_path: "template.txt".to_string(),
                    destination: "general".to_string(),
                    content_hash: hash_file(&managed_file).unwrap(),
                    lifecycle_status: "managed".to_string(),
                    ..GlobalManagedFile::default()
                }],
                updated_at: now_string(),
            },
        )
        .unwrap();

        let backup =
            create_local_backup_from_paths(&global_dir, Vec::new(), &backup_parent, "test-version")
                .unwrap();
        let backup_root = PathBuf::from(&backup.backup_dir);
        let bundle_path = backup_root.join(BACKUP_MANIFEST_FILE);
        let mut bundle: serde_json::Value =
            serde_json::from_slice(&fs::read(&bundle_path).unwrap()).unwrap();
        bundle["schemaVersion"] = json!(2);
        let global_files = bundle["globalFiles"].as_array_mut().unwrap();
        global_files.retain(|entry| {
            !entry["relativePath"]
                .as_str()
                .unwrap_or_default()
                .replace('\\', "/")
                .starts_with("global/managed/")
        });
        fs::write(&bundle_path, serde_json::to_vec(&bundle).unwrap()).unwrap();
        fs::remove_dir_all(backup_root.join("global/managed")).unwrap();

        let restored = restore_local_backup_from_paths(
            &global_dir,
            &backup_root,
            &restore_parent,
            &global_dir.join("projects.json"),
            true,
        )
        .unwrap();

        assert!(restored
            .warnings
            .iter()
            .any(|warning| warning.contains("保留本机现有 managed 目录")));
        assert_eq!(
            fs::read_to_string(&managed_file).unwrap(),
            "keep-current-global-managed-data"
        );
        let registry: GlobalFilesDocument =
            read_json(&global_dir.join("managed/files.json")).unwrap();
        assert_eq!(registry.files[0].id, "current-managed-file");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn backup_refuses_stale_global_registry_fallback_without_partial_bundle() {
        let root = test_root("release-backup-stale-managed-registry");
        let global_dir = root.join("global");
        let backup_parent = root.join("backups");
        fs::create_dir_all(global_dir.join("managed")).unwrap();
        write_json_atomic(
            &global_dir.join("managed/files.json"),
            &GlobalFilesDocument::default(),
        )
        .unwrap();
        write_json_atomic(
            &global_dir.join("managed/files.json"),
            &GlobalFilesDocument {
                updated_at: now_string(),
                ..GlobalFilesDocument::default()
            },
        )
        .unwrap();
        fs::write(global_dir.join("managed/files.json"), b"{damaged").unwrap();

        let error =
            create_local_backup_from_paths(&global_dir, Vec::new(), &backup_parent, "test-version")
                .unwrap_err();

        assert!(error.contains("旧 .bak"));
        assert_eq!(
            fs::read(global_dir.join("managed/files.json")).unwrap(),
            b"{damaged"
        );
        assert_eq!(fs::read_dir(&backup_parent).unwrap().count(), 0);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn failed_restore_after_global_managed_swap_rolls_back_previous_directory() {
        let root = test_root("release-managed-restore-rollback");
        let global_dir = root.join("global");
        let backup_parent = root.join("backups");
        let restore_parent = root.join("restored");
        let managed_file = global_dir.join("managed/general/template.txt");
        fs::create_dir_all(managed_file.parent().unwrap()).unwrap();
        fs::write(&managed_file, "backup-version").unwrap();
        write_json_atomic(
            &global_dir.join("managed/files.json"),
            &GlobalFilesDocument {
                schema_version: 1,
                files: vec![GlobalManagedFile {
                    id: "current-managed-file".to_string(),
                    file_name: "template.txt".to_string(),
                    managed_path: path_to_string(&managed_file),
                    managed_relative_path: "template.txt".to_string(),
                    destination: "general".to_string(),
                    content_hash: hash_file(&managed_file).unwrap(),
                    lifecycle_status: "managed".to_string(),
                    ..GlobalManagedFile::default()
                }],
                updated_at: now_string(),
            },
        )
        .unwrap();
        write_json_atomic(&global_dir.join("projects.json"), &AppRegistry::default()).unwrap();
        let backup =
            create_local_backup_from_paths(&global_dir, Vec::new(), &backup_parent, "test-version")
                .unwrap();

        fs::write(&managed_file, "current-version-to-preserve").unwrap();
        let mut current_registry: GlobalFilesDocument =
            read_json(&global_dir.join("managed/files.json")).unwrap();
        current_registry.files[0].content_hash = hash_file(&managed_file).unwrap();
        write_json_atomic(&global_dir.join("managed/files.json"), &current_registry).unwrap();
        fs::create_dir(global_dir.join("projects.json.bak")).unwrap();

        assert!(restore_local_backup_from_paths(
            &global_dir,
            Path::new(&backup.backup_dir),
            &restore_parent,
            &global_dir.join("projects.json"),
            true,
        )
        .is_err());
        assert_eq!(
            fs::read_to_string(&managed_file).unwrap(),
            "current-version-to-preserve"
        );
        let preserved_registry: GlobalFilesDocument =
            read_json(&global_dir.join("managed/files.json")).unwrap();
        assert_eq!(
            preserved_registry.files[0].content_hash,
            hash_file(&managed_file).unwrap()
        );
        let preserved_projects: AppRegistry = read_json(&global_dir.join("projects.json")).unwrap();
        assert!(preserved_projects.projects.is_empty());
        assert!(preserved_projects.continue_preferences.is_empty());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn backup_restore_rolls_back_prior_global_writes_on_later_failure() {
        let root = test_root("release-backup-rollback");
        let global_dir = root.join("global");
        let backup_parent = root.join("backups");
        let restore_parent = root.join("restored");
        fs::create_dir_all(&global_dir).unwrap();
        write_json_atomic(
            &global_dir.join("memos.json"),
            &vec![MemoItem {
                id: "backup-memo".to_string(),
                text: "from backup".to_string(),
                created_at: "created".to_string(),
                updated_at: "updated".to_string(),
            }],
        )
        .unwrap();
        write_json_atomic(
            &global_dir.join("material-inbox.json"),
            &MaterialInboxDocument::default(),
        )
        .unwrap();

        let backup =
            create_local_backup_from_paths(&global_dir, Vec::new(), &backup_parent, "test-version")
                .unwrap();

        let current_memos = vec![MemoItem {
            id: "current-memo".to_string(),
            text: "must survive failed restore".to_string(),
            created_at: "current-created".to_string(),
            updated_at: "current-updated".to_string(),
        }];
        write_json_atomic(&global_dir.join("memos.json"), &current_memos).unwrap();
        fs::remove_file(global_dir.join("material-inbox.json")).unwrap();
        fs::create_dir(global_dir.join("material-inbox.json")).unwrap();

        let error = restore_local_backup_from_paths(
            &global_dir,
            Path::new(&backup.backup_dir),
            &restore_parent,
            &global_dir.join("projects.json"),
            true,
        )
        .unwrap_err();
        assert!(error.contains("恢复目标不是普通文件"));
        let after_failure: Vec<MemoItem> = read_json(&global_dir.join("memos.json")).unwrap();
        assert_eq!(after_failure, current_memos);
        assert_eq!(fs::read_dir(&restore_parent).unwrap().count(), 0);

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn safe_export_redacts_conversation_and_omits_source_paths() {
        let root = test_root("release-export");
        let project_root = root.join("project");
        fs::create_dir_all(project_root.join(".ganmaoyuan/managed/03_delivery")).unwrap();

        let managed = project_root.join(".ganmaoyuan/managed/03_delivery/result.txt");
        fs::write(&managed, "delivery result").unwrap();

        let manifest = ProjectManifest {
            schema_version: crate::models::MANIFEST_SCHEMA_VERSION,
            project: ProjectSummary {
                id: "project-2".to_string(),
                name: "secure export".to_string(),
                root_dir: path_to_string(&project_root),
                manifest_path: path_to_string(&project_service::manifest_path(&project_root)),
                last_opened_at: now_string(),
                next_step: "整理交付物".to_string(),
                created_at: now_string(),
                description: "导出测试".to_string(),
                repository_path: String::new(),
            },
            files: vec![FileRecord {
                id: "file-2".to_string(),
                file_name: "result.txt".to_string(),
                original_source_path: r"D:\Sensitive\original.txt".to_string(),
                managed_path: path_to_string(&managed),
                managed_relative_path: ".ganmaoyuan/managed/03_delivery/result.txt".to_string(),
                file_type: "txt".to_string(),
                category: "delivery".to_string(),
                recommended_category: "delivery".to_string(),
                content_summary: "包含敏感交付说明".to_string(),
                ..FileRecord::default()
            }],
            messages: vec![WorkspaceMessage {
                id: "msg-2".to_string(),
                author: "assistant".to_string(),
                kind: "analysis".to_string(),
                text: "这里有非常敏感的完整正文，需要在安全导出中脱敏。".to_string(),
                created_at: now_string(),
                status: "completed".to_string(),
                ..WorkspaceMessage::default()
            }],
            project_analysis: ProjectAnalysis {
                status: "ready".to_string(),
                project_definition: "安全导出测试".to_string(),
                ..ProjectAnalysis::default()
            },
            ..ProjectManifest::default()
        };
        project_service::persist_project(&project_root, &manifest, None).unwrap();

        let export_result = export_project_safe_to(
            &path_to_string(&project_root),
            &path_to_string(&root),
            &root,
        )
        .unwrap();
        let export_dir = PathBuf::from(export_result.export_dir);
        let conversations: serde_json::Value =
            read_json(&export_dir.join("conversations.json")).unwrap();
        let file_index: serde_json::Value = read_json(&export_dir.join("file-index.json")).unwrap();

        assert_eq!(conversations[0]["textRedacted"], true);
        assert_ne!(
            conversations[0]["textPreview"].as_str().unwrap_or_default(),
            "这里有非常敏感的完整正文，需要在安全导出中脱敏。"
        );
        assert!(file_index[0].get("originalSourcePath").is_none());
        assert!(export_dir
            .join("materials/.ganmaoyuan/managed/03_delivery/result.txt")
            .exists());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rewrite_manifest_root_paths_updates_nested_absolute_paths() {
        let old_root = PathBuf::from(r"D:\GanMaoYuan\Tests\old-project");
        let new_root = PathBuf::from(r"D:\GanMaoYuan\Tests\new-project");
        let mut manifest = ProjectManifest {
            project: ProjectSummary {
                id: "project-3".to_string(),
                name: "migrate".to_string(),
                root_dir: path_to_string(&old_root),
                manifest_path: path_to_string(&project_service::manifest_path(&old_root)),
                ..ProjectSummary::default()
            },
            files: vec![FileRecord {
                id: "file-3".to_string(),
                file_name: "demo.txt".to_string(),
                managed_path: path_to_string(
                    &old_root.join(".ganmaoyuan/managed/02_requirements/demo.txt"),
                ),
                extracted_text_path: path_to_string(
                    &old_root.join(".ganmaoyuan/analysis/content/demo.txt"),
                ),
                ..FileRecord::default()
            }],
            messages: vec![WorkspaceMessage {
                attachments: vec![crate::models::MessageAttachment {
                    file_id: "file-3".to_string(),
                    file_name: "demo.txt".to_string(),
                    managed_path: path_to_string(
                        &old_root.join(".ganmaoyuan/managed/02_requirements/demo.txt"),
                    ),
                    ..crate::models::MessageAttachment::default()
                }],
                ..WorkspaceMessage::default()
            }],
            artifacts: vec![crate::models::ArtifactRecord {
                id: "artifact-1".to_string(),
                title: "artifact".to_string(),
                artifact_type: "doc".to_string(),
                source: "system".to_string(),
                source_message_id: "msg".to_string(),
                file_id: "file-3".to_string(),
                managed_path: path_to_string(
                    &old_root.join(".ganmaoyuan/managed/03_delivery/demo.txt"),
                ),
                related_task: "task".to_string(),
                created_at: now_string(),
            }],
            location_decisions: vec![crate::models::LocationDecision {
                id: "location-1".to_string(),
                file_name: "demo.txt".to_string(),
                source_path: path_to_string(&old_root.join("source/demo.txt")),
                managed_path: path_to_string(
                    &old_root.join(".ganmaoyuan/managed/02_requirements/demo.txt"),
                ),
                managed_relative_path: ".ganmaoyuan/managed/02_requirements/demo.txt".to_string(),
                category: "requirements".to_string(),
                file_type: "txt".to_string(),
                created_source: "import".to_string(),
                related_task: "task".to_string(),
                purpose: "analysis".to_string(),
                version: "v1".to_string(),
                reason: "rule".to_string(),
                requires_confirmation: false,
                created_at: now_string(),
                operation_id: String::new(),
                location_preview_id: String::new(),
            }],
            ..ProjectManifest::default()
        };

        rewrite_manifest_root_paths(&mut manifest, &new_root);

        assert_eq!(manifest.project.root_dir, path_to_string(&new_root));
        assert!(manifest
            .project
            .manifest_path
            .starts_with(&path_to_string(&new_root)));
        assert!(manifest.files[0]
            .managed_path
            .starts_with(&path_to_string(&new_root)));
        assert!(manifest.files[0]
            .extracted_text_path
            .starts_with(&path_to_string(&new_root)));
        assert!(manifest.messages[0].attachments[0]
            .managed_path
            .starts_with(&path_to_string(&new_root)));
        assert!(manifest.artifacts[0]
            .managed_path
            .starts_with(&path_to_string(&new_root)));
        assert!(manifest.location_decisions[0]
            .managed_path
            .starts_with(&path_to_string(&new_root)));
    }
}
