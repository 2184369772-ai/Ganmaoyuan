use crate::{
    deepseek, launch_inbox,
    models::{
        BackupRestoreResult, CleanupExecutionBatch, CleanupPlan, CleanupPlanReviewFilter,
        CodexPromptResult, CodexReportApplyResult, CodexReportImportResult,
        CodexResultBridgeScanResult, CodexRun, CodexTask, CodexTaskList, DailyContinueSnapshot,
        DeepSeekConnectionResult, DeepSeekModelInfo, DeepSeekSettings, ExecutionRecord,
        FileProjection, FinishWorkResult, GeneratedFileResult, GitSnapshot, GlobalManagedFile,
        GlobalSearchResult, ImportResult, InboxRoutingSettings, LocalBackupResult,
        MaterialInboxItem, MaterialInboxRouteResult, MemoItem, MessageResult,
        PrivacyArtifactsResult, ProjectActionCandidate, ProjectAttention, ProjectContextPacket,
        ProjectCreateResult, ProjectFactCaptureRequest, ProjectImpactAnalysis, ProjectManifest,
        ProjectMigrationResult, ProjectStateProposal, ProjectSummary, SafeExportResult,
        TodayWorkspace, WeeklyReportExportResult, WeeklyReportRecord, WeeklyReviewDashboard,
        WeeklyReviewSettings, WorkEvent, WorkLedgerSnapshot, WorkspaceConfig, WorkspaceDraft,
        WorkspaceScanBatch,
    },
    project_service, release_service,
    storage::{append_json_line, path_to_string},
    weekly_review,
};
#[cfg(target_os = "windows")]
use std::os::windows::{ffi::OsStrExt, process::CommandExt};
use std::{
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
    process::Command,
};
use tauri::Runtime;
#[cfg(target_os = "windows")]
use windows::{
    core::{HRESULT, PCWSTR},
    Win32::{
        System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED},
        UI::Shell::{Common::ITEMIDLIST, ILCreateFromPathW, ILFree, SHOpenFolderAndSelectItems},
    },
};

#[tauri::command]
pub fn list_projects<R: Runtime>(app: tauri::AppHandle<R>) -> Result<Vec<ProjectSummary>, String> {
    project_service::list_projects(&app)
}

#[tauri::command]
pub fn list_file_projections<R: Runtime>(
    app: tauri::AppHandle<R>,
) -> Result<Vec<FileProjection>, String> {
    project_service::list_file_projections(&app)
}

#[tauri::command]
pub fn get_workspace_config<R: Runtime>(
    app: tauri::AppHandle<R>,
) -> Result<WorkspaceConfig, String> {
    project_service::get_workspace_config(&app)
}

#[tauri::command]
pub fn initialize_workspace_root<R: Runtime>(
    app: tauri::AppHandle<R>,
    root: Option<String>,
) -> Result<WorkspaceConfig, String> {
    project_service::initialize_workspace_root(&app, root)
}

#[tauri::command]
pub fn verify_workspace_root<R: Runtime>(
    app: tauri::AppHandle<R>,
) -> Result<WorkspaceConfig, String> {
    project_service::verify_workspace_root(&app)
}

#[tauri::command]
pub fn list_workspace_scan_batches<R: Runtime>(
    app: tauri::AppHandle<R>,
) -> Result<Vec<WorkspaceScanBatch>, String> {
    project_service::list_workspace_scan_batches(&app)
}

#[tauri::command]
pub fn scan_workspace_directory<R: Runtime>(
    app: tauri::AppHandle<R>,
    source_directory: String,
) -> Result<WorkspaceScanBatch, String> {
    project_service::scan_workspace_directory(&app, source_directory)
}

#[tauri::command]
pub fn scan_desktop_directory<R: Runtime>(
    app: tauri::AppHandle<R>,
) -> Result<WorkspaceScanBatch, String> {
    project_service::scan_desktop_directory(&app)
}

#[tauri::command]
pub fn scan_downloads_directory<R: Runtime>(
    app: tauri::AppHandle<R>,
) -> Result<WorkspaceScanBatch, String> {
    project_service::scan_downloads_directory(&app)
}

#[tauri::command]
pub fn generate_cleanup_plan<R: Runtime>(
    app: tauri::AppHandle<R>,
    scan_batch_id: String,
) -> Result<CleanupPlan, String> {
    project_service::generate_cleanup_plan(&app, &scan_batch_id)
}

#[tauri::command]
pub fn list_cleanup_plans<R: Runtime>(
    app: tauri::AppHandle<R>,
) -> Result<Vec<CleanupPlan>, String> {
    project_service::list_cleanup_plans(&app)
}

#[tauri::command]
pub fn get_cleanup_plan<R: Runtime>(
    app: tauri::AppHandle<R>,
    plan_id: String,
) -> Result<CleanupPlan, String> {
    project_service::get_cleanup_plan(&app, &plan_id)
}

#[tauri::command]
pub fn review_cleanup_plan_items<R: Runtime>(
    app: tauri::AppHandle<R>,
    plan_id: String,
    item_ids: Vec<String>,
    review_status: String,
    user_reason: Option<String>,
) -> Result<CleanupPlan, String> {
    project_service::review_cleanup_plan_items(
        &app,
        &plan_id,
        item_ids,
        &review_status,
        user_reason,
    )
}

#[tauri::command]
pub fn bulk_review_cleanup_plan<R: Runtime>(
    app: tauri::AppHandle<R>,
    plan_id: String,
    filter: CleanupPlanReviewFilter,
    review_status: String,
    user_reason: Option<String>,
) -> Result<CleanupPlan, String> {
    project_service::bulk_review_cleanup_plan(&app, &plan_id, filter, &review_status, user_reason)
}

#[tauri::command]
pub fn modify_cleanup_plan_item<R: Runtime>(
    app: tauri::AppHandle<R>,
    plan_id: String,
    item_id: String,
    final_project: Option<String>,
    final_category: Option<String>,
    final_target_path: String,
    user_reason: Option<String>,
) -> Result<CleanupPlan, String> {
    project_service::modify_cleanup_plan_item(
        &app,
        &plan_id,
        &item_id,
        final_project,
        final_category,
        final_target_path,
        user_reason,
    )
}

#[tauri::command]
pub fn reset_cleanup_plan_items<R: Runtime>(
    app: tauri::AppHandle<R>,
    plan_id: String,
    item_ids: Vec<String>,
) -> Result<CleanupPlan, String> {
    project_service::reset_cleanup_plan_items(&app, &plan_id, item_ids)
}

#[tauri::command]
pub fn execute_cleanup_plan<R: Runtime>(
    app: tauri::AppHandle<R>,
    plan_id: String,
) -> Result<CleanupExecutionBatch, String> {
    project_service::execute_cleanup_plan(&app, &plan_id)
}

#[tauri::command]
pub fn list_cleanup_execution_batches<R: Runtime>(
    app: tauri::AppHandle<R>,
) -> Result<Vec<CleanupExecutionBatch>, String> {
    project_service::list_cleanup_execution_batches(&app)
}

#[tauri::command]
pub fn list_execution_records<R: Runtime>(
    app: tauri::AppHandle<R>,
    project_root: Option<String>,
) -> Result<Vec<ExecutionRecord>, String> {
    project_service::list_execution_records(&app, project_root)
}

#[tauri::command]
pub fn undo_cleanup_execution_batch<R: Runtime>(
    app: tauri::AppHandle<R>,
    batch_id: String,
) -> Result<CleanupExecutionBatch, String> {
    project_service::undo_cleanup_execution_batch(&app, &batch_id)
}

#[tauri::command]
pub fn load_project(project_root: String) -> Result<ProjectManifest, String> {
    project_service::load_project(&project_root)
}

#[tauri::command]
pub async fn create_project<R: Runtime>(
    app: tauri::AppHandle<R>,
    name: String,
    root_dir: String,
    file_paths: Vec<String>,
    description: String,
) -> Result<ProjectCreateResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        project_service::create_project(&app, name, root_dir, file_paths, description)
    })
    .await
    .map_err(|err| format!("项目创建任务异常结束：{err}"))?
}

#[tauri::command]
pub async fn import_files(
    project_root: String,
    file_paths: Vec<String>,
    related_task: String,
) -> Result<ImportResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        project_service::import_files(project_root, file_paths, related_task)
    })
    .await
    .map_err(|err| format!("资料导入任务异常结束：{err}"))?
}

#[tauri::command]
pub async fn send_project_message<R: Runtime>(
    app: tauri::AppHandle<R>,
    project_root: String,
    text: String,
) -> Result<MessageResult, String> {
    let initial = project_service::prepare_project_message(&app, project_root.clone(), text)?;
    let message_id = initial.stream_message_id.clone();
    let model_id = initial
        .messages
        .iter()
        .find(|item| item.id == message_id)
        .map(|item| item.model_id.clone())
        .unwrap_or_default();
    tauri::async_runtime::spawn(async move {
        let result = project_service::run_project_message_stream(
            app.clone(),
            project_root.clone(),
            message_id.clone(),
            model_id.clone(),
        )
        .await;
        if let Err(error) = result {
            let _ = project_service::mark_project_message_failed(
                &app,
                &project_root,
                &message_id,
                &model_id,
                &error,
            );
            let _ = deepseek::emit_stream_event(
                &app,
                crate::models::DeepSeekStreamEvent {
                    project_root,
                    message_id,
                    status: "error".to_string(),
                    delta: String::new(),
                    text: String::new(),
                    error,
                    model_id,
                },
            );
        }
    });
    Ok(initial)
}

#[tauri::command]
pub fn stop_project_message(project_root: String) -> Result<(), String> {
    deepseek::stop_stream(&project_root);
    Ok(())
}

#[tauri::command]
pub fn save_project_draft(
    project_root: String,
    text: String,
    pending_file_paths: Vec<String>,
) -> Result<WorkspaceDraft, String> {
    project_service::save_project_draft(project_root, text, pending_file_paths)
}

#[tauri::command]
pub fn finish_project_work(
    project_root: String,
    completed: String,
    next_step: String,
) -> Result<FinishWorkResult, String> {
    project_service::finish_project_work(project_root, completed, next_step)
}

#[tauri::command]
pub fn generate_codex_prompt(project_root: String) -> Result<CodexPromptResult, String> {
    project_service::generate_codex_prompt(project_root)
}

#[tauri::command]
pub fn list_codex_tasks(project_root: String) -> Result<CodexTaskList, String> {
    project_service::list_codex_tasks(project_root)
}

#[tauri::command]
pub fn rebind_codex_task_prompt(
    project_root: String,
    task_id: String,
) -> Result<CodexTask, String> {
    project_service::rebind_codex_task_prompt(project_root, task_id)
}

#[tauri::command]
pub fn mark_codex_task_handed_off(
    project_root: String,
    task_id: String,
) -> Result<CodexTask, String> {
    project_service::mark_codex_task_handed_off(project_root, task_id)
}

#[tauri::command]
pub fn start_codex_task_run(project_root: String, task_id: String) -> Result<CodexRun, String> {
    project_service::start_codex_task_run(project_root, task_id)
}

#[tauri::command]
pub fn cancel_codex_task_run(project_root: String, task_id: String) -> Result<CodexRun, String> {
    project_service::cancel_codex_task_run(project_root, task_id)
}

#[tauri::command]
pub fn accept_codex_task(
    project_root: String,
    task_id: String,
    expected_result_id: String,
) -> Result<CodexTask, String> {
    project_service::accept_codex_task(project_root, task_id, expected_result_id)
}

#[tauri::command]
pub fn reject_codex_task(
    project_root: String,
    task_id: String,
    expected_result_id: String,
    reason: Option<String>,
) -> Result<CodexTask, String> {
    project_service::reject_codex_task(project_root, task_id, expected_result_id, reason)
}

#[tauri::command]
pub fn import_codex_report_text(
    project_root: String,
    report_text: String,
) -> Result<CodexReportImportResult, String> {
    project_service::import_codex_report_text(project_root, report_text)
}

#[tauri::command]
pub fn import_codex_report_file(
    project_root: String,
    report_file_path: String,
) -> Result<CodexReportImportResult, String> {
    project_service::import_codex_report_file(project_root, report_file_path)
}

#[tauri::command]
pub fn scan_codex_result_bridge(
    project_root: String,
) -> Result<CodexResultBridgeScanResult, String> {
    project_service::scan_codex_result_bridge(project_root)
}

#[tauri::command]
pub fn apply_codex_report(
    project_root: String,
    report_id: String,
) -> Result<CodexReportApplyResult, String> {
    project_service::apply_codex_report(project_root, report_id)
}

#[tauri::command]
pub fn get_work_ledger(project_root: String) -> Result<WorkLedgerSnapshot, String> {
    project_service::get_work_ledger(project_root)
}

#[tauri::command]
pub fn get_project_context_packet(project_root: String) -> Result<ProjectContextPacket, String> {
    project_service::get_project_context_packet(project_root)
}

#[tauri::command]
pub fn refresh_git_snapshot(
    project_root: String,
    repository_path: Option<String>,
) -> Result<GitSnapshot, String> {
    project_service::refresh_git_snapshot(project_root, repository_path)
}

#[tauri::command]
pub fn record_user_decision_event(
    project_root: String,
    decision: String,
    reason: Option<String>,
) -> Result<WorkEvent, String> {
    project_service::record_user_decision_event(project_root, decision, reason)
}

#[tauri::command]
pub fn capture_project_fact(
    project_root: String,
    request: ProjectFactCaptureRequest,
) -> Result<WorkEvent, String> {
    project_service::capture_project_fact(project_root, request)
}

#[tauri::command]
pub fn search_projects<R: Runtime>(
    app: tauri::AppHandle<R>,
    query: String,
) -> Result<Vec<GlobalSearchResult>, String> {
    project_service::search_projects(&app, query)
}

#[tauri::command]
pub fn register_generated_file(
    project_root: String,
    file_name: String,
    content: String,
    created_source: String,
    related_task: String,
    purpose: String,
) -> Result<GeneratedFileResult, String> {
    project_service::register_generated_file(
        project_root,
        file_name,
        content,
        created_source,
        related_task,
        purpose,
    )
}

#[tauri::command]
pub fn load_deepseek_settings<R: Runtime>(
    app: tauri::AppHandle<R>,
) -> Result<DeepSeekSettings, String> {
    deepseek::load_settings(&app)
}

#[tauri::command]
pub fn save_deepseek_api_key<R: Runtime>(
    app: tauri::AppHandle<R>,
    api_key: String,
    selected_model_id: String,
) -> Result<DeepSeekSettings, String> {
    deepseek::save_api_key(&app, api_key, selected_model_id)
}

#[tauri::command]
pub fn delete_deepseek_api_key<R: Runtime>(
    app: tauri::AppHandle<R>,
) -> Result<DeepSeekSettings, String> {
    deepseek::delete_api_key(&app)
}

#[tauri::command]
pub async fn test_deepseek_connection<R: Runtime>(
    app: tauri::AppHandle<R>,
    selected_model_id: String,
) -> Result<DeepSeekConnectionResult, String> {
    deepseek::test_connection(&app, selected_model_id).await
}

#[tauri::command]
pub async fn list_deepseek_models() -> Result<Vec<DeepSeekModelInfo>, String> {
    deepseek::list_models().await
}

#[tauri::command]
pub fn grant_project_deepseek_authorization<R: Runtime>(
    app: tauri::AppHandle<R>,
    project_root: String,
) -> Result<ProjectManifest, String> {
    project_service::grant_project_deepseek_authorization(&app, &project_root)
}

#[tauri::command]
pub async fn refresh_project_understanding<R: Runtime>(
    app: tauri::AppHandle<R>,
    project_root: String,
) -> Result<ProjectManifest, String> {
    project_service::refresh_project_understanding(app, project_root).await
}

#[tauri::command]
pub fn scan_project_workspace(project_root: String) -> Result<ProjectManifest, String> {
    let (manifest, _, _) = project_service::scan_project_workspace(&project_root)?;
    Ok(manifest)
}

#[tauri::command]
pub fn open_file(path: String) -> Result<(), String> {
    let target = PathBuf::from(path);
    if !target.exists() {
        return Err(format!("文件不存在，无法打开：{}", target.display()));
    }
    if !target.is_file() {
        return Err(format!("目标不是文件，无法打开：{}", target.display()));
    }
    open_file_with_default_app(&target)
}

#[tauri::command]
pub fn open_folder(path: String) -> Result<(), String> {
    record_open_folder_debug("frontend_emitted_path", &path, None, None, None);
    let target = PathBuf::from(&path);
    let canonical = match canonicalize_existing_path(&target) {
        Ok(path) => path,
        Err(err) => {
            record_open_folder_debug(
                "reveal_validation_failed",
                &path,
                None,
                Some(path_to_string(&target)),
                Some(&err),
            );
            return Err(err);
        }
    };
    let normalized = path_to_string(&canonical).replace('/', "\\");
    let exists = canonical.exists();
    record_open_folder_debug(
        "rust_received_path",
        &path,
        Some(&normalized),
        Some(path_to_string(&target)),
        Some(if exists {
            "exists=true"
        } else {
            "exists=false"
        }),
    );

    #[cfg(target_os = "windows")]
    {
        if canonical.is_file() {
            return reveal_file_in_explorer(&canonical, &path, &normalized);
        }
    }
    open_path(&canonical)
}

#[tauri::command]
pub fn list_memos<R: Runtime>(app: tauri::AppHandle<R>) -> Result<Vec<MemoItem>, String> {
    project_service::list_memos(&app)
}

#[tauri::command]
pub fn save_memos<R: Runtime>(
    app: tauri::AppHandle<R>,
    memos: Vec<MemoItem>,
) -> Result<Vec<MemoItem>, String> {
    project_service::save_memos(&app, memos)
}

#[tauri::command]
pub fn list_material_inbox<R: Runtime>(
    app: tauri::AppHandle<R>,
) -> Result<Vec<MaterialInboxItem>, String> {
    project_service::list_material_inbox(&app)
}

#[tauri::command]
pub fn list_inbox_entries<R: Runtime>(
    app: tauri::AppHandle<R>,
) -> Result<Vec<MaterialInboxItem>, String> {
    project_service::list_inbox_entries(&app)
}

#[tauri::command]
pub fn ingest_material_inbox_files<R: Runtime>(
    app: tauri::AppHandle<R>,
    file_paths: Vec<String>,
) -> Result<Vec<MaterialInboxItem>, String> {
    project_service::ingest_material_inbox_files(&app, file_paths)
}

#[tauri::command]
pub fn receive_inbox_files<R: Runtime>(
    app: tauri::AppHandle<R>,
    file_paths: Vec<String>,
) -> Result<Vec<MaterialInboxItem>, String> {
    project_service::receive_inbox_files(&app, file_paths)
}

#[tauri::command]
pub fn confirm_material_inbox_item<R: Runtime>(
    app: tauri::AppHandle<R>,
    item_id: String,
    project_root: String,
) -> Result<MaterialInboxRouteResult, String> {
    project_service::confirm_material_inbox_item(&app, item_id, project_root)
}

#[tauri::command]
pub fn confirm_inbox_entry<R: Runtime>(
    app: tauri::AppHandle<R>,
    item_id: String,
    project_root: String,
) -> Result<MaterialInboxRouteResult, String> {
    project_service::confirm_inbox_entry(&app, item_id, project_root)
}

#[tauri::command]
pub async fn refine_inbox_entry_with_ai<R: Runtime>(
    app: tauri::AppHandle<R>,
    item_id: String,
) -> Result<MaterialInboxItem, String> {
    project_service::refine_inbox_entry_with_ai(app, item_id).await
}

#[tauri::command]
pub fn update_inbox_route_decision<R: Runtime>(
    app: tauri::AppHandle<R>,
    item_id: String,
    project_root: String,
    semantic_location: String,
) -> Result<MaterialInboxItem, String> {
    project_service::update_inbox_route_decision(&app, item_id, project_root, semantic_location)
}

#[tauri::command]
pub fn ignore_inbox_entry<R: Runtime>(
    app: tauri::AppHandle<R>,
    item_id: String,
) -> Result<MaterialInboxItem, String> {
    project_service::ignore_inbox_entry(&app, item_id)
}

#[tauri::command]
pub fn retry_inbox_entry<R: Runtime>(
    app: tauri::AppHandle<R>,
    item_id: String,
) -> Result<MaterialInboxItem, String> {
    project_service::retry_inbox_entry(&app, item_id)
}

#[tauri::command]
pub fn reanalyze_inbox_entry<R: Runtime>(
    app: tauri::AppHandle<R>,
    item_id: String,
) -> Result<MaterialInboxItem, String> {
    project_service::reanalyze_inbox_entry(&app, item_id)
}

#[tauri::command]
pub fn undo_inbox_route<R: Runtime>(
    app: tauri::AppHandle<R>,
    item_id: String,
) -> Result<MaterialInboxItem, String> {
    project_service::undo_inbox_route(&app, item_id)
}

#[tauri::command]
pub fn route_inbox_item_to_global<R: Runtime>(
    app: tauri::AppHandle<R>,
    item_id: String,
    destination: String,
) -> Result<GlobalManagedFile, String> {
    project_service::route_inbox_item_to_global(&app, item_id, destination)
}

#[tauri::command]
pub fn undo_global_file<R: Runtime>(
    app: tauri::AppHandle<R>,
    file_id: String,
) -> Result<GlobalManagedFile, String> {
    project_service::undo_global_file(&app, file_id)
}

#[tauri::command]
pub fn list_global_files<R: Runtime>(
    app: tauri::AppHandle<R>,
) -> Result<Vec<GlobalManagedFile>, String> {
    project_service::list_global_files(&app)
}

#[tauri::command]
pub fn load_inbox_routing_settings<R: Runtime>(
    app: tauri::AppHandle<R>,
) -> Result<InboxRoutingSettings, String> {
    project_service::load_inbox_routing_settings(&app)
}

#[tauri::command]
pub fn save_inbox_routing_settings<R: Runtime>(
    app: tauri::AppHandle<R>,
    auto_route_high_confidence: bool,
) -> Result<InboxRoutingSettings, String> {
    project_service::save_inbox_routing_settings(&app, auto_route_high_confidence)
}

#[tauri::command]
pub fn create_project_from_material_inbox<R: Runtime>(
    app: tauri::AppHandle<R>,
    item_id: String,
    project_name: String,
    root_dir: String,
    description: String,
) -> Result<MaterialInboxRouteResult, String> {
    project_service::create_project_from_material_inbox(
        &app,
        item_id,
        project_name,
        root_dir,
        description,
    )
}

#[tauri::command]
pub fn create_project_from_inbox<R: Runtime>(
    app: tauri::AppHandle<R>,
    item_id: String,
    project_name: String,
    root_dir: String,
    description: String,
) -> Result<MaterialInboxRouteResult, String> {
    project_service::create_project_from_inbox(&app, item_id, project_name, root_dir, description)
}

#[tauri::command]
pub fn consume_launch_inbox_entries<R: Runtime>(
    app: tauri::AppHandle<R>,
) -> Result<Vec<MaterialInboxItem>, String> {
    launch_inbox::consume_launch_inbox_entries(&app)
}

#[tauri::command]
pub fn create_local_backup<R: Runtime>(
    app: tauri::AppHandle<R>,
    project_roots: Vec<String>,
    destination_dir: String,
) -> Result<LocalBackupResult, String> {
    release_service::create_local_backup(&app, project_roots, destination_dir)
}

#[tauri::command]
pub fn restore_local_backup<R: Runtime>(
    app: tauri::AppHandle<R>,
    backup_dir: String,
    restore_projects_base_dir: String,
) -> Result<BackupRestoreResult, String> {
    release_service::restore_local_backup(&app, backup_dir, restore_projects_base_dir)
}

#[tauri::command]
pub fn migrate_project_root<R: Runtime>(
    app: tauri::AppHandle<R>,
    project_root: String,
    new_root_dir: String,
) -> Result<ProjectMigrationResult, String> {
    release_service::migrate_project_root(&app, project_root, new_root_dir)
}

#[tauri::command]
pub fn export_project_safe(
    project_root: String,
    destination_dir: String,
) -> Result<SafeExportResult, String> {
    release_service::export_project_safe(project_root, destination_dir)
}

#[tauri::command]
pub fn generate_privacy_artifacts<R: Runtime>(
    app: tauri::AppHandle<R>,
    destination_dir: String,
    project_root: Option<String>,
) -> Result<PrivacyArtifactsResult, String> {
    release_service::generate_privacy_artifacts(&app, destination_dir, project_root)
}

#[tauri::command]
pub fn load_weekly_review_dashboard<R: Runtime>(
    app: tauri::AppHandle<R>,
    project_root: Option<String>,
) -> Result<WeeklyReviewDashboard, String> {
    weekly_review::load_weekly_review_dashboard(&app, project_root)
}

#[tauri::command]
pub fn load_weekly_review_settings<R: Runtime>(
    app: tauri::AppHandle<R>,
) -> Result<WeeklyReviewSettings, String> {
    weekly_review::load_weekly_review_settings(&app)
}

#[tauri::command]
pub fn save_weekly_review_settings<R: Runtime>(
    app: tauri::AppHandle<R>,
    generation_weekday: u8,
) -> Result<WeeklyReviewSettings, String> {
    weekly_review::save_weekly_review_settings(&app, generation_weekday)
}

#[tauri::command]
pub fn generate_weekly_reviews<R: Runtime>(
    app: tauri::AppHandle<R>,
    project_root: Option<String>,
    week_key: String,
    week_start: String,
    week_end: String,
    force: bool,
) -> Result<WeeklyReviewDashboard, String> {
    weekly_review::generate_weekly_reviews(
        &app,
        project_root,
        week_key,
        week_start,
        week_end,
        force,
    )
}

#[tauri::command]
pub fn update_weekly_report_markdown<R: Runtime>(
    app: tauri::AppHandle<R>,
    project_root: String,
    report_id: String,
    markdown: String,
) -> Result<WeeklyReportRecord, String> {
    weekly_review::update_weekly_report_markdown(&app, project_root, report_id, markdown)
}

#[tauri::command]
pub fn confirm_weekly_report<R: Runtime>(
    app: tauri::AppHandle<R>,
    project_root: String,
    report_id: String,
) -> Result<WeeklyReportRecord, String> {
    weekly_review::confirm_weekly_report(&app, project_root, report_id)
}

#[tauri::command]
pub fn export_weekly_report<R: Runtime>(
    app: tauri::AppHandle<R>,
    project_root: String,
    report_id: String,
    destination_dir: String,
) -> Result<WeeklyReportExportResult, String> {
    weekly_review::export_weekly_report(&app, project_root, report_id, destination_dir)
}

#[tauri::command]
pub fn get_impact_analyses<R: Runtime>(
    app: tauri::AppHandle<R>,
    project_root: String,
) -> Result<Vec<ProjectImpactAnalysis>, String> {
    project_service::get_impact_analyses(&app, project_root)
}

#[tauri::command]
pub fn get_action_candidates<R: Runtime>(
    app: tauri::AppHandle<R>,
    project_root: String,
) -> Result<Vec<ProjectActionCandidate>, String> {
    project_service::get_action_candidates(&app, project_root)
}

#[tauri::command]
pub fn get_state_proposals<R: Runtime>(
    app: tauri::AppHandle<R>,
    project_root: String,
) -> Result<Vec<ProjectStateProposal>, String> {
    project_service::get_state_proposals(&app, project_root)
}

#[tauri::command]
pub fn confirm_action_candidate<R: Runtime>(
    app: tauri::AppHandle<R>,
    project_root: String,
    candidate_id: String,
) -> Result<ProjectActionCandidate, String> {
    project_service::confirm_action_candidate(&app, project_root, candidate_id)
}

#[tauri::command]
pub fn ignore_action_candidate<R: Runtime>(
    app: tauri::AppHandle<R>,
    project_root: String,
    candidate_id: String,
) -> Result<ProjectActionCandidate, String> {
    project_service::ignore_action_candidate(&app, project_root, candidate_id)
}

#[tauri::command]
pub fn update_action_candidate<R: Runtime>(
    app: tauri::AppHandle<R>,
    project_root: String,
    candidate_id: String,
    title: String,
    description: String,
    suggested_priority: String,
    suggested_due_date: String,
) -> Result<ProjectActionCandidate, String> {
    project_service::update_action_candidate(
        &app,
        project_root,
        candidate_id,
        title,
        description,
        suggested_priority,
        suggested_due_date,
    )
}

#[tauri::command]
pub fn apply_state_proposal<R: Runtime>(
    app: tauri::AppHandle<R>,
    project_root: String,
    proposal_id: String,
) -> Result<ProjectStateProposal, String> {
    project_service::apply_state_proposal(&app, project_root, proposal_id)
}

#[tauri::command]
pub fn undo_state_proposal<R: Runtime>(
    app: tauri::AppHandle<R>,
    project_root: String,
    proposal_id: String,
) -> Result<ProjectStateProposal, String> {
    project_service::undo_state_proposal(&app, project_root, proposal_id)
}

#[tauri::command]
pub fn get_daily_continue<R: Runtime>(
    app: tauri::AppHandle<R>,
    project_root: String,
) -> Result<DailyContinueSnapshot, String> {
    project_service::get_daily_continue(&app, project_root)
}

#[tauri::command]
pub fn regenerate_daily_continue<R: Runtime>(
    app: tauri::AppHandle<R>,
    project_root: String,
) -> Result<DailyContinueSnapshot, String> {
    project_service::regenerate_daily_continue(&app, project_root)
}

#[tauri::command]
pub fn get_today_workspace<R: Runtime>(app: tauri::AppHandle<R>) -> Result<TodayWorkspace, String> {
    project_service::get_today_workspace(&app)
}

#[tauri::command]
pub fn update_project_attention<R: Runtime>(
    app: tauri::AppHandle<R>,
    project_root: String,
    attention_id: String,
    status: String,
) -> Result<ProjectAttention, String> {
    project_service::update_project_attention(&app, project_root, attention_id, status)
}

#[tauri::command]
pub fn update_data_health_status(
    project_root: String,
    record_id: String,
    status: String,
) -> Result<ProjectManifest, String> {
    project_service::update_data_health_status(project_root, record_id, status)
}

#[tauri::command]
pub fn update_pending_review_status(
    project_root: String,
    review_id: String,
    status: String,
) -> Result<ProjectManifest, String> {
    project_service::update_pending_review_status(project_root, review_id, status)
}

#[tauri::command]
pub fn mark_today_done<R: Runtime>(
    app: tauri::AppHandle<R>,
    project_root: String,
    completed: String,
    next_step: String,
) -> Result<FinishWorkResult, String> {
    project_service::mark_today_done(&app, project_root, completed, next_step)
}

#[tauri::command]
pub fn set_state_auto_apply<R: Runtime>(
    app: tauri::AppHandle<R>,
    project_root: String,
    enabled: bool,
) -> Result<ProjectManifest, String> {
    project_service::set_state_auto_apply(&app, project_root, enabled)
}

fn open_path(path: &Path) -> Result<(), String> {
    Command::new("explorer.exe")
        .arg(path)
        .spawn()
        .map_err(|err| format!("打开路径失败：{err}"))?;
    Ok(())
}

fn canonicalize_existing_path(path: &Path) -> Result<PathBuf, String> {
    if !path.exists() {
        return Err(format!(
            "文件或目录不存在，无法打开所在位置：{}",
            path_to_string(path)
        ));
    }
    path.canonicalize()
        .map_err(|err| format!("规范化所在位置路径失败：{}：{err}", path_to_string(path)))
}

#[cfg(target_os = "windows")]
fn reveal_file_in_explorer(
    path: &Path,
    frontend_path: &str,
    normalized_path: &str,
) -> Result<(), String> {
    let _com_scope = ComScope::initialize()?;
    let item = shell_item_id_list(path)?;
    unsafe { SHOpenFolderAndSelectItems(item, None, 0) }
        .map_err(|err| format!("打开文件所在位置失败：{err}"))?;
    unsafe { ILFree(Some(item.cast())) };
    record_open_folder_debug(
        "shell_reveal_dispatched",
        frontend_path,
        Some(normalized_path),
        Some(path_to_string(path)),
        Some("method=SHOpenFolderAndSelectItems"),
    );
    Ok(())
}

#[cfg(target_os = "windows")]
fn shell_item_id_list(path: &Path) -> Result<*mut ITEMIDLIST, String> {
    let display = path_to_string(path).replace('/', "\\");
    let wide = wide_null(&display);
    let item = unsafe { ILCreateFromPathW(PCWSTR(wide.as_ptr())) };
    if item.is_null() {
        return Err(format!("创建文件 PIDL 失败：{display}"));
    }
    Ok(item)
}

#[cfg(target_os = "windows")]
fn wide_null(value: &str) -> Vec<u16> {
    OsStr::new(value).encode_wide().chain(Some(0)).collect()
}

#[cfg(target_os = "windows")]
struct ComScope {
    should_uninitialize: bool,
}

#[cfg(target_os = "windows")]
impl ComScope {
    fn initialize() -> Result<Self, String> {
        match unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) } {
            result if result.is_ok() => Ok(Self {
                should_uninitialize: true,
            }),
            result if result == HRESULT(0x80010106_u32 as i32) => Ok(Self {
                should_uninitialize: false,
            }),
            err => Err(format!("初始化 Windows Shell 失败：{err}")),
        }
    }
}

#[cfg(target_os = "windows")]
impl Drop for ComScope {
    fn drop(&mut self) {
        if self.should_uninitialize {
            unsafe { CoUninitialize() };
        }
    }
}

fn record_open_folder_debug(
    stage: &str,
    frontend_path: &str,
    normalized_absolute_path: Option<&str>,
    rust_received_path: Option<String>,
    note: Option<&str>,
) {
    let log_path = open_folder_debug_log_path();
    let payload = serde_json::json!({
        "timestamp": crate::storage::now_string(),
        "stage": stage,
        "frontendEmittedPath": frontend_path,
        "rustReceivedPath": rust_received_path.unwrap_or_else(|| frontend_path.to_string()),
        "normalizedAbsolutePath": normalized_absolute_path.unwrap_or(""),
        "exists": normalized_absolute_path.map(PathBuf::from).map(|path| path.exists()).unwrap_or(false),
        "note": note.unwrap_or(""),
    });
    let _ = append_json_line(&log_path, &payload);
}

fn open_folder_debug_log_path() -> PathBuf {
    let root = std::env::var_os("GANMAOYUAN_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"D:\GanMaoYuan\AppData"));
    let _ = fs::create_dir_all(&root);
    root.join("open-folder-debug.jsonl")
}

fn open_file_with_default_app(path: &Path) -> Result<(), String> {
    let mut command = Command::new("cmd.exe");
    command.args(["/C", "start", ""]).arg(path);
    #[cfg(target_os = "windows")]
    command.creation_flags(0x08000000);
    command
        .spawn()
        .map_err(|err| format!("打开文件失败：{err}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::test_root;
    use std::fs;

    #[cfg(target_os = "windows")]
    #[test]
    fn canonicalizes_reveal_target_for_chinese_space_path() {
        let root = test_root("reveal space");
        let managed = root
            .join(".ganmaoyuan")
            .join("managed")
            .join("02_requirements");
        fs::create_dir_all(&managed).unwrap();
        let file = managed.join("需求 文档.txt");
        fs::write(&file, "demo").unwrap();

        let absolute = canonicalize_existing_path(&file).unwrap();
        let normalized = path_to_string(&absolute).replace('/', "\\");

        assert!(normalized.contains(".ganmaoyuan\\managed\\02_requirements"));
        assert!(normalized.contains("需求 文档.txt"));
        assert!(!normalized.contains(r"\\?\"));
        assert!(!normalized.contains('/'));
        fs::remove_dir_all(root).unwrap();
    }
}
