use crate::{
    models::{
        AppRegistry, AtlasAssessment, BackupRestoreResult, DeepSeekSettings, FileRecord,
        LocalBackupResult, PrivacyArtifactsResult, ProjectAnalysis, ProjectManifest,
        ProjectMigrationResult, SafeExportResult, WorkspaceMessage,
    },
    project_service,
    storage::{
        canonical_project_root, global_data_dir, hash_file, now_string, path_to_string, read_json,
        write_json_atomic, write_text_atomic,
    },
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    collections::{BTreeMap, HashSet},
    fs,
    path::{Path, PathBuf},
};
use tauri::{AppHandle, Runtime};

const BACKUP_SCHEMA_VERSION: u32 = 1;
const BACKUP_MANIFEST_FILE: &str = "backup-bundle.json";
const MIGRATION_RECORD_FILE: &str = ".ganmaoyuan/root-migration.json";
const DEEPSEEK_SETTINGS_FILE: &str = "deepseek-settings.json";

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
struct BackupBundleManifest {
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
    let registry = project_service::read_registry(app)?;
    let roots = resolve_project_roots(&registry, project_roots);
    let destination_parent = if destination_dir.trim().is_empty() {
        default_release_directory(&global_dir, "Backups")
    } else {
        canonical_project_root(Path::new(&destination_dir))?
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
) -> Result<BackupRestoreResult, String> {
    let global_dir = global_data_dir(app)?;
    let backup_root = PathBuf::from(&backup_dir);
    let restore_base = if restore_projects_base_dir.trim().is_empty() {
        canonical_project_root(&default_release_directory(&global_dir, "RestoredProjects"))?
    } else {
        canonical_project_root(Path::new(&restore_projects_base_dir))?
    };
    restore_local_backup_from_paths(
        &global_dir,
        &backup_root,
        &restore_base,
        &project_service::registry_path(app)?,
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
    app: &AppHandle<R>,
    project_root: String,
    destination_dir: String,
) -> Result<SafeExportResult, String> {
    let global_dir = global_data_dir(app)?;
    export_project_safe_with_default(
        project_root,
        destination_dir,
        &default_release_directory(&global_dir, "Exports"),
    )
}

fn export_project_safe_with_default(
    project_root: String,
    destination_dir: String,
    default_destination_dir: &Path,
) -> Result<SafeExportResult, String> {
    let root = PathBuf::from(&project_root);
    let manifest = project_service::read_and_repair_manifest(&root)?;
    let destination_parent = if destination_dir.trim().is_empty() {
        canonical_project_root(default_destination_dir)?
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
        canonical_project_root(&default_release_directory(&global_dir, "Diagnostics"))?
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

fn resolve_project_roots(registry: &AppRegistry, requested: Vec<String>) -> Vec<String> {
    if requested.is_empty() {
        return registry
            .projects
            .iter()
            .map(|item| item.root_dir.clone())
            .collect();
    }
    requested
}

fn create_local_backup_from_paths(
    global_dir: &Path,
    project_roots: Vec<String>,
    destination_parent: &Path,
    app_version: &str,
) -> Result<LocalBackupResult, String> {
    let backup_dir = unique_child_dir(destination_parent, "ganmaoyuan-backup")?;
    let mut manifest = BackupBundleManifest {
        schema_version: BACKUP_SCHEMA_VERSION,
        app_version: app_version.to_string(),
        created_at: now_string(),
        global_files: Vec::new(),
        projects: Vec::new(),
        excluded_entries: vec![
            "global/open-folder-debug.jsonl".to_string(),
            "project/.ganmaoyuan/analysis/content/**".to_string(),
            "project/.ganmaoyuan/workspace/events.jsonl".to_string(),
            "project/.ganmaoyuan/audit/operations.jsonl".to_string(),
            "project/.ganmaoyuan/staging/**".to_string(),
        ],
    };

    let global_backup_root = backup_dir.join("global");
    fs::create_dir_all(&global_backup_root).map_err(|err| {
        format!(
            "创建全局备份目录失败：{}；{err}",
            path_to_string(&global_backup_root)
        )
    })?;
    for name in [
        "projects.json",
        "memos.json",
        "material-inbox.json",
        "inbox-routing-settings.json",
        DEEPSEEK_SETTINGS_FILE,
    ] {
        let source = global_dir.join(name);
        if !source.exists() {
            continue;
        }
        let destination = global_backup_root.join(name);
        copy_file_verified(&source, &destination)?;
        manifest
            .global_files
            .push(build_backup_file_entry(&backup_dir, &destination)?);
    }

    for root_value in project_roots {
        let root = PathBuf::from(&root_value);
        let project = project_service::read_and_repair_manifest(&root)?;
        let project_backup_relative = PathBuf::from("projects").join(format!(
            "{}-{}",
            sanitize_slug(&project.project.name),
            project.project.id
        ));
        let project_backup_root = backup_dir.join(&project_backup_relative);
        let excluded = copy_project_for_backup(&root, &project_backup_root)?;
        manifest.projects.push(ProjectBackupEntry {
            project_id: project.project.id.clone(),
            project_name: project.project.name.clone(),
            original_root_dir: path_to_string(&root),
            backup_relative_dir: path_to_string(&project_backup_relative),
            file_count: project.files.len(),
            excluded_entries: excluded,
        });
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
        excluded_entries: manifest.excluded_entries,
        checksum_manifest_path: path_to_string(&checksum_manifest_path),
        created_at: manifest.created_at,
    })
}

fn restore_local_backup_from_paths(
    global_dir: &Path,
    backup_root: &Path,
    restore_base: &Path,
    registry_path: &Path,
) -> Result<BackupRestoreResult, String> {
    let manifest: BackupBundleManifest = read_json(&backup_root.join(BACKUP_MANIFEST_FILE))?;
    let mut restored_global_files = Vec::new();
    for item in &manifest.global_files {
        let source = backup_root.join(&item.relative_path);
        let destination = global_dir.join(
            Path::new(&item.relative_path)
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("data.json"),
        );
        copy_file_verified(&source, &destination)?;
        restored_global_files.push(path_to_string(&destination));
    }

    let mut registry: AppRegistry = if registry_path.exists() {
        read_json(registry_path)?
    } else {
        AppRegistry {
            schema_version: crate::models::MANIFEST_SCHEMA_VERSION,
            projects: Vec::new(),
            continue_preferences: Vec::new(),
        }
    };
    registry.schema_version = crate::models::MANIFEST_SCHEMA_VERSION;
    let mut restored_projects = Vec::new();

    for project in &manifest.projects {
        let source_root = backup_root.join(&project.backup_relative_dir);
        let target_root = unique_child_dir(restore_base, &sanitize_slug(&project.project_name))?;
        copy_dir_recursive(&source_root, &target_root)?;
        let mut restored_manifest = project_service::read_and_repair_manifest(&target_root)?;
        rewrite_manifest_root_paths(&mut restored_manifest, &target_root);
        project_service::persist_project(&target_root, &restored_manifest, None)?;
        registry
            .projects
            .retain(|item| item.id != restored_manifest.project.id);
        registry
            .projects
            .insert(0, restored_manifest.project.clone());
        restored_projects.push(restored_manifest.project);
    }
    write_json_atomic(registry_path, &registry)?;

    Ok(BackupRestoreResult {
        backup_dir: path_to_string(backup_root),
        restored_global_dir: path_to_string(global_dir),
        restored_projects,
        restored_global_files,
        warnings: vec!["未恢复 API Key 与 Windows Credential Manager 凭据。".to_string()],
        restored_at: now_string(),
    })
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

fn copy_project_for_backup(source: &Path, destination: &Path) -> Result<Vec<String>, String> {
    let mut excluded = Vec::new();
    copy_dir_recursive_filtered(source, destination, source, &mut |relative, path| {
        let relative_string = path_to_string(relative);
        let deny = relative_string.starts_with(".ganmaoyuan\\analysis\\content")
            || relative_string.starts_with(".ganmaoyuan/analysis/content")
            || relative_string.starts_with(".ganmaoyuan\\audit")
            || relative_string.starts_with(".ganmaoyuan/audit")
            || relative_string.eq_ignore_ascii_case(".ganmaoyuan\\workspace\\events.jsonl")
            || relative_string.eq_ignore_ascii_case(".ganmaoyuan/workspace/events.jsonl")
            || relative_string.starts_with(".ganmaoyuan\\staging")
            || relative_string.starts_with(".ganmaoyuan/staging");
        if deny {
            excluded.push(path_to_string(path));
        }
        !deny
    })?;
    Ok(excluded)
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
        let destination_path = destination.join(
            path.file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("item"),
        );
        let file_type = entry
            .file_type()
            .map_err(|err| format!("读取文件类型失败：{}；{err}", path_to_string(&path)))?;
        if file_type.is_dir() {
            copy_dir_recursive_filtered(&path, &destination_path, root, include)?;
        } else if file_type.is_file() {
            copy_file_verified(&path, &destination_path)?;
        }
    }
    Ok(())
}

fn copy_file_verified(source: &Path, destination: &Path) -> Result<(), String> {
    if let Some(parent) = destination.parent() {
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
    fs::create_dir_all(parent)
        .map_err(|err| format!("创建目录失败：{}；{err}", path_to_string(parent)))?;
    let candidate = parent.join(format!("{prefix}-{}", now_string()));
    fs::create_dir_all(&candidate)
        .map_err(|err| format!("创建目录失败：{}；{err}", path_to_string(&candidate)))?;
    Ok(candidate)
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

pub(crate) fn default_release_directory(global_dir: &Path, name: &str) -> PathBuf {
    let legacy_data_dir = Path::new(r"D:\GanMaoYuan\AppData");
    if same_path(global_dir, legacy_data_dir) {
        return global_dir
            .parent()
            .map(|parent| parent.join(name))
            .unwrap_or_else(|| global_dir.join(name));
    }
    global_dir.join(name)
}

const PRIVACY_NOTICE: &str = r#"# Ganmaoyuan Privacy Notice

- 感冒院 v0.1 仅面向本机项目使用，不做云同步、在线更新或公开远程仓库写回。
- DeepSeek API Key 保存在 Windows Credential Manager，不写入源码、项目 JSON、备份、导出或诊断日志。
- 发送到 DeepSeek 的内容仅限项目说明、项目理解、文件摘要、关键历史和当前问题，不上传原始文件。
- 安全备份、项目导出和诊断日志默认排除原始文件正文提取内容、调试事件日志和凭据。
- 外部资料评估保持只读，不会自动修改外部目录。
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
            DeepSeekAuthorization, MonitoringState, PendingReviewItem, ProjectSummary, TaskRecord,
            WorkspaceDraft,
        },
        storage::test_root,
    };

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
        let codex_evidence = project_root.join(".ganmaoyuan/codex/evidence/result.json");
        fs::create_dir_all(codex_evidence.parent().unwrap()).unwrap();
        fs::write(&codex_evidence, "isolated evidence snapshot").unwrap();
        fs::write(
            project_root.join(".ganmaoyuan/analysis/content/raw.txt"),
            "secret raw content",
        )
        .unwrap();
        write_json_atomic(&global_dir.join("memos.json"), &vec!["memo".to_string()]).unwrap();
        write_json_atomic(
            &global_dir.join("projects.json"),
            &AppRegistry {
                schema_version: crate::models::MANIFEST_SCHEMA_VERSION,
                projects: vec![ProjectSummary {
                    id: "project-1".to_string(),
                    name: "demo".to_string(),
                    root_dir: path_to_string(&project_root),
                    manifest_path: path_to_string(&project_service::manifest_path(&project_root)),
                    last_opened_at: now_string(),
                    next_step: String::new(),
                    created_at: now_string(),
                    description: String::new(),
                    repository_path: String::new(),
                }],
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
                managed_path: path_to_string(&managed),
                managed_relative_path: ".ganmaoyuan/managed/02_requirements/demo.txt".to_string(),
                extracted_text_path: path_to_string(
                    &project_root.join(".ganmaoyuan/analysis/content/raw.txt"),
                ),
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
            decisions: Vec::new(),
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
        assert!(!PathBuf::from(&result.backup_dir)
            .join("projects/demo-project-1/.ganmaoyuan/analysis/content/raw.txt")
            .exists());

        let restore = restore_local_backup_from_paths(
            &global_dir,
            Path::new(&result.backup_dir),
            &restore_parent,
            &global_dir.join("projects.json"),
        )
        .unwrap();
        assert_eq!(restore.restored_projects.len(), 1);
        assert!(PathBuf::from(&restore.restored_projects[0].root_dir)
            .join(".ganmaoyuan/managed/02_requirements/demo.txt")
            .exists());
        let restored_root = PathBuf::from(&restore.restored_projects[0].root_dir);
        let restored_manifest = project_service::read_and_repair_manifest(&restored_root).unwrap();
        let evidence = PathBuf::from(&restored_manifest.codex_reports[0].evidence_managed_path);
        assert!(evidence.starts_with(&restored_root));
        assert_eq!(
            fs::read_to_string(evidence).unwrap(),
            "isolated evidence snapshot"
        );
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

        let default_export_root = root.join("app-data/Exports");
        let export_result = export_project_safe_with_default(
            path_to_string(&project_root),
            String::new(),
            &default_export_root,
        )
        .unwrap();
        let export_dir = PathBuf::from(export_result.export_dir);
        assert!(export_dir.starts_with(&default_export_root));
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
    fn release_directories_follow_app_data_except_for_legacy_data_location() {
        let app_data = PathBuf::from(r"C:\Users\test\AppData\Roaming\com.ganmaoyuan.desktop");
        assert_eq!(
            default_release_directory(&app_data, "Exports"),
            app_data.join("Exports")
        );

        let legacy_app_data = PathBuf::from(r"D:\GanMaoYuan\AppData");
        assert_eq!(
            default_release_directory(&legacy_app_data, "Exports"),
            PathBuf::from(r"D:\GanMaoYuan\Exports")
        );
    }

    #[test]
    fn rewrite_manifest_root_paths_updates_nested_absolute_paths() {
        let old_root = PathBuf::from(r"D:\Example\Tests\old-project");
        let new_root = PathBuf::from(r"D:\Example\Tests\new-project");
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
