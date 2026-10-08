use crate::{
    analysis::{analyze_managed_file, extracted_text_path},
    core_projection::{
        ActivityProjectionAdapter, ExecutionProjectionAdapter, FileProjectionAdapter,
        LocationPreviewService, PendingActionAdapter,
    },
    deepseek, inbox_routing,
    models::{
        ActivityProjection, AppRegistry, AtlasAssessment, AtlasSkillAssessment, AuditEvent,
        ChatAttachmentData, ChatImageAttachmentInput, CleanupExecutionBatch,
        CleanupExecutionDocument, CleanupExecutionItem, CleanupPlan, CleanupPlanDocument,
        CleanupPlanItem, CleanupPlanReviewFilter, CodexCliCapabilityProbe, CodexExternalResult,
        CodexExternalResultsDocument, CodexPromptRecord, CodexPromptResult, CodexReportApplyResult,
        CodexReportImportResult, CodexReportRecord, CodexResultBridgeScanResult, CodexRun,
        CodexRunDocument, CodexTask, CodexTaskAcceptance, CodexTaskCreateRequest,
        CodexTaskGitVerification, CodexTaskList, CodexTaskTodayItem, CodexTaskTodaySummary,
        ContinueProjectPreference, ContinueProjectRecommendation, ContinueWorkFocus,
        ContinueWorkPrimaryAction, DailyContinueItem, DailyContinueSnapshot, DailySession,
        DataHealthRecord, DecisionRecord, DecisionTrace, DecisionTraceConfidence,
        DecisionTraceEvidence, DeepSeekAuthorization, EvidenceRef, ExecutionRecord, FileAnalysis,
        FileAnalysisDocument, FileProjection, FileRecord, FinishWorkResult, GeneratedFileResult,
        GitChangedFile, GitCommitInfo, GitSnapshot, GlobalFilesDocument, GlobalManagedFile,
        GlobalSearchResult, ImportResult, ImprovementCandidate, InboxProjectCandidate,
        InboxRouteOperation, InboxRoutingSettings, InboxSourceEvent, KnowledgePatternCandidate,
        MaterialInboxDocument, MaterialInboxItem, MaterialInboxRouteResult, MemoItem,
        MessageAttachment, MessageEvidenceItem, MessageResult, MonitoringState,
        PendingActionProjection, PendingReviewItem, PersonalSkill, ProjectActionCandidate,
        ProjectAnalysis, ProjectAttention, ProjectContextActivity, ProjectContextCodexResult,
        ProjectContextDecision, ProjectContextFile, ProjectContextFocus, ProjectContextFreshness,
        ProjectContextGitCommit, ProjectContextGitFacts, ProjectContextPacket,
        ProjectContextPendingAction, ProjectCreateResult, ProjectFactCaptureRequest,
        ProjectFactSyncResult, ProjectImpactAnalysis, ProjectImpactEvidence, ProjectImpactFinding,
        ProjectManifest, ProjectStateChange, ProjectStateProposal, ProjectStateSummary,
        ProjectSummary, RecoveryPoint, SearchIndexDocument, SearchIndexEntry, TaskRecord,
        TodayBlocker, TodayPendingItem, TodayProjectFocus, TodayWorkspace, V1Readiness,
        WeeklySkillCandidate, WorkEvent, WorkLedgerDocument, WorkLedgerSnapshot,
        WorkspaceActivityItem, WorkspaceActivitySummary, WorkspaceConfig, WorkspaceDraft,
        WorkspaceMessage, WorkspaceScanBatch, WorkspaceScanDocument, WorkspaceScanFile,
        ANALYSIS_SCHEMA_VERSION, MANIFEST_SCHEMA_VERSION, MATERIAL_INBOX_ANALYSIS_VERSION,
        MATERIAL_INBOX_SCHEMA_VERSION, SEARCH_INDEX_SCHEMA_VERSION,
        WORKSPACE_CONFIG_SCHEMA_VERSION,
    },
    storage::{
        append_json_line, canonical_existing_file, canonical_project_root, global_data_dir,
        hash_file, new_id, now_string, path_to_string, read_json, write_json_atomic,
        write_text_atomic,
    },
};
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use serde_json::json;
use sha2::{Digest, Sha256};
#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;
use std::{
    collections::{HashMap, HashSet},
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tauri::Runtime;

static PROJECT_WRITE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
static PROJECT_MONITOR_HASH_CACHE: OnceLock<Mutex<HashMap<String, ProjectMonitorHashCacheEntry>>> =
    OnceLock::new();

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;
// A restarted desktop process no longer owns the child handle. Keep the result bridge
// available long enough for a real Codex result to arrive before asking for review.
const CODEX_RESTART_RESULT_GRACE_MS: u128 = 2 * 60 * 1000;
// `codex exec --json` can continuously emit events. Drain both pipes while it runs
// and stop a run only after the bounded, user-visible execution window for its task
// type. Coding tasks need a longer window for real repository work; short analysis
// and verification tasks keep the original guard against a hung CLI.
const CODEX_STANDARD_RUN_TIMEOUT: Duration = Duration::from_secs(3 * 60);
const CODEX_CODING_RUN_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const CODEX_RUN_POLL_INTERVAL: Duration = Duration::from_millis(200);
// A real bridge result is sufficient task evidence. Give Codex a short chance to
// exit naturally, then stop only its lingering process instead of timing out the task.
const CODEX_RESULT_EXIT_GRACE: Duration = Duration::from_secs(10);
const AI_REFERENCE_FILE_LIMIT: usize = 3;
const AI_REFERENCE_EXCERPT_CHARS: usize = 1800;
const CHAT_IMAGE_MAX_BYTES: usize = 8 * 1024 * 1024;
const CHAT_IMAGE_MAX_COUNT: usize = 4;
const CHAT_IMAGE_MAX_TOTAL_BYTES: usize = 20 * 1024 * 1024;

#[derive(Debug)]
struct CodexProcessOutput {
    exit_code: Option<i32>,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    timed_out: bool,
    result_received: bool,
    stopped_after_result: bool,
    last_activity_at: String,
    recent_activity: Vec<String>,
}

const CODEX_LIVE_ACTIVITY_LIMIT: usize = 20;
const CODEX_LIVE_ACTIVITY_MAX_CHARS: usize = 240;

#[derive(Debug, Default)]
struct LiveCodexOutput {
    recent_activity: Vec<String>,
    last_activity_at: String,
}

pub(crate) fn project_write_lock() -> &'static Mutex<()> {
    PROJECT_WRITE_LOCK.get_or_init(|| Mutex::new(()))
}

fn hidden_command(program: &str) -> Command {
    let mut command = Command::new(program);
    #[cfg(target_os = "windows")]
    command.creation_flags(CREATE_NO_WINDOW);
    command
}

#[derive(Debug, Clone, Default)]
struct CodexResultContract {
    task_id: String,
    task_type: String,
    status: String,
    summary: String,
    result_text: String,
    completed_at: String,
    remaining_issues: Vec<String>,
    manual_acceptance: Vec<String>,
    result_source: String,
    result_run_id: String,
    changed_files: Vec<String>,
    commits: Vec<String>,
    tests: Vec<String>,
    findings: Vec<String>,
    recommendations: Vec<String>,
    questions: Vec<String>,
    checks: Vec<String>,
    passed: Vec<String>,
    failed: Vec<String>,
    artifacts: Vec<String>,
    target_files: Vec<String>,
    external_thread_id: String,
}

fn trace_confidence(score: u8) -> DecisionTraceConfidence {
    let score = score.min(100);
    let (level, label) = if score >= 78 {
        ("high", "高")
    } else if score >= 45 {
        ("medium", "中")
    } else {
        ("low", "低")
    };
    DecisionTraceConfidence {
        score,
        level: level.to_string(),
        display: format!("{label} {score}%"),
    }
}

fn trace_confidence_from_ratio(value: f32) -> DecisionTraceConfidence {
    trace_confidence((value.clamp(0.0, 1.0) * 100.0).round() as u8)
}

fn new_decision_trace(
    decision_type: &str,
    subject_id: &str,
    project_id: &str,
    input_evidence: Vec<DecisionTraceEvidence>,
    ai_understanding: String,
    recommendation: String,
    confidence: DecisionTraceConfidence,
) -> DecisionTrace {
    let now = now_string();
    DecisionTrace {
        id: new_id(),
        decision_type: decision_type.to_string(),
        subject_id: subject_id.to_string(),
        project_id: project_id.to_string(),
        input_evidence,
        ai_understanding,
        recommendation,
        confidence,
        user_decision: "pending".to_string(),
        execution: "pending".to_string(),
        created_at: now.clone(),
        updated_at: now,
        ..DecisionTrace::default()
    }
}

pub(crate) fn skill_candidate_decision_trace(
    project_id: &str,
    subject: &str,
    scenario: &str,
    evidence: &[String],
    confidence: f32,
) -> DecisionTrace {
    new_decision_trace(
        "skillCandidate",
        subject,
        project_id,
        evidence
            .iter()
            .take(8)
            .map(|item| DecisionTraceEvidence {
                kind: "weeklyRecord".to_string(),
                label: "本周真实记录".to_string(),
                summary: first_line(item, 240),
                source_id: String::new(),
            })
            .collect(),
        scenario.to_string(),
        format!("建议作为技能候选：{subject}"),
        trace_confidence_from_ratio(confidence),
    )
}

fn upsert_pending_trace(traces: &mut Vec<DecisionTrace>, mut next: DecisionTrace) {
    if let Some(existing) = traces.iter_mut().rev().find(|trace| {
        trace.decision_type == next.decision_type
            && trace.subject_id == next.subject_id
            && trace.user_decision == "pending"
            && trace.execution == "pending"
    }) {
        next.id = existing.id.clone();
        next.created_at = existing.created_at.clone();
        *existing = next;
    } else {
        traces.push(next);
    }
}

fn update_trace_outcome(
    traces: &mut [DecisionTrace],
    decision_types: &[&str],
    user_decision: &str,
    execution: &str,
    note: &str,
) {
    let now = now_string();
    for trace in traces
        .iter_mut()
        .filter(|trace| decision_types.contains(&trace.decision_type.as_str()))
    {
        trace.user_decision = user_decision.to_string();
        trace.user_decision_note = note.to_string();
        trace.execution = execution.to_string();
        trace.execution_note = note.to_string();
        trace.updated_at = now.clone();
    }
}

fn inbox_trace_evidence(item: &MaterialInboxItem) -> Vec<DecisionTraceEvidence> {
    let mut evidence = vec![DecisionTraceEvidence {
        kind: "fileName".to_string(),
        label: "文件名".to_string(),
        summary: item.file_name.clone(),
        source_id: item.id.clone(),
    }];
    if !item.content_summary.trim().is_empty() {
        evidence.push(DecisionTraceEvidence {
            kind: "fileSummary".to_string(),
            label: "文件摘要".to_string(),
            summary: first_line(&item.content_summary, 240),
            source_id: item.id.clone(),
        });
    }
    if !item.main_fields_or_sections.is_empty() {
        evidence.push(DecisionTraceEvidence {
            kind: "fields".to_string(),
            label: "表头或章节".to_string(),
            summary: item
                .main_fields_or_sections
                .iter()
                .take(5)
                .map(|value| first_line(value, 100))
                .collect::<Vec<_>>()
                .join("；"),
            source_id: item.id.clone(),
        });
    }
    for candidate in item.project_candidates.iter().take(3) {
        evidence.push(DecisionTraceEvidence {
            kind: "projectHistory".to_string(),
            label: format!("项目候选：{}", candidate.candidate_project_name),
            summary: candidate
                .reasons
                .iter()
                .chain(candidate.evidence.iter())
                .take(5)
                .map(|value| first_line(value, 100))
                .collect::<Vec<_>>()
                .join("；"),
            source_id: candidate.candidate_project_id.clone(),
        });
    }
    evidence
}

fn sync_inbox_decision_traces(item: &mut MaterialInboxItem) {
    let evidence = inbox_trace_evidence(item);
    let confidence = trace_confidence(item.confidence_score);
    upsert_pending_trace(
        &mut item.decision_traces,
        new_decision_trace(
            "fileClassification",
            &item.id,
            &item.target_project_id,
            evidence.clone(),
            format!(
                "识别为{}",
                if item.document_type.trim().is_empty() {
                    item.recommended_category.as_str()
                } else {
                    item.document_type.as_str()
                }
            ),
            format!("建议分类：{}", item.recommended_category),
            confidence.clone(),
        ),
    );
    upsert_pending_trace(
        &mut item.decision_traces,
        new_decision_trace(
            "documentPurpose",
            &item.id,
            &item.target_project_id,
            item.document_purpose_evidence
                .iter()
                .map(|summary| trace_evidence("purposeEvidence", "用途依据", summary, &item.id))
                .collect(),
            format!("资料用途：{}", item.document_purpose),
            "用途判断仅用于辅助分类，证据不足时保持待确认。".to_string(),
            trace_confidence(item.document_purpose_confidence),
        ),
    );
    upsert_pending_trace(
        &mut item.decision_traces,
        new_decision_trace(
            "ownershipDecision",
            &item.id,
            &item.target_project_id,
            evidence.clone(),
            format!("判断归属：{}", item.ownership_type),
            if item.project_candidate_reasons.is_empty() {
                item.decision_basis.join("；")
            } else {
                item.project_candidate_reasons.join("；")
            },
            confidence.clone(),
        ),
    );
    if !item.target_project_id.is_empty()
        || !item.recommended_project_name.is_empty()
        || !item.project_candidates.is_empty()
    {
        upsert_pending_trace(
            &mut item.decision_traces,
            new_decision_trace(
                "projectMatch",
                &item.id,
                &item.target_project_id,
                evidence.clone(),
                format!(
                    "项目匹配：{}",
                    item.target_project_name
                        .is_empty()
                        .then_some(item.recommended_project_name.as_str())
                        .unwrap_or(item.target_project_name.as_str())
                ),
                item.confidence_reasons.join("；"),
                confidence.clone(),
            ),
        );
    }
    upsert_pending_trace(
        &mut item.decision_traces,
        new_decision_trace(
            "locationRecommendation",
            &item.id,
            &item.target_project_id,
            evidence,
            item.location_reason.clone(),
            format!(
                "建议位置：{}",
                if item.recommended_location.is_empty() {
                    "待确认"
                } else {
                    item.recommended_location.as_str()
                }
            ),
            confidence,
        ),
    );
}

#[derive(Debug)]
struct ImportBatch {
    new_records: Vec<FileRecord>,
    duplicates: Vec<FileRecord>,
    location_decisions: Vec<crate::models::LocationDecision>,
    created_paths: Vec<PathBuf>,
    created_content_paths: Vec<PathBuf>,
}

#[derive(Debug, Clone)]
struct LocationPlan {
    destination: PathBuf,
    category: String,
    version_number: u32,
    version_group_id: String,
    previous_version_id: Option<String>,
    relative_path: String,
    reason: String,
}

#[derive(Debug, Clone)]
struct ObservedProjectFile {
    path: String,
    file_name: String,
    file_type: String,
    hash: String,
}

#[derive(Debug, Clone)]
struct ProjectMonitorHashCacheEntry {
    size_bytes: u64,
    modified_at: Option<SystemTime>,
    hash: String,
}

pub fn list_projects<R: Runtime>(app: &tauri::AppHandle<R>) -> Result<Vec<ProjectSummary>, String> {
    let mut registry = read_registry(app)?;
    let mut changed = registry.schema_version != MANIFEST_SCHEMA_VERSION;
    registry.schema_version = MANIFEST_SCHEMA_VERSION;
    registry.projects.retain(|project| {
        let root = PathBuf::from(&project.root_dir);
        let manifest = manifest_path(&root);
        let keep = root.exists() && manifest.exists();
        if !keep {
            changed = true;
        }
        keep
    });
    for project in &mut registry.projects {
        let root = PathBuf::from(&project.root_dir);
        project.manifest_path = path_to_string(&manifest_path(&root));
        if let Ok(manifest) = read_json::<ProjectManifest>(&manifest_path(&root)) {
            if project.repository_path != manifest.project.repository_path {
                project.repository_path = manifest.project.repository_path;
                changed = true;
            }
        }
        if project.created_at.is_empty() {
            project.created_at = project.last_opened_at.clone();
            changed = true;
        }
    }
    registry.projects.sort_by(|left, right| {
        right
            .last_opened_at
            .cmp(&left.last_opened_at)
            .then_with(|| left.name.cmp(&right.name))
    });
    if changed {
        write_registry(app, &registry)?;
    }
    Ok(registry.projects)
}

pub fn list_file_projections<R: Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<Vec<FileProjection>, String> {
    let mut projections = Vec::new();
    for project in read_registry(app)?.projects {
        let root = PathBuf::from(&project.root_dir);
        if let Ok(manifest) = read_and_repair_manifest(&root) {
            projections.extend(
                manifest
                    .files
                    .iter()
                    .map(|file| FileProjectionAdapter::from_project_file(file, &project.id)),
            );
        }
    }
    projections.extend(
        read_global_files_document(app)?
            .files
            .iter()
            .filter(|file| file.undone_at.trim().is_empty())
            .map(FileProjectionAdapter::from_global_file),
    );
    projections.extend(
        read_material_inbox_document(app)?
            .items
            .iter()
            .map(FileProjectionAdapter::from_inbox),
    );
    if let Some(batch) = read_workspace_scan_document(app)?.batches.last() {
        projections.extend(batch.files.iter().map(FileProjectionAdapter::from_scan));
    }
    Ok(projections)
}

#[derive(Debug, Clone)]
struct InboxRouteOverride {
    category: String,
    managed_file_name: String,
    operation_id: String,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct AiProjectMatchDecision {
    #[serde(default)]
    candidate_project_id: String,
    #[serde(default)]
    score: u8,
    #[serde(default)]
    reasons: Vec<String>,
    #[serde(default)]
    evidence: Vec<String>,
    #[serde(default)]
    matched_signals: Vec<String>,
}

pub fn search_projects<R: Runtime>(
    app: &tauri::AppHandle<R>,
    query: String,
) -> Result<Vec<GlobalSearchResult>, String> {
    let normalized_query = normalize_search_text(query.trim());
    if normalized_query.is_empty() {
        return Ok(Vec::new());
    }
    let _guard = project_write_lock()
        .lock()
        .map_err(|_| "项目存储锁已损坏，请重启感冒院。".to_string())?;
    let index = rebuild_search_index(app)?;
    let mut results = index
        .entries
        .iter()
        .filter_map(|entry| search_index_entry_to_result(entry, &normalized_query))
        .collect::<Vec<_>>();
    results.sort_by(|left, right| {
        let left_score =
            match_search_score(&build_search_haystack(left), &normalized_query).unwrap_or(0);
        let right_score =
            match_search_score(&build_search_haystack(right), &normalized_query).unwrap_or(0);
        right_score
            .cmp(&left_score)
            .then_with(|| right.updated_at.cmp(&left.updated_at))
            .then_with(|| left.project_name.cmp(&right.project_name))
            .then_with(|| left.title.cmp(&right.title))
    });
    Ok(results)
}

fn rebuild_search_index<R: Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<SearchIndexDocument, String> {
    let registry = read_registry(app)?;
    let mut all_results = Vec::new();
    for project in registry.projects {
        let root = PathBuf::from(&project.root_dir);
        if !root.exists() {
            continue;
        }
        let manifest = match read_and_repair_manifest(&root) {
            Ok(manifest) => manifest,
            Err(_) => continue,
        };
        collect_project_search_results(&manifest, "", &mut all_results);
    }
    collect_global_search_results(app, "", &mut all_results)?;
    let now = now_string();
    let entries = all_results
        .into_iter()
        .filter_map(|result| search_result_to_index_entry(result, &now))
        .collect::<Vec<_>>();
    let document = SearchIndexDocument {
        schema_version: SEARCH_INDEX_SCHEMA_VERSION,
        entries,
        updated_at: now,
    };
    write_search_index_document(app, &document)?;
    Ok(document)
}

fn collect_global_search_results<R: Runtime>(
    app: &tauri::AppHandle<R>,
    normalized_query: &str,
    results: &mut Vec<GlobalSearchResult>,
) -> Result<(), String> {
    for file in read_global_files_document(app)?.files {
        if !file.undone_at.is_empty() {
            continue;
        }
        let projection = FileProjectionAdapter::from_global_file(&file);
        let result = search_result_from_file_projection(
            &projection,
            String::new(),
            String::new(),
            if file.related_project.trim().is_empty() {
                "个人文件空间".to_string()
            } else {
                file.related_project.clone()
            },
            "global_file",
            &file.decision_trace_id,
            confidence_display_from_score(file.document_purpose_confidence),
        );
        let haystack = build_search_haystack(&result);
        if normalized_query.is_empty()
            || match_search_score(&haystack, normalized_query).unwrap_or(0) > 0
        {
            results.push(result);
        }
    }
    Ok(())
}

pub fn get_workspace_config<R: Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<WorkspaceConfig, String> {
    let path = workspace_config_path(app)?;
    if path.exists() {
        let mut config: WorkspaceConfig = read_json(&path)?;
        repair_workspace_config(&mut config, app)?;
        let verified = verify_workspace_config_status(config);
        write_json_atomic(&path, &verified)?;
        return Ok(verified);
    }
    let config = build_workspace_config(default_workspace_root(app)?, "missing".to_string());
    write_json_atomic(&path, &config)?;
    Ok(config)
}

pub fn initialize_workspace_root<R: Runtime>(
    app: &tauri::AppHandle<R>,
    root: Option<String>,
) -> Result<WorkspaceConfig, String> {
    let _guard = project_write_lock()
        .lock()
        .map_err(|_| "全局存储锁已损坏，请重启感冒院。".to_string())?;
    let root = match root
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    {
        Some(value) => normalize_workspace_root(Path::new(&value))?,
        None => default_workspace_root(app)?,
    };
    let mut config = build_workspace_config(root, "missing".to_string());
    for directory in workspace_standard_dirs(&config) {
        fs::create_dir_all(&directory).map_err(|err| {
            format!(
                "创建个人文件空间目录失败：{}：{err}",
                path_to_string(&directory)
            )
        })?;
    }
    config.status = "ready".to_string();
    config.last_verified_at = now_string();
    config.updated_at = config.last_verified_at.clone();
    config = verify_workspace_config_status(config);
    write_json_atomic(&workspace_config_path(app)?, &config)?;
    Ok(config)
}

pub fn verify_workspace_root<R: Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<WorkspaceConfig, String> {
    let mut config = get_workspace_config(app)?;
    config = verify_workspace_config_status(config);
    write_json_atomic(&workspace_config_path(app)?, &config)?;
    Ok(config)
}

pub fn list_workspace_scan_batches<R: Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<Vec<WorkspaceScanBatch>, String> {
    let mut document = read_workspace_scan_document(app)?;
    document
        .batches
        .sort_by(|left, right| right.created_at.cmp(&left.created_at));
    Ok(document.batches)
}

pub fn scan_workspace_directory<R: Runtime>(
    app: &tauri::AppHandle<R>,
    source_directory: String,
) -> Result<WorkspaceScanBatch, String> {
    run_workspace_scan(app, PathBuf::from(source_directory.trim()))
}

pub fn scan_desktop_directory<R: Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<WorkspaceScanBatch, String> {
    match default_user_directory("Desktop") {
        Ok(root) => run_workspace_scan(app, root),
        Err(err) => failed_workspace_scan_batch(app, "Desktop".to_string(), err),
    }
}

pub fn scan_downloads_directory<R: Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<WorkspaceScanBatch, String> {
    match default_user_directory("Downloads") {
        Ok(root) => run_workspace_scan(app, root),
        Err(err) => failed_workspace_scan_batch(app, "Downloads".to_string(), err),
    }
}

pub fn list_cleanup_plans<R: Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<Vec<CleanupPlan>, String> {
    let mut document = read_cleanup_plan_document(app)?;
    document
        .plans
        .sort_by(|left, right| right.created_at.cmp(&left.created_at));
    Ok(document.plans)
}

pub fn get_cleanup_plan<R: Runtime>(
    app: &tauri::AppHandle<R>,
    plan_id: &str,
) -> Result<CleanupPlan, String> {
    let document = read_cleanup_plan_document(app)?;
    document
        .plans
        .into_iter()
        .find(|plan| plan.id == plan_id)
        .ok_or_else(|| format!("整理方案不存在：{plan_id}"))
}

pub fn generate_cleanup_plan<R: Runtime>(
    app: &tauri::AppHandle<R>,
    scan_batch_id: &str,
) -> Result<CleanupPlan, String> {
    let scan_document = read_workspace_scan_document(app)?;
    let batch = scan_document
        .batches
        .into_iter()
        .find(|item| item.id == scan_batch_id)
        .ok_or_else(|| format!("扫描批次不存在：{scan_batch_id}"))?;
    if batch.status != "completed" {
        return Err(format!(
            "扫描批次尚未完成，不能生成整理方案：{}",
            batch.status
        ));
    }
    let workspace = get_workspace_config(app)?;
    let plan = build_cleanup_plan_from_scan_batch(&batch, &workspace, None);
    persist_cleanup_plan(app, plan)
}

pub fn review_cleanup_plan_items<R: Runtime>(
    app: &tauri::AppHandle<R>,
    plan_id: &str,
    item_ids: Vec<String>,
    review_status: &str,
    user_reason: Option<String>,
) -> Result<CleanupPlan, String> {
    if item_ids.is_empty() {
        return Err("请选择需要审核的整理项。".to_string());
    }
    let workspace = get_workspace_config(app)?;
    update_cleanup_plan(app, plan_id, |plan| {
        let mut changed = 0usize;
        for item in &mut plan.items {
            if item_ids.iter().any(|id| id == &item.id) {
                apply_cleanup_review(item, review_status, user_reason.as_deref(), &workspace)?;
                changed += 1;
            }
        }
        if changed == 0 {
            return Err("没有找到需要审核的整理项。".to_string());
        }
        Ok(())
    })
}

pub fn bulk_review_cleanup_plan<R: Runtime>(
    app: &tauri::AppHandle<R>,
    plan_id: &str,
    filter: CleanupPlanReviewFilter,
    review_status: &str,
    user_reason: Option<String>,
) -> Result<CleanupPlan, String> {
    let workspace = get_workspace_config(app)?;
    update_cleanup_plan(app, plan_id, |plan| {
        let mut changed = 0usize;
        for item in &mut plan.items {
            if cleanup_plan_item_matches_filter(item, &filter) {
                apply_cleanup_review(item, review_status, user_reason.as_deref(), &workspace)?;
                changed += 1;
            }
        }
        if changed == 0 {
            return Err("当前筛选条件下没有可审核的整理项。".to_string());
        }
        Ok(())
    })
}

pub fn modify_cleanup_plan_item<R: Runtime>(
    app: &tauri::AppHandle<R>,
    plan_id: &str,
    item_id: &str,
    final_project: Option<String>,
    final_category: Option<String>,
    final_target_path: String,
    user_reason: Option<String>,
) -> Result<CleanupPlan, String> {
    let workspace = get_workspace_config(app)?;
    update_cleanup_plan(app, plan_id, |plan| {
        let item = plan
            .items
            .iter_mut()
            .find(|item| item.id == item_id)
            .ok_or_else(|| format!("整理项不存在：{item_id}"))?;
        apply_cleanup_modification(
            item,
            final_project,
            final_category,
            final_target_path,
            user_reason.as_deref(),
            &workspace,
        )
    })
}

pub fn reset_cleanup_plan_items<R: Runtime>(
    app: &tauri::AppHandle<R>,
    plan_id: &str,
    item_ids: Vec<String>,
) -> Result<CleanupPlan, String> {
    if item_ids.is_empty() {
        return Err("请选择需要退回待确认的整理项。".to_string());
    }
    update_cleanup_plan(app, plan_id, |plan| {
        let mut changed = 0usize;
        let now = now_string();
        for item in &mut plan.items {
            if item_ids.iter().any(|id| id == &item.id) {
                item.review_status = "pending".to_string();
                item.reviewed_at.clear();
                item.reviewer.clear();
                item.user_reason.clear();
                item.final_project.clear();
                item.final_category.clear();
                item.final_target_path.clear();
                let mut trace = cleanup_review_trace(
                    item,
                    "pending",
                    "pending",
                    "用户已将审核状态退回待确认。",
                    None,
                );
                trace.user_decision = "pending".to_string();
                trace.execution = "pending".to_string();
                trace.execution_note = "用户已将审核状态退回待确认。".to_string();
                trace.updated_at = now.clone();
                push_cleanup_item_trace(item, trace);
                changed += 1;
            }
        }
        if changed == 0 {
            return Err("没有找到需要退回的整理项。".to_string());
        }
        Ok(())
    })
}

pub fn execute_cleanup_plan<R: Runtime>(
    app: &tauri::AppHandle<R>,
    plan_id: &str,
) -> Result<CleanupExecutionBatch, String> {
    let _guard = project_write_lock()
        .lock()
        .map_err(|_| "全局存储锁已损坏，请重启感冒院。".to_string())?;
    let workspace = get_workspace_config(app)?;
    let mut plan_document = read_cleanup_plan_document(app)?;
    let plan_index = plan_document
        .plans
        .iter()
        .position(|plan| plan.id == plan_id)
        .ok_or_else(|| format!("整理方案不存在：{plan_id}"))?;
    if plan_document.plans[plan_index].status != "reviewing" {
        return Err("只有 reviewing 状态的整理方案可以执行。".to_string());
    }
    let executable_items = plan_document.plans[plan_index]
        .items
        .iter()
        .filter(|item| item.review_status == "approved")
        .cloned()
        .collect::<Vec<_>>();
    if executable_items.is_empty() {
        return Err("没有已确认的整理项可执行。".to_string());
    }
    let mut executions = read_cleanup_execution_document(app)?;
    if executions
        .batches
        .iter()
        .any(|batch| batch.cleanup_plan_id == plan_id && batch.status != "cancelled")
    {
        return Err("该整理方案已经执行过，不能重复复制。".to_string());
    }

    let mut global_document = read_global_files_document(app)?;
    let audit_path = global_audit_path(app)?;
    let mut batch = CleanupExecutionBatch {
        id: new_id(),
        cleanup_plan_id: plan_id.to_string(),
        started_at: now_string(),
        status: "running".to_string(),
        ..CleanupExecutionBatch::default()
    };

    for item in executable_items {
        let execution_item =
            execute_cleanup_plan_item(&workspace, &audit_path, &mut global_document, &item);
        batch.items.push(execution_item);
    }

    batch.completed_at = now_string();
    batch.status = if batch.items.iter().any(|item| item.status == "completed") {
        "completed".to_string()
    } else {
        "failed".to_string()
    };

    let plan = &mut plan_document.plans[plan_index];
    for plan_item in &mut plan.items {
        if let Some(executed) = batch
            .items
            .iter()
            .find(|item| item.cleanup_plan_item_id == plan_item.id)
        {
            let mut trace = cleanup_execution_trace(plan_item, executed);
            if executed.status == "completed" {
                trace.execution = "executed".to_string();
                trace.execution_note = "安全复制已完成。".to_string();
            } else {
                trace.execution = "failed".to_string();
                trace.execution_note = non_empty_or_pending(&executed.error);
            }
            push_cleanup_item_trace(plan_item, trace);
        }
    }
    plan.status = "executed".to_string();
    plan_document.updated_at = now_string();
    executions.batches.insert(0, batch.clone());
    executions.batches.truncate(50);
    executions.updated_at = now_string();
    write_global_files_document(app, &global_document)?;
    write_cleanup_plan_document(app, &plan_document)?;
    write_cleanup_execution_document(app, &executions)?;
    let _ = link_workspace_files_to_project_impacts(app, &global_document, &batch);
    Ok(batch)
}

pub fn list_cleanup_execution_batches<R: Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<Vec<CleanupExecutionBatch>, String> {
    Ok(read_cleanup_execution_document(app)?.batches)
}

pub fn list_execution_records<R: Runtime>(
    app: &tauri::AppHandle<R>,
    project_root: Option<String>,
) -> Result<Vec<ExecutionRecord>, String> {
    let cleanup = read_cleanup_execution_document(app)?;
    let mut records = ExecutionProjectionAdapter::from_cleanup_batches(&cleanup.batches);
    if let Some(project_root) = project_root.filter(|value| !value.trim().is_empty()) {
        let root = PathBuf::from(project_root);
        let tasks = read_codex_tasks(&root)?;
        let runs = read_codex_runs(&root)?;
        records.extend(ExecutionProjectionAdapter::from_codex_runs(&runs, &tasks));
    }
    records.sort_by(|left, right| right.started_at.cmp(&left.started_at));
    Ok(records)
}

pub fn undo_cleanup_execution_batch<R: Runtime>(
    app: &tauri::AppHandle<R>,
    batch_id: &str,
) -> Result<CleanupExecutionBatch, String> {
    let _guard = project_write_lock()
        .lock()
        .map_err(|_| "全局存储锁已损坏，请重启感冒院。".to_string())?;
    let mut executions = read_cleanup_execution_document(app)?;
    let batch_index = executions
        .batches
        .iter()
        .position(|batch| batch.id == batch_id)
        .ok_or_else(|| format!("执行批次不存在：{batch_id}"))?;
    if executions.batches[batch_index].status == "cancelled" {
        return Err("该执行批次已经撤销。".to_string());
    }
    let mut global_document = read_global_files_document(app)?;
    let audit_path = global_audit_path(app)?;
    let mut updated = executions.batches[batch_index].clone();
    let mut conflict_count = 0usize;
    let mut undone_count = 0usize;
    for item in &mut updated.items {
        if item.status != "completed" {
            continue;
        }
        match perform_global_undo_by_operation(
            &audit_path,
            &mut global_document,
            &item.operation_id,
        ) {
            Ok(record) => {
                item.status = "undone".to_string();
                item.error.clear();
                item.global_file_id = record.id;
                undone_count += 1;
            }
            Err(error) => {
                item.status = "undoConflict".to_string();
                item.error = error;
                conflict_count += 1;
            }
        }
    }
    updated.completed_at = now_string();
    updated.status = if conflict_count > 0 {
        "undoConflict".to_string()
    } else if undone_count > 0 {
        "cancelled".to_string()
    } else {
        "failed".to_string()
    };
    executions.batches[batch_index] = updated.clone();
    executions.updated_at = now_string();
    write_global_files_document(app, &global_document)?;
    write_cleanup_execution_document(app, &executions)?;
    Ok(updated)
}

pub fn load_project(project_root: &str) -> Result<ProjectManifest, String> {
    let root = PathBuf::from(project_root);
    if !root.exists() {
        return Err(format!(
            "项目根目录不存在或当前不可用：{}",
            path_to_string(&root)
        ));
    }
    let _guard = project_write_lock()
        .lock()
        .map_err(|_| "项目存储锁已损坏，请重启感冒院。".to_string())?;
    read_and_repair_manifest(&root)
}

pub fn mark_project_opened<R: Runtime>(
    app: &tauri::AppHandle<R>,
    project_root: String,
) -> Result<ProjectSummary, String> {
    let root = PathBuf::from(project_root);
    let _guard = project_write_lock()
        .lock()
        .map_err(|_| "项目存储锁已损坏，请重启感冒院。".to_string())?;
    let mut manifest = read_and_repair_manifest(&root)?;
    manifest.project.last_opened_at = now_string();
    let summary = manifest.project.clone();
    persist_project(&root, &manifest, None)?;
    upsert_project(app, summary.clone())?;
    Ok(summary)
}

pub fn create_project<R: Runtime>(
    app: &tauri::AppHandle<R>,
    name: String,
    root_dir: String,
    file_paths: Vec<String>,
    description: String,
) -> Result<ProjectCreateResult, String> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("项目名称不能为空。".to_string());
    }
    let root = canonical_project_root(Path::new(&root_dir))?;
    let _guard = project_write_lock()
        .lock()
        .map_err(|_| "项目存储锁已损坏，请重启感冒院。".to_string())?;
    if manifest_path(&root).exists() {
        return Err(format!(
            "该目录已经存在感冒院项目：{}。请从历史项目继续，或选择新的根目录。",
            path_to_string(&root)
        ));
    }
    ensure_project_dirs(&root)?;

    let now = now_string();
    let summary = ProjectSummary {
        id: new_id(),
        name,
        root_dir: path_to_string(&root),
        manifest_path: path_to_string(&manifest_path(&root)),
        last_opened_at: now.clone(),
        next_step: "查看资料分析结果，补充项目目标与当前任务。".to_string(),
        created_at: now.clone(),
        description: description.trim().to_string(),
        repository_path: String::new(),
    };
    let mut manifest = ProjectManifest {
        schema_version: MANIFEST_SCHEMA_VERSION,
        project: summary.clone(),
        files: Vec::new(),
        messages: Vec::new(),
        tasks: Vec::new(),
        decisions: Vec::new(),
        artifacts: Vec::new(),
        codex_prompts: Vec::new(),
        codex_reports: Vec::new(),
        daily_sessions: Vec::new(),
        location_decisions: Vec::new(),
        pending_reviews: Vec::new(),
        monitoring: MonitoringState::default(),
        atlas: waiting_atlas(),
        project_analysis: ProjectAnalysis::default(),
        audit: Vec::new(),
        draft: WorkspaceDraft {
            project_id: summary.id.clone(),
            text: String::new(),
            pending_file_paths: Vec::new(),
            updated_at: now.clone(),
        },
        recovery_points: Vec::new(),
        deepseek_authorization: DeepSeekAuthorization::default(),
        project_impact_analyses: Vec::new(),
        project_action_candidates: Vec::new(),
        project_state_proposals: Vec::new(),
        daily_continue_snapshots: Vec::new(),
        project_state_summary: ProjectStateSummary::default(),
        project_attentions: Vec::new(),
        work_pattern_candidates: Vec::new(),
        project_state_auto_apply: false,
        data_health_records: Vec::new(),
        atlas_skill_assessments: Vec::new(),
        skill_library: Vec::new(),
        knowledge_pattern_candidates: Vec::new(),
        improvement_candidates: Vec::new(),
        v1_readiness: V1Readiness::default(),
    };
    let original = manifest.clone();
    let batch = import_into_records(
        &root,
        &mut manifest.files,
        file_paths,
        "用户导入",
        "项目初始化",
        None,
    )?;
    manifest
        .location_decisions
        .extend(batch.location_decisions.clone());

    let user_message = workspace_message(
        "user",
        "requirement",
        if description.trim().is_empty() {
            "已创建项目，等待补充项目需求。".to_string()
        } else {
            description.trim().to_string()
        },
        "user",
    );
    record_message_derivatives(&mut manifest, &user_message);
    manifest.messages.push(user_message);
    let analysis_message = workspace_message(
        "ganmaoyuan",
        "analysis",
        format!(
            "已建立项目文件结构，登记 {} 个初始文件；其中 {} 个内容重复项复用了已有受管副本。原文件保持不变。",
            batch.new_records.len(),
            batch.duplicates.len()
        ),
        "local_rule",
    );
    record_message_derivatives(&mut manifest, &analysis_message);
    manifest.messages.push(analysis_message);
    if manifest
        .files
        .iter()
        .any(|file| file.needs_user_confirmation)
    {
        let review_message = workspace_message(
            "ganmaoyuan",
            "review",
            "存在分类不确定、部分解析或解析失败的文件，已进入待检查事项；成功处理的文件无需逐个确认。".to_string(),
            "local_rule",
        );
        record_message_derivatives(&mut manifest, &review_message);
        manifest.messages.push(review_message);
    }
    record_imported_files_in_session(
        &mut manifest,
        batch
            .new_records
            .iter()
            .map(|file| file.id.clone())
            .collect(),
    );
    manifest.audit.push(audit_event(
        "project.create",
        &summary.root_dir,
        "success",
        format!(
            "导入 {} 个文件记录；生成 {} 条位置决策",
            batch.new_records.len(),
            batch.location_decisions.len()
        ),
        false,
        true,
    ));
    manifest.atlas = waiting_atlas();

    if let Err(err) = persist_project(&root, &manifest, Some(&original)) {
        rollback_batch(&batch);
        return Err(err);
    }
    if let Err(err) = upsert_project(app, summary.clone()) {
        return Err(format!(
            "项目文件已安全保存，但写入全局项目列表失败：{err}。重新启动后可通过项目根目录恢复。"
        ));
    }
    append_audit_event(&root, manifest.audit.last().expect("audit event exists"))?;

    Ok(ProjectCreateResult {
        project: summary,
        files: batch.new_records,
        messages: manifest.messages,
        atlas: manifest.atlas,
    })
}

pub fn import_files(
    project_root: String,
    file_paths: Vec<String>,
    related_task: String,
) -> Result<ImportResult, String> {
    import_files_internal(project_root, file_paths, related_task, None)
}

fn import_files_internal(
    project_root: String,
    file_paths: Vec<String>,
    related_task: String,
    route_override: Option<&InboxRouteOverride>,
) -> Result<ImportResult, String> {
    let root = PathBuf::from(project_root);
    let _guard = project_write_lock()
        .lock()
        .map_err(|_| "项目存储锁已损坏，请重启感冒院。".to_string())?;
    let mut manifest = read_and_repair_manifest(&root)?;
    let original = manifest.clone();
    let batch = import_into_records(
        &root,
        &mut manifest.files,
        file_paths,
        "用户导入",
        related_task.trim(),
        route_override,
    )?;
    manifest
        .location_decisions
        .extend(batch.location_decisions.clone());
    let mut location_message = workspace_message(
        "ganmaoyuan",
        "fileLocation",
        format!(
            "已登记 {} 个补充资料，其中 {} 个为重复内容；新副本已按项目规则归位，原文件保持不变。",
            batch.new_records.len(),
            batch.duplicates.len()
        ),
        "local_rule",
    );
    location_message.attachments = batch
        .new_records
        .iter()
        .map(|file| MessageAttachment {
            file_id: file.id.clone(),
            file_name: file.file_name.clone(),
            managed_path: file.managed_path.clone(),
            ..MessageAttachment::default()
        })
        .collect();
    let mut messages = vec![location_message];
    if batch
        .new_records
        .iter()
        .any(|file| file.needs_user_confirmation)
    {
        messages.push(workspace_message(
            "ganmaoyuan",
            "review",
            "新增资料中有分类不确定、部分解析或解析失败项，已进入待检查事项。".to_string(),
            "local_rule",
        ));
    }
    for message in &messages {
        record_message_derivatives(&mut manifest, message);
    }
    manifest.messages.extend(messages.clone());
    record_imported_files_in_session(
        &mut manifest,
        batch
            .new_records
            .iter()
            .map(|file| file.id.clone())
            .collect(),
    );
    manifest.project.last_opened_at = now_string();
    manifest.audit.push(audit_event(
        "file.import",
        &manifest.project.root_dir,
        "success",
        format!(
            "新增 {} 个记录，重复 {} 个，位置决策 {} 条",
            batch.new_records.len(),
            batch.duplicates.len(),
            batch.location_decisions.len()
        ),
        false,
        true,
    ));
    manifest.atlas = waiting_atlas();
    if let Err(err) = persist_project(&root, &manifest, Some(&original)) {
        rollback_batch(&batch);
        return Err(err);
    }
    append_audit_event(&root, manifest.audit.last().expect("audit event exists"))?;

    Ok(ImportResult {
        files: batch.new_records,
        messages,
        atlas: manifest.atlas,
        duplicates: batch.duplicates,
    })
}

pub fn prepare_project_message<R: Runtime>(
    app: &tauri::AppHandle<R>,
    project_root: String,
    text: String,
    image_attachments: Vec<ChatImageAttachmentInput>,
) -> Result<MessageResult, String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err("消息不能为空。".to_string());
    }
    let root = PathBuf::from(project_root);
    let _guard = project_write_lock()
        .lock()
        .map_err(|_| "项目存储锁已损坏，请重启感冒院。".to_string())?;
    let mut manifest = read_and_repair_manifest(&root)?;
    update_project_state_summary(&mut manifest);
    update_project_attentions(&mut manifest);
    update_work_pattern_candidates(&mut manifest);
    ensure_deepseek_authorization(&manifest)?;
    let selected_model = deepseek::selected_model_id(app)?;
    if selected_model.trim().is_empty() {
        return Err("请先在设置页保存 DeepSeek API Key 并测试连接。".to_string());
    }
    let mut user_message = workspace_message("user", "requirement", trimmed.to_string(), "user");
    user_message.attachments =
        persist_chat_image_attachments(&root, &user_message.id, image_attachments)?;
    let question = trimmed.to_string();
    let evidence_items = collect_answer_evidence(&manifest, &question);
    let mut assistant_message = workspace_message_with_status(
        "ganmaoyuan",
        "assistant",
        String::new(),
        "deepseek",
        "streaming",
        &selected_model,
    );
    assistant_message.evidence_items = evidence_items;
    record_message_derivatives(&mut manifest, &user_message);
    record_message_derivatives(&mut manifest, &assistant_message);
    manifest.messages.push(user_message.clone());
    manifest.messages.push(assistant_message.clone());
    let recorded_decision = extract_explicit_recorded_decision(&user_message.text)
        .map(|decision| {
            record_user_decision_event_in_manifest(
                &root,
                &mut manifest,
                &decision,
                None,
                Some(&user_message.id),
            )
        })
        .transpose()?;
    let recorded_next_step = if recorded_decision.is_none() {
        extract_explicit_recorded_next_step(&user_message.text)
            .map(|next_step| {
                record_user_next_step_event_in_manifest(
                    &root,
                    &mut manifest,
                    &next_step,
                    None,
                    Some(&user_message.id),
                )
            })
            .transpose()?
    } else {
        None
    };
    manifest.project.last_opened_at = now_string();
    manifest.draft.text.clear();
    manifest.draft.updated_at = now_string();
    persist_project(&root, &manifest, None)?;
    append_workspace_event(&root, &user_message)?;
    append_workspace_event(&root, &assistant_message)?;
    if let Some(write) = recorded_decision.filter(|write| write.is_new) {
        append_audit_event(&root, manifest.audit.last().expect("audit event exists"))?;
        append_work_event_if_new(&root, write.event.clone())?
            .ok_or_else(|| "该决定事实已存在。".to_string())?;
    }
    if let Some(write) = recorded_next_step.filter(|write| write.is_new) {
        append_audit_event(&root, manifest.audit.last().expect("audit event exists"))?;
        append_work_event_if_new(&root, write.event.clone())?
            .ok_or_else(|| "该下一步事实已存在。".to_string())?;
    }
    Ok(MessageResult {
        project: manifest.project,
        messages: vec![user_message, assistant_message.clone()],
        stream_message_id: assistant_message.id,
    })
}

fn chat_attachment_dir(root: &Path, message_id: &str) -> PathBuf {
    root.join(".ganmaoyuan")
        .join("chat-attachments")
        .join(message_id)
}

fn chat_image_content_type(value: &str) -> Option<&'static str> {
    match value.trim().to_ascii_lowercase().as_str() {
        "image/png" => Some("image/png"),
        "image/jpeg" | "image/jpg" => Some("image/jpeg"),
        "image/webp" => Some("image/webp"),
        _ => None,
    }
}

fn safe_chat_file_name(value: &str, content_type: &str) -> String {
    let raw = Path::new(value)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .trim();
    let fallback = match content_type {
        "image/png" => "pasted-image.png",
        "image/jpeg" => "pasted-image.jpg",
        _ => "pasted-image.webp",
    };
    let candidate = if raw.is_empty() { fallback } else { raw };
    let mut safe = candidate
        .chars()
        .map(|ch| {
            if ch.is_control() || matches!(ch, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*')
            {
                '_'
            } else {
                ch
            }
        })
        .collect::<String>();
    if safe == "." || safe == ".." || safe.trim().is_empty() {
        safe = fallback.to_string();
    }
    safe.chars().take(120).collect()
}

fn parse_chat_image_data_url(
    input: &ChatImageAttachmentInput,
) -> Result<(&'static str, Vec<u8>), String> {
    let (header, payload) = input
        .data_url
        .split_once(',')
        .ok_or_else(|| "图片附件数据格式无效。".to_string())?;
    if !header.to_ascii_lowercase().starts_with("data:")
        || !header.to_ascii_lowercase().contains(";base64")
    {
        return Err("图片附件必须使用 base64 data URL。".to_string());
    }
    let encoded_type = header
        .trim_start_matches("data:")
        .split(';')
        .next()
        .unwrap_or_default();
    let content_type = chat_image_content_type(&input.content_type)
        .or_else(|| chat_image_content_type(encoded_type))
        .ok_or_else(|| "仅支持 PNG、JPG/JPEG 和 WebP 图片。".to_string())?;
    if let Some(input_type) = chat_image_content_type(&input.content_type) {
        if input_type != content_type {
            return Err("图片附件类型与数据内容不一致。".to_string());
        }
    }
    let bytes = BASE64_STANDARD
        .decode(payload)
        .map_err(|_| "图片附件数据无法解析。".to_string())?;
    if bytes.is_empty() || bytes.len() > CHAT_IMAGE_MAX_BYTES {
        return Err("单张图片不能超过 8 MB。".to_string());
    }
    let valid_signature = match content_type {
        "image/png" => bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
        "image/jpeg" => bytes.starts_with(&[0xff, 0xd8, 0xff]),
        "image/webp" => bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP",
        _ => false,
    };
    if !valid_signature {
        return Err("图片附件内容与声明格式不匹配。".to_string());
    }
    Ok((content_type, bytes))
}

fn persist_chat_image_attachments(
    root: &Path,
    message_id: &str,
    inputs: Vec<ChatImageAttachmentInput>,
) -> Result<Vec<MessageAttachment>, String> {
    if inputs.is_empty() {
        return Ok(Vec::new());
    }
    if inputs.len() > CHAT_IMAGE_MAX_COUNT {
        return Err(format!("一条消息最多附加 {CHAT_IMAGE_MAX_COUNT} 张图片。"));
    }
    let mut total_bytes = 0_usize;
    let mut decoded = Vec::with_capacity(inputs.len());
    for input in inputs {
        let (content_type, bytes) = parse_chat_image_data_url(&input)?;
        total_bytes = total_bytes.saturating_add(bytes.len());
        if total_bytes > CHAT_IMAGE_MAX_TOTAL_BYTES {
            return Err("一条消息的图片附件总大小不能超过 20 MB。".to_string());
        }
        let file_name = safe_chat_file_name(&input.file_name, content_type);
        decoded.push((content_type, file_name, bytes));
    }

    let directory = chat_attachment_dir(root, message_id);
    fs::create_dir_all(&directory).map_err(|err| format!("创建聊天图片附件目录失败：{err}"))?;
    let mut attachments = Vec::with_capacity(decoded.len());
    for (content_type, file_name, bytes) in decoded {
        let attachment_id = new_id();
        let file_path = directory.join(format!("{attachment_id}-{file_name}"));
        fs::write(&file_path, &bytes).map_err(|err| format!("保存聊天图片附件失败：{err}"))?;
        let relative_path = file_path
            .strip_prefix(root)
            .map(path_to_string)
            .unwrap_or_else(|_| path_to_string(&file_path));
        attachments.push(MessageAttachment {
            file_id: attachment_id,
            file_name,
            managed_path: String::new(),
            attachment_type: "image".to_string(),
            content_type: content_type.to_string(),
            relative_path,
        });
    }
    Ok(attachments)
}

fn read_chat_attachment_data(
    root: &Path,
    relative_path: &str,
) -> Result<ChatAttachmentData, String> {
    let root = canonical_project_root(root)?;
    let relative = PathBuf::from(relative_path);
    if relative.is_absolute() || relative_path.trim().is_empty() {
        return Err("聊天附件路径无效。".to_string());
    }
    let path = root.join(relative);
    let canonical = canonical_existing_file(&path)?;
    if !canonical.starts_with(&root) {
        return Err("聊天附件不属于当前项目。".to_string());
    }
    let content_type = canonical
        .extension()
        .and_then(|value| value.to_str())
        .and_then(|value| match value.to_ascii_lowercase().as_str() {
            "png" => Some("image/png"),
            "jpg" | "jpeg" => Some("image/jpeg"),
            "webp" => Some("image/webp"),
            _ => None,
        })
        .ok_or_else(|| "聊天附件格式不受支持。".to_string())?;
    let metadata =
        fs::metadata(&canonical).map_err(|err| format!("读取聊天附件信息失败：{err}"))?;
    if metadata.len() as usize > CHAT_IMAGE_MAX_BYTES {
        return Err("聊天附件超过允许读取大小。".to_string());
    }
    let bytes = fs::read(&canonical).map_err(|err| format!("读取聊天附件失败：{err}"))?;
    let data_url = format!(
        "data:{content_type};base64,{}",
        BASE64_STANDARD.encode(bytes)
    );
    Ok(ChatAttachmentData {
        data_url,
        content_type: content_type.to_string(),
    })
}

pub fn read_chat_attachment(
    project_root: String,
    relative_path: String,
) -> Result<ChatAttachmentData, String> {
    read_chat_attachment_data(Path::new(&project_root), &relative_path)
}

pub async fn run_project_message_stream<R: Runtime>(
    app: tauri::AppHandle<R>,
    project_root: String,
    message_id: String,
    model_id: String,
) -> Result<(), String> {
    let root = PathBuf::from(&project_root);
    let payload = {
        let _guard = project_write_lock()
            .lock()
            .map_err(|_| "项目存储锁已损坏，请重试。".to_string())?;
        let manifest = read_and_repair_manifest(&root)?;
        build_deepseek_context(&manifest, &message_id, &model_id)
    }?;
    let cancel_flag = deepseek::start_stream_guard(&project_root)?;
    let result = deepseek::stream_chat(
        app.clone(),
        project_root.clone(),
        message_id.clone(),
        model_id.clone(),
        payload,
        cancel_flag,
    )
    .await;
    deepseek::finish_stream_guard(&project_root);
    match result {
        Ok(text) => finalize_project_message(&app, &project_root, &message_id, &model_id, &text),
        Err(error) => Err(error),
    }
}

pub fn mark_project_message_failed<R: Runtime>(
    app: &tauri::AppHandle<R>,
    project_root: &str,
    message_id: &str,
    model_id: &str,
    error: &str,
) -> Result<(), String> {
    let root = PathBuf::from(project_root);
    let _guard = project_write_lock()
        .lock()
        .map_err(|_| "项目存储锁已损坏，请重试。".to_string())?;
    let mut manifest = read_and_repair_manifest(&root)?;
    if let Some(message) = manifest
        .messages
        .iter_mut()
        .find(|item| item.id == message_id)
    {
        message.status = if error == "已停止生成。" {
            "cancelled".to_string()
        } else {
            "error".to_string()
        };
        message.text.clear();
        message.source = "deepseek".to_string();
        message.model_id = model_id.to_string();
    }
    persist_project(&root, &manifest, None)?;
    let event = workspace_message_with_status(
        "system",
        "error",
        error.to_string(),
        "deepseek",
        "error",
        model_id,
    );
    append_workspace_event(&root, &event)?;
    manifest.audit.push(audit_event(
        "deepseek.chat",
        project_root,
        "failed",
        error.to_string(),
        false,
        true,
    ));
    append_audit_event(&root, manifest.audit.last().expect("audit event exists"))?;
    let _ = app;
    Ok(())
}

pub fn grant_project_deepseek_authorization<R: Runtime>(
    _app: &tauri::AppHandle<R>,
    project_root: &str,
) -> Result<ProjectManifest, String> {
    let root = PathBuf::from(project_root);
    let _guard = project_write_lock()
        .lock()
        .map_err(|_| "项目存储锁已损坏，请重试。".to_string())?;
    let mut manifest = read_and_repair_manifest(&root)?;
    manifest.deepseek_authorization = DeepSeekAuthorization {
        granted_at: now_string(),
        granted_by: "user".to_string(),
    };
    manifest.audit.push(audit_event(
        "deepseek.authorization",
        project_root,
        "granted",
        "用户已授权发送项目说明、摘要和关键历史到 DeepSeek。".to_string(),
        true,
        true,
    ));
    persist_project(&root, &manifest, None)?;
    append_audit_event(&root, manifest.audit.last().expect("audit event exists"))?;
    Ok(manifest)
}

pub async fn refresh_project_understanding<R: Runtime>(
    app: tauri::AppHandle<R>,
    project_root: String,
) -> Result<ProjectManifest, String> {
    let root = PathBuf::from(&project_root);
    let (model_id, payload) = {
        let _guard = project_write_lock()
            .lock()
            .map_err(|_| "项目存储锁已损坏，请重试。".to_string())?;
        let manifest = read_and_repair_manifest(&root)?;
        ensure_deepseek_authorization(&manifest)?;
        let model_id = deepseek::selected_model_id(&app)?;
        if model_id.trim().is_empty() {
            return Err("请先在设置页保存 DeepSeek API Key 并测试连接。".to_string());
        }
        (model_id, build_project_analysis_context(&manifest)?)
    };

    let analysis_result = deepseek::complete_chat(model_id.clone(), payload).await;
    let _guard = project_write_lock()
        .lock()
        .map_err(|_| "项目存储锁已损坏，请重试。".to_string())?;
    let mut manifest = read_and_repair_manifest(&root)?;
    manifest.project_analysis = match analysis_result {
        Ok(text) => parse_project_analysis_response(&text, &model_id)?,
        Err(error) => ProjectAnalysis {
            status: "failed".to_string(),
            updated_at: now_string(),
            model_id,
            failure_reason: error,
            ..ProjectAnalysis::default()
        },
    };
    manifest.atlas = assess_atlas_read_only(&root, &manifest);
    manifest.project.last_opened_at = now_string();
    manifest.audit.push(audit_event(
        "project.understanding",
        &manifest.project.root_dir,
        &manifest.project_analysis.status,
        if manifest.project_analysis.failure_reason.is_empty() {
            "已更新 project-analysis.json 和 atlas-assessment.json。".to_string()
        } else {
            manifest.project_analysis.failure_reason.clone()
        },
        false,
        true,
    ));
    persist_project(&root, &manifest, None)?;
    append_audit_event(&root, manifest.audit.last().expect("audit event exists"))?;
    Ok(manifest)
}

pub fn save_project_draft(
    project_root: String,
    text: String,
    pending_file_paths: Vec<String>,
) -> Result<WorkspaceDraft, String> {
    let root = PathBuf::from(project_root);
    let _guard = project_write_lock()
        .lock()
        .map_err(|_| "项目存储锁已损坏，请重启感冒院。".to_string())?;
    let mut manifest = read_and_repair_manifest(&root)?;
    manifest.draft = WorkspaceDraft {
        project_id: manifest.project.id.clone(),
        text,
        pending_file_paths,
        updated_at: now_string(),
    };
    persist_project(&root, &manifest, None)?;
    Ok(manifest.draft)
}

pub fn finish_project_work(
    project_root: String,
    completed: String,
    next_step: String,
) -> Result<FinishWorkResult, String> {
    let root = PathBuf::from(project_root);
    let _guard = project_write_lock()
        .lock()
        .map_err(|_| "项目存储锁已损坏，请重启感冒院。".to_string())?;
    let mut manifest = read_and_repair_manifest(&root)?;
    let completed = if completed.trim().is_empty() {
        "推进当前项目工作".to_string()
    } else {
        completed.trim().to_string()
    };
    let next_step = if next_step.trim().is_empty() {
        String::new()
    } else {
        next_step.trim().to_string()
    };
    let recovery = RecoveryPoint {
        id: new_id(),
        project_id: manifest.project.id.clone(),
        completed: completed.clone(),
        next_step: next_step.clone(),
        created_at: now_string(),
        message_count: manifest.messages.len() + 1,
        file_count: manifest.files.len(),
    };
    let message = workspace_message(
        "ganmaoyuan",
        "restore",
        format!("已保存恢复点。今天完成：{completed}。下一步：{next_step}。"),
        "local_rule",
    );
    manifest.project.next_step = next_step;
    manifest.project.last_opened_at = now_string();
    record_message_derivatives(&mut manifest, &message);
    manifest.messages.push(message.clone());
    manifest.recovery_points.push(recovery.clone());
    record_recovery_in_session(&mut manifest, &recovery.id);
    manifest.draft.text.clear();
    manifest.draft.updated_at = now_string();
    manifest.audit.push(audit_event(
        "workspace.finish",
        &manifest.project.id,
        "success",
        format!("恢复点 {}", recovery.id),
        false,
        true,
    ));
    persist_project(&root, &manifest, None)?;
    write_json_atomic(
        &recovery_dir(&root).join(format!("{}.json", recovery.id)),
        &recovery,
    )?;
    append_workspace_event(&root, &message)?;
    append_audit_event(&root, manifest.audit.last().expect("audit event exists"))?;
    Ok(FinishWorkResult {
        project: manifest.project,
        message,
        recovery_point: recovery,
    })
}

pub fn generate_codex_prompt(project_root: String) -> Result<CodexPromptResult, String> {
    let root = PathBuf::from(&project_root);
    ensure_project_dirs(&root)?;
    let snapshot = load_project(&project_root)?;
    let task_id = new_id();
    let related_task = active_task_title(&snapshot);
    let task_type =
        infer_codex_task_type(&format!("{}\n{}", related_task, snapshot.project.next_step));
    let expected_result_path = canonical_codex_result_path(&root, &task_id);
    let prompt_text = build_codex_prompt(
        &snapshot,
        &task_id,
        &task_type,
        &path_to_string(&expected_result_path),
    );
    let generated = register_generated_file(
        project_root.clone(),
        format!("codex-task-{task_id}.md"),
        prompt_text.clone(),
        "Ganmaoyuan".to_string(),
        related_task.clone(),
        "Codex prompt".to_string(),
    )?;

    let _guard = project_write_lock()
        .lock()
        .map_err(|_| "项目存储锁已损坏，请重启感冒院。".to_string())?;
    let mut manifest = read_and_repair_manifest(&root)?;
    let prompt = CodexPromptRecord {
        id: task_id,
        title: first_line(&prompt_text, 80),
        related_task,
        prompt_text,
        file_id: generated.file.id.clone(),
        managed_path: generated.file.managed_path.clone(),
        content_hash: generated.file.content_hash.clone(),
        created_at: now_string(),
    };
    manifest.codex_prompts.push(prompt.clone());
    persist_project(&root, &manifest, None)?;
    let repository_path = preferred_project_repository_path(&root, &manifest);
    let task = CodexTask {
        task_id: prompt.id.clone(),
        project_id: manifest.project.id.clone(),
        title: prompt.related_task.clone(),
        task_type,
        prompt: prompt.prompt_text.clone(),
        created_at: prompt.created_at.clone(),
        status: "ready".to_string(),
        repository_path,
        expected_result_path: path_to_string(&expected_result_path),
        evidence_refs: vec![
            format!("codexPrompt:{}", prompt.id),
            format!("generatedFile:{}", prompt.file_id),
        ],
        updated_at: prompt.created_at.clone(),
        ..CodexTask::default()
    };
    write_codex_task(&root, &task)?;
    Ok(CodexPromptResult { prompt, manifest })
}

pub fn create_codex_task(
    project_root: String,
    request: CodexTaskCreateRequest,
) -> Result<CodexTask, String> {
    let root = PathBuf::from(&project_root);
    ensure_project_dirs(&root)?;
    let snapshot = load_project(&project_root)?;
    let title = request.title.trim();
    let instructions = request.instructions.trim();
    if title.is_empty() {
        return Err("请填写 Codex 任务标题。".to_string());
    }
    if title.chars().count() > 160 {
        return Err("Codex 任务标题不能超过 160 个字符。".to_string());
    }
    if instructions.is_empty() {
        return Err("请填写 Codex 要执行的任务说明。".to_string());
    }
    if instructions.chars().count() > 8_000 || instructions.contains('\0') {
        return Err("Codex 任务说明无效或过长。".to_string());
    }
    let task_type = normalize_codex_task_type(&request.task_type);
    if !matches!(
        task_type.as_str(),
        "analysis" | "coding" | "verification" | "fileOperation"
    ) {
        return Err(
            "Codex 任务类型必须是 analysis、coding、verification 或 fileOperation。".to_string(),
        );
    }

    let task_id = new_id();
    let expected_result_path = canonical_codex_result_path(&root, &task_id);
    let created_at = now_string();
    let repository_path = preferred_project_repository_path(&root, &snapshot);
    let task = CodexTask {
        task_id: task_id.clone(),
        project_id: snapshot.project.id,
        title: title.to_string(),
        task_type: task_type.clone(),
        prompt: build_explicit_codex_task_prompt(
            &task_id,
            &task_type,
            title,
            instructions,
            &path_to_string(&expected_result_path),
        ),
        created_at: created_at.clone(),
        status: "ready".to_string(),
        repository_path,
        expected_result_path: path_to_string(&expected_result_path),
        evidence_refs: vec!["taskOrigin:explicit".to_string()],
        updated_at: created_at,
        ..CodexTask::default()
    };
    validate_codex_task_prompt_binding(&task)?;
    write_codex_task(&root, &task)?;
    Ok(task)
}

pub fn list_codex_tasks(project_root: String) -> Result<CodexTaskList, String> {
    let root = PathBuf::from(project_root);
    // The bridge performs restart recovery before reading result files so a late, real
    // result can advance the same task instead of being turned into a failed retry.
    scan_codex_result_bridge(path_to_string(&root))?;
    repair_codex_run_failure_messages(&root)?;
    sync_failed_codex_runs_to_tasks(&root)?;
    Ok(CodexTaskList {
        tasks: read_codex_tasks(&root)?,
        runs: read_codex_runs(&root)?,
    })
}

pub fn rebind_codex_task_prompt(
    project_root: String,
    task_id: String,
) -> Result<CodexTask, String> {
    let root = PathBuf::from(&project_root);
    ensure_project_dirs(&root)?;
    let snapshot = load_project(&project_root)?;
    let _guard = project_write_lock()
        .lock()
        .map_err(|_| "项目存储锁已损坏，请重启感冒院。".to_string())?;
    let mut task = read_codex_task(&root, &task_id)?;
    if matches!(
        task.status.as_str(),
        "resultReceived" | "gitVerified" | "awaitingAcceptance" | "completed"
    ) {
        return Err("该任务已经收到结果，不能重新绑定提示词。".to_string());
    }

    task.task_type = normalize_codex_task_type(&task.task_type);
    if task.task_type.trim().is_empty() || task.task_type == "unknown" {
        task.task_type =
            infer_codex_task_type(&format!("{}\n{}", task.title, snapshot.project.next_step));
    }
    task.expected_result_path = path_to_string(&canonical_codex_result_path(&root, &task.task_id));
    task.prompt = build_codex_prompt(
        &snapshot,
        &task.task_id,
        &task.task_type,
        &task.expected_result_path,
    );
    task.status = "ready".to_string();
    task.updated_at = now_string();
    validate_codex_task_prompt_binding(&task)?;
    write_codex_task(&root, &task)?;
    Ok(task)
}

pub fn start_codex_task_run(project_root: String, task_id: String) -> Result<CodexRun, String> {
    let root = PathBuf::from(&project_root);
    ensure_project_dirs(&root)?;
    recover_stale_codex_runs(&root)?;
    let _guard = project_write_lock()
        .lock()
        .map_err(|_| "项目存储锁已损坏，请重启感冒院。".to_string())?;
    let mut task = read_codex_task(&root, &task_id)?;
    if task.prompt.trim().is_empty() {
        return Err("CodexTask 没有可执行提示词，请先生成提示词。".to_string());
    }
    if !matches!(
        task.status.as_str(),
        "ready" | "draft" | "handedOff" | "awaitingResult" | "failed" | "needsReview" | "running"
    ) {
        return Err("当前任务状态不允许启动，请刷新任务或先完成验收。".to_string());
    }
    validate_codex_task_prompt_binding(&task)?;
    let repository_path = codex_task_repository_path(&root, &task)?;
    if !repository_path.exists() {
        return Err(format!(
            "关联 Git 仓库不存在：{}",
            path_to_string(&repository_path)
        ));
    }
    if let Some(active) = active_codex_run_for_task(&root, &task_id)? {
        return Err(format!(
            "该 CodexTask 已有运行中的本机 Codex 进程：pid {}。",
            active.pid
        ));
    }
    let run_id = new_id();
    let (git_head_before, git_worktree_status_before, git_dirty_files_before) =
        capture_codex_run_git_baseline(&repository_path, &task)?;
    let started_at = now_string();
    let output_last_message_path = codex_run_output_path(&root, &run_id);
    let result_directory = codex_task_result_directory(&root, &task)?;
    if let Some(parent) = output_last_message_path.parent() {
        fs::create_dir_all(parent).map_err(|err| format!("创建 CodexRun 输出目录失败：{err}"))?;
    }
    let executable = codex_command_name();
    let run = CodexRun {
        id: run_id.clone(),
        task_id: task.task_id.clone(),
        project_id: task.project_id.clone(),
        repository_path: path_to_string(&repository_path),
        command_executable: executable.clone(),
        status: "starting".to_string(),
        working_directory: path_to_string(&repository_path),
        git_head_before,
        git_worktree_status_before,
        git_dirty_files_before,
        stdin_prompt_summary: first_line(&task.prompt, 320),
        started_at: started_at.clone(),
        last_activity_at: started_at.clone(),
        recent_activity: vec!["已创建 CodexRun，等待 Codex CLI 启动。".to_string()],
        output_last_message_path: path_to_string(&output_last_message_path),
        ..CodexRun::default()
    };
    let run_prompt = codex_prompt_for_run(&task, &run);
    upsert_codex_run(&root, run.clone())?;
    if matches!(
        task.status.as_str(),
        "ready" | "draft" | "awaitingResult" | "failed" | "needsReview"
    ) {
        task.status = "running".to_string();
        task.handed_off_at = started_at.clone();
        task.updated_at = started_at;
        write_codex_task(&root, &task)?;
    }
    let root_for_thread = root.clone();
    let run_for_thread = run.clone();
    let task_type_for_thread = task.task_type.clone();
    let repository_for_thread = repository_path.clone();
    let result_directory_for_thread = result_directory.clone();
    let output_for_thread = output_last_message_path.clone();
    std::thread::spawn(move || {
        let capability_probe = probe_codex_cli_capabilities(&run_for_thread.command_executable);
        if !capability_probe.available {
            mark_codex_run_start_failed(
                &root_for_thread,
                &run_for_thread,
                format!(
                    "Codex CLI 不可用：{}",
                    empty_label(&capability_probe.failure_reason, "未找到 codex 命令")
                ),
            );
            return;
        }
        let args = match codex_exec_args(
            &repository_for_thread,
            &result_directory_for_thread,
            &output_for_thread,
            &capability_probe,
            is_native_codex_executable(&run_for_thread.command_executable),
        ) {
            Ok(args) => args,
            Err(error) => {
                mark_codex_run_start_failed(&root_for_thread, &run_for_thread, error);
                return;
            }
        };
        if read_codex_run(&root_for_thread, &run_for_thread.id)
            .map(|current| current.status == "cancelled")
            .unwrap_or(false)
        {
            return;
        }
        let mut child = match hidden_command(&run_for_thread.command_executable)
            .args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
        {
            Ok(child) => child,
            Err(error) => {
                mark_codex_run_start_failed(
                    &root_for_thread,
                    &run_for_thread,
                    format!("启动 codex exec 失败，请确认 Codex CLI 已安装并登录：{error}"),
                );
                return;
            }
        };
        let pid = child.id();
        if let Some(mut stdin) = child.stdin.take() {
            if let Err(error) = stdin.write_all(run_prompt.as_bytes()) {
                let _ = child.kill();
                mark_codex_run_start_failed(
                    &root_for_thread,
                    &run_for_thread,
                    format!("写入 Codex prompt 失败：{error}"),
                );
                return;
            }
        }
        let mut running = read_codex_run(&root_for_thread, &run_for_thread.id)
            .unwrap_or_else(|_| run_for_thread.clone());
        if running.status == "cancelled" {
            let _ = child.kill();
            return;
        }
        running.command_args = args;
        running.capability_probe = capability_probe;
        running.pid = pid;
        running.status = "running".to_string();
        running.process_alive = true;
        running.last_activity_at = now_string();
        running.recent_activity = vec!["Codex CLI 进程已启动。".to_string()];
        let _ = upsert_codex_run(&root_for_thread, running.clone());
        let output = wait_for_codex_process(
            &mut child,
            pid,
            &root_for_thread,
            &run_for_thread.task_id,
            &run_for_thread,
            &task_type_for_thread,
        );
        let real_result_received = output
            .as_ref()
            .map(|output| output.result_received)
            .unwrap_or(false);
        let mut next = read_codex_run(&root_for_thread, &run_for_thread.id).unwrap_or(running);
        if next.status == "cancelled" {
            return;
        }
        next.ended_at = now_string();
        next.process_alive = false;
        match output {
            Ok(output) => {
                next.exit_code = output.exit_code;
                next.stdout_summary = summarize_process_output(&output.stdout);
                next.stderr_summary = summarize_process_output(&output.stderr);
                next.last_activity_at = output.last_activity_at;
                next.recent_activity = output.recent_activity;
                next.status = if output.result_received {
                    if output.stopped_after_result {
                        next.error = "已收到本次真实 Codex 结果；Codex CLI 未在宽限期内自行退出，已停止残留进程。"
                            .to_string();
                    }
                    "exited".to_string()
                } else if output.timed_out {
                    next.error = codex_timeout_error(&task_type_for_thread);
                    "failed".to_string()
                } else if output.exit_code == Some(0) {
                    "exited".to_string()
                } else {
                    "failed".to_string()
                };
                if !output.result_received && !output.timed_out && output.exit_code != Some(0) {
                    next.error = codex_run_failure_reason(
                        output.exit_code,
                        &next.stderr_summary,
                        &next.stdout_summary,
                    );
                }
            }
            Err(err) => {
                next.status = "failed".to_string();
                next.error = format!("等待 codex exec 结束失败：{err}");
            }
        }
        let successful_exit =
            real_result_received || (next.status == "exited" && next.exit_code == Some(0));
        let failed_reason = if next.status == "failed" {
            Some(next.error.clone())
        } else {
            None
        };
        let _ = upsert_codex_run(&root_for_thread, next.clone());
        if read_codex_runs(&root_for_thread)
            .ok()
            .and_then(|runs| {
                runs.into_iter()
                    .find(|run| run.task_id == run_for_thread.task_id)
            })
            .map(|run| run.id != run_for_thread.id)
            .unwrap_or(true)
        {
            return;
        }
        if let Some(reason) = failed_reason {
            if is_codex_timeout_error(&next.error) {
                let _ = mark_codex_task_restart_recovery_expired(
                    &root_for_thread,
                    &run_for_thread.task_id,
                    &reason,
                );
            } else {
                let _ =
                    mark_codex_task_run_failed(&root_for_thread, &run_for_thread.task_id, &reason);
            }
        }
        if successful_exit && !real_result_received {
            if let Err(error) = finish_codex_result_handoff(&root_for_thread, &next) {
                next.error = error;
                let _ = upsert_codex_run(&root_for_thread, next);
            }
        } else if !real_result_received {
            let _ = scan_codex_result_bridge(path_to_string(&root_for_thread));
        }
    });
    Ok(run)
}

fn mark_codex_run_start_failed(root: &Path, run: &CodexRun, reason: String) {
    let mut failed = read_codex_run(root, &run.id).unwrap_or_else(|_| run.clone());
    if failed.status == "cancelled" {
        return;
    }
    failed.status = "failed".to_string();
    failed.ended_at = now_string();
    failed.error = reason.clone();
    let _ = upsert_codex_run(root, failed);
    let _ = mark_codex_task_run_failed(root, &run.task_id, &reason);
}

fn finish_codex_result_handoff(root: &Path, run: &CodexRun) -> Result<(), String> {
    let result = (|| {
        mark_codex_task_awaiting_result(root, &run.task_id)?;
        wait_for_matching_codex_result(root, &run.task_id, run, true)?;
        scan_codex_result_bridge(path_to_string(root))?;
        Ok(())
    })();
    result.map_err(|error: String| {
        let reason = format!("Codex 已退出，但结果回流失败：{error}");
        // A successful process is not a completed task. Never manufacture a result
        // from stdout; keep the missing handoff actionable until a real result arrives.
        let _ = mark_codex_task_missing_result(root, &run.task_id, &reason);
        reason
    })
}

fn mark_codex_task_missing_result(root: &Path, task_id: &str, reason: &str) -> Result<(), String> {
    let mut task = read_codex_task(root, task_id)?;
    if matches!(
        task.status.as_str(),
        "resultReceived" | "gitVerified" | "awaitingAcceptance" | "completed" | "cancelled"
    ) {
        return Ok(());
    }
    task.status = "needsReview".to_string();
    task.updated_at = now_string();
    if !task
        .remaining_issues
        .iter()
        .any(|issue| issue.trim() == reason.trim())
    {
        task.remaining_issues.push(reason.to_string());
    }
    write_codex_task(root, &task)
}

fn capture_codex_run_git_baseline(
    repository_path: &Path,
    task: &CodexTask,
) -> Result<(String, String, Vec<GitChangedFile>), String> {
    if !codex_task_git_required(task) {
        return Ok((String::new(), String::new(), Vec::new()));
    }
    let head = run_git(repository_path, &["rev-parse", "HEAD"]).map_err(|error| {
        format!("coding 任务必须在可用 Git 仓库中启动，无法读取任务启动前 HEAD：{error}")
    })?;
    let status = run_git(repository_path, &["status", "--porcelain"])
        .map_err(|error| format!("无法读取 coding 任务启动前 Git 工作区状态：{error}"))?;
    let diff = run_git(repository_path, &["diff", "--name-status"]).unwrap_or_default();
    let dirty_files = parse_git_changed_files(&status, &diff);
    Ok((
        head.trim().to_string(),
        if dirty_files.is_empty() {
            "clean".to_string()
        } else {
            "dirty".to_string()
        },
        dirty_files,
    ))
}

fn codex_prompt_for_run(task: &CodexTask, run: &CodexRun) -> String {
    let file_operation_guardrail = if task.task_type == "fileOperation" {
        "\n执行约束：这是一个文件操作任务。对于文本或 JSON 的创建、修改与删除，优先使用 Codex 专用 apply_patch 文件工具；不要使用 terminal、shell、PowerShell 或 cmd。只有任务明确需要执行程序时，才说明无法完成的原因，不要伪造执行结果。\n"
    } else {
        ""
    };
    // The native Windows Codex sandbox can hang when the agent selects PowerShell.
    // Keep file operations on apply_patch; every other task must use cmd.exe explicitly.
    let windows_command_guardrail = if cfg!(target_os = "windows")
        && task.task_type != "fileOperation"
    {
        "\nWindows 执行约束：如果需要读取文件、运行测试或调用 Git，必须显式选择 C:\\Windows\\System32\\cmd.exe 作为命令 shell，并使用 /d /s /c。不要使用 PowerShell、pwsh 或任何 PowerShell profile；不要把 cmd.exe 再包进带引号的嵌套 shell 命令。\n"
    } else {
        ""
    };
    let coding_commit_contract = if codex_task_git_required(task) {
        codex_coding_commit_contract(run)
    } else {
        String::new()
    };
    format!(
        "{}{}{}{}\n本次执行结果绑定：resultRunId: {run_id}\n写入结果 JSON 时必须包含 resultRunId=\"{run_id}\"；Markdown 结果必须包含独立一行 resultRunId: {run_id}。这是本次执行的标识，不要复用旧结果中的执行标识。taskId 仍为 {task_id}，结果文件仍写入 {expected_result_path}。\n",
        task.prompt,
        file_operation_guardrail,
        windows_command_guardrail,
        coding_commit_contract,
        run_id = run.id,
        task_id = task.task_id,
        expected_result_path = task.expected_result_path,
    )
}

fn codex_coding_commit_contract(run: &CodexRun) -> String {
    let dirty_paths = run
        .git_dirty_files_before
        .iter()
        .map(|file| file.path.trim())
        .filter(|path| !path.is_empty())
        .collect::<Vec<_>>();
    let baseline = if dirty_paths.is_empty() {
        "任务启动前工作区干净。".to_string()
    } else {
        format!(
            "任务启动前已有未提交修改（禁止触碰、暂存或提交）：{}。",
            dirty_paths.join("、")
        )
    };
    format!(
        "\nCoding Git 提交约束：本任务启动前 HEAD 为 {head}。{baseline}\n完成代码修改后，先运行本任务必要测试。只有测试通过、且能明确区分本任务修改时，才可创建 commit。必须只用 `git add -- <本任务文件路径>` 精确暂存；禁止 `git add .`、`git add -A`、`git commit -a`，禁止提交任务启动前已有的修改或无关文件。提交后运行 `git rev-parse HEAD`。\n如果测试失败、没有修改、修改与启动前脏文件重叠，或无法安全判断归属：不要提交、不要伪造 commit 或 Git 核验；结果写 status=\"needsReview\" 并在 remainingIssues 说明原因。\n结果 JSON 的 `coding` 对象必须如实包含 `changedFiles`、`commits`（完整 commit SHA）、`tests`、`gitHead`、`gitStatus`、`gitModifiedFiles`。\n",
        head = empty_label(&run.git_head_before, "未知"),
        baseline = baseline,
    )
}

fn validate_codex_task_prompt_binding(task: &CodexTask) -> Result<(), String> {
    let first_line = task.prompt.lines().next().unwrap_or_default().trim();
    let expected_task_headers = [
        format!("taskId：{}", task.task_id),
        format!("taskId: {}", task.task_id),
        format!("taskId:{}", task.task_id),
    ];
    if !expected_task_headers
        .iter()
        .any(|expected| first_line == expected)
    {
        return Err(format!(
            "CodexTask 提示词与当前 taskId 不一致，请先重新绑定提示词。当前任务：{}。",
            task.task_id
        ));
    }

    let expected_json_name = format!("{}-result.json", task.task_id);
    let expected_markdown_name = format!("{}-result.md", task.task_id);
    if !task.expected_result_path.contains(&expected_json_name)
        || (!task.prompt.contains(&expected_json_name)
            && !task.prompt.contains(&expected_markdown_name))
    {
        return Err(format!(
            "CodexTask 结果路径与当前 taskId 不一致，请先重新绑定提示词。当前任务：{}。",
            task.task_id
        ));
    }
    Ok(())
}

fn codex_task_result_directory(root: &Path, task: &CodexTask) -> Result<PathBuf, String> {
    let expected_directory = codex_result_bridge_dir(root);
    let expected_path = codex_result_path_for_task(root, task, "json");
    let expected_name = format!("{}-result.json", task.task_id);
    fs::create_dir_all(&expected_directory)
        .map_err(|err| format!("创建 Codex Result Bridge 目录失败：{err}"))?;
    let canonical_expected_directory = expected_directory.canonicalize().map_err(|err| {
        format!(
            "规范化 Codex Result Bridge 目录失败：{}：{err}",
            path_to_string(&expected_directory)
        )
    })?;
    let parent = expected_path
        .parent()
        .ok_or_else(|| "CodexTask 结果路径缺少目录。".to_string())?;
    let same_directory = parent
        .canonicalize()
        .map(|directory| directory == canonical_expected_directory)
        .unwrap_or(false);
    if expected_path.file_name().and_then(|name| name.to_str()) != Some(expected_name.as_str())
        || !same_directory
    {
        return Err("CodexTask 结果路径必须位于当前项目的 Result Bridge 目录。".to_string());
    }
    Ok(expected_directory)
}

pub fn cancel_codex_task_run(project_root: String, task_id: String) -> Result<CodexRun, String> {
    let root = PathBuf::from(project_root);
    let mut run = active_codex_run_for_task(&root, &task_id)?
        .ok_or_else(|| "没有正在运行的 Codex 进程。".to_string())?;
    if run.pid != 0 {
        terminate_process_if_running(run.pid)?;
    }
    run.status = "cancelled".to_string();
    run.process_alive = false;
    run.ended_at = now_string();
    run.error = "已停止 Codex 进程，仓库可能存在未提交修改，请查看 Git 事实。".to_string();
    upsert_codex_run(&root, run.clone())?;
    if let Ok(mut task) = read_codex_task(&root, &task_id) {
        if !matches!(
            task.status.as_str(),
            "resultReceived" | "awaitingAcceptance" | "completed"
        ) {
            task.status = "cancelled".to_string();
            task.updated_at = now_string();
            let _ = write_codex_task(&root, &task);
        }
    }
    Ok(run)
}

pub fn mark_codex_task_handed_off(
    project_root: String,
    task_id: String,
) -> Result<CodexTask, String> {
    let root = PathBuf::from(&project_root);
    let mut task = read_codex_task(&root, &task_id)?;
    if task.status == "ready" || task.status == "draft" {
        task.status = "awaitingResult".to_string();
        task.handed_off_at = now_string();
        task.updated_at = task.handed_off_at.clone();
        write_codex_task(&root, &task)?;
    }
    Ok(task)
}

pub fn accept_codex_task(
    project_root: String,
    task_id: String,
    expected_result_id: String,
) -> Result<CodexTask, String> {
    review_codex_task(project_root, task_id, expected_result_id, true, None)
}

pub fn reject_codex_task(
    project_root: String,
    task_id: String,
    expected_result_id: String,
    reason: Option<String>,
) -> Result<CodexTask, String> {
    review_codex_task(project_root, task_id, expected_result_id, false, reason)
}

fn review_codex_task(
    project_root: String,
    task_id: String,
    expected_result_id: String,
    approved: bool,
    reason: Option<String>,
) -> Result<CodexTask, String> {
    let _guard = project_write_lock()
        .lock()
        .map_err(|_| "项目存储锁已损坏，请重启感冒院。".to_string())?;
    let root = PathBuf::from(&project_root);
    let manifest = read_and_repair_manifest(&root)?;
    let mut task = read_codex_task(&root, &task_id)?;
    if task.project_id != manifest.project.id || task.task_id != task_id {
        return Err("任务不属于当前项目，不能验收。".to_string());
    }
    if expected_result_id.is_empty() || task.result_id != expected_result_id {
        return Err("任务结果已变化或尚未收到，请刷新后查看最新结果再验收。".to_string());
    }
    let report = manifest
        .codex_reports
        .iter()
        .find(|report| report.id == expected_result_id)
        .ok_or_else(|| "找不到待验收的结果证据，请刷新后重试。".to_string())?;
    let raw = fs::read_to_string(&report.evidence_managed_path)
        .map_err(|_| "结果证据无法读取，请恢复结果文件后再验收。".to_string())?;
    let evidence_text = if report.source_kind == "bridge" {
        let extension = Path::new(&report.evidence_managed_path)
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        normalize_codex_bridge_result_text(&raw, &extension)
    } else {
        raw.clone()
    };
    if hash_text(&evidence_text) != report.report_hash {
        return Err("结果文件已变化，请重新检查结果后再验收。".to_string());
    }
    let contract = codex_result_contract_from_text(report, &raw);
    if contract.task_id != task_id {
        return Err("结果与当前任务不匹配，不能验收。".to_string());
    }
    if !task.result_run_id.is_empty()
        && !contract.result_run_id.is_empty()
        && task.result_run_id != contract.result_run_id
    {
        return Err("结果对应的执行记录已变化，不能验收。".to_string());
    }
    let expected_run_id = if !task.result_run_id.is_empty() {
        task.result_run_id.clone()
    } else {
        contract.result_run_id.clone()
    };
    let runs = read_codex_runs(&root)?;
    let matching_run = if expected_run_id.is_empty() {
        runs.iter()
            .filter(|run| run.task_id == task_id)
            .max_by(|left, right| left.started_at.cmp(&right.started_at))
    } else {
        runs.iter()
            .find(|run| run.id == expected_run_id && run.task_id == task_id)
    };
    if !expected_run_id.is_empty() && matching_run.is_none() {
        return Err("当前结果没有匹配的执行记录，请刷新后重试。".to_string());
    }
    if let Some(run) = matching_run {
        if matches!(
            run.status.as_str(),
            "starting" | "running" | "failed" | "cancelled"
        ) || (run.exit_code.map(|code| code != 0).unwrap_or(false)
            && !codex_run_finished_with_bound_result(run, Path::new(&report.evidence_managed_path)))
            || !codex_result_file_matches_run(Path::new(&report.evidence_managed_path), run)
        {
            return Err("当前执行尚未成功结束，或结果属于旧执行，请刷新后检查。".to_string());
        }
    }
    let decision = if approved { "approved" } else { "rejected" };
    let final_status = if approved { "completed" } else { "failed" };
    if task.status == final_status && task.acceptance.status == decision {
        return Ok(task);
    }
    if !matches!(task.status.as_str(), "awaitingAcceptance" | "needsReview") {
        return Err("当前任务不处于可人工验收状态，请刷新任务。".to_string());
    }
    let original = task.clone();
    if approved {
        if !contract.status.eq_ignore_ascii_case("completed")
            || contract.result_source == "runnerFallback"
            || task.result_source == "runnerFallback"
            || report.parse_status != "ready"
            || !codex_result_has_useful_content(&task)
        {
            return Err("结果尚未证明任务完成，不能通过验收。".to_string());
        }
        verify_codex_task_against_git(&root, &mut task);
        if codex_task_git_required(&task) && task.git_verification.status != "verified" {
            return Err("代码事实尚未通过 Git 核验，请检查提交与关联仓库。".to_string());
        }
    }
    let reason = reason
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(if approved {
            "用户人工验收通过。"
        } else {
            "用户人工验收未通过。"
        })
        .to_string();
    task.status = final_status.to_string();
    task.updated_at = now_string();
    task.completed_at = if approved {
        task.updated_at.clone()
    } else {
        String::new()
    };
    task.acceptance = CodexTaskAcceptance {
        status: decision.to_string(),
        reason: reason.clone(),
        decided_at: task.updated_at.clone(),
    };
    if !approved && !task.remaining_issues.iter().any(|item| item == &reason) {
        task.remaining_issues.push(reason.clone());
    }
    write_codex_task(&root, &task)?;
    if let Err(error) = append_work_event_if_new(
        &root,
        fact_event(
            &manifest.project.id,
            "codex",
            &format!(
                "{}:{}:{}",
                if approved { "accept" } else { "reject" },
                task.task_id,
                task.result_id
            ),
            if approved {
                "codex.taskAccepted"
            } else {
                "codex.taskRejected"
            },
            format!(
                "Codex 任务人工验收{}：{}",
                if approved { "通过" } else { "未通过" },
                task_display_title(&task)
            ),
            vec![
                format!("codexTask:{}", task.task_id),
                format!("codexReport:{}", task.result_id),
                format!("reason:{}", reason),
            ],
            100,
        ),
    ) {
        write_codex_task(&root, &original)
            .map_err(|rollback| format!("验收事实保存失败：{error}；恢复任务失败：{rollback}"))?;
        // Atomic writes retain the previous value in .bak; restore that too.
        write_codex_task(&root, &original).map_err(|rollback| {
            format!("验收事实保存失败：{error}；恢复任务备份失败：{rollback}")
        })?;
        return Err(format!("验收事实保存失败，已恢复待验收状态：{error}"));
    }
    Ok(task)
}

pub fn import_codex_report_text(
    project_root: String,
    report_text: String,
) -> Result<CodexReportImportResult, String> {
    let text = report_text.trim().to_string();
    if text.is_empty() {
        return Err("Codex report text cannot be empty".to_string());
    }
    let report_hash = hash_text(&text);
    let root = PathBuf::from(&project_root);
    if let Some(existing) = find_codex_report(&load_project(&project_root)?, &report_hash) {
        return Ok(CodexReportImportResult {
            report: existing,
            manifest: load_project(&project_root)?,
            duplicate: true,
        });
    }
    let related_task = active_task_title(&load_project(&project_root)?);
    let evidence = register_generated_file(
        project_root.clone(),
        format!("codex-report-{}.md", now_string()),
        text.clone(),
        "Codex".to_string(),
        related_task,
        "Codex report evidence".to_string(),
    )?;
    persist_codex_report_import(
        &root,
        text,
        report_hash,
        "pasted".to_string(),
        "pasted report text".to_string(),
        evidence.file,
    )
}

pub fn import_codex_report_file(
    project_root: String,
    report_file_path: String,
) -> Result<CodexReportImportResult, String> {
    let source = canonical_existing_file(Path::new(&report_file_path))?;
    let extension = source
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if !matches!(extension.as_str(), "txt" | "md") {
        return Err("Codex report file must be txt or md".to_string());
    }
    let text = read_text_report_file(&source)?;
    if text.trim().is_empty() {
        return Err("Codex report file is empty".to_string());
    }
    let report_hash = hash_text(&text);
    if let Some(existing) = find_codex_report(&load_project(&project_root)?, &report_hash) {
        return Ok(CodexReportImportResult {
            report: existing,
            manifest: load_project(&project_root)?,
            duplicate: true,
        });
    }
    let import = import_files(
        project_root.clone(),
        vec![path_to_string(&source)],
        "Codex report import".to_string(),
    )?;
    let source_path = path_to_string(&source);
    let evidence = import
        .files
        .into_iter()
        .find(|file| normalize_key(&file.original_source_path) == normalize_key(&source_path))
        .ok_or_else(|| "failed to register Codex report evidence file".to_string())?;
    persist_codex_report_import(
        &PathBuf::from(&project_root),
        text,
        report_hash,
        "file".to_string(),
        source
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("codex-report")
            .to_string(),
        evidence,
    )
}

pub fn scan_codex_result_bridge(
    project_root: String,
) -> Result<CodexResultBridgeScanResult, String> {
    let root = PathBuf::from(&project_root);
    ensure_project_dirs(&root)?;
    // This command also backs the background bridge poll after restart, so it must
    // keep incomplete runs recoverable even when no WorkPage is currently open.
    recover_stale_codex_runs(&root)?;
    let result_dir = codex_result_bridge_dir(&root);
    let mut entries = fs::read_dir(&result_dir)
        .map_err(|err| {
            format!(
                "读取 Codex 结果目录失败：{}：{err}",
                path_to_string(&result_dir)
            )
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| format!("读取 Codex 结果文件失败：{err}"))?;
    entries.sort_by_key(|entry| entry.path());

    let mut scanned_count = 0usize;
    let mut imported_reports = Vec::new();
    for entry in entries {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let extension = path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if !matches!(extension.as_str(), "json" | "md") {
            continue;
        }
        scanned_count += 1;
        let raw_text = read_text_report_file(&path)?;
        if raw_text.trim().is_empty() {
            continue;
        }
        if let Some(task_id) = path
            .file_stem()
            .and_then(|name| name.to_str())
            .and_then(|name| name.strip_suffix("-result"))
        {
            if let Some(run) = read_codex_runs(&root)?
                .into_iter()
                .find(|run| run.task_id == task_id)
            {
                if !codex_result_text_matches_run(&path, &raw_text, &run) {
                    continue;
                }
            }
        }
        let report_text = normalize_codex_bridge_result_text(&raw_text, &extension);
        let report_hash = hash_text(&report_text);
        let manifest = read_and_repair_manifest(&root)?;
        if let Some(existing) = find_codex_report(&manifest, &report_hash) {
            let _guard = project_write_lock()
                .lock()
                .map_err(|_| "项目存储锁已损坏，请重启感冒院。".to_string())?;
            let _ = upsert_codex_task_from_report(&root, &existing)?;
            continue;
        }
        let evidence = bridge_result_evidence_file(&root, &path, &raw_text)?;
        let import = persist_codex_report_import(
            &root,
            report_text,
            report_hash,
            "bridge".to_string(),
            path.file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("codex-result")
                .to_string(),
            evidence,
        )?;
        if !import.duplicate {
            imported_reports.push(import.report);
        }
    }

    Ok(CodexResultBridgeScanResult {
        result_dir: path_to_string(&result_dir),
        scanned_count,
        imported_count: imported_reports.len(),
        reports: imported_reports,
        manifest: read_and_repair_manifest(&root)?,
    })
}

pub fn apply_codex_report(
    project_root: String,
    report_id: String,
) -> Result<CodexReportApplyResult, String> {
    let root = PathBuf::from(&project_root);
    let _guard = project_write_lock()
        .lock()
        .map_err(|_| "项目存储锁已损坏，请重启感冒院。".to_string())?;
    let mut manifest = read_and_repair_manifest(&root)?;
    let report_index = manifest
        .codex_reports
        .iter()
        .position(|report| report.id == report_id)
        .ok_or_else(|| "Codex report record not found".to_string())?;
    if manifest.codex_reports[report_index].applied {
        return Ok(CodexReportApplyResult {
            report: manifest.codex_reports[report_index].clone(),
            manifest,
        });
    }
    if manifest.codex_reports[report_index].parse_status != "ready"
        || manifest.codex_reports[report_index].completed_status == "uncertain"
    {
        return Err("Codex report still needs manual review before applying".to_string());
    }

    let report = manifest.codex_reports[report_index].clone();
    let now = now_string();
    let summary = if !report.summary.trim().is_empty() {
        report.summary.clone()
    } else {
        first_line(&report.modified_content.join("; "), 160)
    };
    manifest.codex_reports[report_index].applied = true;
    manifest.codex_reports[report_index].confirmed_at = now;
    persist_project(&root, &manifest, None)?;
    let _ = append_work_event_if_new(
        &root,
        fact_event(
            &manifest.project.id,
            "codex",
            &format!("apply:{}", report.report_hash),
            "codex.reportAccepted",
            format!("已确认 Codex 结果：{}", summary),
            vec![
                format!("codexReport:{}", report.id),
                format!("evidence:{}", report.evidence_managed_path),
            ],
            94,
        ),
    )?;
    Ok(CodexReportApplyResult {
        report: manifest.codex_reports[report_index].clone(),
        manifest,
    })
}

pub fn get_work_ledger(project_root: String) -> Result<WorkLedgerSnapshot, String> {
    let root = PathBuf::from(project_root);
    let ledger = read_work_ledger(&root)?;
    let git_snapshot = read_git_snapshot(&root)?;
    let codex_results = read_codex_external_results(&root)?.results;
    let codex_tasks = read_codex_tasks(&root)?;
    let codex_runs = read_codex_runs(&root)?;
    let manifest = read_and_repair_manifest(&root)?;
    let activity_timeline = ActivityProjectionAdapter::visible_timeline(&ledger.events);
    let pending_actions =
        PendingActionAdapter::from_project(&manifest, &codex_tasks, &ledger.events);
    let executions = ExecutionProjectionAdapter::from_codex_runs(&codex_runs, &codex_tasks);
    Ok(WorkLedgerSnapshot {
        events: ledger.events,
        git_snapshot,
        codex_results,
        codex_tasks,
        activity_timeline,
        pending_actions,
        executions,
    })
}

pub fn synchronize_project_facts<R: Runtime>(
    app: &tauri::AppHandle<R>,
    project_root: String,
) -> Result<ProjectFactSyncResult, String> {
    let reconciled = reconcile_current_project_facts(project_root.clone())?;
    let today_workspace = build_today_workspace(app, false)?;
    Ok(ProjectFactSyncResult {
        manifest: reconciled.manifest,
        work_ledger: reconciled.work_ledger,
        today_workspace,
        file_facts_changed: reconciled.file_facts_changed,
        imported_codex_results: reconciled.imported_codex_results,
    })
}

struct ReconciledProjectFacts {
    manifest: ProjectManifest,
    work_ledger: WorkLedgerSnapshot,
    file_facts_changed: bool,
    imported_codex_results: usize,
}

fn reconcile_current_project_facts(project_root: String) -> Result<ReconciledProjectFacts, String> {
    let root = PathBuf::from(&project_root);
    let (_, file_facts_changed, _) = scan_project_workspace(&project_root)?;
    let bridge = scan_codex_result_bridge(project_root.clone())?;
    let manifest_after_bridge = bridge.manifest;

    let repository_path =
        configured_repository_path(&manifest_after_bridge.project).or_else(|| {
            read_git_snapshot(&root)
                .ok()
                .flatten()
                .and_then(|snapshot| {
                    (!snapshot.repository_path.trim().is_empty())
                        .then(|| PathBuf::from(snapshot.repository_path))
                })
        });
    if let Some(repository_path) = repository_path {
        if repository_path.is_dir() {
            let _ =
                refresh_git_snapshot(project_root.clone(), Some(path_to_string(&repository_path)))?;
        }
    }

    let mut manifest = read_and_repair_manifest(&root)?;
    update_daily_continue_snapshot(&mut manifest);
    persist_project(&root, &manifest, None)?;
    let work_ledger = get_work_ledger(project_root)?;
    Ok(ReconciledProjectFacts {
        manifest,
        work_ledger,
        file_facts_changed,
        imported_codex_results: bridge.imported_count,
    })
}

pub fn get_current_project_context_packet(
    project_root: String,
) -> Result<ProjectContextPacket, String> {
    reconcile_current_project_facts(project_root.clone())?;
    get_project_context_packet(project_root)
}

pub fn get_project_context_packet(project_root: String) -> Result<ProjectContextPacket, String> {
    let root = PathBuf::from(project_root);
    // This command intentionally reads raw persisted state. Generating a handoff must not repair,
    // refresh, or otherwise change a user's project while they are preparing context.
    let manifest: ProjectManifest = read_json(&manifest_path(&root))?;
    let ledger = read_work_ledger(&root)?;
    let codex_tasks = read_codex_tasks(&root)?;
    let activities = ActivityProjectionAdapter::visible_timeline(&ledger.events);
    let pending_actions = govern_pending_actions_for_today(PendingActionAdapter::from_project(
        &manifest,
        &codex_tasks,
        &ledger.events,
    ));

    let focus = pending_actions.first().map(|action| ProjectContextFocus {
        title: context_text(&action.title, 120),
        summary: context_text(&action.reason, 220),
        reason: "来自当前待处理事项。".to_string(),
        evidence: context_evidence_labels(&action.evidence_refs, 3),
    });

    let recent_activity = activities
        .iter()
        .take(6)
        .map(|activity| ProjectContextActivity {
            occurred_at: activity.occurred_at.clone(),
            summary: context_text(&activity.summary, 220),
            evidence: context_evidence_labels(&activity.evidence_refs, 3),
        })
        .collect::<Vec<_>>();
    let context_pending_actions = pending_actions
        .iter()
        .take(5)
        .map(|action| ProjectContextPendingAction {
            title: context_text(&action.title, 120),
            reason: context_text(&action.reason, 180),
            priority: action.priority.clone(),
        })
        .collect::<Vec<_>>();
    let files = manifest
        .files
        .iter()
        .rev()
        .take(5)
        .map(|file| FileProjectionAdapter::from_project_file(file, &manifest.project.id))
        .map(context_file_from_projection)
        .collect::<Vec<_>>();
    let codex_result = codex_tasks
        .iter()
        .filter(|task| !codex_task_is_superseded_by_accepted_target(task, &codex_tasks))
        .max_by(|left, right| {
            let left_time = if left.updated_at.trim().is_empty() {
                &left.created_at
            } else {
                &left.updated_at
            };
            let right_time = if right.updated_at.trim().is_empty() {
                &right.created_at
            } else {
                &right.updated_at
            };
            left_time.cmp(right_time)
        })
        .map(context_codex_result);
    let decisions = manifest
        .decisions
        .iter()
        .rev()
        .filter(|decision| is_user_confirmed_context_decision(&manifest, &ledger.events, decision))
        .take(5)
        .map(|decision| ProjectContextDecision {
            summary: context_text(&decision.summary, 180),
            created_at: decision.created_at.clone(),
        })
        .filter(|decision| !decision.summary.is_empty())
        .collect::<Vec<_>>();
    let git_snapshot = read_git_snapshot(&root)?.filter(|snapshot| {
        snapshot.project_id.is_empty() || snapshot.project_id == manifest.project.id
    });
    let git_facts = git_snapshot.as_ref().map(context_git_facts);
    let risks = manifest
        .project_state_summary
        .blockers
        .iter()
        .chain(manifest.project_state_summary.current_risks.iter())
        .map(|risk| context_text(risk, 180))
        .filter(|risk| !risk.is_empty())
        .take(5)
        .collect::<Vec<_>>();
    let next_step = if !manifest.project.next_step.trim().is_empty()
        && !is_system_generated_next_step(&manifest, &manifest.project.next_step)
    {
        context_text(&manifest.project.next_step, 180)
    } else {
        focus
            .as_ref()
            .map(|item| item.title.clone())
            .filter(|item| !item.is_empty())
            .unwrap_or_default()
    };
    let sparse = focus.is_none()
        && recent_activity.is_empty()
        && context_pending_actions.is_empty()
        && files.is_empty()
        && codex_result.is_none();
    let generated_at = now_string();
    let mut packet = ProjectContextPacket {
        project_id: manifest.project.id,
        project_name: context_text(&manifest.project.name, 120),
        project_description: context_text(&manifest.project.description, 280),
        current_phase: context_text(&manifest.project_state_summary.current_phase, 120),
        generated_at: generated_at.clone(),
        privacy_notice: "仅包含感冒院中的事实摘要与相对资料位置；不包含原文件正文、绝对路径、提示词、技术日志或凭据。".to_string(),
        focus,
        recent_activity,
        pending_actions: context_pending_actions,
        files,
        codex_result,
        decisions,
        git_facts,
        risks,
        next_step,
        freshness: ProjectContextFreshness {
            generated_at: generated_at.clone(),
            facts_synced_at: manifest
                .daily_continue_snapshots
                .last()
                .map(|snapshot| snapshot.generated_at.clone())
                .filter(|value| !value.is_empty())
                .or_else(|| git_snapshot.as_ref().map(|snapshot| snapshot.captured_at.clone()))
                .unwrap_or_else(|| generated_at.clone()),
            status: "current".to_string(),
        },
        sparse,
        markdown: String::new(),
    };
    packet.markdown = format_project_context_markdown(&packet);
    Ok(packet)
}

fn context_git_facts(snapshot: &GitSnapshot) -> ProjectContextGitFacts {
    ProjectContextGitFacts {
        branch: context_text(&snapshot.branch, 80),
        head_short: context_text(&snapshot.head_short, 16),
        subject: snapshot
            .recent_commits
            .first()
            .map(|commit| context_text(&commit.subject, 160))
            .unwrap_or_default(),
        recent_commits: snapshot
            .recent_commits
            .iter()
            .take(5)
            .map(|commit| ProjectContextGitCommit {
                short_hash: context_text(&commit.short_hash, 16),
                subject: context_text(&commit.subject, 160),
            })
            .filter(|commit| !commit.short_hash.is_empty() && !commit.subject.is_empty())
            .collect(),
        is_dirty: snapshot.is_dirty,
        changed_files: snapshot
            .changed_files
            .iter()
            .take(8)
            .map(|file| context_text(&file.path, 160))
            .filter(|path| !path.is_empty())
            .collect(),
        captured_at: snapshot.captured_at.clone(),
        status: snapshot.status.clone(),
    }
}

fn codex_task_is_superseded_by_accepted_target(task: &CodexTask, tasks: &[CodexTask]) -> bool {
    if !matches!(task.status.as_str(), "needsReview" | "failed")
        || !task.remaining_issues.is_empty()
        || task.target_files.is_empty()
    {
        return false;
    }
    tasks.iter().any(|candidate| {
        candidate.task_id != task.task_id
            && candidate.status == "completed"
            && candidate.acceptance.status == "approved"
            && candidate.created_at > task.created_at
            && target_files_overlap(&task.target_files, &candidate.target_files)
    })
}

fn target_files_overlap(left: &[String], right: &[String]) -> bool {
    left.iter().any(|left_path| {
        let left_key = normalize_target_path(left_path);
        !left_key.is_empty()
            && right.iter().any(|right_path| {
                let right_key = normalize_target_path(right_path);
                !right_key.is_empty()
                    && (left_key == right_key
                        || left_key.ends_with(&format!("/{right_key}"))
                        || right_key.ends_with(&format!("/{left_key}")))
            })
    })
}

fn normalize_target_path(path: &str) -> String {
    path.trim()
        .replace('\\', "/")
        .trim_start_matches("./")
        .trim_matches('/')
        .to_ascii_lowercase()
}

fn context_file_from_projection(projection: FileProjection) -> ProjectContextFile {
    ProjectContextFile {
        name: context_text(&projection.identity.original_name, 120),
        document_purpose: context_text(&projection.metadata.document_purpose, 64),
        lifecycle_status: context_text(&projection.metadata.lifecycle_status, 64),
        location: context_text(&projection.location.workspace_relative_path, 180),
        summary: context_file_summary(&projection.summary),
    }
}

fn context_file_summary(value: &str) -> String {
    let normalized = value.to_ascii_lowercase();
    if [
        "taskid",
        "runid",
        "resultid",
        "prompt",
        "stdout",
        "stderr",
        "exitcode",
        "runnerfallback",
    ]
    .iter()
    .any(|term| normalized.contains(term))
    {
        return "资料摘要包含技术执行记录，已隐藏详细内容。".to_string();
    }
    context_text(value, 180)
}

fn context_codex_result(task: &CodexTask) -> ProjectContextCodexResult {
    ProjectContextCodexResult {
        title: context_task_title(task),
        task_type: context_task_type(&task.task_type).to_string(),
        status: context_task_status(&task.status).to_string(),
        summary: context_text(
            if task.summary.trim().is_empty() {
                &task.result_text
            } else {
                &task.summary
            },
            280,
        ),
        manual_acceptance: task
            .manual_acceptance
            .iter()
            .take(4)
            .map(|item| context_text(item, 160))
            .filter(|item| !item.is_empty())
            .collect(),
    }
}

fn context_task_title(task: &CodexTask) -> String {
    let title = task.title.trim();
    if title.is_empty() || looks_like_context_identifier(title) || looks_like_context_path(title) {
        return "未命名 Codex 任务".to_string();
    }
    context_text(title, 120)
}

fn context_task_type(task_type: &str) -> &'static str {
    match normalize_codex_task_type(task_type).as_str() {
        "analysis" => "分析任务",
        "coding" => "代码任务",
        "verification" => "验收任务",
        "fileOperation" => "文件任务",
        _ => "Codex 任务",
    }
}

fn context_task_status(status: &str) -> &'static str {
    match status {
        "ready" => "待交给 Codex",
        "running" => "Codex 正在执行",
        "awaitingResult" => "正在整理结果",
        "resultReceived" | "verifying" => "结果已收到，正在核验",
        "awaitingAcceptance" => "等待人工验收",
        "completed" => "已完成",
        "failed" => "执行失败",
        "needsReview" => "需要检查",
        "cancelled" => "已取消",
        _ => "已创建",
    }
}

fn context_evidence_labels(evidence: &[EvidenceRef], limit: usize) -> Vec<String> {
    evidence
        .iter()
        .map(evidence_ref_label)
        .map(|label| context_text(&label, 120))
        .filter(|label| !label.is_empty())
        .take(limit)
        .collect()
}

fn is_user_confirmed_context_decision(
    manifest: &ProjectManifest,
    work_events: &[WorkEvent],
    decision: &DecisionRecord,
) -> bool {
    let summary = decision.summary.trim();
    if summary.is_empty() {
        return false;
    }
    if is_legacy_decision_pollution(summary) {
        return false;
    }
    let lower = summary.to_lowercase();
    let system_fact_markers = [
        "项目目录扫描",
        "待确认事项",
        "发现新文件",
        "inbox",
        "git ",
        "git:",
        "codex",
        "result bridge",
        "诊断",
        "错误",
        "刷新项目事实",
    ];
    if system_fact_markers
        .iter()
        .any(|marker| lower.contains(&marker.to_lowercase()))
    {
        return false;
    }

    if !decision.source_message_id.trim().is_empty() {
        return manifest
            .messages
            .iter()
            .find(|message| message.id == decision.source_message_id)
            .map(|message| {
                message.author == "user"
                    && extract_explicit_recorded_decision(&message.text)
                        .map(|value| normalize_user_text(&value) == normalize_user_text(summary))
                        .unwrap_or(false)
            })
            .unwrap_or(false);
    }

    let decision_ref = format!("decision:{}", decision.id);
    work_events.iter().any(|event| {
        event.project_id == manifest.project.id
            && event.source_type == "user"
            && event.event_type == "user.decisionRecorded"
            && (event
                .evidence_refs
                .iter()
                .any(|reference| reference == &decision_ref)
                || event
                    .summary
                    .strip_prefix("用户确认决定：")
                    .map(|value| normalize_user_text(value) == normalize_user_text(summary))
                    .unwrap_or(false))
    })
}

// Historical AI/system records sometimes stored a Markdown section heading as
// a DecisionRecord summary. Treat those records as analysis history, not as a
// user-confirmed decision, even if an old ledger entry happens to reference it.
fn is_legacy_decision_pollution(summary: &str) -> bool {
    let first_line = summary
        .trim()
        .lines()
        .next()
        .unwrap_or_default()
        .trim_start_matches(|character: char| {
            character.is_whitespace() || matches!(character, '-' | '*' | '#')
        })
        .trim();
    let lower = first_line.to_lowercase();
    let legacy_analysis_prefixes = [
        "已知事实",
        "ai总结",
        "建议",
        "项目说明",
        "初始化登记",
        "拟记录条目",
        "本会话已整理为 decision trace",
    ];
    legacy_analysis_prefixes
        .iter()
        .any(|marker| lower.starts_with(&marker.to_lowercase()))
}

fn normalize_user_text(value: &str) -> String {
    value.split_whitespace().collect::<String>()
}

fn context_text(value: &str, max_chars: usize) -> String {
    let line = first_line(value, max_chars);
    let normalized = redact_context_identifiers(&line)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if normalized.is_empty() {
        return String::new();
    }
    normalized
        .split_whitespace()
        .map(|part| {
            let lower = part.to_ascii_lowercase();
            if lower.starts_with("sk-")
                || lower.contains("api_key")
                || lower.contains("apikey")
                || lower.contains("token=")
                || lower.contains("password=")
                || lower.starts_with("authorization:")
            {
                "[已隐藏的敏感信息]".to_string()
            } else if looks_like_context_path(part) {
                "[已隐藏的本地路径]".to_string()
            } else {
                part.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn redact_context_identifiers(value: &str) -> String {
    let chars = value.chars().collect::<Vec<_>>();
    let mut output = String::new();
    let mut index = 0;
    while index < chars.len() {
        if index + 36 <= chars.len() {
            let candidate = chars[index..index + 36].iter().collect::<String>();
            if looks_like_context_identifier(&candidate) {
                output.push_str("[已隐藏的任务标识]");
                index += 36;
                continue;
            }
        }
        output.push(chars[index]);
        index += 1;
    }
    output
}

fn looks_like_context_identifier(value: &str) -> bool {
    let value = value.trim();
    value.len() == 36
        && value.chars().enumerate().all(|(index, character)| {
            matches!(index, 8 | 13 | 18 | 23) && character == '-' || character.is_ascii_hexdigit()
        })
}

fn looks_like_context_path(value: &str) -> bool {
    let value = value.trim();
    value.starts_with(r"\\") || value.contains(":\\") || value.contains(":/")
}

fn format_project_context_markdown(packet: &ProjectContextPacket) -> String {
    let mut lines = vec![
        format!("# 项目上下文：{}", packet.project_name),
        format!("生成时间：{}", packet.generated_at),
        String::new(),
        format!("> {}", packet.privacy_notice),
    ];
    if let Some(focus) = &packet.focus {
        lines.extend([
            String::new(),
            "## 当前焦点".to_string(),
            format!("- {}", focus.title),
            format!("- {}", focus.summary),
        ]);
        if !focus.evidence.is_empty() {
            lines.push(format!("- 依据：{}", focus.evidence.join("；")));
        }
    }
    if !packet.current_phase.is_empty() || !packet.next_step.is_empty() {
        lines.extend([String::new(), "## 当前状态".to_string()]);
        if !packet.current_phase.is_empty() {
            lines.push(format!("- 阶段：{}", packet.current_phase));
        }
        if !packet.next_step.is_empty() {
            lines.push(format!("- 下一步：{}", packet.next_step));
        }
    }
    append_context_activity_markdown(&mut lines, &packet.recent_activity);
    append_context_pending_markdown(&mut lines, &packet.pending_actions);
    append_context_file_markdown(&mut lines, &packet.files);
    if !packet.decisions.is_empty() {
        lines.extend([String::new(), "## 已确认决定".to_string()]);
        for decision in &packet.decisions {
            lines.push(format!("- {}", decision.summary));
        }
    }
    if let Some(git) = &packet.git_facts {
        lines.extend([String::new(), "## Git 事实".to_string()]);
        lines.push(format!(
            "- {} {}{}",
            git.branch,
            git.head_short,
            if git.subject.is_empty() {
                String::new()
            } else {
                format!(" · {}", git.subject)
            }
        ));
        for commit in &git.recent_commits {
            lines.push(format!("- {} · {}", commit.short_hash, commit.subject));
        }
    }
    if !packet.risks.is_empty() {
        lines.extend([String::new(), "## 已知风险或阻塞".to_string()]);
        for risk in &packet.risks {
            lines.push(format!("- {}", risk));
        }
    }
    if let Some(result) = &packet.codex_result {
        lines.extend([
            String::new(),
            "## 最近 Codex 结果".to_string(),
            format!("- {} · {}", result.title, result.status),
        ]);
        if !result.summary.is_empty() {
            lines.push(format!("- {}", result.summary));
        }
        for acceptance in &result.manual_acceptance {
            lines.push(format!("- 待验收：{}", acceptance));
        }
    }
    if packet.sparse {
        lines.extend([
            String::new(),
            "当前项目可用事实较少，感冒院不会据此虚构下一步。".to_string(),
        ]);
    }
    lines.join("\n")
}

fn append_context_activity_markdown(
    lines: &mut Vec<String>,
    activities: &[ProjectContextActivity],
) {
    if activities.is_empty() {
        return;
    }
    lines.extend([String::new(), "## 最近事实".to_string()]);
    for activity in activities {
        lines.push(format!("- {}：{}", activity.occurred_at, activity.summary));
    }
}

fn append_context_pending_markdown(
    lines: &mut Vec<String>,
    actions: &[ProjectContextPendingAction],
) {
    if actions.is_empty() {
        return;
    }
    lines.extend([String::new(), "## 待你处理".to_string()]);
    for action in actions {
        lines.push(format!("- {}：{}", action.title, action.reason));
    }
}

fn append_context_file_markdown(lines: &mut Vec<String>, files: &[ProjectContextFile]) {
    if files.is_empty() {
        return;
    }
    lines.extend([String::new(), "## 相关资料".to_string()]);
    for file in files {
        let location = if file.location.is_empty() {
            String::new()
        } else {
            format!(" · {}", file.location)
        };
        lines.push(format!("- {}{}", file.name, location));
    }
}

pub fn refresh_git_snapshot(
    project_root: String,
    repository_path: Option<String>,
) -> Result<GitSnapshot, String> {
    let root = PathBuf::from(&project_root);
    let mut manifest = read_and_repair_manifest(&root)?;
    let previous = read_git_snapshot(&root)?;
    let repo = repository_path
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
        .or_else(|| configured_repository_path(&manifest.project))
        .or_else(|| {
            previous
                .as_ref()
                .map(|snapshot| PathBuf::from(&snapshot.repository_path))
        })
        .unwrap_or_else(|| root.clone());
    let repo = repo
        .canonicalize()
        .map_err(|err| format!("Git 仓库路径不可用：{}：{err}", path_to_string(&repo)))?;
    let snapshot = capture_git_snapshot(&manifest.project.id, &repo)?;
    // Only a repository proven readable through Git becomes a stable project link.
    if manifest.project.repository_path != snapshot.repository_path {
        let previous_manifest = manifest.clone();
        manifest.project.repository_path = snapshot.repository_path.clone();
        persist_project(&root, &manifest, Some(&previous_manifest))?;
    }
    write_git_snapshot(&root, &snapshot)?;
    record_git_snapshot_events(&root, &manifest.project.id, previous.as_ref(), &snapshot)?;
    // Today already holds this lock; explicit refresh must also serialize task writes.
    let _guard = project_write_lock()
        .lock()
        .map_err(|_| "项目存储锁已损坏，请重启感冒院。".to_string())?;
    refresh_codex_task_git_verifications(&root, &snapshot.repository_path)?;
    Ok(snapshot)
}

pub fn record_user_decision_event(
    project_root: String,
    decision: String,
    reason: Option<String>,
) -> Result<WorkEvent, String> {
    let decision = decision.trim().to_string();
    if decision.is_empty() {
        return Err("决定内容不能为空。".to_string());
    }
    let root = PathBuf::from(&project_root);
    let _guard = project_write_lock()
        .lock()
        .map_err(|_| "项目存储锁已损坏，请重启感冒院。".to_string())?;
    let mut manifest = read_and_repair_manifest(&root)?;
    let write = record_user_decision_event_in_manifest(
        &root,
        &mut manifest,
        &decision,
        reason.as_deref(),
        None,
    )?;
    if !write.is_new {
        return Ok(write.event);
    }
    persist_project(&root, &manifest, None)?;
    append_audit_event(&root, manifest.audit.last().expect("audit event exists"))?;
    append_work_event_if_new(&root, write.event.clone())?
        .ok_or_else(|| "该决定事实已存在。".to_string())
}

struct DecisionEventWrite {
    event: WorkEvent,
    is_new: bool,
}

#[derive(Debug)]
struct NextStepEventWrite {
    event: WorkEvent,
    is_new: bool,
}

fn record_user_decision_event_in_manifest(
    root: &Path,
    manifest: &mut ProjectManifest,
    decision: &str,
    reason: Option<&str>,
    source_message_id: Option<&str>,
) -> Result<DecisionEventWrite, String> {
    let decision = decision.trim();
    if decision.is_empty() {
        return Err("决定内容不能为空。".to_string());
    }
    let reason_text = reason
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("");
    let source_ref = hash_text(&format!(
        "{}\n{}\n{}",
        manifest.project.id, decision, reason_text
    ));
    let event_key = work_event_key("user", &source_ref, "user.decisionRecorded");
    if let Some(existing) = read_work_ledger(root)?.events.into_iter().find(|event| {
        work_event_key(&event.source_type, &event.source_ref, &event.event_type) == event_key
    }) {
        return Ok(DecisionEventWrite {
            event: existing,
            is_new: false,
        });
    }
    let now = now_string();
    let decision_record = manifest
        .decisions
        .iter()
        .find(|item| item.summary.trim() == decision)
        .cloned()
        .unwrap_or_else(|| DecisionRecord {
            id: new_id(),
            summary: decision.to_string(),
            source_message_id: source_message_id.unwrap_or_default().to_string(),
            created_at: now.clone(),
        });
    let trace = new_decision_trace(
        "decisionCandidate",
        &decision_record.id,
        &manifest.project.id,
        vec![DecisionTraceEvidence {
            kind: "userDecision".to_string(),
            label: "用户手动记录".to_string(),
            summary: first_line(
                if reason_text.is_empty() {
                    decision
                } else {
                    reason_text
                },
                240,
            ),
            source_id: decision_record.id.clone(),
        }],
        "用户确认的项目决定。".to_string(),
        decision.to_string(),
        trace_confidence(100),
    );
    if !manifest
        .decisions
        .iter()
        .any(|item| item.id == decision_record.id)
    {
        manifest.decisions.push(decision_record.clone());
    }
    manifest.audit.push(audit_event(
        "workLedger.userDecision",
        &manifest.project.id,
        "success",
        decision.to_string(),
        false,
        true,
    ));
    let mut event = fact_event(
        &manifest.project.id,
        "user",
        &source_ref,
        "user.decisionRecorded",
        format!("用户确认决定：{}", decision),
        vec![format!("decision:{}", decision_record.id)],
        100,
    );
    event.decision_trace_id = trace.id.clone();
    event.decision_trace = trace;
    Ok(DecisionEventWrite {
        event,
        is_new: true,
    })
}

fn record_user_next_step_event_in_manifest(
    root: &Path,
    manifest: &mut ProjectManifest,
    next_step: &str,
    reason: Option<&str>,
    source_message_id: Option<&str>,
) -> Result<NextStepEventWrite, String> {
    let next_step = next_step.trim();
    if next_step.is_empty() {
        return Err("下一步内容不能为空。".to_string());
    }
    let reason_text = reason
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("");
    let source_ref =
        captured_fact_source_ref(&manifest.project.id, "nextAction", next_step, reason_text);
    let event_key = work_event_key("user", &source_ref, "user.nextActionRecorded");

    // The manifest is the canonical current next-step surface. Re-assert it
    // even for a duplicate submission so a repaired/older manifest converges
    // without creating another ledger event.
    manifest.project.next_step = next_step.to_string();
    manifest.project.last_opened_at = now_string();

    if let Some(existing) = read_work_ledger(root)?.events.into_iter().find(|event| {
        work_event_key(&event.source_type, &event.source_ref, &event.event_type) == event_key
    }) {
        return Ok(NextStepEventWrite {
            event: existing,
            is_new: false,
        });
    }

    manifest.audit.push(audit_event(
        "workLedger.userNextStep",
        &manifest.project.id,
        "success",
        next_step.to_string(),
        false,
        true,
    ));
    let mut evidence_refs = vec![format!("userCapture:{source_ref}")];
    if let Some(message_id) = source_message_id.filter(|value| !value.trim().is_empty()) {
        evidence_refs.push(format!("message:{message_id}"));
    }
    let mut event = fact_event(
        &manifest.project.id,
        "user",
        &source_ref,
        "user.nextActionRecorded",
        format!("用户记录下一步：{next_step}"),
        evidence_refs,
        100,
    );
    event.fact_kind = "userConfirmed".to_string();
    Ok(NextStepEventWrite {
        event,
        is_new: true,
    })
}

pub fn capture_project_fact(
    project_root: String,
    request: ProjectFactCaptureRequest,
) -> Result<WorkEvent, String> {
    let capture_type = request.capture_type.trim();
    if capture_type == "decision" {
        return record_user_decision_event(
            project_root,
            request.content,
            (!request.reason.trim().is_empty()).then_some(request.reason),
        );
    }

    let root = PathBuf::from(&project_root);
    let manifest = read_and_repair_manifest(&root)?;
    let ledger = read_work_ledger(&root)?;
    let content = first_line(&request.content, 240);
    let reason = first_line(&request.reason, 240);
    if capture_type == "nextAction" {
        if content.is_empty() {
            return Err("请说明下一步要做什么。".to_string());
        }
        let mut manifest = manifest;
        let write = record_user_next_step_event_in_manifest(
            &root,
            &mut manifest,
            &content,
            (!reason.is_empty()).then_some(reason.as_str()),
            None,
        )?;
        persist_project(&root, &manifest, None)?;
        if write.is_new {
            append_audit_event(&root, manifest.audit.last().expect("audit event exists"))?;
            append_work_event_if_new(&root, write.event.clone())?
                .ok_or_else(|| "该下一步事实已存在。".to_string())?;
        }
        return Ok(write.event);
    }
    let (event_type, summary, source_ref) = match capture_type {
        "progress" => {
            if content.is_empty() {
                return Err("请写下本次已经完成的进展。".to_string());
            }
            (
                "user.progressRecorded",
                format!("用户记录进展：{content}"),
                captured_fact_source_ref(&manifest.project.id, capture_type, &content, &reason),
            )
        }
        "blocker" => {
            if content.is_empty() {
                return Err("请说明当前阻塞。".to_string());
            }
            (
                "user.blockerRecorded",
                format!("用户记录阻塞：{content}"),
                captured_fact_source_ref(&manifest.project.id, capture_type, &content, &reason),
            )
        }
        "resolveAction" => {
            let source_ref = request.action_source_ref.trim();
            if source_ref.is_empty() {
                return Err("缺少需要关闭的工作事项。".to_string());
            }
            let source_event = ledger.events.iter().find(|event| {
                event.project_id == manifest.project.id
                    && event.source_type == "user"
                    && event.source_ref == source_ref
                    && matches!(
                        event.event_type.as_str(),
                        "user.blockerRecorded" | "user.nextActionRecorded"
                    )
            });
            let Some(source_event) = source_event else {
                return Err("该工作事项已变化，无法安全关闭。请刷新项目状态后重试。".to_string());
            };
            (
                "user.actionResolved",
                if content.is_empty() {
                    format!("用户已处理事项：{}", first_line(&source_event.summary, 180))
                } else {
                    format!(
                        "用户已处理事项：{}（{}）",
                        first_line(&source_event.summary, 160),
                        content
                    )
                },
                source_ref.to_string(),
            )
        }
        _ => {
            return Err("不支持的工作记录类型。".to_string());
        }
    };
    let resolves_current_next_step = event_type == "user.actionResolved"
        && ledger.events.iter().any(|event| {
            event.project_id == manifest.project.id
                && event.source_type == "user"
                && event.source_ref == source_ref
                && event.event_type == "user.nextActionRecorded"
                && event
                    .summary
                    .strip_prefix("用户记录下一步：")
                    .map(str::trim)
                    .is_some_and(|value| {
                        normalize_key(value) == normalize_key(&manifest.project.next_step)
                    })
        });

    if let Some(existing) = ledger.events.into_iter().find(|event| {
        work_event_key(&event.source_type, &event.source_ref, &event.event_type)
            == work_event_key("user", &source_ref, event_type)
    }) {
        return Ok(existing);
    }

    let mut event = fact_event(
        &manifest.project.id,
        "user",
        &source_ref,
        event_type,
        summary.clone(),
        vec![format!("userCapture:{source_ref}")],
        100,
    );
    event.fact_kind = "userConfirmed".to_string();
    append_work_event_if_new(&root, event.clone())?
        .ok_or_else(|| "该工作事实已存在。".to_string())?;

    let mut manifest = manifest;
    if resolves_current_next_step {
        manifest.project.next_step.clear();
    }
    manifest.audit.push(audit_event(
        "workLedger.userFactCaptured",
        &manifest.project.id,
        "success",
        summary,
        false,
        true,
    ));
    persist_project(&root, &manifest, None)?;
    append_audit_event(&root, manifest.audit.last().expect("audit event exists"))?;
    Ok(event)
}

fn captured_fact_source_ref(
    project_id: &str,
    capture_type: &str,
    content: &str,
    reason: &str,
) -> String {
    hash_text(&format!(
        "user-capture\n{project_id}\n{capture_type}\n{}\n{}",
        normalize_key(content),
        normalize_key(reason)
    ))
}

fn capture_git_snapshot(project_id: &str, repo: &Path) -> Result<GitSnapshot, String> {
    let branch = run_git(repo, &["rev-parse", "--abbrev-ref", "HEAD"])?;
    let head = run_git(repo, &["rev-parse", "HEAD"])?;
    let head_short = run_git(repo, &["rev-parse", "--short", "HEAD"])?;
    let log = run_git(
        repo,
        &["log", "-5", "--pretty=format:%H%x1f%h%x1f%an%x1f%ct%x1f%s"],
    )?;
    let status = run_git(repo, &["status", "--porcelain"])?;
    let changed = run_git(repo, &["diff", "--name-status"]).unwrap_or_default();
    let tags = run_git(repo, &["tag", "--points-at", "HEAD"]).unwrap_or_default();
    let changed_files = parse_git_changed_files(&status, &changed);
    Ok(GitSnapshot {
        id: new_id(),
        project_id: project_id.to_string(),
        repository_path: path_to_string(repo),
        branch: branch.trim().to_string(),
        head: head.trim().to_string(),
        head_short: head_short.trim().to_string(),
        recent_commits: parse_git_log(&log),
        is_dirty: !changed_files.is_empty(),
        changed_files,
        tags: tags
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(ToOwned::to_owned)
            .collect(),
        status: "ready".to_string(),
        captured_at: now_string(),
        ..GitSnapshot::default()
    })
}

fn run_git(repo: &Path, args: &[&str]) -> Result<String, String> {
    let output = hidden_command("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .map_err(|err| format!("执行 Git 只读命令失败：git {:?}：{err}", args))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(format!("Git 命令失败：git {:?}：{}", args, stderr));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn parse_git_log(text: &str) -> Vec<GitCommitInfo> {
    text.lines()
        .filter_map(|line| {
            let parts = line.split('\u{1f}').collect::<Vec<_>>();
            if parts.len() < 5 {
                return None;
            }
            Some(GitCommitInfo {
                hash: parts[0].to_string(),
                short_hash: parts[1].to_string(),
                author: parts[2].to_string(),
                timestamp: parts[3].to_string(),
                subject: parts[4].to_string(),
            })
        })
        .collect()
}

fn parse_git_changed_files(status: &str, diff: &str) -> Vec<GitChangedFile> {
    let mut seen = HashSet::new();
    let mut files = Vec::new();
    for line in status.lines().chain(diff.lines()) {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let (status_code, path) = if trimmed.len() > 3 && trimmed.as_bytes().get(2) == Some(&b' ') {
            (
                trimmed[..2].trim().to_string(),
                trimmed[3..].trim().to_string(),
            )
        } else {
            let mut parts = trimmed.splitn(2, char::is_whitespace);
            (
                parts.next().unwrap_or("").trim().to_string(),
                parts.next().unwrap_or("").trim().to_string(),
            )
        };
        let path = path.trim_matches('"').to_string();
        if path.is_empty() || !seen.insert(normalize_key(&path)) {
            continue;
        }
        files.push(GitChangedFile {
            status: status_code,
            path,
        });
    }
    files
}

fn record_git_snapshot_events(
    root: &Path,
    project_id: &str,
    previous: Option<&GitSnapshot>,
    snapshot: &GitSnapshot,
) -> Result<(), String> {
    if snapshot.status != "ready" {
        return Ok(());
    }
    if previous.map(|item| item.head.as_str()) != Some(snapshot.head.as_str()) {
        let subject = snapshot
            .recent_commits
            .first()
            .map(|item| item.subject.clone())
            .unwrap_or_default();
        append_work_event_if_new(
            root,
            fact_event(
                project_id,
                "git",
                &snapshot.head,
                "git.headChanged",
                format!("Git HEAD 更新到 {}：{}", snapshot.head_short, subject),
                vec![
                    format!("repo:{}", snapshot.repository_path),
                    format!("branch:{}", snapshot.branch),
                ],
                98,
            ),
        )?;
    }
    if snapshot.is_dirty {
        let dirty_ref = hash_text(
            &snapshot
                .changed_files
                .iter()
                .map(|file| format!("{} {}", file.status, file.path))
                .collect::<Vec<_>>()
                .join("\n"),
        );
        append_work_event_if_new(
            root,
            fact_event(
                project_id,
                "git",
                &dirty_ref,
                "git.workingTreeDirty",
                format!(
                    "Git 工作区存在 {} 个未提交改动。",
                    snapshot.changed_files.len()
                ),
                snapshot
                    .changed_files
                    .iter()
                    .take(12)
                    .map(|file| format!("{} {}", file.status, file.path))
                    .collect(),
                96,
            ),
        )?;
    }
    Ok(())
}

fn codex_external_result_from_report(
    project_id: &str,
    report: &CodexReportRecord,
) -> CodexExternalResult {
    let contract = codex_result_contract_from_report(report);
    CodexExternalResult {
        id: format!(
            "codex-result-{}",
            &report.report_hash.chars().take(12).collect::<String>()
        ),
        project_id: project_id.to_string(),
        source_hash: report.report_hash.clone(),
        source_label: report.source_label.clone(),
        result_source: contract.result_source,
        task_id: contract.task_id,
        task_type: contract.task_type,
        status: contract.status,
        result_text: contract.result_text,
        completed_content: report.modified_content.clone(),
        changed_files: contract.changed_files,
        commits: contract.commits,
        tests: contract.tests,
        findings: contract.findings,
        recommendations: contract.recommendations,
        questions: contract.questions,
        checks: contract.checks,
        passed: contract.passed,
        failed: contract.failed,
        artifacts: contract.artifacts,
        target_files: contract.target_files,
        unresolved_items: contract.remaining_issues,
        manual_acceptance: contract.manual_acceptance,
        external_thread_id: contract.external_thread_id,
        result_run_id: contract.result_run_id,
        raw_evidence_path: report.evidence_managed_path.clone(),
        created_at: report.created_at.clone(),
    }
}

fn codex_result_source_from_text(text: &str) -> String {
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(text) {
        let result_source = json_string_field(&value, &["resultSource", "result_source"]);
        if !result_source.trim().is_empty() {
            return result_source;
        }
    }
    let value = extract_labeled_token(text, &["resultSource", "result_source"])
        .trim_matches([',', '"', '\''])
        .to_string();
    if !value.trim().is_empty() {
        return value;
    }
    String::new()
}

fn codex_result_contract_from_report(report: &CodexReportRecord) -> CodexResultContract {
    let raw = read_optional_text(Path::new(&report.evidence_managed_path)).unwrap_or_default();
    if report.source_kind == "bridge" {
        let extension = Path::new(&report.evidence_managed_path)
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if raw.trim().is_empty()
            || hash_text(&normalize_codex_bridge_result_text(&raw, &extension))
                != report.report_hash
        {
            // Legacy reports may point at an overwritten handoff file. Never parse
            // another run's content as this report; retain only cached summaries.
            let mut contract = codex_result_contract_from_text(report, "");
            contract.status = "needsReview".to_string();
            contract
                .remaining_issues
                .push("历史结果证据缺失或已变化，无法确认原始内容。".to_string());
            return contract;
        }
    }
    codex_result_contract_from_text(report, &raw)
}

fn codex_result_contract_from_text(report: &CodexReportRecord, raw: &str) -> CodexResultContract {
    let source_task_id = codex_task_id_from_source_label(report);
    let parsed_json = serde_json::from_str::<serde_json::Value>(&raw).ok();
    let has_explicit_manual_acceptance = parsed_json
        .as_ref()
        .map(|value| {
            value.get("manualAcceptance").is_some() || value.get("manual_acceptance").is_some()
        })
        .unwrap_or(false);
    let has_explicit_remaining = parsed_json
        .as_ref()
        .map(|value| {
            value.get("remainingIssues").is_some() || value.get("remaining_issues").is_some()
        })
        .unwrap_or(false);
    let mut contract = if let Some(value) = parsed_json.as_ref() {
        CodexResultContract {
            task_id: json_string_field(&value, &["taskId", "task_id"]),
            task_type: normalize_codex_task_type(&json_string_field(
                &value,
                &["taskType", "task_type"],
            )),
            status: json_string_field(&value, &["status"]),
            summary: json_string_field(&value, &["summary"]),
            result_text: json_string_field(&value, &["resultText", "result_text"]),
            completed_at: json_string_field(&value, &["completedAt", "completed_at"]),
            remaining_issues: json_string_array_field(
                &value,
                &["remainingIssues", "remaining_issues"],
            ),
            manual_acceptance: json_string_array_field(
                &value,
                &["manualAcceptance", "manual_acceptance"],
            ),
            result_source: json_string_field(&value, &["resultSource", "result_source"]),
            result_run_id: json_string_field(&value, &["resultRunId", "runId", "result_run_id"]),
            changed_files: json_coding_string_array_field(
                &value,
                &["changedFiles", "changed_files"],
            ),
            commits: json_coding_string_array_field(&value, &["commits"]),
            tests: json_coding_display_array_field(&value, &["tests"]),
            findings: json_string_array_field(&value, &["findings"]),
            recommendations: json_string_array_field(&value, &["recommendations"]),
            questions: json_string_array_field(&value, &["questions"]),
            checks: json_string_array_field(&value, &["checks"]),
            passed: json_string_array_field(&value, &["passed"]),
            failed: json_string_array_field(&value, &["failed"]),
            artifacts: json_string_array_field(&value, &["artifacts"]),
            target_files: json_string_array_field(&value, &["targetFiles", "target_files"]),
            external_thread_id: json_string_field(
                &value,
                &["externalThreadId", "threadId", "external_thread_id"],
            ),
        }
    } else {
        CodexResultContract::default()
    };

    if !source_task_id.trim().is_empty()
        && !contract.task_id.trim().is_empty()
        && source_task_id != contract.task_id
    {
        return CodexResultContract {
            task_id: source_task_id,
            task_type: "unknown".to_string(),
            status: "needsReview".to_string(),
            summary: "Codex 结果文件名与内容 taskId 不一致，需要人工检查。".to_string(),
            result_text: first_line(&raw, 600),
            result_source: if contract.result_source.trim().is_empty() {
                "bridge".to_string()
            } else {
                contract.result_source
            },
            remaining_issues: vec!["结果文件名与内容 taskId 不一致。".to_string()],
            ..CodexResultContract::default()
        };
    }

    if contract.task_id.trim().is_empty() {
        contract.task_id = extract_task_id_from_text(&report.summary);
    }
    if contract.task_id.trim().is_empty() {
        contract.task_id = codex_task_id_from_source_label(report);
    }
    if contract.task_type.trim().is_empty() {
        contract.task_type = infer_codex_task_type(&format!(
            "{}\n{}\n{}",
            report.summary,
            report.modified_content.join("\n"),
            report.test_results.join("\n")
        ));
    }
    if contract.status.trim().is_empty() {
        contract.status = report.completed_status.clone();
    }
    if contract.summary.trim().is_empty() {
        contract.summary = report.summary.clone();
    }
    if contract.result_text.trim().is_empty() {
        contract.result_text = first_non_empty(&[
            report.summary.clone(),
            report.modified_content.join("\n"),
            report.next_step_suggestions.join("\n"),
        ]);
    }
    if contract.completed_at.trim().is_empty() {
        contract.completed_at = extract_labeled_token(&raw, &["completedAt", "completed_at"]);
    }
    if parsed_json.is_none() && contract.result_run_id.trim().is_empty() {
        contract.result_run_id =
            extract_labeled_token(&raw, &["resultRunId", "runId", "result_run_id"]);
    }
    if contract.remaining_issues.is_empty() && !has_explicit_remaining {
        contract.remaining_issues = report.unresolved_issues.clone();
    }
    if contract.manual_acceptance.is_empty() && !has_explicit_manual_acceptance {
        contract.manual_acceptance = collect_lines_by_keywords(
            &report
                .unresolved_issues
                .iter()
                .chain(report.next_step_suggestions.iter())
                .map(|item| item.as_str())
                .collect::<Vec<_>>(),
            &["验收", "人工", "确认"],
        );
    }
    if contract.result_source.trim().is_empty() {
        contract.result_source = codex_result_source_from_text(raw);
    }
    if contract.changed_files.is_empty() {
        contract.changed_files = extract_changed_files_from_lines(&report.modified_content);
    }
    if contract.commits.is_empty() && !report.commit.trim().is_empty() {
        contract.commits = vec![report.commit.clone()];
    }
    if contract.tests.is_empty() {
        contract.tests = report.test_results.clone();
    }
    if contract.artifacts.is_empty() {
        contract.artifacts = extract_artifacts_from_lines(&report.modified_content);
    }
    if contract.external_thread_id.trim().is_empty() {
        contract.external_thread_id = extract_thread_id_from_text(&report.summary);
    }
    contract
}

fn first_non_empty(values: &[String]) -> String {
    values
        .iter()
        .map(|value| value.trim())
        .find(|value| !value.is_empty())
        .unwrap_or_default()
        .to_string()
}

fn normalize_codex_task_type(value: &str) -> String {
    match value.trim() {
        "analysis" | "coding" | "verification" | "fileOperation" | "legacy" | "unknown" => {
            value.trim().to_string()
        }
        "file_operation" | "file-operation" | "file" => "fileOperation".to_string(),
        "" => String::new(),
        _ => "unknown".to_string(),
    }
}

fn infer_codex_task_type(text: &str) -> String {
    let lower = text.to_ascii_lowercase();
    if text.contains("项目理解")
        || text.contains("下一步建议")
        || text.contains("项目分析")
        || text.contains("待确认问题")
    {
        return "analysis".to_string();
    }
    if lower.contains("cargo")
        || lower.contains("npm run")
        || lower.contains("commit")
        || text.contains("实现")
        || text.contains("修复")
        || text.contains("代码")
        || text.contains("构建")
    {
        return "coding".to_string();
    }
    if text.contains("测试")
        || text.contains("验收")
        || text.contains("核验")
        || text.contains("检查")
        || lower.contains("verify")
    {
        return "verification".to_string();
    }
    if text.contains("文件")
        || text.contains("导入")
        || text.contains("整理")
        || text.contains("转换")
        || lower.contains("artifact")
    {
        return "fileOperation".to_string();
    }
    "analysis".to_string()
}

fn normalize_codex_task_status(status: &str) -> String {
    match status.trim() {
        "draft" | "ready" | "running" | "awaitingResult" | "resultReceived" | "verifying"
        | "awaitingAcceptance" | "completed" | "failed" | "cancelled" | "needsReview" => {
            status.trim().to_string()
        }
        "handedOff" => "awaitingResult".to_string(),
        "gitVerified" => "resultReceived".to_string(),
        "" => "ready".to_string(),
        _ => "needsReview".to_string(),
    }
}

fn normalize_codex_run_status(status: &str) -> String {
    match status.trim() {
        "starting" | "running" | "exited" | "failed" | "cancelled" | "unknownAfterRestart" => {
            status.trim().to_string()
        }
        "" => String::new(),
        _ => "unknownAfterRestart".to_string(),
    }
}

fn codex_task_git_required(task: &CodexTask) -> bool {
    match normalize_codex_task_type(&task.task_type).as_str() {
        "analysis" => false,
        "coding" => true,
        // File operations can intentionally create non-committed artifacts, such as
        // a user-approved document or the Golden Path verification file. Git only
        // becomes a completion gate when the Codex result explicitly reports commits.
        "verification" | "fileOperation" => !task.reported_commits.is_empty(),
        _ => !task.reported_commits.is_empty(),
    }
}

fn codex_result_has_useful_content(task: &CodexTask) -> bool {
    if !task.summary.trim().is_empty() || !task.result_text.trim().is_empty() {
        return true;
    }
    [
        &task.findings,
        &task.recommendations,
        &task.questions,
        &task.checks,
        &task.passed,
        &task.failed,
        &task.artifacts,
        &task.target_files,
    ]
    .iter()
    .any(|items| items.iter().any(|value| !value.trim().is_empty()))
}

fn codex_task_id_from_source_label(report: &CodexReportRecord) -> String {
    for text in [&report.source_label, &report.evidence_managed_path] {
        if let Some((task_id, _)) = text.rsplit_once("-result.") {
            let task_id = task_id.rsplit(['\\', '/']).next().unwrap_or(task_id).trim();
            if !task_id.is_empty() {
                return task_id.to_string();
            }
        }
    }
    String::new()
}

fn persist_codex_external_result_for_report(
    root: &Path,
    project_id: &str,
    report: &CodexReportRecord,
) -> Result<Option<WorkEvent>, String> {
    let mut document = read_codex_external_results(root)?;
    let result = codex_external_result_from_report(project_id, report);
    let duplicate = document
        .results
        .iter()
        .any(|item| item.source_hash == result.source_hash);
    if !duplicate {
        document.results.push(result);
        document.updated_at = now_string();
        write_codex_external_results(root, &document)?;
    }
    append_work_event_if_new(
        root,
        fact_event(
            project_id,
            "codex",
            &report.report_hash,
            "codex.reportImported",
            format!("导入 Codex 结果：{}", report.summary),
            vec![
                format!("status:{}", report.completed_status),
                format!("commit:{}", report.commit),
                format!("evidence:{}", report.evidence_managed_path),
            ],
            if report.parse_status == "ready" {
                90
            } else {
                62
            },
        ),
    )
}

fn upsert_codex_task_from_report(
    root: &Path,
    report: &CodexReportRecord,
) -> Result<CodexTask, String> {
    let manifest = read_and_repair_manifest(root)?;
    let contract = codex_result_contract_from_report(report);
    let task_id = if contract.task_id.trim().is_empty() {
        codex_task_id_from_report(report)
    } else {
        contract.task_id.clone()
    };
    if task_id.is_empty() {
        return Ok(CodexTask::default());
    }
    let now = now_string();
    let mut task = read_codex_task(root, &task_id).unwrap_or_else(|_| CodexTask {
        task_id: task_id.clone(),
        project_id: manifest.project.id.clone(),
        title: first_line(&report.summary, 80),
        created_at: now.clone(),
        status: "resultReceived".to_string(),
        expected_result_path: path_to_string(&canonical_codex_result_path(root, &task_id)),
        ..CodexTask::default()
    });
    // Scanning an immutable report again must not replay its state transition.
    // In particular, preserve acceptance and a retry already started by the user.
    if task.result_id == report.id
        || task
            .evidence_refs
            .contains(&format!("codexReport:{}", report.id))
    {
        // Legacy scans replayed outcomes after acceptance. Only restore a decision
        // made after this report; earlier decisions may belong to a previous result.
        if task.result_id == report.id
            && matches!(task.status.as_str(), "completed" | "awaitingAcceptance")
        {
            let acceptance_is_current = task
                .acceptance
                .decided_at
                .parse::<u128>()
                .ok()
                .zip(report.created_at.parse::<u128>().ok())
                .map(|(decision, received)| decision >= received)
                .unwrap_or(false);
            let corrected = match (task.acceptance.status.as_str(), acceptance_is_current) {
                ("approved", true) => Some("completed"),
                ("rejected", true) => Some("failed"),
                _ if task.acceptance.status != "approved" => {
                    match contract.status.to_ascii_lowercase().as_str() {
                        "blocked" | "failed" | "partial" => Some("failed"),
                        "needsreview" => Some("needsReview"),
                        _ if contract.result_source == "runnerFallback" => Some("needsReview"),
                        _ => None,
                    }
                }
                _ => None,
            };
            if let Some(status) = corrected.filter(|status| *status != task.status) {
                task.status = status.to_string();
                task.completed_at = if status == "completed" {
                    task.acceptance.decided_at.clone()
                } else {
                    String::new()
                };
                task.updated_at = now;
                write_codex_task(root, &task)?;
            }
        }
        return Ok(task);
    }
    if let Some(latest_run) = read_codex_runs(root)?
        .into_iter()
        .find(|run| run.task_id == task_id)
    {
        if !contract.result_run_id.is_empty() && contract.result_run_id != latest_run.id {
            return Ok(task);
        }
    }
    task.project_id = manifest.project.id.clone();
    if task.title.trim().is_empty() {
        task.title = first_line(&contract.summary, 80);
    }
    task.task_type = if contract.task_type.trim().is_empty() {
        normalize_codex_task_type(&task.task_type)
    } else {
        contract.task_type.clone()
    };
    if task.task_type.trim().is_empty() {
        task.task_type = infer_codex_task_type(&format!("{}\n{}", task.title, contract.summary));
    }
    task.result_received_at = now.clone();
    task.result_id = report.id.clone();
    task.result_source = contract.result_source.clone();
    task.result_run_id = contract.result_run_id.clone();
    task.summary = contract.summary.clone();
    task.result_text = contract.result_text.clone();
    task.changed_files = contract.changed_files.clone();
    task.reported_commits = contract.commits.clone();
    task.tests = contract.tests.clone();
    task.findings = contract.findings.clone();
    task.recommendations = contract.recommendations.clone();
    task.questions = contract.questions.clone();
    task.checks = contract.checks.clone();
    task.passed = contract.passed.clone();
    task.failed = contract.failed.clone();
    task.artifacts = contract.artifacts.clone();
    task.target_files = contract.target_files.clone();
    task.remaining_issues = contract.remaining_issues.clone();
    task.manual_acceptance = contract.manual_acceptance.clone();
    task.evidence_refs = merge_unique(
        &task.evidence_refs,
        &[
            format!("codexReport:{}", report.id),
            format!("evidence:{}", report.evidence_managed_path),
        ],
    );
    task.status = "resultReceived".to_string();
    task.acceptance = CodexTaskAcceptance::default();
    task.completed_at.clear();
    verify_codex_task_against_git(root, &mut task);
    let result_status = contract.status.to_ascii_lowercase();
    if matches!(result_status.as_str(), "failed" | "partial" | "blocked") {
        task.status = "failed".to_string();
    } else if result_status != "completed"
        || contract.result_source == "runnerFallback"
        || report.parse_status != "ready"
        || !codex_result_has_useful_content(&task)
    {
        task.status = "needsReview".to_string();
    } else if codex_task_git_required(&task) && task.git_verification.status != "verified" {
        task.status = "needsReview".to_string();
    } else if !task.manual_acceptance.is_empty() {
        task.status = "awaitingAcceptance".to_string();
    } else if !codex_task_git_required(&task) || task.git_verification.status == "verified" {
        task.status = "completed".to_string();
        if task.completed_at.is_empty() {
            task.completed_at = if contract.completed_at.trim().is_empty() {
                now.clone()
            } else {
                contract.completed_at.clone()
            };
        }
    } else if matches!(
        task.git_verification.status.as_str(),
        "mismatch" | "unavailable"
    ) {
        task.status = "needsReview".to_string();
    }
    task.updated_at = now;
    write_codex_task(root, &task)?;
    Ok(task)
}

fn verify_codex_task_against_git(root: &Path, task: &mut CodexTask) {
    if !codex_task_git_required(task) {
        let repository_path = if !task.repository_path.trim().is_empty() {
            task.repository_path.clone()
        } else {
            read_git_snapshot(root)
                .ok()
                .flatten()
                .map(|snapshot| snapshot.repository_path)
                .unwrap_or_default()
        };
        task.git_verification = CodexTaskGitVerification {
            status: "notApplicable".to_string(),
            repository_path,
            checked_at: now_string(),
            reason: "该 Codex 任务类型不要求 Git 核验。".to_string(),
        };
        return;
    }
    let repo = if !task.repository_path.trim().is_empty() {
        PathBuf::from(&task.repository_path)
    } else if let Ok(Some(snapshot)) = read_git_snapshot(root) {
        PathBuf::from(snapshot.repository_path)
    } else {
        task.git_verification = CodexTaskGitVerification {
            status: "unavailable".to_string(),
            checked_at: now_string(),
            reason: "没有关联 Git 仓库路径。".to_string(),
            ..CodexTaskGitVerification::default()
        };
        return;
    };
    task.repository_path = path_to_string(&repo);
    if task.reported_commits.is_empty() {
        task.git_verification = CodexTaskGitVerification {
            status: "unavailable".to_string(),
            repository_path: path_to_string(&repo),
            checked_at: now_string(),
            reason: "Codex 结果没有报告 commit。".to_string(),
        };
        return;
    }
    if let Err(reason) = verify_codex_task_commit_baseline(root, task, &repo) {
        task.git_verification = CodexTaskGitVerification {
            status: "mismatch".to_string(),
            repository_path: path_to_string(&repo),
            checked_at: now_string(),
            reason,
        };
        return;
    }
    let head = run_git(&repo, &["rev-parse", "HEAD"]);
    let mut verified = Vec::new();
    let mut missing = Vec::new();
    for commit in &task.reported_commits {
        let exists = run_git(&repo, &["cat-file", "-e", &format!("{commit}^{{commit}}")]).is_ok();
        let in_history = exists
            && head.is_ok()
            && run_git(&repo, &["merge-base", "--is-ancestor", commit, "HEAD"]).is_ok();
        if exists && in_history {
            verified.push(commit.clone());
            if let Ok(files) = run_git(&repo, &["show", "--name-only", "--format=", commit]) {
                task.changed_files = merge_unique(
                    &task.changed_files,
                    &files
                        .lines()
                        .map(str::trim)
                        .filter(|line| !line.is_empty())
                        .map(ToOwned::to_owned)
                        .collect::<Vec<_>>(),
                );
            }
        } else {
            missing.push(commit.clone());
        }
    }
    task.verified_commits = verified;
    let status = if missing.is_empty() {
        "verified"
    } else if task.verified_commits.is_empty() {
        "mismatch"
    } else {
        "partial"
    };
    task.git_verification = CodexTaskGitVerification {
        status: status.to_string(),
        repository_path: path_to_string(&repo),
        checked_at: now_string(),
        reason: if missing.is_empty() {
            "报告中的 commit 已在关联仓库历史中核验。".to_string()
        } else {
            format!("以下 commit 未在关联仓库历史中核验：{}", missing.join(", "))
        },
    };
}

fn verify_codex_task_commit_baseline(
    root: &Path,
    task: &CodexTask,
    repository_path: &Path,
) -> Result<(), String> {
    if task.result_run_id.trim().is_empty() {
        return Ok(());
    }
    let run = read_codex_runs(root)?
        .into_iter()
        .find(|run| run.id == task.result_run_id && run.task_id == task.task_id)
        .ok_or_else(|| "Codex 结果没有匹配的任务启动 Git 基线。".to_string())?;
    // Existing runs predate baseline persistence. They remain compatible, while every
    // newly started coding run must prove its reported commit descended from its start.
    if run.git_head_before.trim().is_empty() {
        return Ok(());
    }
    let baseline_paths = run
        .git_dirty_files_before
        .iter()
        .map(|file| normalize_key(&file.path))
        .filter(|path| !path.is_empty())
        .collect::<HashSet<_>>();
    for commit in &task.reported_commits {
        if run_git(
            repository_path,
            &["merge-base", "--is-ancestor", &run.git_head_before, commit],
        )
        .is_err()
        {
            return Err(format!(
                "报告的 commit {commit} 不在本任务启动前 HEAD {} 之后，不能作为本次任务证据。",
                run.git_head_before
            ));
        }
        if baseline_paths.is_empty() {
            continue;
        }
        let commit_files = run_git(
            repository_path,
            &["diff-tree", "--no-commit-id", "--name-only", "-r", commit],
        )?;
        let overlaps = commit_files
            .lines()
            .map(str::trim)
            .filter(|path| baseline_paths.contains(&normalize_key(path)))
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>();
        if !overlaps.is_empty() {
            return Err(format!(
                "本次任务开始前已存在未提交修改，报告的 commit 包含这些文件：{}。为避免混入无关改动，不能通过 Git 核验。",
                overlaps.join("、")
            ));
        }
    }
    Ok(())
}

fn wait_for_matching_codex_result(
    root: &Path,
    task_id: &str,
    run: &CodexRun,
    wait_for_result_file: bool,
) -> Result<(), String> {
    if let Some(latest) = read_codex_runs(root)?
        .into_iter()
        .find(|item| item.task_id == task_id)
    {
        if latest.id != run.id {
            return Err("当前执行已被更新的 CodexRun 替代，不能接收旧执行结果。".to_string());
        }
    }
    let task = read_codex_task(root, task_id)?;
    let json_path = codex_result_path_for_task(root, &task, "json");
    let markdown_path = codex_result_path_for_task(root, &task, "md");
    let has_current_result = || {
        [json_path.as_path(), markdown_path.as_path()]
            .into_iter()
            .any(|path| codex_result_file_matches_run(path, run))
    };
    if has_current_result() {
        return Ok(());
    }
    if wait_for_result_file {
        for _ in 0..20 {
            std::thread::sleep(Duration::from_millis(250));
            if has_current_result() {
                return Ok(());
            }
        }
    }

    if json_path.exists() || markdown_path.exists() {
        return Err(
            "本次执行未生成可匹配的结果；现有结果属于旧执行或无法解析，已保留，请检查后重试。"
                .to_string(),
        );
    }
    Err("Codex 已退出，但没有在当前项目 Result Bridge 目录写入匹配本次运行的结果。".to_string())
}

fn codex_result_path_for_task(root: &Path, task: &CodexTask, extension: &str) -> PathBuf {
    if extension == "json" && !task.expected_result_path.trim().is_empty() {
        return PathBuf::from(task.expected_result_path.trim());
    }
    codex_result_bridge_dir(root).join(format!("{}-result.{extension}", task.task_id))
}

fn canonical_codex_result_path(root: &Path, task_id: &str) -> PathBuf {
    codex_result_bridge_dir(root).join(format!("{task_id}-result.json"))
}

fn read_optional_text(path: &Path) -> Option<String> {
    fs::read_to_string(path).ok()
}

fn refresh_codex_task_git_verifications(root: &Path, repository_path: &str) -> Result<(), String> {
    for mut task in read_codex_tasks(root)? {
        if task.result_id.is_empty()
            || matches!(
                task.status.as_str(),
                "ready" | "running" | "awaitingResult" | "failed" | "cancelled"
            )
            || task.acceptance.status == "rejected"
        {
            continue;
        }
        if task.repository_path.trim().is_empty() {
            task.repository_path = repository_path.to_string();
        }
        verify_codex_task_against_git(root, &mut task);
        if task.result_source == "runnerFallback" && task.status != "completed" {
            write_codex_task(root, &task)?;
            continue;
        }
        if task.git_verification.status == "verified" {
            if !task.manual_acceptance.is_empty() && task.status != "completed" {
                task.status = "awaitingAcceptance".to_string();
            } else if matches!(
                task.status.as_str(),
                "resultReceived" | "verifying" | "needsReview"
            ) {
                task.status = "completed".to_string();
                if task.completed_at.is_empty() {
                    task.completed_at = now_string();
                }
            }
        } else if task.git_verification.status == "notApplicable"
            && matches!(task.status.as_str(), "resultReceived" | "verifying")
            && codex_result_has_useful_content(&task)
        {
            task.status = "completed".to_string();
            if task.completed_at.is_empty() {
                task.completed_at = now_string();
            }
        } else if task.status == "completed"
            && task.git_verification.status != "notApplicable"
            && task.acceptance.status != "approved"
        {
            task.status = "resultReceived".to_string();
        }
        task.updated_at = now_string();
        write_codex_task(root, &task)?;
    }
    Ok(())
}

fn codex_task_repository_path(root: &Path, task: &CodexTask) -> Result<PathBuf, String> {
    if !task.repository_path.trim().is_empty() {
        return Ok(PathBuf::from(task.repository_path.trim()));
    }
    if let Ok(manifest) = read_and_repair_manifest(root) {
        if let Some(path) = configured_repository_path(&manifest.project) {
            return Ok(path);
        }
    }
    read_git_snapshot(root)?
        .map(|snapshot| PathBuf::from(snapshot.repository_path))
        .ok_or_else(|| "CodexTask 没有关联 Git 仓库路径，请先刷新 Git 事实。".to_string())
}

fn configured_repository_path(project: &ProjectSummary) -> Option<PathBuf> {
    (!project.repository_path.trim().is_empty())
        .then(|| PathBuf::from(project.repository_path.trim()))
}

fn preferred_project_repository_path(root: &Path, manifest: &ProjectManifest) -> String {
    configured_repository_path(&manifest.project)
        .map(|path| path_to_string(&path))
        .or_else(|| {
            read_git_snapshot(root)
                .ok()
                .flatten()
                .map(|snapshot| snapshot.repository_path)
        })
        .unwrap_or_default()
}

fn active_codex_run_for_task(root: &Path, task_id: &str) -> Result<Option<CodexRun>, String> {
    Ok(read_codex_runs(root)?.into_iter().find(|run| {
        run.task_id == task_id
            && (run.status == "starting" || (run.status == "running" && process_exists(run.pid)))
    }))
}

fn recover_stale_codex_runs(root: &Path) -> Result<(), String> {
    let mut runs = read_codex_runs(root)?;
    let mut changed = false;
    let mut awaiting_result = Vec::new();
    let mut timed_out_tasks = Vec::new();
    let latest_run_ids = latest_codex_run_ids(&runs);
    for run in &mut runs {
        if matches!(run.status.as_str(), "starting" | "running") {
            let process_alive = run.pid != 0 && process_exists(run.pid);
            if run.process_alive != process_alive {
                run.process_alive = process_alive;
                changed = true;
            }
        }
        // Result Bridge is authoritative once a result is already bound to this run.
        // A concurrent list refresh can otherwise mistake the short CLI exit window
        // for an application restart and surface a false recovery warning.
        if matches!(
            run.status.as_str(),
            "starting" | "running" | "unknownAfterRestart"
        ) && codex_result_exists_for_run(root, run)
        {
            run.status = "exited".to_string();
            run.process_alive = false;
            if run.ended_at.trim().is_empty() {
                run.ended_at = now_string();
            }
            run.error.clear();
            changed = true;
            continue;
        }
        // A test may use the host PID as a liveness sentinel. A real Codex child
        // can never be the desktop process itself, so never terminate the host.
        let task_type = read_codex_task(root, &run.task_id)
            .map(|task| task.task_type)
            .unwrap_or_default();
        let execution_timed_out = run.status == "running"
            && run.pid != std::process::id()
            && process_exists(run.pid)
            && codex_run_execution_timed_out(run, &task_type)
            && !codex_result_exists_for_run(root, run);
        if execution_timed_out {
            if process_exists(run.pid) {
                let _ = terminate_process(run.pid);
            }
            run.status = "failed".to_string();
            run.process_alive = false;
            run.ended_at = now_string();
            run.error = codex_timeout_error(&task_type);
            changed = true;
            if latest_run_ids
                .get(&run.task_id)
                .is_some_and(|latest_id| latest_id == &run.id)
            {
                timed_out_tasks.push(run.task_id.clone());
            }
            continue;
        }
        let lost_running_process = run.status == "running" && !process_exists(run.pid);
        let expired_startup = run.status == "starting" && codex_startup_window_expired(run);
        if lost_running_process || expired_startup {
            run.status = "unknownAfterRestart".to_string();
            run.process_alive = false;
            run.ended_at = now_string();
            run.error =
                "应用重启后未检测到原 Codex 进程；请等待 Result Bridge 或查看仓库 Git 事实。"
                    .to_string();
            changed = true;
            if latest_run_ids
                .get(&run.task_id)
                .is_some_and(|latest_id| latest_id == &run.id)
                && read_codex_task(root, &run.task_id).is_ok()
                && !codex_result_exists_for_run(root, run)
            {
                awaiting_result.push(run.task_id.clone());
            }
        }
    }
    if changed {
        write_codex_runs(root, runs)?;
    }
    for task_id in awaiting_result {
        mark_codex_task_awaiting_result(root, &task_id)?;
    }
    for task_id in timed_out_tasks {
        let task_type = read_codex_task(root, &task_id)
            .map(|task| task.task_type)
            .unwrap_or_default();
        mark_codex_task_restart_recovery_expired(root, &task_id, &codex_timeout_error(&task_type))?;
    }
    sync_failed_codex_runs_to_tasks(root)
}

fn codex_expected_result_exists(root: &Path, task_id: &str) -> bool {
    let latest = read_codex_runs(root).ok().and_then(|runs| {
        runs.into_iter()
            .filter(|run| run.task_id == task_id)
            .max_by_key(codex_run_started_at)
    });
    latest
        .as_ref()
        .map(|run| codex_result_exists_for_run(root, run))
        .unwrap_or(false)
}

fn codex_run_started_at(run: &CodexRun) -> u128 {
    run.started_at.parse::<u128>().unwrap_or_default()
}

fn codex_startup_window_expired(run: &CodexRun) -> bool {
    let now = now_string().parse::<u128>().unwrap_or_default();
    now.saturating_sub(codex_run_started_at(run)) >= 30_000
}

fn codex_run_timeout(task_type: &str) -> Duration {
    if normalize_codex_task_type(task_type) == "coding" {
        CODEX_CODING_RUN_TIMEOUT
    } else {
        CODEX_STANDARD_RUN_TIMEOUT
    }
}

fn codex_timeout_error(task_type: &str) -> String {
    let timeout = codex_run_timeout(task_type).as_secs() / 60;
    if normalize_codex_task_type(task_type) == "coding" {
        format!(
            "Codex 代码任务执行超过 {timeout} 分钟仍未结束，已停止该任务进程。请检查 Codex 登录、审批、网络或仓库状态后重新执行。"
        )
    } else {
        format!(
            "Codex 任务执行超过 {timeout} 分钟仍未结束，已停止该任务进程。请检查 Codex 登录、审批、网络或仓库状态后重新执行。"
        )
    }
}

fn is_codex_timeout_error(error: &str) -> bool {
    error.contains("Codex 代码任务执行超过")
        || error.contains("Codex 任务执行超过")
        || error.contains("Codex 执行超过 3 分钟")
}

fn codex_run_execution_timed_out(run: &CodexRun, task_type: &str) -> bool {
    let now = now_string().parse::<u128>().unwrap_or_default();
    now.saturating_sub(codex_run_started_at(run)) >= codex_run_timeout(task_type).as_millis()
}

fn latest_codex_run_ids(runs: &[CodexRun]) -> HashMap<String, String> {
    let mut latest = HashMap::new();
    for run in runs {
        let replace = latest
            .get(&run.task_id)
            .and_then(|run_id: &String| runs.iter().find(|item| item.id == *run_id))
            .is_none_or(|current| codex_run_started_at(run) >= codex_run_started_at(current));
        if replace {
            latest.insert(run.task_id.clone(), run.id.clone());
        }
    }
    latest
}

fn codex_result_exists_for_run(root: &Path, run: &CodexRun) -> bool {
    let Ok(task) = read_codex_task(root, &run.task_id) else {
        return false;
    };
    ["json", "md"]
        .into_iter()
        .map(|extension| codex_result_path_for_task(root, &task, extension))
        .any(|path| codex_result_file_matches_run(&path, run))
}

fn codex_restart_result_grace_expired(run: &CodexRun) -> bool {
    let recovered_at = run.ended_at.parse::<u128>().ok();
    let now = now_string().parse::<u128>().unwrap_or_default();
    recovered_at
        .map(|value| now.saturating_sub(value) >= CODEX_RESTART_RESULT_GRACE_MS)
        .unwrap_or(false)
}

fn mark_codex_task_restart_recovery_expired(
    root: &Path,
    task_id: &str,
    reason: &str,
) -> Result<(), String> {
    let mut task = read_codex_task(root, task_id)?;
    if matches!(
        task.status.as_str(),
        "resultReceived" | "awaitingAcceptance" | "completed" | "cancelled"
    ) {
        return Ok(());
    }
    task.status = "needsReview".to_string();
    task.updated_at = now_string();
    if !task
        .remaining_issues
        .iter()
        .any(|item| item.trim() == reason.trim())
    {
        task.remaining_issues.push(reason.to_string());
    }
    write_codex_task(root, &task)
}

fn codex_result_file_matches_run(path: &Path, run: &CodexRun) -> bool {
    let Ok(text) = fs::read_to_string(path) else {
        return false;
    };
    codex_result_text_matches_run(path, &text, run)
}

fn codex_run_finished_with_bound_result(run: &CodexRun, result_path: &Path) -> bool {
    run.status == "exited"
        && run.exit_code.map(|code| code != 0).unwrap_or(false)
        && run.error.contains("已收到本次真实 Codex 结果")
        && codex_result_file_matches_run(result_path, run)
}

fn codex_result_text_matches_run(path: &Path, text: &str, run: &CodexRun) -> bool {
    let (task_id, result_run_id) = if path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.eq_ignore_ascii_case("json"))
        .unwrap_or(false)
    {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
            return false;
        };
        (
            json_string_field(&value, &["taskId", "task_id"]),
            json_string_field(&value, &["resultRunId", "runId", "result_run_id"]),
        )
    } else {
        (
            extract_labeled_token(&text, &["taskId", "task_id"]),
            extract_labeled_token(&text, &["resultRunId", "runId", "result_run_id"]),
        )
    };
    if !task_id.is_empty() && task_id != run.task_id {
        return false;
    }
    if !result_run_id.is_empty() {
        return result_run_id == run.id;
    }
    // Legacy results have no run ID. They must at least postdate this run.
    let Ok(started_at) = run.started_at.parse::<u128>() else {
        return true;
    };
    path.metadata()
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
        .map(|modified| modified.as_millis() >= started_at)
        .unwrap_or(false)
}

fn repair_codex_run_failure_messages(root: &Path) -> Result<(), String> {
    let mut runs = read_codex_runs(root)?;
    let mut changed = false;
    for run in &mut runs {
        if run.status == "failed"
            && !run.stderr_summary.trim().is_empty()
            && (run.error.trim().is_empty() || run.error.starts_with("codex exec 非 0 退出"))
        {
            run.error =
                codex_run_failure_reason(run.exit_code, &run.stderr_summary, &run.stdout_summary);
            changed = true;
        }
    }
    if changed {
        write_codex_runs(root, runs)?;
    }
    Ok(())
}

fn sync_failed_codex_runs_to_tasks(root: &Path) -> Result<(), String> {
    let runs = read_codex_runs(root)?;
    let latest_ids = latest_codex_run_ids(&runs);
    let latest_by_task: HashMap<String, CodexRun> = runs
        .into_iter()
        .filter(|run| latest_ids.get(&run.task_id).is_some_and(|id| id == &run.id))
        .map(|run| (run.task_id.clone(), run))
        .collect();
    for (task_id, run) in latest_by_task {
        if !matches!(run.status.as_str(), "failed" | "unknownAfterRestart") {
            continue;
        }
        let task = match read_codex_task(root, &task_id) {
            Ok(task) => task,
            Err(_) => continue,
        };
        if run.status == "unknownAfterRestart" {
            if codex_expected_result_exists(root, &task_id) {
                continue;
            }
            if !codex_restart_result_grace_expired(&run) {
                mark_codex_task_awaiting_result(root, &task_id)?;
                continue;
            }
            mark_codex_task_restart_recovery_expired(
                root,
                &task_id,
                "应用重启后未收到匹配的 Codex 结果；恢复窗口已结束，请检查仓库后重新执行。",
            )?;
            continue;
        }
        if matches!(
            task.status.as_str(),
            "resultReceived" | "awaitingAcceptance" | "completed" | "cancelled"
        ) || (!task.result_id.trim().is_empty() && task.status != "running")
        {
            continue;
        }
        if is_codex_timeout_error(&run.error) {
            mark_codex_task_restart_recovery_expired(root, &task_id, &run.error)?;
            continue;
        }
        mark_codex_task_run_failed(root, &task_id, &run.error)?;
    }
    Ok(())
}

fn codex_command_name() -> String {
    #[cfg(target_os = "windows")]
    {
        if let Some(path) = discover_native_codex_exe() {
            return path_to_string(&path);
        }
        if let Some(path) = discover_path_codex_cmd() {
            return path_to_string(&path);
        }
        return "codex.cmd".to_string();
    }
    #[cfg(not(target_os = "windows"))]
    {
        "codex".to_string()
    }
}

#[cfg(target_os = "windows")]
fn discover_path_codex_cmd() -> Option<PathBuf> {
    let output = hidden_command("where.exe").arg("codex.cmd").output().ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .map(PathBuf::from)
        .find(|path| path.is_file())
}

#[cfg(target_os = "windows")]
fn discover_native_codex_exe() -> Option<PathBuf> {
    let bin_dir = PathBuf::from(std::env::var_os("LOCALAPPDATA")?)
        .join("OpenAI")
        .join("Codex")
        .join("bin");
    let mut candidates: Vec<(SystemTime, PathBuf)> = Vec::new();
    for entry in fs::read_dir(bin_dir).ok()?.flatten() {
        let candidate = entry.path().join("codex.exe");
        if !candidate.is_file() {
            continue;
        }
        let modified = candidate
            .metadata()
            .and_then(|metadata| metadata.modified())
            .unwrap_or(UNIX_EPOCH);
        candidates.push((modified, candidate));
    }
    candidates
        .into_iter()
        .max_by_key(|(modified, _)| *modified)
        .map(|(_, path)| path)
}

fn codex_exec_args(
    repo: &Path,
    result_directory: &Path,
    output_last_message: &Path,
    probe: &CodexCliCapabilityProbe,
    native_executable: bool,
) -> Result<Vec<String>, String> {
    let mut missing = Vec::new();
    if !probe.supports_cd {
        missing.push("-C/--cd");
    }
    if !probe.supports_add_dir {
        missing.push("--add-dir");
    }
    if !probe.supports_json {
        missing.push("--json");
    }
    if !probe.supports_output_last_message {
        missing.push("--output-last-message");
    }
    if !native_executable && !probe.supports_sandbox {
        missing.push("--sandbox");
    }
    if !probe.supports_stdin {
        missing.push("stdin prompt");
    }
    if !missing.is_empty() {
        return Err(format!(
            "当前 Codex CLI 缺少非交互执行能力：{}。请升级 Codex CLI 后重试。",
            missing.join(", ")
        ));
    }
    let mut args = vec![
        "exec".to_string(),
        "-C".to_string(),
        path_to_string(repo),
        "--add-dir".to_string(),
        path_to_string(result_directory),
        "--json".to_string(),
        "--output-last-message".to_string(),
        path_to_string(output_last_message),
    ];
    if native_executable {
        // Current native Codex supports this safe workspace-write approval mode,
        // while combining it with --sandbox is rejected by the CLI.
        args.extend([
            "--approve-for-me".to_string(),
            "--ignore-user-config".to_string(),
        ]);
    } else {
        args.extend(["--sandbox".to_string(), "workspace-write".to_string()]);
    }
    args.push("-".to_string());
    Ok(args)
}

fn probe_codex_cli_capabilities(executable: &str) -> CodexCliCapabilityProbe {
    let checked_at = now_string();
    let help_output = hidden_command(executable).args(["exec", "--help"]).output();
    match help_output {
        Ok(output) if output.status.success() => parse_codex_cli_capability_probe(
            "Codex CLI（exec 能力已验证）",
            &String::from_utf8_lossy(&output.stdout),
            checked_at,
        ),
        Ok(output) => CodexCliCapabilityProbe {
            available: false,
            version: "Codex CLI".to_string(),
            checked_at,
            failure_reason: first_line(&String::from_utf8_lossy(&output.stderr), 240),
            ..CodexCliCapabilityProbe::default()
        },
        Err(err) => CodexCliCapabilityProbe {
            available: false,
            version: "Codex CLI".to_string(),
            checked_at,
            failure_reason: format!("执行 codex exec --help 失败：{err}"),
            ..CodexCliCapabilityProbe::default()
        },
    }
}

fn is_native_codex_executable(executable: &str) -> bool {
    Path::new(executable)
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.eq_ignore_ascii_case("exe"))
        .unwrap_or(false)
}

fn parse_codex_cli_capability_probe(
    version: &str,
    help: &str,
    checked_at: String,
) -> CodexCliCapabilityProbe {
    CodexCliCapabilityProbe {
        available: true,
        version: version.trim().to_string(),
        supports_cd: help.contains("-C, --cd") || help.contains("--cd <"),
        supports_add_dir: help.contains("--add-dir"),
        supports_json: help.contains("--json"),
        supports_output_last_message: help.contains("--output-last-message"),
        supports_sandbox: help.contains("--sandbox"),
        supports_stdin: help.contains("[PROMPT]")
            || help.contains("read from stdin")
            || help.contains("Use `-`"),
        checked_at,
        failure_reason: String::new(),
    }
}

fn codex_run_failure_reason(exit_code: Option<i32>, stderr: &str, stdout: &str) -> String {
    let combined = format!("{stderr}\n{stdout}");
    let lower = combined.to_ascii_lowercase();
    let detail = first_line(combined.trim(), 240);
    if lower.contains("unexpected argument") || lower.contains("unrecognized option") {
        return format!("Codex CLI 参数无效：{detail}");
    }
    if lower.contains("not logged in")
        || lower.contains("login")
        || lower.contains("auth")
        || lower.contains("authentication")
        || lower.contains("unauthorized")
    {
        return format!("Codex CLI 未登录或认证失败：{detail}");
    }
    if lower.contains("no such file")
        || lower.contains("cannot find")
        || lower.contains("not found")
        || lower.contains("does not exist")
    {
        return format!("Codex CLI 找不到所需路径或文件：{detail}");
    }
    if detail.is_empty() {
        format!("codex exec 非 0 退出，退出码：{exit_code:?}")
    } else {
        format!("codex exec 非 0 退出，退出码：{exit_code:?}：{detail}")
    }
}

fn mark_codex_task_run_failed(root: &Path, task_id: &str, reason: &str) -> Result<(), String> {
    let mut task = read_codex_task(root, task_id)?;
    if matches!(task.status.as_str(), "completed" | "awaitingAcceptance") {
        return Ok(());
    }
    if task.status == "failed"
        && (reason.trim().is_empty()
            || task
                .remaining_issues
                .iter()
                .any(|item| item.trim() == reason.trim()))
    {
        return Ok(());
    }
    task.status = "failed".to_string();
    task.updated_at = now_string();
    if !reason.trim().is_empty()
        && !task
            .remaining_issues
            .iter()
            .any(|item| item.trim() == reason.trim())
    {
        task.remaining_issues.push(reason.to_string());
    }
    write_codex_task(root, &task)
}

fn mark_codex_task_awaiting_result(root: &Path, task_id: &str) -> Result<(), String> {
    // Serialize the exit callback with result import before reading task state.
    let _guard = project_write_lock()
        .lock()
        .map_err(|_| "项目存储锁已损坏，请重启感冒院。".to_string())?;
    let mut task = read_codex_task(root, task_id)?;
    // A result or user decision may arrive before the process exit callback.
    if !matches!(
        task.status.as_str(),
        "ready" | "draft" | "handedOff" | "running" | "awaitingResult"
    ) {
        return Ok(());
    }
    task.status = "awaitingResult".to_string();
    task.updated_at = now_string();
    write_codex_task(root, &task)
}

fn summarize_process_output(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .take(20)
        .collect::<Vec<_>>()
        .join("\n");
    first_line(&text, 1800)
}

fn spawn_codex_output_reader<R>(
    mut reader: R,
    live: Arc<Mutex<LiveCodexOutput>>,
    stream: &'static str,
) -> std::thread::JoinHandle<std::io::Result<Vec<u8>>>
where
    R: Read + Send + 'static,
{
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let mut buffer = [0_u8; 4096];
        let mut pending = String::new();
        loop {
            let count = reader.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            bytes.extend_from_slice(&buffer[..count]);
            pending.push_str(&String::from_utf8_lossy(&buffer[..count]));
            let has_trailing_newline = pending.ends_with('\n');
            let mut lines = pending.split('\n').map(str::to_string).collect::<Vec<_>>();
            if !has_trailing_newline {
                pending = lines.pop().unwrap_or_default();
            } else {
                pending.clear();
            }
            for line in lines {
                record_codex_live_activity(&live, stream, &line);
            }
        }
        if !pending.trim().is_empty() {
            record_codex_live_activity(&live, stream, &pending);
        }
        Ok(bytes)
    })
}

fn record_codex_live_activity(live: &Arc<Mutex<LiveCodexOutput>>, stream: &str, raw: &str) {
    let text = humanize_codex_output_line(raw);
    if text.is_empty() {
        return;
    }
    let activity = format!("{stream}：{text}");
    let Ok(mut live) = live.lock() else {
        return;
    };
    if live
        .recent_activity
        .last()
        .is_some_and(|previous| previous == &activity)
    {
        return;
    }
    live.recent_activity.push(activity);
    if live.recent_activity.len() > CODEX_LIVE_ACTIVITY_LIMIT {
        let excess = live.recent_activity.len() - CODEX_LIVE_ACTIVITY_LIMIT;
        live.recent_activity.drain(..excess);
    }
    live.last_activity_at = now_string();
}

fn humanize_codex_output_line(raw: &str) -> String {
    let trimmed =
        raw.trim_matches(|character: char| character.is_whitespace() || character == '\0');
    if trimmed.is_empty() {
        return String::new();
    }
    let text = serde_json::from_str::<serde_json::Value>(trimmed)
        .ok()
        .and_then(|value| {
            let object = value.as_object()?;
            [
                "message", "text", "summary", "event", "type", "command", "cmd", "path", "file",
            ]
            .into_iter()
            .filter_map(|key| object.get(key).and_then(|item| item.as_str()))
            .find(|item| !item.trim().is_empty())
            .map(str::to_string)
        })
        .unwrap_or_else(|| trimmed.to_string());
    first_line(&text, CODEX_LIVE_ACTIVITY_MAX_CHARS)
}

fn persist_live_codex_activity(root: &Path, run_id: &str, live: &Arc<Mutex<LiveCodexOutput>>) {
    let Ok(snapshot) = live.lock().map(|value| {
        (
            value.last_activity_at.clone(),
            value.recent_activity.clone(),
        )
    }) else {
        return;
    };
    let Ok(mut run) = read_codex_run(root, run_id) else {
        return;
    };
    if !matches!(run.status.as_str(), "starting" | "running") {
        return;
    }
    if run.last_activity_at == snapshot.0 && run.recent_activity == snapshot.1 {
        return;
    }
    run.last_activity_at = snapshot.0;
    run.recent_activity = snapshot.1;
    let _ = upsert_codex_run(root, run);
}

fn wait_for_codex_process(
    child: &mut Child,
    pid: u32,
    root: &Path,
    task_id: &str,
    run: &CodexRun,
    task_type: &str,
) -> Result<CodexProcessOutput, String> {
    // `--json` can produce enough events to fill an unread pipe. Reader threads keep
    // Codex unblocked while the main thread owns the timeout and process-tree cleanup.
    let live = Arc::new(Mutex::new(LiveCodexOutput {
        recent_activity: run.recent_activity.clone(),
        last_activity_at: run.last_activity_at.clone(),
    }));
    let stdout_reader = child
        .stdout
        .take()
        .map(|stdout| spawn_codex_output_reader(stdout, Arc::clone(&live), "stdout"));
    let stderr_reader = child
        .stderr
        .take()
        .map(|stderr| spawn_codex_output_reader(stderr, Arc::clone(&live), "stderr"));

    let started = Instant::now();
    let run_timeout = codex_run_timeout(task_type);
    let mut result_received_at = None;
    let mut stopped_after_result = false;
    let mut last_activity_persisted = Instant::now();
    let (status, timed_out) = loop {
        if last_activity_persisted.elapsed() >= Duration::from_secs(1) {
            persist_live_codex_activity(root, &run.id, &live);
            last_activity_persisted = Instant::now();
        }
        match child.try_wait() {
            Ok(Some(status)) => break (status, false),
            Ok(None) => {
                // The result file is the task's fact boundary. Some Windows CLI runs keep
                // their parent process alive after writing it, so import it before waiting
                // for process exit and avoid misclassifying a completed task as a timeout.
                if result_received_at.is_none()
                    && wait_for_matching_codex_result(root, task_id, run, false).is_ok()
                {
                    let _ = scan_codex_result_bridge(path_to_string(root));
                    if read_codex_task(root, task_id)
                        .map(|task| {
                            !task.result_received_at.is_empty() && task.result_run_id == run.id
                        })
                        .unwrap_or(false)
                    {
                        result_received_at = Some(Instant::now());
                    }
                }
                if result_received_at
                    .map(|received_at| received_at.elapsed() >= CODEX_RESULT_EXIT_GRACE)
                    .unwrap_or(false)
                {
                    if process_exists(pid) {
                        let _ = terminate_process(pid);
                    }
                    if process_exists(pid) {
                        let _ = child.kill();
                    }
                    let status = child
                        .wait()
                        .map_err(|error| format!("停止已回流结果的 Codex 进程失败：{error}"))?;
                    stopped_after_result = true;
                    break (status, false);
                }
                if started.elapsed() >= run_timeout {
                    terminate_process_if_running(pid)?;
                    let status = child
                        .wait()
                        .map_err(|error| format!("停止超时的 Codex 进程失败：{error}"))?;
                    break (status, true);
                }
                std::thread::sleep(CODEX_RUN_POLL_INTERVAL);
            }
            Err(error) => {
                if process_exists(pid) {
                    let _ = terminate_process(pid);
                }
                return Err(format!("读取 Codex 进程状态失败：{error}"));
            }
        }
    };

    let read_pipe = |reader: Option<std::thread::JoinHandle<std::io::Result<Vec<u8>>>>| {
        reader
            .map(|reader| {
                reader
                    .join()
                    .map_err(|_| "读取 Codex 输出线程异常退出。".to_string())?
                    .map_err(|error| format!("读取 Codex 输出失败：{error}"))
            })
            .transpose()
            .map(|value| value.unwrap_or_default())
    };
    let recent_activity = live
        .lock()
        .map(|value| {
            (
                value.last_activity_at.clone(),
                value.recent_activity.clone(),
            )
        })
        .unwrap_or_default();

    Ok(CodexProcessOutput {
        exit_code: status.code(),
        stdout: read_pipe(stdout_reader)?,
        stderr: read_pipe(stderr_reader)?,
        timed_out,
        result_received: result_received_at.is_some(),
        stopped_after_result,
        last_activity_at: recent_activity.0,
        recent_activity: recent_activity.1,
    })
}

fn process_exists(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    if cfg!(target_os = "windows") {
        let output = hidden_command("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
            .output();
        return output
            .ok()
            .map(|output| {
                output.status.success()
                    && String::from_utf8_lossy(&output.stdout)
                        .to_ascii_lowercase()
                        .contains(&pid.to_string())
            })
            .unwrap_or(false);
    }
    Command::new("kill")
        .args(["-0", &pid.to_string()])
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn terminate_process(pid: u32) -> Result<(), String> {
    if pid == 0 {
        return Err("Codex 进程 pid 无效。".to_string());
    }
    let status = if cfg!(target_os = "windows") {
        hidden_command("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .status()
    } else {
        Command::new("kill").arg(pid.to_string()).status()
    }
    .map_err(|err| format!("停止 Codex 进程失败：{err}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("停止 Codex 进程失败，退出码：{:?}", status.code()))
    }
}

fn terminate_process_if_running(pid: u32) -> Result<(), String> {
    if !process_exists(pid) {
        return Ok(());
    }
    match terminate_process(pid) {
        Ok(()) => Ok(()),
        Err(_error) if !process_exists(pid) => Ok(()),
        Err(error) => Err(error),
    }
}

fn codex_task_id_from_report(report: &CodexReportRecord) -> String {
    let from_summary = extract_task_id_from_text(&report.summary);
    if !from_summary.is_empty() {
        return from_summary;
    }
    codex_task_id_from_source_label(report)
}

fn task_display_title(task: &CodexTask) -> String {
    if task.title.trim().is_empty() {
        task.task_id.clone()
    } else {
        task.title.clone()
    }
}

fn merge_unique(existing: &[String], incoming: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut merged = Vec::new();
    for item in existing.iter().chain(incoming.iter()) {
        let item = item.trim();
        if item.is_empty() || !seen.insert(normalize_key(item)) {
            continue;
        }
        merged.push(item.to_string());
    }
    merged
}

fn extract_task_id_from_text(text: &str) -> String {
    extract_labeled_token(text, &["taskId", "task_id", "任务ID", "任务"])
}

fn extract_thread_id_from_text(text: &str) -> String {
    extract_labeled_token(text, &["externalThreadId", "threadId", "线程", "任务链接"])
}

fn extract_labeled_token(text: &str, labels: &[&str]) -> String {
    for line in text.lines() {
        if !labels.iter().any(|label| line.contains(label)) {
            continue;
        }
        if let Some((_, value)) = line.split_once(':').or_else(|| line.split_once('：')) {
            return first_line(value.trim(), 80);
        }
    }
    String::new()
}

fn extract_changed_files_from_lines(lines: &[String]) -> Vec<String> {
    lines
        .iter()
        .filter(|line| line.contains('.') || line.contains('/') || line.contains('\\'))
        .map(|line| first_line(line, 160))
        .collect()
}

fn extract_artifacts_from_lines(lines: &[String]) -> Vec<String> {
    collect_lines_by_keywords(
        &lines.iter().map(|item| item.as_str()).collect::<Vec<_>>(),
        &["报告", "安装包", "artifact", "产物", "输出"],
    )
}

pub fn register_generated_file(
    project_root: String,
    file_name: String,
    content: String,
    created_source: String,
    related_task: String,
    purpose: String,
) -> Result<GeneratedFileResult, String> {
    let root = PathBuf::from(project_root);
    let _guard = project_write_lock()
        .lock()
        .map_err(|_| "项目存储锁已损坏，请重启感冒院。".to_string())?;
    let mut manifest = read_and_repair_manifest(&root)?;
    let original = manifest.clone();
    let file_type = Path::new(&file_name)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("txt")
        .to_lowercase();
    let plan = decide_file_location(
        &root,
        &manifest.files,
        &file_name,
        &file_type,
        &created_source,
        &related_task,
        &purpose,
    )?;
    write_text_atomic(&plan.destination, &content)?;
    let content_hash = hash_file(&plan.destination)?;
    let size_bytes = fs::metadata(&plan.destination)
        .map_err(|err| {
            format!(
                "读取生成文件信息失败：{}：{err}",
                path_to_string(&plan.destination)
            )
        })?
        .len();
    let now = now_string();
    let file_id = new_id();
    let (mut analysis, extracted_text) = analyze_managed_file(
        &file_id,
        &file_name,
        &plan.destination,
        &file_type,
        &content_hash,
    );
    if let Some(text) = extracted_text {
        let content_path = extracted_text_path(&root, &file_id);
        write_text_atomic(&content_path, &text)?;
        analysis.extracted_text_path = path_to_string(&content_path);
    }
    let record = FileRecord {
        id: file_id.clone(),
        file_name: file_name.clone(),
        original_source_path: String::new(),
        managed_path: path_to_string(&plan.destination),
        file_type,
        category: plan.category.clone(),
        created_source: created_source.clone(),
        related_task: related_task.clone(),
        current_version: format!("v{}", plan.version_number),
        history_versions: Vec::new(),
        created_at: now.clone(),
        modified_at: now.clone(),
        still_exists: true,
        needs_user_confirmation: false,
        parse_status: analysis.parse_status,
        content_summary: analysis.content_summary,
        main_fields_or_sections: analysis.main_fields_or_sections,
        recommended_category: analysis.recommended_category,
        parse_failure_reason: analysis.parse_failure_reason,
        parser: analysis.parser,
        extracted_text_path: analysis.extracted_text_path,
        page_count: analysis.page_count,
        sheet_count: analysis.sheet_count,
        row_count: analysis.row_count,
        column_count: analysis.column_count,
        analysis_warnings: analysis.warnings,
        content_hash,
        size_bytes,
        version_group_id: plan.version_group_id.clone(),
        duplicate_of_file_id: None,
        previous_version_id: plan.previous_version_id,
        source_paths: Vec::new(),
        managed_relative_path: plan.relative_path.clone(),
        location_reason: plan.reason.clone(),
        import_transaction_id: new_id(),
        last_verified_at: now.clone(),
        original_file_name: file_name.clone(),
        document_family_id: plan.version_group_id.clone(),
        version_id: file_id.clone(),
        version_number: plan.version_number,
    };
    manifest.files.push(record.clone());
    manifest.location_decisions.push(location_decision(
        &record,
        "",
        &created_source,
        &related_task,
        &purpose,
        &plan.reason,
        false,
    ));
    let mut message = workspace_message(
        "ganmaoyuan",
        "fileLocation",
        format!(
            "已通过统一位置服务保存文件：{}。位置：{}。原因：{}",
            record.file_name, record.managed_relative_path, record.location_reason
        ),
        "location_service",
    );
    message.attachments = vec![MessageAttachment {
        file_id: record.id.clone(),
        file_name: record.file_name.clone(),
        managed_path: record.managed_path.clone(),
        ..MessageAttachment::default()
    }];
    record_message_derivatives(&mut manifest, &message);
    record_imported_files_in_session(&mut manifest, vec![record.id.clone()]);
    manifest.messages.push(message.clone());
    manifest.audit.push(audit_event(
        "file.generated",
        &record.managed_path,
        "success",
        format!("来源：{created_source}；用途：{purpose}；关联任务：{related_task}"),
        false,
        true,
    ));
    if let Err(err) = persist_project(&root, &manifest, Some(&original)) {
        let _ = fs::remove_file(&plan.destination);
        return Err(err);
    }
    append_workspace_event(&root, &message)?;
    append_audit_event(&root, manifest.audit.last().expect("audit event exists"))?;
    Ok(GeneratedFileResult {
        file: record,
        message,
    })
}

pub fn list_memos<R: Runtime>(app: &tauri::AppHandle<R>) -> Result<Vec<MemoItem>, String> {
    let path = memos_path(app)?;
    if !path.exists() {
        return Ok(Vec::new());
    }
    read_json(&path)
}

pub fn save_memos<R: Runtime>(
    app: &tauri::AppHandle<R>,
    memos: Vec<MemoItem>,
) -> Result<Vec<MemoItem>, String> {
    let _guard = project_write_lock()
        .lock()
        .map_err(|_| "全局存储锁已损坏，请重启感冒院。".to_string())?;
    write_json_atomic(&memos_path(app)?, &memos)?;
    Ok(memos)
}

pub fn list_material_inbox<R: Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<Vec<MaterialInboxItem>, String> {
    let mut document = read_material_inbox_document(app)?;
    document
        .items
        .sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
    Ok(document.items)
}

pub fn recover_interrupted_inbox_routes<R: Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<(), String> {
    let _guard = project_write_lock()
        .lock()
        .map_err(|_| "全局存储锁已损坏，请重启感冒院。".to_string())?;
    let mut document = read_material_inbox_document(app)?;
    let mut changed = false;
    for item in &mut document.items {
        if item.processing_status == inbox_routing::ROUTING {
            item.failed_stage = "routing".to_string();
            if let Some(operation) = &mut item.route_operation {
                operation.status = "failed".to_string();
                operation.failure_reason = "应用在归位完成前中断".to_string();
            }
            inbox_routing::transition(
                item,
                inbox_routing::FAILED,
                "检测到上次归位在提交中中断，可安全重试。",
                "system",
                Some(("interruptedRoute", "应用在归位完成前中断")),
            )?;
            changed = true;
        }
    }
    if changed {
        document.updated_at = now_string();
        write_material_inbox_document(app, &document)?;
    }
    Ok(())
}

pub fn list_inbox_entries<R: Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<Vec<MaterialInboxItem>, String> {
    list_material_inbox(app)
}

pub fn ingest_material_inbox_files<R: Runtime>(
    app: &tauri::AppHandle<R>,
    file_paths: Vec<String>,
) -> Result<Vec<MaterialInboxItem>, String> {
    ingest_material_inbox_files_with_source(app, file_paths, "appImport")
}

pub fn ingest_material_inbox_files_with_source<R: Runtime>(
    app: &tauri::AppHandle<R>,
    file_paths: Vec<String>,
    received_from: &str,
) -> Result<Vec<MaterialInboxItem>, String> {
    if file_paths.is_empty() {
        return Ok(Vec::new());
    }
    let _guard = project_write_lock()
        .lock()
        .map_err(|_| "全局存储锁已损坏，请重启感冒院。".to_string())?;
    let mut document = read_material_inbox_document(app)?;
    let registry = read_registry(app)?;
    let project_contexts = registry
        .projects
        .into_iter()
        .filter_map(|project| {
            let root = PathBuf::from(&project.root_dir);
            read_and_repair_manifest(&root)
                .ok()
                .map(|manifest| (project, manifest))
        })
        .collect::<Vec<_>>();
    let general_history = read_global_files_document(app)?.files;
    let mut changed_items = Vec::new();
    let mut seen = HashSet::new();

    for raw_path in file_paths {
        if !seen.insert(normalize_key(&raw_path)) {
            continue;
        }
        let source = match canonical_existing_file(Path::new(&raw_path)) {
            Ok(source) => source,
            Err(err) => {
                let item =
                    failed_inbox_item(&raw_path, received_from, "sourceMissing", "receiving", &err);
                upsert_material_inbox_item(&mut document.items, item.clone());
                changed_items.push(item);
                continue;
            }
        };
        let source_path = path_to_string(&source);
        let source_hash = match hash_file(&source) {
            Ok(hash) => hash,
            Err(err) => {
                let item =
                    failed_inbox_item(&source_path, received_from, "hashFailed", "hashing", &err);
                upsert_material_inbox_item(&mut document.items, item.clone());
                changed_items.push(item);
                continue;
            }
        };
        let size_bytes = match fs::metadata(&source) {
            Ok(metadata) => metadata.len(),
            Err(err) => {
                let message = format!("读取收件箱文件信息失败：{}：{err}", source_path);
                let item = failed_inbox_item(
                    &source_path,
                    received_from,
                    "metadataFailed",
                    "receiving",
                    &message,
                );
                upsert_material_inbox_item(&mut document.items, item.clone());
                changed_items.push(item);
                continue;
            }
        };
        let file_name = source
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| format!("无法识别文件名称：{}", source_path))?
            .to_string();
        let file_type = source
            .extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or("unknown")
            .to_lowercase();
        if let Some(existing_index) = document
            .items
            .iter()
            .position(|item| !item.source_hash.is_empty() && item.source_hash == source_hash)
        {
            let refreshed = process_repeated_inbox_file(
                &document.items[existing_index],
                &project_contexts,
                &general_history,
                &source,
                &source_hash,
                size_bytes,
                received_from,
            )?;
            document.items[existing_index] = refreshed.clone();
            changed_items.push(refreshed);
            continue;
        }
        let inbox_id = new_id();
        let (analysis, _) =
            analyze_managed_file(&inbox_id, &file_name, &source, &file_type, &source_hash);
        let item = build_material_inbox_item_with_history(
            &project_contexts,
            &general_history,
            &source_path,
            &source_hash,
            size_bytes,
            &analysis,
            received_from,
        )?;
        upsert_material_inbox_item(&mut document.items, item.clone());
        changed_items.push(item);
    }
    let changed_ids = changed_items
        .iter()
        .map(|item| item.id.clone())
        .collect::<HashSet<_>>();
    reassess_batch_project_candidates(&mut document.items, &changed_ids, &project_contexts);
    for item in document
        .items
        .iter_mut()
        .filter(|item| changed_ids.contains(&item.id))
    {
        sync_inbox_decision_traces(item);
    }
    changed_items = document
        .items
        .iter()
        .filter(|item| changed_ids.contains(&item.id))
        .cloned()
        .collect();
    document.updated_at = now_string();
    write_material_inbox_document(app, &document)?;
    drop(_guard);
    for item in &mut changed_items {
        if should_auto_route_general_material(item) {
            let _ = route_inbox_item_to_global_internal(
                app,
                item.id.clone(),
                inbox_routing::GLOBAL_DEST_GENERAL.to_string(),
                true,
            );
            if let Some(updated) = list_material_inbox(app)?
                .into_iter()
                .find(|entry| entry.id == item.id)
            {
                *item = updated;
            }
        }
    }
    Ok(changed_items)
}

fn should_auto_route_general_material(item: &MaterialInboxItem) -> bool {
    item.ownership_type == inbox_routing::OWNERSHIP_GENERAL_WORK
        && item.confidence_score >= 78
        && item.confidence_level == "high"
        && item.general_material_category != "other"
        && valid_general_material_path(
            &item.general_material_domain,
            &item.general_material_category,
        )
        .is_some()
        && matches!(
            item.processing_status.as_str(),
            inbox_routing::PENDING_REVIEW | inbox_routing::READY_TO_ROUTE
        )
}

fn reassess_batch_project_candidates(
    items: &mut [MaterialInboxItem],
    changed_ids: &HashSet<String>,
    projects: &[(ProjectSummary, ProjectManifest)],
) {
    let changed = items
        .iter()
        .filter(|item| changed_ids.contains(&item.id))
        .cloned()
        .collect::<Vec<_>>();
    for item in items
        .iter_mut()
        .filter(|item| changed_ids.contains(&item.id))
    {
        if item.parse_status != "success"
            || item.material_semantic_type == "masterDataOrTemplate"
            || matches!(
                item.ownership_type.as_str(),
                inbox_routing::OWNERSHIP_EXISTING_PROJECT
                    | inbox_routing::OWNERSHIP_TEMPORARY
                    | inbox_routing::OWNERSHIP_UNSUPPORTED
            )
            || matches!(item.judgement_status.as_str(), "duplicate" | "new_version")
        {
            continue;
        }
        let parent = Path::new(&item.source_path)
            .parent()
            .map(path_to_string)
            .map(|value| normalize_key(&value))
            .unwrap_or_default();
        let related = changed
            .iter()
            .filter(|candidate| {
                Path::new(&candidate.source_path)
                    .parent()
                    .map(path_to_string)
                    .map(|value| normalize_key(&value))
                    .unwrap_or_default()
                    == parent
                    && candidate.parse_status == "success"
                    && candidate.material_semantic_type != "masterDataOrTemplate"
            })
            .cloned()
            .collect::<Vec<_>>();
        let assessment = assess_project_candidate(item, &related, projects);
        let may_override_general = item.ownership_type != inbox_routing::OWNERSHIP_GENERAL_WORK
            || (assessment.score >= 75
                && assessment.related_count >= 3
                && assessment.complementary_roles >= 2);
        if assessment.score < 12 || !may_override_general {
            continue;
        }
        apply_project_candidate_assessment(item, assessment);
        item.confidence_score = item.project_candidate_score;
        item.confidence_level = item.project_candidate_confidence.clone();
        item.confidence_reasons = item.project_candidate_reasons.clone();
        item.general_material_domain.clear();
        item.general_material_category.clear();
        item.global_destination.clear();
        item.recommended_location = if item.ownership_type == inbox_routing::OWNERSHIP_NEW_PROJECT {
            "建议创建新项目后归位".to_string()
        } else {
            "待补充项目证据".to_string()
        };
        item.updated_at = now_string();
    }
}

fn should_refresh_inbox_analysis(analysis_version: u32) -> bool {
    analysis_version < MATERIAL_INBOX_ANALYSIS_VERSION
}

fn process_repeated_inbox_file(
    existing: &MaterialInboxItem,
    projects: &[(ProjectSummary, ProjectManifest)],
    general_history: &[GlobalManagedFile],
    source: &Path,
    source_hash: &str,
    size_bytes: u64,
    received_from: &str,
) -> Result<MaterialInboxItem, String> {
    let mut received = existing.clone();
    let now = now_string();
    received.source_path = path_to_string(source);
    received.received_at = now.clone();
    received.updated_at = now.clone();
    received.received_count = received.received_count.saturating_add(1).max(2);
    received.source_history.push(InboxSourceEvent {
        source_path: path_to_string(source),
        received_from: received_from.to_string(),
        received_at: now,
        hash: source_hash.to_string(),
    });

    if !should_refresh_inbox_analysis(received.analysis_version) {
        received.result_note =
            "已再次收到相同内容；分析版本一致，复用缓存且未创建重复记录。".to_string();
        return Ok(received);
    }

    let old_version = received.analysis_version;
    match rebuild_material_inbox_analysis(
        &received,
        projects,
        general_history,
        source,
        source_hash,
        size_bytes,
    ) {
        Ok(mut refreshed) => {
            refreshed.result_note = format!(
                "已再次收到相同内容；分析从 v{} 更新到 v{}，未重复复制文件。",
                old_version, MATERIAL_INBOX_ANALYSIS_VERSION
            );
            Ok(refreshed)
        }
        Err(error) => {
            received.result_note =
                format!("已记录本次重复接收，但新版分析失败：{error}。原分析缓存仍保留。");
            Ok(received)
        }
    }
}

fn rebuild_material_inbox_analysis(
    previous: &MaterialInboxItem,
    projects: &[(ProjectSummary, ProjectManifest)],
    general_history: &[GlobalManagedFile],
    source: &Path,
    source_hash: &str,
    size_bytes: u64,
) -> Result<MaterialInboxItem, String> {
    let file_name = source
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(previous.file_name.as_str());
    let file_type = source
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or(previous.file_type.as_str())
        .to_lowercase();
    let (analysis, _) =
        analyze_managed_file(&previous.id, file_name, source, &file_type, source_hash);
    let mut rebuilt = build_material_inbox_item_with_history(
        projects,
        general_history,
        &path_to_string(source),
        source_hash,
        size_bytes,
        &analysis,
        &previous.received_from,
    )?;

    rebuilt.id = previous.id.clone();
    rebuilt.created_at = previous.created_at.clone();
    rebuilt.source_path = previous.source_path.clone();
    rebuilt.received_at = previous.received_at.clone();
    rebuilt.received_count = previous.received_count.max(1);
    rebuilt.source_history = previous.source_history.clone();
    rebuilt.decision_traces = previous.decision_traces.clone();
    rebuilt.analysis_version = MATERIAL_INBOX_ANALYSIS_VERSION;

    let fresh_history = rebuilt.status_history.clone();
    rebuilt.status_history = previous.status_history.clone();
    rebuilt.status_history.extend(fresh_history);

    if matches!(
        previous.processing_status.as_str(),
        inbox_routing::ROUTED | inbox_routing::PROJECT_CREATED | inbox_routing::IGNORED
    ) {
        rebuilt.status_history = previous.status_history.clone();
        rebuilt.processing_status = previous.processing_status.clone();
        rebuilt.last_transition_at = previous.last_transition_at.clone();
        rebuilt.route_operation = previous.route_operation.clone();
        rebuilt.managed_file_id = previous.managed_file_id.clone();
        rebuilt.auto_routed = previous.auto_routed;
        rebuilt.can_undo = previous.can_undo;
        rebuilt.global_destination = previous.global_destination.clone();
        rebuilt.target_project_id = previous.target_project_id.clone();
        rebuilt.target_project_root = previous.target_project_root.clone();
        rebuilt.target_project_name = previous.target_project_name.clone();
        rebuilt.suggested_managed_path = previous.suggested_managed_path.clone();
        rebuilt.recommended_location = previous.recommended_location.clone();
        rebuilt.duplicate_of_file_id = previous.duplicate_of_file_id.clone();
        rebuilt.previous_version_file_id = previous.previous_version_file_id.clone();
        rebuilt.document_family_id = previous.document_family_id.clone();
        rebuilt.version_id = previous.version_id.clone();
        rebuilt.version_number = previous.version_number;
    }
    rebuilt.updated_at = now_string();
    sync_inbox_decision_traces(&mut rebuilt);
    Ok(rebuilt)
}

pub fn load_inbox_routing_settings<R: Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<InboxRoutingSettings, String> {
    let path = inbox_routing_settings_path(app)?;
    if !path.exists() {
        return Ok(InboxRoutingSettings::default());
    }
    read_json(&path)
}

pub fn save_inbox_routing_settings<R: Runtime>(
    app: &tauri::AppHandle<R>,
    auto_route_high_confidence: bool,
) -> Result<InboxRoutingSettings, String> {
    let settings = InboxRoutingSettings {
        auto_route_high_confidence,
        updated_at: now_string(),
    };
    write_json_atomic(&inbox_routing_settings_path(app)?, &settings)?;
    Ok(settings)
}

pub async fn refine_inbox_entry_with_ai<R: Runtime>(
    app: tauri::AppHandle<R>,
    item_id: String,
) -> Result<MaterialInboxItem, String> {
    let item = list_material_inbox(&app)?
        .into_iter()
        .find(|item| item.id == item_id)
        .ok_or_else(|| "收件箱记录不存在。".to_string())?;
    if matches!(
        item.duplicate_kind.as_str(),
        "exactDuplicate" | "newVersionCandidate"
    ) {
        return mutate_inbox_item(&app, &item_id, |entry| {
            entry.ai_match_status = "notRequired".to_string();
            Ok(())
        });
    }
    if item.project_candidates.is_empty() {
        return mutate_inbox_item(&app, &item_id, |entry| {
            entry.ai_match_status = "unavailable".to_string();
            entry
                .confidence_reasons
                .push("没有项目候选可供 AI 复核。".to_string());
            Ok(())
        });
    }
    let deepseek_settings = deepseek::load_settings(&app)?;
    if !deepseek_settings.has_api_key || deepseek_settings.selected_model_id.is_empty() {
        return mutate_inbox_item(&app, &item_id, |entry| {
            entry.ai_match_status = "unavailable".to_string();
            entry
                .confidence_reasons
                .push("DeepSeek 未配置，保留规则候选并等待人工确认。".to_string());
            if entry.processing_status == inbox_routing::READY_TO_ROUTE {
                inbox_routing::transition(
                    entry,
                    inbox_routing::PENDING_REVIEW,
                    "AI 项目复核不可用，保守降级人工确认。",
                    "system",
                    None,
                )?;
            }
            Ok(())
        });
    }

    let mut candidate_payloads = Vec::new();
    for candidate in &item.project_candidates {
        let manifest = load_project(&candidate.candidate_project_root)?;
        if manifest.deepseek_authorization.granted_at.is_empty() {
            return mutate_inbox_item(&app, &item_id, |entry| {
                entry.ai_match_status = "authorizationRequired".to_string();
                entry.confidence_reasons.push(format!(
                    "项目“{}”尚未授权发送摘要到 DeepSeek。",
                    candidate.candidate_project_name
                ));
                if entry.processing_status == inbox_routing::READY_TO_ROUTE {
                    inbox_routing::transition(
                        entry,
                        inbox_routing::PENDING_REVIEW,
                        "候选项目尚未授权 AI 复核。",
                        "system",
                        None,
                    )?;
                }
                Ok(())
            });
        }
        candidate_payloads.push(json!({
            "projectId": manifest.project.id,
            "projectName": manifest.project.name,
            "description": manifest.project.description,
            "projectDefinition": manifest.project_analysis.project_definition,
            "knownRequirements": manifest.project_analysis.known_requirements,
            "constraints": manifest.project_analysis.constraints,
            "nextSteps": manifest.project_analysis.next_steps,
            "recentFiles": manifest.files.iter().rev().take(8).map(|file| json!({
                "name": file.file_name,
                "summary": file.content_summary,
                "category": file.category,
            })).collect::<Vec<_>>(),
            "recentTasks": manifest.tasks.iter().rev().take(6).map(|task| task.title.clone()).collect::<Vec<_>>(),
            "recentDecisions": manifest.decisions.iter().rev().take(6).map(|decision| decision.summary.clone()).collect::<Vec<_>>(),
            "ruleScore": candidate.rule_score,
            "ruleReasons": candidate.reasons,
        }));
    }
    let messages = json!([
        {
            "role": "system",
            "content": "你是感冒院 Inbox 项目匹配器。只能依据给出的文件摘要和项目摘要判断，绝不能声称读取原始文件。只返回 JSON：candidateProjectId、score(0-100)、confidence(high/medium/low)、reasons、evidence、matchedSignals。证据不足时 candidateProjectId 为空且 confidence=low。"
        },
        {
            "role": "user",
            "content": serde_json::to_string(&json!({
                "file": {
                    "name": item.file_name,
                    "sourceDirectoryHint": Path::new(&item.source_path)
                        .parent()
                        .and_then(|path| path.file_name())
                        .and_then(|value| value.to_str())
                        .unwrap_or_default(),
                    "type": item.file_type,
                    "summary": item.content_summary,
                    "sections": item.main_fields_or_sections,
                    "classification": item.recommended_category,
                },
                "candidates": candidate_payloads,
            })).map_err(|err| format!("构建 Inbox AI 匹配上下文失败：{err}"))?
        }
    ]);
    let model_id = deepseek_settings.selected_model_id;
    let response = match deepseek::complete_chat(model_id.clone(), messages).await {
        Ok(response) => response,
        Err(err) => {
            mutate_inbox_item(&app, &item_id, |entry| {
                entry.ai_match_status = "failed".to_string();
                entry.ai_match_model = model_id.clone();
                entry.failed_stage = "aiMatching".to_string();
                if entry.processing_status == inbox_routing::PENDING_REVIEW
                    || entry.processing_status == inbox_routing::READY_TO_ROUTE
                {
                    inbox_routing::transition(
                        entry,
                        inbox_routing::FAILED,
                        "DeepSeek 项目匹配失败，可重试或人工选择。",
                        "ai",
                        Some(("aiMatchFailed", &err)),
                    )?;
                }
                Ok(())
            })?;
            return Err(err);
        }
    };
    let decision = match parse_ai_project_match(&response) {
        Ok(decision) => decision,
        Err(err) => {
            mutate_inbox_item(&app, &item_id, |entry| {
                entry.ai_match_status = "failed".to_string();
                entry.ai_match_model = model_id.clone();
                entry.failed_stage = "aiMatching".to_string();
                if matches!(
                    entry.processing_status.as_str(),
                    inbox_routing::PENDING_REVIEW | inbox_routing::READY_TO_ROUTE
                ) {
                    inbox_routing::transition(
                        entry,
                        inbox_routing::FAILED,
                        "DeepSeek 项目匹配结果无法解析，可重试或人工选择。",
                        "ai",
                        Some(("aiMatchInvalidResponse", &err)),
                    )?;
                }
                Ok(())
            })?;
            return Err(err);
        }
    };
    let candidate = item
        .project_candidates
        .iter()
        .find(|candidate| candidate.candidate_project_id == decision.candidate_project_id)
        .cloned();
    let candidate = match candidate {
        Some(candidate) => candidate,
        None => {
            return mutate_inbox_item(&app, &item_id, |entry| {
                entry.ai_match_status = "needsReview".to_string();
                entry.ai_match_model = model_id.clone();
                entry.confidence_level = "low".to_string();
                entry
                    .confidence_reasons
                    .push("DeepSeek 未选择规则筛选出的候选项目。".to_string());
                if entry.processing_status == inbox_routing::READY_TO_ROUTE {
                    inbox_routing::transition(
                        entry,
                        inbox_routing::PENDING_REVIEW,
                        "AI 证据不足，等待人工选择项目。",
                        "ai",
                        None,
                    )?;
                }
                Ok(())
            });
        }
    };
    let manifest = match load_project(&candidate.candidate_project_root) {
        Ok(manifest) => manifest,
        Err(err) => {
            mutate_inbox_item(&app, &item_id, |entry| {
                entry.failed_stage = "projectMatching".to_string();
                inbox_routing::transition(
                    entry,
                    inbox_routing::FAILED,
                    "AI 选中的项目已不可用。",
                    "system",
                    Some(("projectUnavailable", &err)),
                )
            })?;
            return Err(err);
        }
    };
    let category = controlled_category(&item.suggested_category, &item.file_type);
    let plan = match decide_inbox_file_location(
        Path::new(&manifest.project.root_dir),
        &manifest.files,
        &item.suggested_file_name,
        &item.file_type,
        "用户导入",
        "资料收件箱",
        &category,
    ) {
        Ok(plan) => plan,
        Err(err) => {
            mutate_inbox_item(&app, &item_id, |entry| {
                entry.failed_stage = "locationDecision".to_string();
                inbox_routing::transition(
                    entry,
                    inbox_routing::FAILED,
                    "Location Service 无法生成安全位置。",
                    "system",
                    Some(("locationFailed", &err)),
                )
            })?;
            return Err(err);
        }
    };
    let second_score = item
        .project_candidates
        .iter()
        .filter(|value| value.candidate_project_id != candidate.candidate_project_id)
        .map(|value| value.score)
        .max()
        .unwrap_or_default();
    let combined_project_score =
        ((candidate.rule_score as u16 * 60 + decision.score as u16 * 40) / 100) as u8;
    let conflicting = combined_project_score.saturating_sub(second_score) < 12;
    let parse_score = if item.parse_status == "success" {
        100_u16
    } else {
        55_u16
    };
    let confidence_score =
        ((combined_project_score as u16 * 65 + parse_score * 20 + 85 * 15) / 100) as u8;
    let confidence_level =
        inbox_routing::confidence_level(confidence_score, conflicting).to_string();
    let updated = mutate_inbox_item(&app, &item_id, |entry| {
        for value in &mut entry.project_candidates {
            if value.candidate_project_id == candidate.candidate_project_id {
                value.ai_score = Some(decision.score);
                value.score = combined_project_score;
                value.confidence = confidence_level.clone();
                value.reasons.extend(decision.reasons.clone());
                value.evidence.extend(decision.evidence.clone());
                value
                    .matched_signals
                    .extend(decision.matched_signals.clone());
            }
        }
        entry
            .project_candidates
            .sort_by(|left, right| right.score.cmp(&left.score));
        entry.target_project_id = manifest.project.id.clone();
        entry.target_project_root = manifest.project.root_dir.clone();
        entry.target_project_name = manifest.project.name.clone();
        entry.recommended_project_name = manifest.project.name.clone();
        entry.suggested_category = category.clone();
        entry.recommended_relative_location =
            inbox_routing::semantic_location(&category).to_string();
        entry.suggested_managed_path = path_to_string(&plan.destination);
        entry.recommended_location = entry.suggested_managed_path.clone();
        entry.location_reason = plan.reason.clone();
        entry.confidence_score = confidence_score;
        entry.confidence_level = confidence_level.clone();
        entry.confidence_reasons.extend(decision.reasons.clone());
        entry.ai_match_status = "success".to_string();
        entry.ai_match_model = model_id.clone();
        entry.error_code.clear();
        entry.error_message.clear();
        entry.failed_stage.clear();
        match (entry.processing_status.as_str(), confidence_level.as_str()) {
            (inbox_routing::PENDING_REVIEW, "high") | (inbox_routing::FAILED, "high") => {
                inbox_routing::transition(
                    entry,
                    inbox_routing::READY_TO_ROUTE,
                    "AI 已复核项目和位置建议。",
                    "ai",
                    None,
                )?;
            }
            (inbox_routing::READY_TO_ROUTE, "medium" | "low") => {
                inbox_routing::transition(
                    entry,
                    inbox_routing::PENDING_REVIEW,
                    "AI 复核后置信度不足，转人工确认。",
                    "ai",
                    None,
                )?;
            }
            (inbox_routing::FAILED, _) => {
                inbox_routing::transition(
                    entry,
                    inbox_routing::PENDING_REVIEW,
                    "AI 重试完成但仍需人工确认。",
                    "ai",
                    None,
                )?;
            }
            _ => {}
        }
        Ok(())
    })?;
    let settings = load_inbox_routing_settings(&app)?;
    if settings.auto_route_high_confidence
        && updated.processing_status == inbox_routing::READY_TO_ROUTE
        && updated.confidence_level == "high"
        && updated.ai_match_status == "success"
        && updated.parse_status == "success"
        && updated.ownership_type == inbox_routing::OWNERSHIP_EXISTING_PROJECT
        && updated.suggested_category != "待检查"
        && updated.duplicate_kind.is_empty()
    {
        return Ok(
            route_material_inbox_item(&app, &item_id, &updated.target_project_root, true)?.item,
        );
    }
    Ok(updated)
}

pub fn receive_inbox_files<R: Runtime>(
    app: &tauri::AppHandle<R>,
    file_paths: Vec<String>,
) -> Result<Vec<MaterialInboxItem>, String> {
    receive_inbox_files_with_source(app, file_paths, "appImport")
}

pub fn receive_inbox_files_with_source<R: Runtime>(
    app: &tauri::AppHandle<R>,
    file_paths: Vec<String>,
    received_from: &str,
) -> Result<Vec<MaterialInboxItem>, String> {
    ingest_material_inbox_files_with_source(app, file_paths, received_from)
}

pub fn confirm_material_inbox_item<R: Runtime>(
    app: &tauri::AppHandle<R>,
    item_id: String,
    project_root: String,
) -> Result<MaterialInboxRouteResult, String> {
    route_material_inbox_item(app, &item_id, &project_root, false)
}

pub fn confirm_inbox_entry<R: Runtime>(
    app: &tauri::AppHandle<R>,
    item_id: String,
    project_root: String,
) -> Result<MaterialInboxRouteResult, String> {
    confirm_material_inbox_item(app, item_id, project_root)
}

pub fn update_inbox_route_decision<R: Runtime>(
    app: &tauri::AppHandle<R>,
    item_id: String,
    project_root: String,
    semantic_location: String,
) -> Result<MaterialInboxItem, String> {
    let project_manifest = load_project(&project_root)?;
    let current = list_material_inbox(app)?
        .into_iter()
        .find(|item| item.id == item_id)
        .ok_or_else(|| "收件箱记录不存在。".to_string())?;
    let category = controlled_category(&semantic_location, &current.file_type);
    let managed_name = inbox_routing::normalize_managed_file_name(
        &current.file_name,
        current.main_fields_or_sections.first().map(String::as_str),
    );
    let plan = decide_inbox_file_location(
        Path::new(&project_manifest.project.root_dir),
        &project_manifest.files,
        &managed_name,
        &current.file_type,
        "用户导入",
        "资料收件箱",
        &category,
    )?;
    mutate_inbox_item(app, &item_id, |item| {
        item.target_project_id = project_manifest.project.id.clone();
        item.target_project_root = project_manifest.project.root_dir.clone();
        item.target_project_name = project_manifest.project.name.clone();
        item.recommended_project_name = project_manifest.project.name.clone();
        item.suggested_category = category.clone();
        item.recommended_relative_location =
            inbox_routing::semantic_location(&category).to_string();
        item.suggested_managed_path = path_to_string(&plan.destination);
        item.recommended_location = item.suggested_managed_path.clone();
        item.location_reason = plan.reason.clone();
        item.suggested_file_name = managed_name.clone();
        item.confidence_level = "medium".to_string();
        item.confidence_score = item.confidence_score.max(60);
        item.confidence_reasons
            .push("用户已明确选择项目或语义位置。".to_string());
        sync_inbox_decision_traces(item);
        update_trace_outcome(
            &mut item.decision_traces,
            &["projectMatch", "locationRecommendation"],
            "modified",
            "pending",
            "用户修改了推荐项目或位置。",
        );
        if item.processing_status == inbox_routing::FAILED {
            inbox_routing::transition(
                item,
                inbox_routing::READY_TO_ROUTE,
                "用户修正失败项的项目和位置。",
                "user",
                None,
            )
        } else if item.processing_status == inbox_routing::PENDING_REVIEW {
            inbox_routing::transition(
                item,
                inbox_routing::READY_TO_ROUTE,
                "用户确认项目和位置建议。",
                "user",
                None,
            )
        } else if item.processing_status == inbox_routing::READY_TO_ROUTE {
            Ok(())
        } else {
            Err(format!(
                "当前状态不能修改归位建议：{}",
                item.processing_status
            ))
        }
    })
}

pub fn ignore_inbox_entry<R: Runtime>(
    app: &tauri::AppHandle<R>,
    item_id: String,
) -> Result<MaterialInboxItem, String> {
    mutate_inbox_item(app, &item_id, |item| {
        sync_inbox_decision_traces(item);
        inbox_routing::transition(
            item,
            inbox_routing::IGNORED,
            "用户选择暂不处理。",
            "user",
            None,
        )?;
        item.result_note = "已忽略；原文件未移动、未删除。".to_string();
        update_trace_outcome(
            &mut item.decision_traces,
            &[
                "fileClassification",
                "ownershipDecision",
                "projectMatch",
                "locationRecommendation",
                "autoRoute",
            ],
            "rejected",
            "cancelled",
            "用户选择暂不处理。",
        );
        Ok(())
    })
}

pub fn retry_inbox_entry<R: Runtime>(
    app: &tauri::AppHandle<R>,
    item_id: String,
) -> Result<MaterialInboxItem, String> {
    let previous = list_material_inbox(app)?
        .into_iter()
        .find(|item| item.id == item_id)
        .ok_or_else(|| "收件箱记录不存在。".to_string())?;
    if previous.processing_status != inbox_routing::FAILED {
        return Err("只有失败记录可以重试。".to_string());
    }
    if previous.failed_stage == "routing" && !previous.target_project_root.is_empty() {
        mutate_inbox_item(app, &item_id, |item| {
            inbox_routing::transition(
                item,
                inbox_routing::READY_TO_ROUTE,
                "从归位失败点安全重试。",
                "user",
                None,
            )
        })?;
        return Ok(
            route_material_inbox_item(app, &item_id, &previous.target_project_root, false)?.item,
        );
    }

    mutate_inbox_item(app, &item_id, |item| {
        inbox_routing::transition(
            item,
            inbox_routing::ANALYZING,
            "用户重试文件分析。",
            "user",
            None,
        )
    })?;
    let retrying = list_material_inbox(app)?
        .into_iter()
        .find(|item| item.id == item_id)
        .ok_or_else(|| "重试中的收件箱记录不存在。".to_string())?;
    let source = match canonical_existing_file(Path::new(&previous.source_path)) {
        Ok(source) => source,
        Err(err) => {
            return mutate_inbox_item(app, &item_id, |item| {
                item.failed_stage = "receiving".to_string();
                inbox_routing::transition(
                    item,
                    inbox_routing::FAILED,
                    "重试时原文件仍不可用。",
                    "system",
                    Some(("sourceMissing", &err)),
                )
            });
        }
    };
    let source_hash = match hash_file(&source) {
        Ok(hash) => hash,
        Err(err) => {
            return mutate_inbox_item(app, &item_id, |item| {
                item.failed_stage = "hashing".to_string();
                inbox_routing::transition(
                    item,
                    inbox_routing::FAILED,
                    "重试时哈希计算失败。",
                    "system",
                    Some(("hashFailed", &err)),
                )
            });
        }
    };
    let file_name = source
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("file");
    let file_type = source
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("unknown")
        .to_lowercase();
    let (analysis, _) =
        analyze_managed_file(&item_id, file_name, &source, &file_type, &source_hash);
    let registry = read_registry(app)?;
    let projects = registry
        .projects
        .into_iter()
        .filter_map(|project| {
            read_and_repair_manifest(Path::new(&project.root_dir))
                .ok()
                .map(|manifest| (project, manifest))
        })
        .collect::<Vec<_>>();
    let general_history = read_global_files_document(app)?.files;
    let mut rebuilt = build_material_inbox_item_with_history(
        &projects,
        &general_history,
        &previous.source_path,
        &source_hash,
        fs::metadata(&source)
            .map(|value| value.len())
            .unwrap_or_default(),
        &analysis,
        &previous.received_from,
    )?;
    rebuilt.id = previous.id.clone();
    rebuilt.created_at = previous.created_at.clone();
    rebuilt.received_count = previous.received_count;
    rebuilt.source_history = previous.source_history.clone();
    let final_status = rebuilt.processing_status.clone();
    let final_error_code = rebuilt.error_code.clone();
    let final_error_message = rebuilt.error_message.clone();
    rebuilt.status_history = retrying.status_history;
    rebuilt.processing_status = inbox_routing::ANALYZING.to_string();
    rebuilt.last_transition_at = retrying.last_transition_at;
    let final_error = if final_error_message.is_empty() {
        None
    } else {
        Some((final_error_code.as_str(), final_error_message.as_str()))
    };
    inbox_routing::transition(
        &mut rebuilt,
        &final_status,
        "重试处理完成。",
        "system",
        final_error,
    )?;
    replace_inbox_item(app, rebuilt)
}

pub fn reanalyze_inbox_entry<R: Runtime>(
    app: &tauri::AppHandle<R>,
    item_id: String,
) -> Result<MaterialInboxItem, String> {
    let _guard = project_write_lock()
        .lock()
        .map_err(|_| "全局存储锁已损坏，请重启感冒院。".to_string())?;
    let mut document = read_material_inbox_document(app)?;
    let item_index = document
        .items
        .iter()
        .position(|item| item.id == item_id)
        .ok_or_else(|| "收件箱记录不存在。".to_string())?;
    let previous = document.items[item_index].clone();
    let source = resolve_inbox_reanalysis_source(&previous)?;
    let source_hash = hash_file(&source)?;
    if !previous.source_hash.is_empty() && previous.source_hash != source_hash {
        return Err("源文件内容已经变化，请将它作为新文件重新导入。".to_string());
    }
    let registry = read_registry(app)?;
    let projects = registry
        .projects
        .into_iter()
        .filter_map(|project| {
            let root = PathBuf::from(&project.root_dir);
            read_and_repair_manifest(&root)
                .ok()
                .map(|manifest| (project, manifest))
        })
        .collect::<Vec<_>>();
    let general_history = read_global_files_document(app)?.files;
    let mut refreshed = rebuild_material_inbox_analysis(
        &previous,
        &projects,
        &general_history,
        &source,
        &source_hash,
        fs::metadata(&source)
            .map(|value| value.len())
            .unwrap_or_default(),
    )?;
    refreshed.result_note = format!(
        "已按当前分析版本 v{} 重新分析；未复制、移动或修改原文件。",
        MATERIAL_INBOX_ANALYSIS_VERSION
    );
    document.items[item_index] = refreshed.clone();
    document.updated_at = now_string();
    write_material_inbox_document(app, &document)?;
    Ok(refreshed)
}

fn resolve_inbox_reanalysis_source(item: &MaterialInboxItem) -> Result<PathBuf, String> {
    let mut candidates = Vec::new();
    if !item.source_path.trim().is_empty() {
        candidates.push(item.source_path.as_str());
    }
    candidates.extend(
        item.source_history
            .iter()
            .rev()
            .map(|event| event.source_path.as_str()),
    );
    if let Some(operation) = &item.route_operation {
        if !operation.target_path.trim().is_empty() {
            candidates.push(operation.target_path.as_str());
        }
    }
    for candidate in candidates {
        if let Ok(path) = canonical_existing_file(Path::new(candidate)) {
            return Ok(path);
        }
    }
    Err("找不到可用于重新分析的原文件或受管副本。".to_string())
}

pub fn undo_inbox_route<R: Runtime>(
    app: &tauri::AppHandle<R>,
    item_id: String,
) -> Result<MaterialInboxItem, String> {
    let item = list_material_inbox(app)?
        .into_iter()
        .find(|item| item.id == item_id)
        .ok_or_else(|| "收件箱记录不存在。".to_string())?;
    if !item.global_destination.is_empty() && !item.managed_file_id.is_empty() {
        undo_global_file(app, item.managed_file_id.clone())?;
        return list_material_inbox(app)?
            .into_iter()
            .find(|entry| entry.id == item.id)
            .ok_or_else(|| "撤销后未找到收件箱记录。".to_string());
    }
    let operation = item
        .route_operation
        .clone()
        .ok_or_else(|| "该记录没有可撤销的归位操作。".to_string())?;
    if operation.status == "undone" || !item.can_undo {
        return Err("该归位操作已经撤销。".to_string());
    }
    let root = PathBuf::from(&operation.project_root);
    let _guard = project_write_lock()
        .lock()
        .map_err(|_| "项目存储锁已损坏，请重启感冒院。".to_string())?;
    let mut manifest = read_and_repair_manifest(&root)?;
    let original = manifest.clone();
    let record = manifest
        .files
        .iter()
        .find(|file| file.id == operation.managed_file_id)
        .cloned()
        .ok_or_else(|| "受管文件登记已不存在，撤销需要人工检查。".to_string())?;
    let undo_staging = root
        .join(".ganmaoyuan")
        .join("staging")
        .join(format!("undo-{}", operation.id));
    let mut staged_paths: Vec<(PathBuf, PathBuf)> = Vec::new();
    if operation.created_managed_copy {
        let managed = PathBuf::from(&operation.target_path);
        if managed.exists() {
            let current_hash = hash_file(&managed)?;
            if current_hash != operation.target_hash_at_route {
                drop(_guard);
                return mutate_inbox_item(app, &item_id, |entry| {
                    entry.result_note =
                        "受管副本在归位后已被修改，未自动删除，已进入待检查。".to_string();
                    inbox_routing::transition(
                        entry,
                        inbox_routing::PENDING_REVIEW,
                        "撤销检测到受管副本已修改。",
                        "system",
                        Some(("undoConflict", "目标文件已被后续修改")),
                    )
                });
            }
            fs::create_dir_all(&undo_staging).map_err(|err| {
                format!(
                    "创建撤销暂存目录失败：{}：{err}",
                    path_to_string(&undo_staging)
                )
            })?;
            let staged = undo_staging.join("managed-copy");
            fs::rename(&managed, &staged).map_err(|err| {
                format!("暂存待撤销受管副本失败：{}：{err}", operation.target_path)
            })?;
            staged_paths.push((staged, managed));
        }
        if !record.extracted_text_path.is_empty() {
            let extracted = PathBuf::from(&record.extracted_text_path);
            if extracted.exists() {
                fs::create_dir_all(&undo_staging).map_err(|err| {
                    format!(
                        "创建撤销暂存目录失败：{}：{err}",
                        path_to_string(&undo_staging)
                    )
                })?;
                let staged = undo_staging.join("extracted-text");
                if let Err(err) = fs::rename(&extracted, &staged) {
                    for (temporary, original_path) in staged_paths.iter().rev() {
                        let _ = fs::rename(temporary, original_path);
                    }
                    return Err(format!(
                        "暂存待撤销解析文本失败：{}：{err}",
                        record.extracted_text_path
                    ));
                }
                staged_paths.push((staged, extracted));
            }
        }
    }
    manifest
        .files
        .retain(|file| file.id != operation.managed_file_id);
    let reverted_message_ids = manifest
        .messages
        .iter()
        .filter(|message| {
            message
                .attachments
                .iter()
                .any(|attachment| attachment.file_id == operation.managed_file_id)
        })
        .map(|message| message.id.clone())
        .collect::<HashSet<_>>();
    let reverted_task_ids = manifest
        .tasks
        .iter()
        .filter(|task| reverted_message_ids.contains(&task.source_message_id))
        .map(|task| task.id.clone())
        .collect::<HashSet<_>>();
    let reverted_decision_ids = manifest
        .decisions
        .iter()
        .filter(|decision| reverted_message_ids.contains(&decision.source_message_id))
        .map(|decision| decision.id.clone())
        .collect::<HashSet<_>>();
    let reverted_artifact_ids = manifest
        .artifacts
        .iter()
        .filter(|artifact| {
            artifact.file_id == operation.managed_file_id
                || reverted_message_ids.contains(&artifact.source_message_id)
        })
        .map(|artifact| artifact.id.clone())
        .collect::<HashSet<_>>();
    manifest
        .messages
        .retain(|message| !reverted_message_ids.contains(&message.id));
    manifest
        .tasks
        .retain(|task| !reverted_task_ids.contains(&task.id));
    manifest
        .decisions
        .retain(|decision| !reverted_decision_ids.contains(&decision.id));
    manifest
        .artifacts
        .retain(|artifact| !reverted_artifact_ids.contains(&artifact.id));
    for session in &mut manifest.daily_sessions {
        session
            .message_ids
            .retain(|id| !reverted_message_ids.contains(id));
        session
            .task_ids
            .retain(|id| !reverted_task_ids.contains(id));
        session
            .decision_ids
            .retain(|id| !reverted_decision_ids.contains(id));
        session
            .artifact_ids
            .retain(|id| !reverted_artifact_ids.contains(id));
        session
            .imported_file_ids
            .retain(|id| id != &operation.managed_file_id);
    }
    for file in &mut manifest.files {
        file.history_versions
            .retain(|id| id != &operation.managed_file_id);
        if !file
            .original_source_path
            .eq_ignore_ascii_case(&operation.source_path)
        {
            file.source_paths
                .retain(|path| !path.eq_ignore_ascii_case(&operation.source_path));
        }
    }
    manifest
        .location_decisions
        .retain(|decision| decision.operation_id != operation.id);
    manifest.audit.push(audit_event(
        "inbox.route.undo",
        &item_id,
        "success",
        format!("撤销归位操作 {}；用户原文件保持不变。", operation.id),
        false,
        true,
    ));
    if let Err(err) = persist_project(&root, &manifest, Some(&original)) {
        for (temporary, original_path) in staged_paths.iter().rev() {
            let _ = fs::rename(temporary, original_path);
        }
        let _ = fs::remove_dir_all(&undo_staging);
        return Err(err);
    }
    let _ = fs::remove_dir_all(&undo_staging);
    append_audit_event(&root, manifest.audit.last().expect("audit event exists"))?;
    drop(_guard);
    mutate_inbox_item(app, &item_id, |entry| {
        if let Some(route) = &mut entry.route_operation {
            route.status = "undone".to_string();
            route.undone_at = now_string();
        }
        entry.can_undo = false;
        entry.auto_routed = false;
        entry.managed_file_id.clear();
        entry.result_note = "已撤销本次受管归位；原文件未受影响。".to_string();
        sync_inbox_decision_traces(entry);
        for trace in &mut entry.decision_traces {
            if matches!(
                trace.decision_type.as_str(),
                "locationRecommendation" | "autoRoute"
            ) {
                trace.execution = "cancelled".to_string();
                trace.execution_note = "用户撤销归位。".to_string();
                trace.updated_at = now_string();
            }
        }
        inbox_routing::transition(
            entry,
            inbox_routing::READY_TO_ROUTE,
            "用户撤销归位。",
            "user",
            None,
        )
    })
}

pub fn create_project_from_material_inbox<R: Runtime>(
    app: &tauri::AppHandle<R>,
    item_id: String,
    project_name: String,
    root_dir: String,
    description: String,
) -> Result<MaterialInboxRouteResult, String> {
    let item = list_material_inbox(app)?
        .into_iter()
        .find(|entry| entry.id == item_id)
        .ok_or_else(|| "收件箱记录不存在。".to_string())?;
    if item.source_path.trim().is_empty() {
        return Err("收件箱记录缺少原始文件路径。".to_string());
    }
    mutate_inbox_item(app, &item_id, |entry| {
        if entry.processing_status == inbox_routing::PENDING_REVIEW
            || entry.processing_status == inbox_routing::FAILED
        {
            inbox_routing::transition(
                entry,
                inbox_routing::READY_TO_ROUTE,
                "用户选择从 Inbox 创建新项目。",
                "user",
                None,
            )?;
        }
        inbox_routing::transition(
            entry,
            inbox_routing::ROUTING,
            "开始创建项目并归位首份资料。",
            "user",
            None,
        )
    })?;
    let create_result = match create_project(
        app,
        project_name,
        root_dir,
        vec![item.source_path.clone()],
        description,
    ) {
        Ok(result) => result,
        Err(err) => {
            mutate_inbox_item(app, &item_id, |entry| {
                entry.failed_stage = "routing".to_string();
                inbox_routing::transition(
                    entry,
                    inbox_routing::FAILED,
                    "创建项目或归位失败，可重试。",
                    "system",
                    Some(("projectCreateFailed", &err)),
                )
            })?;
            return Err(err);
        }
    };
    let project_root = create_result.project.root_dir.clone();
    let manifest = load_project(&project_root)?;
    let matched_file = manifest
        .files
        .iter()
        .find(|file| {
            normalize_key(&file.original_source_path) == normalize_key(&item.source_path)
                || file.content_hash == item.source_hash
        })
        .cloned();
    let _guard = project_write_lock()
        .lock()
        .map_err(|_| "全局存储锁已损坏，请重启感冒院。".to_string())?;
    let mut document = read_material_inbox_document(app)?;
    let updated = update_material_inbox_item(&mut document.items, &item.id, |entry| {
        entry.target_project_root = manifest.project.root_dir.clone();
        entry.target_project_id = manifest.project.id.clone();
        entry.target_project_name = manifest.project.name.clone();
        entry.judgement_status = "new_project".to_string();
        if let Some(file) = &matched_file {
            entry.managed_file_id = file.id.clone();
            entry.suggested_managed_path = file.managed_path.clone();
            entry.suggested_category = file.category.clone();
            entry.result_note = format!("已新建项目并归位到 {}。", file.managed_relative_path);
            entry.route_operation = Some(InboxRouteOperation {
                id: file.import_transaction_id.clone(),
                project_id: manifest.project.id.clone(),
                project_root: manifest.project.root_dir.clone(),
                source_path: item.source_path.clone(),
                target_path: file.managed_path.clone(),
                managed_file_id: file.id.clone(),
                source_hash: item.source_hash.clone(),
                target_hash_at_route: file.content_hash.clone(),
                created_managed_copy: file.duplicate_of_file_id.is_none(),
                created_at: now_string(),
                status: "committed".to_string(),
                ..InboxRouteOperation::default()
            });
            entry.can_undo = true;
        } else {
            entry.result_note = "已新建项目并完成归位。".to_string();
        }
        entry.failed_stage.clear();
        sync_inbox_decision_traces(entry);
        update_trace_outcome(
            &mut entry.decision_traces,
            &[
                "fileClassification",
                "ownershipDecision",
                "projectMatch",
                "locationRecommendation",
            ],
            "approved",
            "executed",
            "用户确认创建新项目并完成首份资料归位。",
        );
        let _ = inbox_routing::transition(
            entry,
            inbox_routing::PROJECT_CREATED,
            "新项目和首份受管资料已提交。",
            "user",
            None,
        );
    })?;
    document.updated_at = now_string();
    write_material_inbox_document(app, &document)?;
    Ok(MaterialInboxRouteResult {
        item: updated,
        project: Some(manifest.project),
        files: matched_file.into_iter().collect(),
    })
}

pub fn create_project_from_inbox<R: Runtime>(
    app: &tauri::AppHandle<R>,
    item_id: String,
    project_name: String,
    root_dir: String,
    description: String,
) -> Result<MaterialInboxRouteResult, String> {
    create_project_from_material_inbox(app, item_id, project_name, root_dir, description)
}

fn build_codex_prompt(
    manifest: &ProjectManifest,
    task_id: &str,
    task_type: &str,
    expected_result_path: &str,
) -> String {
    let current_task = active_task_title(manifest);
    let completed = manifest
        .artifacts
        .iter()
        .rev()
        .filter(|artifact| !is_codex_generated_context(&artifact.title))
        .take(3)
        .map(|artifact| artifact.title.clone())
        .collect::<Vec<_>>()
        .join("; ");
    let materials = manifest
        .files
        .iter()
        .rev()
        .filter(|file| !is_codex_generated_context(&file.file_name))
        .take(6)
        .map(|file| format!("- {} | {}", file.file_name, file.managed_path))
        .collect::<Vec<_>>()
        .join("\n");
    let decisions = manifest
        .decisions
        .iter()
        .rev()
        .filter(|decision| !is_codex_generated_context(&decision.summary))
        .take(4)
        .map(|decision| format!("- {}", decision.summary))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "taskId：{task_id}\ntaskType：{task_type}\n当前任务：{current_task}\n目标：{goal}\n已完成内容：{completed}\n相关资料与文件位置：\n{materials}\n项目约束和已有决定：\n{decisions}\n验收要求：完成后必须写入固定结果文件。分析任务不需要虚构 commit/tests；代码任务需要报告真实 changedFiles、commits、tests。\nGit 要求：代码任务必须先通过必要测试，再只提交本轮明确归属的改动；若无法安全区分已有脏改动，不能提交，必须标记 needsReview。\n停止条件：完成功能、通过必要验证后停止，不扩展其他功能。\n\nCodex Result Bridge：\n请在任务完成后由你主动写入这个绝对路径的结果文件：{expected_result_path}\n项目内相对位置：.ganmaoyuan/codex/results/{task_id}-result.json\nJSON 字段：taskId、taskType、status、summary、resultText、completedAt、remainingIssues、manualAcceptance、resultSource。\n请将 resultSource 写为 \"codex\"，不要写 runnerFallback；analysis 可包含：findings、recommendations、questions。\ncoding 必须如实包含：changedFiles、commits（完整 SHA）、tests、gitHead、gitStatus、gitModifiedFiles。\nverification 可包含：checks、passed、failed。\nfileOperation 可包含：artifacts、targetFiles。\n如果无法写 JSON，也可以写入同目录下：{task_id}-result.md，并保留同等字段。\n不要只在对话中汇报，必须写入结果文件；不要把 API Key、凭据或无关私密正文写入结果文件。",
        task_id = task_id,
        task_type = task_type,
        goal = empty_label(&manifest.project.next_step, "继续推进当前项目"),
        completed = empty_label(&completed, "暂无明确成果"),
        materials = if materials.is_empty() { "- 暂无资料".to_string() } else { materials },
        decisions = if decisions.is_empty() { "- 暂无关键决定".to_string() } else { decisions },
        expected_result_path = expected_result_path,
    )
}

fn build_explicit_codex_task_prompt(
    task_id: &str,
    task_type: &str,
    title: &str,
    instructions: &str,
    expected_result_path: &str,
) -> String {
    format!(
        "taskId：{task_id}\ntaskType：{task_type}\n任务标题：{title}\n\n本次明确任务：\n{instructions}\n\n执行边界：仅处理本次任务明确涉及的仓库内容和文件。不要读取、修改或引用任何无关目录、历史任务、外部项目或凭据。不要扩展任务范围。\n\nCodex Result Bridge：\n完成后必须由你主动写入这个绝对路径的结果文件：{expected_result_path}\n项目内相对位置：.ganmaoyuan/codex/results/{task_id}-result.json\nJSON 字段：taskId、taskType、status、summary、resultText、completedAt、remainingIssues、manualAcceptance、resultSource。\n请将 resultSource 写为 \"codex\"，不要写 runnerFallback。fileOperation 可包含 artifacts、targetFiles；analysis 可包含 findings、recommendations、questions；coding 必须如实包含 changedFiles、commits（完整 SHA）、tests、gitHead、gitStatus、gitModifiedFiles；verification 可包含 checks、passed、failed。\n代码任务必须先通过必要测试，再只提交本任务可明确归属的文件；若存在无法区分的既有脏改动，不得提交或伪造 Git 核验，必须写 needsReview 和真实原因。结果必须如实记录；不要虚构 commit、测试或文件变更。不要只在对话中汇报，必须写入结果文件。",
        task_id = task_id,
        task_type = task_type,
        title = title,
        instructions = instructions,
        expected_result_path = expected_result_path,
    )
}

fn is_codex_generated_context(value: &str) -> bool {
    value.to_ascii_lowercase().contains("codex-task-")
}

fn active_task_title(manifest: &ProjectManifest) -> String {
    manifest
        .tasks
        .iter()
        .rev()
        .find(|task| task.status == "active")
        .map(|task| task.title.clone())
        .unwrap_or_else(|| first_line(&manifest.project.next_step, 80))
}

fn persist_codex_report_import(
    root: &Path,
    report_text: String,
    report_hash: String,
    source_kind: String,
    source_label: String,
    evidence: FileRecord,
) -> Result<CodexReportImportResult, String> {
    let _guard = project_write_lock()
        .lock()
        .map_err(|_| "项目存储锁已损坏，请重启感冒院。".to_string())?;
    let mut manifest = read_and_repair_manifest(root)?;
    if let Some(existing) = find_codex_report(&manifest, &report_hash) {
        let _ = upsert_codex_task_from_report(root, &existing)?;
        return Ok(CodexReportImportResult {
            report: existing,
            manifest,
            duplicate: true,
        });
    }
    let report = parse_codex_report(
        &report_text,
        &report_hash,
        source_kind,
        source_label,
        &evidence,
    );
    let needs_review = report.parse_status != "ready" || report.completed_status == "uncertain";
    if needs_review {
        upsert_pending_review(
            &mut manifest.pending_reviews,
            pending_item(
                format!("codex-report:{report_hash}"),
                "codex-report",
                "Codex report needs review".to_string(),
                if report.parse_failure_reason.is_empty() {
                    "Report parsing was not confident enough, manual review is required."
                        .to_string()
                } else {
                    report.parse_failure_reason.clone()
                },
                &evidence.id,
                &evidence.managed_path,
                "",
                "",
            ),
        );
    }
    manifest.codex_reports.push(report.clone());
    persist_project(root, &manifest, None)?;
    let _ = persist_codex_external_result_for_report(root, &manifest.project.id, &report)?;
    let _ = upsert_codex_task_from_report(root, &report)?;
    Ok(CodexReportImportResult {
        report,
        manifest,
        duplicate: false,
    })
}

fn parse_codex_report(
    report_text: &str,
    report_hash: &str,
    source_kind: String,
    source_label: String,
    evidence: &FileRecord,
) -> CodexReportRecord {
    let lines = report_text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    let lower_report = report_text.to_ascii_lowercase();
    let explicit_status =
        extract_labeled_token(report_text, &["完成状态", "status"]).to_ascii_lowercase();
    let completed_status = if explicit_status.contains("needsreview") {
        "needsReview"
    } else if explicit_status.contains("failed") || explicit_status.contains("partial") {
        if explicit_status.contains("partial") {
            "partial"
        } else {
            "failed"
        }
    } else if explicit_status.contains("completed") {
        "completed"
    } else if report_text.contains("未完成") || lower_report.contains("blocked") {
        "partial"
    } else if report_text.contains("完成") || report_text.contains("通过") {
        "completed"
    } else {
        "uncertain"
    };
    let modified_content = collect_lines_by_keywords(&lines, &["修改", "实现", "完成", "变更"]);
    let test_results =
        collect_lines_by_keywords(&lines, &["测试", "构建", "build", "cargo test", "npm run"]);
    let unresolved_issues =
        collect_lines_by_keywords(&lines, &["遗留", "问题", "风险", "未解决", "人工验收"]);
    let next_step_suggestions = collect_lines_by_keywords(&lines, &["下一步", "建议", "后续"]);
    let commit = extract_commit(&lines);
    let signal_count = [
        !modified_content.is_empty(),
        !test_results.is_empty(),
        !unresolved_issues.is_empty(),
        !next_step_suggestions.is_empty(),
        !commit.is_empty(),
        completed_status != "uncertain",
    ]
    .into_iter()
    .filter(|value| *value)
    .count();
    let parse_status = if signal_count >= 2 {
        "ready"
    } else {
        "review_required"
    };
    CodexReportRecord {
        id: new_id(),
        source_kind,
        source_label,
        report_hash: report_hash.to_string(),
        evidence_file_id: evidence.id.clone(),
        evidence_managed_path: evidence.managed_path.clone(),
        parse_status: parse_status.to_string(),
        completed_status: completed_status.to_string(),
        summary: first_line(report_text, 160),
        modified_content,
        test_results,
        commit,
        unresolved_issues,
        next_step_suggestions,
        parse_failure_reason: if parse_status == "ready" {
            String::new()
        } else {
            "Could not confidently extract enough structured signals from the report.".to_string()
        },
        duplicate_of_report_id: None,
        applied: false,
        confirmed_at: String::new(),
        created_at: now_string(),
    }
}

fn collect_lines_by_keywords(lines: &[&str], keywords: &[&str]) -> Vec<String> {
    let mut values = Vec::new();
    for line in lines {
        let normalized = line
            .trim_start_matches(['-', '*', '1', '2', '3', '4', '5', '.', ' '])
            .trim();
        if normalized.is_empty() {
            continue;
        }
        let lower = normalized.to_lowercase();
        if keywords
            .iter()
            .any(|keyword| normalized.contains(keyword) || lower.contains(&keyword.to_lowercase()))
        {
            if !values.iter().any(|item| item == normalized) {
                values.push(normalized.to_string());
            }
        }
    }
    values
}

fn extract_commit(lines: &[&str]) -> String {
    for line in lines {
        let lower = line.to_lowercase();
        if !lower.contains("commit") {
            continue;
        }
        for token in line.split(|ch: char| ch.is_whitespace() || matches!(ch, '`' | ':' | ',')) {
            let trimmed = token.trim();
            if (7..=40).contains(&trimmed.len()) && trimmed.chars().all(|ch| ch.is_ascii_hexdigit())
            {
                return trimmed.to_string();
            }
        }
    }
    String::new()
}

fn find_codex_report(manifest: &ProjectManifest, report_hash: &str) -> Option<CodexReportRecord> {
    manifest
        .codex_reports
        .iter()
        .find(|report| report.report_hash == report_hash)
        .cloned()
}

fn read_text_report_file(path: &Path) -> Result<String, String> {
    let bytes = fs::read(path).map_err(|err| {
        format!(
            "failed to read Codex report file: {}: {err}",
            path_to_string(path)
        )
    })?;
    Ok(String::from_utf8_lossy(&bytes).to_string())
}

fn normalize_codex_bridge_result_text(raw_text: &str, extension: &str) -> String {
    if extension != "json" {
        return raw_text.trim().to_string();
    }
    let Ok(value) = serde_json::from_str::<serde_json::Value>(raw_text) else {
        return raw_text.trim().to_string();
    };
    let task_id = json_string_field(&value, &["taskId", "task_id"]);
    let task_type = json_string_field(&value, &["taskType", "task_type"]);
    let result_source = json_string_field(&value, &["resultSource", "result_source"]);
    let result_run_id = json_string_field(&value, &["resultRunId", "runId", "result_run_id"]);
    let status = json_string_field(&value, &["status"]);
    let summary = json_string_field(&value, &["summary"]);
    let result_text = json_string_field(&value, &["resultText", "result_text"]);
    let changed_files = json_coding_string_array_field(&value, &["changedFiles", "changed_files"]);
    let commits = json_coding_string_array_field(&value, &["commits"]);
    let tests = json_coding_display_array_field(&value, &["tests"]);
    let git_head = json_coding_string_field(&value, &["gitHead", "git_head"]);
    let git_status = json_coding_string_field(&value, &["gitStatus", "git_status"]);
    let git_modified_files =
        json_coding_string_array_field(&value, &["gitModifiedFiles", "git_modified_files"]);
    let findings = json_string_array_field(&value, &["findings"]);
    let recommendations = json_string_array_field(&value, &["recommendations"]);
    let questions = json_string_array_field(&value, &["questions"]);
    let checks = json_string_array_field(&value, &["checks"]);
    let passed = json_string_array_field(&value, &["passed"]);
    let failed = json_string_array_field(&value, &["failed"]);
    let artifacts = json_string_array_field(&value, &["artifacts"]);
    let target_files = json_string_array_field(&value, &["targetFiles", "target_files"]);
    let remaining = json_string_array_field(&value, &["remainingIssues", "remaining_issues"]);
    let manual_acceptance =
        json_string_array_field(&value, &["manualAcceptance", "manual_acceptance"]);
    let completed_at = json_string_field(&value, &["completedAt", "completed_at"]);

    let mut lines = Vec::new();
    if !task_id.is_empty() {
        lines.push(format!("taskId：{task_id}"));
    }
    if !task_type.is_empty() {
        lines.push(format!("taskType：{task_type}"));
    }
    if !result_source.is_empty() {
        lines.push(format!("resultSource：{result_source}"));
    }
    if !result_run_id.is_empty() {
        lines.push(format!("resultRunId：{result_run_id}"));
    }
    if !status.is_empty() {
        lines.push(format!("完成状态：{status}"));
    }
    if !summary.is_empty() {
        lines.push(format!("完成摘要：{summary}"));
    }
    if !result_text.is_empty() {
        lines.push(format!("结果正文：{result_text}"));
    }
    for item in changed_files {
        lines.push(format!("修改文件：{item}"));
    }
    for item in commits {
        lines.push(format!("commit: {item}"));
    }
    for item in tests {
        lines.push(format!("测试：{item}"));
    }
    if !git_head.is_empty() {
        lines.push(format!("Git HEAD：{git_head}"));
    }
    if !git_status.is_empty() {
        lines.push(format!("Git 状态：{git_status}"));
    }
    for item in git_modified_files {
        lines.push(format!("Git 修改：{item}"));
    }
    for item in findings {
        lines.push(format!("发现：{item}"));
    }
    for item in recommendations {
        lines.push(format!("建议：{item}"));
    }
    for item in questions {
        lines.push(format!("待确认问题：{item}"));
    }
    for item in checks {
        lines.push(format!("检查项：{item}"));
    }
    for item in passed {
        lines.push(format!("通过：{item}"));
    }
    for item in failed {
        lines.push(format!("未通过：{item}"));
    }
    for item in artifacts {
        lines.push(format!("产物：{item}"));
    }
    for item in target_files {
        lines.push(format!("目标文件：{item}"));
    }
    for item in remaining {
        lines.push(format!("遗留问题：{item}"));
    }
    for item in manual_acceptance {
        lines.push(format!("人工验收：{item}"));
    }
    if !completed_at.is_empty() {
        lines.push(format!("completedAt：{completed_at}"));
    }
    if lines.is_empty() {
        raw_text.trim().to_string()
    } else {
        lines.join("\n")
    }
}

fn json_string_field(value: &serde_json::Value, keys: &[&str]) -> String {
    keys.iter()
        .find_map(|key| value.get(*key))
        .and_then(json_value_to_string)
        .unwrap_or_default()
}

fn json_string_array_field(value: &serde_json::Value, keys: &[&str]) -> Vec<String> {
    keys.iter()
        .find_map(|key| value.get(*key))
        .map(|item| match item {
            serde_json::Value::Array(values) => values
                .iter()
                .filter_map(json_value_to_string)
                .filter(|text| !text.trim().is_empty())
                .collect(),
            _ => json_value_to_string(item).into_iter().collect(),
        })
        .unwrap_or_default()
}

fn json_nested_value<'a>(
    value: &'a serde_json::Value,
    section: &str,
    keys: &[&str],
) -> Option<&'a serde_json::Value> {
    value
        .get(section)
        .and_then(|nested| keys.iter().find_map(|key| nested.get(*key)))
}

fn json_coding_string_field(value: &serde_json::Value, keys: &[&str]) -> String {
    let top_level = json_string_field(value, keys);
    if !top_level.is_empty() {
        return top_level;
    }
    json_nested_value(value, "coding", keys)
        .and_then(json_value_to_string)
        .unwrap_or_default()
}

fn json_coding_string_array_field(value: &serde_json::Value, keys: &[&str]) -> Vec<String> {
    let top_level = json_string_array_field(value, keys);
    if !top_level.is_empty() {
        return top_level;
    }
    json_nested_value(value, "coding", keys)
        .map(json_value_to_string_array)
        .unwrap_or_default()
}

fn json_coding_display_array_field(value: &serde_json::Value, keys: &[&str]) -> Vec<String> {
    let top_level = json_display_array_field(value, keys);
    if !top_level.is_empty() {
        return top_level;
    }
    json_nested_value(value, "coding", keys)
        .map(json_value_to_display_array)
        .unwrap_or_default()
}

fn json_display_array_field(value: &serde_json::Value, keys: &[&str]) -> Vec<String> {
    keys.iter()
        .find_map(|key| value.get(*key))
        .map(json_value_to_display_array)
        .unwrap_or_default()
}

fn json_value_to_string_array(value: &serde_json::Value) -> Vec<String> {
    match value {
        serde_json::Value::Array(values) => values
            .iter()
            .filter_map(json_value_to_string)
            .filter(|text| !text.trim().is_empty())
            .collect(),
        _ => json_value_to_string(value).into_iter().collect(),
    }
}

fn json_value_to_display_array(value: &serde_json::Value) -> Vec<String> {
    match value {
        serde_json::Value::Array(values) => values
            .iter()
            .filter_map(json_value_to_display_string)
            .filter(|text| !text.trim().is_empty())
            .collect(),
        _ => json_value_to_display_string(value).into_iter().collect(),
    }
}

fn json_value_to_display_string(value: &serde_json::Value) -> Option<String> {
    if let Some(text) = json_value_to_string(value) {
        return Some(text);
    }
    let serde_json::Value::Object(object) = value else {
        return None;
    };
    let name = object
        .get("name")
        .and_then(json_value_to_string)
        .unwrap_or_default();
    let status = object
        .get("status")
        .and_then(json_value_to_string)
        .unwrap_or_default();
    let details = object
        .get("details")
        .and_then(json_value_to_string)
        .unwrap_or_default();
    let mut result = first_non_empty(&[name, status]);
    if !details.is_empty() {
        result = if result.is_empty() {
            details
        } else {
            format!("{result}（{details}）")
        };
    }
    (!result.is_empty()).then_some(result)
}

fn json_value_to_string(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(text) => Some(text.trim().to_string()),
        serde_json::Value::Number(number) => Some(number.to_string()),
        serde_json::Value::Bool(flag) => Some(flag.to_string()),
        _ => None,
    }
}

fn bridge_result_evidence_file(
    root: &Path,
    path: &Path,
    raw_text: &str,
) -> Result<FileRecord, String> {
    let _guard = project_write_lock()
        .lock()
        .map_err(|_| "项目存储锁已损坏，请重启感冒院。".to_string())?;
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("codex-result")
        .to_string();
    let file_type = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("md")
        .to_ascii_lowercase();
    let content_hash = format!("{:x}", Sha256::digest(raw_text.as_bytes()));
    let relative = format!(".ganmaoyuan/codex/evidence/{content_hash}.{file_type}");
    let snapshot = root.join(&relative);
    if snapshot.exists() {
        let stored =
            fs::read(&snapshot).map_err(|err| format!("读取 Codex 证据快照失败：{err}"))?;
        if stored != raw_text.as_bytes() {
            return Err("Codex 证据快照已变化；为保护历史，未覆盖该文件。".to_string());
        }
    } else {
        write_text_atomic(&snapshot, raw_text)?;
    }
    let now = now_string();
    Ok(FileRecord {
        id: format!("codex-bridge-evidence-{content_hash}"),
        file_name,
        original_source_path: path_to_string(path),
        managed_path: path_to_string(&snapshot),
        file_type,
        category: "Codex结果".to_string(),
        created_source: "Codex Result Bridge".to_string(),
        related_task: String::new(),
        current_version: "v1".to_string(),
        history_versions: Vec::new(),
        created_at: now.clone(),
        modified_at: now,
        still_exists: true,
        needs_user_confirmation: false,
        parse_status: "ready".to_string(),
        content_summary: "Codex 结果导入时的独立证据快照。".to_string(),
        content_hash,
        size_bytes: raw_text.len() as u64,
        managed_relative_path: relative,
        location_reason: "保留导入时的结果内容，不随下一次执行覆盖。".to_string(),
        ..FileRecord::default()
    })
}

fn hash_text(text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(text.replace("\r\n", "\n").trim().as_bytes());
    format!("{:x}", hasher.finalize())
}

fn upsert_pending_review(items: &mut Vec<PendingReviewItem>, next: PendingReviewItem) {
    if let Some(existing) = items.iter_mut().find(|item| item.key == next.key) {
        // An explicit resolution is authoritative for this exact fact. A
        // materially changed observation must produce a different key.
        if matches!(existing.status.as_str(), "resolved" | "ignored") {
            return;
        }
        *existing = next;
        return;
    }
    items.push(next);
}

pub fn scan_project_workspace(
    project_root: &str,
) -> Result<(ProjectManifest, bool, String), String> {
    let root = PathBuf::from(project_root);
    let _guard = project_write_lock()
        .lock()
        .map_err(|_| "项目写入锁已被占用，请稍后重试。".to_string())?;
    let mut manifest = read_and_repair_manifest(&root)?;
    let original = manifest.clone();
    // Git already provides the authoritative change index for an associated
    // repository. Walking and hashing that same tree on every project open is
    // both redundant and expensive, especially for database/build artifacts.
    let repository_root = configured_repository_path(&manifest.project)
        .or_else(|| {
            let project_root = PathBuf::from(&manifest.project.root_dir);
            project_root.join(".git").exists().then_some(project_root)
        })
        .and_then(|path| canonical_existing_directory(&path).ok());
    let observed = collect_observed_project_files(&root, repository_root.as_deref())?;
    let mut findings = Vec::new();
    let mut seen_keys = HashSet::new();
    let tracked_by_hash = manifest
        .files
        .iter()
        .filter(|file| !file.content_hash.is_empty())
        .fold(HashMap::new(), |mut map, file| {
            map.entry(file.content_hash.clone())
                .or_insert_with(Vec::new)
                .push(file.clone());
            map
        });
    let tracked_source_paths = manifest
        .files
        .iter()
        .flat_map(|file| file.source_paths.iter().cloned())
        .map(|path| normalize_key(&path))
        .collect::<HashSet<_>>();
    let managed_paths = manifest
        .files
        .iter()
        .map(|file| normalize_key(&file.managed_path))
        .collect::<HashSet<_>>();
    // An associated repository is an existing project source, not an Inbox
    // import. Keep observing it for project facts, but do not turn every
    // untracked source/config file into a manual material-review action.
    // Older scans turned an unavailable original import path into both a
    // source-missing review and a generic "needs review" flag. A managed copy
    // is the project's durable record, so that provenance gap must not keep
    // interrupting normal work after the copy is safely present.
    let source_only_confirmation_file_ids = manifest
        .files
        .iter()
        .filter_map(|file| {
            let reviews = manifest
                .pending_reviews
                .iter()
                .filter(|item| {
                    item.file_id == file.id
                        && !matches!(item.status.as_str(), "resolved" | "ignored")
                })
                .collect::<Vec<_>>();
            let has_source_gap = reviews
                .iter()
                .any(|item| matches!(item.kind.as_str(), "source_missing" | "source_moved"));
            let contains_only_source_gap_artifacts = reviews.iter().all(|item| {
                matches!(item.kind.as_str(), "source_missing" | "source_moved")
                    || (item.kind == "review_required" && item.key.starts_with("review-needed|"))
            });
            (has_source_gap && contains_only_source_gap_artifacts).then(|| file.id.clone())
        })
        .collect::<HashSet<_>>();

    for file in &mut manifest.files {
        let managed_exists = PathBuf::from(&file.managed_path).exists();
        if file.still_exists != managed_exists {
            file.still_exists = managed_exists;
        }
        if managed_exists
            && source_only_confirmation_file_ids.contains(&file.id)
            && !matches!(
                file.parse_status.as_str(),
                "failed" | "partial" | "review_required"
            )
        {
            file.needs_user_confirmation = false;
        }
        if !managed_exists {
            file.needs_user_confirmation = true;
            let key = format!("managed-missing|{}", normalize_key(&file.managed_path));
            if seen_keys.insert(key.clone()) {
                findings.push(pending_item(
                    key,
                    "managed_missing",
                    format!("受管文件缺失：{}", file.file_name),
                    format!("记录中的受管路径不可用：{}", file.managed_relative_path),
                    &file.id,
                    &file.managed_path,
                    "",
                    &file.category,
                ));
            }
        }

        for source_path in file.source_paths.clone() {
            if source_path.trim().is_empty() {
                continue;
            }
            let source_key = normalize_key(&source_path);
            if observed
                .iter()
                .any(|item| normalize_key(&item.path) == source_key)
            {
                continue;
            }
            // The source is provenance only once the managed project copy is
            // present. Do not turn a cleaned-up import folder into a blocker.
            if managed_exists {
                continue;
            }
            file.needs_user_confirmation = true;
            let moved_candidate = observed.iter().find(|item| {
                !item.hash.is_empty()
                    && item.hash == file.content_hash
                    && !tracked_source_paths.contains(&normalize_key(&item.path))
            });
            let key = format!("source-missing|{}|{}", file.id, source_key);
            if seen_keys.insert(key.clone()) {
                findings.push(pending_item(
                    key,
                    if moved_candidate.is_some() {
                        "source_moved"
                    } else {
                        "source_missing"
                    },
                    if moved_candidate.is_some() {
                        format!("原始资料疑似被移动: {}", file.file_name)
                    } else {
                        format!("原始资料缺失: {}", file.file_name)
                    },
                    if let Some(candidate) = moved_candidate {
                        format!("原路径不可用，发现相同内容的新位置: {}", candidate.path)
                    } else {
                        format!("记录中的原始资料路径不可用: {}", source_path)
                    },
                    &file.id,
                    &source_path,
                    moved_candidate.map(|item| item.path.as_str()).unwrap_or(""),
                    &file.category,
                ));
            }
        }

        if (file.needs_user_confirmation
            || matches!(
                file.parse_status.as_str(),
                "failed" | "partial" | "review_required"
            ))
            && !file.managed_path.trim().is_empty()
        {
            let key = format!("review-needed|{}", file.id);
            if seen_keys.insert(key.clone()) {
                let detail = if file.parse_failure_reason.trim().is_empty() {
                    format!(
                        "分类：{}；推荐分类：{}；需要人工确认。",
                        file.category,
                        empty_label(&file.recommended_category, "未知")
                    )
                } else {
                    format!("解析失败原因：{}", file.parse_failure_reason)
                };
                findings.push(pending_item(
                    key,
                    "review_required",
                    format!("需要检查：{}", file.file_name),
                    detail,
                    &file.id,
                    &file.managed_path,
                    &file.managed_path,
                    &file.category,
                ));
            }
        }
    }

    for item in &observed {
        let path_key = normalize_key(&item.path);
        if managed_paths.contains(&path_key) || tracked_source_paths.contains(&path_key) {
            continue;
        }
        if repository_root
            .as_ref()
            .is_some_and(|repository| path_is_within(repository, Path::new(&item.path)))
        {
            continue;
        }
        let key = format!("observed|{}", path_key);
        if !seen_keys.insert(key.clone()) {
            continue;
        }

        if let Some(existing) = tracked_by_hash
            .get(&item.hash)
            .and_then(|files| files.first())
        {
            findings.push(pending_item(
                key,
                "duplicate_file",
                format!("重复文件：{}", item.file_name),
                format!(
                    "发现相同内容已登记，不会重复复制；请确认是否保留新来源：{}",
                    existing.managed_relative_path
                ),
                &existing.id,
                &item.path,
                &existing.managed_path,
                &existing.category,
            ));
            continue;
        }

        let same_series = manifest.files.iter().find(|file| {
            logical_version_key(&file.file_name) == logical_version_key(&item.file_name)
        });
        if let Some(previous) = same_series {
            let plan = decide_file_location(
                &root,
                &manifest.files,
                &item.file_name,
                &item.file_type,
                "外部工具生成",
                &previous.related_task,
                "目录差异扫描",
            )?;
            findings.push(pending_item(
                key,
                "modified_new_version",
                format!("发现新版本：{}", item.file_name),
                format!("疑似已有文件的新版本，建议归入：{}", plan.relative_path),
                &previous.id,
                &item.path,
                &path_to_string(&plan.destination),
                &plan.category,
            ));
            continue;
        }

        let suggested = decide_file_location(
            &root,
            &manifest.files,
            &item.file_name,
            &item.file_type,
            "用户补充或外部工具生成",
            "待确认任务",
            "目录差异扫描",
        )?;
        findings.push(pending_item(
            key,
            "new_file",
            format!("发现新文件：{}", item.file_name),
            format!(
                "建议位置：{}。{}",
                suggested.relative_path, suggested.reason
            ),
            "",
            &item.path,
            &path_to_string(&suggested.destination),
            &suggested.category,
        ));
    }

    findings.sort_by(|left, right| {
        left.title
            .cmp(&right.title)
            .then_with(|| left.path.cmp(&right.path))
    });
    let previous_keys = manifest
        .pending_reviews
        .iter()
        .filter(|item| item.status != "resolved")
        .map(|item| item.key.clone())
        .collect::<HashSet<_>>();
    let next_keys = findings
        .iter()
        .map(|item| item.key.clone())
        .collect::<HashSet<_>>();
    let mut changed = previous_keys != next_keys
        || manifest
            .files
            .iter()
            .zip(original.files.iter())
            .any(|(left, right)| {
                left.still_exists != right.still_exists
                    || left.needs_user_confirmation != right.needs_user_confirmation
            });

    manifest.pending_reviews = findings;
    manifest.monitoring.status = "ready".to_string();
    manifest.monitoring.last_scanned_at = now_string();
    if manifest.monitoring.watch_started_at.is_empty() {
        manifest.monitoring.watch_started_at = manifest.monitoring.last_scanned_at.clone();
    }
    manifest.monitoring.pending_count = manifest
        .pending_reviews
        .iter()
        .filter(|item| item.status != "resolved")
        .count();
    if repair_garbled_pending_reviews(&mut manifest) {
        changed = true;
    }
    manifest.monitoring.last_error.clear();
    manifest.monitoring.last_event_at = manifest.monitoring.last_scanned_at.clone();

    let summary = if manifest.monitoring.pending_count == 0 {
        "项目目录扫描完成，暂无待确认事项。".to_string()
    } else {
        format!(
            "项目目录扫描发现 {} 个待确认事项。",
            manifest.monitoring.pending_count
        )
    };

    if changed {
        let message = workspace_message("system", "monitor", summary.clone(), "project_monitor");
        record_message_derivatives(&mut manifest, &message);
        manifest.messages.push(message.clone());
        append_workspace_event(&root, &message)?;
        manifest.audit.push(audit_event(
            "monitor.scan",
            &manifest.project.root_dir,
            "success",
            summary.clone(),
            false,
            true,
        ));
        update_next_step_and_auto_recovery(&root, &mut manifest, "完成项目目录扫描");
        persist_project(&root, &manifest, Some(&original))?;
        append_audit_event(&root, manifest.audit.last().expect("audit event exists"))?;
    }
    Ok((manifest, changed, summary))
}

fn collect_observed_project_files(
    root: &Path,
    repository_root: Option<&Path>,
) -> Result<Vec<ObservedProjectFile>, String> {
    let mut files = Vec::new();
    collect_observed_project_files_recursive(root, root, repository_root, &mut files)?;
    Ok(files)
}

fn collect_observed_project_files_recursive(
    root: &Path,
    current: &Path,
    repository_root: Option<&Path>,
    files: &mut Vec<ObservedProjectFile>,
) -> Result<(), String> {
    if repository_root.is_some_and(|repository| {
        normalize_key(&path_to_string(current)) == normalize_key(&path_to_string(repository))
            || path_is_within(current, repository)
    }) {
        return Ok(());
    }
    let entries = fs::read_dir(current)
        .map_err(|err| format!("读取项目目录失败：{}；{}", path_to_string(current), err))?;
    for entry in entries {
        let entry =
            entry.map_err(|err| format!("读取目录项失败：{}；{}", path_to_string(current), err))?;
        let path = entry.path();
        let relative = path
            .strip_prefix(root)
            .map(path_to_string)
            .unwrap_or_else(|_| path_to_string(&path));
        if relative.eq_ignore_ascii_case(".ganmaoyuan")
            || relative.starts_with(".ganmaoyuan\\")
            || relative.starts_with(".ganmaoyuan/")
        {
            continue;
        }
        let file_type = entry
            .file_type()
            .map_err(|err| format!("读取文件类型失败：{}；{}", path_to_string(&path), err))?;
        if file_type.is_dir() {
            // Project monitoring observes user materials, not dependency caches or build output.
            // Reusing the Workspace Scanner exclusions keeps an associated code repository from
            // turning a periodic monitor pass into a full dependency-tree hash.
            if should_exclude_scan_directory(root, &path) {
                continue;
            }
            collect_observed_project_files_recursive(root, &path, repository_root, files)?;
            continue;
        }
        if !file_type.is_file() {
            continue;
        }
        let canonical = canonical_existing_file(&path)?;
        let hash = monitored_file_hash(&canonical)?;
        let file_name = canonical
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("unknown")
            .to_string();
        let extension = canonical
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("unknown")
            .to_lowercase();
        files.push(ObservedProjectFile {
            path: path_to_string(&canonical),
            file_name,
            file_type: extension,
            hash,
        });
    }
    Ok(())
}

fn monitored_file_hash(path: &Path) -> Result<String, String> {
    let metadata = fs::metadata(path)
        .map_err(|err| format!("读取文件信息失败：{}；{err}", path_to_string(path)))?;
    let cache_key = normalize_key(&path_to_string(path));
    let modified_at = metadata.modified().ok();
    let mut cache = PROJECT_MONITOR_HASH_CACHE
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .map_err(|_| "项目监测哈希缓存不可用，请稍后重试。".to_string())?;
    if let Some(existing) = cache.get(&cache_key) {
        if existing.size_bytes == metadata.len() && existing.modified_at == modified_at {
            return Ok(existing.hash.clone());
        }
    }
    let hash = hash_file(path)?;
    cache.insert(
        cache_key,
        ProjectMonitorHashCacheEntry {
            size_bytes: metadata.len(),
            modified_at,
            hash: hash.clone(),
        },
    );
    Ok(hash)
}

fn canonical_existing_directory(path: &Path) -> Result<PathBuf, String> {
    if !path.exists() {
        return Err(format!("目录不存在：{}", path_to_string(path)));
    }
    if !path.is_dir() {
        return Err(format!("路径不是目录：{}", path_to_string(path)));
    }
    path.canonicalize()
        .map_err(|err| format!("规范化目录路径失败：{}：{err}", path_to_string(path)))
}

fn path_is_within(root: &Path, candidate: &Path) -> bool {
    candidate
        .canonicalize()
        .ok()
        .is_some_and(|path| path.starts_with(root))
}

fn normalize_key(value: &str) -> String {
    value.trim().replace('/', "\\").to_lowercase()
}

fn contains_any(text: &str, keywords: &[&str]) -> bool {
    keywords.iter().any(|kw| text.contains(*kw))
}

fn pending_item(
    key: String,
    kind: &str,
    title: String,
    detail: String,
    file_id: &str,
    path: &str,
    suggested_managed_path: &str,
    suggested_category: &str,
) -> PendingReviewItem {
    PendingReviewItem {
        id: new_id(),
        key,
        kind: kind.to_string(),
        title,
        detail,
        file_id: file_id.to_string(),
        path: path.to_string(),
        suggested_managed_path: suggested_managed_path.to_string(),
        suggested_category: suggested_category.to_string(),
        status: "open".to_string(),
        detected_at: now_string(),
        updated_at: now_string(),
    }
}

fn update_next_step_and_auto_recovery(
    root: &Path,
    manifest: &mut ProjectManifest,
    completed: &str,
) {
    let suggested_next_step = build_monitoring_next_step(manifest);
    if !suggested_next_step.trim().is_empty()
        && (manifest.project.next_step.trim().is_empty()
            || is_system_generated_next_step(manifest, &manifest.project.next_step))
    {
        manifest.project.next_step = suggested_next_step;
    } else if suggested_next_step.trim().is_empty() {
        clear_non_user_next_step(manifest);
    }
    let next_step = manifest.project.next_step.clone();
    manifest.project.last_opened_at = now_string();
    let should_create_recovery = manifest
        .recovery_points
        .last()
        .map(|item| {
            item.completed != completed
                || item.next_step != next_step
                || item.file_count != manifest.files.len()
                || item.message_count != manifest.messages.len()
        })
        .unwrap_or(true);
    if !should_create_recovery {
        return;
    }
    let recovery = RecoveryPoint {
        id: new_id(),
        project_id: manifest.project.id.clone(),
        completed: completed.to_string(),
        next_step,
        created_at: now_string(),
        message_count: manifest.messages.len(),
        file_count: manifest.files.len(),
    };
    manifest.recovery_points.push(recovery.clone());
    record_recovery_in_session(manifest, &recovery.id);
    let _ = write_json_atomic(
        &recovery_dir(root).join(format!("{}.json", recovery.id)),
        &recovery,
    );
}

fn build_monitoring_next_step(manifest: &ProjectManifest) -> String {
    if let Some(item) = manifest.pending_reviews.iter().find(|item| {
        !matches!(item.status.as_str(), "resolved" | "ignored")
            && !looks_garbled_or_meaningless(&item.title)
    }) {
        return format!("请先处理待确认事项：{}", item.title);
    }
    if let Some(task) = manifest
        .tasks
        .iter()
        .rev()
        .find(|task| task.status == "active" && !looks_garbled_or_meaningless(&task.title))
    {
        return format!("继续推进当前任务：{}", task.title);
    }
    String::new()
}

fn clear_non_user_next_step(manifest: &mut ProjectManifest) -> bool {
    let current = manifest.project.next_step.trim();
    if current.is_empty() {
        return false;
    }
    if is_system_generated_next_step(manifest, current) {
        let replacement = build_monitoring_next_step(manifest);
        manifest.project.next_step = replacement;
        return true;
    }
    false
}

fn is_system_generated_next_step(manifest: &ProjectManifest, value: &str) -> bool {
    let current = value.trim();
    current == INITIAL_PROJECT_NEXT_STEP
        || current == GENERATED_NEXT_STEP_PLACEHOLDER
        || current == FACTS_ONLY_NEXT_STEP_PLACEHOLDER
        || current.starts_with("请先处理待确认事项：")
        || current.starts_with("继续推进当前任务：")
        || manifest
            .project_analysis
            .next_steps
            .iter()
            .any(|item| item.trim() == current)
}

fn repair_garbled_project_display_text(manifest: &mut ProjectManifest) -> bool {
    if manifest.project.next_step.trim().is_empty() {
        return false;
    }
    if manifest.project.next_step.trim() == GENERATED_NEXT_STEP_PLACEHOLDER {
        manifest.project.next_step.clear();
        return true;
    }
    if !looks_garbled_or_meaningless(&manifest.project.next_step) {
        return false;
    }
    manifest.project.next_step = build_monitoring_next_step(manifest);
    true
}

fn repair_garbled_history_text(manifest: &mut ProjectManifest) -> bool {
    let mut changed = false;
    for point in &mut manifest.recovery_points {
        if looks_garbled_or_meaningless(&point.completed) {
            point.completed = "历史恢复点内容存在编码问题，等待人工确认。".to_string();
            changed = true;
        }
        if looks_garbled_or_meaningless(&point.next_step) {
            point.next_step = "请基于当前项目记录确认下一步。".to_string();
            changed = true;
        }
    }
    for message in &mut manifest.messages {
        if !looks_garbled_or_meaningless(&message.text) {
            continue;
        }
        message.text = match message.source.as_str() {
            "project_monitor" => "项目目录扫描记录已修复，请查看待确认事项。".to_string(),
            "local_rule" => "本条系统记录因历史编码问题已修复，建议查看项目动态。".to_string(),
            _ => "本条历史消息存在编码问题，已标记为需人工确认。".to_string(),
        };
        changed = true;
    }
    changed
}

fn repair_garbled_pending_reviews(manifest: &mut ProjectManifest) -> bool {
    let files_by_id = manifest
        .files
        .iter()
        .map(|file| (file.id.clone(), file.file_name.clone()))
        .collect::<HashMap<_, _>>();
    let mut changed = false;
    for item in &mut manifest.pending_reviews {
        let file_name = files_by_id
            .get(&item.file_id)
            .cloned()
            .or_else(|| {
                Path::new(&item.path)
                    .file_name()
                    .and_then(|value| value.to_str())
                    .map(|value| value.to_string())
            })
            .unwrap_or_else(|| "未命名文件".to_string());
        if looks_garbled_or_meaningless(&item.title) {
            item.title = match item.kind.as_str() {
                "managed_missing" => format!("受管文件缺失：{file_name}"),
                "source_moved" => format!("原始资料疑似被移动：{file_name}"),
                "source_missing" => format!("原始资料缺失：{file_name}"),
                "review_required" => format!("需要检查：{file_name}"),
                "duplicate_file" => format!("重复文件：{file_name}"),
                "modified_new_version" => format!("发现新版本：{file_name}"),
                "new_file" => format!("发现新文件：{file_name}"),
                _ => format!("待确认事项：{file_name}"),
            };
            changed = true;
        }
        if looks_garbled_or_meaningless(&item.detail) {
            item.detail = match item.kind.as_str() {
                "managed_missing" => {
                    "记录中的受管文件不存在，需要确认是否重新导入或修复路径。".to_string()
                }
                "source_moved" => {
                    "原始路径不可用，但发现疑似相同内容的新位置，需要人工确认。".to_string()
                }
                "source_missing" => "记录中的原始资料路径不可用，需要人工确认。".to_string(),
                "review_required" => "分类、解析或位置建议需要人工确认。".to_string(),
                "duplicate_file" => {
                    "发现相同内容已登记，不会重复复制；请确认是否保留新来源。".to_string()
                }
                "modified_new_version" => {
                    "疑似已有文件的新版本，需要确认版本关系和保存位置。".to_string()
                }
                "new_file" => "项目目录中发现未登记文件，需要确认是否纳入感冒院管理。".to_string(),
                _ => "该事项需要人工确认后再处理。".to_string(),
            };
            changed = true;
        }
    }
    changed
}

fn demote_analysis_generated_pending_reviews(manifest: &mut ProjectManifest) -> bool {
    let mut changed = false;
    for item in &mut manifest.pending_reviews {
        if item.kind == "projectState.proposal"
            && !matches!(item.status.as_str(), "resolved" | "ignored")
        {
            item.status = "ignored".to_string();
            item.updated_at = now_string();
            changed = true;
        }
    }
    changed
}

fn build_project_analysis_context(manifest: &ProjectManifest) -> Result<serde_json::Value, String> {
    let file_analysis = FileAnalysisDocument {
        schema_version: ANALYSIS_SCHEMA_VERSION,
        files: manifest
            .files
            .iter()
            .filter(|file| !unhealthy_source(manifest, "file", &file.id))
            .take(24)
            .map(|file| FileAnalysis {
                file_id: file.id.clone(),
                file_name: file.file_name.clone(),
                managed_path: file.managed_relative_path.clone(),
                parse_status: file.parse_status.clone(),
                content_summary: file.content_summary.clone(),
                main_fields_or_sections: file.main_fields_or_sections.clone(),
                recommended_category: file.recommended_category.clone(),
                parse_failure_reason: file.parse_failure_reason.clone(),
                extracted_at: file.modified_at.clone(),
                parser: file.parser.clone(),
                content_hash: file.content_hash.clone(),
                extracted_text_path: String::new(),
                page_count: file.page_count,
                sheet_count: file.sheet_count,
                row_count: file.row_count,
                column_count: file.column_count,
                warnings: file.analysis_warnings.clone(),
                document_type: String::new(),
                business_domain: String::new(),
                business_purpose: String::new(),
                business_summary: String::new(),
                document_purpose: String::new(),
                document_purpose_confidence: 0,
                document_purpose_evidence: Vec::new(),
                technical_detail: String::new(),
            })
            .collect(),
        updated_at: now_string(),
    };
    let key_history = manifest
        .messages
        .iter()
        .rev()
        .filter(|message| {
            matches!(
                message.kind.as_str(),
                "requirement" | "analysis" | "assistant" | "restore" | "fileLocation"
            )
        })
        .take(18)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .map(|message| {
            json!({
                "author": message.author,
                "kind": message.kind,
                "text": message.text,
                "createdAt": message.created_at,
                "source": message.source,
            })
        })
        .collect::<Vec<_>>();
    Ok(json!([
        {
            "role": "system",
            "content": "你是感冒院的项目理解分析器。只基于输入的项目说明、file-analysis、项目对话和关键决定输出 JSON。不要声称读取了原始文件。字段必须包含 status、projectDefinition、goals、roles、materialUsage、knownRequirements、gaps、questions、constraints、evidence、nextSteps。status 固定为 success。所有数组用简短中文句子。"
        },
        {
            "role": "user",
            "content": serde_json::to_string(&json!({
                "project": manifest.project,
                "fileAnalysis": file_analysis,
                "keyHistory": key_history,
                "currentAtlas": manifest.atlas,
            })).map_err(|err| format!("构建项目理解上下文失败：{err}"))?
        }
    ]))
}

fn parse_project_analysis_response(text: &str, model_id: &str) -> Result<ProjectAnalysis, String> {
    let trimmed = text.trim();
    let json_text = if trimmed.starts_with('{') {
        trimmed.to_string()
    } else {
        let start = trimmed
            .find('{')
            .ok_or_else(|| "DeepSeek 项目理解未返回 JSON 对象。".to_string())?;
        let end = trimmed
            .rfind('}')
            .ok_or_else(|| "DeepSeek 项目理解 JSON 不完整。".to_string())?;
        trimmed[start..=end].to_string()
    };
    let mut analysis: ProjectAnalysis = serde_json::from_str(&json_text)
        .map_err(|err| format!("解析 project-analysis 失败：{err}"))?;
    analysis.status = if analysis.status.trim().is_empty() {
        "success".to_string()
    } else {
        analysis.status
    };
    analysis.updated_at = now_string();
    analysis.model_id = model_id.to_string();
    analysis.failure_reason.clear();
    Ok(analysis)
}

fn import_into_records(
    root: &Path,
    records: &mut Vec<FileRecord>,
    file_paths: Vec<String>,
    created_source: &str,
    related_task: &str,
    route_override: Option<&InboxRouteOverride>,
) -> Result<ImportBatch, String> {
    ensure_project_dirs(root)?;
    let transaction_id = route_override
        .map(|intent| intent.operation_id.clone())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(new_id);
    let staging_dir = root
        .join(".ganmaoyuan")
        .join("staging")
        .join(&transaction_id);
    fs::create_dir_all(&staging_dir).map_err(|err| {
        format!(
            "创建导入暂存目录失败：{}：{err}",
            path_to_string(&staging_dir)
        )
    })?;

    let sources = validate_sources(file_paths)?;
    let original_records = records.clone();
    let mut created_paths = Vec::new();
    let mut created_content_paths = Vec::new();
    let mut new_records = Vec::new();
    let mut duplicates = Vec::new();
    let mut location_decisions = Vec::new();

    let result = (|| {
        for source in sources {
            let file_name = source
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(|| format!("无法识别文件名称：{}", path_to_string(&source)))?
                .to_string();
            let file_type = source
                .extension()
                .and_then(|extension| extension.to_str())
                .unwrap_or("unknown")
                .to_lowercase();
            let source_hash = hash_file(&source)?;
            let size_bytes = fs::metadata(&source)
                .map_err(|err| format!("读取文件信息失败：{file_name}：{err}"))?
                .len();
            let source_string = path_to_string(&source);

            if let Some(existing) = records
                .iter()
                .find(|item| !item.content_hash.is_empty() && item.content_hash == source_hash)
                .cloned()
            {
                if let Some(original) = records.iter_mut().find(|item| item.id == existing.id) {
                    if !original
                        .source_paths
                        .iter()
                        .any(|path| path.eq_ignore_ascii_case(&source_string))
                    {
                        original.source_paths.push(source_string.clone());
                    }
                }
                let mut duplicate = existing;
                duplicate.id = new_id();
                duplicate.original_source_path = source_string.clone();
                duplicate.source_paths = vec![source_string.clone()];
                duplicate.duplicate_of_file_id = records
                    .iter()
                    .find(|item| item.content_hash == source_hash)
                    .map(|item| item.id.clone());
                duplicate.created_at = now_string();
                duplicate.modified_at = duplicate.created_at.clone();
                duplicate.created_source = created_source.to_string();
                duplicate.related_task = related_task.to_string();
                duplicate.import_transaction_id = transaction_id.clone();
                duplicate.location_reason =
                    "内容哈希与已登记文件完全一致，复用受管副本，未重复复制。".to_string();
                records.push(duplicate.clone());
                location_decisions.push(location_decision(
                    &duplicate,
                    &source_string,
                    created_source,
                    related_task,
                    "导入资料",
                    &duplicate.location_reason,
                    duplicate.needs_user_confirmation,
                ));
                duplicates.push(duplicate.clone());
                new_records.push(duplicate);
                continue;
            }

            let staged = staging_dir.join(format!("{}-{}", new_id(), file_name));
            fs::copy(&source, &staged)
                .map_err(|err| format!("暂存复制失败：{file_name}：{err}"))?;
            let staged_hash = hash_file(&staged)?;
            if staged_hash != source_hash {
                return Err(format!("复制校验失败，原文件与暂存副本不一致：{file_name}"));
            }

            let plan = if let Some(intent) = route_override {
                decide_inbox_file_location(
                    root,
                    records,
                    &intent.managed_file_name,
                    &file_type,
                    created_source,
                    related_task,
                    &intent.category,
                )?
            } else {
                decide_file_location(
                    root,
                    records,
                    &file_name,
                    &file_type,
                    created_source,
                    related_task,
                    "导入资料",
                )?
            };
            fs::rename(&staged, &plan.destination)
                .map_err(|err| format!("提交受管副本失败：{file_name}：{err}"))?;
            created_paths.push(plan.destination.clone());
            let destination_hash = hash_file(&plan.destination)?;
            if destination_hash != source_hash {
                return Err(format!("归位校验失败，受管副本与原文件不一致：{file_name}"));
            }

            let now = now_string();
            let file_id = new_id();
            let managed_file_name = plan
                .destination
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or(&file_name)
                .to_string();
            let (mut analysis, extracted_text) = analyze_managed_file(
                &file_id,
                &managed_file_name,
                &plan.destination,
                &file_type,
                &source_hash,
            );
            if let Some(text) = extracted_text {
                let content_path = extracted_text_path(root, &file_id);
                write_text_atomic(&content_path, &text)?;
                analysis.extracted_text_path = path_to_string(&content_path);
                created_content_paths.push(content_path);
            }
            let needs_user_confirmation = matches!(
                analysis.parse_status.as_str(),
                "failed" | "partial" | "review_required"
            ) || plan.category == "待检查";
            let record = FileRecord {
                id: file_id.clone(),
                file_name: managed_file_name,
                original_source_path: source_string.clone(),
                managed_path: path_to_string(&plan.destination),
                file_type,
                category: plan.category.clone(),
                created_source: created_source.to_string(),
                related_task: related_task.to_string(),
                current_version: format!("v{}", plan.version_number),
                history_versions: Vec::new(),
                created_at: now.clone(),
                modified_at: now.clone(),
                still_exists: true,
                needs_user_confirmation,
                parse_status: analysis.parse_status,
                content_summary: analysis.content_summary,
                main_fields_or_sections: analysis.main_fields_or_sections,
                recommended_category: analysis.recommended_category,
                parse_failure_reason: analysis.parse_failure_reason,
                parser: analysis.parser,
                extracted_text_path: analysis.extracted_text_path,
                page_count: analysis.page_count,
                sheet_count: analysis.sheet_count,
                row_count: analysis.row_count,
                column_count: analysis.column_count,
                analysis_warnings: analysis.warnings,
                content_hash: source_hash,
                size_bytes,
                version_group_id: plan.version_group_id.clone(),
                duplicate_of_file_id: None,
                previous_version_id: plan.previous_version_id.clone(),
                source_paths: vec![source_string.clone()],
                managed_relative_path: plan.relative_path.clone(),
                location_reason: plan.reason.clone(),
                import_transaction_id: transaction_id.clone(),
                last_verified_at: now,
                original_file_name: file_name,
                document_family_id: plan.version_group_id.clone(),
                version_id: file_id.clone(),
                version_number: plan.version_number,
            };
            if let Some(previous_id) = plan.previous_version_id {
                if let Some(previous_record) =
                    records.iter_mut().find(|item| item.id == previous_id)
                {
                    if !previous_record.history_versions.contains(&file_id) {
                        previous_record.history_versions.push(file_id.clone());
                    }
                }
            }
            location_decisions.push(location_decision(
                &record,
                &source_string,
                created_source,
                related_task,
                "导入资料",
                &record.location_reason,
                record.needs_user_confirmation,
            ));
            records.push(record.clone());
            new_records.push(record);
        }
        Ok(())
    })();

    let _ = fs::remove_dir_all(&staging_dir);
    if let Err(err) = result {
        for path in created_content_paths.iter().rev() {
            let _ = fs::remove_file(path);
        }
        for path in created_paths.iter().rev() {
            let _ = fs::remove_file(path);
        }
        *records = original_records;
        return Err(err);
    }

    Ok(ImportBatch {
        new_records,
        duplicates,
        location_decisions,
        created_paths,
        created_content_paths,
    })
}

fn validate_sources(file_paths: Vec<String>) -> Result<Vec<PathBuf>, String> {
    if file_paths.is_empty() {
        return Ok(Vec::new());
    }
    let mut seen = HashSet::new();
    let mut result = Vec::new();
    let mut errors = Vec::new();
    for value in file_paths {
        let raw = PathBuf::from(&value);
        match canonical_existing_file(&raw) {
            Ok(path) => {
                let key = path_to_string(&path).to_lowercase();
                if seen.insert(key) {
                    result.push(path);
                }
            }
            Err(err) => errors.push(err),
        }
    }
    if errors.is_empty() {
        Ok(result)
    } else {
        Err(format!(
            "导入前校验失败，本轮没有复制任何文件：{}",
            errors.join("；")
        ))
    }
}

fn rollback_batch(batch: &ImportBatch) {
    for path in batch.created_content_paths.iter().rev() {
        let _ = fs::remove_file(path);
    }
    for path in batch.created_paths.iter().rev() {
        let _ = fs::remove_file(path);
    }
}

pub(crate) fn read_and_repair_manifest(root: &Path) -> Result<ProjectManifest, String> {
    let path = manifest_path(root);
    if !path.exists() {
        return Err(format!("项目位置清单不存在：{}", path_to_string(&path)));
    }
    let mut manifest: ProjectManifest = read_json(&path)?;
    let mut changed = manifest.schema_version != MANIFEST_SCHEMA_VERSION;
    manifest.schema_version = MANIFEST_SCHEMA_VERSION;
    // Projects created before a user entered a real next step carried this
    // initializer text forever. Treat only that exact system placeholder as
    // unset; preserve every other user-provided value.
    if manifest.project.next_step.trim() == INITIAL_PROJECT_NEXT_STEP {
        manifest.project.next_step.clear();
        changed = true;
    }
    if clear_non_user_next_step(&mut manifest) {
        changed = true;
    }
    if manifest.project.created_at.is_empty() {
        manifest.project.created_at = if manifest.project.last_opened_at.is_empty() {
            now_string()
        } else {
            manifest.project.last_opened_at.clone()
        };
        changed = true;
    }
    if manifest.project.manifest_path != path_to_string(&path) {
        manifest.project.manifest_path = path_to_string(&path);
        changed = true;
    }
    // Legacy projects may already have a verified Git snapshot but no durable
    // project-level link. Adopt only a same-project snapshot whose directory still exists.
    if manifest.project.repository_path.trim().is_empty() {
        if let Ok(Some(snapshot)) = read_git_snapshot(root) {
            let candidate = PathBuf::from(snapshot.repository_path.trim());
            if snapshot.project_id == manifest.project.id
                && !snapshot.repository_path.trim().is_empty()
                && candidate.is_dir()
            {
                manifest.project.repository_path = path_to_string(&candidate);
                changed = true;
            }
        }
    }
    if manifest.draft.project_id.is_empty() {
        manifest.draft.project_id = manifest.project.id.clone();
        changed = true;
    }
    if manifest.monitoring.status.is_empty() {
        manifest.monitoring.status = "idle".to_string();
        changed = true;
    }
    if demote_analysis_generated_pending_reviews(&mut manifest) {
        changed = true;
    }
    manifest.monitoring.pending_count = manifest
        .pending_reviews
        .iter()
        .filter(|item| !matches!(item.status.as_str(), "resolved" | "ignored"))
        .count();
    if manifest.daily_sessions.is_empty() && !manifest.messages.is_empty() {
        let message_ids = manifest
            .messages
            .iter()
            .map(|message| message.id.clone())
            .collect::<Vec<_>>();
        manifest.daily_sessions.push(crate::models::DailySession {
            id: new_id(),
            date_key: current_date_key(),
            started_at: manifest.project.created_at.clone(),
            updated_at: now_string(),
            message_ids,
            task_ids: manifest.tasks.iter().map(|task| task.id.clone()).collect(),
            decision_ids: manifest
                .decisions
                .iter()
                .map(|decision| decision.id.clone())
                .collect(),
            artifact_ids: manifest
                .artifacts
                .iter()
                .map(|artifact| artifact.id.clone())
                .collect(),
            imported_file_ids: manifest.files.iter().map(|file| file.id.clone()).collect(),
            recovery_point_ids: manifest
                .recovery_points
                .iter()
                .map(|point| point.id.clone())
                .collect(),
        });
        changed = true;
    }
    for file in &mut manifest.files {
        let managed = PathBuf::from(&file.managed_path);
        let exists = managed.exists() && managed.is_file();
        if file.still_exists != exists {
            file.still_exists = exists;
            file.needs_user_confirmation = true;
            changed = true;
        }
        if file.source_paths.is_empty() {
            file.source_paths.push(file.original_source_path.clone());
            changed = true;
        }
        if file.version_group_id.is_empty() {
            file.version_group_id = new_id();
            changed = true;
        }
        if file.original_file_name.is_empty() {
            file.original_file_name = file.file_name.clone();
            changed = true;
        }
        if file.document_family_id.is_empty() {
            file.document_family_id = file.version_group_id.clone();
            changed = true;
        }
        if file.version_id.is_empty() {
            file.version_id = file.id.clone();
            changed = true;
        }
        if file.version_number == 0 {
            file.version_number = parse_version_number(&file.current_version);
            changed = true;
        }
        if file.managed_relative_path.is_empty() {
            file.managed_relative_path = managed
                .strip_prefix(root)
                .map(path_to_string)
                .unwrap_or_else(|_| file.managed_path.clone());
            changed = true;
        }
        if file.content_hash.is_empty() && exists {
            file.content_hash = hash_file(&managed)?;
            file.size_bytes = fs::metadata(&managed)
                .map_err(|err| {
                    format!("读取受管文件信息失败：{}：{err}", path_to_string(&managed))
                })?
                .len();
            changed = true;
        }
        if file.last_verified_at.is_empty() {
            file.last_verified_at = now_string();
            changed = true;
        }
    }
    let previous_health = manifest.data_health_records.clone();
    update_data_health_records(&mut manifest);
    if manifest.data_health_records != previous_health {
        changed = true;
    }
    if repair_garbled_pending_reviews(&mut manifest) {
        changed = true;
    }
    if repair_garbled_history_text(&mut manifest) {
        changed = true;
    }
    if repair_garbled_project_display_text(&mut manifest) {
        changed = true;
    }
    if changed {
        persist_project(root, &manifest, None)?;
    }
    Ok(manifest)
}

const INITIAL_PROJECT_NEXT_STEP: &str = "查看资料分析结果，补充项目目标与当前任务。";
const GENERATED_NEXT_STEP_PLACEHOLDER: &str = "继续整理项目资料，确认下一步工作。";
const FACTS_ONLY_NEXT_STEP_PLACEHOLDER: &str = "根据当前项目事实继续下一步工作。";

pub(crate) fn persist_project(
    root: &Path,
    manifest: &ProjectManifest,
    previous: Option<&ProjectManifest>,
) -> Result<(), String> {
    // v1 summaries are derived from durable project facts. Refresh them only while
    // persisting an actual change, never from the high-frequency read paths.
    let mut prepared_manifest = manifest.clone();
    update_v1_foundation(root, &mut prepared_manifest)?;
    let manifest = &prepared_manifest;
    write_file_analysis(root, &manifest.files)?;
    write_json_atomic(&project_analysis_path(root), &manifest.project_analysis)?;
    write_json_atomic(&atlas_assessment_path(root), &manifest.atlas)?;
    write_json_atomic(
        &project_impact_analysis_path(root),
        &manifest.project_impact_analyses,
    )?;
    write_json_atomic(
        &project_state_proposals_path(root),
        &manifest.project_state_proposals,
    )?;
    write_json_atomic(
        &daily_continue_path(root),
        &manifest.daily_continue_snapshots,
    )?;
    write_json_atomic(
        &atlas_skill_assessments_path(root),
        &manifest.atlas_skill_assessments,
    )?;
    write_json_atomic(&skill_library_path(root), &manifest.skill_library)?;
    write_json_atomic(
        &knowledge_pattern_candidates_path(root),
        &manifest.knowledge_pattern_candidates,
    )?;
    write_json_atomic(
        &improvement_candidates_path(root),
        &manifest.improvement_candidates,
    )?;
    write_json_atomic(&v1_readiness_path(root), &manifest.v1_readiness)?;
    if let Err(err) = write_json_atomic(&manifest_path(root), manifest) {
        if let Some(previous) = previous {
            let _ = write_file_analysis(root, &previous.files);
            let _ = write_json_atomic(&project_analysis_path(root), &previous.project_analysis);
            let _ = write_json_atomic(&atlas_assessment_path(root), &previous.atlas);
            let _ = write_json_atomic(
                &project_impact_analysis_path(root),
                &previous.project_impact_analyses,
            );
            let _ = write_json_atomic(
                &project_state_proposals_path(root),
                &previous.project_state_proposals,
            );
            let _ = write_json_atomic(
                &daily_continue_path(root),
                &previous.daily_continue_snapshots,
            );
            let _ = write_json_atomic(
                &atlas_skill_assessments_path(root),
                &previous.atlas_skill_assessments,
            );
            let _ = write_json_atomic(&skill_library_path(root), &previous.skill_library);
            let _ = write_json_atomic(
                &knowledge_pattern_candidates_path(root),
                &previous.knowledge_pattern_candidates,
            );
            let _ = write_json_atomic(
                &improvement_candidates_path(root),
                &previous.improvement_candidates,
            );
            let _ = write_json_atomic(&v1_readiness_path(root), &previous.v1_readiness);
        }
        return Err(err);
    }
    Ok(())
}

pub(crate) fn write_file_analysis(root: &Path, files: &[FileRecord]) -> Result<(), String> {
    let analyses = files
        .iter()
        .map(|file| FileAnalysis {
            file_id: file.id.clone(),
            file_name: file.file_name.clone(),
            managed_path: file.managed_path.clone(),
            parse_status: file.parse_status.clone(),
            content_summary: file.content_summary.clone(),
            main_fields_or_sections: file.main_fields_or_sections.clone(),
            recommended_category: file.recommended_category.clone(),
            parse_failure_reason: file.parse_failure_reason.clone(),
            extracted_at: file.modified_at.clone(),
            parser: file.parser.clone(),
            content_hash: file.content_hash.clone(),
            extracted_text_path: file.extracted_text_path.clone(),
            page_count: file.page_count,
            sheet_count: file.sheet_count,
            row_count: file.row_count,
            column_count: file.column_count,
            warnings: file.analysis_warnings.clone(),
            document_type: String::new(),
            business_domain: String::new(),
            business_purpose: String::new(),
            business_summary: String::new(),
            document_purpose: String::new(),
            document_purpose_confidence: 0,
            document_purpose_evidence: Vec::new(),
            technical_detail: String::new(),
        })
        .collect::<Vec<_>>();
    write_json_atomic(
        &file_analysis_path(root),
        &FileAnalysisDocument {
            schema_version: ANALYSIS_SCHEMA_VERSION,
            files: analyses,
            updated_at: now_string(),
        },
    )
}

pub(crate) fn read_registry<R: Runtime>(app: &tauri::AppHandle<R>) -> Result<AppRegistry, String> {
    let path = registry_path(app)?;
    if !path.exists() {
        return Ok(AppRegistry {
            schema_version: MANIFEST_SCHEMA_VERSION,
            projects: Vec::new(),
            continue_preferences: Vec::new(),
        });
    }
    read_json(&path)
}

pub(crate) fn write_registry<R: Runtime>(
    app: &tauri::AppHandle<R>,
    registry: &AppRegistry,
) -> Result<(), String> {
    write_json_atomic(&registry_path(app)?, registry)
}

pub(crate) fn upsert_project<R: Runtime>(
    app: &tauri::AppHandle<R>,
    project: ProjectSummary,
) -> Result<(), String> {
    let mut registry = read_registry(app)?;
    registry.schema_version = MANIFEST_SCHEMA_VERSION;
    registry.projects.retain(|item| {
        item.id != project.id && !item.root_dir.eq_ignore_ascii_case(&project.root_dir)
    });
    registry.projects.insert(0, project);
    write_registry(app, &registry)
}

pub(crate) fn registry_path<R: Runtime>(app: &tauri::AppHandle<R>) -> Result<PathBuf, String> {
    Ok(global_data_dir(app)?.join("projects.json"))
}

pub(crate) fn memos_path<R: Runtime>(app: &tauri::AppHandle<R>) -> Result<PathBuf, String> {
    Ok(global_data_dir(app)?.join("memos.json"))
}

fn parse_ai_project_match(text: &str) -> Result<AiProjectMatchDecision, String> {
    let trimmed = text.trim();
    let start = trimmed
        .find('{')
        .ok_or_else(|| "DeepSeek 项目匹配未返回 JSON 对象。".to_string())?;
    let end = trimmed
        .rfind('}')
        .ok_or_else(|| "DeepSeek 项目匹配 JSON 不完整。".to_string())?;
    let mut decision: AiProjectMatchDecision = serde_json::from_str(&trimmed[start..=end])
        .map_err(|err| format!("解析 DeepSeek 项目匹配结果失败：{err}"))?;
    decision.score = decision.score.min(100);
    Ok(decision)
}

pub(crate) fn material_inbox_path<R: Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<PathBuf, String> {
    Ok(global_data_dir(app)?.join("material-inbox.json"))
}

// ---------------------------------------------------------------------------
// 全局（非项目）受管资料区。
// 原文件从不移动；只复制一份受管副本到 <AppData>/managed/{general,temporary}/，
// 通过 <AppData>/managed/files.json 登记，支持搜索、audit、undo。
// ---------------------------------------------------------------------------

fn global_managed_root<R: Runtime>(app: &tauri::AppHandle<R>) -> Result<PathBuf, String> {
    let root = global_data_dir(app)?.join("managed");
    fs::create_dir_all(&root)
        .map_err(|err| format!("创建全局受管资料区失败：{}：{err}", path_to_string(&root)))?;
    Ok(root)
}

fn global_files_document_path<R: Runtime>(app: &tauri::AppHandle<R>) -> Result<PathBuf, String> {
    Ok(global_managed_root(app)?.join("files.json"))
}

fn search_index_document_path<R: Runtime>(app: &tauri::AppHandle<R>) -> Result<PathBuf, String> {
    Ok(global_data_dir(app)?.join("search-index.json"))
}

fn write_search_index_document<R: Runtime>(
    app: &tauri::AppHandle<R>,
    document: &SearchIndexDocument,
) -> Result<(), String> {
    write_json_atomic(&search_index_document_path(app)?, document)
}

fn global_audit_path<R: Runtime>(app: &tauri::AppHandle<R>) -> Result<PathBuf, String> {
    let dir = global_managed_root(app)?.join("audit");
    fs::create_dir_all(&dir)
        .map_err(|err| format!("创建全局资料审计目录失败：{}：{err}", path_to_string(&dir)))?;
    Ok(dir.join("operations.jsonl"))
}

fn read_global_files_document<R: Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<GlobalFilesDocument, String> {
    let path = global_files_document_path(app)?;
    if !path.exists() {
        return Ok(GlobalFilesDocument::default());
    }
    read_json(&path)
}

fn write_global_files_document<R: Runtime>(
    app: &tauri::AppHandle<R>,
    document: &GlobalFilesDocument,
) -> Result<(), String> {
    write_json_atomic(&global_files_document_path(app)?, document)
}

fn global_destination_dir<R: Runtime>(
    app: &tauri::AppHandle<R>,
    destination: &str,
    item: &MaterialInboxItem,
) -> Result<PathBuf, String> {
    let folder = match destination {
        inbox_routing::GLOBAL_DEST_GENERAL => "general",
        inbox_routing::GLOBAL_DEST_TEMPORARY => "temporary",
        _ => return Err(format!("未知全局归位目标：{destination}")),
    };
    let mut dir = global_managed_root(app)?.join(folder);
    if destination == inbox_routing::GLOBAL_DEST_GENERAL {
        if let Some((domain, category)) = valid_general_material_path(
            &item.general_material_domain,
            &item.general_material_category,
        ) {
            dir = dir.join(domain).join(category);
        }
    }
    fs::create_dir_all(&dir)
        .map_err(|err| format!("创建全局资料目录失败：{}：{err}", path_to_string(&dir)))?;
    Ok(dir)
}

fn valid_general_material_path(
    domain: &str,
    category: &str,
) -> Option<(&'static str, &'static str)> {
    match (domain, category) {
        ("manufacturing", "productionData") => Some(("manufacturing", "productionData")),
        ("manufacturing", "productionReport") => Some(("manufacturing", "productionReport")),
        ("manufacturing", "orderData") => Some(("manufacturing", "orderData")),
        ("manufacturing", "qualityData") => Some(("manufacturing", "qualityData")),
        ("engineering", "vave") => Some(("engineering", "vave")),
        ("engineering", "improvement") => Some(("engineering", "improvement")),
        ("engineering", "designReference") => Some(("engineering", "designReference")),
        ("engineering", "technicalDocument") => Some(("engineering", "technicalDocument")),
        ("management", "workPlan") => Some(("management", "workPlan")),
        ("management", "masterData") => Some(("management", "masterData")),
        ("management", "meeting") => Some(("management", "meeting")),
        ("management", "notice") => Some(("management", "notice")),
        ("management", "report") => Some(("management", "report")),
        ("business", "quotation") => Some(("business", "quotation")),
        ("business", "supplier") => Some(("business", "supplier")),
        ("business", "customer") => Some(("business", "customer")),
        ("reference", "template") => Some(("reference", "template")),
        ("reference", "learning") => Some(("reference", "learning")),
        ("reference", "other") => Some(("reference", "other")),
        _ => None,
    }
}

fn general_material_location_label(category: &str) -> &'static str {
    match category {
        "productionData" => "生产数据",
        "productionReport" => "生产报表",
        "orderData" => "订单数据",
        "qualityData" => "质量数据",
        "vave" => "改善申请",
        "improvement" => "改善资料",
        "designReference" => "设计参考",
        "technicalDocument" => "技术文档",
        "workPlan" => "工作计划",
        "masterData" => "主数据/模板",
        "meeting" => "会议资料",
        "notice" => "通知公告",
        "report" => "管理报告",
        "quotation" => "报价资料",
        "supplier" => "供应商资料",
        "customer" => "客户资料",
        "template" => "模板",
        "learning" => "学习资料",
        "other" => "其他参考资料",
        _ => "通用工作资料区",
    }
}

pub fn list_global_files<R: Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<Vec<GlobalManagedFile>, String> {
    let document = read_global_files_document(&app)?;
    Ok(document
        .files
        .into_iter()
        .filter(|file| file.undone_at.is_empty())
        .collect())
}

/// 把 Inbox 条目安全归入全局受管区（general / temporary）。
/// 不移动原文件；复制受管副本；登记 + audit；支持 undo。
pub fn route_inbox_item_to_global<R: Runtime>(
    app: &tauri::AppHandle<R>,
    item_id: String,
    destination: String,
) -> Result<GlobalManagedFile, String> {
    route_inbox_item_to_global_internal(app, item_id, destination, false)
}

fn route_inbox_item_to_global_internal<R: Runtime>(
    app: &tauri::AppHandle<R>,
    item_id: String,
    destination: String,
    auto_routed: bool,
) -> Result<GlobalManagedFile, String> {
    let _guard = project_write_lock()
        .lock()
        .map_err(|_| "全局存储锁已损坏，请重启感冒院。".to_string())?;
    let mut document = read_material_inbox_document(&app)?;
    let idx = document
        .items
        .iter()
        .position(|item| item.id == item_id)
        .ok_or_else(|| "收件箱记录不存在。".to_string())?;
    let item = document.items[idx].clone();
    if item.source_path.trim().is_empty() || !Path::new(&item.source_path).is_file() {
        return Err("原文件不存在，无法归入全局资料区。".to_string());
    }
    if item.processing_status == inbox_routing::ROUTED && !item.global_destination.is_empty() {
        return Err("该记录已经归入全局资料区。".to_string());
    }
    let dir = global_destination_dir(&app, &destination, &item)?;
    let audit_path = global_audit_path(&app)?;
    let actor = if auto_routed { "system" } else { "user" };
    let entry = &mut document.items[idx];
    if matches!(
        entry.processing_status.as_str(),
        inbox_routing::PENDING_REVIEW | inbox_routing::FAILED
    ) {
        inbox_routing::transition(
            entry,
            inbox_routing::READY_TO_ROUTE,
            "通用资料归位条件已确认。",
            actor,
            None,
        )?;
    }
    if entry.processing_status != inbox_routing::READY_TO_ROUTE {
        return Err(format!(
            "当前 Inbox 状态不能归位：{}",
            entry.processing_status
        ));
    }
    inbox_routing::transition(
        entry,
        inbox_routing::ROUTING,
        "开始复制到全局受管目录。",
        actor,
        None,
    )?;
    document.updated_at = now_string();
    write_material_inbox_document(app, &document)?;

    let routing_item = document.items[idx].clone();
    let record = match perform_global_route(&dir, &audit_path, &routing_item, &destination) {
        Ok(record) => record,
        Err(error) => {
            let entry = &mut document.items[idx];
            sync_inbox_decision_traces(entry);
            update_trace_outcome(
                &mut entry.decision_traces,
                &["locationRecommendation", "autoRoute"],
                "pending",
                "failed",
                &error,
            );
            inbox_routing::transition(
                entry,
                inbox_routing::FAILED,
                "复制到全局受管目录失败。",
                "system",
                Some(("globalRouteFailed", &error)),
            )?;
            document.updated_at = now_string();
            write_material_inbox_document(app, &document)?;
            return Err(error);
        }
    };
    let mut global = read_global_files_document(&app)?;
    global.files.push(record.clone());
    global.updated_at = now_string();
    if let Err(error) = write_global_files_document(&app, &global) {
        if Path::new(&record.managed_path).is_file()
            && hash_file(Path::new(&record.managed_path)).ok().as_deref()
                == Some(record.content_hash.as_str())
        {
            let _ = fs::remove_file(&record.managed_path);
        }
        let entry = &mut document.items[idx];
        sync_inbox_decision_traces(entry);
        update_trace_outcome(
            &mut entry.decision_traces,
            &["locationRecommendation", "autoRoute"],
            "pending",
            "failed",
            &error,
        );
        inbox_routing::transition(
            entry,
            inbox_routing::FAILED,
            "写入全局文件登记失败，已回滚受管副本。",
            "system",
            Some(("globalManifestWriteFailed", &error)),
        )?;
        document.updated_at = now_string();
        write_material_inbox_document(app, &document)?;
        return Err(error);
    }
    let routed_note = format!(
        "已归入全局{}资料区。",
        match destination.as_str() {
            inbox_routing::GLOBAL_DEST_GENERAL => "通用",
            _ => "临时参考",
        }
    );
    let entry = &mut document.items[idx];
    entry.global_destination = destination.clone();
    entry.auto_routed = auto_routed;
    entry.recommended_location = if destination == inbox_routing::GLOBAL_DEST_GENERAL {
        general_material_location_label(&entry.general_material_category).to_string()
    } else {
        routed_note.clone()
    };
    entry.suggested_managed_path = record.managed_path.clone();
    entry.managed_file_id = record.id.clone();
    entry.can_undo = true;
    sync_inbox_decision_traces(entry);
    if auto_routed {
        let mut trace = new_decision_trace(
            "autoRoute",
            &entry.id,
            "",
            inbox_trace_evidence(entry),
            "高置信通用资料满足现有自动归位规则。".to_string(),
            format!("自动归入{}", entry.recommended_location),
            trace_confidence(entry.confidence_score),
        );
        trace.execution = "executed".to_string();
        trace.execution_note = "自动归位已完成。".to_string();
        upsert_pending_trace(&mut entry.decision_traces, trace);
    } else {
        update_trace_outcome(
            &mut entry.decision_traces,
            &[
                "fileClassification",
                "ownershipDecision",
                "locationRecommendation",
            ],
            "approved",
            "executed",
            "用户确认并完成全局资料归位。",
        );
    }
    inbox_routing::transition(entry, inbox_routing::ROUTED, routed_note, actor, None)?;
    document.updated_at = now_string();
    write_material_inbox_document(app, &document)?;
    Ok(record)
}

/// 全局归位核心（不依赖 app，便于测试）：复制受管副本并写审计行。
fn perform_global_route(
    destination_dir: &Path,
    audit_path: &Path,
    item: &MaterialInboxItem,
    destination: &str,
) -> Result<GlobalManagedFile, String> {
    let managed_file_name = if item.suggested_file_name.trim().is_empty() {
        inbox_routing::normalize_managed_file_name(&item.file_name, None)
    } else {
        item.suggested_file_name.clone()
    };
    let managed_path = unique_destination(destination_dir, &managed_file_name, None)?;
    fs::copy(&item.source_path, &managed_path).map_err(|err| {
        format!(
            "复制受管副本到全局资料区失败：{}：{err}",
            path_to_string(&managed_path)
        )
    })?;
    let operation_id = new_id();
    let managed_relative_path = valid_general_material_path(
        &item.general_material_domain,
        &item.general_material_category,
    )
    .map(|(domain, category)| format!("{domain}/{category}/{managed_file_name}"))
    .unwrap_or_else(|| managed_file_name.clone());
    let record = GlobalManagedFile {
        id: new_id(),
        file_name: item.file_name.clone(),
        source_path: item.source_path.clone(),
        managed_path: path_to_string(&managed_path),
        managed_relative_path,
        ownership_type: item.ownership_type.clone(),
        destination: destination.to_string(),
        category: item.suggested_category.clone(),
        related_project: item.recommended_project_name.clone(),
        general_material_domain: item.general_material_domain.clone(),
        general_material_category: item.general_material_category.clone(),
        material_semantic_type: item.material_semantic_type.clone(),
        file_type: item.file_type.clone(),
        content_hash: item.source_hash.clone(),
        document_type: item.document_type.clone(),
        business_domain: item.business_domain.clone(),
        business_purpose: item.business_purpose.clone(),
        business_summary: item.business_summary.clone(),
        document_purpose: item.document_purpose.clone(),
        document_purpose_confidence: item.document_purpose_confidence,
        document_purpose_evidence: item.document_purpose_evidence.clone(),
        content_summary: item.content_summary.clone(),
        inbox_item_id: item.id.clone(),
        operation_id: operation_id.clone(),
        created_at: now_string(),
        can_undo: true,
        undone_at: String::new(),
        lifecycle_status: "managed".to_string(),
        decision_trace_id: item
            .decision_traces
            .first()
            .map(|trace| trace.id.clone())
            .unwrap_or_default(),
    };
    if let Err(error) = append_json_line(
        audit_path,
        &serde_json::json!({
            "operation": "global.route",
            "operationId": operation_id,
            "inboxItemId": item.id,
            "destination": destination,
            "generalMaterialDomain": item.general_material_domain,
            "generalMaterialCategory": item.general_material_category,
            "materialSemanticType": item.material_semantic_type,
            "managedPath": record.managed_path,
            "createdAt": record.created_at,
        }),
    ) {
        if hash_file(&managed_path).ok().as_deref() == Some(record.content_hash.as_str()) {
            let _ = fs::remove_file(&managed_path);
        }
        return Err(error);
    }
    Ok(record)
}

/// 撤销全局归位：校验受管副本未被修改后删除副本，标记 undone。
pub fn undo_global_file<R: Runtime>(
    app: &tauri::AppHandle<R>,
    file_id: String,
) -> Result<GlobalManagedFile, String> {
    let _guard = project_write_lock()
        .lock()
        .map_err(|_| "全局存储锁已损坏，请重启感冒院。".to_string())?;
    let mut document = read_global_files_document(&app)?;
    let audit_path = global_audit_path(&app)?;
    let updated = perform_global_undo(&audit_path, &mut document, &file_id)?;
    document.updated_at = now_string();
    write_global_files_document(&app, &document)?;
    let mut inbox = read_material_inbox_document(app)?;
    let entry = inbox
        .items
        .iter_mut()
        .find(|entry| entry.id == updated.inbox_item_id)
        .ok_or_else(|| "全局资料对应的 Inbox 记录不存在。".to_string())?;
    entry.can_undo = false;
    entry.result_note = "全局归位已撤销，受管副本已删除。".to_string();
    sync_inbox_decision_traces(entry);
    for trace in &mut entry.decision_traces {
        if matches!(
            trace.decision_type.as_str(),
            "locationRecommendation" | "autoRoute"
        ) {
            trace.execution = "cancelled".to_string();
            trace.execution_note = "用户撤销全局归位。".to_string();
            trace.updated_at = now_string();
        }
    }
    inbox_routing::transition(
        entry,
        inbox_routing::PENDING_REVIEW,
        "全局归位已撤销。",
        "user",
        None,
    )?;
    inbox.updated_at = now_string();
    write_material_inbox_document(app, &inbox)?;
    Ok(updated)
}

/// 全局撤销核心（不依赖 app，便于测试）：删除受管副本并标记 undone。
fn perform_global_undo(
    audit_path: &Path,
    document: &mut GlobalFilesDocument,
    file_id: &str,
) -> Result<GlobalManagedFile, String> {
    let idx = document
        .files
        .iter()
        .position(|file| file.id == file_id)
        .ok_or_else(|| "全局资料登记不存在。".to_string())?;
    let record = document.files[idx].clone();
    if !record.can_undo {
        return Err("该全局资料不允许撤销。".to_string());
    }
    if !record.undone_at.is_empty() {
        return Err("该全局资料已经撤销。".to_string());
    }
    let managed = PathBuf::from(&record.managed_path);
    if managed.exists() {
        let current_hash = hash_file(&managed)?;
        if current_hash != record.content_hash {
            return Err("受管副本在归位后已被修改，未自动删除；请人工检查后再处理。".to_string());
        }
        fs::remove_file(&managed)
            .map_err(|err| format!("删除全局受管副本失败：{}：{err}", record.managed_path))?;
    }
    let mut updated = record.clone();
    updated.undone_at = now_string();
    updated.lifecycle_status = "superseded".to_string();
    document.files[idx] = updated.clone();
    append_json_line(
        audit_path,
        &serde_json::json!({
            "operation": "global.undo",
            "fileId": record.id,
            "managedPath": record.managed_path,
            "undoneAt": updated.undone_at,
        }),
    )?;
    Ok(updated)
}

fn perform_global_undo_by_operation(
    audit_path: &Path,
    document: &mut GlobalFilesDocument,
    operation_id: &str,
) -> Result<GlobalManagedFile, String> {
    let file_id = document
        .files
        .iter()
        .find(|file| file.operation_id == operation_id)
        .map(|file| file.id.clone())
        .ok_or_else(|| "未找到该整理操作产生的 Workspace 文件记录。".to_string())?;
    perform_global_undo(audit_path, document, &file_id)
}

fn inbox_routing_settings_path<R: Runtime>(app: &tauri::AppHandle<R>) -> Result<PathBuf, String> {
    Ok(global_data_dir(app)?.join("inbox-routing-settings.json"))
}

fn workspace_config_path<R: Runtime>(app: &tauri::AppHandle<R>) -> Result<PathBuf, String> {
    Ok(global_data_dir(app)?.join("workspace-config.json"))
}

fn default_workspace_root<R: Runtime>(app: &tauri::AppHandle<R>) -> Result<PathBuf, String> {
    let existing_legacy_root = Path::new(r"D:\GanMaoYuan_Workspace");
    if existing_legacy_root.exists() {
        return Ok(PathBuf::from(r"D:\GanMaoYuan_Workspace"));
    }
    let global = global_data_dir(app)?;
    Ok(global
        .parent()
        .map(|parent| parent.join("GanMaoYuan_Workspace"))
        .unwrap_or_else(|| global.join("GanMaoYuan_Workspace")))
}

fn normalize_workspace_root(root: &Path) -> Result<PathBuf, String> {
    if root.as_os_str().is_empty() {
        return Err("个人文件空间根目录不能为空。".to_string());
    }
    let absolute = if root.is_absolute() {
        root.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|err| format!("无法解析当前目录：{err}"))?
            .join(root)
    };
    Ok(absolute)
}

fn build_workspace_config(root: PathBuf, status: String) -> WorkspaceConfig {
    let root = normalize_workspace_root(&root).unwrap_or(root);
    let now = now_string();
    WorkspaceConfig {
        schema_version: WORKSPACE_CONFIG_SCHEMA_VERSION,
        workspace_root: path_to_string(&root),
        inbox_root: path_to_string(&root.join("00_Inbox")),
        projects_root: path_to_string(&root.join("10_Projects")),
        general_root: path_to_string(&root.join("20_General")),
        temporary_root: path_to_string(&root.join("30_Temporary")),
        archive_root: path_to_string(&root.join("40_Archive")),
        system_root: path_to_string(&root.join(".ganmaoyuan")),
        created_at: now.clone(),
        updated_at: now,
        status,
        last_verified_at: String::new(),
    }
}

fn repair_workspace_config<R: Runtime>(
    config: &mut WorkspaceConfig,
    app: &tauri::AppHandle<R>,
) -> Result<(), String> {
    let root = if config.workspace_root.trim().is_empty() {
        default_workspace_root(app)?
    } else {
        normalize_workspace_root(Path::new(&config.workspace_root))?
    };
    let previous_created_at = config.created_at.clone();
    let previous_updated_at = config.updated_at.clone();
    *config = build_workspace_config(root, config.status.clone());
    config.created_at = if previous_created_at.is_empty() {
        config.created_at.clone()
    } else {
        previous_created_at
    };
    config.updated_at = if previous_updated_at.is_empty() {
        config.updated_at.clone()
    } else {
        previous_updated_at
    };
    Ok(())
}

fn workspace_standard_dirs(config: &WorkspaceConfig) -> Vec<PathBuf> {
    [
        &config.workspace_root,
        &config.inbox_root,
        &config.projects_root,
        &config.general_root,
        &config.temporary_root,
        &config.archive_root,
        &config.system_root,
    ]
    .iter()
    .map(|value| PathBuf::from(value))
    .collect()
}

fn verify_workspace_config_status(mut config: WorkspaceConfig) -> WorkspaceConfig {
    let dirs = workspace_standard_dirs(&config);
    let all_exist = dirs.iter().all(|directory| directory.is_dir());
    config.status = if all_exist {
        match verify_workspace_writable(Path::new(&config.system_root)) {
            Ok(()) => "ready".to_string(),
            Err(_) => "unavailable".to_string(),
        }
    } else {
        "missing".to_string()
    };
    config.schema_version = WORKSPACE_CONFIG_SCHEMA_VERSION;
    config.last_verified_at = now_string();
    config.updated_at = config.last_verified_at.clone();
    config
}

fn verify_workspace_writable(system_root: &Path) -> Result<(), String> {
    let probe = system_root.join(".write-test.tmp");
    write_text_atomic(&probe, "ganmaoyuan workspace write probe")?;
    fs::remove_file(&probe).map_err(|err| {
        format!(
            "清理个人文件空间写入探针失败：{}：{err}",
            path_to_string(&probe)
        )
    })
}

fn workspace_scan_document_path<R: Runtime>(app: &tauri::AppHandle<R>) -> Result<PathBuf, String> {
    Ok(global_data_dir(app)?.join("workspace-scan-batches.json"))
}

fn read_workspace_scan_document<R: Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<WorkspaceScanDocument, String> {
    let path = workspace_scan_document_path(app)?;
    if !path.exists() {
        return Ok(WorkspaceScanDocument::default());
    }
    read_json(&path)
}

fn write_workspace_scan_document<R: Runtime>(
    app: &tauri::AppHandle<R>,
    document: &WorkspaceScanDocument,
) -> Result<(), String> {
    write_json_atomic(&workspace_scan_document_path(app)?, document)
}

fn cleanup_plan_document_path<R: Runtime>(app: &tauri::AppHandle<R>) -> Result<PathBuf, String> {
    Ok(global_data_dir(app)?.join("cleanup-plans.json"))
}

fn read_cleanup_plan_document<R: Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<CleanupPlanDocument, String> {
    let path = cleanup_plan_document_path(app)?;
    if !path.exists() {
        return Ok(CleanupPlanDocument::default());
    }
    read_json(&path)
}

fn write_cleanup_plan_document<R: Runtime>(
    app: &tauri::AppHandle<R>,
    document: &CleanupPlanDocument,
) -> Result<(), String> {
    write_json_atomic(&cleanup_plan_document_path(app)?, document)
}

fn cleanup_execution_document_path<R: Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<PathBuf, String> {
    Ok(global_data_dir(app)?.join("cleanup-executions.json"))
}

fn read_cleanup_execution_document<R: Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<CleanupExecutionDocument, String> {
    let path = cleanup_execution_document_path(app)?;
    if !path.exists() {
        return Ok(CleanupExecutionDocument::default());
    }
    read_json(&path)
}

fn write_cleanup_execution_document<R: Runtime>(
    app: &tauri::AppHandle<R>,
    document: &CleanupExecutionDocument,
) -> Result<(), String> {
    write_json_atomic(&cleanup_execution_document_path(app)?, document)
}

fn persist_cleanup_plan<R: Runtime>(
    app: &tauri::AppHandle<R>,
    plan: CleanupPlan,
) -> Result<CleanupPlan, String> {
    let _guard = project_write_lock()
        .lock()
        .map_err(|_| "全局存储锁已损坏，请重启感冒院。".to_string())?;
    let mut document = read_cleanup_plan_document(app)?;
    append_cleanup_plan(&mut document, plan.clone());
    write_cleanup_plan_document(app, &document)?;
    Ok(plan)
}

fn append_cleanup_plan(document: &mut CleanupPlanDocument, plan: CleanupPlan) {
    document
        .plans
        .retain(|item| item.id != plan.id && item.scan_batch_id != plan.scan_batch_id);
    document.plans.push(plan);
    document
        .plans
        .sort_by(|left, right| right.created_at.cmp(&left.created_at));
    document.plans.truncate(50);
    document.updated_at = now_string();
}

fn update_cleanup_plan<R, F>(
    app: &tauri::AppHandle<R>,
    plan_id: &str,
    mutate: F,
) -> Result<CleanupPlan, String>
where
    R: Runtime,
    F: FnOnce(&mut CleanupPlan) -> Result<(), String>,
{
    let _guard = project_write_lock()
        .lock()
        .map_err(|_| "全局存储锁已损坏，请重启感冒院。".to_string())?;
    let mut document = read_cleanup_plan_document(app)?;
    let index = document
        .plans
        .iter()
        .position(|plan| plan.id == plan_id)
        .ok_or_else(|| format!("整理方案不存在：{plan_id}"))?;
    mutate(&mut document.plans[index])?;
    if document.plans[index].status == "draft" {
        document.plans[index].status = "reviewing".to_string();
    }
    document.updated_at = now_string();
    let plan = document.plans[index].clone();
    write_cleanup_plan_document(app, &document)?;
    Ok(plan)
}

fn apply_cleanup_review(
    item: &mut CleanupPlanItem,
    review_status: &str,
    user_reason: Option<&str>,
    workspace: &WorkspaceConfig,
) -> Result<(), String> {
    let normalized = normalize_cleanup_review_status(review_status)?;
    if normalized == "approved" {
        validate_cleanup_item_target(item, workspace)?;
    }
    let (user_decision, execution, note) = match normalized.as_str() {
        "approved" => ("approved", "pending", "用户确认整理建议，等待后续执行。"),
        "rejected" => ("rejected", "cancelled", "用户拒绝该整理建议。"),
        _ => return Err("批量审核仅支持 approved 或 rejected。".to_string()),
    };
    item.review_status = normalized;
    item.reviewed_at = now_string();
    item.reviewer = "localUser".to_string();
    item.user_reason = user_reason.unwrap_or("").trim().to_string();
    let trace = cleanup_review_trace(item, user_decision, execution, note, user_reason);
    push_cleanup_item_trace(item, trace);
    Ok(())
}

fn apply_cleanup_modification(
    item: &mut CleanupPlanItem,
    final_project: Option<String>,
    final_category: Option<String>,
    final_target_path: String,
    user_reason: Option<&str>,
    workspace: &WorkspaceConfig,
) -> Result<(), String> {
    let final_target_path = validate_cleanup_target_path(&final_target_path, workspace)?;
    item.review_status = "modified".to_string();
    item.reviewed_at = now_string();
    item.reviewer = "localUser".to_string();
    item.user_reason = user_reason.unwrap_or("").trim().to_string();
    item.final_project = final_project.unwrap_or_default().trim().to_string();
    item.final_category = final_category.unwrap_or_default().trim().to_string();
    item.final_target_path = final_target_path;
    let preview = LocationPreviewService::preview(
        &item.source_path,
        Path::new(&item.final_target_path),
        Some(Path::new(&workspace.workspace_root)),
        &item.final_project,
        &item.recommended_ownership,
        &item.final_category,
        "用户修改后的统一位置预览。",
    );
    item.location_preview_id = preview.preview_id;
    let trace = cleanup_review_trace(
        item,
        "modified",
        "pending",
        "用户修改了整理建议，等待后续执行。",
        user_reason,
    );
    push_cleanup_item_trace(item, trace);
    Ok(())
}

fn push_cleanup_item_trace(item: &mut CleanupPlanItem, trace: DecisionTrace) {
    item.decision_trace_id = trace.id.clone();
    item.decision_trace = trace.clone();
    item.decision_traces.push(trace);
}

fn cleanup_review_trace(
    item: &CleanupPlanItem,
    user_decision: &str,
    execution: &str,
    execution_note: &str,
    user_reason: Option<&str>,
) -> DecisionTrace {
    let mut evidence = item.evidence.clone();
    if !item.recommended_target_path.trim().is_empty() {
        evidence.push(DecisionTraceEvidence {
            kind: "aiRecommendation".to_string(),
            label: "AI 原建议".to_string(),
            summary: format!(
                "{} -> {}",
                item.recommended_ownership, item.recommended_target_path
            ),
            source_id: item.id.clone(),
        });
    }
    if !item.final_target_path.trim().is_empty() {
        evidence.push(DecisionTraceEvidence {
            kind: "userModification".to_string(),
            label: "用户最终位置".to_string(),
            summary: item.final_target_path.clone(),
            source_id: item.id.clone(),
        });
    }
    let mut trace = new_decision_trace(
        "locationRecommendation",
        &item.id,
        "",
        evidence,
        format!(
            "整理项审核：{}",
            if item.final_target_path.trim().is_empty() {
                non_empty_or_pending(&item.recommended_target_path)
            } else {
                item.final_target_path.clone()
            }
        ),
        user_reason
            .filter(|value| !value.trim().is_empty())
            .map(|value| format!("用户说明：{}", value.trim()))
            .unwrap_or_else(|| execution_note.to_string()),
        item.confidence.clone(),
    );
    trace.user_decision = user_decision.to_string();
    trace.execution = execution.to_string();
    trace.execution_note = execution_note.to_string();
    trace
}

fn normalize_cleanup_review_status(value: &str) -> Result<String, String> {
    match value.trim() {
        "approved" => Ok("approved".to_string()),
        "rejected" => Ok("rejected".to_string()),
        "pending" => Ok("pending".to_string()),
        "modified" => Ok("modified".to_string()),
        other => Err(format!("不支持的整理审核状态：{other}")),
    }
}

fn cleanup_plan_item_matches_filter(
    item: &CleanupPlanItem,
    filter: &CleanupPlanReviewFilter,
) -> bool {
    if !filter.recommended_target_path_prefix.trim().is_empty()
        && !item
            .recommended_target_path
            .to_lowercase()
            .starts_with(&filter.recommended_target_path_prefix.trim().to_lowercase())
    {
        return false;
    }
    if !filter.recommended_ownership.trim().is_empty()
        && item.recommended_ownership != filter.recommended_ownership.trim()
    {
        return false;
    }
    if !filter.document_purpose.trim().is_empty()
        && item.document_purpose != filter.document_purpose.trim()
    {
        return false;
    }
    if !filter.final_category.trim().is_empty()
        && item.final_category != filter.final_category.trim()
        && item.recommended_target_path != filter.final_category.trim()
    {
        return false;
    }
    if !filter.confidence_level.trim().is_empty()
        && item.confidence.level != filter.confidence_level.trim()
    {
        return false;
    }
    true
}

fn validate_cleanup_item_target(
    item: &CleanupPlanItem,
    workspace: &WorkspaceConfig,
) -> Result<(), String> {
    let target = if item.final_target_path.trim().is_empty() {
        &item.recommended_target_path
    } else {
        &item.final_target_path
    };
    let target = validate_cleanup_target_path(target, workspace)?;
    let preview = LocationPreviewService::preview(
        &item.source_path,
        Path::new(&target),
        Some(Path::new(&workspace.workspace_root)),
        if item.final_project.trim().is_empty() {
            &item.recommended_project
        } else {
            &item.final_project
        },
        &item.recommended_ownership,
        if item.final_category.trim().is_empty() {
            &item.document_purpose
        } else {
            &item.final_category
        },
        "Cleanup Plan 审核后的统一位置预览。",
    );
    if !item.location_preview_id.trim().is_empty()
        && !LocationPreviewService::matches(&item.location_preview_id, &preview)
    {
        return Err("目标位置已偏离审核时的位置预览，请重新审核。".to_string());
    }
    Ok(())
}

fn validate_cleanup_target_path(
    target: &str,
    workspace: &WorkspaceConfig,
) -> Result<String, String> {
    let target = target.trim();
    if target.is_empty() || target == "待确认" {
        return Err("目标位置不能为空，待确认项不能直接确认。".to_string());
    }
    let path = PathBuf::from(target);
    if path
        .components()
        .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err("目标位置不能包含上级目录跳转。".to_string());
    }
    let absolute = if path.is_absolute() {
        path
    } else {
        PathBuf::from(&workspace.workspace_root).join(path)
    };
    let normalized_target = path_to_string(&absolute).to_lowercase();
    let normalized_root = path_to_string(&PathBuf::from(&workspace.workspace_root)).to_lowercase();
    if normalized_target.starts_with(r"\\") || !normalized_target.starts_with(&normalized_root) {
        return Err("目标位置必须位于当前 Workspace Root 内。".to_string());
    }
    Ok(path_to_string(&absolute))
}

fn execute_cleanup_plan_item(
    workspace: &WorkspaceConfig,
    audit_path: &Path,
    global_document: &mut GlobalFilesDocument,
    item: &CleanupPlanItem,
) -> CleanupExecutionItem {
    let operation_id = new_id();
    let source = PathBuf::from(&item.source_path);
    let mut execution = CleanupExecutionItem {
        cleanup_plan_item_id: item.id.clone(),
        source_path: item.source_path.clone(),
        operation_id: operation_id.clone(),
        status: "running".to_string(),
        ..CleanupExecutionItem::default()
    };
    let result = (|| -> Result<GlobalManagedFile, String> {
        if !source.is_file() {
            return Err("原文件不存在，无法执行整理。".to_string());
        }
        let hash_before = hash_file(&source)?;
        if !item.hash.trim().is_empty() && item.hash != hash_before {
            return Err("执行前原文件 hash 已变化，请重新扫描后再整理。".to_string());
        }
        let target = cleanup_item_final_target(item, workspace)?;
        let target_dir = target
            .parent()
            .ok_or_else(|| "目标路径缺少父目录。".to_string())?;
        let target_file_name = target
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or(&item.file_name);
        let destination = unique_destination(target_dir, target_file_name, None)?;
        if global_document
            .files
            .iter()
            .any(|file| file.operation_id == operation_id && file.undone_at.is_empty())
        {
            return Err("该整理操作已经登记，拒绝重复执行。".to_string());
        }
        fs::copy(&source, &destination).map_err(|err| {
            format!(
                "复制到 Workspace 失败：{}：{err}",
                path_to_string(&destination)
            )
        })?;
        let hash_after = hash_file(&destination)?;
        if hash_before != hash_after {
            if hash_file(&destination).ok().as_deref() == Some(hash_after.as_str()) {
                let _ = fs::remove_file(&destination);
            }
            return Err("复制后 hash 校验失败，已停止登记。".to_string());
        }
        execution.hash_before = hash_before.clone();
        execution.hash_after = hash_after.clone();
        execution.target_path = path_to_string(&destination);
        let record =
            cleanup_global_record(item, &destination, &hash_after, &operation_id, workspace);
        append_json_line(
            audit_path,
            &serde_json::json!({
                "operation": "cleanup.execute.copy",
                "operationId": operation_id,
                "cleanupPlanItemId": item.id,
                "sourcePath": item.source_path,
                "targetPath": record.managed_path,
                "hash": hash_after,
                "createdAt": record.created_at,
            }),
        )
        .map_err(|error| {
            if hash_file(&destination).ok().as_deref() == Some(hash_after.as_str()) {
                let _ = fs::remove_file(&destination);
            }
            error
        })?;
        global_document.files.push(record.clone());
        global_document.updated_at = now_string();
        Ok(record)
    })();
    match result {
        Ok(record) => {
            execution.status = "completed".to_string();
            execution.global_file_id = record.id;
        }
        Err(error) => {
            execution.status = "failed".to_string();
            execution.error = error;
        }
    }
    execution
}

fn link_workspace_files_to_project_impacts<R: Runtime>(
    app: &tauri::AppHandle<R>,
    global_document: &GlobalFilesDocument,
    batch: &CleanupExecutionBatch,
) -> Result<(), String> {
    let registry = read_registry(app)?;
    for execution in batch.items.iter().filter(|item| item.status == "completed") {
        let Some(global_file) = global_document
            .files
            .iter()
            .find(|file| file.id == execution.global_file_id)
        else {
            continue;
        };
        let related_project = global_file.related_project.trim();
        if related_project.is_empty() {
            continue;
        }
        let Some(project) = registry.projects.iter().find(|project| {
            normalize_key(&project.name) == normalize_key(related_project)
                || normalize_key(&project.id) == normalize_key(related_project)
        }) else {
            continue;
        };
        let root = PathBuf::from(&project.root_dir);
        if !root.exists() {
            continue;
        }
        let mut manifest = match read_and_repair_manifest(&root) {
            Ok(value) => value,
            Err(_) => continue,
        };
        let file = workspace_global_file_as_project_file(global_file);
        if manifest.project_impact_analyses.iter().any(|item| {
            item.source_file_id == file.id
                && item.source_hash == file.content_hash
                && item.analysis_version == "v0.1-local"
        }) {
            continue;
        }
        let impact = build_local_project_impact(&manifest, &file);
        let candidates = build_action_candidates_from_impact(&impact);
        let proposal = build_project_state_proposal(&manifest, &file, &impact);
        upsert_pending_reviews_for_impact(&mut manifest, &impact, &candidates, &proposal);
        manifest.project_impact_analyses.push(impact);
        for candidate in candidates {
            if !manifest.project_action_candidates.iter().any(|item| {
                item.source_hash == candidate.source_hash
                    && item.candidate_type == candidate.candidate_type
                    && normalize_key(&item.title) == normalize_key(&candidate.title)
            }) {
                manifest.project_action_candidates.push(candidate);
            }
        }
        if !manifest.project_state_proposals.iter().any(|item| {
            item.source_hashes.contains(&file.content_hash) && item.status != "rejected"
        }) {
            manifest.project_state_proposals.push(proposal);
        }
        update_daily_continue_snapshot(&mut manifest);
        manifest.audit.push(audit_event(
            "workspace.project.impact",
            &global_file.id,
            "needsReview",
            format!(
                "Workspace 文件已关联项目并生成影响提醒：{}",
                global_file.file_name
            ),
            false,
            true,
        ));
        persist_project(&root, &manifest, None)?;
        append_audit_event(&root, manifest.audit.last().expect("audit event exists"))?;
    }
    Ok(())
}

fn workspace_global_file_as_project_file(file: &GlobalManagedFile) -> FileRecord {
    FileRecord {
        id: format!("workspace-{}", file.id),
        file_name: file.file_name.clone(),
        original_source_path: file.source_path.clone(),
        managed_path: file.managed_path.clone(),
        managed_relative_path: file.managed_relative_path.clone(),
        file_type: file.file_type.clone(),
        category: workspace_category_display(file),
        created_source: "Workspace".to_string(),
        related_task: String::new(),
        current_version: "v1".to_string(),
        created_at: file.created_at.clone(),
        modified_at: file.created_at.clone(),
        still_exists: Path::new(&file.managed_path).exists(),
        needs_user_confirmation: false,
        parse_status: "success".to_string(),
        content_summary: if file.business_summary.trim().is_empty() {
            file.content_summary.clone()
        } else {
            file.business_summary.clone()
        },
        main_fields_or_sections: file.document_purpose_evidence.clone(),
        recommended_category: workspace_category_display(file),
        content_hash: file.content_hash.clone(),
        source_paths: vec![file.source_path.clone()],
        location_reason: "Workspace 整理执行后关联项目影响分析。".to_string(),
        import_transaction_id: file.operation_id.clone(),
        last_verified_at: now_string(),
        ..FileRecord::default()
    }
}

fn cleanup_item_final_target(
    item: &CleanupPlanItem,
    workspace: &WorkspaceConfig,
) -> Result<PathBuf, String> {
    let target = if item.final_target_path.trim().is_empty() {
        &item.recommended_target_path
    } else {
        &item.final_target_path
    };
    validate_cleanup_target_path(target, workspace).map(PathBuf::from)
}

fn cleanup_global_record(
    item: &CleanupPlanItem,
    destination: &Path,
    hash: &str,
    operation_id: &str,
    workspace: &WorkspaceConfig,
) -> GlobalManagedFile {
    let managed_path = path_to_string(destination);
    let managed_relative_path = destination
        .strip_prefix(&workspace.workspace_root)
        .map(path_to_string)
        .unwrap_or_else(|_| managed_path.clone());
    GlobalManagedFile {
        id: new_id(),
        file_name: item.file_name.clone(),
        source_path: item.source_path.clone(),
        managed_path,
        managed_relative_path,
        ownership_type: item.recommended_ownership.clone(),
        destination: "workspaceCleanup".to_string(),
        category: if item.final_category.trim().is_empty() {
            item.required_action.clone()
        } else {
            item.final_category.clone()
        },
        related_project: if item.final_project.trim().is_empty() {
            item.recommended_project.clone()
        } else {
            item.final_project.clone()
        },
        material_semantic_type: item.document_purpose.clone(),
        file_type: Path::new(&item.file_name)
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("")
            .to_string(),
        content_hash: hash.to_string(),
        document_type: item
            .evidence
            .iter()
            .find(|evidence| evidence.kind == "documentType")
            .map(|evidence| evidence.summary.clone())
            .unwrap_or_else(|| item.document_purpose.clone()),
        business_summary: item
            .evidence
            .iter()
            .map(|evidence| evidence.summary.clone())
            .filter(|value| !value.trim().is_empty())
            .take(3)
            .collect::<Vec<_>>()
            .join("；"),
        document_purpose: item.document_purpose.clone(),
        document_purpose_confidence: item.confidence.score,
        document_purpose_evidence: item
            .evidence
            .iter()
            .map(|evidence| evidence.summary.clone())
            .collect(),
        content_summary: item
            .evidence
            .iter()
            .map(|evidence| evidence.summary.clone())
            .find(|value| !value.trim().is_empty())
            .unwrap_or_else(|| item.file_name.clone()),
        operation_id: operation_id.to_string(),
        created_at: now_string(),
        can_undo: true,
        undone_at: String::new(),
        lifecycle_status: "managed".to_string(),
        decision_trace_id: item.decision_trace_id.clone(),
        ..GlobalManagedFile::default()
    }
}

fn cleanup_execution_trace(
    item: &CleanupPlanItem,
    execution: &CleanupExecutionItem,
) -> DecisionTrace {
    let mut evidence = item.evidence.clone();
    evidence.push(DecisionTraceEvidence {
        kind: "execution".to_string(),
        label: "安全复制结果".to_string(),
        summary: if execution.status == "completed" {
            format!("已复制到 {}", execution.target_path)
        } else {
            format!("复制失败：{}", non_empty_or_pending(&execution.error))
        },
        source_id: execution.operation_id.clone(),
    });
    let mut trace = new_decision_trace(
        "autoRoute",
        &item.id,
        "",
        evidence,
        "按用户已确认的整理方案执行安全复制。".to_string(),
        non_empty_or_pending(&execution.target_path),
        item.confidence.clone(),
    );
    trace.user_decision = "approved".to_string();
    trace
}

fn build_cleanup_plan_from_scan_batch(
    batch: &WorkspaceScanBatch,
    workspace: &WorkspaceConfig,
    existing_plan_id: Option<String>,
) -> CleanupPlan {
    let created_at = now_string();
    let items = batch
        .files
        .iter()
        .map(|file| build_cleanup_plan_item(file, workspace))
        .collect::<Vec<_>>();
    CleanupPlan {
        id: existing_plan_id.unwrap_or_else(new_id),
        scan_batch_id: batch.id.clone(),
        created_at,
        status: "draft".to_string(),
        items,
    }
}

fn build_cleanup_plan_item(
    file: &WorkspaceScanFile,
    workspace: &WorkspaceConfig,
) -> CleanupPlanItem {
    let recommended_ownership = cleanup_plan_ownership(file);
    let evidence = cleanup_plan_evidence(file);
    let decision_trace = select_cleanup_decision_trace(file, &evidence, &recommended_ownership);
    let required_action = cleanup_plan_required_action(file, &recommended_ownership);
    let location_preview = cleanup_plan_location_preview(file, workspace, &recommended_ownership);
    CleanupPlanItem {
        id: new_id(),
        source_path: file.path.clone(),
        file_name: file.file_name.clone(),
        hash: file.hash.clone(),
        current_location: Path::new(&file.path)
            .parent()
            .map(path_to_string)
            .unwrap_or_default(),
        recommended_project: file.recommended_project_name.clone(),
        recommended_target_path: location_preview.target_path.clone(),
        document_purpose: file.document_purpose.clone(),
        confidence: trace_confidence(file.confidence_score),
        evidence,
        decision_trace_id: decision_trace.id.clone(),
        location_preview_id: location_preview.preview_id,
        decision_trace: decision_trace.clone(),
        decision_traces: vec![decision_trace],
        review_status: "pending".to_string(),
        recommended_ownership,
        required_action,
        ..CleanupPlanItem::default()
    }
}

fn cleanup_plan_ownership(file: &WorkspaceScanFile) -> String {
    if file.status == "failed" || file.not_suitable_for_ai_context {
        return "unsupportedOrFailed".to_string();
    }
    if file.needs_confirmation || file.confidence_score < 78 {
        return "needsReview".to_string();
    }
    match file.ownership_type.as_str() {
        "existingProjectMaterial" => "existingProject",
        "generalWorkMaterial" => "generalWorkMaterial",
        "temporaryOrReference" => "temporaryOrReference",
        "newProjectCandidate" => "newProjectCandidate",
        "unsupportedOrFailed" => "unsupportedOrFailed",
        _ => "needsReview",
    }
    .to_string()
}

fn cleanup_plan_required_action(file: &WorkspaceScanFile, recommended_ownership: &str) -> String {
    if cleanup_scan_file_is_duplicate(file) {
        return "duplicateNoCopy".to_string();
    }
    match recommended_ownership {
        "existingProject" => "readyForReview".to_string(),
        "generalWorkMaterial" if file.confidence_score >= 78 && !file.needs_confirmation => {
            "readyForReview".to_string()
        }
        "generalWorkMaterial" => "confirmGeneralLocation".to_string(),
        "temporaryOrReference" => "confirmTemporaryOrReference".to_string(),
        "newProjectCandidate" => "createProjectCandidate".to_string(),
        "unsupportedOrFailed" => "skipUnsupported".to_string(),
        _ => "manualReview".to_string(),
    }
}

fn cleanup_plan_location_preview(
    file: &WorkspaceScanFile,
    workspace: &WorkspaceConfig,
    recommended_ownership: &str,
) -> crate::models::LocationPreview {
    let target = match recommended_ownership {
        "existingProject" => PathBuf::from(non_empty_or_pending(&file.recommended_location)),
        "generalWorkMaterial" => path_to_string(
            &PathBuf::from(&workspace.general_root)
                .join(cleanup_general_subdir(file))
                .join(&file.file_name),
        )
        .into(),
        "temporaryOrReference" => PathBuf::from(&workspace.temporary_root).join(&file.file_name),
        "newProjectCandidate" => {
            let project_name = cleanup_path_segment(&file.recommended_project_name)
                .or_else(|| cleanup_path_segment(&project_candidate_name_from_file(file)))
                .unwrap_or_else(|| "待命名项目".to_string());
            PathBuf::from(&workspace.projects_root)
                .join(project_name)
                .join("00_Inbox")
                .join(&file.file_name)
        }
        _ => PathBuf::from("待确认"),
    };
    LocationPreviewService::preview(
        &file.path,
        &target,
        Some(Path::new(&workspace.workspace_root)),
        &file.recommended_project_name,
        recommended_ownership,
        &file.document_purpose,
        "Workspace 扫描理解结果经统一位置预览生成，尚未执行文件操作。",
    )
}

fn cleanup_general_subdir(file: &WorkspaceScanFile) -> String {
    let category = file.recommended_category.to_lowercase();
    let purpose = file.document_purpose.to_lowercase();
    let doc_type = file.document_type.to_lowercase();
    if category.contains("模板") || purpose == "template" || doc_type.contains("模板") {
        "模板".to_string()
    } else if category.contains("主数据")
        || doc_type.contains("主数据")
        || doc_type.contains("编码")
        || doc_type.contains("bom")
    {
        "主数据".to_string()
    } else if category.contains("参考") || purpose == "reference" {
        "参考资料".to_string()
    } else if purpose == "report" || category.contains("报告") {
        "工作报告".to_string()
    } else if purpose == "plan" || category.contains("计划") {
        "工作计划".to_string()
    } else {
        non_empty_or_pending(&file.recommended_category)
    }
}

fn cleanup_plan_evidence(file: &WorkspaceScanFile) -> Vec<DecisionTraceEvidence> {
    if let Some(trace) = preferred_cleanup_trace(file) {
        return trace.input_evidence.iter().take(8).cloned().collect();
    }
    let mut evidence = vec![DecisionTraceEvidence {
        kind: "fileName".to_string(),
        label: "文件名".to_string(),
        summary: first_line(&file.file_name, 160),
        source_id: file.id.clone(),
    }];
    if !file.content_summary.trim().is_empty() {
        evidence.push(DecisionTraceEvidence {
            kind: "summary".to_string(),
            label: "内容摘要".to_string(),
            summary: first_line(&file.content_summary, 240),
            source_id: file.id.clone(),
        });
    }
    if !file.document_type.trim().is_empty() {
        evidence.push(DecisionTraceEvidence {
            kind: "documentType".to_string(),
            label: "系统判断".to_string(),
            summary: file.document_type.clone(),
            source_id: file.id.clone(),
        });
    }
    if !file.recommended_project_name.trim().is_empty() {
        evidence.push(DecisionTraceEvidence {
            kind: "projectCandidate".to_string(),
            label: "项目候选".to_string(),
            summary: file.recommended_project_name.clone(),
            source_id: file.id.clone(),
        });
    }
    if !file.recommended_location.trim().is_empty() {
        evidence.push(DecisionTraceEvidence {
            kind: "locationRecommendation".to_string(),
            label: "推荐位置".to_string(),
            summary: file.recommended_location.clone(),
            source_id: file.id.clone(),
        });
    }
    evidence
}

fn select_cleanup_decision_trace(
    file: &WorkspaceScanFile,
    evidence: &[DecisionTraceEvidence],
    recommended_ownership: &str,
) -> DecisionTrace {
    if let Some(trace) = preferred_cleanup_trace(file) {
        return trace.clone();
    }
    let recommendation = match recommended_ownership {
        "existingProject" => format!(
            "建议作为已有项目资料处理，计划位置：{}",
            non_empty_or_pending(&file.recommended_location)
        ),
        "generalWorkMaterial" => {
            "建议作为通用工作资料，进入个人工作文件空间的 General 区域。".to_string()
        }
        "temporaryOrReference" => "建议作为临时或参考资料，先进入 Temporary 区域。".to_string(),
        "newProjectCandidate" => "疑似新项目资料，建议先作为候选项目待确认。".to_string(),
        "unsupportedOrFailed" => "不适合进入 AI 上下文或解析失败，建议跳过或人工检查。".to_string(),
        _ => "证据不足，建议人工确认后再整理。".to_string(),
    };
    new_decision_trace(
        "locationRecommendation",
        &file.id,
        "",
        evidence.to_vec(),
        format!(
            "{}；{}",
            non_empty_or_pending(&file.document_type),
            non_empty_or_pending(&file.document_purpose)
        ),
        recommendation,
        trace_confidence(file.confidence_score),
    )
}

fn preferred_cleanup_trace(file: &WorkspaceScanFile) -> Option<&DecisionTrace> {
    let preferred_types = [
        "locationRecommendation",
        "projectMatch",
        "ownershipDecision",
        "fileClassification",
    ];
    preferred_types
        .iter()
        .find_map(|trace_type| {
            file.decision_traces
                .iter()
                .find(|trace| trace.decision_type == *trace_type)
        })
        .or_else(|| file.decision_traces.first())
}

fn cleanup_scan_file_is_duplicate(file: &WorkspaceScanFile) -> bool {
    file.decision_traces.iter().any(|trace| {
        let haystack = format!(
            "{} {} {}",
            trace.ai_understanding, trace.recommendation, trace.execution_note
        );
        haystack.contains("重复") || haystack.to_lowercase().contains("duplicate")
    })
}

fn project_candidate_name_from_file(file: &WorkspaceScanFile) -> String {
    file.file_name
        .split(['.', '-', '_', ' '])
        .next()
        .unwrap_or("")
        .to_string()
}

fn cleanup_path_segment(value: &str) -> Option<String> {
    let cleaned = value
        .trim()
        .chars()
        .map(|ch| match ch {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => ' ',
            _ => ch,
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if cleaned.is_empty() {
        None
    } else {
        Some(cleaned)
    }
}

fn non_empty_or_pending(value: &str) -> String {
    if value.trim().is_empty() {
        "待确认".to_string()
    } else {
        value.trim().to_string()
    }
}

fn canonical_scan_directory(path: &Path) -> Result<PathBuf, String> {
    if path.as_os_str().is_empty() {
        return Err("扫描目录不能为空。".to_string());
    }
    if !path.exists() {
        return Err(format!("扫描目录不存在：{}", path_to_string(path)));
    }
    if !path.is_dir() {
        return Err(format!("扫描路径不是目录：{}", path_to_string(path)));
    }
    path.canonicalize()
        .map_err(|err| format!("规范化扫描目录失败：{}：{err}", path_to_string(path)))
}

fn default_user_directory(name: &str) -> Result<PathBuf, String> {
    let user_profile = std::env::var("USERPROFILE")
        .map_err(|_| "无法读取 USERPROFILE，不能定位用户目录。".to_string())?;
    canonical_scan_directory(&PathBuf::from(user_profile).join(name))
}

fn persist_workspace_scan_batch<R: Runtime>(
    app: &tauri::AppHandle<R>,
    batch: WorkspaceScanBatch,
) -> Result<WorkspaceScanBatch, String> {
    let _guard = project_write_lock()
        .lock()
        .map_err(|_| "全局存储锁已损坏，请重启感冒院。".to_string())?;
    let mut document = read_workspace_scan_document(app)?;
    append_workspace_scan_batch(&mut document, batch.clone());
    write_workspace_scan_document(app, &document)?;
    Ok(batch)
}

fn append_workspace_scan_batch(document: &mut WorkspaceScanDocument, batch: WorkspaceScanBatch) {
    document.batches.retain(|item| item.id != batch.id);
    document.batches.push(batch);
    document
        .batches
        .sort_by(|left, right| right.created_at.cmp(&left.created_at));
    document.batches.truncate(30);
    document.updated_at = now_string();
}

fn build_failed_workspace_scan_batch(
    source_directory: String,
    reason: String,
) -> WorkspaceScanBatch {
    let now = now_string();
    WorkspaceScanBatch {
        id: new_id(),
        source_directory,
        created_at: now.clone(),
        completed_at: now,
        status: "failed".to_string(),
        failure_reason: reason,
        ..WorkspaceScanBatch::default()
    }
}

fn failed_workspace_scan_batch<R: Runtime>(
    app: &tauri::AppHandle<R>,
    source_directory: String,
    reason: String,
) -> Result<WorkspaceScanBatch, String> {
    persist_workspace_scan_batch(
        app,
        build_failed_workspace_scan_batch(source_directory, reason),
    )
}

fn run_workspace_scan<R: Runtime>(
    app: &tauri::AppHandle<R>,
    source_directory: PathBuf,
) -> Result<WorkspaceScanBatch, String> {
    let source_directory = match canonical_scan_directory(&source_directory) {
        Ok(value) => value,
        Err(err) => {
            return failed_workspace_scan_batch(app, path_to_string(&source_directory), err)
        }
    };
    let batch_id = new_id();
    let created_at = now_string();
    let source_directory_string = path_to_string(&source_directory);
    let files = match collect_workspace_scan_files(&source_directory, &batch_id) {
        Ok(files) => files,
        Err(err) => {
            return failed_workspace_scan_batch(app, source_directory_string, err);
        }
    };
    let registry = read_registry(app)?;
    let project_contexts = registry
        .projects
        .into_iter()
        .filter_map(|project| {
            let root = PathBuf::from(&project.root_dir);
            read_json::<ProjectManifest>(&manifest_path(&root))
                .ok()
                .map(|manifest| (project, manifest))
        })
        .collect::<Vec<_>>();
    let general_history = read_global_files_document(app)?.files;
    let mut analyzed_files = Vec::new();
    for file in files {
        analyzed_files.push(analyze_workspace_scan_file(
            &file,
            &project_contexts,
            &general_history,
        ));
    }
    let mut batch = WorkspaceScanBatch {
        id: batch_id,
        source_directory: source_directory_string,
        created_at,
        completed_at: now_string(),
        file_count: analyzed_files.len() as u64,
        status: "completed".to_string(),
        files: analyzed_files,
        ..WorkspaceScanBatch::default()
    };
    summarize_workspace_scan_batch(&mut batch);
    persist_workspace_scan_batch(app, batch)
}

fn collect_workspace_scan_files(
    root: &Path,
    batch_id: &str,
) -> Result<Vec<WorkspaceScanFile>, String> {
    let mut files = Vec::new();
    collect_workspace_scan_files_recursive(root, root, batch_id, &mut files)?;
    Ok(files)
}

fn collect_workspace_scan_files_recursive(
    root: &Path,
    current: &Path,
    batch_id: &str,
    files: &mut Vec<WorkspaceScanFile>,
) -> Result<(), String> {
    let entries = fs::read_dir(current)
        .map_err(|err| format!("读取扫描目录失败：{}：{err}", path_to_string(current)))?;
    for entry in entries {
        let entry = entry
            .map_err(|err| format!("读取扫描目录项失败：{}：{err}", path_to_string(current)))?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|err| format!("读取文件类型失败：{}：{err}", path_to_string(&path)))?;
        if file_type.is_dir() {
            if should_exclude_scan_directory(root, &path) {
                continue;
            }
            collect_workspace_scan_files_recursive(root, &path, batch_id, files)?;
            continue;
        }
        if !file_type.is_file() || should_exclude_scan_file(&path) {
            continue;
        }
        let path_string = path_to_string(&path);
        let file_name = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("")
            .to_string();
        let extension = path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("unknown")
            .to_lowercase();
        let metadata = match fs::metadata(&path) {
            Ok(metadata) => metadata,
            Err(err) => {
                files.push(WorkspaceScanFile {
                    id: new_id(),
                    path: path_string,
                    file_name,
                    extension,
                    scan_batch_id: batch_id.to_string(),
                    status: "failed".to_string(),
                    failure_reason: format!("读取文件信息失败：{err}"),
                    needs_confirmation: true,
                    ..WorkspaceScanFile::default()
                });
                continue;
            }
        };
        files.push(WorkspaceScanFile {
            id: new_id(),
            path: path_string,
            file_name,
            extension,
            size_bytes: metadata.len(),
            modified_at: metadata
                .modified()
                .map(system_time_to_string)
                .unwrap_or_default(),
            scan_batch_id: batch_id.to_string(),
            status: "scanned".to_string(),
            ..WorkspaceScanFile::default()
        });
    }
    Ok(())
}

fn should_exclude_scan_directory(root: &Path, path: &Path) -> bool {
    if is_system_scan_directory(path) {
        return true;
    }
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_lowercase();
    if matches!(
        name.as_str(),
        ".git"
            | "node_modules"
            | "target"
            | ".cache"
            | "cache"
            | ".tmp"
            | "tmp"
            | "temp"
            | "dist"
            | "build"
            | ".next"
            | ".vite"
            | "__pycache__"
    ) {
        return true;
    }
    path.strip_prefix(root)
        .ok()
        .and_then(|relative| relative.components().next())
        .is_some_and(|component| {
            component
                .as_os_str()
                .to_string_lossy()
                .eq_ignore_ascii_case("System Volume Information")
        })
}

fn is_system_scan_directory(path: &Path) -> bool {
    let value = path_to_string(path).to_lowercase();
    value.starts_with(r"c:\windows")
        || value.starts_with(r"c:\program files")
        || value.starts_with(r"c:\program files (x86)")
        || value.contains(r"\appdata\local\temp")
}

fn should_exclude_scan_file(path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_lowercase();
    name.starts_with("~$")
        || name.ends_with(".tmp")
        || name.ends_with(".temp")
        || name.ends_with(".crdownload")
        || name.ends_with(".part")
        || name.ends_with(".lnk")
}

fn analyze_workspace_scan_file(
    file: &WorkspaceScanFile,
    projects: &[(ProjectSummary, ProjectManifest)],
    general_history: &[GlobalManagedFile],
) -> WorkspaceScanFile {
    if file.status == "failed" {
        return file.clone();
    }
    let mut next = file.clone();
    next.not_suitable_for_ai_context = is_not_suitable_for_ai_context(&next.extension);
    if next.not_suitable_for_ai_context {
        next.status = "notSuitableForAIContext".to_string();
        next.parse_status = "skipped".to_string();
        next.content_summary = "可作为文件事实记录，但不适合进入 AI 项目上下文。".to_string();
        next.document_type = "程序或安装包".to_string();
        next.document_purpose = "reference".to_string();
        next.ownership_type = "unsupportedOrFailed".to_string();
        next.recommended_category = "待人工确认".to_string();
        next.recommended_location = "不建议进入 AI 上下文".to_string();
        next.confidence_level = "low".to_string();
        next.confidence_score = 0;
        next.needs_confirmation = true;
        return next;
    }
    let hash = match hash_file(Path::new(&next.path)) {
        Ok(hash) => hash,
        Err(err) => {
            next.status = "failed".to_string();
            next.failure_reason = err;
            next.needs_confirmation = true;
            return next;
        }
    };
    next.hash = hash.clone();
    let (analysis, _) = analyze_managed_file(
        &next.id,
        &next.file_name,
        Path::new(&next.path),
        &next.extension,
        &hash,
    );
    match build_material_inbox_item_with_history(
        projects,
        general_history,
        &next.path,
        &hash,
        next.size_bytes,
        &analysis,
        "workspaceScanner",
    ) {
        Ok(mut item) => {
            sync_inbox_decision_traces(&mut item);
            let is_high_confidence = matches!(item.confidence_level.as_str(), "high");
            next.status = "preview".to_string();
            next.parse_status = item.parse_status;
            next.content_summary = item.content_summary;
            next.document_type = item.document_type;
            next.document_purpose = item.document_purpose;
            next.ownership_type = item.ownership_type;
            next.recommended_project_name = if item.recommended_project_name.is_empty() {
                item.target_project_name
            } else {
                item.recommended_project_name
            };
            next.recommended_location = item.recommended_location;
            next.recommended_category = item.recommended_category;
            next.confidence_level = item.confidence_level;
            next.confidence_score = item.confidence_score;
            next.needs_confirmation = !is_high_confidence
                || matches!(
                    item.processing_status.as_str(),
                    inbox_routing::PENDING_REVIEW | inbox_routing::FAILED
                );
            next.decision_traces = item.decision_traces;
        }
        Err(err) => {
            next.status = "failed".to_string();
            next.failure_reason = err;
            next.needs_confirmation = true;
        }
    }
    next
}

fn is_not_suitable_for_ai_context(extension: &str) -> bool {
    matches!(
        extension,
        "exe" | "msi" | "msix" | "appx" | "dll" | "sys" | "bat" | "cmd" | "ps1"
    )
}

fn summarize_workspace_scan_batch(batch: &mut WorkspaceScanBatch) {
    batch.organizable_count = batch
        .files
        .iter()
        .filter(|file| {
            file.status == "preview"
                && !file.needs_confirmation
                && !file.not_suitable_for_ai_context
                && file.confidence_score >= 78
        })
        .count() as u64;
    batch.needs_confirmation_count = batch
        .files
        .iter()
        .filter(|file| file.needs_confirmation && file.status != "failed")
        .count() as u64;
    batch.unknown_count = batch
        .files
        .iter()
        .filter(|file| {
            file.status == "failed"
                || file.not_suitable_for_ai_context
                || file.confidence_score < 45
                || file.ownership_type.is_empty()
        })
        .count() as u64;
}

fn system_time_to_string(time: SystemTime) -> String {
    time.duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .to_string()
}

pub fn manifest_path(root: &Path) -> PathBuf {
    root.join(".ganmaoyuan/project-location-manifest.json")
}

pub fn file_analysis_path(root: &Path) -> PathBuf {
    root.join(".ganmaoyuan/file-analysis.json")
}

pub fn project_analysis_path(root: &Path) -> PathBuf {
    root.join(".ganmaoyuan/project-analysis.json")
}

pub fn project_impact_analysis_path(root: &Path) -> PathBuf {
    root.join(".ganmaoyuan/project-impact-analyses.json")
}

pub fn project_state_proposals_path(root: &Path) -> PathBuf {
    root.join(".ganmaoyuan/project-state-proposals.json")
}

pub fn daily_continue_path(root: &Path) -> PathBuf {
    root.join(".ganmaoyuan/daily-continue-snapshots.json")
}

pub fn atlas_assessment_path(root: &Path) -> PathBuf {
    root.join(".ganmaoyuan/atlas-assessment.json")
}

pub fn atlas_skill_assessments_path(root: &Path) -> PathBuf {
    root.join(".ganmaoyuan/atlas-skill-assessments.json")
}

pub fn skill_library_path(root: &Path) -> PathBuf {
    root.join(".ganmaoyuan/skill-library.json")
}

pub fn knowledge_pattern_candidates_path(root: &Path) -> PathBuf {
    root.join(".ganmaoyuan/knowledge-pattern-candidates.json")
}

pub fn improvement_candidates_path(root: &Path) -> PathBuf {
    root.join(".ganmaoyuan/improvement-candidates.json")
}

pub fn v1_readiness_path(root: &Path) -> PathBuf {
    root.join(".ganmaoyuan/v1-readiness/v1-readiness.json")
}

pub fn work_ledger_path(root: &Path) -> PathBuf {
    root.join(".ganmaoyuan/work-ledger.json")
}

pub fn git_snapshot_path(root: &Path) -> PathBuf {
    root.join(".ganmaoyuan/git-snapshot.json")
}

pub fn codex_results_path(root: &Path) -> PathBuf {
    root.join(".ganmaoyuan/codex-results.json")
}

pub fn codex_result_bridge_dir(root: &Path) -> PathBuf {
    root.join(".ganmaoyuan").join("codex").join("results")
}

pub fn codex_tasks_dir(root: &Path) -> PathBuf {
    root.join(".ganmaoyuan/codex/tasks")
}

pub fn codex_runs_dir(root: &Path) -> PathBuf {
    root.join(".ganmaoyuan/codex/runs")
}

pub fn codex_runs_path(root: &Path) -> PathBuf {
    root.join(".ganmaoyuan/codex/runs.json")
}

fn codex_task_path(root: &Path, task_id: &str) -> PathBuf {
    codex_tasks_dir(root).join(format!("{task_id}.json"))
}

fn codex_run_output_path(root: &Path, run_id: &str) -> PathBuf {
    codex_runs_dir(root).join(format!("{run_id}-last-message.txt"))
}

pub(crate) fn recovery_dir(root: &Path) -> PathBuf {
    root.join(".ganmaoyuan/recovery")
}

fn workspace_events_path(root: &Path) -> PathBuf {
    root.join(".ganmaoyuan/workspace/events.jsonl")
}

fn audit_events_path(root: &Path) -> PathBuf {
    root.join(".ganmaoyuan/audit/operations.jsonl")
}

fn append_workspace_event(root: &Path, message: &WorkspaceMessage) -> Result<(), String> {
    append_json_line(&workspace_events_path(root), message)
}

fn append_audit_event(root: &Path, event: &AuditEvent) -> Result<(), String> {
    append_json_line(&audit_events_path(root), event)
}

fn read_work_ledger(root: &Path) -> Result<WorkLedgerDocument, String> {
    let path = work_ledger_path(root);
    if !path.exists() {
        return Ok(WorkLedgerDocument::default());
    }
    read_json(&path)
}

fn write_work_ledger(root: &Path, document: &WorkLedgerDocument) -> Result<(), String> {
    write_json_atomic(&work_ledger_path(root), document)
}

fn read_codex_external_results(root: &Path) -> Result<CodexExternalResultsDocument, String> {
    let path = codex_results_path(root);
    if !path.exists() {
        return Ok(CodexExternalResultsDocument::default());
    }
    read_json(&path)
}

fn write_codex_external_results(
    root: &Path,
    document: &CodexExternalResultsDocument,
) -> Result<(), String> {
    write_json_atomic(&codex_results_path(root), document)
}

fn read_codex_task(root: &Path, task_id: &str) -> Result<CodexTask, String> {
    let path = codex_task_path(root, task_id);
    if !path.exists() {
        return Err(format!("Codex task not found: {task_id}"));
    }
    read_json(&path).map(normalize_codex_task_compat)
}

fn write_codex_task(root: &Path, task: &CodexTask) -> Result<(), String> {
    fs::create_dir_all(codex_tasks_dir(root))
        .map_err(|err| format!("创建 Codex 任务目录失败：{err}"))?;
    let normalized = normalize_codex_task_compat(task.clone());
    write_json_atomic(&codex_task_path(root, &normalized.task_id), &normalized)
}

fn read_codex_run(root: &Path, run_id: &str) -> Result<CodexRun, String> {
    read_codex_runs(root)?
        .into_iter()
        .find(|run| run.id == run_id)
        .ok_or_else(|| format!("Codex run not found: {run_id}"))
}

fn read_codex_runs(root: &Path) -> Result<Vec<CodexRun>, String> {
    let path = codex_runs_path(root);
    if !path.exists() {
        return Ok(Vec::new());
    }
    let mut document: CodexRunDocument = read_json(&path)?;
    document.runs.sort_by(|left, right| {
        right
            .started_at
            .cmp(&left.started_at)
            .then_with(|| right.id.cmp(&left.id))
    });
    Ok(document
        .runs
        .into_iter()
        .map(normalize_codex_run_compat)
        .collect())
}

fn write_codex_runs(root: &Path, runs: Vec<CodexRun>) -> Result<(), String> {
    let document = CodexRunDocument {
        schema_version: 1,
        runs: runs.into_iter().map(normalize_codex_run_compat).collect(),
        updated_at: now_string(),
    };
    write_json_atomic(&codex_runs_path(root), &document)
}

fn upsert_codex_run(root: &Path, run: CodexRun) -> Result<(), String> {
    if let Some(parent) = codex_runs_path(root).parent() {
        fs::create_dir_all(parent).map_err(|err| format!("创建 CodexRun 目录失败：{err}"))?;
    }
    let mut runs = read_codex_runs(root)?;
    if let Some(existing) = runs.iter_mut().find(|item| item.id == run.id) {
        *existing = run;
    } else {
        runs.push(run);
    }
    write_codex_runs(root, runs)
}

fn read_codex_tasks(root: &Path) -> Result<Vec<CodexTask>, String> {
    let dir = codex_tasks_dir(root);
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut tasks = Vec::new();
    for entry in fs::read_dir(&dir).map_err(|err| format!("读取 Codex 任务目录失败：{err}"))?
    {
        let path = entry
            .map_err(|err| format!("读取 Codex 任务条目失败：{err}"))?
            .path();
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        if let Ok(task) = read_json::<CodexTask>(&path) {
            tasks.push(normalize_codex_task_compat(task));
        }
    }
    tasks.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
    Ok(tasks)
}

fn normalize_codex_task_compat(mut task: CodexTask) -> CodexTask {
    task.status = normalize_codex_task_status(&task.status);
    task.task_type = normalize_codex_task_type(&task.task_type);
    if task.task_type.trim().is_empty() {
        task.task_type = if task.prompt.trim().is_empty()
            && task.result_id.trim().is_empty()
            && task.summary.trim().is_empty()
        {
            "legacy".to_string()
        } else {
            infer_codex_task_type(&format!(
                "{}\n{}\n{}",
                task.title, task.prompt, task.summary
            ))
        };
    }
    if task.result_text.trim().is_empty() && !task.summary.trim().is_empty() {
        task.result_text = task.summary.clone();
    }
    task
}

fn normalize_codex_run_compat(mut run: CodexRun) -> CodexRun {
    run.status = normalize_codex_run_status(&run.status);
    run
}

fn read_git_snapshot(root: &Path) -> Result<Option<GitSnapshot>, String> {
    let path = git_snapshot_path(root);
    if !path.exists() {
        return Ok(None);
    }
    read_json(&path).map(Some)
}

fn write_git_snapshot(root: &Path, snapshot: &GitSnapshot) -> Result<(), String> {
    write_json_atomic(&git_snapshot_path(root), snapshot)
}

fn work_event_key(source_type: &str, source_ref: &str, event_type: &str) -> String {
    format!(
        "{}|{}|{}",
        normalize_key(source_type),
        normalize_key(source_ref),
        normalize_key(event_type)
    )
}

fn append_work_event_if_new(
    root: &Path,
    mut event: WorkEvent,
) -> Result<Option<WorkEvent>, String> {
    // Appending from Git and user review must not lose either source's facts.
    static WRITE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    let _guard = WRITE_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .map_err(|_| "工作事实存储锁已损坏，请重启感冒院。".to_string())?;
    let mut document = read_work_ledger(root)?;
    let next_key = work_event_key(&event.source_type, &event.source_ref, &event.event_type);
    if document.events.iter().any(|existing| {
        work_event_key(
            &existing.source_type,
            &existing.source_ref,
            &existing.event_type,
        ) == next_key
    }) {
        return Ok(None);
    }
    if event.id.is_empty() {
        event.id = new_id();
    }
    if event.created_at.is_empty() {
        event.created_at = now_string();
    }
    if event.occurred_at.is_empty() {
        event.occurred_at = event.created_at.clone();
    }
    if event.fact_kind.is_empty() {
        event.fact_kind = "fact".to_string();
    }
    document.events.push(event.clone());
    document.updated_at = now_string();
    write_work_ledger(root, &document)?;
    Ok(Some(event))
}

fn fact_event(
    project_id: &str,
    source_type: &str,
    source_ref: &str,
    event_type: &str,
    summary: String,
    evidence_refs: Vec<String>,
    score: u8,
) -> WorkEvent {
    WorkEvent {
        id: new_id(),
        project_id: project_id.to_string(),
        source_type: source_type.to_string(),
        source_ref: source_ref.to_string(),
        event_type: event_type.to_string(),
        fact_kind: "fact".to_string(),
        occurred_at: now_string(),
        summary,
        evidence_refs,
        confidence: trace_confidence(score),
        created_at: now_string(),
        ..WorkEvent::default()
    }
}

fn ensure_project_dirs(root: &Path) -> Result<(), String> {
    for directory in [
        ".ganmaoyuan",
        ".ganmaoyuan/managed",
        ".ganmaoyuan/managed/01_originals",
        ".ganmaoyuan/managed/02_requirements",
        ".ganmaoyuan/managed/03_data",
        ".ganmaoyuan/managed/04_images",
        ".ganmaoyuan/managed/05_outputs",
        ".ganmaoyuan/managed/06_prompts",
        ".ganmaoyuan/managed/07_tests",
        ".ganmaoyuan/managed/08_deliverables",
        ".ganmaoyuan/managed/99_review",
        ".ganmaoyuan/analysis/content",
        ".ganmaoyuan/workspace",
        ".ganmaoyuan/codex",
        ".ganmaoyuan/codex/results",
        ".ganmaoyuan/codex/tasks",
        ".ganmaoyuan/recovery",
        ".ganmaoyuan/audit",
        ".ganmaoyuan/staging",
        ".ganmaoyuan/v1-readiness",
    ] {
        let path = root.join(directory);
        fs::create_dir_all(&path)
            .map_err(|err| format!("创建项目管理目录失败：{}：{err}", path_to_string(&path)))?;
    }
    Ok(())
}

fn classify_file(file_type: &str) -> String {
    match file_type {
        "doc" | "docx" | "pdf" | "txt" | "md" => "需求文档",
        "xls" | "xlsx" | "xlsb" | "ods" | "csv" | "tsv" => "数据表格",
        "png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp" | "tif" | "tiff" => "图片资料",
        "ppt" | "pptx" => "方案演示",
        _ => "待检查",
    }
    .to_string()
}

fn managed_dir_for_category(root: &Path, category: &str) -> PathBuf {
    let folder = match category {
        "需求文档" | "项目资料" => "02_requirements",
        "会议记录" => "09_meetings",
        "方案文档" => "10_design",
        "数据表格" => "03_data",
        "图片资料" => "04_images",
        "方案演示" | "产出文件" => "05_outputs",
        "提示词" => "06_prompts",
        "测试资料" => "07_tests",
        "交付文件" => "08_deliverables",
        "参考资料" => "11_reference",
        "开发文件" => "12_development",
        "待检查" => "99_review",
        _ => "01_originals",
    };
    root.join(".ganmaoyuan/managed").join(folder)
}

fn unique_destination(
    directory: &Path,
    file_name: &str,
    preferred_version: Option<u32>,
) -> Result<PathBuf, String> {
    fs::create_dir_all(directory)
        .map_err(|err| format!("创建分类目录失败：{}：{err}", path_to_string(directory)))?;
    let leaf_name = file_name
        .rsplit(['/', '\\'])
        .find(|value| !value.trim().is_empty())
        .unwrap_or("file");
    let safe_name = inbox_routing::normalize_managed_file_name(leaf_name, None);
    let source = Path::new(&safe_name);
    let stem = source
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("file");
    let extension = source
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("");
    let initial_name = match preferred_version {
        Some(version) if explicit_version_number(stem) == Some(version) => safe_name.clone(),
        Some(version) => versioned_name(stem, extension, version),
        None => safe_name.clone(),
    };
    let initial = directory.join(&initial_name);
    if !initial.starts_with(directory) {
        return Err("Location Service 拒绝目标目录之外的文件名。".to_string());
    }
    if !initial.exists() {
        return Ok(initial);
    }
    for index in 2..10_000 {
        let candidate = directory.join(if extension.is_empty() {
            format!("{stem}-{index}")
        } else {
            format!("{stem}-{index}.{extension}")
        });
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    Err("无法生成不覆盖现有文件的保存路径。".to_string())
}

fn versioned_name(stem: &str, extension: &str, version: u32) -> String {
    if extension.is_empty() {
        format!("{stem}-v{version}")
    } else {
        format!("{stem}-v{version}.{extension}")
    }
}

fn explicit_version_number(stem: &str) -> Option<u32> {
    let lower = stem.to_lowercase();
    for separator in ["-v", "_v", " v", "-版本", "_版本"] {
        if let Some(index) = lower.rfind(separator) {
            let suffix = &lower[index + separator.len()..];
            if !suffix.is_empty() && suffix.chars().all(|ch| ch.is_ascii_digit()) {
                return suffix.parse().ok();
            }
        }
    }
    None
}

fn logical_version_key(file_name: &str) -> String {
    let stem = Path::new(file_name)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or(file_name)
        .to_lowercase();
    let mut value = stem
        .replace("最终版", "")
        .replace("最新版", "")
        .replace("新版本", "")
        .replace("副本", "");
    for separator in ["-v", "_v", " v", "-版本", "_版本"] {
        if let Some(index) = value.rfind(separator) {
            let suffix = &value[index + separator.len()..];
            if !suffix.is_empty() && suffix.chars().all(|ch| ch.is_ascii_digit()) {
                value.truncate(index);
                break;
            }
        }
    }
    value
        .trim_matches(|ch: char| ch == '-' || ch == '_' || ch.is_whitespace())
        .to_string()
}

fn parse_version_number(version: &str) -> u32 {
    version
        .trim_start_matches(|ch: char| !ch.is_ascii_digit())
        .parse()
        .unwrap_or(1)
}

fn decide_file_location(
    root: &Path,
    records: &[FileRecord],
    file_name: &str,
    file_type: &str,
    created_source: &str,
    related_task: &str,
    purpose: &str,
) -> Result<LocationPlan, String> {
    let category = classify_file_for_purpose(file_type, created_source, purpose);
    let logical_key = logical_version_key(file_name);
    let previous = records
        .iter()
        .filter(|item| item.duplicate_of_file_id.is_none())
        .filter(|item| logical_version_key(&item.file_name) == logical_key)
        .max_by_key(|item| parse_version_number(&item.current_version))
        .cloned();
    let version_number = previous
        .as_ref()
        .map(|item| parse_version_number(&item.current_version) + 1)
        .unwrap_or(1);
    let version_group_id = previous
        .as_ref()
        .map(|item| item.version_group_id.clone())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(new_id);
    let destination_dir = managed_dir_for_category(root, &category);
    let destination = unique_destination(
        &destination_dir,
        file_name,
        if version_number > 1 {
            Some(version_number)
        } else {
            None
        },
    )?;
    let relative_path = destination
        .strip_prefix(root)
        .map(path_to_string)
        .unwrap_or_else(|_| path_to_string(&destination));
    let reason = format!(
        "统一位置服务根据来源“{}”、用途“{}”、类型“{}”归入“{}”；关联任务：{}。",
        empty_label(created_source, "未知来源"),
        empty_label(purpose, "补充资料"),
        empty_label(file_type, "unknown"),
        category,
        empty_label(related_task, "未指定")
    );
    Ok(LocationPlan {
        destination,
        category,
        version_number,
        version_group_id,
        previous_version_id: previous.map(|item| item.id),
        relative_path,
        reason,
    })
}

fn classify_file_for_purpose(file_type: &str, created_source: &str, purpose: &str) -> String {
    let source = created_source.to_lowercase();
    let purpose_lower = purpose.to_lowercase();
    let purpose_text = format!("{created_source} {purpose}").to_lowercase();
    if purpose.contains("交付") || purpose.contains("报告") || purpose.contains("验收") {
        return "交付文件".to_string();
    }
    if purpose_text.contains("codex") || purpose_text.contains("test") || purpose.contains("测试")
    {
        return "测试资料".to_string();
    }
    if purpose_text.contains("opendesign") || purpose.contains("设计") || purpose.contains("原型")
    {
        return "方案演示".to_string();
    }
    if purpose.contains("提示词") || purpose_lower.contains("prompt") {
        return "提示词".to_string();
    }
    if source.contains("deepseek")
        || source.contains("ganmaoyuan")
        || created_source.contains("感冒院")
        || purpose.contains("产出")
    {
        return "产出文件".to_string();
    }
    classify_file(file_type)
}

fn empty_label<'a>(value: &'a str, fallback: &'a str) -> &'a str {
    if value.trim().is_empty() {
        fallback
    } else {
        value
    }
}

fn location_decision(
    file: &FileRecord,
    source_path: &str,
    created_source: &str,
    related_task: &str,
    purpose: &str,
    reason: &str,
    requires_confirmation: bool,
) -> crate::models::LocationDecision {
    let preview = LocationPreviewService::preview(
        source_path,
        Path::new(&file.managed_path),
        None,
        "",
        "existingProjectMaterial",
        &file.category,
        reason,
    );
    crate::models::LocationDecision {
        id: new_id(),
        file_name: file.file_name.clone(),
        source_path: source_path.to_string(),
        managed_path: file.managed_path.clone(),
        managed_relative_path: file.managed_relative_path.clone(),
        category: file.category.clone(),
        file_type: file.file_type.clone(),
        created_source: created_source.to_string(),
        related_task: related_task.to_string(),
        purpose: purpose.to_string(),
        version: file.current_version.clone(),
        reason: reason.to_string(),
        requires_confirmation,
        created_at: now_string(),
        operation_id: file.import_transaction_id.clone(),
        location_preview_id: preview.preview_id,
    }
}

fn decide_inbox_file_location(
    root: &Path,
    records: &[FileRecord],
    managed_file_name: &str,
    file_type: &str,
    created_source: &str,
    related_task: &str,
    recommended_category: &str,
) -> Result<LocationPlan, String> {
    let category = controlled_category(recommended_category, file_type);
    let logical_key = logical_version_key(managed_file_name);
    let previous = records
        .iter()
        .filter(|item| item.duplicate_of_file_id.is_none())
        .filter(|item| logical_version_key(&item.file_name) == logical_key)
        .max_by_key(|item| parse_version_number(&item.current_version))
        .cloned();
    let version_number = previous
        .as_ref()
        .map(|item| parse_version_number(&item.current_version) + 1)
        .unwrap_or(1);
    let version_group_id = previous
        .as_ref()
        .map(|item| item.version_group_id.clone())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(new_id);
    let destination_dir = managed_dir_for_category(root, &category);
    let destination = unique_destination(
        &destination_dir,
        managed_file_name,
        if version_number > 1 {
            Some(version_number)
        } else {
            None
        },
    )?;
    if !destination.starts_with(root) {
        return Err("Location Service 拒绝项目根目录之外的目标路径。".to_string());
    }
    let relative_path = destination
        .strip_prefix(root)
        .map(path_to_string)
        .map_err(|_| "Location Service 无法生成项目内相对路径。".to_string())?;
    let reason = format!(
        "Inbox 只提供语义分类“{}”，统一 Location Service 将其映射到项目内受管目录；来源：{}；关联任务：{}。",
        inbox_routing::semantic_location(&category),
        empty_label(created_source, "未知来源"),
        empty_label(related_task, "资料收件箱")
    );
    Ok(LocationPlan {
        destination,
        category: category.clone(),
        version_number,
        version_group_id,
        previous_version_id: previous.map(|item| item.id),
        relative_path,
        reason,
    })
}

fn controlled_category(value: &str, file_type: &str) -> String {
    match value {
        "requirements" | "需求文档" | "项目资料" => "需求文档".to_string(),
        "meetings" | "会议记录" => "会议记录".to_string(),
        "design" | "方案文档" | "方案演示" | "图片资料" => "方案文档".to_string(),
        "test" | "测试资料" => "测试资料".to_string(),
        "reports" | "报告" | "产出文件" => "产出文件".to_string(),
        "data" | "数据表格" => "数据表格".to_string(),
        "reference" | "参考资料" => "参考资料".to_string(),
        "delivery" | "交付文件" => "交付文件".to_string(),
        "development" | "开发文件" | "提示词" => "开发文件".to_string(),
        "other" | "待检查" => "待检查".to_string(),
        _ => classify_file(file_type),
    }
}

fn assess_atlas_read_only(_root: &Path, manifest: &ProjectManifest) -> AtlasAssessment {
    let atlas_root = PathBuf::from(r"D:\Atlas");
    let mut assessment = AtlasAssessment {
        atlas_version: String::new(),
        status: "unavailable".to_string(),
        reusable_parts: Vec::new(),
        uncovered_parts: Vec::new(),
        training_candidates: Vec::new(),
        evidence_references: Vec::new(),
        review_required: true,
        assessed_at: now_string(),
        failure_reason: String::new(),
    };

    if !atlas_root.exists() {
        assessment.failure_reason = "D:\\Atlas 不存在，无法执行只读核验。".to_string();
        assessment
            .uncovered_parts
            .push("Atlas 本地仓库不可用。".to_string());
        return assessment;
    }

    let git_head = run_git_read_only(&atlas_root, ["rev-parse", "--short", "HEAD"]);
    let git_status = run_git_read_only(&atlas_root, ["status", "--short"]);
    let roadmap = fs::read_to_string(atlas_root.join("ROADMAP.md")).unwrap_or_default();
    let registry =
        fs::read_to_string(atlas_root.join("registry/candidate-modules.md")).unwrap_or_default();
    let readme = fs::read_to_string(atlas_root.join("README.md")).unwrap_or_default();
    if roadmap.is_empty() && registry.is_empty() && readme.is_empty() {
        assessment.failure_reason =
            "D:\\Atlas 存在，但 README、ROADMAP 或候选模块注册表不可读。".to_string();
        assessment
            .uncovered_parts
            .push("Atlas 入口文档不可用。".to_string());
        return assessment;
    }

    let version_label = if roadmap.contains("Atlas v2 is now frozen") {
        "Atlas v2 Tabular Baseline"
    } else if readme.contains("Atlas v1 is frozen") {
        "Atlas v1 Candidate Baseline"
    } else {
        "Atlas local repository"
    };
    assessment.atlas_version = match git_head {
        Ok(head) if !head.trim().is_empty() => format!("{version_label} ({})", head.trim()),
        _ => version_label.to_string(),
    };
    assessment.status = match git_status {
        Ok(status) if status.trim().is_empty() => "available_read_only_clean".to_string(),
        Ok(_) => "available_read_only_dirty".to_string(),
        Err(err) => {
            assessment.failure_reason = format!("Atlas Git 状态读取失败：{err}");
            "available_read_only_git_unverified".to_string()
        }
    };
    assessment.evidence_references.extend([
        "D:\\Atlas\\README.md".to_string(),
        "D:\\Atlas\\ROADMAP.md".to_string(),
        "D:\\Atlas\\registry\\candidate-modules.md".to_string(),
    ]);
    if atlas_root
        .join("docs/releases/atlas-v2-tabular-baseline.md")
        .exists()
    {
        assessment
            .evidence_references
            .push("D:\\Atlas\\docs\\releases\\atlas-v2-tabular-baseline.md".to_string());
    }

    let has_tabular = registry.contains("packages/tabular-input");
    let has_file_lifecycle = registry.contains("packages/file-lifecycle");
    let has_runtime_config = registry.contains("packages/runtime-config");
    let has_operation_outcome = registry.contains("Operation Outcome");
    let tabular_files = manifest
        .files
        .iter()
        .filter(|file| {
            matches!(
                file.file_type.as_str(),
                "xlsx" | "xls" | "csv" | "tsv" | "ods"
            )
        })
        .count();
    let managed_files = manifest.files.len();

    if has_tabular && tabular_files > 0 {
        assessment.reusable_parts.push(format!(
            "Tabular Core 可用于 {} 个表格类资料的表头、行列、警告和摘要语义核验。",
            tabular_files
        ));
    }
    if has_file_lifecycle && managed_files > 0 {
        assessment.reusable_parts.push(format!(
            "File Lifecycle 可用于 {} 个受管文件的位置、版本、来源和保留状态表达。",
            managed_files
        ));
    }
    if has_runtime_config {
        assessment.reusable_parts.push(
            "Runtime Config 可借鉴 Key、模型和本地路径配置的有效值与密钥边界表达。".to_string(),
        );
    }
    if has_operation_outcome {
        assessment.reusable_parts.push(
            "Operation Outcome 可借鉴导入、解析、对话、收工等操作结果的状态语义。".to_string(),
        );
    }
    if assessment.reusable_parts.is_empty() {
        assessment
            .uncovered_parts
            .push("当前资料尚未匹配到 Atlas 已登记候选能力。".to_string());
    }

    assessment.uncovered_parts.extend([
        "DeepSeek 项目对话、上下文选择和流式生成不属于当前 Atlas v2 Tabular Core。".to_string(),
        "项目理解、待确认问题和下一步建议仍是感冒院业务层能力，不写回 Atlas。".to_string(),
        "文件自动分类规则、资料摘要质量和人工复核策略仍需要感冒院项目内继续验证。".to_string(),
    ]);
    if manifest
        .files
        .iter()
        .any(|file| file.parse_status == "failed" || file.needs_user_confirmation)
    {
        assessment.training_candidates.push(
            "解析失败、分类不确定和待检查资料可沉淀为 File Lifecycle 与人工复核边界证据。"
                .to_string(),
        );
    }
    if manifest
        .files
        .iter()
        .any(|file| file.duplicate_of_file_id.is_some())
        || manifest
            .files
            .iter()
            .any(|file| file.previous_version_id.is_some())
    {
        assessment
            .training_candidates
            .push("重复文件、版本链和受管副本记录可沉淀为文件生命周期证据。".to_string());
    }
    for next in manifest.project_analysis.next_steps.iter().take(3) {
        assessment
            .training_candidates
            .push(format!("项目下一步可作为 Atlas Discovery 观察点：{next}"));
    }
    assessment.training_candidates.sort();
    assessment.training_candidates.dedup();
    assessment
}

fn run_git_read_only<const N: usize>(root: &Path, args: [&str; N]) -> Result<String, String> {
    let output = hidden_command("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .map_err(|err| format!("执行 git 只读命令失败：{err}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn collect_project_search_results(
    manifest: &ProjectManifest,
    normalized_query: &str,
    results: &mut Vec<GlobalSearchResult>,
) {
    let project = &manifest.project;
    try_push_search_result(
        results,
        normalized_query,
        &project.id,
        &project.id,
        &project.root_dir,
        &project.name,
        "project",
        &project.name,
        &join_search_fields(&[&project.description, &project.next_step]),
        &project.last_opened_at,
        "",
    );

    for file in &manifest.files {
        let projection = FileProjectionAdapter::from_project_file(file, &project.id);
        push_file_projection_search_result(
            results,
            normalized_query,
            search_result_from_file_projection(
                &projection,
                project.id.clone(),
                project.root_dir.clone(),
                project.name.clone(),
                "file",
                "",
                String::new(),
            ),
        );
    }

    for message in &manifest.messages {
        let title = if message.author == "user" {
            "用户对话"
        } else {
            "工作台回复"
        };
        try_push_search_result(
            results,
            normalized_query,
            &format!("message:{}", message.id),
            &project.id,
            &project.root_dir,
            &project.name,
            "message",
            title,
            &message.text,
            &message.created_at,
            "",
        );
    }

    for task in &manifest.tasks {
        try_push_search_result(
            results,
            normalized_query,
            &format!("task:{}", task.id),
            &project.id,
            &project.root_dir,
            &project.name,
            "task",
            &task.title,
            &format!("状态：{}", task.status),
            &task.updated_at,
            "",
        );
    }

    for decision in &manifest.decisions {
        try_push_search_result(
            results,
            normalized_query,
            &format!("decision:{}", decision.id),
            &project.id,
            &project.root_dir,
            &project.name,
            "decision",
            "项目决定",
            &decision.summary,
            &decision.created_at,
            "",
        );
    }

    for artifact in &manifest.artifacts {
        try_push_search_result(
            results,
            normalized_query,
            &format!("artifact:{}", artifact.id),
            &project.id,
            &project.root_dir,
            &project.name,
            "artifact",
            &artifact.title,
            &join_search_fields(&[
                &artifact.artifact_type,
                &artifact.related_task,
                &artifact.source,
            ]),
            &artifact.created_at,
            &artifact.managed_path,
        );
    }

    if !manifest.project_analysis.status.is_empty() {
        try_push_search_result(
            results,
            normalized_query,
            &format!("analysis:{}", project.id),
            &project.id,
            &project.root_dir,
            &project.name,
            "projectAnalysis",
            "项目理解",
            &join_search_fields(&[
                &manifest.project_analysis.project_definition,
                &manifest.project_analysis.known_requirements.join("；"),
                &manifest.project_analysis.questions.join("；"),
                &manifest.project_analysis.next_steps.join("；"),
            ]),
            &manifest.project_analysis.updated_at,
            "",
        );
    }

    if !manifest.atlas.status.is_empty() {
        try_push_search_result(
            results,
            normalized_query,
            &format!("atlas:{}", project.id),
            &project.id,
            &project.root_dir,
            &project.name,
            "atlas",
            "Atlas 评估",
            &join_search_fields(&[
                &manifest.atlas.status,
                &manifest.atlas.reusable_parts.join("；"),
                &manifest.atlas.uncovered_parts.join("；"),
                &manifest.atlas.training_candidates.join("；"),
                &manifest.atlas.evidence_references.join("；"),
            ]),
            &manifest.atlas.assessed_at,
            "",
        );
    }

    for report in &manifest.codex_reports {
        try_push_search_result(
            results,
            normalized_query,
            &format!("codex:{}", report.id),
            &project.id,
            &project.root_dir,
            &project.name,
            "codexReport",
            if report.source_label.is_empty() {
                "Codex 报告"
            } else {
                &report.source_label
            },
            &join_search_fields(&[
                &report.summary,
                &report.commit,
                &report.completed_status,
                &report.modified_content.join("；"),
                &report.test_results.join("；"),
                &report.unresolved_issues.join("；"),
                &report.next_step_suggestions.join("；"),
            ]),
            &report.created_at,
            &report.evidence_managed_path,
        );
    }
}

fn read_material_inbox_document<R: Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<MaterialInboxDocument, String> {
    let path = material_inbox_path(app)?;
    if !path.exists() {
        return Ok(MaterialInboxDocument {
            schema_version: MATERIAL_INBOX_SCHEMA_VERSION,
            items: Vec::new(),
            updated_at: now_string(),
        });
    }
    let mut document: MaterialInboxDocument = read_json(&path)?;
    let mut changed = document.schema_version != MATERIAL_INBOX_SCHEMA_VERSION;
    document.schema_version = MATERIAL_INBOX_SCHEMA_VERSION;
    for item in &mut document.items {
        if item.updated_at.is_empty() {
            item.updated_at = item.created_at.clone();
            changed = true;
        }
        changed |= inbox_routing::migrate_legacy_item(item);
    }
    if changed {
        write_json_atomic(&path, &document)?;
    }
    Ok(document)
}

fn write_material_inbox_document<R: Runtime>(
    app: &tauri::AppHandle<R>,
    document: &MaterialInboxDocument,
) -> Result<(), String> {
    write_json_atomic(&material_inbox_path(app)?, document)
}

fn upsert_material_inbox_item(items: &mut Vec<MaterialInboxItem>, next: MaterialInboxItem) {
    if let Some(existing) = items.iter_mut().find(|item| item.id == next.id) {
        *existing = next;
    } else {
        items.push(next);
    }
}

fn update_material_inbox_item(
    items: &mut [MaterialInboxItem],
    item_id: &str,
    update: impl FnOnce(&mut MaterialInboxItem),
) -> Result<MaterialInboxItem, String> {
    let item = items
        .iter_mut()
        .find(|entry| entry.id == item_id)
        .ok_or_else(|| "收件箱记录不存在。".to_string())?;
    update(item);
    Ok(item.clone())
}

fn mutate_inbox_item<R: Runtime>(
    app: &tauri::AppHandle<R>,
    item_id: &str,
    update: impl FnOnce(&mut MaterialInboxItem) -> Result<(), String>,
) -> Result<MaterialInboxItem, String> {
    let _guard = project_write_lock()
        .lock()
        .map_err(|_| "全局存储锁已损坏，请重启感冒院。".to_string())?;
    let mut document = read_material_inbox_document(app)?;
    let item = document
        .items
        .iter_mut()
        .find(|entry| entry.id == item_id)
        .ok_or_else(|| "收件箱记录不存在。".to_string())?;
    let original = item.clone();
    if let Err(error) = update(item) {
        let rejected_transition = item
            .status_history
            .last()
            .filter(|transition| transition.error_code == "invalidTransition")
            .cloned();
        *item = original;
        if let Some(transition) = rejected_transition {
            item.updated_at = transition.changed_at.clone();
            item.last_transition_at = transition.changed_at.clone();
            item.status_history.push(transition);
            document.updated_at = now_string();
            write_material_inbox_document(app, &document)?;
        }
        return Err(error);
    }
    let result = item.clone();
    document.updated_at = now_string();
    write_material_inbox_document(app, &document)?;
    Ok(result)
}

fn replace_inbox_item<R: Runtime>(
    app: &tauri::AppHandle<R>,
    item: MaterialInboxItem,
) -> Result<MaterialInboxItem, String> {
    let _guard = project_write_lock()
        .lock()
        .map_err(|_| "全局存储锁已损坏，请重启感冒院。".to_string())?;
    let mut document = read_material_inbox_document(app)?;
    upsert_material_inbox_item(&mut document.items, item.clone());
    document.updated_at = now_string();
    write_material_inbox_document(app, &document)?;
    Ok(item)
}

fn route_material_inbox_item<R: Runtime>(
    app: &tauri::AppHandle<R>,
    item_id: &str,
    project_root: &str,
    auto_routed: bool,
) -> Result<MaterialInboxRouteResult, String> {
    let mut item = list_material_inbox(app)?
        .into_iter()
        .find(|entry| entry.id == item_id)
        .ok_or_else(|| "收件箱记录不存在。".to_string())?;
    if item.source_path.trim().is_empty() {
        return Err("收件箱记录缺少原始文件路径。".to_string());
    }
    if !Path::new(&item.source_path).is_file() {
        let error = "原文件不存在，无法归位；Inbox 记录已保留。";
        mutate_inbox_item(app, item_id, |entry| {
            if entry.processing_status == inbox_routing::PENDING_REVIEW {
                inbox_routing::transition(
                    entry,
                    inbox_routing::READY_TO_ROUTE,
                    "用户确认归位。",
                    "user",
                    None,
                )?;
            }
            entry.failed_stage = "routing".to_string();
            inbox_routing::transition(
                entry,
                inbox_routing::FAILED,
                "归位前原文件检查失败。",
                "system",
                Some(("sourceMissing", error)),
            )
        })?;
        return Err(error.to_string());
    }
    if item.processing_status == inbox_routing::ROUTED {
        let files = load_project(project_root)?
            .files
            .into_iter()
            .filter(|file| file.id == item.managed_file_id)
            .collect();
        return Ok(MaterialInboxRouteResult {
            project: Some(load_project(project_root)?.project),
            item,
            files,
        });
    }
    if item.processing_status == inbox_routing::PENDING_REVIEW {
        item = mutate_inbox_item(app, item_id, |entry| {
            inbox_routing::transition(
                entry,
                inbox_routing::READY_TO_ROUTE,
                "用户确认处理建议。",
                "user",
                None,
            )
        })?;
    }
    if item.processing_status != inbox_routing::READY_TO_ROUTE {
        return Err(format!(
            "当前 Inbox 状态不能归位：{}",
            item.processing_status
        ));
    }
    let project_manifest = match load_project(project_root) {
        Ok(manifest) => manifest,
        Err(err) => {
            mutate_inbox_item(app, item_id, |entry| {
                entry.failed_stage = "projectLookup".to_string();
                inbox_routing::transition(
                    entry,
                    inbox_routing::FAILED,
                    "归位目标项目不存在或不可读取。",
                    "system",
                    Some(("projectUnavailable", &err)),
                )
            })?;
            return Err(err);
        }
    };
    let operation_id = item
        .route_operation
        .as_ref()
        .filter(|operation| operation.status != "undone")
        .map(|operation| operation.id.clone())
        .unwrap_or_else(new_id);
    mutate_inbox_item(app, item_id, |entry| {
        entry.target_project_id = project_manifest.project.id.clone();
        entry.target_project_root = project_manifest.project.root_dir.clone();
        entry.target_project_name = project_manifest.project.name.clone();
        entry.route_operation = Some(InboxRouteOperation {
            id: operation_id.clone(),
            project_id: project_manifest.project.id.clone(),
            project_root: project_manifest.project.root_dir.clone(),
            source_path: entry.source_path.clone(),
            source_hash: entry.source_hash.clone(),
            created_at: now_string(),
            status: "routing".to_string(),
            ..InboxRouteOperation::default()
        });
        inbox_routing::transition(
            entry,
            inbox_routing::ROUTING,
            "开始执行受管副本归位事务。",
            if auto_routed { "system" } else { "user" },
            None,
        )
    })?;

    let existing_for_operation = project_manifest
        .files
        .iter()
        .find(|file| file.import_transaction_id == operation_id)
        .cloned();
    let import_result = if let Some(existing) = existing_for_operation.clone() {
        ImportResult {
            files: vec![existing],
            messages: Vec::new(),
            atlas: project_manifest.atlas.clone(),
            duplicates: Vec::new(),
        }
    } else {
        let route_override = InboxRouteOverride {
            category: if item.suggested_category.is_empty() {
                item.recommended_category.clone()
            } else {
                item.suggested_category.clone()
            },
            managed_file_name: if item.suggested_file_name.is_empty() {
                inbox_routing::normalize_managed_file_name(
                    &item.file_name,
                    item.main_fields_or_sections.first().map(String::as_str),
                )
            } else {
                item.suggested_file_name.clone()
            },
            operation_id: operation_id.clone(),
        };
        match import_files_internal(
            project_root.to_string(),
            vec![item.source_path.clone()],
            "资料收件箱".to_string(),
            Some(&route_override),
        ) {
            Ok(result) => result,
            Err(err) => {
                mutate_inbox_item(app, item_id, |entry| {
                    sync_inbox_decision_traces(entry);
                    update_trace_outcome(
                        &mut entry.decision_traces,
                        &["projectMatch", "locationRecommendation", "autoRoute"],
                        "pending",
                        "failed",
                        &err,
                    );
                    entry.failed_stage = "routing".to_string();
                    entry.result_note = err.clone();
                    if let Some(operation) = &mut entry.route_operation {
                        operation.status = "failed".to_string();
                        operation.failure_reason = err.clone();
                    }
                    inbox_routing::transition(
                        entry,
                        inbox_routing::FAILED,
                        "归位事务失败，可安全重试。",
                        "system",
                        Some(("routeFailed", &err)),
                    )
                })?;
                return Err(err);
            }
        }
    };
    let matched_file = import_result
        .files
        .iter()
        .find(|file| file.import_transaction_id == operation_id)
        .cloned()
        .or_else(|| import_result.files.first().cloned())
        .ok_or_else(|| "归位完成但未找到对应文件登记。".to_string())?;
    let project = load_project(project_root)?.project;
    let target_hash = if Path::new(&matched_file.managed_path).is_file() {
        hash_file(Path::new(&matched_file.managed_path))
            .unwrap_or_else(|_| matched_file.content_hash.clone())
    } else {
        matched_file.content_hash.clone()
    };
    let operation = InboxRouteOperation {
        id: operation_id,
        project_id: project.id.clone(),
        project_root: project.root_dir.clone(),
        source_path: item.source_path.clone(),
        target_path: matched_file.managed_path.clone(),
        managed_file_id: matched_file.id.clone(),
        source_hash: item.source_hash.clone(),
        target_hash_at_route: target_hash,
        created_managed_copy: matched_file.duplicate_of_file_id.is_none(),
        created_at: now_string(),
        status: "committed".to_string(),
        ..InboxRouteOperation::default()
    };
    let updated = mutate_inbox_item(app, item_id, |entry| {
        entry.target_project_root = project.root_dir.clone();
        entry.target_project_id = project.id.clone();
        entry.target_project_name = project.name.clone();
        entry.managed_file_id = matched_file.id.clone();
        entry.suggested_managed_path = matched_file.managed_path.clone();
        entry.suggested_category = matched_file.category.clone();
        entry.recommended_location = matched_file.managed_path.clone();
        entry.recommended_relative_location =
            inbox_routing::semantic_location(&matched_file.category).to_string();
        entry.result_note = format!(
            "{}整理到 {} / {}。",
            if auto_routed { "已自动" } else { "已" },
            project.name,
            matched_file.managed_relative_path
        );
        entry.duplicate_of_file_id = matched_file
            .duplicate_of_file_id
            .clone()
            .unwrap_or_default();
        entry.previous_version_file_id =
            matched_file.previous_version_id.clone().unwrap_or_default();
        entry.document_family_id = matched_file.version_group_id.clone();
        entry.version_id = matched_file.version_id.clone();
        entry.version_number = matched_file
            .version_number
            .max(parse_version_number(&matched_file.current_version));
        entry.judgement_status = if matched_file.duplicate_of_file_id.is_some() {
            "duplicate".to_string()
        } else if matched_file.previous_version_id.is_some() {
            "new_version".to_string()
        } else {
            "matched_project".to_string()
        };
        entry.route_operation = Some(operation.clone());
        entry.auto_routed = auto_routed;
        entry.can_undo = true;
        entry.failed_stage.clear();
        sync_inbox_decision_traces(entry);
        if auto_routed {
            let mut trace = new_decision_trace(
                "autoRoute",
                &entry.id,
                &project.id,
                inbox_trace_evidence(entry),
                "高置信判断满足现有自动归位规则。".to_string(),
                format!("自动归位到 {}", matched_file.managed_relative_path),
                trace_confidence(entry.confidence_score),
            );
            trace.execution = "executed".to_string();
            trace.execution_note = "自动归位已完成。".to_string();
            upsert_pending_trace(&mut entry.decision_traces, trace);
        } else {
            update_trace_outcome(
                &mut entry.decision_traces,
                &[
                    "fileClassification",
                    "ownershipDecision",
                    "projectMatch",
                    "locationRecommendation",
                ],
                "approved",
                "executed",
                "用户确认并完成归位。",
            );
        }
        inbox_routing::transition(
            entry,
            inbox_routing::ROUTED,
            "受管副本、manifest、分析和搜索数据已提交。",
            if auto_routed { "system" } else { "user" },
            None,
        )
    })?;
    let _ = analyze_routed_file_impact(project_root, &matched_file);
    Ok(MaterialInboxRouteResult {
        item: updated,
        project: Some(project),
        files: vec![matched_file],
    })
}

fn analyze_routed_file_impact(project_root: &str, file: &FileRecord) -> Result<(), String> {
    let root = PathBuf::from(project_root);
    let _guard = project_write_lock()
        .lock()
        .map_err(|_| "项目存储锁已损坏，请重启感冒院。".to_string())?;
    let mut manifest = read_and_repair_manifest(&root)?;
    if manifest.project_impact_analyses.iter().any(|item| {
        item.source_file_id == file.id
            && item.source_hash == file.content_hash
            && item.analysis_version == "v0.1-local"
    }) {
        update_daily_continue_snapshot(&mut manifest);
        persist_project(&root, &manifest, None)?;
        return Ok(());
    }

    let impact = build_local_project_impact(&manifest, file);
    let candidates = build_action_candidates_from_impact(&impact);
    let proposal = build_project_state_proposal(&manifest, file, &impact);
    upsert_pending_reviews_for_impact(&mut manifest, &impact, &candidates, &proposal);
    manifest.project_impact_analyses.push(impact);
    for candidate in candidates {
        if !manifest.project_action_candidates.iter().any(|item| {
            item.source_hash == candidate.source_hash
                && item.candidate_type == candidate.candidate_type
                && normalize_key(&item.title) == normalize_key(&candidate.title)
        }) {
            manifest.project_action_candidates.push(candidate);
        }
    }
    if !manifest
        .project_state_proposals
        .iter()
        .any(|item| item.source_hashes.contains(&file.content_hash) && item.status != "rejected")
    {
        manifest.project_state_proposals.push(proposal);
    }
    update_daily_continue_snapshot(&mut manifest);
    manifest.audit.push(audit_event(
        "project.impact.analysis",
        &file.id,
        "needsReview",
        format!("已基于受管文件摘要生成项目影响分析：{}", file.file_name),
        false,
        true,
    ));
    persist_project(&root, &manifest, None)?;
    append_audit_event(&root, manifest.audit.last().expect("audit event exists"))?;
    Ok(())
}

fn build_local_project_impact(
    manifest: &ProjectManifest,
    file: &FileRecord,
) -> ProjectImpactAnalysis {
    let evidence = project_impact_evidence(file);
    let text = format!(
        "{}\n{}\n{}",
        file.file_name,
        file.content_summary,
        file.main_fields_or_sections.join("；")
    );
    let mut findings = Vec::new();
    if contains_any(&text, &["需求", "要求", "必须", "需要", "PRD"]) {
        findings.push(project_impact_finding(
            "requirement",
            "可能包含项目需求",
            &format!("{} 的摘要或章节包含需求信号。", file.file_name),
            "核对是否需要加入项目需求或待办。",
            &evidence,
            0.72,
            "inference",
        ));
    }
    if contains_any(&text, &["变更", "调整", "修改", "新增", "删除", "冲突"]) {
        findings.push(project_impact_finding(
            "requirementChange",
            "可能包含需求变化",
            &format!("{} 出现新增、调整或冲突类信号。", file.file_name),
            "人工确认后再更新需求或项目状态。",
            &evidence,
            0.68,
            "toConfirm",
        ));
    }
    if contains_any(&text, &["任务", "待办", "负责", "完成", "跟进", "整改"]) {
        findings.push(project_impact_finding(
            "taskCandidate",
            "可能产生新任务",
            &format!("{} 中出现任务或跟进动作。", file.file_name),
            "确认后加入任务列表。",
            &evidence,
            0.7,
            "inference",
        ));
    }
    if contains_any(&text, &["决定", "确认", "最终", "结论", "同意"]) {
        findings.push(project_impact_finding(
            "decisionCandidate",
            "可能包含明确决定",
            &format!("{} 中出现决定或确认类表述。", file.file_name),
            "确认后加入项目决定。",
            &evidence,
            0.74,
            "inference",
        ));
    }
    if contains_any(&text, &["风险", "问题", "失败", "异常", "阻塞", "未通过"]) {
        findings.push(project_impact_finding(
            if contains_any(&text, &["阻塞", "无法", "不能"]) {
                "blocker"
            } else {
                "risk"
            },
            "可能包含风险或阻塞",
            &format!("{} 中出现风险、失败或阻塞信号。", file.file_name),
            "进入待确认，必要时转为阻塞或修复任务。",
            &evidence,
            0.76,
            "inference",
        ));
    }
    if contains_any(&text, &["通过", "完成", "验收", "交付", "结果"]) {
        findings.push(project_impact_finding(
            "outcome",
            "可能包含阶段成果",
            &format!("{} 中出现完成、验收或交付信号。", file.file_name),
            "确认后收录为成果或恢复点依据。",
            &evidence,
            0.67,
            "inference",
        ));
    }
    if contains_any(
        &text,
        &[
            "里程碑",
            "阶段目标",
            "版本发布",
            "上线节点",
            "交付节点",
            "关键节点",
        ],
    ) {
        findings.push(project_impact_finding(
            "milestone",
            "可能包含里程碑或阶段目标",
            &format!("{} 中出现里程碑或阶段目标信号。", file.file_name),
            "确认后纳入项目里程碑跟踪。",
            &evidence,
            0.66,
            "toConfirm",
        ));
    }
    if contains_any(
        &text,
        &[
            "存疑",
            "待澄清",
            "疑问点",
            "需要确认的问题",
            "未定论",
            "待定",
        ],
    ) {
        findings.push(project_impact_finding(
            "question",
            "可能存在待澄清问题",
            &format!("{} 中出现待澄清或存疑信号。", file.file_name),
            "进入待确认，补充资料或询问后再处理。",
            &evidence,
            0.64,
            "toConfirm",
        ));
    }
    if findings.is_empty() {
        findings.push(project_impact_finding(
            "information",
            "参考资料已归位",
            &format!(
                "{} 已完成解析和位置登记，当前未识别出明确任务或决定。",
                file.file_name
            ),
            "作为项目资料保留，暂不生成任务。",
            &evidence,
            0.62,
            "inference",
        ));
    }
    for finding in &mut findings {
        finding.decision_trace.project_id = manifest.project.id.clone();
    }
    let impact_level = if findings
        .iter()
        .any(|item| matches!(item.finding_type.as_str(), "blocker" | "requirementChange"))
    {
        "high"
    } else if findings.iter().any(|item| {
        matches!(
            item.finding_type.as_str(),
            "requirement" | "taskCandidate" | "decisionCandidate" | "risk"
        )
    }) {
        "medium"
    } else {
        "low"
    };
    let id = new_id();
    let summary = format!(
        "{}：{}",
        file.file_name,
        first_line(&file.content_summary, 180)
    );
    let decision_trace = new_decision_trace(
        "projectImpact",
        &id,
        &manifest.project.id,
        trace_evidence_from_impact(&evidence),
        summary.clone(),
        format!("项目影响等级：{impact_level}，需人工复核。"),
        trace_confidence_from_ratio(0.7),
    );
    ProjectImpactAnalysis {
        id,
        source_file_id: file.id.clone(),
        source_hash: file.content_hash.clone(),
        project_id: manifest.project.id.clone(),
        generated_at: now_string(),
        analysis_version: "v0.1-local".to_string(),
        summary,
        relevance: if file.category == "other" {
            "uncertain"
        } else {
            "projectRelated"
        }
        .to_string(),
        impact_level: impact_level.to_string(),
        findings,
        evidence,
        confidence: 0.7,
        status: "needsReview".to_string(),
        failure_reason: String::new(),
        decision_trace,
    }
}

fn project_impact_evidence(file: &FileRecord) -> Vec<ProjectImpactEvidence> {
    vec![ProjectImpactEvidence {
        basis_kind: "fact".to_string(),
        file_id: file.id.clone(),
        file_name: file.file_name.clone(),
        section: file
            .main_fields_or_sections
            .first()
            .cloned()
            .unwrap_or_default(),
        excerpt: if file.content_summary.trim().is_empty() {
            "无可提取摘要，证据粒度为文件级。".to_string()
        } else {
            first_line(&file.content_summary, 220)
        },
        source_hash: file.content_hash.clone(),
        granularity: if file.main_fields_or_sections.is_empty() {
            "file".to_string()
        } else {
            "section".to_string()
        },
    }]
}

fn trace_evidence_from_impact(evidence: &[ProjectImpactEvidence]) -> Vec<DecisionTraceEvidence> {
    evidence
        .iter()
        .map(|item| DecisionTraceEvidence {
            kind: if item.basis_kind.is_empty() {
                "sourceFile".to_string()
            } else {
                item.basis_kind.clone()
            },
            label: if item.section.is_empty() {
                item.file_name.clone()
            } else {
                format!("{} · {}", item.file_name, item.section)
            },
            summary: first_line(&item.excerpt, 240),
            source_id: item.file_id.clone(),
        })
        .collect()
}

fn project_impact_finding(
    finding_type: &str,
    title: &str,
    description: &str,
    suggested_action: &str,
    evidence: &[ProjectImpactEvidence],
    confidence: f32,
    basis_kind: &str,
) -> ProjectImpactFinding {
    let id = new_id();
    let trace_type = match finding_type {
        "taskCandidate" => "taskCandidate",
        "decisionCandidate" => "decisionCandidate",
        "risk" | "blocker" => "riskCandidate",
        _ => "projectImpact",
    };
    ProjectImpactFinding {
        id: id.clone(),
        finding_type: finding_type.to_string(),
        title: title.to_string(),
        description: description.to_string(),
        confidence,
        evidence: evidence.to_vec(),
        source_reference: evidence
            .first()
            .map(|item| format!("{} / {}", item.file_name, item.section))
            .unwrap_or_else(|| "文件级证据".to_string()),
        suggested_action: suggested_action.to_string(),
        basis_kind: basis_kind.to_string(),
        review_required: true,
        status: "pending".to_string(),
        decision_trace: new_decision_trace(
            trace_type,
            &id,
            "",
            trace_evidence_from_impact(evidence),
            description.to_string(),
            suggested_action.to_string(),
            trace_confidence_from_ratio(confidence),
        ),
    }
}

fn build_action_candidates_from_impact(
    impact: &ProjectImpactAnalysis,
) -> Vec<ProjectActionCandidate> {
    impact
        .findings
        .iter()
        .filter_map(|finding| {
            let candidate_type = match finding.finding_type.as_str() {
                "taskCandidate" => "task",
                "decisionCandidate" => "decision",
                "risk" => "risk",
                "blocker" => "blocker",
                "requirement" | "requirementChange" => "requirementChange",
                _ => return None,
            };
            let id = new_id();
            let trace_type = match candidate_type {
                "task" => "taskCandidate",
                "decision" => "decisionCandidate",
                "risk" | "blocker" => "riskCandidate",
                _ => "projectImpact",
            };
            Some(ProjectActionCandidate {
                id: id.clone(),
                candidate_type: candidate_type.to_string(),
                title: finding.title.clone(),
                description: finding.description.clone(),
                suggested_priority: if matches!(candidate_type, "blocker" | "risk") {
                    "high".to_string()
                } else {
                    "normal".to_string()
                },
                suggested_due_date: String::new(),
                related_file_id: impact.source_file_id.clone(),
                evidence: finding.evidence.clone(),
                confidence: finding.confidence,
                review_required: true,
                status: "pending".to_string(),
                source_impact_id: impact.id.clone(),
                source_hash: impact.source_hash.clone(),
                created_at: now_string(),
                updated_at: now_string(),
                applied_record_id: String::new(),
                decision_trace: new_decision_trace(
                    trace_type,
                    &id,
                    &impact.project_id,
                    trace_evidence_from_impact(&finding.evidence),
                    finding.description.clone(),
                    finding.suggested_action.clone(),
                    trace_confidence_from_ratio(finding.confidence),
                ),
            })
        })
        .collect()
}

fn build_project_state_proposal(
    manifest: &ProjectManifest,
    file: &FileRecord,
    impact: &ProjectImpactAnalysis,
) -> ProjectStateProposal {
    let next_step = impact
        .findings
        .iter()
        .find(|finding| finding.finding_type != "information")
        .map(|finding| finding.suggested_action.clone())
        .unwrap_or_else(|| manifest.project.next_step.clone());
    let before = vec![ProjectStateChange {
        field: "nextStep".to_string(),
        before: manifest.project.next_step.clone(),
        after: manifest.project.next_step.clone(),
        reason: "当前项目恢复点中的下一步。".to_string(),
    }];
    let proposed_changes = if next_step.trim().is_empty() {
        Vec::new()
    } else {
        vec![ProjectStateChange {
            field: "nextStep".to_string(),
            before: manifest.project.next_step.clone(),
            after: next_step,
            reason: format!("来自文件 {} 的影响分析提案。", file.file_name),
        }]
    };
    let id = new_id();
    let recommendation = proposed_changes
        .iter()
        .map(|change| format!("{}：{} -> {}", change.field, change.before, change.after))
        .collect::<Vec<_>>()
        .join("；");
    ProjectStateProposal {
        id: id.clone(),
        before,
        proposed_changes,
        evidence: impact.evidence.clone(),
        confidence: impact.confidence,
        source_file_ids: vec![file.id.clone()],
        source_hashes: vec![file.content_hash.clone()],
        created_at: now_string(),
        applied_at: String::new(),
        status: "pending".to_string(),
        previous_next_step: manifest.project.next_step.clone(),
        decision_trace: new_decision_trace(
            "projectStateProposal",
            &id,
            &manifest.project.id,
            trace_evidence_from_impact(&impact.evidence),
            "根据项目影响分析形成状态更新提案。".to_string(),
            recommendation,
            trace_confidence_from_ratio(impact.confidence),
        ),
    }
}

fn ensure_action_candidate_trace(candidate: &mut ProjectActionCandidate, project_id: &str) {
    if !candidate.decision_trace.id.is_empty() {
        return;
    }
    let trace_type = match candidate.candidate_type.as_str() {
        "task" => "taskCandidate",
        "decision" => "decisionCandidate",
        "risk" | "blocker" => "riskCandidate",
        _ => "projectImpact",
    };
    candidate.decision_trace = new_decision_trace(
        trace_type,
        &candidate.id,
        project_id,
        trace_evidence_from_impact(&candidate.evidence),
        candidate.description.clone(),
        candidate.title.clone(),
        trace_confidence_from_ratio(candidate.confidence),
    );
}

fn ensure_state_proposal_trace(proposal: &mut ProjectStateProposal, project_id: &str) {
    if !proposal.decision_trace.id.is_empty() {
        return;
    }
    proposal.decision_trace = new_decision_trace(
        "projectStateProposal",
        &proposal.id,
        project_id,
        trace_evidence_from_impact(&proposal.evidence),
        "根据项目影响分析形成状态更新提案。".to_string(),
        proposal
            .proposed_changes
            .iter()
            .map(|change| format!("{}：{} -> {}", change.field, change.before, change.after))
            .collect::<Vec<_>>()
            .join("；"),
        trace_confidence_from_ratio(proposal.confidence),
    );
}

fn upsert_pending_reviews_for_impact(
    manifest: &mut ProjectManifest,
    _impact: &ProjectImpactAnalysis,
    candidates: &[ProjectActionCandidate],
    _proposal: &ProjectStateProposal,
) {
    for candidate in candidates {
        let key = format!(
            "impact-candidate:{}:{}",
            candidate.source_hash, candidate.candidate_type
        );
        if manifest.pending_reviews.iter().any(|item| item.key == key) {
            continue;
        }
        manifest.pending_reviews.push(PendingReviewItem {
            id: new_id(),
            key,
            kind: format!("projectImpact.{}", candidate.candidate_type),
            title: candidate.title.clone(),
            detail: candidate.description.clone(),
            file_id: candidate.related_file_id.clone(),
            path: String::new(),
            suggested_managed_path: String::new(),
            suggested_category: candidate.candidate_type.clone(),
            status: "pending".to_string(),
            detected_at: now_string(),
            updated_at: now_string(),
        });
    }
}

fn update_daily_continue_snapshot(manifest: &mut ProjectManifest) {
    clear_non_user_next_step(manifest);
    update_data_health_records(manifest);
    update_project_state_summary(manifest);
    update_project_attentions(manifest);
    update_work_pattern_candidates(manifest);
    let pending_confirmations = manifest
        .pending_reviews
        .iter()
        .filter(|item| item.status != "resolved" && item.status != "ignored")
        .map(|item| item.title.clone())
        .take(8)
        .collect::<Vec<_>>();
    // Repository scan findings are derived prompts, not durable user intent.
    // Once all reviews are cleared, keep their audit history but stop carrying
    // an obsolete "new file" prompt into the next Continue Work snapshot.
    if pending_confirmations.is_empty()
        && (manifest.project.next_step.contains("待确认事项")
            || manifest.project.next_step.contains("发现新文件"))
    {
        manifest.project.next_step.clear();
    }
    let blockers = manifest
        .project_action_candidates
        .iter()
        .filter(|item| {
            item.status == "pending" && matches!(item.candidate_type.as_str(), "blocker" | "risk")
        })
        .map(|item| item.title.clone())
        .take(6)
        .collect::<Vec<_>>();
    let mut recommended_actions = manifest
        .tasks
        .iter()
        .filter(|task| {
            !matches!(
                task.status.as_str(),
                "done" | "completed" | "closed" | "resolved"
            ) && !unhealthy_source(manifest, "task", &task.id)
                && task_is_confirmed_project_task(manifest, task)
        })
        .rev()
        .take(3)
        .map(|task| {
            let evidence = vec![task.source_message_id.clone()];
            DailyContinueItem {
                action: task.title.clone(),
                reason: "来自未完成任务。".to_string(),
                evidence: evidence.clone(),
                related_project: manifest.project.name.clone(),
                related_files: Vec::new(),
                priority: "normal".to_string(),
                item_type: "fact".to_string(),
                source: format!("task:{}", task.id),
                confidence: trace_confidence(92),
                category: "task".to_string(),
                decision_trace: new_decision_trace(
                    "taskCandidate",
                    &task.id,
                    &manifest.project.id,
                    vec![trace_evidence("task", "未完成任务", &task.title, &task.id)],
                    "该任务仍未完成。".to_string(),
                    format!("今天继续：{}", task.title),
                    trace_confidence(92),
                ),
            }
        })
        .collect::<Vec<_>>();
    if recommended_actions.is_empty() {
        recommended_actions.extend(
            manifest
                .project_action_candidates
                .iter()
                .filter(|item| item.status == "pending")
                .rev()
                .take(3)
                .map(|item| DailyContinueItem {
                    action: item.title.clone(),
                    reason: "来自最近文件影响分析候选。".to_string(),
                    evidence: item
                        .evidence
                        .iter()
                        .map(|evidence| evidence.file_name.clone())
                        .collect(),
                    related_project: manifest.project.name.clone(),
                    related_files: vec![item.related_file_id.clone()],
                    priority: item.suggested_priority.clone(),
                    item_type: "suggestion".to_string(),
                    source: format!("projectImpact:{}", item.id),
                    confidence: trace_confidence_from_ratio(item.confidence),
                    category: "projectImpact".to_string(),
                    decision_trace: item.decision_trace.clone(),
                }),
        );
    }
    let healthy_recovery = manifest
        .recovery_points
        .iter()
        .rev()
        .find(|point| !unhealthy_source(manifest, "recoveryPoint", &point.id));
    if recommended_actions.is_empty()
        && healthy_recovery.is_some()
        && !manifest.project.next_step.trim().is_empty()
    {
        recommended_actions.push(DailyContinueItem {
            action: manifest.project.next_step.clone(),
            reason: "来自上次恢复点。".to_string(),
            evidence: healthy_recovery
                .map(|point| vec![point.id.clone()])
                .unwrap_or_default(),
            related_project: manifest.project.name.clone(),
            related_files: Vec::new(),
            priority: "normal".to_string(),
            item_type: "suggestion".to_string(),
            source: healthy_recovery
                .map(|point| format!("recoveryPoint:{}", point.id))
                .unwrap_or_default(),
            confidence: trace_confidence(88),
            category: "recovery".to_string(),
            decision_trace: new_decision_trace(
                "taskCandidate",
                healthy_recovery
                    .map(|point| point.id.as_str())
                    .unwrap_or(&manifest.project.id),
                &manifest.project.id,
                healthy_recovery
                    .map(|point| {
                        vec![trace_evidence(
                            "recoveryPoint",
                            "恢复点",
                            &point.next_step,
                            &point.id,
                        )]
                    })
                    .unwrap_or_default(),
                "恢复点记录了尚未完成的下一步。".to_string(),
                manifest.project.next_step.clone(),
                trace_confidence(88),
            ),
        });
    }
    recommended_actions.sort_by(|left, right| {
        let rank = |value: &str| match value {
            "fact" => 0,
            "suggestion" => 1,
            _ => 2,
        };
        rank(&left.item_type)
            .cmp(&rank(&right.item_type))
            .then_with(|| right.confidence.score.cmp(&left.confidence.score))
    });
    let snapshot = DailyContinueSnapshot {
        id: new_id(),
        project_id: manifest.project.id.clone(),
        generated_at: now_string(),
        last_progress: healthy_recovery
            .map(|point| vec![point.completed.clone(), point.next_step.clone()])
            .unwrap_or_default(),
        recommended_actions,
        blockers,
        pending_confirmations,
        recent_changes: manifest
            .project_impact_analyses
            .iter()
            .rev()
            .take(5)
            .map(|impact| impact.summary.clone())
            .collect(),
        status: "ready".to_string(),
    };
    manifest.daily_continue_snapshots.push(snapshot);
    if manifest.daily_continue_snapshots.len() > 30 {
        let excess = manifest.daily_continue_snapshots.len() - 30;
        manifest.daily_continue_snapshots.drain(0..excess);
    }
}

fn looks_garbled_or_meaningless(value: &str) -> bool {
    let text = value.trim();
    if text.is_empty() {
        return true;
    }
    let suspicious = text
        .chars()
        .filter(|ch| matches!(ch, '?' | '\u{fffd}' | '\0'))
        .count();
    suspicious >= 2 || suspicious.saturating_mul(5) > text.chars().count()
}

fn task_has_action(value: &str) -> bool {
    [
        "完成",
        "修复",
        "实现",
        "检查",
        "确认",
        "整理",
        "生成",
        "更新",
        "验证",
        "测试",
        "提交",
        "分析",
        "导入",
        "创建",
        "优化",
        "处理",
        "继续",
        "review",
        "fix",
        "build",
        "test",
        "implement",
        "update",
    ]
    .iter()
    .any(|verb| value.to_lowercase().contains(verb))
}

fn data_health_key(record: &DataHealthRecord) -> String {
    format!("{}|{}", record.health_type, record.source)
}

fn update_data_health_records(manifest: &mut ProjectManifest) {
    let previous = manifest
        .data_health_records
        .iter()
        .map(|record| (data_health_key(record), record.clone()))
        .collect::<HashMap<_, _>>();
    let mut records = Vec::new();
    let mut push = |health_type: &str,
                    severity: &str,
                    source: String,
                    description: String,
                    suggested_action: &str| {
        let key = format!("{health_type}|{source}");
        if let Some(existing) = previous.get(&key) {
            records.push(existing.clone());
        } else {
            records.push(DataHealthRecord {
                id: new_id(),
                health_type: health_type.to_string(),
                severity: severity.to_string(),
                source,
                description,
                suggested_action: suggested_action.to_string(),
                status: "open".to_string(),
                created_at: now_string(),
            });
        }
    };
    for point in &manifest.recovery_points {
        if looks_garbled_or_meaningless(&point.completed)
            || looks_garbled_or_meaningless(&point.next_step)
        {
            push(
                "suspect",
                "high",
                format!("recoveryPoint:{}", point.id),
                "恢复点为空、乱码或缺少可用语义。".to_string(),
                "人工核对恢复点；处理前不用于 Today 或 AI 上下文。",
            );
        }
    }
    for task in &manifest.tasks {
        if task.title.chars().count() > 160
            || task.title.contains('\n')
            || !task_has_action(&task.title)
        {
            push(
                "lowQuality",
                "medium",
                format!("task:{}", task.id),
                "任务标题过长、像整段说明，或缺少可执行动作。".to_string(),
                "将任务改写为简短、可执行且可验收的动作。",
            );
        }
    }
    for file in &manifest.files {
        let ext = file.file_type.to_lowercase();
        let path = file.managed_path.to_lowercase().replace('/', "\\");
        if matches!(
            ext.as_str(),
            "exe" | "dll" | "pdb" | "msi" | "tmp" | "cache"
        ) || path.contains("\\target\\")
            || path.contains("\\dist\\")
            || path.contains("\\node_modules\\")
        {
            push(
                "notSuitableForAIContext",
                "high",
                format!("file:{}", file.id),
                format!("{} 是可执行文件、临时文件或构建产物。", file.file_name),
                "保留位置记录，但默认排除在 AI 项目理解之外。",
            );
        }
    }
    for artifact in &manifest.artifacts {
        let ext = Path::new(&artifact.managed_path)
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_lowercase();
        if matches!(ext.as_str(), "exe" | "dll" | "pdb" | "msi") {
            push(
                "notSuitableForAIContext",
                "high",
                format!("artifact:{}", artifact.id),
                format!("成果 {} 是二进制构建产物。", artifact.title),
                "仅保留成果记录，不作为 AI 事实依据。",
            );
        }
    }
    manifest.data_health_records = records;
}

fn unhealthy_source(manifest: &ProjectManifest, kind: &str, id: &str) -> bool {
    let source = format!("{kind}:{id}");
    manifest.data_health_records.iter().any(|record| {
        record.source == source
            && record.status == "open"
            && matches!(
                record.health_type.as_str(),
                "invalid" | "suspect" | "lowQuality" | "notSuitableForAIContext"
            )
    })
}

fn trace_evidence(
    kind: &str,
    label: &str,
    summary: &str,
    source_id: &str,
) -> DecisionTraceEvidence {
    DecisionTraceEvidence {
        kind: kind.to_string(),
        label: label.to_string(),
        summary: first_line(summary, 240),
        source_id: source_id.to_string(),
    }
}

fn update_project_state_summary(manifest: &mut ProjectManifest) {
    update_data_health_records(manifest);
    let unfinished = manifest
        .tasks
        .iter()
        .filter(|task| {
            !matches!(
                task.status.as_str(),
                "done" | "completed" | "closed" | "resolved"
            ) && !unhealthy_source(manifest, "task", &task.id)
        })
        .collect::<Vec<_>>();
    let latest_recovery = manifest
        .recovery_points
        .iter()
        .rev()
        .find(|point| !unhealthy_source(manifest, "recoveryPoint", &point.id));
    let latest_impact = manifest.project_impact_analyses.last();
    let risks = manifest
        .project_action_candidates
        .iter()
        .filter(|item| item.status == "pending" && item.candidate_type == "risk")
        .map(|item| item.title.clone())
        .take(5)
        .collect::<Vec<_>>();
    let blockers = manifest
        .project_action_candidates
        .iter()
        .filter(|item| item.status == "pending" && item.candidate_type == "blocker")
        .map(|item| item.title.clone())
        .take(5)
        .collect::<Vec<_>>();
    let current_phase = if !manifest.artifacts.is_empty() || !manifest.codex_reports.is_empty() {
        "执行与验收"
    } else if !unfinished.is_empty() {
        "任务推进"
    } else if !manifest.files.is_empty() {
        "资料理解"
    } else {
        "尚未形成阶段"
    };
    let mut facts = Vec::new();
    let mut evidence = Vec::new();
    if let Some(point) = latest_recovery {
        facts.push(format!("最近恢复点：{}", point.completed));
        evidence.push(trace_evidence(
            "recoveryPoint",
            "最近恢复点",
            &point.completed,
            &point.id,
        ));
    }
    if let Some(impact) = latest_impact {
        facts.push(format!("最近资料变化：{}", impact.summary));
        evidence.push(trace_evidence(
            "projectImpact",
            "最近资料变化",
            &impact.summary,
            &impact.id,
        ));
    }
    if let Some(task) = unfinished.last() {
        facts.push(format!("未完成任务：{}", task.title));
        evidence.push(trace_evidence("task", "未完成任务", &task.title, &task.id));
    }
    if let Some(artifact) = manifest
        .artifacts
        .iter()
        .rev()
        .find(|item| !unhealthy_source(manifest, "artifact", &item.id))
    {
        facts.push(format!("最近成果：{}", artifact.title));
        evidence.push(trace_evidence(
            "outcome",
            "最近成果",
            &artifact.title,
            &artifact.id,
        ));
    }
    let next_milestone = latest_recovery
        .filter(|point| !point.next_step.trim().is_empty())
        .map(|point| point.next_step.clone())
        .or_else(|| unfinished.last().map(|task| task.title.clone()))
        .unwrap_or_else(|| {
            if looks_garbled_or_meaningless(&manifest.project.next_step) {
                String::new()
            } else {
                manifest.project.next_step.clone()
            }
        });
    let inferences = if facts.is_empty() {
        Vec::new()
    } else {
        vec![format!("依据现有记录，项目处于“{current_phase}”阶段。")]
    };
    let suggestions = if next_milestone.trim().is_empty() {
        Vec::new()
    } else {
        vec![format!("建议下一步：{next_milestone}")]
    };
    let trace = new_decision_trace(
        "projectStateProposal",
        &manifest.project.id,
        &manifest.project.id,
        evidence.clone(),
        inferences
            .first()
            .cloned()
            .unwrap_or_else(|| "现有记录不足以判断项目阶段。".to_string()),
        suggestions
            .first()
            .cloned()
            .unwrap_or_else(|| "暂不提出项目状态建议。".to_string()),
        trace_confidence(if evidence.len() >= 2 {
            84
        } else if evidence.len() == 1 {
            65
        } else {
            20
        }),
    );
    manifest.project_state_summary = ProjectStateSummary {
        project_id: manifest.project.id.clone(),
        current_phase: current_phase.to_string(),
        recent_progress: latest_recovery
            .map(|point| vec![point.completed.clone()])
            .unwrap_or_default(),
        current_risks: risks,
        blockers,
        next_milestone,
        recent_changes: latest_impact
            .map(|impact| vec![impact.summary.clone()])
            .unwrap_or_default(),
        facts,
        inferences,
        suggestions,
        evidence,
        generated_at: now_string(),
        decision_trace: trace,
    };
}

fn update_project_attentions(manifest: &mut ProjectManifest) {
    let existing = manifest
        .project_attentions
        .iter()
        .map(|item| {
            (
                (item.attention_type.clone(), item.title.clone()),
                item.clone(),
            )
        })
        .collect::<HashMap<_, _>>();
    let mut next = Vec::new();
    let recovery_time = manifest
        .recovery_points
        .last()
        .and_then(|point| point.created_at.parse::<u128>().ok())
        .unwrap_or_default();
    if let Some(file) = (recovery_time > 0)
        .then(|| {
            manifest
                .files
                .iter()
                .filter(|file| !unhealthy_source(manifest, "file", &file.id))
                .filter(|file| file.modified_at.parse::<u128>().unwrap_or_default() > recovery_time)
                .max_by_key(|file| file.modified_at.parse::<u128>().unwrap_or_default())
        })
        .flatten()
    {
        next.push(build_attention(
            manifest,
            &existing,
            "fileChange",
            "新增资料尚未反映到恢复点",
            &format!("文件“{}”晚于最近恢复点进入项目。", file.file_name),
            "检查资料影响并确认是否更新任务或下一步。",
            86,
            vec![trace_evidence(
                "file",
                "新增资料",
                &file.content_summary,
                &file.id,
            )],
        ));
    }
    let now_ms = now_string().parse::<u128>().unwrap_or_default();
    let last_activity = manifest
        .daily_sessions
        .last()
        .map(|session| session.updated_at.as_str())
        .unwrap_or(&manifest.project.last_opened_at)
        .parse::<u128>()
        .unwrap_or_default();
    if last_activity > 0 && now_ms.saturating_sub(last_activity) >= 14 * 86_400_000 {
        next.push(build_attention(
            manifest,
            &existing,
            "inactive",
            "项目已较长时间没有活动",
            "最近项目活动距今已超过 14 天。",
            "确认项目是否继续、等待他人，或暂时搁置。",
            98,
            vec![trace_evidence(
                "dailySession",
                "最近活动",
                &last_activity.to_string(),
                &manifest.project.id,
            )],
        ));
    }
    if let Some(task) = manifest.tasks.iter().find(|task| {
        !matches!(
            task.status.as_str(),
            "done" | "completed" | "closed" | "resolved"
        ) && !unhealthy_source(manifest, "task", &task.id)
            && task_is_confirmed_project_task(manifest, task)
            && task
                .created_at
                .parse::<u128>()
                .map(|time| now_ms.saturating_sub(time) >= 14 * 86_400_000)
                .unwrap_or(false)
    }) {
        next.push(build_attention(
            manifest,
            &existing,
            "staleTask",
            "任务长期未关闭",
            &format!("任务“{}”创建超过 14 天且仍未完成。", task.title),
            "确认任务状态、阻塞原因或下一动作。",
            95,
            vec![trace_evidence(
                "task",
                "长期未完成任务",
                &task.title,
                &task.id,
            )],
        ));
    }
    if manifest.decisions.len() >= 2 {
        let latest = manifest.decisions.last().expect("two decisions exist");
        let conflict = ["取消", "不再", "改为", "替换", "停止"]
            .iter()
            .any(|signal| latest.summary.contains(signal));
        if conflict {
            let previous = &manifest.decisions[manifest.decisions.len() - 2];
            next.push(build_attention(
                manifest,
                &existing,
                "decisionConflict",
                "新决定可能影响既有决定",
                &format!(
                    "新决定“{}”包含变更信号，需要与上一决定核对。",
                    latest.summary
                ),
                "人工确认新旧决定是否冲突。",
                72,
                vec![
                    trace_evidence("decision", "上一决定", &previous.summary, &previous.id),
                    trace_evidence("decision", "新决定", &latest.summary, &latest.id),
                ],
            ));
        }
    }
    manifest.project_attentions = next;
}

fn build_attention(
    manifest: &ProjectManifest,
    existing: &HashMap<(String, String), ProjectAttention>,
    attention_type: &str,
    title: &str,
    reason: &str,
    suggested_action: &str,
    score: u8,
    evidence: Vec<DecisionTraceEvidence>,
) -> ProjectAttention {
    let prior = existing.get(&(attention_type.to_string(), title.to_string()));
    let now = now_string();
    let confidence = trace_confidence(score);
    ProjectAttention {
        id: prior.map(|item| item.id.clone()).unwrap_or_else(new_id),
        project_id: manifest.project.id.clone(),
        attention_type: attention_type.to_string(),
        title: title.to_string(),
        reason: reason.to_string(),
        evidence: evidence.clone(),
        suggested_action: suggested_action.to_string(),
        confidence: confidence.clone(),
        status: prior
            .map(|item| item.status.clone())
            .unwrap_or_else(|| "pending".to_string()),
        created_at: prior
            .map(|item| item.created_at.clone())
            .unwrap_or_else(|| now.clone()),
        updated_at: now,
        decision_trace: prior
            .map(|item| item.decision_trace.clone())
            .unwrap_or_else(|| {
                new_decision_trace(
                    "riskCandidate",
                    attention_type,
                    &manifest.project.id,
                    evidence,
                    reason.to_string(),
                    suggested_action.to_string(),
                    confidence,
                )
            }),
    }
}

fn update_work_pattern_candidates(manifest: &mut ProjectManifest) {
    let mut candidates = crate::weekly_review::build_skill_candidates(manifest, "project")
        .into_iter()
        .filter(|candidate| candidate.occurrence_count >= 2)
        .collect::<Vec<_>>();
    if manifest.daily_sessions.len() >= 2 && manifest.recovery_points.len() >= 2 {
        let evidence = manifest
            .recovery_points
            .iter()
            .rev()
            .take(5)
            .map(|point| format!("{} → {}", point.completed, point.next_step))
            .collect::<Vec<_>>();
        candidates.push(work_pattern_candidate(
            manifest,
            "恢复点驱动的连续推进",
            "跨日继续项目工作",
            manifest.daily_sessions.len(),
            evidence,
            vec![
                "记录完成内容".to_string(),
                "明确下一步".to_string(),
                "下次从恢复点继续".to_string(),
            ],
        ));
    }
    manifest.work_pattern_candidates = candidates;
}

fn update_v1_foundation(root: &Path, manifest: &mut ProjectManifest) -> Result<(), String> {
    if manifest.work_pattern_candidates.is_empty() {
        update_work_pattern_candidates(manifest);
    }
    manifest.atlas_skill_assessments = assess_skill_candidates_against_atlas(manifest);
    manifest.skill_library = build_personal_skill_library(manifest);
    manifest.knowledge_pattern_candidates = build_knowledge_pattern_candidates(manifest);
    manifest.improvement_candidates = build_improvement_candidates(manifest);
    manifest.v1_readiness = build_v1_readiness(root, manifest)?;
    Ok(())
}

fn assess_skill_candidates_against_atlas(manifest: &ProjectManifest) -> Vec<AtlasSkillAssessment> {
    let atlas_root = PathBuf::from(r"D:\Atlas");
    let readme = fs::read_to_string(atlas_root.join("README.md")).unwrap_or_default();
    let roadmap = fs::read_to_string(atlas_root.join("ROADMAP.md")).unwrap_or_default();
    let registry =
        fs::read_to_string(atlas_root.join("registry/candidate-modules.md")).unwrap_or_default();
    let atlas_text = format!("{readme}\n{roadmap}\n{registry}").to_lowercase();
    let atlas_available = atlas_root.exists() && !atlas_text.trim().is_empty();
    manifest
        .work_pattern_candidates
        .iter()
        .map(|candidate| {
            let skill_text = format!(
                "{} {} {} {}",
                candidate.name,
                candidate.scenario,
                candidate.reusable_steps.join(" "),
                candidate.evidence.join(" ")
            )
            .to_lowercase();
            let mut matched_capability = "unmatched".to_string();
            let mut reusable_evidence = Vec::new();
            let mut similarity = 0.0_f32;
            if atlas_available {
                let checks: [(&str, &[&str]); 4] = [
                    (
                        "Tabular Core",
                        &["excel", "xlsx", "表格", "数据", "sheet", "tabular"],
                    ),
                    (
                        "File Lifecycle",
                        &["文件", "位置", "归位", "版本", "file", "location"],
                    ),
                    (
                        "Operation Outcome",
                        &["验收", "结果", "报告", "outcome", "测试"],
                    ),
                    ("Runtime Config", &["配置", "key", "凭据", "环境", "config"]),
                ];
                for (capability, keywords) in checks {
                    let skill_hits = keywords
                        .iter()
                        .filter(|keyword| skill_text.contains(**keyword))
                        .count();
                    let atlas_hits = keywords
                        .iter()
                        .filter(|keyword| atlas_text.contains(**keyword))
                        .count();
                    if skill_hits > 0 && atlas_hits > 0 {
                        matched_capability = capability.to_string();
                        similarity = ((skill_hits.min(atlas_hits) as f32) / 3.0).clamp(0.32, 0.92);
                        reusable_evidence.push(format!(
                            "技能候选和 Atlas 文档共同命中 {} 类关键词。",
                            capability
                        ));
                        break;
                    }
                }
            }
            let status = if !atlas_available {
                "unavailable".to_string()
            } else if matched_capability == "unmatched" {
                "unmatched".to_string()
            } else {
                "matched".to_string()
            };
            let confidence_score = if similarity >= 0.75 {
                82
            } else if similarity >= 0.32 {
                64
            } else {
                36
            };
            let evidence_refs = if atlas_available {
                vec![
                    "D:\\Atlas\\README.md".to_string(),
                    "D:\\Atlas\\ROADMAP.md".to_string(),
                    "D:\\Atlas\\registry\\candidate-modules.md".to_string(),
                ]
            } else {
                Vec::new()
            };
            let uncovered_part = if status == "matched" {
                "仍需人工评审是否可沉淀为 Atlas 证据，当前不会写入 Atlas。".to_string()
            } else if status == "unavailable" {
                "Atlas 本地入口不可读，无法只读对照。".to_string()
            } else {
                "未找到可靠匹配，作为感冒院新发现能力候选保留。".to_string()
            };
            AtlasSkillAssessment {
                id: format!("atlas-skill:{}", stable_key(&candidate.name)),
                skill_name: candidate.name.clone(),
                matched_capability,
                reusable_evidence: reusable_evidence.clone(),
                uncovered_part: uncovered_part.clone(),
                similarity,
                confidence: trace_confidence(confidence_score),
                review_required: true,
                status: status.clone(),
                assessed_at: now_string(),
                evidence_references: evidence_refs,
                decision_trace: new_decision_trace(
                    "skillCandidate",
                    &format!("atlas-skill:{}", stable_key(&candidate.name)),
                    &manifest.project.id,
                    candidate
                        .evidence
                        .iter()
                        .take(5)
                        .map(|item| trace_evidence("skillCandidate", "技能候选证据", item, ""))
                        .collect(),
                    format!("Atlas Shadow 只读对照结果：{status}"),
                    uncovered_part,
                    trace_confidence(confidence_score),
                ),
            }
        })
        .collect()
}

fn build_personal_skill_library(manifest: &ProjectManifest) -> Vec<PersonalSkill> {
    manifest
        .work_pattern_candidates
        .iter()
        .filter(|candidate| candidate.review_required && candidate.occurrence_count >= 2)
        .map(|candidate| PersonalSkill {
            id: format!("skill:{}", stable_key(&candidate.name)),
            name: candidate.name.clone(),
            scenario: candidate.scenario.clone(),
            evidence: candidate.evidence.iter().take(8).cloned().collect(),
            frequency: candidate.occurrence_count,
            steps: candidate.reusable_steps.clone(),
            examples: candidate.evidence.iter().take(3).cloned().collect(),
            confidence: trace_confidence_from_ratio(candidate.confidence),
            status: "candidate".to_string(),
            created_at: now_string(),
            updated_at: now_string(),
            decision_trace: candidate.decision_trace.clone(),
        })
        .collect()
}

fn build_knowledge_pattern_candidates(
    manifest: &ProjectManifest,
) -> Vec<KnowledgePatternCandidate> {
    let mut candidates = Vec::new();
    if manifest.codex_reports.len() >= 2 && manifest.codex_prompts.len() >= 2 {
        let evidence = manifest
            .codex_reports
            .iter()
            .rev()
            .take(5)
            .map(|report| {
                if report.summary.trim().is_empty() {
                    format!("Codex 报告：{}", report.id)
                } else {
                    report.summary.clone()
                }
            })
            .collect::<Vec<_>>();
        candidates.push(knowledge_pattern_candidate(
            manifest,
            "Codex 执行闭环",
            evidence,
            vec![
                "生成收敛提示词".to_string(),
                "交给 Codex 执行".to_string(),
                "导入报告并验收".to_string(),
                "更新项目恢复点".to_string(),
            ],
            "减少重复背景整理，让执行报告成为项目证据。",
            78,
        ));
    }
    if manifest.files.len() >= 3
        && manifest
            .decision_traces_for_types(&["locationRecommendation", "autoRoute"])
            .len()
            >= 2
    {
        let evidence = manifest
            .files
            .iter()
            .rev()
            .take(5)
            .map(|file| format!("{} -> {}", file.file_name, file.managed_relative_path))
            .collect::<Vec<_>>();
        candidates.push(knowledge_pattern_candidate(
            manifest,
            "文件理解与位置归位",
            evidence,
            vec![
                "接收资料".to_string(),
                "解析摘要".to_string(),
                "判断归属".to_string(),
                "推荐位置并记录证据".to_string(),
            ],
            "适合复用到需要长期掌控资料位置的项目。",
            82,
        ));
    }
    candidates
}

fn knowledge_pattern_candidate(
    manifest: &ProjectManifest,
    name: &str,
    evidence: Vec<String>,
    common_steps: Vec<String>,
    reusable_value: &str,
    score: u8,
) -> KnowledgePatternCandidate {
    let id = format!("knowledge:{}", stable_key(name));
    KnowledgePatternCandidate {
        id: id.clone(),
        pattern_name: name.to_string(),
        related_projects: vec![manifest.project.name.clone()],
        evidence: evidence.clone(),
        common_steps,
        reusable_value: reusable_value.to_string(),
        confidence: trace_confidence(score),
        review_required: true,
        created_at: now_string(),
        updated_at: now_string(),
        decision_trace: new_decision_trace(
            "skillCandidate",
            &id,
            &manifest.project.id,
            evidence
                .iter()
                .take(6)
                .map(|item| trace_evidence("projectRecord", "项目记录", item, ""))
                .collect(),
            format!("发现可复用知识模式：{name}"),
            reusable_value.to_string(),
            trace_confidence(score),
        ),
    }
}

fn build_improvement_candidates(manifest: &ProjectManifest) -> Vec<ImprovementCandidate> {
    let mut candidates = Vec::new();
    let open_health = manifest
        .data_health_records
        .iter()
        .filter(|item| item.status == "open")
        .count();
    if open_health >= 2 {
        candidates.push(improvement_candidate(
            manifest,
            "历史数据质量会影响 Today 和 AI 上下文",
            manifest
                .data_health_records
                .iter()
                .filter(|item| item.status == "open")
                .take(5)
                .map(|item| item.description.clone())
                .collect(),
            "降低建议可信度，需要更多人工确认。",
            "继续优化数据健康入口，让用户更快处理低质量记录。",
            78,
        ));
    }
    let pending_reviews = manifest
        .pending_reviews
        .iter()
        .filter(|item| item.status == "pending")
        .count();
    if pending_reviews >= 3 {
        candidates.push(improvement_candidate(
            manifest,
            "待确认事项积压",
            manifest
                .pending_reviews
                .iter()
                .filter(|item| item.status == "pending")
                .take(5)
                .map(|item| item.title.clone())
                .collect(),
            "用户需要反复手动处理归位、影响分析或状态提案。",
            "合并相似待确认项，并突出高证据事项。",
            74,
        ));
    }
    let rejected_or_modified = manifest
        .decision_traces_for_types(&[
            "fileClassification",
            "ownershipDecision",
            "projectMatch",
            "locationRecommendation",
            "autoRoute",
            "projectImpact",
            "taskCandidate",
            "decisionCandidate",
            "riskCandidate",
            "projectStateProposal",
            "skillCandidate",
        ])
        .into_iter()
        .filter(|trace| matches!(trace.user_decision.as_str(), "rejected" | "modified"))
        .count();
    if rejected_or_modified >= 2 {
        candidates.push(improvement_candidate(
            manifest,
            "AI 建议被多次拒绝或修改",
            manifest
                .decision_traces_for_types(&[
                    "fileClassification",
                    "ownershipDecision",
                    "projectMatch",
                    "locationRecommendation",
                    "autoRoute",
                    "projectImpact",
                    "taskCandidate",
                    "decisionCandidate",
                    "riskCandidate",
                    "projectStateProposal",
                    "skillCandidate",
                ])
                .into_iter()
                .filter(|trace| matches!(trace.user_decision.as_str(), "rejected" | "modified"))
                .take(5)
                .map(|trace| format!("{}：{}", trace.decision_type, trace.recommendation))
                .collect(),
            "分类、状态或行动建议规则可能仍不贴近真实工作。",
            "优先复盘被修改的 Decision Trace，补充更可靠的判断证据。",
            80,
        ));
    }
    candidates
}

fn improvement_candidate(
    manifest: &ProjectManifest,
    problem: &str,
    evidence: Vec<String>,
    impact: &str,
    suggested_improvement: &str,
    score: u8,
) -> ImprovementCandidate {
    let id = format!("improvement:{}", stable_key(problem));
    ImprovementCandidate {
        id: id.clone(),
        problem: problem.to_string(),
        evidence: evidence.clone(),
        impact: impact.to_string(),
        suggested_improvement: suggested_improvement.to_string(),
        confidence: trace_confidence(score),
        status: "candidate".to_string(),
        created_at: now_string(),
        updated_at: now_string(),
        decision_trace: new_decision_trace(
            "projectStateProposal",
            &id,
            &manifest.project.id,
            evidence
                .iter()
                .take(6)
                .map(|item| trace_evidence("usageFeedback", "使用反馈证据", item, ""))
                .collect(),
            problem.to_string(),
            suggested_improvement.to_string(),
            trace_confidence(score),
        ),
    }
}

fn build_v1_readiness(root: &Path, manifest: &ProjectManifest) -> Result<V1Readiness, String> {
    let dir = root.join(".ganmaoyuan/v1-readiness");
    fs::create_dir_all(&dir)
        .map_err(|err| format!("创建 v1 收口目录失败：{}：{err}", path_to_string(&dir)))?;
    let capability_overview = vec![
        "文件入口：Inbox、桌面拖入、SendTo、应用内导入统一进入文件接收链路。".to_string(),
        "AI 理解：基于项目说明、文件摘要、对话、Decision Trace 和项目分析回答问题。".to_string(),
        "项目管理：项目根目录、managed 文件、副本、版本、manifest 和审计记录本地保存。".to_string(),
        "Decision Trace：记录分类、归位、项目影响、状态建议、技能候选等判断依据。".to_string(),
        "主动工作：Today Workspace、Project State、Attention 和恢复点帮助继续工作。".to_string(),
        "技能沉淀：Weekly Review、Skill Candidate、个人技能库和知识模式候选均需人工确认。"
            .to_string(),
        "Atlas 接口：仅只读 Shadow 评估，不写入、不训练、不提交。".to_string(),
    ];
    let current_limitations = vec![
        "云同步、多用户、账号系统和在线更新未实现。".to_string(),
        "Atlas 只读，不自动写入证据或训练能力。".to_string(),
        "图片 OCR、语音识别和复杂文件语义仍不是完整能力。".to_string(),
        "AI 判断依赖摘要和项目记录，资料不足时不能替代人工确认。".to_string(),
        "安装包覆盖和迁移仍建议人工验收后再用于长期生产。".to_string(),
    ];
    let roadmap_candidates = vec![
        "第26步以后先进入真实试运行，记录阻塞和误判。".to_string(),
        "完善资料确认体验，减少待检查积压。".to_string(),
        "在人工评审后选择少量技能候选进入 Atlas 证据准备。".to_string(),
        "补充更细的项目导出、诊断和安装验收脚本。".to_string(),
    ];
    let security_checks = vec![
        "API Key 使用 Windows Credential Manager，不写入项目 JSON 或导出。".to_string(),
        "备份、导出和诊断日志应继续排除凭据和敏感正文。".to_string(),
        "D:\\Atlas 保持只读访问。".to_string(),
        "prototype/opendesign 仅作为设计证据保留。".to_string(),
    ];
    let build_root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let installer_paths = vec![
        path_to_string(
            &build_root.join("src-tauri/target/release/bundle/nsis/Ganmaoyuan_0.1.1_x64-setup.exe"),
        ),
        path_to_string(
            &build_root.join("src-tauri/target/release/bundle/msi/Ganmaoyuan_0.1.1_x64_en-US.msi"),
        ),
    ];
    let overview_path = dir.join("system-capability-overview.md");
    let usage_guide_path = dir.join("v1-usage-guide.md");
    let limitations_path = dir.join("current-limitations.md");
    let roadmap_path = dir.join("future-roadmap-candidates.md");
    write_text_atomic(
        &overview_path,
        &format!(
            "# 感冒院 v1 能力总览\n\n项目：{}\n\n{}\n",
            manifest.project.name,
            capability_overview
                .iter()
                .map(|item| format!("- {item}"))
                .collect::<Vec<_>>()
                .join("\n")
        ),
    )?;
    write_text_atomic(
        &usage_guide_path,
        "# 感冒院 v1 使用说明\n\n- 从 Today Workspace 查看今天应该做什么。\n- 新文件先进入 Inbox，由感冒院解析、判断、推荐位置。\n- 项目工作台用于连续对话、交给 Codex、导入执行报告和收工。\n- AI 回答需要查看依据；资料不足时以人工确认为准。\n- 技能候选、知识模式和 Atlas Shadow 结果都需要人工评审。\n",
    )?;
    write_text_atomic(
        &limitations_path,
        &format!(
            "# 当前限制列表\n\n{}\n",
            current_limitations
                .iter()
                .map(|item| format!("- {item}"))
                .collect::<Vec<_>>()
                .join("\n")
        ),
    )?;
    write_text_atomic(
        &roadmap_path,
        &format!(
            "# 未来路线候选\n\n{}\n",
            roadmap_candidates
                .iter()
                .map(|item| format!("- {item}"))
                .collect::<Vec<_>>()
                .join("\n")
        ),
    )?;
    Ok(V1Readiness {
        generated_at: now_string(),
        overview_path: path_to_string(&overview_path),
        usage_guide_path: path_to_string(&usage_guide_path),
        limitations_path: path_to_string(&limitations_path),
        roadmap_path: path_to_string(&roadmap_path),
        capability_overview,
        current_limitations,
        roadmap_candidates,
        security_checks,
        installer_paths,
    })
}

trait ManifestTraceExt {
    fn decision_traces_for_types(&self, trace_types: &[&str]) -> Vec<&DecisionTrace>;
}

impl ManifestTraceExt for ProjectManifest {
    fn decision_traces_for_types(&self, trace_types: &[&str]) -> Vec<&DecisionTrace> {
        self.project_impact_analyses
            .iter()
            .map(|item| &item.decision_trace)
            .chain(
                self.project_action_candidates
                    .iter()
                    .map(|item| &item.decision_trace),
            )
            .chain(
                self.project_state_proposals
                    .iter()
                    .map(|item| &item.decision_trace),
            )
            .chain(
                self.project_attentions
                    .iter()
                    .map(|item| &item.decision_trace),
            )
            .chain(
                self.work_pattern_candidates
                    .iter()
                    .map(|item| &item.decision_trace),
            )
            .filter(|trace| trace_types.contains(&trace.decision_type.as_str()))
            .collect()
    }
}

fn stable_key(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    format!("{:x}", hasher.finalize())[..12].to_string()
}

fn work_pattern_candidate(
    manifest: &ProjectManifest,
    name: &str,
    scenario: &str,
    occurrence_count: usize,
    evidence: Vec<String>,
    reusable_steps: Vec<String>,
) -> WeeklySkillCandidate {
    let confidence = if occurrence_count >= 4 { 0.88 } else { 0.72 };
    WeeklySkillCandidate {
        name: name.to_string(),
        scenario: scenario.to_string(),
        evidence: evidence.clone(),
        occurrence_count,
        reusable_steps,
        expected_value: "减少重复整理并保持项目连续性。".to_string(),
        current_gap: "仍需人工评审后才能作为稳定技能使用。".to_string(),
        confidence,
        review_required: true,
        decision_trace: skill_candidate_decision_trace(
            &manifest.project.id,
            name,
            scenario,
            &evidence,
            confidence,
        ),
    }
}

// ===== V3 第16~20步：用户交互、状态提案应用与今日继续 =====

fn apply_state_proposal_changes(manifest: &mut ProjectManifest, proposal: &ProjectStateProposal) {
    for change in &proposal.proposed_changes {
        if change.field == "nextStep" {
            manifest.project.next_step = change.after.clone();
        }
    }
}

fn revert_state_proposal_changes(manifest: &mut ProjectManifest, proposal: &ProjectStateProposal) {
    for change in &proposal.before {
        if change.field == "nextStep" {
            manifest.project.next_step = change.before.clone();
        }
    }
}

fn resolve_pending_review_for_candidate(
    manifest: &mut ProjectManifest,
    candidate: &ProjectActionCandidate,
) {
    let key = format!(
        "impact-candidate:{}:{}",
        candidate.source_hash, candidate.candidate_type
    );
    for item in manifest.pending_reviews.iter_mut() {
        if item.key == key && item.status != "ignored" {
            item.status = "resolved".to_string();
            item.updated_at = now_string();
        }
    }
}

pub fn apply_state_proposal<R: Runtime>(
    _app: &tauri::AppHandle<R>,
    project_root: String,
    proposal_id: String,
) -> Result<ProjectStateProposal, String> {
    let _guard = project_write_lock()
        .lock()
        .map_err(|_| "项目存储锁已损坏，请重启感冒院。".to_string())?;
    let root = PathBuf::from(&project_root);
    let mut manifest = read_and_repair_manifest(&root)?;
    let idx = manifest
        .project_state_proposals
        .iter()
        .position(|item| item.id == proposal_id)
        .ok_or_else(|| "状态更新提案不存在。".to_string())?;
    let mut proposal = manifest.project_state_proposals[idx].clone();
    ensure_state_proposal_trace(&mut proposal, &manifest.project.id);
    if proposal.status == "applied" || proposal.status == "autoApplied" {
        return Ok(proposal);
    }
    apply_state_proposal_changes(&mut manifest, &proposal);
    proposal.applied_at = now_string();
    proposal.status = "applied".to_string();
    proposal.decision_trace.user_decision = "approved".to_string();
    proposal.decision_trace.user_decision_note = "用户确认应用状态提案。".to_string();
    proposal.decision_trace.execution = "executed".to_string();
    proposal.decision_trace.execution_note = "项目状态已更新。".to_string();
    proposal.decision_trace.updated_at = now_string();
    manifest.project_state_proposals[idx] = proposal.clone();
    manifest.audit.push(audit_event(
        "project.state.apply",
        &proposal.id,
        "applied",
        "用户确认应用项目状态更新提案。".to_string(),
        false,
        true,
    ));
    persist_project(&root, &manifest, None)?;
    append_audit_event(&root, manifest.audit.last().expect("audit event exists"))?;
    Ok(proposal)
}

pub fn undo_state_proposal<R: Runtime>(
    _app: &tauri::AppHandle<R>,
    project_root: String,
    proposal_id: String,
) -> Result<ProjectStateProposal, String> {
    let _guard = project_write_lock()
        .lock()
        .map_err(|_| "项目存储锁已损坏，请重启感冒院。".to_string())?;
    let root = PathBuf::from(&project_root);
    let mut manifest = read_and_repair_manifest(&root)?;
    let idx = manifest
        .project_state_proposals
        .iter()
        .position(|item| item.id == proposal_id)
        .ok_or_else(|| "状态更新提案不存在。".to_string())?;
    let mut proposal = manifest.project_state_proposals[idx].clone();
    ensure_state_proposal_trace(&mut proposal, &manifest.project.id);
    if proposal.status != "applied" && proposal.status != "autoApplied" {
        return Ok(proposal);
    }
    revert_state_proposal_changes(&mut manifest, &proposal);
    proposal.status = "undone".to_string();
    proposal.applied_at = String::new();
    proposal.decision_trace.execution = "cancelled".to_string();
    proposal.decision_trace.execution_note = "用户撤销状态提案。".to_string();
    proposal.decision_trace.updated_at = now_string();
    manifest.project_state_proposals[idx] = proposal.clone();
    manifest.audit.push(audit_event(
        "project.state.undo",
        &proposal.id,
        "undone",
        "撤销项目状态更新提案，已恢复应用前数值。".to_string(),
        false,
        true,
    ));
    persist_project(&root, &manifest, None)?;
    append_audit_event(&root, manifest.audit.last().expect("audit event exists"))?;
    Ok(proposal)
}

pub fn confirm_action_candidate<R: Runtime>(
    _app: &tauri::AppHandle<R>,
    project_root: String,
    candidate_id: String,
) -> Result<ProjectActionCandidate, String> {
    let _guard = project_write_lock()
        .lock()
        .map_err(|_| "项目存储锁已损坏，请重启感冒院。".to_string())?;
    let root = PathBuf::from(&project_root);
    let mut manifest = read_and_repair_manifest(&root)?;
    let idx = manifest
        .project_action_candidates
        .iter()
        .position(|item| item.id == candidate_id)
        .ok_or_else(|| "行动候选不存在。".to_string())?;
    let mut candidate = manifest.project_action_candidates[idx].clone();
    ensure_action_candidate_trace(&mut candidate, &manifest.project.id);
    if candidate.status == "confirmed" || candidate.status == "ignored" {
        return Ok(candidate);
    }
    candidate.status = "confirmed".to_string();
    candidate.updated_at = now_string();
    let mut created_id = String::new();
    match candidate.candidate_type.as_str() {
        "task" => {
            if !manifest
                .tasks
                .iter()
                .any(|task| normalize_key(&task.title) == normalize_key(&candidate.title))
            {
                let tid = new_id();
                manifest.tasks.push(TaskRecord {
                    id: tid.clone(),
                    title: candidate.title.clone(),
                    status: "todo".to_string(),
                    source_message_id: candidate.related_file_id.clone(),
                    created_at: now_string(),
                    updated_at: now_string(),
                });
                created_id = tid;
            }
        }
        "decision" => {
            if !manifest
                .decisions
                .iter()
                .any(|decision| normalize_key(&decision.summary) == normalize_key(&candidate.title))
            {
                let did = new_id();
                manifest.decisions.push(DecisionRecord {
                    id: did.clone(),
                    summary: candidate.title.clone(),
                    source_message_id: candidate.related_file_id.clone(),
                    created_at: now_string(),
                });
                created_id = did;
            }
        }
        _ => {}
    }
    candidate.applied_record_id = created_id;
    candidate.decision_trace.user_decision = "approved".to_string();
    candidate.decision_trace.user_decision_note = "用户确认行动候选。".to_string();
    candidate.decision_trace.execution = "executed".to_string();
    candidate.decision_trace.execution_note = "候选已确认并更新项目记录。".to_string();
    candidate.decision_trace.updated_at = now_string();
    manifest.project_action_candidates[idx] = candidate.clone();
    resolve_pending_review_for_candidate(&mut manifest, &candidate);
    manifest.audit.push(audit_event(
        "project.candidate.confirm",
        &candidate.id,
        "confirmed",
        candidate.title.clone(),
        false,
        true,
    ));
    persist_project(&root, &manifest, None)?;
    append_audit_event(&root, manifest.audit.last().expect("audit event exists"))?;
    Ok(candidate)
}

pub fn ignore_action_candidate<R: Runtime>(
    _app: &tauri::AppHandle<R>,
    project_root: String,
    candidate_id: String,
) -> Result<ProjectActionCandidate, String> {
    let _guard = project_write_lock()
        .lock()
        .map_err(|_| "项目存储锁已损坏，请重启感冒院。".to_string())?;
    let root = PathBuf::from(&project_root);
    let mut manifest = read_and_repair_manifest(&root)?;
    let idx = manifest
        .project_action_candidates
        .iter()
        .position(|item| item.id == candidate_id)
        .ok_or_else(|| "行动候选不存在。".to_string())?;
    let mut candidate = manifest.project_action_candidates[idx].clone();
    ensure_action_candidate_trace(&mut candidate, &manifest.project.id);
    if candidate.status == "ignored" {
        return Ok(candidate);
    }
    candidate.status = "ignored".to_string();
    candidate.updated_at = now_string();
    candidate.decision_trace.user_decision = "rejected".to_string();
    candidate.decision_trace.user_decision_note = "用户忽略行动候选。".to_string();
    candidate.decision_trace.execution = "cancelled".to_string();
    candidate.decision_trace.execution_note = "未更新项目记录。".to_string();
    candidate.decision_trace.updated_at = now_string();
    manifest.project_action_candidates[idx] = candidate.clone();
    resolve_pending_review_for_candidate(&mut manifest, &candidate);
    manifest.audit.push(audit_event(
        "project.candidate.ignore",
        &candidate.id,
        "ignored",
        candidate.title.clone(),
        false,
        true,
    ));
    persist_project(&root, &manifest, None)?;
    append_audit_event(&root, manifest.audit.last().expect("audit event exists"))?;
    Ok(candidate)
}

pub fn update_action_candidate<R: Runtime>(
    _app: &tauri::AppHandle<R>,
    project_root: String,
    candidate_id: String,
    title: String,
    description: String,
    suggested_priority: String,
    suggested_due_date: String,
) -> Result<ProjectActionCandidate, String> {
    let _guard = project_write_lock()
        .lock()
        .map_err(|_| "项目存储锁已损坏，请重启感冒院。".to_string())?;
    let root = PathBuf::from(&project_root);
    let mut manifest = read_and_repair_manifest(&root)?;
    let idx = manifest
        .project_action_candidates
        .iter()
        .position(|item| item.id == candidate_id)
        .ok_or_else(|| "行动候选不存在。".to_string())?;
    let mut candidate = manifest.project_action_candidates[idx].clone();
    ensure_action_candidate_trace(&mut candidate, &manifest.project.id);
    if !title.trim().is_empty() {
        candidate.title = title.trim().to_string();
    }
    if !description.trim().is_empty() {
        candidate.description = description.trim().to_string();
    }
    if !suggested_priority.trim().is_empty() {
        candidate.suggested_priority = suggested_priority.trim().to_string();
    }
    if !suggested_due_date.trim().is_empty() {
        candidate.suggested_due_date = suggested_due_date.trim().to_string();
    }
    candidate.updated_at = now_string();
    candidate.decision_trace.user_decision = "modified".to_string();
    candidate.decision_trace.user_decision_note = "用户修改了候选内容。".to_string();
    candidate.decision_trace.recommendation = format!(
        "{}；优先级：{}；截止：{}",
        candidate.description, candidate.suggested_priority, candidate.suggested_due_date
    );
    candidate.decision_trace.updated_at = now_string();
    manifest.project_action_candidates[idx] = candidate.clone();
    manifest.audit.push(audit_event(
        "project.candidate.update",
        &candidate.id,
        "updated",
        candidate.title.clone(),
        false,
        true,
    ));
    persist_project(&root, &manifest, None)?;
    append_audit_event(&root, manifest.audit.last().expect("audit event exists"))?;
    Ok(candidate)
}

pub fn get_impact_analyses<R: Runtime>(
    _app: &tauri::AppHandle<R>,
    project_root: String,
) -> Result<Vec<ProjectImpactAnalysis>, String> {
    Ok(load_project(&project_root)?.project_impact_analyses)
}

pub fn get_action_candidates<R: Runtime>(
    _app: &tauri::AppHandle<R>,
    project_root: String,
) -> Result<Vec<ProjectActionCandidate>, String> {
    Ok(load_project(&project_root)?.project_action_candidates)
}

pub fn get_state_proposals<R: Runtime>(
    _app: &tauri::AppHandle<R>,
    project_root: String,
) -> Result<Vec<ProjectStateProposal>, String> {
    Ok(load_project(&project_root)?.project_state_proposals)
}

pub fn get_daily_continue<R: Runtime>(
    _app: &tauri::AppHandle<R>,
    project_root: String,
) -> Result<DailyContinueSnapshot, String> {
    let manifest = load_project(&project_root)?;
    manifest
        .daily_continue_snapshots
        .last()
        .cloned()
        .ok_or_else(|| "暂无今日继续快照，请先归位文件或手动刷新。".to_string())
}

pub fn regenerate_daily_continue<R: Runtime>(
    _app: &tauri::AppHandle<R>,
    project_root: String,
) -> Result<DailyContinueSnapshot, String> {
    let _guard = project_write_lock()
        .lock()
        .map_err(|_| "项目存储锁已损坏，请重启感冒院。".to_string())?;
    let root = PathBuf::from(&project_root);
    let mut manifest = read_and_repair_manifest(&root)?;
    update_daily_continue_snapshot(&mut manifest);
    persist_project(&root, &manifest, None)?;
    Ok(manifest
        .daily_continue_snapshots
        .last()
        .cloned()
        .unwrap_or_default())
}

pub fn get_today_workspace<R: Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<TodayWorkspace, String> {
    build_today_workspace(app, false)
}

pub fn refresh_today_workspace<R: Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<TodayWorkspace, String> {
    build_today_workspace(app, true)
}

pub fn update_continue_project_preference<R: Runtime>(
    app: &tauri::AppHandle<R>,
    project_id: String,
    pinned: Option<bool>,
    snoozed_until: Option<String>,
) -> Result<TodayWorkspace, String> {
    let mut registry = read_registry(app)?;
    if !registry
        .projects
        .iter()
        .any(|project| project.id == project_id)
    {
        return Err("项目不存在，无法更新继续工作偏好。".to_string());
    }
    let preference = if let Some(existing) = registry
        .continue_preferences
        .iter_mut()
        .find(|item| item.project_id == project_id)
    {
        existing
    } else {
        registry
            .continue_preferences
            .push(ContinueProjectPreference {
                project_id: project_id.clone(),
                ..ContinueProjectPreference::default()
            });
        registry
            .continue_preferences
            .last_mut()
            .expect("preference exists")
    };
    if let Some(value) = pinned {
        preference.pinned = value;
    }
    if let Some(value) = snoozed_until {
        preference.snoozed_until = value;
    }
    preference.updated_at = now_string();
    registry
        .continue_preferences
        .retain(|item| item.pinned || !item.snoozed_until.is_empty());
    write_registry(app, &registry)?;
    build_today_workspace(app, false)
}

// Today normally reads durable projections only. Git capture and daily snapshot
// generation are explicit refresh work because they may touch multiple projects.
fn build_today_workspace<R: Runtime>(
    app: &tauri::AppHandle<R>,
    refresh_facts: bool,
) -> Result<TodayWorkspace, String> {
    let _guard = if refresh_facts {
        Some(
            project_write_lock()
                .lock()
                .map_err(|_| "项目存储锁已损坏，请重启感冒院。".to_string())?,
        )
    } else {
        None
    };
    let registry = read_registry(app)?;
    let mut project_manifests = Vec::new();
    for project in registry.projects.iter().take(12) {
        let root = PathBuf::from(&project.root_dir);
        if !root.exists() {
            continue;
        }
        let mut manifest = match read_and_repair_manifest(&root) {
            Ok(value) => value,
            Err(_) => continue,
        };
        if refresh_facts {
            update_daily_continue_snapshot(&mut manifest);
            persist_project(&root, &manifest, None)?;
        }
        project_manifests.push(manifest);
    }
    let mut focus_projects = Vec::new();
    let mut recommended_actions = Vec::new();
    let mut blockers = Vec::new();
    let mut pending_items = Vec::new();
    let mut codex_task_summary = CodexTaskTodaySummary::default();
    let mut activity_timeline = Vec::new();
    let mut pending_actions = Vec::new();
    let mut executions = Vec::new();
    let mut freshness_checks = Vec::new();
    for manifest in &project_manifests {
        let project_root = Path::new(&manifest.project.root_dir);
        let freshness = if refresh_facts {
            refresh_project_git_facts_for_today(project_root, manifest)
        } else {
            read_project_git_facts_for_today(project_root, manifest)
        };
        freshness_checks.push(freshness);
        focus_projects.push(TodayProjectFocus {
            project_id: manifest.project.id.clone(),
            project_name: manifest.project.name.clone(),
            project_root: manifest.project.root_dir.clone(),
            current_status: manifest.project_state_summary.current_phase.clone(),
            recent_change: manifest
                .project_state_summary
                .recent_changes
                .first()
                .cloned()
                .or_else(|| {
                    manifest
                        .project_state_summary
                        .recent_progress
                        .first()
                        .cloned()
                })
                .unwrap_or_default(),
            next_step: manifest.project_state_summary.next_milestone.clone(),
            updated_at: manifest.project.last_opened_at.clone(),
        });
        let ledger_events = if let Ok(ledger) = read_work_ledger(project_root) {
            let timeline = ActivityProjectionAdapter::visible_timeline(&ledger.events);
            for activity in timeline.iter().take(4) {
                if !last_progress_contains(&recommended_actions, &activity.summary) {
                    recommended_actions.push(DailyContinueItem {
                        action: activity.summary.clone(),
                        reason: "来自 Activity Timeline 的真实工作事实。".to_string(),
                        evidence: activity
                            .evidence_refs
                            .iter()
                            .map(evidence_ref_label)
                            .collect(),
                        related_project: manifest.project.name.clone(),
                        related_files: Vec::new(),
                        priority: "medium".to_string(),
                        item_type: "fact".to_string(),
                        source: activity.source_type.clone(),
                        confidence: activity.confidence.clone(),
                        category: "真实进展".to_string(),
                        decision_trace: DecisionTrace::default(),
                    });
                }
            }
            activity_timeline.extend(timeline);
            ledger.events
        } else {
            Vec::new()
        };
        if let Ok(tasks) = read_codex_tasks(project_root) {
            if let Ok(runs) = read_codex_runs(project_root) {
                executions.extend(ExecutionProjectionAdapter::from_codex_runs(&runs, &tasks));
            }
            let project_pending_actions = govern_pending_actions_for_today(
                PendingActionAdapter::from_project(manifest, &tasks, &ledger_events),
            );
            for action in project_pending_actions.iter().take(5) {
                pending_items.push(today_pending_from_projection(
                    &action,
                    &manifest.project.name,
                ));
                if action.priority == "high" {
                    blockers.push(today_blocker_from_projection(
                        &action,
                        &manifest.project.name,
                    ));
                }
            }
            pending_actions.extend(project_pending_actions);
            for task in tasks.into_iter().take(5) {
                match task.status.as_str() {
                    "running" | "awaitingResult" | "resultReceived" | "verifying" => {
                        codex_task_summary.active_or_delivered += 1;
                    }
                    "awaitingAcceptance" => {
                        codex_task_summary.awaiting_acceptance += 1;
                    }
                    "failed" | "cancelled" | "needsReview" => {
                        codex_task_summary.abnormal += 1;
                    }
                    _ => {}
                }
                codex_task_summary.latest_tasks.push(CodexTaskTodayItem {
                    task_id: task.task_id.clone(),
                    project_id: manifest.project.id.clone(),
                    project_name: manifest.project.name.clone(),
                    project_root: manifest.project.root_dir.clone(),
                    title: task_display_title(&task),
                    status: task.status.clone(),
                    git_status: task.git_verification.status.clone(),
                    manual_acceptance_count: task.manual_acceptance.len(),
                    updated_at: task.updated_at.clone(),
                });
            }
        }
    }
    let inbox = read_material_inbox_document(app)?;
    let inbox_actions = PendingActionAdapter::from_inbox(&inbox.items);
    if inbox_actions.len() >= 5 {
        blockers.push(TodayBlocker {
            category: "等待确认".to_string(),
            title: format!("Inbox 有 {} 份资料待处理", inbox_actions.len()),
            related_project: String::new(),
            evidence: inbox_actions
                .iter()
                .take(5)
                .map(|item| item.title.clone())
                .collect(),
        });
    }
    for action in inbox_actions.iter().take(5) {
        pending_items.push(today_pending_from_projection(&action, ""));
    }
    let workspace_activity = build_workspace_activity_summary(app)?;
    let cleanup_actions =
        PendingActionAdapter::from_cleanup(&read_cleanup_plan_document(app)?.plans);
    executions.extend(ExecutionProjectionAdapter::from_cleanup_batches(
        &read_cleanup_execution_document(app)?.batches,
    ));
    for action in cleanup_actions.iter().take(5) {
        pending_items.push(today_pending_from_projection(&action, ""));
    }
    pending_actions.extend(inbox_actions);
    pending_actions.extend(cleanup_actions);
    if workspace_activity.failed_items > 0 || workspace_activity.conflicts > 0 {
        blockers.push(TodayBlocker {
            category: "等待确认".to_string(),
            title: format!(
                "Workspace 有 {} 个失败项、{} 个撤销冲突",
                workspace_activity.failed_items, workspace_activity.conflicts
            ),
            related_project: String::new(),
            evidence: workspace_activity
                .recent_items
                .iter()
                .filter(|item| item.kind == "failed" || item.kind == "undoConflict")
                .map(|item| item.title.clone())
                .take(5)
                .collect(),
        });
    }
    recommended_actions.truncate(3);
    blockers.truncate(8);
    pending_items.truncate(12);
    recommended_actions.sort_by(|left, right| {
        let rank = |value: &str| match value {
            "fact" => 0,
            "suggestion" => 1,
            _ => 2,
        };
        rank(&left.item_type)
            .cmp(&rank(&right.item_type))
            .then_with(|| right.confidence.score.cmp(&left.confidence.score))
    });
    activity_timeline.sort_by(|left, right| right.occurred_at.cmp(&left.occurred_at));
    activity_timeline.truncate(24);
    pending_actions.sort_by(|left, right| right.created_at.cmp(&left.created_at));
    pending_actions.dedup_by(|left, right| left.id == right.id);
    pending_actions.truncate(24);
    let last_progress = project_manifests
        .iter()
        .find_map(|manifest| {
            read_work_ledger(Path::new(&manifest.project.root_dir))
                .ok()
                .and_then(|ledger| {
                    ActivityProjectionAdapter::visible_timeline(&ledger.events)
                        .first()
                        .map(|activity| vec![activity.summary.clone()])
                })
        })
        .unwrap_or_default();
    let status = if project_manifests.is_empty() && workspace_activity.new_managed_files == 0 {
        "empty"
    } else {
        "ready"
    }
    .to_string();
    let continue_work_focus = build_continue_work_focus(
        &project_manifests,
        &focus_projects,
        &activity_timeline,
        &pending_actions,
        &codex_task_summary,
        &workspace_activity,
        &freshness_checks,
    );
    let continue_projects = build_continue_project_recommendations(
        &project_manifests,
        &focus_projects,
        &pending_actions,
        &activity_timeline,
        &freshness_checks,
        &read_registry(app)?.continue_preferences,
    );
    // Keep the secondary project list on the same derived facts as Continue
    // Work. The persisted project_state_summary is historical and can retain
    // an obsolete repository-scan prompt after reviews are cleared.
    for project in &mut focus_projects {
        if let Some(recommendation) = continue_projects
            .iter()
            .find(|item| item.project_id == project.project_id)
        {
            if let Some(focus) = &recommendation.focus {
                project.next_step = focus.summary.clone();
            } else if !project.recent_change.trim().is_empty() {
                project.next_step = project.recent_change.clone();
            } else {
                project.next_step = "当前没有待处理事项。".to_string();
            }
        } else if !project.recent_change.trim().is_empty() {
            // Do not fall back to persisted nextMilestone when the current
            // fact set has no actionable focus.
            project.next_step = project.recent_change.clone();
        } else {
            project.next_step = "当前没有待处理事项。".to_string();
        }
    }
    Ok(TodayWorkspace {
        generated_at: now_string(),
        continue_work_focus,
        last_progress,
        focus_projects: focus_projects.into_iter().take(3).collect(),
        recommended_actions,
        blockers,
        pending_items,
        workspace_activity,
        codex_task_summary,
        activity_timeline,
        pending_actions,
        executions,
        status,
        continue_projects,
    })
}

fn build_continue_project_recommendations(
    manifests: &[ProjectManifest],
    focus_projects: &[TodayProjectFocus],
    pending_actions: &[PendingActionProjection],
    activities: &[ActivityProjection],
    freshness_checks: &[TodayFreshnessCheck],
    preferences: &[ContinueProjectPreference],
) -> Vec<ContinueProjectRecommendation> {
    let mut items = manifests
        .iter()
        .filter_map(|manifest| {
            let project_id = &manifest.project.id;
            let preference = preferences
                .iter()
                .find(|item| item.project_id == *project_id);
            let snoozed = preference
                .and_then(|item| item.snoozed_until.parse::<u128>().ok())
                .map(|until| until > now_string().parse::<u128>().unwrap_or_default())
                .unwrap_or(false);
            let pinned = preference.map(|item| item.pinned).unwrap_or(false);
            if snoozed && !pinned {
                return None;
            }
            let actions = pending_actions
                .iter()
                .filter(|action| action.project_id == *project_id)
                .collect::<Vec<_>>();
            let focus = build_continue_work_focus(
                std::slice::from_ref(manifest),
                &focus_projects
                    .iter()
                    .filter(|item| item.project_id == *project_id)
                    .cloned()
                    .collect::<Vec<_>>(),
                &activities
                    .iter()
                    .filter(|item| item.project_id == *project_id)
                    .cloned()
                    .collect::<Vec<_>>(),
                &actions
                    .iter()
                    .map(|action| (*action).clone())
                    .collect::<Vec<_>>(),
                &CodexTaskTodaySummary::default(),
                &WorkspaceActivitySummary::default(),
                &freshness_checks
                    .iter()
                    .filter(|item| item.project_id == *project_id)
                    .cloned()
                    .collect::<Vec<_>>(),
            );
            let focus = focus.filter(|item| !item.title.contains("没有足够事实"));
            let recent_change = activities
                .iter()
                .find(|item| item.project_id == *project_id)
                .map(|item| item.summary.clone())
                .or_else(|| {
                    manifest
                        .project_state_summary
                        .recent_changes
                        .first()
                        .cloned()
                })
                .unwrap_or_default();
            let blocker = manifest
                .project_state_summary
                .blockers
                .first()
                .cloned()
                .unwrap_or_default();
            let mut score = if focus.is_some() { 40 } else { 0 };
            let mut basis = Vec::new();
            if pinned {
                score += 100;
                basis.push("用户固定".to_string());
            }
            if !actions.is_empty() {
                score += 50;
                basis.push("存在有效待处理事项".to_string());
            }
            if actions.iter().any(|action| {
                matches!(
                    action.action_type.as_str(),
                    "codexAcceptance" | "codexReview"
                )
            }) {
                score += 25;
                basis.push("Codex结果等待处理".to_string());
            }
            if !blocker.is_empty() {
                score += 20;
                basis.push("存在项目阻塞".to_string());
            }
            if !recent_change.is_empty() {
                score += 10;
                basis.push("最近有真实变化".to_string());
            }
            if focus.is_none() && !pinned {
                return None;
            }
            let reason = if pinned {
                "这是你固定优先处理的项目。".to_string()
            } else if !actions.is_empty() {
                "当前有需要你处理的有效事项。".to_string()
            } else if !recent_change.is_empty() {
                "项目最近有真实变化。".to_string()
            } else {
                "当前项目仍有可恢复的工作焦点。".to_string()
            };
            Some((
                score,
                ContinueProjectRecommendation {
                    project_id: project_id.clone(),
                    project_name: manifest.project.name.clone(),
                    project_root: manifest.project.root_dir.clone(),
                    focus,
                    reason,
                    recent_change,
                    blocker,
                    priority: if score >= 80 {
                        "P0".to_string()
                    } else {
                        "P1".to_string()
                    },
                    score_basis: basis,
                    pinned,
                    snoozed,
                },
            ))
        })
        .collect::<Vec<_>>();
    items.sort_by(|left, right| right.0.cmp(&left.0));
    items.into_iter().take(3).map(|(_, item)| item).collect()
}

#[derive(Debug, Clone, Default)]
struct TodayFreshnessCheck {
    project_id: String,
    project_root: String,
    status: String,
    reason: String,
    evidence_refs: Vec<EvidenceRef>,
}

fn refresh_project_git_facts_for_today(
    root: &Path,
    manifest: &ProjectManifest,
) -> TodayFreshnessCheck {
    let previous = read_git_snapshot(root).ok().flatten();
    let repo_path = configured_repository_path(&manifest.project)
        .map(|path| path_to_string(&path))
        .or_else(|| {
            previous
                .as_ref()
                .map(|snapshot| snapshot.repository_path.trim().to_string())
                .filter(|value| !value.is_empty())
        })
        .or_else(|| root.join(".git").is_dir().then(|| path_to_string(root)));
    let Some(repo_path) = repo_path else {
        return TodayFreshnessCheck {
            project_id: manifest.project.id.clone(),
            project_root: manifest.project.root_dir.clone(),
            status: "notApplicable".to_string(),
            reason: "当前项目没有关联代码仓库，Git 事实不适用。".to_string(),
            evidence_refs: vec![EvidenceRef {
                kind: "project".to_string(),
                id: manifest.project.id.clone(),
                label: "项目未关联代码仓库".to_string(),
                ..EvidenceRef::default()
            }],
        };
    };
    let repo = PathBuf::from(&repo_path);
    match repo
        .canonicalize()
        .map_err(|err| format!("Git 仓库路径不可用：{err}"))
        .and_then(|canonical| capture_git_snapshot(&manifest.project.id, &canonical))
    {
        Ok(snapshot) => {
            let previous_head = previous.as_ref().map(|item| item.head.as_str());
            let freshness_status = if previous_head == Some(snapshot.head.as_str()) {
                "fresh"
            } else {
                "fresh"
            };
            if let Err(err) = write_git_snapshot(root, &snapshot)
                .and_then(|_| {
                    record_git_snapshot_events(
                        root,
                        &manifest.project.id,
                        previous.as_ref(),
                        &snapshot,
                    )
                })
                .and_then(|_| refresh_codex_task_git_verifications(root, &snapshot.repository_path))
            {
                return TodayFreshnessCheck {
                    project_id: manifest.project.id.clone(),
                    project_root: manifest.project.root_dir.clone(),
                    status: "stale".to_string(),
                    reason: format!("Git 事实刷新后写入失败：{err}"),
                    evidence_refs: vec![EvidenceRef {
                        kind: "git".to_string(),
                        id: snapshot.head_short,
                        label: "Git 事实写入失败".to_string(),
                        ..EvidenceRef::default()
                    }],
                };
            }
            TodayFreshnessCheck {
                project_id: manifest.project.id.clone(),
                project_root: manifest.project.root_dir.clone(),
                status: freshness_status.to_string(),
                reason: format!("Git 事实已刷新到 {}。", snapshot.head_short),
                evidence_refs: vec![EvidenceRef {
                    kind: "git".to_string(),
                    id: snapshot.head.clone(),
                    label: format!("Git HEAD {}", snapshot.head_short),
                    ..EvidenceRef::default()
                }],
            }
        }
        Err(err) => TodayFreshnessCheck {
            project_id: manifest.project.id.clone(),
            project_root: manifest.project.root_dir.clone(),
            status: if previous.is_some() {
                "stale"
            } else {
                "unknown"
            }
            .to_string(),
            reason: err,
            evidence_refs: vec![EvidenceRef {
                kind: "git".to_string(),
                id: repo_path,
                label: "Git 事实刷新失败".to_string(),
                ..EvidenceRef::default()
            }],
        },
    }
}

fn read_project_git_facts_for_today(
    root: &Path,
    manifest: &ProjectManifest,
) -> TodayFreshnessCheck {
    let previous = read_git_snapshot(root).ok().flatten();
    let repo_path = configured_repository_path(&manifest.project)
        .map(|path| path_to_string(&path))
        .or_else(|| {
            previous
                .as_ref()
                .map(|snapshot| snapshot.repository_path.trim().to_string())
                .filter(|value| !value.is_empty())
        })
        .or_else(|| root.join(".git").is_dir().then(|| path_to_string(root)));
    let Some(repo_path) = repo_path else {
        return TodayFreshnessCheck {
            project_id: manifest.project.id.clone(),
            project_root: manifest.project.root_dir.clone(),
            status: "notApplicable".to_string(),
            reason: "当前项目没有关联代码仓库，Git 事实不适用。".to_string(),
            evidence_refs: vec![EvidenceRef {
                kind: "project".to_string(),
                id: manifest.project.id.clone(),
                label: "项目未关联代码仓库".to_string(),
                ..EvidenceRef::default()
            }],
        };
    };
    let repository_available = PathBuf::from(&repo_path).is_dir();
    match previous {
        Some(snapshot) if repository_available => TodayFreshnessCheck {
            project_id: manifest.project.id.clone(),
            project_root: manifest.project.root_dir.clone(),
            status: "fresh".to_string(),
            reason: format!("已读取上次核验的 Git HEAD {}。", snapshot.head_short),
            evidence_refs: vec![EvidenceRef {
                kind: "git".to_string(),
                id: snapshot.head,
                label: format!("Git HEAD {}", snapshot.head_short),
                ..EvidenceRef::default()
            }],
        },
        Some(snapshot) => TodayFreshnessCheck {
            project_id: manifest.project.id.clone(),
            project_root: manifest.project.root_dir.clone(),
            status: "stale".to_string(),
            reason: "上次核验的代码仓库当前不可用，请刷新项目事实。".to_string(),
            evidence_refs: vec![EvidenceRef {
                kind: "git".to_string(),
                id: snapshot.head,
                label: "Git 仓库当前不可用".to_string(),
                ..EvidenceRef::default()
            }],
        },
        None if repository_available => TodayFreshnessCheck {
            project_id: manifest.project.id.clone(),
            project_root: manifest.project.root_dir.clone(),
            status: "unknown".to_string(),
            reason: "尚未读取关联代码仓库的 Git 事实，请刷新项目事实。".to_string(),
            evidence_refs: vec![EvidenceRef {
                kind: "git".to_string(),
                id: repo_path,
                label: "尚未核验 Git 事实".to_string(),
                ..EvidenceRef::default()
            }],
        },
        None => TodayFreshnessCheck {
            project_id: manifest.project.id.clone(),
            project_root: manifest.project.root_dir.clone(),
            status: "unknown".to_string(),
            reason: "关联代码仓库当前不可用。".to_string(),
            evidence_refs: vec![EvidenceRef {
                kind: "git".to_string(),
                id: repo_path,
                label: "Git 仓库当前不可用".to_string(),
                ..EvidenceRef::default()
            }],
        },
    }
}

fn govern_pending_actions_for_today(
    actions: Vec<PendingActionProjection>,
) -> Vec<PendingActionProjection> {
    let mut governed = Vec::new();
    for action in actions {
        if pending_action_is_low_value_history(&action) {
            continue;
        }
        if governed.iter().any(|existing: &PendingActionProjection| {
            normalize_key(&existing.title) == normalize_key(&action.title)
                && existing.action_type == action.action_type
        }) {
            continue;
        }
        governed.push(action);
    }
    governed.sort_by(|left, right| {
        focus_action_rank(right)
            .cmp(&focus_action_rank(left))
            .then_with(|| right.created_at.cmp(&left.created_at))
    });
    governed
}

fn pending_action_is_low_value_history(action: &PendingActionProjection) -> bool {
    let title = action.title.to_ascii_lowercase();
    let reason = action.reason.to_ascii_lowercase();
    let text = format!("{title}\n{reason}");
    text.contains("app.exe")
        || text.contains(".exe")
        || text.contains("target\\")
        || text.contains("target/")
        || text.contains("node_modules")
        || text.contains("原始资料缺失")
        || (text.contains("需要检查") && action.evidence_refs.is_empty())
}

fn build_continue_work_focus(
    project_manifests: &[ProjectManifest],
    focus_projects: &[TodayProjectFocus],
    activity_timeline: &[ActivityProjection],
    pending_actions: &[PendingActionProjection],
    codex_task_summary: &CodexTaskTodaySummary,
    workspace_activity: &WorkspaceActivitySummary,
    freshness_checks: &[TodayFreshnessCheck],
) -> Option<ContinueWorkFocus> {
    if project_manifests.is_empty() && activity_timeline.is_empty() && pending_actions.is_empty() {
        return Some(empty_continue_work_focus(now_string()));
    }
    if let Some(check) = freshness_checks
        .iter()
        .find(|check| freshness_requires_user_refresh(check))
    {
        return Some(ContinueWorkFocus {
            project_id: check.project_id.clone(),
            title: "项目事实需要刷新".to_string(),
            summary: "当前 Git / Codex / 项目状态事实可能不是最新，先刷新事实再判断下一步。"
                .to_string(),
            reason: check.reason.clone(),
            priority: "P0".to_string(),
            confidence: confidence_from_score(96),
            freshness_status: check.status.clone(),
            evidence_refs: check.evidence_refs.clone(),
            source_action_ids: Vec::new(),
            primary_action: ContinueWorkPrimaryAction {
                action_type: "refreshFacts".to_string(),
                label: "刷新项目事实".to_string(),
                project_id: check.project_id.clone(),
                project_root: check.project_root.clone(),
                ..ContinueWorkPrimaryAction::default()
            },
            generated_at: now_string(),
        });
    }
    if let Some(action) = pending_actions.iter().max_by(|left, right| {
        focus_action_rank(left)
            .cmp(&focus_action_rank(right))
            .then_with(|| left.created_at.cmp(&right.created_at))
    }) {
        let project = focus_projects
            .iter()
            .find(|project| project.project_id == action.project_id);
        return Some(ContinueWorkFocus {
            project_id: action.project_id.clone(),
            title: focus_title_from_action(action),
            summary: action.reason.clone(),
            reason: focus_reason_from_action(action),
            priority: focus_priority_label(action),
            confidence: confidence_from_score(if action.priority == "high" { 94 } else { 82 }),
            freshness_status: "fresh".to_string(),
            evidence_refs: ensure_focus_evidence(action.evidence_refs.clone(), action),
            source_action_ids: vec![action.id.clone()],
            primary_action: primary_action_for_pending(action, project),
            generated_at: now_string(),
        });
    }
    if let Some(activity) = activity_timeline.first() {
        return Some(ContinueWorkFocus {
            project_id: activity.project_id.clone(),
            title: "查看最近真实进展".to_string(),
            summary: activity.summary.clone(),
            reason: "已有最近活动事实，但当前没有足够证据确定唯一业务下一步。".to_string(),
            priority: "P2".to_string(),
            confidence: confidence_from_score(72),
            freshness_status: "fresh".to_string(),
            evidence_refs: activity.evidence_refs.clone(),
            source_action_ids: vec![activity.id.clone()],
            primary_action: ContinueWorkPrimaryAction {
                action_type: "openProject".to_string(),
                label: "打开项目".to_string(),
                project_id: activity.project_id.clone(),
                project_root: focus_projects
                    .iter()
                    .find(|project| project.project_id == activity.project_id)
                    .map(|project| project.project_root.clone())
                    .unwrap_or_default(),
                ..ContinueWorkPrimaryAction::default()
            },
            generated_at: now_string(),
        });
    }
    if codex_task_summary.active_or_delivered > 0
        || codex_task_summary.awaiting_acceptance > 0
        || codex_task_summary.abnormal > 0
        || workspace_activity.pending_cleanup_plans > 0
    {
        return Some(ContinueWorkFocus {
            title: "查看待处理事项".to_string(),
            summary: "当前存在待处理状态，但证据不足以安全收敛为唯一下一步。".to_string(),
            reason: "系统不会根据不完整事实猜测业务优先级。".to_string(),
            priority: "P2".to_string(),
            confidence: confidence_from_score(65),
            freshness_status: "fresh".to_string(),
            evidence_refs: vec![EvidenceRef {
                kind: "today".to_string(),
                label: "Today 待处理汇总".to_string(),
                ..EvidenceRef::default()
            }],
            primary_action: ContinueWorkPrimaryAction {
                action_type: "openPendingAction".to_string(),
                label: "查看待处理事项".to_string(),
                ..ContinueWorkPrimaryAction::default()
            },
            generated_at: now_string(),
            ..ContinueWorkFocus::default()
        });
    }
    Some(empty_continue_work_focus(now_string()))
}

fn freshness_requires_user_refresh(check: &TodayFreshnessCheck) -> bool {
    check.status == "stale"
        || (check.status == "unknown"
            && check.reason.contains("Git")
            && !check.evidence_refs.is_empty())
}

fn empty_continue_work_focus(generated_at: String) -> ContinueWorkFocus {
    ContinueWorkFocus {
        title: "当前没有足够事实确定唯一下一步".to_string(),
        summary: "暂未发现可靠的当前焦点。可以先查看待处理事项或打开最近项目。".to_string(),
        reason: "没有高价值 PendingAction、最新 Activity 或可验证的项目状态依据。".to_string(),
        priority: "P2".to_string(),
        confidence: confidence_from_score(55),
        freshness_status: "unknown".to_string(),
        evidence_refs: Vec::new(),
        source_action_ids: Vec::new(),
        primary_action: ContinueWorkPrimaryAction {
            action_type: "openPendingAction".to_string(),
            label: "查看待处理事项".to_string(),
            ..ContinueWorkPrimaryAction::default()
        },
        generated_at,
        ..ContinueWorkFocus::default()
    }
}

fn focus_action_rank(action: &PendingActionProjection) -> u8 {
    match action.action_type.as_str() {
        "codexReview" => 90,
        "codexAcceptance" => 82,
        "projectStateConfirmation" => 74,
        "projectAttention" => 70,
        "inboxReview" => 62,
        "cleanupReview" => 58,
        "userBlocker" => 56,
        "userNextAction" => 48,
        "projectReview" if action.priority == "high" => 54,
        "projectReview" => 42,
        _ if action.priority == "high" => 50,
        _ => 30,
    }
}

fn focus_priority_label(action: &PendingActionProjection) -> String {
    match focus_action_rank(action) {
        80..=u8::MAX => "P0",
        60..=79 => "P1",
        _ => "P2",
    }
    .to_string()
}

fn focus_title_from_action(action: &PendingActionProjection) -> String {
    if action.action_type == "codexAcceptance" {
        return "验收 Codex 交付结果".to_string();
    }
    if action.action_type == "codexReview" {
        return "检查 Codex 任务结果".to_string();
    }
    first_line(&action.title, 42)
}

fn focus_reason_from_action(action: &PendingActionProjection) -> String {
    match action.action_type.as_str() {
        "codexReview" => "该 Codex 任务存在失败或需检查状态，会阻塞继续信任后续结果。".to_string(),
        "codexAcceptance" => "Codex 已交付结果，下一步需要用户完成真实验收。".to_string(),
        "projectStateConfirmation" => "项目状态变化需要用户确认后才能作为事实使用。".to_string(),
        "inboxReview" => "资料已进入 Inbox，但仍需要确认归属或处理结果。".to_string(),
        "cleanupReview" => "整理方案还未审核，执行前必须由用户确认。".to_string(),
        _ if !action.reason.trim().is_empty() => action.reason.clone(),
        _ => "该事项有真实记录依据，等待处理。".to_string(),
    }
}

fn primary_action_for_pending(
    action: &PendingActionProjection,
    project: Option<&TodayProjectFocus>,
) -> ContinueWorkPrimaryAction {
    let (action_type, label, panel) = match action.action_type.as_str() {
        "codexAcceptance" => ("acceptCodexResult", "去验收", "codex"),
        "codexReview" => ("reviewCodexTask", "查看问题", "codex"),
        "inboxReview" => ("openPendingAction", "查看 Inbox", "inbox"),
        "cleanupReview" => ("openPendingAction", "查看整理方案", "inbox"),
        "userBlocker" => ("openPendingAction", "处理阻塞", "work-capture"),
        "userNextAction" => ("openPendingAction", "开始下一步", "work-capture"),
        _ => ("openPendingAction", "查看待处理", ""),
    };
    ContinueWorkPrimaryAction {
        action_type: action_type.to_string(),
        label: label.to_string(),
        project_id: action.project_id.clone(),
        project_root: project
            .map(|project| project.project_root.clone())
            .unwrap_or_default(),
        source_id: action.source_ref.clone(),
        panel: panel.to_string(),
    }
}

fn ensure_focus_evidence(
    mut evidence_refs: Vec<EvidenceRef>,
    action: &PendingActionProjection,
) -> Vec<EvidenceRef> {
    if evidence_refs.is_empty() {
        evidence_refs.push(EvidenceRef {
            kind: action.action_type.clone(),
            id: action.source_ref.clone(),
            label: first_line(&action.title, 80),
            ..EvidenceRef::default()
        });
    }
    evidence_refs.truncate(3);
    evidence_refs
}

fn confidence_from_score(score: u8) -> DecisionTraceConfidence {
    let level = if score >= 80 {
        "high"
    } else if score >= 55 {
        "medium"
    } else {
        "low"
    };
    DecisionTraceConfidence {
        score,
        level: level.to_string(),
        display: confidence_display_from_score(score),
    }
}

fn last_progress_contains(actions: &[DailyContinueItem], summary: &str) -> bool {
    actions.iter().any(|item| item.action == summary)
}

fn evidence_ref_label(reference: &EvidenceRef) -> String {
    if !reference.label.trim().is_empty() {
        reference.label.clone()
    } else if !reference.id.trim().is_empty() {
        format!("{}:{}", reference.kind, reference.id)
    } else {
        reference.path_snapshot.clone()
    }
}

fn today_pending_from_projection(
    action: &PendingActionProjection,
    project_name: &str,
) -> TodayPendingItem {
    TodayPendingItem {
        kind: action.action_type.clone(),
        title: action.title.clone(),
        related_project: project_name.to_string(),
        source_id: action.source_ref.clone(),
    }
}

fn today_blocker_from_projection(
    action: &PendingActionProjection,
    project_name: &str,
) -> TodayBlocker {
    TodayBlocker {
        category: "等待确认".to_string(),
        title: action.title.clone(),
        related_project: project_name.to_string(),
        evidence: action
            .evidence_refs
            .iter()
            .map(evidence_ref_label)
            .collect(),
    }
}

fn build_workspace_activity_summary<R: Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<WorkspaceActivitySummary, String> {
    let global = read_global_files_document(app)?;
    let plans = read_cleanup_plan_document(app)?;
    let executions = read_cleanup_execution_document(app)?;
    Ok(workspace_activity_summary_from_documents(
        &global,
        &plans,
        &executions,
    ))
}

fn workspace_activity_summary_from_documents(
    global: &GlobalFilesDocument,
    plans: &CleanupPlanDocument,
    executions: &CleanupExecutionDocument,
) -> WorkspaceActivitySummary {
    let mut summary = WorkspaceActivitySummary::default();

    let mut seen_files = HashSet::new();
    let mut active_files = global
        .files
        .iter()
        .filter(|file| file.undone_at.trim().is_empty())
        .filter(|file| {
            let key = if file.content_hash.trim().is_empty() {
                normalize_key(&file.managed_path)
            } else {
                normalize_key(&file.content_hash)
            };
            !key.is_empty() && seen_files.insert(key)
        })
        .collect::<Vec<_>>();
    active_files.sort_by(|left, right| right.created_at.cmp(&left.created_at));
    summary.new_managed_files = active_files.len();
    for file in &active_files {
        let projection = FileProjectionAdapter::from_global_file(file);
        let lifecycle = projection.metadata.lifecycle_status.clone();
        match projection.location.ownership_type.as_str() {
            "existingProject" | "existingProjectMaterial" => summary.project_files += 1,
            "generalWorkMaterial" => summary.general_files += 1,
            "temporaryOrReference" => summary.temporary_files += 1,
            _ if !projection.location.project_id.trim().is_empty() => summary.project_files += 1,
            _ => {}
        }
        if lifecycle == "versioned" || looks_like_versioned_file(&projection.identity.original_name)
        {
            summary.versioned_files += 1;
        }
        summary.recent_items.push(WorkspaceActivityItem {
            kind: lifecycle.clone(),
            title: projection.identity.original_name.clone(),
            summary: workspace_file_recent_status(file, &lifecycle),
            project_name: projection.location.project_id.clone(),
            category: projection.metadata.category.clone(),
            managed_path: projection.location.managed_path.clone(),
            decision_trace_id: file.decision_trace_id.clone(),
            occurred_at: file.created_at.clone(),
        });
    }

    summary.pending_cleanup_plans = plans
        .plans
        .iter()
        .filter(|plan| plan.status == "draft" || plan.status == "reviewing")
        .filter(|plan| {
            plan.items
                .iter()
                .any(|item| item.review_status == "pending" || item.review_status.is_empty())
        })
        .count();

    for batch in &executions.batches {
        for item in &batch.items {
            match item.status.as_str() {
                "failed" => {
                    summary.failed_items += 1;
                    summary.recent_items.push(WorkspaceActivityItem {
                        kind: "failed".to_string(),
                        title: Path::new(&item.source_path)
                            .file_name()
                            .and_then(|value| value.to_str())
                            .unwrap_or(&item.source_path)
                            .to_string(),
                        summary: non_empty_or_pending(&item.error),
                        managed_path: item.target_path.clone(),
                        occurred_at: batch.completed_at.clone(),
                        ..WorkspaceActivityItem::default()
                    });
                }
                "undoConflict" => {
                    summary.conflicts += 1;
                    summary.recent_items.push(WorkspaceActivityItem {
                        kind: "undoConflict".to_string(),
                        title: Path::new(&item.target_path)
                            .file_name()
                            .and_then(|value| value.to_str())
                            .unwrap_or(&item.target_path)
                            .to_string(),
                        summary: non_empty_or_pending(&item.error),
                        managed_path: item.target_path.clone(),
                        occurred_at: batch.completed_at.clone(),
                        ..WorkspaceActivityItem::default()
                    });
                }
                _ => {}
            }
        }
    }

    summary.recent_items.sort_by(|left, right| {
        right
            .occurred_at
            .cmp(&left.occurred_at)
            .then_with(|| left.title.cmp(&right.title))
    });
    summary.recent_items.truncate(8);
    summary
}

fn looks_like_versioned_file(file_name: &str) -> bool {
    explicit_version_number(
        Path::new(file_name)
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or(file_name),
    )
    .is_some()
}

pub fn update_project_attention<R: Runtime>(
    _app: &tauri::AppHandle<R>,
    project_root: String,
    attention_id: String,
    status: String,
) -> Result<ProjectAttention, String> {
    if !matches!(status.as_str(), "confirmed" | "ignored" | "later") {
        return Err("提醒状态无效。".to_string());
    }
    let _guard = project_write_lock()
        .lock()
        .map_err(|_| "项目存储锁已损坏，请重启感冒院。".to_string())?;
    let root = PathBuf::from(&project_root);
    let mut manifest = read_and_repair_manifest(&root)?;
    let attention = manifest
        .project_attentions
        .iter_mut()
        .find(|item| item.id == attention_id)
        .ok_or_else(|| "未找到该项目提醒。".to_string())?;
    attention.status = status.clone();
    attention.updated_at = now_string();
    match status.as_str() {
        "confirmed" => {
            attention.decision_trace.user_decision = "approved".to_string();
            attention.decision_trace.execution = "pending".to_string();
        }
        "ignored" => {
            attention.decision_trace.user_decision = "rejected".to_string();
            attention.decision_trace.execution = "cancelled".to_string();
        }
        _ => {
            attention.decision_trace.user_decision = "pending".to_string();
            attention.decision_trace.execution = "pending".to_string();
        }
    }
    attention.decision_trace.user_decision_note = match status.as_str() {
        "confirmed" => "用户确认该提醒需要处理。",
        "ignored" => "用户忽略该提醒。",
        _ => "用户选择稍后处理。",
    }
    .to_string();
    attention.decision_trace.updated_at = now_string();
    let result = attention.clone();
    manifest.audit.push(audit_event(
        "project.attention",
        &attention_id,
        &status,
        result.title.clone(),
        true,
        true,
    ));
    persist_project(&root, &manifest, None)?;
    append_audit_event(&root, manifest.audit.last().expect("audit event exists"))?;
    Ok(result)
}

pub fn update_data_health_status(
    project_root: String,
    record_id: String,
    status: String,
) -> Result<ProjectManifest, String> {
    if !matches!(status.as_str(), "open" | "resolved" | "ignored") {
        return Err("数据健康状态无效。".to_string());
    }
    let _guard = project_write_lock()
        .lock()
        .map_err(|_| "项目存储锁已损坏，请重启感冒院。".to_string())?;
    let root = PathBuf::from(&project_root);
    let mut manifest = read_and_repair_manifest(&root)?;
    let record = manifest
        .data_health_records
        .iter_mut()
        .find(|record| record.id == record_id)
        .ok_or_else(|| "未找到数据健康记录。".to_string())?;
    record.status = status.clone();
    manifest.audit.push(audit_event(
        "dataHealth.status",
        &record_id,
        &status,
        format!("数据健康记录更新为 {status}"),
        false,
        false,
    ));
    update_daily_continue_snapshot(&mut manifest);
    persist_project(&root, &manifest, None)?;
    Ok(manifest)
}

pub fn update_pending_review_status(
    project_root: String,
    review_id: String,
    status: String,
) -> Result<ProjectManifest, String> {
    if !matches!(status.as_str(), "open" | "pending" | "resolved" | "ignored") {
        return Err("待检查状态无效。".to_string());
    }
    let _guard = project_write_lock()
        .lock()
        .map_err(|_| "项目存储锁已损坏，请重启感冒院。".to_string())?;
    let root = PathBuf::from(&project_root);
    let mut manifest = read_and_repair_manifest(&root)?;
    let review = manifest
        .pending_reviews
        .iter_mut()
        .find(|review| review.id == review_id || review.key == review_id)
        .ok_or_else(|| "未找到待检查事项。".to_string())?;
    review.status = status.clone();
    review.updated_at = now_string();
    let title = review.title.clone();
    manifest.monitoring.pending_count = manifest
        .pending_reviews
        .iter()
        .filter(|item| item.status != "resolved" && item.status != "ignored")
        .count();
    manifest.audit.push(audit_event(
        "pendingReview.status",
        &review_id,
        &status,
        format!("待检查事项更新为 {status}：{title}"),
        false,
        true,
    ));
    update_daily_continue_snapshot(&mut manifest);
    persist_project(&root, &manifest, None)?;
    append_audit_event(&root, manifest.audit.last().expect("audit event exists"))?;
    Ok(manifest)
}

pub fn mark_today_done<R: Runtime>(
    _app: &tauri::AppHandle<R>,
    project_root: String,
    completed: String,
    next_step: String,
) -> Result<FinishWorkResult, String> {
    let _guard = project_write_lock()
        .lock()
        .map_err(|_| "项目存储锁已损坏，请重启感冒院。".to_string())?;
    let root = PathBuf::from(&project_root);
    let mut manifest = read_and_repair_manifest(&root)?;
    let session_id = new_id();
    let point_id = new_id();
    let date_key = current_date_key();
    manifest.daily_sessions.push(DailySession {
        id: session_id.clone(),
        date_key,
        started_at: now_string(),
        updated_at: now_string(),
        message_ids: Vec::new(),
        task_ids: Vec::new(),
        decision_ids: Vec::new(),
        artifact_ids: Vec::new(),
        imported_file_ids: Vec::new(),
        recovery_point_ids: vec![point_id.clone()],
    });
    let recovery_point = RecoveryPoint {
        id: point_id.clone(),
        project_id: manifest.project.id.clone(),
        completed: completed.clone(),
        next_step: next_step.clone(),
        created_at: now_string(),
        message_count: manifest.messages.len(),
        file_count: manifest.files.len(),
    };
    manifest.recovery_points.push(recovery_point.clone());
    manifest.project.next_step = next_step.clone();
    if let Some(session) = manifest
        .daily_sessions
        .iter_mut()
        .find(|session| session.id == session_id)
    {
        session.updated_at = now_string();
    }
    manifest.audit.push(audit_event(
        "project.mark_today_done",
        &session_id,
        "completed",
        completed,
        false,
        true,
    ));
    persist_project(&root, &manifest, None)?;
    append_audit_event(&root, manifest.audit.last().expect("audit event exists"))?;
    Ok(FinishWorkResult {
        project: manifest.project.clone(),
        message: WorkspaceMessage::default(),
        recovery_point,
    })
}

pub fn set_state_auto_apply<R: Runtime>(
    _app: &tauri::AppHandle<R>,
    project_root: String,
    enabled: bool,
) -> Result<ProjectManifest, String> {
    let _guard = project_write_lock()
        .lock()
        .map_err(|_| "项目存储锁已损坏，请重启感冒院。".to_string())?;
    let root = PathBuf::from(&project_root);
    let mut manifest = read_and_repair_manifest(&root)?;
    manifest.project_state_auto_apply = enabled;
    persist_project(&root, &manifest, None)?;
    Ok(manifest)
}

fn failed_inbox_item(
    source_path: &str,
    received_from: &str,
    error_code: &str,
    failed_stage: &str,
    error_message: &str,
) -> MaterialInboxItem {
    let now = now_string();
    let file_name = Path::new(source_path)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(source_path)
        .to_string();
    let mut item = MaterialInboxItem {
        id: new_id(),
        file_name,
        source_path: source_path.to_string(),
        received_at: now.clone(),
        received_from: received_from.to_string(),
        created_at: now.clone(),
        updated_at: now.clone(),
        received_count: 1,
        failed_stage: failed_stage.to_string(),
        confidence_level: "low".to_string(),
        confidence_reasons: vec!["文件接收或读取失败，不能继续自动处理。".to_string()],
        source_history: vec![InboxSourceEvent {
            source_path: source_path.to_string(),
            received_from: received_from.to_string(),
            received_at: now,
            hash: String::new(),
        }],
        ..MaterialInboxItem::default()
    };
    let _ = inbox_routing::transition(
        &mut item,
        inbox_routing::RECEIVED,
        "文件路径已进入 Inbox。",
        "system",
        None,
    );
    let _ = inbox_routing::transition(
        &mut item,
        inbox_routing::FAILED,
        "接收阶段失败，记录保留并可重试。",
        "system",
        Some((error_code, error_message)),
    );
    item.result_note = error_message.to_string();
    item
}

#[cfg(test)]
fn build_material_inbox_item(
    projects: &[(ProjectSummary, ProjectManifest)],
    source_path: &str,
    source_hash: &str,
    size_bytes: u64,
    analysis: &FileAnalysis,
    received_from: &str,
) -> Result<MaterialInboxItem, String> {
    let mut item = build_material_inbox_item_with_history(
        projects,
        &[],
        source_path,
        source_hash,
        size_bytes,
        analysis,
        received_from,
    )?;
    sync_inbox_decision_traces(&mut item);
    Ok(item)
}

fn build_material_inbox_item_with_history(
    projects: &[(ProjectSummary, ProjectManifest)],
    general_history: &[GlobalManagedFile],
    source_path: &str,
    source_hash: &str,
    size_bytes: u64,
    analysis: &FileAnalysis,
    received_from: &str,
) -> Result<MaterialInboxItem, String> {
    let now = now_string();
    let suggested_category = if analysis.recommended_category.is_empty() {
        classify_file(&analysis.file_name.rsplit('.').next().unwrap_or("unknown"))
    } else {
        analysis.recommended_category.clone()
    };
    let suggested_stem = analysis
        .main_fields_or_sections
        .iter()
        .find(|value| !value.trim().is_empty())
        .map(String::as_str);
    let mut item = MaterialInboxItem {
        id: analysis.file_id.clone(),
        file_name: analysis.file_name.clone(),
        source_path: source_path.to_string(),
        received_at: now.clone(),
        received_from: received_from.to_string(),
        source_hash: source_hash.to_string(),
        file_type: Path::new(source_path)
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("unknown")
            .to_lowercase(),
        size_bytes,
        parse_status: analysis.parse_status.clone(),
        content_summary: analysis.content_summary.clone(),
        analysis_result: format!(
            "{} / {}",
            analysis.parse_status,
            if analysis.document_type.is_empty() {
                &analysis.recommended_category
            } else {
                &analysis.document_type
            }
        ),
        main_fields_or_sections: analysis.main_fields_or_sections.clone(),
        recommended_category: analysis.recommended_category.clone(),
        recommended_project_name: String::new(),
        recommended_location: String::new(),
        parse_failure_reason: analysis.parse_failure_reason.clone(),
        parser: analysis.parser.clone(),
        judgement_status: "uncertain".to_string(),
        target_project_id: String::new(),
        target_project_root: String::new(),
        target_project_name: String::new(),
        suggested_managed_path: String::new(),
        suggested_category,
        duplicate_of_file_id: String::new(),
        previous_version_file_id: String::new(),
        proposed_project_name: guess_project_name(analysis, source_path),
        decision_basis: Vec::new(),
        processing_status: String::new(),
        result_note: String::new(),
        managed_file_id: String::new(),
        created_at: now.clone(),
        updated_at: now,
        received_count: 1,
        source_history: vec![InboxSourceEvent {
            source_path: source_path.to_string(),
            received_from: received_from.to_string(),
            received_at: now_string(),
            hash: source_hash.to_string(),
        }],
        suggested_file_name: inbox_routing::normalize_managed_file_name(
            &analysis.file_name,
            suggested_stem,
        ),
        ai_match_status: "notRun".to_string(),
        document_type: analysis.document_type.clone(),
        business_domain: analysis.business_domain.clone(),
        business_purpose: analysis.business_purpose.clone(),
        business_summary: analysis.business_summary.clone(),
        document_purpose: analysis.document_purpose.clone(),
        document_purpose_confidence: analysis.document_purpose_confidence,
        document_purpose_evidence: analysis.document_purpose_evidence.clone(),
        technical_detail: analysis.technical_detail.clone(),
        analysis_version: MATERIAL_INBOX_ANALYSIS_VERSION,
        ..MaterialInboxItem::default()
    };
    if let Some(history) = general_history.iter().find(|record| {
        let record_stem = Path::new(&record.file_name)
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or(&record.file_name);
        let item_stem = Path::new(&item.file_name)
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or(&item.file_name);
        !record.document_purpose.is_empty()
            && normalize_search_text(record_stem) == normalize_search_text(item_stem)
    }) {
        item.document_purpose = history.document_purpose.clone();
        item.document_purpose_confidence = item
            .document_purpose_confidence
            .max(history.document_purpose_confidence)
            .max(80);
        item.document_purpose_evidence.push(format!(
            "历史已确认同名资料“{}”的用途为 {}。",
            history.file_name, history.document_purpose
        ));
    }

    inbox_routing::transition(
        &mut item,
        inbox_routing::RECEIVED,
        "文件已进入统一 Inbox。",
        "system",
        None,
    )?;
    inbox_routing::transition(
        &mut item,
        inbox_routing::ANALYZING,
        "开始执行哈希、识别、摘要和分类。",
        "system",
        None,
    )?;

    if !analysis.parse_failure_reason.is_empty() {
        item.decision_basis
            .push(format!("解析失败：{}", analysis.parse_failure_reason));
        item.technical_detail = analysis.technical_detail.clone();
    }
    if let Some((project, file)) = find_duplicate_candidate(projects, source_hash) {
        item.judgement_status = "duplicate".to_string();
        item.target_project_id = project.id.clone();
        item.target_project_root = project.root_dir.clone();
        item.target_project_name = project.name.clone();
        item.suggested_managed_path = file.managed_path.clone();
        item.suggested_category = file.category.clone();
        item.recommended_project_name = project.name.clone();
        item.recommended_location = file.managed_path.clone();
        item.duplicate_of_file_id = file.id.clone();
        item.duplicate_kind = "exactDuplicate".to_string();
        item.document_family_id = file.version_group_id.clone();
        item.version_id = file.version_id.clone();
        item.version_number = file
            .version_number
            .max(parse_version_number(&file.current_version));
        item.recommended_relative_location =
            inbox_routing::semantic_location(&file.category).to_string();
        item.location_reason = "完全相同内容复用既有受管副本，不再次复制。".to_string();
        item.confidence_score = 100;
        item.confidence_level = "high".to_string();
        item.confidence_reasons
            .push("SHA256 与既有文件完全一致。".to_string());
        item.project_candidates.push(InboxProjectCandidate {
            candidate_project_id: project.id.clone(),
            candidate_project_name: project.name.clone(),
            candidate_project_root: project.root_dir.clone(),
            score: 100,
            confidence: "high".to_string(),
            reasons: vec!["项目中已有完全相同哈希的文件。".to_string()],
            evidence: vec![file.file_name.clone(), file.managed_relative_path.clone()],
            matched_signals: vec!["sha256".to_string()],
            rule_score: 100,
            ai_score: None,
        });
        item.decision_basis.push(format!(
            "内容哈希与项目“{}”中的 {} 完全一致。",
            project.name, file.file_name
        ));
        inbox_routing::transition(
            &mut item,
            inbox_routing::READY_TO_ROUTE,
            "完全重复文件已形成确定性处理建议。",
            "system",
            None,
        )?;
        item.ownership_type = inbox_routing::OWNERSHIP_EXISTING_PROJECT.to_string();
        return Ok(item);
    }

    // 解析失败或暂不支持解析：不允许自动归入任何项目，留在 Inbox。
    // （完全重复文件例外，已在上面复用既有事实关系。）
    if analysis.parse_status == "failed" || analysis.parse_status == "review_required" {
        item.ownership_type = inbox_routing::OWNERSHIP_UNSUPPORTED.to_string();
        item.judgement_status = "unsupported".to_string();
        item.failed_stage = "parsing".to_string();
        item.confidence_score = 0;
        item.confidence_level = "low".to_string();
        item.confidence_reasons
            .push("正文解析失败，不允许自动归入任何项目。".to_string());
        item.recommended_location = "待人工检查".to_string();
        let (next_status, note) = if analysis.parse_status == "failed" {
            (
                inbox_routing::FAILED,
                "文件解析失败，留在 Inbox，可重试或手动归类。",
            )
        } else {
            (
                inbox_routing::PENDING_REVIEW,
                "暂不支持解析该格式，留在 Inbox 待人工检查。",
            )
        };
        inbox_routing::transition(&mut item, next_status, note, "system", None)?;
        return Ok(item);
    }

    if let Some((project, file, suggested_path)) =
        find_new_version_candidate(projects, &item, analysis)?
    {
        item.judgement_status = "new_version".to_string();
        item.target_project_id = project.id.clone();
        item.target_project_root = project.root_dir.clone();
        item.target_project_name = project.name.clone();
        item.suggested_managed_path = suggested_path;
        item.suggested_category = file.category.clone();
        item.recommended_project_name = project.name.clone();
        item.recommended_location = item.suggested_managed_path.clone();
        item.previous_version_file_id = file.id.clone();
        item.duplicate_kind = "newVersionCandidate".to_string();
        item.document_family_id = file.version_group_id.clone();
        item.version_number = parse_version_number(&file.current_version) + 1;
        item.version_id = new_id();
        item.recommended_relative_location =
            inbox_routing::semantic_location(&file.category).to_string();
        item.location_reason = "与既有文档族匹配，继续保存在同一分类目录。".to_string();
        item.confidence_score = 84;
        item.confidence_level = "high".to_string();
        item.confidence_reasons
            .push("文件名、类型和内容摘要支持新版本关系。".to_string());
        item.project_candidates.push(InboxProjectCandidate {
            candidate_project_id: project.id.clone(),
            candidate_project_name: project.name.clone(),
            candidate_project_root: project.root_dir.clone(),
            score: 88,
            confidence: "high".to_string(),
            reasons: vec!["存在同文档族的历史版本。".to_string()],
            evidence: vec![file.file_name.clone(), file.content_summary.clone()],
            matched_signals: vec![
                "fileName".to_string(),
                "fileType".to_string(),
                "summary".to_string(),
            ],
            rule_score: 88,
            ai_score: None,
        });
        item.decision_basis.push(format!(
            "文件名与项目“{}”中的 {} 属于同一版本组，建议作为新版本归位。",
            project.name, file.file_name
        ));
        inbox_routing::transition(
            &mut item,
            inbox_routing::READY_TO_ROUTE,
            "检测到明确的新版本候选。",
            "system",
            None,
        )?;
        return Ok(item);
    }

    // 一级归属类型判断（发生在项目匹配之前）：只有 existingProject 才继续做项目匹配。
    item.ownership_type = classify_ownership(&item, analysis, projects);
    item.document_type = analysis.document_type.clone();
    item.business_domain = analysis.business_domain.clone();
    item.business_purpose = analysis.business_purpose.clone();
    item.business_summary = analysis.business_summary.clone();
    item.document_purpose = analysis.document_purpose.clone();
    item.document_purpose_confidence = analysis.document_purpose_confidence;
    item.document_purpose_evidence = analysis.document_purpose_evidence.clone();
    if item.technical_detail.is_empty() {
        item.technical_detail = analysis.technical_detail.clone();
    }
    if is_master_data_or_template(&item, analysis) {
        item.ownership_type = inbox_routing::OWNERSHIP_GENERAL_WORK.to_string();
        item.material_semantic_type = "masterDataOrTemplate".to_string();
        if item.document_type.is_empty()
            || matches!(
                item.document_type.as_str(),
                "数据表格" | "表格资料" | "其他资料"
            )
        {
            item.document_type = "主数据/导入模板".to_string();
        }
    } else if item.ownership_type == inbox_routing::OWNERSHIP_NEEDS_REVIEW {
        let assessment = assess_project_candidate(&item, std::slice::from_ref(&item), projects);
        apply_project_candidate_assessment(&mut item, assessment);
    }
    if item.ownership_type != inbox_routing::OWNERSHIP_EXISTING_PROJECT {
        apply_non_project_ownership(&mut item, analysis, general_history);
        return Ok(item);
    }

    item.project_candidates = rank_project_candidates(projects, &item, analysis);
    if let Some(top) = item.project_candidates.first().cloned() {
        let second_score = item
            .project_candidates
            .get(1)
            .map(|candidate| candidate.score)
            .unwrap_or_default();
        let conflicting = top.score.saturating_sub(second_score) < 12;
        if top.score >= 45 && !conflicting {
            let (project, manifest) = projects
                .iter()
                .find(|(project, _)| project.id == top.candidate_project_id)
                .ok_or_else(|| "Inbox 项目候选已失效。".to_string())?;
            let plan = decide_file_location(
                Path::new(&project.root_dir),
                &manifest.files,
                &item.file_name,
                &item.file_type,
                "用户导入",
                "资料收件箱",
                "导入资料",
            )?;
            item.judgement_status = "matched_project".to_string();
            item.ownership_type = inbox_routing::OWNERSHIP_EXISTING_PROJECT.to_string();
            item.target_project_id = project.id.clone();
            item.target_project_root = project.root_dir.clone();
            item.target_project_name = project.name.clone();
            item.suggested_managed_path = path_to_string(&plan.destination);
            item.suggested_category = plan.category.clone();
            item.recommended_project_name = project.name.clone();
            item.recommended_location = item.suggested_managed_path.clone();
            item.recommended_relative_location =
                inbox_routing::semantic_location(&plan.category).to_string();
            item.location_reason = plan.reason;
            let parse_score = if analysis.parse_status == "success" {
                100
            } else {
                55
            };
            item.confidence_score =
                ((top.score as u16 * 65 + parse_score * 20 + 85 * 15) / 100) as u8;
            item.confidence_level =
                inbox_routing::confidence_level(item.confidence_score, false).to_string();
            item.confidence_reasons.extend(top.reasons.clone());
            item.confidence_reasons
                .push("分类和位置均由受控 Location Service 映射。".to_string());
            item.decision_basis.push(format!(
                "规则筛选根据项目说明、项目理解、资料摘要和近期记录匹配到“{}”（{} 分）。",
                project.name, top.score
            ));
            let next_status = if item.confidence_level == "high" {
                inbox_routing::READY_TO_ROUTE
            } else {
                inbox_routing::PENDING_REVIEW
            };
            inbox_routing::transition(
                &mut item,
                next_status,
                "已生成项目和位置处理建议。",
                "system",
                None,
            )?;
            return Ok(item);
        }
        item.decision_basis.push(format!(
            "项目候选证据不足或分数接近，最高为“{}”({})，需要人工确认。",
            top.candidate_project_name, top.score
        ));
        item.confidence_score = top.score;
        item.confidence_level = inbox_routing::confidence_level(top.score, conflicting).to_string();
        item.confidence_reasons = if conflicting {
            vec!["多个项目候选分数接近，存在冲突信号。".to_string()]
        } else {
            vec!["现有项目证据不足。".to_string()]
        };
        inbox_routing::transition(
            &mut item,
            inbox_routing::PENDING_REVIEW,
            "项目匹配需要用户确认。",
            "system",
            None,
        )?;
        return Ok(item);
    }

    item.ownership_type = inbox_routing::OWNERSHIP_NEEDS_REVIEW.to_string();
    let assessment = assess_project_candidate(&item, std::slice::from_ref(&item), projects);
    apply_project_candidate_assessment(&mut item, assessment);
    item.confidence_score = item.project_candidate_score.max(25);
    item.confidence_level =
        inbox_routing::confidence_level(item.confidence_score, false).to_string();
    if item.confidence_reasons.is_empty() {
        item.confidence_reasons
            .push("没有可用项目候选。".to_string());
    }
    item.recommended_project_name = item.proposed_project_name.clone();
    item.recommended_location = "待人工确认".to_string();
    item.decision_basis
        .push("没有找到足够明确的已有项目匹配，建议作为新项目资料检查。".to_string());
    inbox_routing::transition(
        &mut item,
        inbox_routing::PENDING_REVIEW,
        "未找到可靠项目匹配。",
        "system",
        None,
    )?;
    Ok(item)
}

/// 一级归属类型判断（在项目匹配之前执行）。
/// 规则：
/// - 文件名/摘要中出现已知项目名（长度≥2）→ existingProject（项目证据）
/// - 临时/参考特征 → temporaryOrReference
/// - 通用业务特征（报表/申请/排班/财务/人事等）→ generalWorkMaterial
/// - 主数据/导入模板 → generalWorkMaterial
/// - 其余 → needsReview；是否形成新项目由独立候选评分决定
fn classify_ownership(
    item: &MaterialInboxItem,
    analysis: &FileAnalysis,
    projects: &[(ProjectSummary, ProjectManifest)],
) -> String {
    let material_text = normalize_search_text(&format!(
        "{}\n{}\n{}",
        item.file_name,
        analysis.content_summary,
        analysis.main_fields_or_sections.join("\n")
    ));
    let project_name_hit = projects.iter().any(|(project, _)| {
        let name = normalize_search_text(project.name.trim());
        name.len() >= 2 && material_text.contains(&name)
    });
    if project_name_hit {
        return inbox_routing::OWNERSHIP_EXISTING_PROJECT.to_string();
    }
    if is_master_data_or_template(item, analysis) {
        return inbox_routing::OWNERSHIP_GENERAL_WORK.to_string();
    }
    let haystack = format!("{}\n{}", item.file_name, analysis.content_summary).to_lowercase();
    let has = |keywords: &[&str]| keywords.iter().any(|kw| haystack.contains(kw));
    if has(&["临时", "tmp", "草稿", "scratch", "backup", "备份"]) {
        return inbox_routing::OWNERSHIP_TEMPORARY.to_string();
    }
    if has(&[
        "发票",
        "报销",
        "考勤",
        "排班",
        "周计划",
        "工作计划",
        "工作安排",
        "月计划",
        "周报",
        "培训",
        "通讯录",
        "日程",
        "模板",
        "vave",
        "降本",
        "申请单",
        "申请表",
        "采购单",
        "送货单",
        "库存",
        "物料",
        "订单",
        "工单",
        "报废",
        "合格率",
        "良率",
        "报表",
        "对账",
        "预算",
        "会议",
        "通知",
        "报价",
        "供应商",
        "客户",
        "技术文档",
        "设计参考",
        "改善",
        "模板",
        "学习",
        "参考资料",
        "reference",
    ]) {
        return inbox_routing::OWNERSHIP_GENERAL_WORK.to_string();
    }
    inbox_routing::OWNERSHIP_NEEDS_REVIEW.to_string()
}

fn is_master_data_or_template(item: &MaterialInboxItem, analysis: &FileAnalysis) -> bool {
    let text = normalize_search_text(&format!(
        "{} {} {} {} {} {}",
        item.file_name,
        analysis.document_type,
        analysis.business_purpose,
        analysis.content_summary,
        analysis.main_fields_or_sections.join(" "),
        analysis.business_summary
    ));
    [
        "account_import_template",
        "account import template",
        "账号导入模板",
        "用户账号模板",
        "用户导入模板",
        "员工信息",
        "员工主数据",
        "人员信息",
        "供应商主数据",
        "供应商资料导入",
        "产品编码",
        "产品主数据",
        "物料编码",
        "物料主数据",
        "物料清单",
        "导入模板",
        "master data",
        "masterdata",
        "bom",
    ]
    .iter()
    .any(|keyword| text.contains(keyword))
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct ProjectCandidateAssessment {
    score: u8,
    confidence: &'static str,
    reasons: Vec<String>,
    evidence: Vec<String>,
    related_count: usize,
    complementary_roles: usize,
}

fn assess_project_candidate(
    item: &MaterialInboxItem,
    related_items: &[MaterialInboxItem],
    projects: &[(ProjectSummary, ProjectManifest)],
) -> ProjectCandidateAssessment {
    let own_name = normalize_search_text(&item.file_name);
    let own_text = normalize_search_text(&format!(
        "{} {} {} {} {} {}",
        item.file_name,
        item.document_type,
        item.business_purpose,
        item.business_summary,
        item.content_summary,
        item.main_fields_or_sections.join(" ")
    ));
    let group_text = normalize_search_text(
        &related_items
            .iter()
            .map(|entry| {
                format!(
                    "{} {} {} {} {}",
                    entry.file_name,
                    entry.document_type,
                    entry.business_purpose,
                    entry.content_summary,
                    entry.main_fields_or_sections.join(" ")
                )
            })
            .collect::<Vec<_>>()
            .join(" "),
    );
    let has = |text: &str, keywords: &[&str]| keywords.iter().any(|key| text.contains(key));
    let mut score = 0_u8;
    let mut reasons = Vec::new();
    let mut evidence = Vec::new();

    if has(&own_name, &["项目", "project"]) {
        score = score.saturating_add(18);
        reasons.push("文件名包含明确项目信号。".to_string());
        evidence.push(item.file_name.clone());
    } else if has_code_style_project_prefix(&item.file_name) {
        score = score.saturating_add(16);
        reasons.push("文件名包含疑似项目代号。".to_string());
        evidence.push(item.file_name.clone());
    }
    if has(&group_text, &["项目", "project", "立项"]) {
        score = score.saturating_add(10);
        reasons.push("资料内容包含项目语义。".to_string());
    }
    if has(
        &group_text,
        &["项目目标", "目标", "项目范围", "范围", "需求", "实施方案"],
    ) {
        score = score.saturating_add(20);
        reasons.push("资料包含目标、范围或需求证据。".to_string());
        evidence.push("目标/范围/需求".to_string());
    }
    if has(&group_text, &["任务", "待办", "工作项", "里程碑", "交付物"]) {
        score = score.saturating_add(12);
        reasons.push("资料包含任务或交付描述。".to_string());
        evidence.push("任务/交付物".to_string());
    }
    if has(
        &group_text,
        &["计划", "周期", "开始时间", "结束时间", "截止日期", "进度"],
    ) {
        score = score.saturating_add(8);
        reasons.push("资料包含计划或时间周期。".to_string());
        evidence.push("计划/时间周期".to_string());
    }
    if has(
        &group_text,
        &["负责人", "责任人", "参与人", "参会人员", "项目成员"],
    ) {
        score = score.saturating_add(8);
        reasons.push("资料包含负责人或参与人。".to_string());
        evidence.push("负责人/参与人".to_string());
    }

    let related_count = related_items.len();
    if related_count >= 3 {
        score = score.saturating_add(22);
        reasons.push(format!("同批发现 {related_count} 份关联资料。"));
    } else if related_count == 2 {
        score = score.saturating_add(12);
        reasons.push("同批发现 2 份关联资料。".to_string());
    } else if score > 0 {
        reasons.push("当前仅发现 1 份资料，项目证据不足。".to_string());
    }
    let complementary_roles = [
        has(&group_text, &["项目说明", "项目定义", "项目需求"]),
        has(&group_text, &["计划", "里程碑"]),
        has(&group_text, &["会议纪要", "会议记录"]),
    ]
    .into_iter()
    .filter(|matched| *matched)
    .count();
    if complementary_roles >= 2 {
        score = score.saturating_add(18);
        reasons.push("关联资料覆盖说明、计划或会议等互补角色。".to_string());
        evidence.push("项目资料组合".to_string());
    }

    let proposed = normalize_search_text(&item.proposed_project_name);
    let similar_history = !proposed.is_empty()
        && projects.iter().any(|(project, manifest)| {
            normalize_search_text(&project.name).contains(&proposed)
                || manifest
                    .files
                    .iter()
                    .any(|file| normalize_search_text(&file.file_name).contains(&proposed))
        });
    if similar_history {
        score = score.saturating_add(8);
        reasons.push("历史项目或资料中存在相似名称。".to_string());
        evidence.push("历史项目资料".to_string());
    }

    if own_text.trim().is_empty() {
        score = 0;
    }
    let score = score.min(96);
    let confidence = if score >= 75 {
        "high"
    } else if score >= 45 {
        "medium"
    } else {
        "low"
    };
    ProjectCandidateAssessment {
        score,
        confidence,
        reasons,
        evidence,
        related_count,
        complementary_roles,
    }
}

fn has_code_style_project_prefix(file_name: &str) -> bool {
    let stem = Path::new(file_name)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or(file_name);
    let Some(prefix) = stem.split(['-', '_']).next() else {
        return false;
    };
    (2..=10).contains(&prefix.len())
        && prefix
            .chars()
            .all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit())
        && stem.len() > prefix.len() + 1
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GeneralMaterialClassification {
    domain: &'static str,
    category: &'static str,
    location_label: &'static str,
    score: u8,
    reasons: Vec<String>,
}

fn classify_general_material(
    item: &MaterialInboxItem,
    analysis: &FileAnalysis,
    history: &[GlobalManagedFile],
) -> GeneralMaterialClassification {
    let file_name = normalize_search_text(&item.file_name);
    let fields = normalize_search_text(&analysis.main_fields_or_sections.join(" "));
    let summary = normalize_search_text(&analysis.content_summary);
    let semantics = normalize_search_text(&format!(
        "{} {} {} {}",
        analysis.document_type,
        analysis.business_domain,
        analysis.business_purpose,
        analysis.business_summary
    ));
    let all = format!("{file_name} {fields} {summary} {semantics}");
    let has = |text: &str, keywords: &[&str]| keywords.iter().any(|key| text.contains(key));

    let (domain, category, label, keywords): (&str, &str, &str, &[&str]) =
        if item.material_semantic_type == "masterDataOrTemplate"
            || is_master_data_or_template(item, analysis)
        {
            (
                "management",
                "masterData",
                "主数据/模板",
                &[
                    "账号",
                    "用户",
                    "员工",
                    "供应商",
                    "产品编码",
                    "物料编码",
                    "物料主数据",
                    "bom",
                    "导入模板",
                ],
            )
        } else if has(&all, &["vave", "降本申请", "成本改善申请"]) {
            ("engineering", "vave", "改善申请", &["vave", "降本", "申请"])
        } else if has(&all, &["改善提案", "改善项目", "kaizen", "持续改善"]) {
            (
                "engineering",
                "improvement",
                "改善资料",
                &["改善", "提案", "kaizen"],
            )
        } else if has(&all, &["报废", "不良", "合格率", "良率", "质检", "缺陷"])
            && !has(&all, &["车间", "生产订单", "工序", "排产", "报工"])
        {
            (
                "manufacturing",
                "qualityData",
                "质量数据",
                &["报废", "不良", "合格率", "良率", "质检"],
            )
        } else if has(
            &all,
            &["周工作", "周计划", "工作计划", "工作安排", "月计划"],
        ) {
            (
                "management",
                "workPlan",
                "工作计划",
                &["周工作", "周计划", "工作计划", "工作安排"],
            )
        } else if has(&all, &["会议纪要", "会议记录", "会议议题", "参会人员"]) {
            (
                "management",
                "meeting",
                "会议资料",
                &["会议", "纪要", "议题", "参会"],
            )
        } else if has(&all, &["通知", "公告", "通告"]) {
            (
                "management",
                "notice",
                "通知公告",
                &["通知", "公告", "通告"],
            )
        } else if has(&all, &["报价单", "报价", "询价"]) {
            ("business", "quotation", "报价资料", &["报价", "询价"])
        } else if has(&all, &["供应商", "供方", "采购商"]) {
            (
                "business",
                "supplier",
                "供应商资料",
                &["供应商", "供方", "采购"],
            )
        } else if has(&all, &["客户", "客诉", "客户需求"]) {
            ("business", "customer", "客户资料", &["客户", "客诉"])
        } else if has(&all, &["模板", "样表", "空白表"]) {
            ("reference", "template", "模板", &["模板", "样表", "空白表"])
        } else if has(&all, &["培训", "学习", "教程", "课程", "教材"]) {
            (
                "reference",
                "learning",
                "学习资料",
                &["培训", "学习", "教程", "课程"],
            )
        } else if has(
            &all,
            &["生产日报", "生产周报", "生产月报", "生产报表", "产量报表"],
        ) {
            (
                "manufacturing",
                "productionReport",
                "生产报表",
                &["生产", "报表", "产量"],
            )
        } else if (has(
            &all,
            &["车间", "工序", "排产", "报工", "完工数量", "生产订单"],
        ) && has(&all, &["订单", "工单", "产量", "完工", "工序"]))
            || has(&all, &["生产数据", "车间生产"])
        {
            (
                "manufacturing",
                "productionData",
                "生产数据",
                &["车间", "工序", "排产", "报工", "订单", "完工"],
            )
        } else if has(
            &all,
            &["订单号", "订单数据", "订单明细", "销售订单", "采购订单"],
        ) {
            (
                "manufacturing",
                "orderData",
                "订单数据",
                &["订单", "订单号", "订单明细"],
            )
        } else if has(&all, &["设计参考", "图纸参考", "设计规范", "设计资料"]) {
            (
                "engineering",
                "designReference",
                "设计参考",
                &["设计", "图纸", "规范"],
            )
        } else if has(
            &all,
            &["技术文档", "技术规范", "技术标准", "工艺文件", "作业指导书"],
        ) {
            (
                "engineering",
                "technicalDocument",
                "技术文档",
                &["技术", "规范", "标准", "工艺"],
            )
        } else if has(
            &all,
            &[
                "工作报告",
                "总结报告",
                "管理报告",
                "汇报材料",
                "周报",
                "月报",
            ],
        ) {
            (
                "management",
                "report",
                "管理报告",
                &["报告", "总结", "汇报", "周报", "月报"],
            )
        } else {
            ("reference", "other", "其他参考资料", &[])
        };

    if category == "other" {
        return GeneralMaterialClassification {
            domain,
            category,
            location_label: label,
            score: 28,
            reasons: vec!["未识别到稳定业务语义，保守建议放入其他参考资料。".to_string()],
        };
    }

    let mut score = 18_u8;
    let mut reasons = Vec::new();
    if has(&file_name, keywords) {
        score = score.saturating_add(28);
        reasons.push("文件名包含明确业务关键词。".to_string());
    }
    if has(&fields, keywords) {
        score = score.saturating_add(24);
        reasons.push("工作表名称或表头包含明确业务关键词。".to_string());
    }
    if has(&summary, keywords) {
        score = score.saturating_add(12);
        reasons.push("内容摘要支持该分类。".to_string());
    }
    if has(&semantics, keywords) {
        score = score.saturating_add(18);
        reasons.push("文档类型与业务语义支持该分类。".to_string());
    }
    if !analysis.business_domain.trim().is_empty() && analysis.business_domain != "未分类" {
        score = score.saturating_add(8);
        reasons.push("业务域与分类方向一致。".to_string());
    }
    let history_count = history
        .iter()
        .filter(|record| record.undone_at.is_empty())
        .filter(|record| {
            record.general_material_category == category
                || (record.general_material_category.is_empty()
                    && has(
                        &normalize_search_text(&format!(
                            "{} {} {} {}",
                            record.file_name,
                            record.document_type,
                            record.business_domain,
                            record.business_purpose
                        )),
                        keywords,
                    ))
        })
        .count();
    if history_count > 0 {
        score = score.saturating_add((history_count as u8).saturating_mul(3).min(8));
        reasons.push(format!("已有 {history_count} 份同类历史归档。"));
    }
    GeneralMaterialClassification {
        domain,
        category,
        location_label: label,
        score: score.min(96),
        reasons,
    }
}

fn apply_project_candidate_assessment(
    item: &mut MaterialInboxItem,
    assessment: ProjectCandidateAssessment,
) {
    item.project_candidate_score = assessment.score;
    item.project_candidate_confidence = assessment.confidence.to_string();
    item.project_candidate_reasons = assessment.reasons.clone();
    item.project_candidate_evidence = assessment.evidence.clone();
    if assessment.score >= 75 {
        item.ownership_type = inbox_routing::OWNERSHIP_NEW_PROJECT.to_string();
        item.judgement_status = "project_candidate_high".to_string();
    } else if assessment.score >= 45 {
        item.ownership_type = inbox_routing::OWNERSHIP_NEEDS_REVIEW.to_string();
        item.judgement_status = "project_candidate_medium".to_string();
    } else if assessment.score >= 12 {
        item.ownership_type = inbox_routing::OWNERSHIP_NEEDS_REVIEW.to_string();
        item.judgement_status = "project_candidate_low".to_string();
    }
}

/// 非项目归属的安全归类建议：不匹配项目，给出通用语义目录/临时区/待检查建议。
fn apply_non_project_ownership(
    item: &mut MaterialInboxItem,
    analysis: &FileAnalysis,
    general_history: &[GlobalManagedFile],
) {
    match item.ownership_type.as_str() {
        inbox_routing::OWNERSHIP_GENERAL_WORK => {
            let classification = classify_general_material(item, analysis, general_history);
            item.judgement_status = "general_material".to_string();
            item.global_destination = inbox_routing::GLOBAL_DEST_GENERAL.to_string();
            item.general_material_domain = classification.domain.to_string();
            item.general_material_category = classification.category.to_string();
            item.recommended_relative_location = format!(
                "general/{}/{}",
                classification.domain, classification.category
            );
            item.recommended_location = classification.location_label.to_string();
            item.location_reason = format!(
                "根据文件名、工作表/表头、摘要、业务域和历史归档推荐至“{}”。",
                classification.location_label
            );
            item.confidence_score = classification.score;
            item.confidence_level =
                inbox_routing::confidence_level(classification.score, false).to_string();
            item.confidence_reasons = classification.reasons;
            item.confidence_reasons
                .push("判定为通用工作资料，不归属任何具体项目。".to_string());
            item.decision_basis.push(format!(
                "未发现项目归属证据，建议归入“{}”。",
                classification.location_label
            ));
        }
        inbox_routing::OWNERSHIP_TEMPORARY => {
            item.judgement_status = "temporary_reference".to_string();
            item.global_destination = inbox_routing::GLOBAL_DEST_TEMPORARY.to_string();
            item.recommended_location = "临时/参考资料区".to_string();
            item.confidence_score = 35;
            item.confidence_level = "low".to_string();
            item.confidence_reasons = vec!["判定为临时或参考资料。".to_string()];
            item.decision_basis
                .push("识别到临时/参考特征，按临时资料处理。".to_string());
        }
        inbox_routing::OWNERSHIP_NEW_PROJECT => {
            item.judgement_status = "project_candidate_high".to_string();
            item.recommended_project_name = item.proposed_project_name.clone();
            item.recommended_location = "建议创建新项目后归位".to_string();
            item.confidence_score = item.project_candidate_score;
            item.confidence_level = item.project_candidate_confidence.clone();
            item.confidence_reasons = item.project_candidate_reasons.clone();
            item.decision_basis.extend(
                item.project_candidate_reasons
                    .iter()
                    .map(|reason| format!("项目候选：{reason}")),
            );
        }
        _ => {
            if !matches!(
                item.judgement_status.as_str(),
                "project_candidate_low" | "project_candidate_medium"
            ) {
                item.judgement_status = "uncertain".to_string();
            }
            item.recommended_location = "待人工检查".to_string();
            if item.project_candidate_score > 0 {
                item.confidence_score = item.project_candidate_score;
                item.confidence_level = item.project_candidate_confidence.clone();
                item.confidence_reasons = item.project_candidate_reasons.clone();
                item.decision_basis.extend(
                    item.project_candidate_reasons
                        .iter()
                        .map(|reason| format!("项目候选：{reason}")),
                );
            } else {
                item.confidence_score = 25;
                item.confidence_level = "low".to_string();
                item.confidence_reasons = vec!["无法可靠判断文件归属。".to_string()];
                item.decision_basis
                    .push("归属类型无法可靠判断，留在 Inbox 待检查。".to_string());
            }
        }
    }
    let _ = inbox_routing::transition(
        item,
        inbox_routing::PENDING_REVIEW,
        "非项目资料已给出安全归类建议，等待确认。",
        "system",
        None,
    );
}

fn find_duplicate_candidate(
    projects: &[(ProjectSummary, ProjectManifest)],
    source_hash: &str,
) -> Option<(ProjectSummary, FileRecord)> {
    let matches = projects
        .iter()
        .filter_map(|(project, manifest)| {
            manifest
                .files
                .iter()
                .find(|file| !file.content_hash.is_empty() && file.content_hash == source_hash)
                .cloned()
                .map(|file| (project.clone(), file))
        })
        .collect::<Vec<_>>();
    let project_count = matches
        .iter()
        .map(|(project, _)| project.id.as_str())
        .collect::<HashSet<_>>()
        .len();
    if project_count == 1 {
        matches.into_iter().next()
    } else {
        None
    }
}

fn find_new_version_candidate(
    projects: &[(ProjectSummary, ProjectManifest)],
    item: &MaterialInboxItem,
    analysis: &FileAnalysis,
) -> Result<Option<(ProjectSummary, FileRecord, String)>, String> {
    let logical_key = logical_version_key(&item.file_name);
    if logical_key.is_empty() {
        return Ok(None);
    }
    let analysis_text = format!(
        "{} {}",
        analysis.content_summary,
        analysis.main_fields_or_sections.join(" ")
    );
    let mut candidates = Vec::new();
    for (project, manifest) in projects {
        for file in manifest
            .files
            .iter()
            .filter(|file| file.file_type == item.file_type)
        {
            let same_family_name = logical_version_key(&file.file_name) == logical_key;
            let similarity = text_similarity_percent(
                &analysis_text,
                &format!(
                    "{} {}",
                    file.content_summary,
                    file.main_fields_or_sections.join(" ")
                ),
            );
            if !same_family_name && similarity < 55 {
                continue;
            }
            let plan = decide_file_location(
                Path::new(&project.root_dir),
                &manifest.files,
                &item.file_name,
                &item.file_type,
                "用户导入",
                "资料收件箱",
                "导入资料",
            )?;
            let relationship_score = if same_family_name {
                80_u8.saturating_add(similarity / 5)
            } else {
                similarity
            };
            candidates.push((
                relationship_score,
                project.clone(),
                file.clone(),
                path_to_string(&plan.destination),
            ));
        }
    }
    candidates.sort_by(|left, right| right.0.cmp(&left.0));
    if let (Some(first), Some(second)) = (candidates.first(), candidates.get(1)) {
        if first.1.id != second.1.id && first.0.saturating_sub(second.0) < 12 {
            return Ok(None);
        }
    }
    Ok(candidates
        .into_iter()
        .next()
        .map(|(_, project, file, path)| (project, file, path)))
}

fn rank_project_candidates(
    projects: &[(ProjectSummary, ProjectManifest)],
    item: &MaterialInboxItem,
    analysis: &FileAnalysis,
) -> Vec<InboxProjectCandidate> {
    let mut candidates = projects
        .iter()
        .map(|(project, manifest)| score_material_for_project(project, manifest, item, analysis))
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| {
        right.score.cmp(&left.score).then_with(|| {
            left.candidate_project_name
                .cmp(&right.candidate_project_name)
        })
    });
    candidates.truncate(3);
    candidates
}

fn score_material_for_project(
    project: &ProjectSummary,
    manifest: &ProjectManifest,
    item: &MaterialInboxItem,
    analysis: &FileAnalysis,
) -> InboxProjectCandidate {
    let material_text = normalize_search_text(&format!(
        "{}\n{}\n{}\n{}\n{}\n{}",
        item.file_name,
        item.source_path,
        item.file_type,
        item.suggested_category,
        analysis.content_summary,
        analysis.main_fields_or_sections.join("\n")
    ));
    let mut score = 0_u8;
    let mut reasons = Vec::new();
    let mut evidence = Vec::new();
    let mut matched_signals = Vec::new();
    let project_name = normalize_search_text(&project.name);
    if project_name.len() >= 2 && material_text.contains(&project_name) {
        score = score.saturating_add(55);
        reasons.push("文件名、路径或内容明确出现项目名称。".to_string());
        evidence.push(project.name.clone());
        matched_signals.push("projectName".to_string());
    }

    if !item.source_hash.is_empty() {
        if let Some(file) = manifest
            .files
            .iter()
            .find(|file| file.content_hash == item.source_hash)
        {
            score = score.saturating_add(60);
            reasons.push("项目中已有完全相同哈希的文件。".to_string());
            evidence.push(file.file_name.clone());
            matched_signals.push("sha256".to_string());
        }
    }

    let logical_key = logical_version_key(&item.file_name);
    if !logical_key.is_empty() {
        if let Some(file) = manifest.files.iter().find(|file| {
            file.file_type == item.file_type && logical_version_key(&file.file_name) == logical_key
        }) {
            score = score.saturating_add(24);
            reasons.push("项目中存在同名文档族。".to_string());
            evidence.push(file.file_name.clone());
            matched_signals.push("documentFamily".to_string());
        }
    }

    let description_score = text_similarity_percent(&material_text, &project.description);
    if description_score >= 18 {
        score = score.saturating_add((description_score / 4).min(18));
        reasons.push("文件摘要与项目说明存在共同主题。".to_string());
        evidence.push(first_line(&project.description, 80));
        matched_signals.push("projectDescription".to_string());
    }
    let analysis_score = text_similarity_percent(
        &material_text,
        &format!(
            "{} {} {} {}",
            manifest.project_analysis.project_definition,
            manifest.project_analysis.known_requirements.join(" "),
            manifest.project_analysis.constraints.join(" "),
            manifest.project_analysis.next_steps.join(" ")
        ),
    );
    if analysis_score >= 18 {
        score = score.saturating_add((analysis_score / 5).min(16));
        reasons.push("与 project-analysis 中的定义、要求或下一步相关。".to_string());
        evidence.push(first_line(
            &manifest.project_analysis.project_definition,
            80,
        ));
        matched_signals.push("projectAnalysis".to_string());
    }

    let recent_work = format!(
        "{} {} {}",
        manifest
            .tasks
            .iter()
            .rev()
            .take(8)
            .map(|value| value.title.as_str())
            .collect::<Vec<_>>()
            .join(" "),
        manifest
            .decisions
            .iter()
            .rev()
            .take(8)
            .map(|value| value.summary.as_str())
            .collect::<Vec<_>>()
            .join(" "),
        manifest
            .artifacts
            .iter()
            .rev()
            .take(8)
            .map(|value| value.title.as_str())
            .collect::<Vec<_>>()
            .join(" ")
    );
    let work_score = text_similarity_percent(&material_text, &recent_work);
    if work_score >= 18 {
        score = score.saturating_add((work_score / 5).min(14));
        reasons.push("与项目近期任务、决定或成果相关。".to_string());
        evidence.push(first_line(&recent_work, 80));
        matched_signals.push("recentWork".to_string());
    }

    let mut best_file_score = 0_u8;
    let mut best_file_evidence = String::new();
    for file in manifest.files.iter().rev().take(24) {
        let candidate = text_similarity_percent(
            &material_text,
            &format!(
                "{} {} {}",
                file.file_name, file.content_summary, file.category
            ),
        );
        if candidate > best_file_score {
            best_file_score = candidate;
            best_file_evidence = format!(
                "{}：{}",
                file.file_name,
                first_line(&file.content_summary, 60)
            );
        }
    }
    if best_file_score >= 18 {
        score = score.saturating_add((best_file_score / 4).min(20));
        reasons.push("与项目已有文件名称或摘要相似。".to_string());
        evidence.push(best_file_evidence);
        matched_signals.push("existingFileSummary".to_string());
    }

    InboxProjectCandidate {
        candidate_project_id: project.id.clone(),
        candidate_project_name: project.name.clone(),
        candidate_project_root: project.root_dir.clone(),
        score: score.min(100),
        confidence: inbox_routing::confidence_level(score.min(100), false).to_string(),
        reasons,
        evidence,
        matched_signals,
        rule_score: score.min(100),
        ai_score: None,
    }
}

fn text_similarity_percent(left: &str, right: &str) -> u8 {
    let left = character_bigrams(&normalize_search_text(left));
    let right = character_bigrams(&normalize_search_text(right));
    if left.is_empty() || right.is_empty() {
        return 0;
    }
    let intersection = left.intersection(&right).count();
    let union = left.union(&right).count();
    ((intersection * 100) / union.max(1)).min(100) as u8
}

fn character_bigrams(value: &str) -> HashSet<String> {
    let chars = value
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect::<Vec<_>>();
    if chars.len() == 1 {
        return [chars[0].to_string()].into_iter().collect();
    }
    chars
        .windows(2)
        .map(|pair| pair.iter().collect::<String>())
        .collect()
}

fn guess_project_name(analysis: &FileAnalysis, source_path: &str) -> String {
    let base = Path::new(source_path)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("新项目");
    let first = analysis
        .main_fields_or_sections
        .iter()
        .find(|value| value.chars().count() >= 4)
        .cloned()
        .unwrap_or_else(|| base.to_string());
    first_line(&first, 24)
}

fn try_push_search_result(
    results: &mut Vec<GlobalSearchResult>,
    normalized_query: &str,
    id: &str,
    project_id: &str,
    project_root: &str,
    project_name: &str,
    content_type: &str,
    title: &str,
    snippet_source: &str,
    updated_at: &str,
    managed_path: &str,
) {
    let haystack = join_search_fields(&[project_name, content_type, title, snippet_source]);
    if match_search_score(&haystack, normalized_query).is_none() {
        return;
    }
    results.push(GlobalSearchResult {
        id: id.to_string(),
        project_id: project_id.to_string(),
        project_root: project_root.to_string(),
        project_name: project_name.to_string(),
        content_type: content_type.to_string(),
        title: first_line(title, 80),
        snippet: summarize_search_snippet(snippet_source),
        updated_at: updated_at.to_string(),
        managed_path: managed_path.to_string(),
        workspace_relative_path: String::new(),
        ownership_type: String::new(),
        category: String::new(),
        document_type: String::new(),
        document_purpose: String::new(),
        decision_trace_id: String::new(),
        recent_status: String::new(),
        confidence_display: String::new(),
        matched_field: String::new(),
        match_snippet: String::new(),
        ..GlobalSearchResult::default()
    });
}

fn push_file_projection_search_result(
    results: &mut Vec<GlobalSearchResult>,
    normalized_query: &str,
    result: GlobalSearchResult,
) {
    let haystack = build_search_haystack(&result);
    if normalized_query.is_empty()
        || match_search_score(&haystack, normalized_query).unwrap_or(0) > 0
    {
        results.push(result);
    }
}

fn search_result_from_file_projection(
    projection: &FileProjection,
    project_id: String,
    project_root: String,
    project_name: String,
    content_type: &str,
    decision_trace_id: &str,
    confidence_display: String,
) -> GlobalSearchResult {
    GlobalSearchResult {
        id: if content_type == "file" {
            format!("file:{}", projection.identity.file_id)
        } else {
            projection.identity.file_id.clone()
        },
        file_id: projection.identity.file_id.clone(),
        hash: projection.identity.hash.clone(),
        project_id,
        project_root,
        project_name,
        content_type: content_type.to_string(),
        title: projection.identity.original_name.clone(),
        snippet: projection.summary.clone(),
        updated_at: projection.identity.created_at.clone(),
        managed_path: projection.location.managed_path.clone(),
        workspace_relative_path: non_empty_or_pending(&projection.location.workspace_relative_path),
        file_type: projection.metadata.file_type.clone(),
        ownership_type: projection.location.ownership_type.clone(),
        category: projection.metadata.category.clone(),
        document_type: projection.metadata.document_type.clone(),
        document_purpose: projection.metadata.document_purpose.clone(),
        business_domain: projection.metadata.business_domain.clone(),
        lifecycle_status: projection.metadata.lifecycle_status.clone(),
        duplicate_of: projection.metadata.duplicate_of.clone(),
        version_group_id: projection.metadata.version_group_id.clone(),
        version_number: projection.metadata.version_number,
        evidence_refs: projection.evidence_refs.clone(),
        decision_trace_id: decision_trace_id.to_string(),
        recent_status: projection_recent_status(projection),
        confidence_display,
        matched_field: String::new(),
        match_snippet: String::new(),
    }
}

fn projection_recent_status(projection: &FileProjection) -> String {
    match projection.metadata.lifecycle_status.as_str() {
        "missing" => "文件缺失，记录保留。".to_string(),
        "superseded" => "已撤销或被新版本替代。".to_string(),
        "managed" => "已整理。".to_string(),
        "received" => "已接收，等待进一步处理。".to_string(),
        "understood" => "已理解，等待确认。".to_string(),
        "active" => "近期活跃。".to_string(),
        "versioned" => "已建立版本关系。".to_string(),
        "archived" => "已归档。".to_string(),
        value if !value.trim().is_empty() => value.to_string(),
        _ => "已记录。".to_string(),
    }
}

fn search_result_to_index_entry(
    result: GlobalSearchResult,
    indexed_at: &str,
) -> Option<SearchIndexEntry> {
    let haystack = build_search_haystack(&result);
    if is_sensitive_search_entry(&result, &haystack) {
        return None;
    }
    Some(SearchIndexEntry {
        id: result.id,
        file_id: result.file_id,
        hash: result.hash,
        project_id: result.project_id,
        project_root: result.project_root,
        project_name: result.project_name,
        content_type: result.content_type,
        title: result.title,
        summary: result.snippet,
        updated_at: result.updated_at,
        managed_path: result.managed_path,
        workspace_relative_path: result.workspace_relative_path,
        file_type: result.file_type,
        ownership_type: result.ownership_type,
        category: result.category,
        document_type: result.document_type,
        document_purpose: result.document_purpose,
        business_domain: result.business_domain,
        lifecycle_status: result.lifecycle_status,
        duplicate_of: result.duplicate_of,
        version_group_id: result.version_group_id,
        version_number: result.version_number,
        evidence_refs: result.evidence_refs,
        decision_trace_id: result.decision_trace_id,
        recent_status: result.recent_status,
        confidence_display: result.confidence_display,
        haystack,
        indexed_at: indexed_at.to_string(),
    })
}

fn search_index_entry_to_result(
    entry: &SearchIndexEntry,
    normalized_query: &str,
) -> Option<GlobalSearchResult> {
    if match_search_score(&entry.haystack, normalized_query).is_none() {
        return None;
    }
    let (matched_field, match_snippet) = best_search_match(entry, normalized_query);
    let projection = FileProjectionAdapter::from_search_entry(entry);
    let is_file = matches!(entry.content_type.as_str(), "file" | "global_file");
    Some(GlobalSearchResult {
        id: entry.id.clone(),
        file_id: if is_file {
            projection.identity.file_id.clone()
        } else {
            entry.file_id.clone()
        },
        hash: if is_file {
            projection.identity.hash.clone()
        } else {
            entry.hash.clone()
        },
        project_id: entry.project_id.clone(),
        project_root: entry.project_root.clone(),
        project_name: entry.project_name.clone(),
        content_type: entry.content_type.clone(),
        title: if is_file {
            projection.identity.original_name.clone()
        } else {
            entry.title.clone()
        },
        snippet: if is_file {
            projection.summary.clone()
        } else {
            entry.summary.clone()
        },
        updated_at: entry.updated_at.clone(),
        managed_path: if is_file {
            projection.location.managed_path.clone()
        } else {
            entry.managed_path.clone()
        },
        workspace_relative_path: if is_file {
            projection.location.workspace_relative_path.clone()
        } else {
            entry.workspace_relative_path.clone()
        },
        file_type: if is_file {
            projection.metadata.file_type.clone()
        } else {
            entry.file_type.clone()
        },
        ownership_type: if is_file {
            projection.location.ownership_type.clone()
        } else {
            entry.ownership_type.clone()
        },
        category: if is_file {
            projection.metadata.category.clone()
        } else {
            entry.category.clone()
        },
        document_type: if is_file {
            projection.metadata.document_type.clone()
        } else {
            entry.document_type.clone()
        },
        document_purpose: if is_file {
            projection.metadata.document_purpose.clone()
        } else {
            entry.document_purpose.clone()
        },
        business_domain: if is_file {
            projection.metadata.business_domain.clone()
        } else {
            entry.business_domain.clone()
        },
        lifecycle_status: if is_file {
            projection.metadata.lifecycle_status.clone()
        } else {
            entry.lifecycle_status.clone()
        },
        duplicate_of: if is_file {
            projection.metadata.duplicate_of.clone()
        } else {
            entry.duplicate_of.clone()
        },
        version_group_id: if is_file {
            projection.metadata.version_group_id.clone()
        } else {
            entry.version_group_id.clone()
        },
        version_number: if is_file {
            projection.metadata.version_number
        } else {
            entry.version_number
        },
        evidence_refs: if is_file {
            projection.evidence_refs.clone()
        } else {
            entry.evidence_refs.clone()
        },
        decision_trace_id: entry.decision_trace_id.clone(),
        recent_status: entry.recent_status.clone(),
        confidence_display: entry.confidence_display.clone(),
        matched_field,
        match_snippet,
    })
}

fn is_sensitive_search_entry(result: &GlobalSearchResult, haystack: &str) -> bool {
    let text = normalize_search_text(&join_search_fields(&[
        &result.title,
        &result.managed_path,
        &result.workspace_relative_path,
        haystack,
    ]));
    [
        "api_key",
        "apikey",
        "secret",
        "credential",
        "credentials",
        "token",
        "password",
        ".env",
        "deepseek_api",
        "sk-",
    ]
    .iter()
    .any(|needle| text.contains(needle))
}

fn best_search_match(entry: &SearchIndexEntry, normalized_query: &str) -> (String, String) {
    let fields = [
        ("文件名", entry.title.as_str()),
        ("摘要", entry.summary.as_str()),
        ("项目", entry.project_name.as_str()),
        ("位置", entry.workspace_relative_path.as_str()),
        ("分类", entry.category.as_str()),
        ("类型", entry.document_type.as_str()),
        ("用途", entry.document_purpose.as_str()),
        ("状态", entry.recent_status.as_str()),
    ];
    fields
        .iter()
        .filter_map(|(label, value)| {
            match_search_score(value, normalized_query).map(|score| {
                (
                    score,
                    (*label).to_string(),
                    matched_search_snippet(value, normalized_query),
                )
            })
        })
        .max_by(|left, right| left.0.cmp(&right.0))
        .map(|(_, label, snippet)| (label, snippet))
        .unwrap_or_else(|| ("全文".to_string(), summarize_search_snippet(&entry.summary)))
}

fn matched_search_snippet(text: &str, normalized_query: &str) -> String {
    let source = if text.trim().is_empty() {
        return String::new();
    } else {
        text.trim()
    };
    let normalized_source = normalize_search_text(source);
    let tokens = normalized_query
        .split_whitespace()
        .filter(|token| !token.trim().is_empty())
        .collect::<Vec<_>>();
    let tokens = if tokens.is_empty() {
        vec![normalized_query]
    } else {
        tokens
    };
    if let Some((start, end)) = tokens.iter().find_map(|token| {
        normalized_source
            .find(token)
            .map(|start| (start, start + token.len()))
    }) {
        return excerpt_by_byte_range(source, start, end, 48);
    }
    summarize_search_snippet(source)
}

fn excerpt_by_byte_range(text: &str, start: usize, end: usize, context_chars: usize) -> String {
    let char_positions = text.char_indices().collect::<Vec<_>>();
    let start_char = char_positions
        .iter()
        .position(|(byte_index, _)| *byte_index >= start)
        .unwrap_or(0);
    let end_char = char_positions
        .iter()
        .position(|(byte_index, _)| *byte_index >= end)
        .unwrap_or(char_positions.len());
    let window_start = start_char.saturating_sub(context_chars);
    let window_end = (end_char + context_chars).min(char_positions.len());
    let prefix = if window_start > 0 { "..." } else { "" };
    let suffix = if window_end < char_positions.len() {
        "..."
    } else {
        ""
    };
    let body = text
        .chars()
        .skip(window_start)
        .take(window_end.saturating_sub(window_start))
        .collect::<String>();
    format!("{prefix}{}{suffix}", summarize_search_snippet(&body))
}

fn join_search_fields(fields: &[&str]) -> String {
    fields
        .iter()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn summarize_search_snippet(text: &str) -> String {
    let collapsed = text
        .split_whitespace()
        .filter(|part| !part.trim().is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    first_line(
        if collapsed.is_empty() {
            text
        } else {
            &collapsed
        },
        140,
    )
}

fn build_search_haystack(result: &GlobalSearchResult) -> String {
    join_search_fields(&[
        &result.project_name,
        &result.content_type,
        &result.title,
        &result.snippet,
        &result.workspace_relative_path,
        &result.hash,
        &result.file_type,
        &result.ownership_type,
        &result.category,
        &result.document_type,
        &result.document_purpose,
        &result.business_domain,
        &result.lifecycle_status,
        &result.duplicate_of,
        &result.version_group_id,
        &result.recent_status,
    ])
}

fn workspace_file_recent_status(file: &GlobalManagedFile, lifecycle_status: &str) -> String {
    match lifecycle_status {
        "missing" => "Workspace 文件缺失，记录保留。".to_string(),
        "superseded" => "已撤销或被新版本替代。".to_string(),
        "managed" => format!(
            "{} 已整理",
            if file.created_at.trim().is_empty() {
                "Workspace 文件".to_string()
            } else {
                file.created_at.clone()
            }
        ),
        "received" => "已接收，等待进一步处理。".to_string(),
        "active" => "近期活跃。".to_string(),
        "versioned" => "已建立版本关系。".to_string(),
        "archived" => "已归档。".to_string(),
        _ => lifecycle_status.to_string(),
    }
}

fn workspace_category_display(file: &GlobalManagedFile) -> String {
    [
        file.general_material_domain.as_str(),
        file.general_material_category.as_str(),
        file.category.as_str(),
        file.destination.as_str(),
    ]
    .into_iter()
    .find(|value| !value.trim().is_empty())
    .unwrap_or("Workspace")
    .to_string()
}

fn confidence_display_from_score(score: u8) -> String {
    if score == 0 {
        return String::new();
    }
    let level = if score >= 80 {
        "高"
    } else if score >= 60 {
        "中"
    } else {
        "低"
    };
    format!("{level} {score}%")
}

fn normalize_search_text(text: &str) -> String {
    text.chars()
        .flat_map(char::to_lowercase)
        .filter(|ch| !ch.is_control())
        .collect::<String>()
}

fn match_search_score(haystack: &str, normalized_query: &str) -> Option<i64> {
    let normalized_haystack = normalize_search_text(haystack);
    if normalized_haystack.is_empty() {
        return None;
    }
    let query_tokens = normalized_query
        .split_whitespace()
        .filter(|token| !token.trim().is_empty())
        .collect::<Vec<_>>();
    let tokens = if query_tokens.is_empty() {
        vec![normalized_query]
    } else {
        query_tokens
    };
    let mut score = 0_i64;
    for token in tokens {
        if normalized_haystack == token {
            score += 240;
            continue;
        }
        if normalized_haystack.contains(token) {
            score += 120 + (token.chars().count() as i64 * 4);
            continue;
        }
        if let Some(distance) = subsequence_distance(&normalized_haystack, token) {
            score += 32_i64.saturating_sub(distance as i64).max(6);
            continue;
        }
        return None;
    }
    Some(score)
}

fn subsequence_distance(haystack: &str, needle: &str) -> Option<usize> {
    let haystack_chars = haystack.chars().collect::<Vec<_>>();
    let needle_chars = needle.chars().collect::<Vec<_>>();
    if needle_chars.is_empty() {
        return Some(0);
    }
    let mut first_index = None;
    let mut last_index = 0_usize;
    let mut needle_index = 0_usize;
    for (index, ch) in haystack_chars.iter().enumerate() {
        if *ch == needle_chars[needle_index] {
            if first_index.is_none() {
                first_index = Some(index);
            }
            last_index = index;
            needle_index += 1;
            if needle_index == needle_chars.len() {
                break;
            }
        }
    }
    if needle_index == needle_chars.len() {
        Some(last_index.saturating_sub(first_index.unwrap_or(0)))
    } else {
        None
    }
}

fn waiting_atlas() -> AtlasAssessment {
    AtlasAssessment {
        atlas_version: String::new(),
        status: "waiting_for_integration".to_string(),
        reusable_parts: Vec::new(),
        uncovered_parts: vec!["等待只读 Atlas Adapter 接入后评估。".to_string()],
        training_candidates: Vec::new(),
        evidence_references: Vec::new(),
        review_required: true,
        assessed_at: now_string(),
        failure_reason: String::new(),
    }
}

fn workspace_message(
    author: &str,
    kind: &str,
    text: impl Into<String>,
    source: &str,
) -> WorkspaceMessage {
    workspace_message_with_status(author, kind, text, source, "completed", "")
}

fn workspace_message_with_status(
    author: &str,
    kind: &str,
    text: impl Into<String>,
    source: &str,
    status: &str,
    model_id: &str,
) -> WorkspaceMessage {
    WorkspaceMessage {
        id: new_id(),
        author: author.to_string(),
        kind: kind.to_string(),
        text: text.into(),
        created_at: now_string(),
        status: status.to_string(),
        attachments: Vec::new(),
        related_task: String::new(),
        source: source.to_string(),
        model_id: model_id.to_string(),
        evidence_items: Vec::new(),
    }
}

fn record_message_derivatives(manifest: &mut ProjectManifest, message: &WorkspaceMessage) {
    record_message_in_session(manifest, &message.id);
    if message.kind == "restore" {
        for task in &mut manifest.tasks {
            if task.status == "active" {
                task.status = "paused".to_string();
                task.updated_at = now_string();
            }
        }
    }
    if message.author != "user" && looks_like_artifact(&message.text) {
        let artifact = crate::models::ArtifactRecord {
            id: new_id(),
            title: first_line(&message.text, 100),
            artifact_type: message.kind.clone(),
            source: message.source.clone(),
            source_message_id: message.id.clone(),
            file_id: String::new(),
            managed_path: String::new(),
            related_task: message.related_task.clone(),
            created_at: now_string(),
        };
        record_artifact_in_session(manifest, &artifact.id);
        manifest.artifacts.push(artifact);
    }
    for attachment in &message.attachments {
        if attachment.attachment_type == "image" {
            continue;
        }
        let artifact = crate::models::ArtifactRecord {
            id: new_id(),
            title: attachment.file_name.clone(),
            artifact_type: "file".to_string(),
            source: message.source.clone(),
            source_message_id: message.id.clone(),
            file_id: attachment.file_id.clone(),
            managed_path: attachment.managed_path.clone(),
            related_task: message.related_task.clone(),
            created_at: now_string(),
        };
        record_artifact_in_session(manifest, &artifact.id);
        manifest.artifacts.push(artifact);
    }
}

// Requirement messages are context, not confirmed work items. Older versions
// derived active tasks from every user message; retain those records for
// history, but do not let them drive today's work without confirmation.
fn task_is_confirmed_project_task(manifest: &ProjectManifest, task: &TaskRecord) -> bool {
    !manifest.messages.iter().any(|message| {
        message.id == task.source_message_id
            && message.author == "user"
            && message.kind == "requirement"
    })
}

fn extract_explicit_recorded_decision(text: &str) -> Option<String> {
    const DECISION_MARKERS: [&str; 3] = ["我决定", "我确认", "决定采用"];
    const RECORD_MARKERS: [&str; 7] = [
        "请把这个决定记下来",
        "请记下来",
        "记录下来",
        "保存这个决定",
        "帮我记住这个决定",
        "请记录这个决定",
        "请记录下来",
    ];

    let (decision_start, decision_marker) = DECISION_MARKERS
        .iter()
        .filter_map(|marker| text.find(marker).map(|start| (start, *marker)))
        .min_by_key(|(start, _)| *start)?;
    let marker_end = decision_start + decision_marker.len();
    let record_start = RECORD_MARKERS
        .iter()
        .filter_map(|marker| {
            text[marker_end..]
                .find(marker)
                .map(|offset| marker_end + offset)
        })
        .min()?;
    if record_start <= marker_end {
        return None;
    }

    let decision = text[marker_end..record_start]
        .trim()
        .trim_matches(|character: char| "：:，,；;。".contains(character))
        .trim();
    (!decision.is_empty()).then(|| decision.to_string())
}

fn extract_explicit_recorded_next_step(text: &str) -> Option<String> {
    const NEXT_STEP_MARKERS: [&str; 2] = ["下一步", "接下来"];
    const RECORD_MARKERS: [&str; 8] = [
        "请把这个下一步记下来",
        "保存这个下一步",
        "请保存这个下一步",
        "帮我记住这个下一步",
        "记住这个下一步",
        "请记下来",
        "记录下来",
        "帮我记住",
    ];

    let (next_step_start, marker) = NEXT_STEP_MARKERS
        .iter()
        .filter_map(|marker| text.find(marker).map(|start| (start, *marker)))
        .min_by_key(|(start, _)| *start)?;
    let leading_context = text[..next_step_start]
        .trim_end()
        .trim_end_matches(|character: char| "：:，,；;。 ".contains(character));
    if ["建议", "推荐", "应该", "可以", "AI", "助手"]
        .iter()
        .any(|marker| leading_context.ends_with(marker))
    {
        return None;
    }
    let marker_end = next_step_start + marker.len();
    let record_start = RECORD_MARKERS
        .iter()
        .filter_map(|marker| {
            text[marker_end..]
                .find(marker)
                .map(|offset| marker_end + offset)
        })
        .min()?;
    if record_start <= marker_end {
        return None;
    }

    let mut next_step = text[marker_end..record_start]
        .trim()
        .trim_matches(|character: char| "：:，,；;。".contains(character))
        .trim()
        .to_string();
    for prefix in ["我准备", "我会", "我将", "准备", "打算"] {
        if let Some(rest) = next_step.strip_prefix(prefix) {
            next_step = rest
                .trim()
                .trim_matches(|character: char| "：:，,；;。".contains(character))
                .trim()
                .to_string();
            break;
        }
    }
    (!next_step.is_empty()).then_some(next_step)
}

fn looks_like_artifact(text: &str) -> bool {
    ["方案", "提示词", "报告", "结果", "产出", "保存"]
        .iter()
        .any(|word| text.contains(word))
}

fn first_line(text: &str, max_chars: usize) -> String {
    let line = text
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or(text)
        .trim();
    let mut result = line.chars().take(max_chars).collect::<String>();
    if line.chars().count() > max_chars {
        result.push_str("...");
    }
    result
}

fn record_message_in_session(manifest: &mut ProjectManifest, id: &str) {
    let session = active_daily_session(manifest);
    push_unique(&mut session.message_ids, id);
}

fn record_artifact_in_session(manifest: &mut ProjectManifest, id: &str) {
    let session = active_daily_session(manifest);
    push_unique(&mut session.artifact_ids, id);
}

fn record_imported_files_in_session(manifest: &mut ProjectManifest, ids: Vec<String>) {
    let session = active_daily_session(manifest);
    for id in ids {
        push_unique(&mut session.imported_file_ids, &id);
    }
}

fn record_recovery_in_session(manifest: &mut ProjectManifest, id: &str) {
    let session = active_daily_session(manifest);
    push_unique(&mut session.recovery_point_ids, id);
}

fn active_daily_session(manifest: &mut ProjectManifest) -> &mut crate::models::DailySession {
    let date_key = current_date_key();
    if let Some(index) = manifest
        .daily_sessions
        .iter()
        .position(|session| session.date_key == date_key)
    {
        manifest.daily_sessions[index].updated_at = now_string();
        return &mut manifest.daily_sessions[index];
    }
    manifest.daily_sessions.push(crate::models::DailySession {
        id: new_id(),
        date_key,
        started_at: now_string(),
        updated_at: now_string(),
        message_ids: Vec::new(),
        task_ids: Vec::new(),
        decision_ids: Vec::new(),
        artifact_ids: Vec::new(),
        imported_file_ids: Vec::new(),
        recovery_point_ids: Vec::new(),
    });
    manifest
        .daily_sessions
        .last_mut()
        .expect("daily session exists")
}

fn current_date_key() -> String {
    let millis = now_string().parse::<u128>().unwrap_or_default();
    format!("day-{}", millis / 86_400_000)
}

fn push_unique(values: &mut Vec<String>, value: &str) {
    if !value.is_empty() && !values.iter().any(|item| item == value) {
        values.push(value.to_string());
    }
}

fn ensure_deepseek_authorization(manifest: &ProjectManifest) -> Result<(), String> {
    if manifest.deepseek_authorization.granted_at.is_empty() {
        return Err("首次发送项目信息到 DeepSeek 前，请先在工作台明确授权。".to_string());
    }
    Ok(())
}

fn build_deepseek_context(
    manifest: &ProjectManifest,
    message_id: &str,
    model_id: &str,
) -> Result<serde_json::Value, String> {
    let current_question = current_question_for_message(manifest, message_id);
    let evidence_items = manifest
        .messages
        .iter()
        .find(|item| item.id == message_id)
        .map(|item| item.evidence_items.clone())
        .unwrap_or_default();
    let facts = evidence_items
        .iter()
        .filter(|item| item.basis_kind == "fact")
        .map(|item| {
            format!(
                "{}｜{}｜{}",
                item.title,
                item.content_type,
                empty_label(&item.summary, "无摘要")
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let pending = evidence_items
        .iter()
        .filter(|item| item.basis_kind == "pending")
        .map(|item| {
            format!(
                "{}｜{}｜{}",
                item.title,
                item.content_type,
                empty_label(&item.summary, "待人工确认")
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let has_grounded_project_material = evidence_items.iter().any(|item| {
        item.basis_kind == "fact"
            && matches!(
                item.content_type.as_str(),
                "file" | "task" | "decision" | "outcome" | "projectState" | "projectAnalysis"
            )
    });
    let reference_material = reference_files_for_question(manifest, &current_question)
        .into_iter()
        .filter(|file| reference_body_is_allowed(file, &current_question))
        .map(|file| build_reference_material(manifest, file, &current_question))
        .collect::<Vec<_>>();
    let recorded_next_step = (!manifest.project.next_step.trim().is_empty()
        && !is_system_generated_next_step(manifest, &manifest.project.next_step))
    .then(|| context_text(&manifest.project.next_step, 180));
    let image_attachments = image_attachments_for_message(manifest, message_id);
    let mut image_parts = Vec::new();
    let image_context_note = if image_attachments.is_empty() {
        String::new()
    } else if !deepseek::model_supports_vision(model_id) {
        " 当前消息包含图片附件，但当前 DeepSeek 模型不支持视觉输入；不得声称已经看到了图片，回答只能依据文字和项目资料。".to_string()
    } else {
        let mut unreadable = 0_usize;
        for attachment in &image_attachments {
            if attachment.relative_path.trim().is_empty() {
                unreadable += 1;
                continue;
            }
            match read_chat_attachment_data(
                Path::new(&manifest.project.root_dir),
                &attachment.relative_path,
            ) {
                Ok(data) => image_parts.push(json!({
                    "type": "image_url",
                    "image_url": { "url": data.data_url, "detail": "auto" }
                })),
                Err(_) => unreadable += 1,
            }
        }
        if image_parts.is_empty() {
            " 当前消息包含图片附件，但图片暂时不可读取；不得根据文件名猜测图片内容。".to_string()
        } else if unreadable > 0 {
            format!(" 当前消息已发送可读取的图片附件；另有 {unreadable} 张图片不可读取，不得猜测其内容。")
        } else {
            " 当前消息已包含图片视觉输入；请只依据实际图片和文字回答，不要把推断写成图片事实。"
                .to_string()
        }
    };
    let system_prompt = format!(
        "你是感冒院的项目专属工作助手。上下文优先级固定为：当前项目真实资料、Decision Trace、任务/决定/成果、项目分析，最后才可使用明确标注的通用常识。不得补充资料中不存在的组织背景、技术栈、历史或目标，不要假装看到了未列出的原始文件。回答固定区分“已知事实”“AI总结”“建议”；待确认内容不能写成事实。{} 项目名称：{}。资料事实：{}。待确认内容：{}。{}{}{}",
        if has_grounded_project_material {
            "所有事实必须引用下方依据。"
        } else {
            "当前项目资料不足，无法确认。只能说明缺少哪些依据，不得猜测或补全背景。"
        },
        manifest.project.name,
        if facts.is_empty() { "无项目资料依据" } else { &facts },
        if pending.is_empty() { "暂无待确认内容" } else { &pending },
        recorded_next_step
            .as_deref()
            .map(|value| format!(" 当前已记录的用户下一步：{}。它是已保存事实，不要称为草稿、待确认事项或要求用户再次确认。", value))
            .unwrap_or_default(),
        if reference_material.is_empty() {
            String::new()
        } else {
            format!(
                " 本次问题明确命中了当前项目的参考资料。以下只提供有限的文件摘要和正文片段；正文不可用时只能依据摘要回答，并明确说明依据摘要：\n{}",
                reference_material.join("\n\n")
            )
        },
        image_context_note,
    );
    let user_content = if image_parts.is_empty() {
        json!(current_question)
    } else {
        let mut parts = vec![json!({ "type": "text", "text": current_question })];
        parts.extend(image_parts);
        serde_json::Value::Array(parts)
    };
    Ok(json!([
        { "role": "system", "content": system_prompt },
        { "role": "user", "content": user_content }
    ]))
}

fn image_attachments_for_message<'a>(
    manifest: &'a ProjectManifest,
    message_id: &str,
) -> Vec<&'a MessageAttachment> {
    let Some(index) = manifest
        .messages
        .iter()
        .position(|message| message.id == message_id)
    else {
        return Vec::new();
    };

    let message = &manifest.messages[index];
    if message.author == "user" {
        return message
            .attachments
            .iter()
            .filter(|attachment| attachment.attachment_type == "image")
            .collect();
    }

    manifest.messages[..index]
        .iter()
        .rev()
        .find(|candidate| candidate.author == "user")
        .map(|candidate| {
            candidate
                .attachments
                .iter()
                .filter(|attachment| attachment.attachment_type == "image")
                .collect()
        })
        .unwrap_or_default()
}

fn reference_files_for_question<'a>(
    manifest: &'a ProjectManifest,
    question: &str,
) -> Vec<&'a FileRecord> {
    let normalized_query = normalize_search_text(question);
    let mut matched = manifest
        .files
        .iter()
        .filter(|file| !unhealthy_source(manifest, "file", &file.id))
        .filter(|file| {
            matches_query(
                &join_search_fields(&[
                    &file.file_name,
                    &file.content_summary,
                    &file.recommended_category,
                    &file.category,
                    &file.main_fields_or_sections.join("；"),
                ]),
                &normalized_query,
            )
        })
        .take(AI_REFERENCE_FILE_LIMIT)
        .collect::<Vec<_>>();

    if matched.is_empty() && asks_for_reference_material(question) {
        matched = manifest
            .files
            .iter()
            .rev()
            .filter(|file| !unhealthy_source(manifest, "file", &file.id))
            .take(AI_REFERENCE_FILE_LIMIT)
            .collect();
    }
    matched
}

fn asks_for_reference_material(question: &str) -> bool {
    [
        "这个文件",
        "这份文件",
        "这份资料",
        "这份文档",
        "刚才加入",
        "刚加入",
        "刚才导入",
        "刚导入",
        "当前参考资料",
        "文件内容",
        "文档内容",
        "资料内容",
        "正文",
        "摘要",
        "清单",
        "manifest",
    ]
    .iter()
    .any(|marker| {
        question
            .to_ascii_lowercase()
            .contains(&marker.to_ascii_lowercase())
    })
}

fn reference_body_is_allowed(file: &FileRecord, question: &str) -> bool {
    if asks_for_reference_material(question) {
        return true;
    }
    let normalized_query = normalize_search_text(question);
    let normalized_name = normalize_search_text(&file.file_name);
    if normalized_name.len() >= 5 && normalized_query.contains(&normalized_name) {
        return true;
    }
    let stem = file
        .file_name
        .rsplit_once('.')
        .map(|(value, _)| value)
        .unwrap_or(&file.file_name);
    stem.chars().count() >= 6 && normalized_query.contains(&normalize_search_text(stem))
}

fn build_reference_material(
    manifest: &ProjectManifest,
    file: &FileRecord,
    question: &str,
) -> String {
    let name = context_text(&file.file_name, 160);
    let category = context_text(
        if file.recommended_category.trim().is_empty() {
            &file.category
        } else {
            &file.recommended_category
        },
        80,
    );
    let summary = context_excerpt(&file.content_summary, 420);
    let excerpt = read_project_reference_text(manifest, file)
        .map(|text| reference_text_excerpt(&text, question))
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| "正文片段不可用，只能依据已登记解析摘要。".to_string());
    format!(
        "- 文件：{}\n  分类：{}\n  解析摘要：{}\n  有限正文片段：{}",
        if name.is_empty() {
            "未命名资料"
        } else {
            &name
        },
        if category.is_empty() {
            "未分类"
        } else {
            &category
        },
        if summary.is_empty() {
            "暂无摘要"
        } else {
            &summary
        },
        excerpt,
    )
}

fn read_project_reference_text(manifest: &ProjectManifest, file: &FileRecord) -> Option<String> {
    let root = fs::canonicalize(&manifest.project.root_dir).ok()?;
    let candidates = [&file.extracted_text_path, &file.managed_path];
    candidates.iter().find_map(|value| {
        if value.trim().is_empty() {
            return None;
        }
        let path = fs::canonicalize(value).ok()?;
        if !path.starts_with(&root) || !path.is_file() {
            return None;
        }
        read_optional_text(&path)
    })
}

fn reference_text_excerpt(text: &str, question: &str) -> String {
    let normalized_text = normalize_search_text(text);
    let normalized_query = normalize_search_text(question);
    let related = normalized_query
        .split_whitespace()
        .filter(|token| token.chars().count() >= 2)
        .find_map(|token| {
            normalized_text
                .find(token)
                .map(|start| excerpt_by_byte_range(text, start, start + token.len(), 360))
        });
    let preview = text.lines().take(18).collect::<Vec<_>>().join("\n");
    let combined = match related {
        Some(related) if !preview.contains(&related) => format!("{preview}\n相关片段：{related}"),
        _ => preview,
    };
    context_excerpt(&combined, AI_REFERENCE_EXCERPT_CHARS)
}

fn context_excerpt(value: &str, max_chars: usize) -> String {
    let mut lines = Vec::new();
    let mut length = 0;
    for line in value.lines().map(str::trim).filter(|line| !line.is_empty()) {
        let safe = context_text(line, 360);
        if safe.is_empty() {
            continue;
        }
        let next_length = length + safe.chars().count() + usize::from(!lines.is_empty());
        if next_length > max_chars {
            break;
        }
        length = next_length;
        lines.push(safe);
    }
    lines.join("\n")
}

fn current_question_for_message(manifest: &ProjectManifest, message_id: &str) -> String {
    if let Some(index) = manifest
        .messages
        .iter()
        .position(|item| item.id == message_id)
    {
        for item in manifest.messages[..index].iter().rev() {
            if item.author == "user" && !item.text.trim().is_empty() {
                return item.text.clone();
            }
        }
    }
    manifest
        .messages
        .iter()
        .rev()
        .find(|item| item.author == "user" && !item.text.trim().is_empty())
        .map(|item| item.text.clone())
        .unwrap_or_default()
}

fn collect_answer_evidence(
    manifest: &ProjectManifest,
    current_question: &str,
) -> Vec<MessageEvidenceItem> {
    let mut evidence_items = Vec::new();
    let normalized_query = normalize_search_text(current_question);
    let asks_project_status = ["现在", "情况", "进展", "状态", "风险", "阻塞", "下一步"]
        .iter()
        .any(|signal| current_question.contains(signal));

    if !manifest.project.description.trim().is_empty()
        || !manifest.project.next_step.trim().is_empty()
    {
        evidence_items.push(MessageEvidenceItem {
            id: new_id(),
            title: "项目说明".to_string(),
            content_type: "project".to_string(),
            basis_kind: "fact".to_string(),
            summary: summarize_search_snippet(&join_search_fields(&[
                &manifest.project.description,
                &format!("下一步：{}", manifest.project.next_step),
            ])),
            managed_path: String::new(),
            source_record_id: manifest.project.id.clone(),
        });
    }

    for file in reference_files_for_question(manifest, current_question)
        .into_iter()
        .take(4)
    {
        evidence_items.push(MessageEvidenceItem {
            id: new_id(),
            title: file.file_name.clone(),
            content_type: "file".to_string(),
            basis_kind: "fact".to_string(),
            summary: summarize_search_snippet(&join_search_fields(&[
                &file.content_summary,
                &file.main_fields_or_sections.join("；"),
            ])),
            managed_path: file.managed_path.clone(),
            source_record_id: file.id.clone(),
        });
    }

    for message in manifest
        .messages
        .iter()
        .rev()
        .filter(|message| message.status == "completed" && !message.text.trim().is_empty())
        .filter(|message| message.kind != "assistant")
        .filter(|message| matches_query(&message.text, &normalized_query))
        .take(3)
    {
        evidence_items.push(MessageEvidenceItem {
            id: new_id(),
            title: if message.author == "user" {
                "历史对话".to_string()
            } else {
                "工作台记录".to_string()
            },
            content_type: "message".to_string(),
            basis_kind: "fact".to_string(),
            summary: summarize_search_snippet(&message.text),
            managed_path: String::new(),
            source_record_id: message.id.clone(),
        });
    }

    for decision in manifest
        .decisions
        .iter()
        .rev()
        .filter(|decision| matches_query(&decision.summary, &normalized_query))
        .take(3)
    {
        evidence_items.push(MessageEvidenceItem {
            id: new_id(),
            title: "项目决定".to_string(),
            content_type: "decision".to_string(),
            basis_kind: "fact".to_string(),
            summary: summarize_search_snippet(&decision.summary),
            managed_path: String::new(),
            source_record_id: decision.id.clone(),
        });
    }

    for task in manifest
        .tasks
        .iter()
        .rev()
        .filter(|task| !unhealthy_source(manifest, "task", &task.id))
        .filter(|task| asks_project_status || matches_query(&task.title, &normalized_query))
        .take(4)
    {
        evidence_items.push(MessageEvidenceItem {
            id: new_id(),
            title: "项目任务".to_string(),
            content_type: "task".to_string(),
            basis_kind: "fact".to_string(),
            summary: format!("{}｜状态：{}", task.title, task.status),
            managed_path: String::new(),
            source_record_id: task.id.clone(),
        });
    }

    for artifact in manifest
        .artifacts
        .iter()
        .rev()
        .filter(|artifact| !unhealthy_source(manifest, "artifact", &artifact.id))
        .filter(|artifact| asks_project_status || matches_query(&artifact.title, &normalized_query))
        .take(3)
    {
        evidence_items.push(MessageEvidenceItem {
            id: new_id(),
            title: artifact.title.clone(),
            content_type: "outcome".to_string(),
            basis_kind: "fact".to_string(),
            summary: format!(
                "成果类型：{}；来源：{}",
                artifact.artifact_type, artifact.source
            ),
            managed_path: artifact.managed_path.clone(),
            source_record_id: artifact.id.clone(),
        });
    }

    if asks_project_status && !manifest.project_state_summary.facts.is_empty() {
        evidence_items.push(MessageEvidenceItem {
            id: new_id(),
            title: "项目状态摘要".to_string(),
            content_type: "projectState".to_string(),
            basis_kind: "fact".to_string(),
            summary: summarize_search_snippet(&manifest.project_state_summary.facts.join("；")),
            managed_path: String::new(),
            source_record_id: manifest.project.id.clone(),
        });
        if !manifest.project_state_summary.inferences.is_empty() {
            evidence_items.push(MessageEvidenceItem {
                id: new_id(),
                title: "项目阶段判断".to_string(),
                content_type: "projectState".to_string(),
                basis_kind: "inference".to_string(),
                summary: summarize_search_snippet(
                    &manifest.project_state_summary.inferences.join("；"),
                ),
                managed_path: String::new(),
                source_record_id: manifest.project_state_summary.decision_trace.id.clone(),
            });
        }
    }

    for trace in manifest
        .project_action_candidates
        .iter()
        .rev()
        .map(|item| &item.decision_trace)
        .filter(|trace| !trace.id.is_empty())
        .take(3)
    {
        let summary = format!("{}；{}", trace.ai_understanding, trace.recommendation);
        if asks_project_status || matches_query(&summary, &normalized_query) {
            evidence_items.push(MessageEvidenceItem {
                id: new_id(),
                title: "Decision Trace".to_string(),
                content_type: "decisionTrace".to_string(),
                basis_kind: "pending".to_string(),
                summary: summarize_search_snippet(&summary),
                managed_path: String::new(),
                source_record_id: trace.id.clone(),
            });
        }
    }

    for pattern in manifest
        .work_pattern_candidates
        .iter()
        .filter(|pattern| asks_project_status || matches_query(&pattern.name, &normalized_query))
        .take(2)
    {
        evidence_items.push(MessageEvidenceItem {
            id: new_id(),
            title: pattern.name.clone(),
            content_type: "weeklyReview".to_string(),
            basis_kind: "pending".to_string(),
            summary: format!("出现 {} 次；仍需人工评审。", pattern.occurrence_count),
            managed_path: String::new(),
            source_record_id: pattern.decision_trace.id.clone(),
        });
    }

    let project_analysis_is_healthy = !manifest
        .data_health_records
        .iter()
        .any(|record| record.status == "open" && record.health_type == "notSuitableForAIContext");
    if manifest.project_analysis.status == "success" && project_analysis_is_healthy {
        let analysis_fact = join_search_fields(&[
            &manifest.project_analysis.project_definition,
            &manifest.project_analysis.known_requirements.join("；"),
            &manifest.project_analysis.constraints.join("；"),
            &manifest.project_analysis.evidence.join("；"),
        ]);
        if !analysis_fact.is_empty() && matches_query(&analysis_fact, &normalized_query) {
            evidence_items.push(MessageEvidenceItem {
                id: new_id(),
                title: "项目分析".to_string(),
                content_type: "projectAnalysis".to_string(),
                basis_kind: "fact".to_string(),
                summary: summarize_search_snippet(&analysis_fact),
                managed_path: String::new(),
                source_record_id: manifest.project.id.clone(),
            });
        }
        let analysis_pending = join_search_fields(&[
            &manifest.project_analysis.gaps.join("；"),
            &manifest.project_analysis.questions.join("；"),
        ]);
        if !analysis_pending.is_empty() && matches_query(&analysis_pending, &normalized_query) {
            evidence_items.push(MessageEvidenceItem {
                id: new_id(),
                title: "待确认分析项".to_string(),
                content_type: "projectAnalysis".to_string(),
                basis_kind: "pending".to_string(),
                summary: summarize_search_snippet(&analysis_pending),
                managed_path: String::new(),
                source_record_id: manifest.project.id.clone(),
            });
        }
    }

    for review in manifest
        .pending_reviews
        .iter()
        .rev()
        .filter(|review| {
            matches_query(
                &join_search_fields(&[&review.title, &review.detail]),
                &normalized_query,
            )
        })
        .take(3)
    {
        evidence_items.push(MessageEvidenceItem {
            id: new_id(),
            title: review.title.clone(),
            content_type: "pendingReview".to_string(),
            basis_kind: "pending".to_string(),
            summary: summarize_search_snippet(&review.detail),
            managed_path: review.path.clone(),
            source_record_id: review.id.clone(),
        });
    }

    dedupe_evidence_items(evidence_items)
}

fn matches_query(haystack: &str, normalized_query: &str) -> bool {
    if normalized_query.trim().is_empty() {
        return !haystack.trim().is_empty();
    }
    match_search_score(haystack, normalized_query).is_some()
}

fn dedupe_evidence_items(items: Vec<MessageEvidenceItem>) -> Vec<MessageEvidenceItem> {
    let mut seen = HashSet::new();
    let mut deduped = Vec::new();
    for item in items {
        let key = format!(
            "{}|{}|{}|{}",
            item.basis_kind,
            item.content_type,
            item.source_record_id,
            normalize_key(&item.managed_path)
        );
        if seen.insert(key) {
            deduped.push(item);
        }
    }
    deduped
}

fn finalize_project_message<R: Runtime>(
    _app: &tauri::AppHandle<R>,
    project_root: &str,
    message_id: &str,
    model_id: &str,
    text: &str,
) -> Result<(), String> {
    let root = PathBuf::from(project_root);
    let _guard = project_write_lock()
        .lock()
        .map_err(|_| "项目存储锁已损坏，请重试。".to_string())?;
    let mut manifest = read_and_repair_manifest(&root)?;
    if let Some(message) = manifest
        .messages
        .iter_mut()
        .find(|item| item.id == message_id)
    {
        message.text = text.to_string();
        message.status = "completed".to_string();
        message.source = "deepseek".to_string();
        message.model_id = model_id.to_string();
        if !message
            .evidence_items
            .iter()
            .any(|item| item.basis_kind == "inference")
        {
            message.evidence_items.push(MessageEvidenceItem {
                id: new_id(),
                title: "AI 推断".to_string(),
                content_type: "inference".to_string(),
                basis_kind: "inference".to_string(),
                summary: if message
                    .evidence_items
                    .iter()
                    .any(|item| item.basis_kind == "fact")
                {
                    "本条回答由 DeepSeek 基于以上项目资料、历史记录和当前问题整理推断。".to_string()
                } else {
                    "本条回答未命中可引用的项目资料，属于基于当前问题的通用推断。".to_string()
                },
                managed_path: String::new(),
                source_record_id: message.id.clone(),
            });
        }
    }
    if let Some(message) = manifest
        .messages
        .iter()
        .find(|item| item.id == message_id)
        .cloned()
    {
        record_message_derivatives(&mut manifest, &message);
    }
    manifest.project.last_opened_at = now_string();
    manifest.audit.push(audit_event(
        "deepseek.chat",
        project_root,
        "success",
        format!("模型：{model_id}"),
        false,
        true,
    ));
    persist_project(&root, &manifest, None)?;
    append_audit_event(&root, manifest.audit.last().expect("audit event exists"))?;
    Ok(())
}

fn audit_event(
    action: &str,
    target: &str,
    outcome: &str,
    details: String,
    requires_confirmation: bool,
    confirmed: bool,
) -> AuditEvent {
    AuditEvent {
        id: new_id(),
        action: action.to_string(),
        target: target.to_string(),
        outcome: outcome.to_string(),
        created_at: now_string(),
        details,
        requires_confirmation,
        confirmed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::test_root;

    #[test]
    fn import_is_transactional_when_any_source_is_missing() {
        let root = test_root("transaction");
        fs::create_dir_all(&root).unwrap();
        let source = root.join("存在.txt");
        fs::write(&source, "真实资料").unwrap();
        let missing = root.join("不存在.txt");
        let mut records = Vec::new();

        let error = import_into_records(
            &root,
            &mut records,
            vec![path_to_string(&source), path_to_string(&missing)],
            "用户导入",
            "测试",
            None,
        )
        .unwrap_err();

        assert!(error.contains("导入前校验失败"));
        assert!(records.is_empty());
        assert_eq!(
            fs::read_dir(root.join(".ganmaoyuan/managed/02_requirements"))
                .unwrap()
                .count(),
            0
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn duplicate_and_version_relationships_are_recorded_without_overwrite() {
        let root = test_root("version");
        let source_dir = root.join("source");
        let project_dir = root.join("project");
        fs::create_dir_all(&source_dir).unwrap();
        fs::create_dir_all(&project_dir).unwrap();
        let first = source_dir.join("需求.txt");
        let duplicate = source_dir.join("需求副本.txt");
        let second = source_dir.join("需求-v2.txt");
        fs::write(&first, "第一版需求").unwrap();
        fs::write(&duplicate, "第一版需求").unwrap();
        fs::write(&second, "第二版需求").unwrap();
        let mut records = Vec::new();

        let first_batch = import_into_records(
            &project_dir,
            &mut records,
            vec![path_to_string(&first)],
            "用户导入",
            "初始化",
            None,
        )
        .unwrap();
        let first_managed = first_batch.new_records[0].managed_path.clone();
        let duplicate_batch = import_into_records(
            &project_dir,
            &mut records,
            vec![path_to_string(&duplicate)],
            "用户导入",
            "补充",
            None,
        )
        .unwrap();
        assert_eq!(duplicate_batch.duplicates.len(), 1);
        assert_eq!(duplicate_batch.new_records[0].managed_path, first_managed);
        assert!(duplicate_batch.new_records[0]
            .duplicate_of_file_id
            .is_some());

        let version_batch = import_into_records(
            &project_dir,
            &mut records,
            vec![path_to_string(&second)],
            "用户导入",
            "补充",
            None,
        )
        .unwrap();
        assert_eq!(version_batch.new_records[0].current_version, "v2");
        assert!(version_batch.new_records[0].previous_version_id.is_some());
        assert!(version_batch.new_records[0]
            .managed_path
            .ends_with("需求-v2.txt"));
        assert_ne!(version_batch.new_records[0].managed_path, first_managed);
        assert_eq!(fs::read_to_string(first_managed).unwrap(), "第一版需求");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn atomic_json_keeps_a_readable_backup() {
        let root = test_root("atomic");
        fs::create_dir_all(&root).unwrap();
        let path = root.join("data.json");
        write_json_atomic(&path, &vec!["first".to_string()]).unwrap();
        write_json_atomic(&path, &vec!["second".to_string()]).unwrap();
        let current: Vec<String> = read_json(&path).unwrap();
        let backup: Vec<String> = read_json(&path.with_file_name("data.json.bak")).unwrap();
        assert_eq!(current, vec!["second"]);
        assert_eq!(backup, vec!["first"]);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn workspace_scanner_handles_empty_directory() {
        let root = test_root("workspace-scanner-empty");
        fs::create_dir_all(&root).unwrap();
        let files = collect_workspace_scan_files(&root, "batch-empty").unwrap();
        assert!(files.is_empty());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn workspace_scanner_records_regular_files_and_chinese_paths() {
        let root = test_root("workspace-scanner-cn");
        let source = root.join("中文 资料");
        fs::create_dir_all(&source).unwrap();
        let file = source.join("需求 文档.txt");
        fs::write(&file, "这是中文测试资料").unwrap();

        let files = collect_workspace_scan_files(&root, "batch-cn").unwrap();

        assert_eq!(files.len(), 1);
        assert_eq!(files[0].file_name, "需求 文档.txt");
        assert_eq!(files[0].extension, "txt");
        assert_eq!(files[0].scan_batch_id, "batch-cn");
        assert!(files[0].path.contains("中文 资料"));
        assert!(files[0].size_bytes > 0);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn workspace_scanner_accepts_long_but_normal_paths() {
        let root = test_root("workspace-scanner-long");
        let source = root.join("很长的目录名称用于验证扫描器不会因为普通长路径失败");
        fs::create_dir_all(&source).unwrap();
        let file = source.join("一份名称也比较长的项目资料说明文件.txt");
        fs::write(&file, "长路径测试").unwrap();

        let files = collect_workspace_scan_files(&root, "batch-long").unwrap();

        assert_eq!(files.len(), 1);
        assert!(files[0].path.contains("一份名称也比较长"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn workspace_scanner_excludes_build_and_cache_directories() {
        let root = test_root("workspace-scanner-excludes");
        fs::create_dir_all(root.join("node_modules/pkg")).unwrap();
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::create_dir_all(root.join("target/debug")).unwrap();
        fs::write(root.join("node_modules/pkg/ignored.txt"), "ignore").unwrap();
        fs::write(root.join(".git/config"), "ignore").unwrap();
        fs::write(root.join("target/debug/app.exe"), "ignore").unwrap();
        fs::write(root.join("保留.txt"), "keep").unwrap();

        let files = collect_workspace_scan_files(&root, "batch-excludes").unwrap();

        assert_eq!(files.len(), 1);
        assert_eq!(files[0].file_name, "保留.txt");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn workspace_scanner_marks_installers_as_not_suitable_for_ai_context() {
        let file = WorkspaceScanFile {
            id: "scan-file".to_string(),
            path: "D:\\Example\\setup.exe".to_string(),
            file_name: "setup.exe".to_string(),
            extension: "exe".to_string(),
            status: "scanned".to_string(),
            ..WorkspaceScanFile::default()
        };

        let analyzed = analyze_workspace_scan_file(&file, &[], &[]);

        assert_eq!(analyzed.status, "notSuitableForAIContext");
        assert!(analyzed.not_suitable_for_ai_context);
        assert!(analyzed.needs_confirmation);
        assert_eq!(analyzed.confidence_level, "low");
    }

    #[test]
    fn workspace_scanner_builds_failed_batch_without_crashing() {
        let batch = build_failed_workspace_scan_batch(
            "D:\\missing".to_string(),
            "扫描目录不存在".to_string(),
        );

        assert_eq!(batch.status, "failed");
        assert_eq!(batch.file_count, 0);
        assert!(batch.failure_reason.contains("扫描目录不存在"));
    }

    #[test]
    fn workspace_scanner_keeps_latest_batches_and_replaces_duplicate_id() {
        let mut document = WorkspaceScanDocument::default();
        for index in 0..35 {
            append_workspace_scan_batch(
                &mut document,
                WorkspaceScanBatch {
                    id: format!("batch-{index}"),
                    source_directory: format!("D:\\scan\\{index}"),
                    created_at: format!("2026-08-20T00:{index:02}:00Z"),
                    status: "completed".to_string(),
                    ..WorkspaceScanBatch::default()
                },
            );
        }
        append_workspace_scan_batch(
            &mut document,
            WorkspaceScanBatch {
                id: "batch-34".to_string(),
                source_directory: "D:\\scan\\updated".to_string(),
                created_at: "2026-08-20T01:00:00Z".to_string(),
                status: "completed".to_string(),
                ..WorkspaceScanBatch::default()
            },
        );

        assert_eq!(document.batches.len(), 30);
        assert_eq!(document.batches[0].id, "batch-34");
        assert_eq!(document.batches[0].source_directory, "D:\\scan\\updated");
        assert_eq!(
            document
                .batches
                .iter()
                .filter(|batch| batch.id == "batch-34")
                .count(),
            1
        );
    }

    #[test]
    fn cleanup_plan_generates_existing_project_item_with_trace() {
        let batch = cleanup_test_batch(vec![cleanup_scan_file(
            "WTS测试报告.docx",
            "existingProjectMaterial",
            "report",
            "WTS",
            "D:\\GanMaoYuan_Workspace\\10_Projects\\WTS\\测试验收\\WTS测试报告.docx",
            92,
            false,
        )]);
        let workspace = cleanup_test_workspace();

        let plan = build_cleanup_plan_from_scan_batch(&batch, &workspace, None);

        assert_eq!(plan.status, "draft");
        assert_eq!(plan.items.len(), 1);
        let item = &plan.items[0];
        assert_eq!(item.recommended_ownership, "existingProject");
        assert_eq!(item.recommended_project, "WTS");
        assert!(item.recommended_target_path.contains("WTS\\测试验收"));
        assert_eq!(item.required_action, "readyForReview");
        assert!(!item.decision_trace_id.is_empty());
    }

    #[test]
    fn cleanup_plan_routes_general_template_to_general_root() {
        let batch = cleanup_test_batch(vec![WorkspaceScanFile {
            recommended_category: "模板".to_string(),
            document_type: "用户账号导入模板".to_string(),
            ..cleanup_scan_file(
                "account_import_template.xlsx",
                "generalWorkMaterial",
                "template",
                "",
                "模板",
                86,
                false,
            )
        }]);
        let workspace = cleanup_test_workspace();

        let plan = build_cleanup_plan_from_scan_batch(&batch, &workspace, None);

        let item = &plan.items[0];
        assert_eq!(item.recommended_ownership, "generalWorkMaterial");
        assert!(item.recommended_target_path.contains("20_General\\模板"));
        assert_eq!(item.required_action, "readyForReview");
    }

    #[test]
    fn cleanup_plan_keeps_master_data_for_confirmation_when_needed() {
        let batch = cleanup_test_batch(vec![WorkspaceScanFile {
            recommended_category: "主数据".to_string(),
            document_type: "物料主数据".to_string(),
            ..cleanup_scan_file(
                "物料编码.xlsx",
                "generalWorkMaterial",
                "data",
                "",
                "主数据",
                68,
                false,
            )
        }]);
        let workspace = cleanup_test_workspace();

        let plan = build_cleanup_plan_from_scan_batch(&batch, &workspace, None);

        let item = &plan.items[0];
        assert_eq!(item.recommended_ownership, "needsReview");
        assert_eq!(item.required_action, "manualReview");
    }

    #[test]
    fn cleanup_plan_marks_unknown_low_confidence_as_needs_review() {
        let batch = cleanup_test_batch(vec![cleanup_scan_file(
            "临时说明.txt",
            "",
            "unknown",
            "",
            "",
            32,
            true,
        )]);
        let workspace = cleanup_test_workspace();

        let plan = build_cleanup_plan_from_scan_batch(&batch, &workspace, None);

        let item = &plan.items[0];
        assert_eq!(item.recommended_ownership, "needsReview");
        assert_eq!(item.recommended_target_path, "待确认");
        assert_eq!(item.required_action, "manualReview");
    }

    #[test]
    fn cleanup_plan_marks_failed_and_installers_as_unsupported() {
        let batch = cleanup_test_batch(vec![
            WorkspaceScanFile {
                status: "failed".to_string(),
                failure_reason: "解析失败".to_string(),
                ..cleanup_scan_file("坏文件.pdf", "", "unknown", "", "", 0, true)
            },
            WorkspaceScanFile {
                status: "notSuitableForAIContext".to_string(),
                not_suitable_for_ai_context: true,
                ..cleanup_scan_file(
                    "setup.exe",
                    "unsupportedOrFailed",
                    "reference",
                    "",
                    "",
                    0,
                    true,
                )
            },
        ]);
        let workspace = cleanup_test_workspace();

        let plan = build_cleanup_plan_from_scan_batch(&batch, &workspace, None);

        assert!(plan
            .items
            .iter()
            .all(|item| item.recommended_ownership == "unsupportedOrFailed"));
        assert!(plan
            .items
            .iter()
            .all(|item| item.required_action == "skipUnsupported"));
    }

    #[test]
    fn cleanup_plan_marks_duplicate_without_copy_action() {
        let mut file = cleanup_scan_file(
            "WTS测试报告.docx",
            "existingProjectMaterial",
            "report",
            "WTS",
            "D:\\GanMaoYuan_Workspace\\10_Projects\\WTS\\测试验收\\WTS测试报告.docx",
            100,
            false,
        );
        file.decision_traces.push(new_decision_trace(
            "locationRecommendation",
            &file.id,
            "",
            cleanup_plan_evidence(&file),
            "检测到完全相同的重复文件。".to_string(),
            "重复文件，不建议创建新的 managed copy。".to_string(),
            trace_confidence(100),
        ));
        let batch = cleanup_test_batch(vec![file]);
        let workspace = cleanup_test_workspace();

        let plan = build_cleanup_plan_from_scan_batch(&batch, &workspace, None);

        assert_eq!(plan.items[0].required_action, "duplicateNoCopy");
        assert_eq!(plan.items[0].recommended_ownership, "existingProject");
    }

    #[test]
    fn cleanup_plan_document_persists_latest_plan_per_scan_batch() {
        let mut document = CleanupPlanDocument::default();
        append_cleanup_plan(
            &mut document,
            CleanupPlan {
                id: "plan-old".to_string(),
                scan_batch_id: "batch-1".to_string(),
                created_at: "2026-08-20T00:00:00Z".to_string(),
                status: "draft".to_string(),
                ..CleanupPlan::default()
            },
        );
        append_cleanup_plan(
            &mut document,
            CleanupPlan {
                id: "plan-new".to_string(),
                scan_batch_id: "batch-1".to_string(),
                created_at: "2026-08-20T01:00:00Z".to_string(),
                status: "draft".to_string(),
                ..CleanupPlan::default()
            },
        );

        assert_eq!(document.plans.len(), 1);
        assert_eq!(document.plans[0].id, "plan-new");
        assert_eq!(document.plans[0].scan_batch_id, "batch-1");
        assert!(!document.updated_at.is_empty());
    }

    #[test]
    fn cleanup_review_approves_all_high_confidence_items() {
        let workspace = cleanup_test_workspace();
        let mut plan = build_cleanup_plan_from_scan_batch(
            &cleanup_test_batch(vec![
                cleanup_scan_file(
                    "WTS测试报告.docx",
                    "existingProjectMaterial",
                    "report",
                    "WTS",
                    "D:\\GanMaoYuan_Workspace\\10_Projects\\WTS\\测试验收\\WTS测试报告.docx",
                    92,
                    false,
                ),
                cleanup_scan_file(
                    "模板.xlsx",
                    "generalWorkMaterial",
                    "template",
                    "",
                    "模板",
                    86,
                    false,
                ),
            ]),
            &workspace,
            None,
        );

        for item in &mut plan.items {
            apply_cleanup_review(item, "approved", Some("批量确认高置信"), &workspace).unwrap();
        }

        assert!(plan
            .items
            .iter()
            .all(|item| item.review_status == "approved"));
        assert!(plan
            .items
            .iter()
            .all(|item| item.decision_trace.user_decision == "approved"));
        assert!(plan
            .items
            .iter()
            .all(|item| item.decision_trace.execution == "pending"));
    }

    #[test]
    fn cleanup_review_partially_approves_selected_items() {
        let workspace = cleanup_test_workspace();
        let mut plan = build_cleanup_plan_from_scan_batch(
            &cleanup_test_batch(vec![
                cleanup_scan_file(
                    "A.docx",
                    "existingProjectMaterial",
                    "report",
                    "WTS",
                    "D:\\GanMaoYuan_Workspace\\10_Projects\\WTS\\A.docx",
                    90,
                    false,
                ),
                cleanup_scan_file(
                    "B.docx",
                    "existingProjectMaterial",
                    "report",
                    "WTS",
                    "D:\\GanMaoYuan_Workspace\\10_Projects\\WTS\\B.docx",
                    90,
                    false,
                ),
            ]),
            &workspace,
            None,
        );

        apply_cleanup_review(&mut plan.items[0], "approved", None, &workspace).unwrap();

        assert_eq!(plan.items[0].review_status, "approved");
        assert_eq!(plan.items[1].review_status, "pending");
    }

    #[test]
    fn cleanup_review_rejects_item_and_cancels_execution() {
        let workspace = cleanup_test_workspace();
        let mut item = build_cleanup_plan_from_scan_batch(
            &cleanup_test_batch(vec![cleanup_scan_file(
                "未知.txt",
                "",
                "unknown",
                "",
                "",
                40,
                true,
            )]),
            &workspace,
            None,
        )
        .items
        .remove(0);

        apply_cleanup_review(&mut item, "rejected", Some("不需要整理"), &workspace).unwrap();

        assert_eq!(item.review_status, "rejected");
        assert_eq!(item.decision_trace.user_decision, "rejected");
        assert_eq!(item.decision_trace.execution, "cancelled");
    }

    #[test]
    fn cleanup_review_modifies_target_and_records_trace() {
        let workspace = cleanup_test_workspace();
        let mut item = build_cleanup_plan_from_scan_batch(
            &cleanup_test_batch(vec![cleanup_scan_file(
                "资料.xlsx",
                "generalWorkMaterial",
                "data",
                "",
                "主数据",
                88,
                false,
            )]),
            &workspace,
            None,
        )
        .items
        .remove(0);

        apply_cleanup_modification(
            &mut item,
            Some("感冒院".to_string()),
            Some("测试资料".to_string()),
            "20_General\\测试资料\\资料.xlsx".to_string(),
            Some("放到测试资料更准确"),
            &workspace,
        )
        .unwrap();

        assert_eq!(item.review_status, "modified");
        assert!(item.final_target_path.contains("20_General\\测试资料"));
        assert_eq!(item.decision_trace.user_decision, "modified");
        assert!(item.decision_traces.len() >= 2);
    }

    #[test]
    fn cleanup_review_filters_by_target_and_category() {
        let workspace = cleanup_test_workspace();
        let plan = build_cleanup_plan_from_scan_batch(
            &cleanup_test_batch(vec![
                cleanup_scan_file(
                    "A.docx",
                    "existingProjectMaterial",
                    "report",
                    "WTS",
                    "D:\\GanMaoYuan_Workspace\\10_Projects\\WTS\\A.docx",
                    90,
                    false,
                ),
                cleanup_scan_file(
                    "B.xlsx",
                    "generalWorkMaterial",
                    "template",
                    "",
                    "模板",
                    90,
                    false,
                ),
            ]),
            &workspace,
            None,
        );
        let project_filter = CleanupPlanReviewFilter {
            recommended_target_path_prefix: "D:\\GanMaoYuan_Workspace\\10_Projects".to_string(),
            ..CleanupPlanReviewFilter::default()
        };
        let general_filter = CleanupPlanReviewFilter {
            recommended_ownership: "generalWorkMaterial".to_string(),
            ..CleanupPlanReviewFilter::default()
        };

        assert!(cleanup_plan_item_matches_filter(
            &plan.items[0],
            &project_filter
        ));
        assert!(!cleanup_plan_item_matches_filter(
            &plan.items[1],
            &project_filter
        ));
        assert!(cleanup_plan_item_matches_filter(
            &plan.items[1],
            &general_filter
        ));
    }

    #[test]
    fn cleanup_review_allows_explicit_medium_confidence_review() {
        let workspace = cleanup_test_workspace();
        let mut item = build_cleanup_plan_from_scan_batch(
            &cleanup_test_batch(vec![cleanup_scan_file(
                "待确认.xlsx",
                "generalWorkMaterial",
                "data",
                "",
                "D:\\GanMaoYuan_Workspace\\20_General\\主数据\\待确认.xlsx",
                62,
                true,
            )]),
            &workspace,
            None,
        )
        .items
        .remove(0);
        apply_cleanup_modification(
            &mut item,
            None,
            Some("主数据".to_string()),
            "D:\\GanMaoYuan_Workspace\\20_General\\主数据\\待确认.xlsx".to_string(),
            Some("人工选择主数据目录"),
            &workspace,
        )
        .unwrap();

        apply_cleanup_review(&mut item, "approved", Some("人工确认中置信"), &workspace).unwrap();

        assert_eq!(item.review_status, "approved");
    }

    #[test]
    fn cleanup_review_reset_keeps_trace_history() {
        let workspace = cleanup_test_workspace();
        let mut item = build_cleanup_plan_from_scan_batch(
            &cleanup_test_batch(vec![cleanup_scan_file(
                "A.docx",
                "existingProjectMaterial",
                "report",
                "WTS",
                "D:\\GanMaoYuan_Workspace\\10_Projects\\WTS\\A.docx",
                90,
                false,
            )]),
            &workspace,
            None,
        )
        .items
        .remove(0);
        apply_cleanup_review(&mut item, "approved", None, &workspace).unwrap();
        let before = item.decision_traces.len();

        item.review_status = "pending".to_string();
        item.reviewed_at.clear();
        let trace = cleanup_review_trace(
            &item,
            "pending",
            "pending",
            "用户已将审核状态退回待确认。",
            None,
        );
        push_cleanup_item_trace(&mut item, trace);

        assert_eq!(item.review_status, "pending");
        assert!(item.decision_traces.len() > before);
    }

    #[test]
    fn cleanup_review_rejects_target_outside_workspace() {
        let workspace = cleanup_test_workspace();

        assert!(validate_cleanup_target_path("C:\\Users\\Desktop\\bad.txt", &workspace).is_err());
        assert!(validate_cleanup_target_path("..\\bad.txt", &workspace).is_err());
        assert!(validate_cleanup_target_path("\\\\server\\share\\bad.txt", &workspace).is_err());
    }

    #[test]
    fn cleanup_execution_copies_single_file_and_keeps_source() {
        let root = test_root("cleanup-execute-single");
        let source_dir = root.join("desktop");
        fs::create_dir_all(&source_dir).unwrap();
        let source = source_dir.join("报告.txt");
        fs::write(&source, "真实测试报告").unwrap();
        let workspace = cleanup_test_workspace_at(&root.join("workspace"));
        fs::create_dir_all(&workspace.system_root).unwrap();
        let audit = root.join("audit.jsonl");
        let mut item = cleanup_plan_item_for_source(&source, &workspace, 92);
        apply_cleanup_review(&mut item, "approved", None, &workspace).unwrap();
        let mut global = GlobalFilesDocument::default();

        let executed = execute_cleanup_plan_item(&workspace, &audit, &mut global, &item);

        assert_eq!(executed.status, "completed");
        assert!(source.exists());
        assert!(Path::new(&executed.target_path).exists());
        assert_eq!(executed.hash_before, executed.hash_after);
        assert_eq!(hash_file(&source).unwrap(), executed.hash_after);
        assert_eq!(global.files.len(), 1);
        assert_eq!(global.files[0].managed_path, executed.target_path);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn cleanup_execution_isolates_failed_files() {
        let root = test_root("cleanup-execute-partial");
        let source_dir = root.join("desktop");
        fs::create_dir_all(&source_dir).unwrap();
        let ok_source = source_dir.join("可复制.txt");
        fs::write(&ok_source, "可以复制").unwrap();
        let missing_source = source_dir.join("不存在.txt");
        let workspace = cleanup_test_workspace_at(&root.join("workspace"));
        fs::create_dir_all(&workspace.system_root).unwrap();
        let audit = root.join("audit.jsonl");
        let mut ok_item = cleanup_plan_item_for_source(&ok_source, &workspace, 91);
        let mut missing_item = cleanup_plan_item_for_source(&missing_source, &workspace, 91);
        apply_cleanup_review(&mut ok_item, "approved", None, &workspace).unwrap();
        missing_item.review_status = "approved".to_string();
        let mut global = GlobalFilesDocument::default();

        let ok = execute_cleanup_plan_item(&workspace, &audit, &mut global, &ok_item);
        let failed = execute_cleanup_plan_item(&workspace, &audit, &mut global, &missing_item);

        assert_eq!(ok.status, "completed");
        assert_eq!(failed.status, "failed");
        assert!(failed.error.contains("原文件不存在"));
        assert_eq!(global.files.len(), 1);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn cleanup_execution_undo_removes_copy_but_not_source() {
        let root = test_root("cleanup-execute-undo");
        let source_dir = root.join("desktop");
        fs::create_dir_all(&source_dir).unwrap();
        let source = source_dir.join("资料.txt");
        fs::write(&source, "可撤销资料").unwrap();
        let workspace = cleanup_test_workspace_at(&root.join("workspace"));
        fs::create_dir_all(&workspace.system_root).unwrap();
        let audit = root.join("audit.jsonl");
        let mut item = cleanup_plan_item_for_source(&source, &workspace, 90);
        apply_cleanup_review(&mut item, "approved", None, &workspace).unwrap();
        let mut global = GlobalFilesDocument::default();
        let executed = execute_cleanup_plan_item(&workspace, &audit, &mut global, &item);

        let undone =
            perform_global_undo_by_operation(&audit, &mut global, &executed.operation_id).unwrap();

        assert!(!Path::new(&executed.target_path).exists());
        assert!(source.exists());
        assert!(!undone.undone_at.is_empty());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn cleanup_execution_undo_detects_modified_copy_conflict() {
        let root = test_root("cleanup-execute-undo-conflict");
        let source_dir = root.join("desktop");
        fs::create_dir_all(&source_dir).unwrap();
        let source = source_dir.join("资料.txt");
        fs::write(&source, "原内容").unwrap();
        let workspace = cleanup_test_workspace_at(&root.join("workspace"));
        fs::create_dir_all(&workspace.system_root).unwrap();
        let audit = root.join("audit.jsonl");
        let mut item = cleanup_plan_item_for_source(&source, &workspace, 90);
        apply_cleanup_review(&mut item, "approved", None, &workspace).unwrap();
        let mut global = GlobalFilesDocument::default();
        let executed = execute_cleanup_plan_item(&workspace, &audit, &mut global, &item);
        fs::write(&executed.target_path, "用户修改后的内容").unwrap();

        let result = perform_global_undo_by_operation(&audit, &mut global, &executed.operation_id);

        assert!(result.is_err());
        assert!(Path::new(&executed.target_path).exists());
        assert!(source.exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn cleanup_execution_record_is_search_compatible_global_file() {
        let root = test_root("cleanup-execute-search-record");
        let source_dir = root.join("desktop");
        fs::create_dir_all(&source_dir).unwrap();
        let source = source_dir.join("WTS测试报告.txt");
        fs::write(&source, "WTS 回归测试通过").unwrap();
        let workspace = cleanup_test_workspace_at(&root.join("workspace"));
        fs::create_dir_all(&workspace.system_root).unwrap();
        let audit = root.join("audit.jsonl");
        let mut item = cleanup_plan_item_for_source(&source, &workspace, 93);
        item.evidence.push(DecisionTraceEvidence {
            kind: "summary".to_string(),
            label: "摘要".to_string(),
            summary: "WTS 回归测试报告".to_string(),
            source_id: item.id.clone(),
        });
        apply_cleanup_review(&mut item, "approved", None, &workspace).unwrap();
        let mut global = GlobalFilesDocument::default();

        let executed = execute_cleanup_plan_item(&workspace, &audit, &mut global, &item);

        assert_eq!(executed.status, "completed");
        let haystack = join_search_fields(&[
            &global.files[0].file_name,
            &global.files[0].content_summary,
            &global.files[0].business_summary,
        ]);
        assert!(match_search_score(&haystack, &normalize_search_text("WTS测试")).is_some());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn workspace_search_haystack_includes_workspace_metadata() {
        let result = GlobalSearchResult {
            project_name: "个人文件空间".to_string(),
            content_type: "global_file".to_string(),
            title: "VAVE申请单.xlsx".to_string(),
            snippet: "降本申请资料".to_string(),
            workspace_relative_path: "20_General/engineering/vave/VAVE申请单.xlsx".to_string(),
            ownership_type: "generalWorkMaterial".to_string(),
            category: "改善申请".to_string(),
            document_type: "VAVE降本申请单".to_string(),
            document_purpose: "template".to_string(),
            recent_status: "2026-08-20 已整理".to_string(),
            ..GlobalSearchResult::default()
        };

        let haystack = build_search_haystack(&result);

        assert!(match_search_score(&haystack, &normalize_search_text("改善申请")).is_some());
        assert!(match_search_score(&haystack, &normalize_search_text("已整理")).is_some());
    }

    #[test]
    fn search_index_result_returns_chinese_match_snippet() {
        let result = GlobalSearchResult {
            id: "file-1".to_string(),
            project_name: "感冒院".to_string(),
            content_type: "global_file".to_string(),
            title: "车间生产订单报表.xlsx".to_string(),
            snippet: "包含车间订单、生产数量和交付日期，用于制造生产分析。".to_string(),
            workspace_relative_path:
                "20_General/manufacturing/productionData/车间生产订单报表.xlsx".to_string(),
            document_type: "车间生产订单报表".to_string(),
            document_purpose: "data".to_string(),
            ..GlobalSearchResult::default()
        };
        let entry = search_result_to_index_entry(result, "2026-08-25T10:00:00").unwrap();
        let matched = search_index_entry_to_result(&entry, &normalize_search_text("生产订单"))
            .expect("indexed file should match Chinese keyword");

        assert!(["文件名", "类型", "位置"].contains(&matched.matched_field.as_str()));
        assert!(matched.match_snippet.contains("生产订单"));
    }

    #[test]
    fn file_search_result_round_trips_projection_metadata_through_index() {
        let file = GlobalManagedFile {
            id: "global-1".to_string(),
            file_name: "WTS测试报告-v2.txt".to_string(),
            source_path: r"D:\桌面\WTS测试报告-v2.txt".to_string(),
            managed_path: r"D:\GanMaoYuan_Workspace\10_Projects\WTS\测试验收\WTS测试报告-v2.txt"
                .to_string(),
            managed_relative_path: r"10_Projects\WTS\测试验收\WTS测试报告-v2.txt".to_string(),
            content_hash: "hash-v2".to_string(),
            file_type: "txt".to_string(),
            document_type: "测试报告".to_string(),
            document_purpose: "report".to_string(),
            business_domain: "项目验收".to_string(),
            category: "测试验收".to_string(),
            ownership_type: "existingProjectMaterial".to_string(),
            related_project: "WTS".to_string(),
            lifecycle_status: "versioned".to_string(),
            content_summary: "WTS 回归测试报告第二版".to_string(),
            decision_trace_id: "trace-wts".to_string(),
            ..GlobalManagedFile::default()
        };
        let projection = FileProjectionAdapter::from_global_file(&file);
        let mut result = search_result_from_file_projection(
            &projection,
            String::new(),
            String::new(),
            "WTS".to_string(),
            "global_file",
            &file.decision_trace_id,
            "高 92%".to_string(),
        );
        result.duplicate_of = "global-0".to_string();
        result.version_group_id = "family-wts".to_string();
        result.version_number = 2;

        let entry = search_result_to_index_entry(result, "2026-08-26T00:00:00Z").unwrap();
        let matched = search_index_entry_to_result(&entry, &normalize_search_text("测试报告"))
            .expect("projected file index should be searchable");

        assert_eq!(matched.file_id, "global-1");
        assert_eq!(matched.hash, "hash-v2");
        assert_eq!(matched.document_type, "测试报告");
        assert_eq!(matched.document_purpose, "report");
        assert_eq!(matched.business_domain, "项目验收");
        assert_eq!(matched.lifecycle_status, "versioned");
        assert_eq!(matched.ownership_type, "existingProjectMaterial");
        assert_eq!(matched.duplicate_of, "global-0");
        assert_eq!(matched.version_group_id, "family-wts");
        assert_eq!(matched.version_number, 2);
        assert_eq!(matched.evidence_refs[0].hash_snapshot, "hash-v2");
    }

    #[test]
    fn search_index_excludes_sensitive_entries() {
        let result = GlobalSearchResult {
            id: "secret-1".to_string(),
            project_name: "感冒院".to_string(),
            content_type: "file".to_string(),
            title: ".env".to_string(),
            snippet: "DEEPSEEK_API_KEY=[redacted-test-value]".to_string(),
            managed_path: "D:/GanMaoYuan/AppData/.env".to_string(),
            ..GlobalSearchResult::default()
        };

        assert!(search_result_to_index_entry(result, "2026-08-25T10:00:00").is_none());
    }

    #[test]
    fn search_index_supports_subsequence_fuzzy_match() {
        let result = GlobalSearchResult {
            id: "codex-1".to_string(),
            project_name: "感冒院".to_string(),
            content_type: "codexReport".to_string(),
            title: "Codex Result Bridge 完成报告".to_string(),
            snippet: "记录 Work Ledger 回流、Git 核验和人工验收 Gate。".to_string(),
            ..GlobalSearchResult::default()
        };
        let entry = search_result_to_index_entry(result, "2026-08-25T10:00:00").unwrap();

        assert!(
            search_index_entry_to_result(&entry, &normalize_search_text("cdx bridge")).is_some()
        );
    }

    #[test]
    fn workspace_activity_summary_counts_real_records_and_deduplicates_hashes() {
        let global = GlobalFilesDocument {
            files: vec![
                GlobalManagedFile {
                    id: "g1".to_string(),
                    file_name: "WTS测试报告-v2.txt".to_string(),
                    managed_path: "D:/GanMaoYuan_Workspace/10_Projects/WTS/WTS测试报告-v2.txt"
                        .to_string(),
                    managed_relative_path: "10_Projects/WTS/WTS测试报告-v2.txt".to_string(),
                    ownership_type: "existingProjectMaterial".to_string(),
                    related_project: "WTS".to_string(),
                    category: "测试验收".to_string(),
                    content_hash: "same-hash".to_string(),
                    lifecycle_status: "managed".to_string(),
                    created_at: "2026-08-20T08:00:00Z".to_string(),
                    decision_trace_id: "trace-1".to_string(),
                    ..GlobalManagedFile::default()
                },
                GlobalManagedFile {
                    id: "g2".to_string(),
                    file_name: "WTS测试报告-v2-copy.txt".to_string(),
                    managed_path: "D:/GanMaoYuan_Workspace/10_Projects/WTS/WTS测试报告-v2-copy.txt"
                        .to_string(),
                    managed_relative_path: "10_Projects/WTS/WTS测试报告-v2-copy.txt".to_string(),
                    ownership_type: "existingProjectMaterial".to_string(),
                    related_project: "WTS".to_string(),
                    content_hash: "same-hash".to_string(),
                    lifecycle_status: "managed".to_string(),
                    created_at: "2026-08-20T08:01:00Z".to_string(),
                    ..GlobalManagedFile::default()
                },
                GlobalManagedFile {
                    id: "g3".to_string(),
                    file_name: "模板.txt".to_string(),
                    managed_path: "D:/GanMaoYuan_Workspace/20_General/template/模板.txt"
                        .to_string(),
                    managed_relative_path: "20_General/template/模板.txt".to_string(),
                    ownership_type: "generalWorkMaterial".to_string(),
                    content_hash: "template-hash".to_string(),
                    lifecycle_status: "managed".to_string(),
                    created_at: "2026-08-20T09:00:00Z".to_string(),
                    ..GlobalManagedFile::default()
                },
            ],
            ..GlobalFilesDocument::default()
        };
        let plans = CleanupPlanDocument {
            plans: vec![CleanupPlan {
                status: "reviewing".to_string(),
                items: vec![CleanupPlanItem {
                    review_status: "pending".to_string(),
                    ..CleanupPlanItem::default()
                }],
                ..CleanupPlan::default()
            }],
            ..CleanupPlanDocument::default()
        };
        let executions = CleanupExecutionDocument {
            batches: vec![CleanupExecutionBatch {
                completed_at: "2026-08-20T10:00:00Z".to_string(),
                items: vec![
                    CleanupExecutionItem {
                        source_path: "D:/Desktop/失败.txt".to_string(),
                        status: "failed".to_string(),
                        error: "权限不足".to_string(),
                        ..CleanupExecutionItem::default()
                    },
                    CleanupExecutionItem {
                        target_path: "D:/GanMaoYuan_Workspace/20_General/冲突.txt".to_string(),
                        status: "undoConflict".to_string(),
                        error: "受管副本已被修改".to_string(),
                        ..CleanupExecutionItem::default()
                    },
                ],
                ..CleanupExecutionBatch::default()
            }],
            ..CleanupExecutionDocument::default()
        };

        let summary = workspace_activity_summary_from_documents(&global, &plans, &executions);

        assert_eq!(summary.new_managed_files, 2, "相同 hash 不应重复统计");
        assert_eq!(summary.project_files, 1);
        assert_eq!(summary.general_files, 1);
        assert_eq!(summary.pending_cleanup_plans, 1);
        assert_eq!(summary.failed_items, 1);
        assert_eq!(summary.conflicts, 1);
        assert!(summary.versioned_files >= 1);
        assert!(summary
            .recent_items
            .iter()
            .any(|item| item.decision_trace_id == "trace-1"));
    }

    #[test]
    fn cleanup_execution_undo_marks_workspace_lifecycle_superseded() {
        let root = test_root("cleanup-execute-lifecycle");
        let source_dir = root.join("desktop");
        fs::create_dir_all(&source_dir).unwrap();
        let source = source_dir.join("资料.txt");
        fs::write(&source, "可撤销资料").unwrap();
        let workspace = cleanup_test_workspace_at(&root.join("workspace"));
        fs::create_dir_all(&workspace.system_root).unwrap();
        let audit = root.join("audit.jsonl");
        let mut item = cleanup_plan_item_for_source(&source, &workspace, 90);
        apply_cleanup_review(&mut item, "approved", None, &workspace).unwrap();
        let mut global = GlobalFilesDocument::default();
        let executed = execute_cleanup_plan_item(&workspace, &audit, &mut global, &item);

        let undone =
            perform_global_undo_by_operation(&audit, &mut global, &executed.operation_id).unwrap();

        assert_eq!(undone.lifecycle_status, "superseded");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn workspace_global_file_can_feed_project_impact_without_new_system() {
        let global_file = GlobalManagedFile {
            id: "global-wts".to_string(),
            file_name: "WTS测试报告.txt".to_string(),
            source_path: "D:/Desktop/WTS测试报告.txt".to_string(),
            managed_path: "D:/GanMaoYuan_Workspace/10_Projects/WTS/WTS测试报告.txt".to_string(),
            managed_relative_path: "10_Projects/WTS/WTS测试报告.txt".to_string(),
            related_project: "WTS".to_string(),
            content_hash: "hash-wts".to_string(),
            business_summary: "测试报告显示回归通过，但存在多人反馈风险。".to_string(),
            document_purpose_evidence: vec!["回归测试".to_string(), "风险".to_string()],
            created_at: "2026-08-20T08:00:00Z".to_string(),
            ..GlobalManagedFile::default()
        };
        let file = workspace_global_file_as_project_file(&global_file);
        let manifest = ProjectManifest {
            project: ProjectSummary {
                id: "wts".to_string(),
                name: "WTS".to_string(),
                ..ProjectSummary::default()
            },
            ..ProjectManifest::default()
        };

        let impact = build_local_project_impact(&manifest, &file);

        assert_eq!(impact.source_file_id, "workspace-global-wts");
        assert_eq!(impact.source_hash, "hash-wts");
        assert!(!impact.findings.is_empty());
    }

    fn cleanup_test_workspace() -> WorkspaceConfig {
        WorkspaceConfig {
            workspace_root: "D:\\GanMaoYuan_Workspace".to_string(),
            inbox_root: "D:\\GanMaoYuan_Workspace\\00_Inbox".to_string(),
            projects_root: "D:\\GanMaoYuan_Workspace\\10_Projects".to_string(),
            general_root: "D:\\GanMaoYuan_Workspace\\20_General".to_string(),
            temporary_root: "D:\\GanMaoYuan_Workspace\\30_Temporary".to_string(),
            archive_root: "D:\\GanMaoYuan_Workspace\\40_Archive".to_string(),
            system_root: "D:\\GanMaoYuan_Workspace\\.ganmaoyuan".to_string(),
            status: "ready".to_string(),
            ..WorkspaceConfig::default()
        }
    }

    fn cleanup_test_workspace_at(root: &Path) -> WorkspaceConfig {
        WorkspaceConfig {
            workspace_root: path_to_string(root),
            inbox_root: path_to_string(&root.join("00_Inbox")),
            projects_root: path_to_string(&root.join("10_Projects")),
            general_root: path_to_string(&root.join("20_General")),
            temporary_root: path_to_string(&root.join("30_Temporary")),
            archive_root: path_to_string(&root.join("40_Archive")),
            system_root: path_to_string(&root.join(".ganmaoyuan")),
            status: "ready".to_string(),
            ..WorkspaceConfig::default()
        }
    }

    fn cleanup_plan_item_for_source(
        source: &Path,
        workspace: &WorkspaceConfig,
        confidence: u8,
    ) -> CleanupPlanItem {
        let name = source
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("资料.txt");
        let mut item = build_cleanup_plan_from_scan_batch(
            &cleanup_test_batch(vec![WorkspaceScanFile {
                path: path_to_string(source),
                hash: hash_file(source).unwrap_or_default(),
                ..cleanup_scan_file(
                    name,
                    "generalWorkMaterial",
                    "report",
                    "",
                    "通用报告",
                    confidence,
                    false,
                )
            }]),
            workspace,
            None,
        )
        .items
        .remove(0);
        item.recommended_target_path = path_to_string(
            &PathBuf::from(&workspace.general_root)
                .join("报告")
                .join(name),
        );
        item.location_preview_id = LocationPreviewService::preview(
            &item.source_path,
            Path::new(&item.recommended_target_path),
            Some(Path::new(&workspace.workspace_root)),
            &item.recommended_project,
            &item.recommended_ownership,
            &item.document_purpose,
            "测试使用与正式流程相同的位置预览。",
        )
        .preview_id;
        item
    }

    fn cleanup_test_batch(files: Vec<WorkspaceScanFile>) -> WorkspaceScanBatch {
        WorkspaceScanBatch {
            id: "batch-cleanup".to_string(),
            source_directory: "D:\\Desktop".to_string(),
            created_at: "2026-08-20T00:00:00Z".to_string(),
            completed_at: "2026-08-20T00:01:00Z".to_string(),
            file_count: files.len() as u64,
            status: "completed".to_string(),
            files,
            ..WorkspaceScanBatch::default()
        }
    }

    fn cleanup_scan_file(
        name: &str,
        ownership_type: &str,
        purpose: &str,
        project_name: &str,
        location: &str,
        confidence: u8,
        needs_confirmation: bool,
    ) -> WorkspaceScanFile {
        WorkspaceScanFile {
            id: format!("scan-{name}"),
            path: format!("D:\\Desktop\\{name}"),
            file_name: name.to_string(),
            extension: Path::new(name)
                .extension()
                .and_then(|value| value.to_str())
                .unwrap_or("unknown")
                .to_string(),
            hash: format!("hash-{name}"),
            scan_batch_id: "batch-cleanup".to_string(),
            status: "preview".to_string(),
            parse_status: "success".to_string(),
            content_summary: format!("{name} 的扫描摘要。"),
            document_type: if purpose == "template" {
                "模板文件".to_string()
            } else {
                "项目资料".to_string()
            },
            document_purpose: purpose.to_string(),
            ownership_type: ownership_type.to_string(),
            recommended_project_name: project_name.to_string(),
            recommended_location: location.to_string(),
            recommended_category: location.to_string(),
            confidence_level: trace_confidence(confidence).level,
            confidence_score: confidence,
            needs_confirmation,
            ..WorkspaceScanFile::default()
        }
    }

    #[test]
    fn parses_deepseek_project_analysis_json() {
        let analysis = parse_project_analysis_response(
            r#"{
                "status": "success",
                "projectDefinition": "感冒院用于掌控项目资料和连续工作上下文。",
                "goals": ["整理项目资料"],
                "roles": ["项目负责人"],
                "materialUsage": ["资料用于形成项目理解"],
                "knownRequirements": ["不上传原始文件"],
                "gaps": ["需要人工确认业务边界"],
                "questions": ["一期先验收哪些项目？"],
                "constraints": ["Atlas 只读"],
                "evidence": ["file-analysis.json"],
                "nextSteps": ["核验 Atlas 可复用部分"]
            }"#,
            "deepseek-test",
        )
        .unwrap();

        assert_eq!(analysis.status, "success");
        assert_eq!(analysis.model_id, "deepseek-test");
        assert_eq!(analysis.questions, vec!["一期先验收哪些项目？"]);
        assert!(analysis.failure_reason.is_empty());
    }

    #[test]
    fn location_service_versions_generated_deliverables_without_overwrite() {
        let root = test_root("location-service");
        let project_dir = root.join("project");
        let deliverables = project_dir.join(".ganmaoyuan/managed/08_deliverables");
        fs::create_dir_all(&deliverables).unwrap();
        let existing_path = deliverables.join("验收报告.txt");
        fs::write(&existing_path, "old").unwrap();
        let existing = FileRecord {
            id: "file-1".to_string(),
            file_name: "验收报告.txt".to_string(),
            managed_path: path_to_string(&existing_path),
            file_type: "txt".to_string(),
            category: "交付文件".to_string(),
            current_version: "v1".to_string(),
            version_group_id: "group-1".to_string(),
            ..FileRecord::default()
        };

        let plan = decide_file_location(
            &project_dir,
            &[existing],
            "验收报告.txt",
            "txt",
            "Codex",
            "阶段验收",
            "交付报告",
        )
        .unwrap();

        assert_eq!(plan.category, "交付文件");
        assert_eq!(plan.version_number, 2);
        assert!(path_to_string(&plan.destination).ends_with("验收报告-v2.txt"));
        assert!(existing_path.exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn scan_workspace_detects_new_and_missing_files() {
        let root = test_root("workspace-scan");
        let project_dir = root.join("project");
        let source_dir = root.join("source");
        fs::create_dir_all(&project_dir).unwrap();
        fs::create_dir_all(&source_dir).unwrap();
        ensure_project_dirs(&project_dir).unwrap();

        let source = source_dir.join("source.txt");
        fs::write(&source, "seed").unwrap();
        let mut files = Vec::new();
        let batch = import_into_records(
            &project_dir,
            &mut files,
            vec![path_to_string(&source)],
            "user-import",
            "scan-test",
            None,
        )
        .unwrap();

        let manifest = ProjectManifest {
            schema_version: MANIFEST_SCHEMA_VERSION,
            project: ProjectSummary {
                id: "project-scan".to_string(),
                name: "scan".to_string(),
                root_dir: path_to_string(&project_dir),
                manifest_path: path_to_string(&manifest_path(&project_dir)),
                last_opened_at: now_string(),
                next_step: String::new(),
                created_at: now_string(),
                description: String::new(),
                repository_path: String::new(),
            },
            files,
            messages: Vec::new(),
            tasks: Vec::new(),
            decisions: Vec::new(),
            artifacts: Vec::new(),
            codex_prompts: Vec::new(),
            codex_reports: Vec::new(),
            daily_sessions: Vec::new(),
            location_decisions: batch.location_decisions,
            pending_reviews: Vec::new(),
            monitoring: MonitoringState::default(),
            atlas: waiting_atlas(),
            project_analysis: ProjectAnalysis::default(),
            audit: Vec::new(),
            draft: WorkspaceDraft::default(),
            recovery_points: Vec::new(),
            deepseek_authorization: DeepSeekAuthorization::default(),
            ..ProjectManifest::default()
        };
        persist_project(&project_dir, &manifest, None).unwrap();

        let managed_path = PathBuf::from(&manifest.files[0].managed_path);
        fs::remove_file(&managed_path).unwrap();
        fs::write(project_dir.join("new-material.txt"), "new").unwrap();

        let (scanned, changed, _) = scan_project_workspace(&path_to_string(&project_dir)).unwrap();

        assert!(changed);
        assert!(scanned
            .pending_reviews
            .iter()
            .any(|item| item.kind == "managed_missing"));
        assert!(scanned
            .pending_reviews
            .iter()
            .any(|item| item.kind == "new_file"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn project_monitor_skips_generated_directories_and_refreshes_changed_hashes() {
        let root = test_root("project-monitor-fast-path");
        fs::create_dir_all(root.join("node_modules/dependency")).unwrap();
        fs::create_dir_all(root.join("target/debug")).unwrap();
        fs::create_dir_all(root.join(".git/objects")).unwrap();
        fs::write(root.join("project-note.txt"), "first").unwrap();
        fs::write(root.join("node_modules/dependency/package.js"), "ignored").unwrap();
        fs::write(root.join("target/debug/build-output.bin"), "ignored").unwrap();
        fs::write(root.join(".git/objects/object"), "ignored").unwrap();

        let first = collect_observed_project_files(&root, None).unwrap();
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].file_name, "project-note.txt");
        let first_hash = first[0].hash.clone();

        fs::write(
            root.join("project-note.txt"),
            "changed content with another size",
        )
        .unwrap();
        let second = collect_observed_project_files(&root, None).unwrap();
        assert_eq!(second.len(), 1);
        assert_ne!(second[0].hash, first_hash);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn project_monitor_skips_an_associated_repository_tree() {
        let root = test_root("project-monitor-repository-skip");
        fs::create_dir_all(root.join(".git/objects")).unwrap();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join(".gitignore"), "target\n").unwrap();
        fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();

        let observed = collect_observed_project_files(&root, Some(&root)).unwrap();

        assert!(observed.is_empty());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn scan_workspace_does_not_block_when_only_import_source_is_gone() {
        let root = test_root("workspace-source-provenance");
        let project_dir = root.join("project");
        let source_dir = root.join("source");
        fs::create_dir_all(&project_dir).unwrap();
        fs::create_dir_all(&source_dir).unwrap();
        ensure_project_dirs(&project_dir).unwrap();

        let source = source_dir.join("临时导入资料.txt");
        fs::write(&source, "seed").unwrap();
        let mut files = Vec::new();
        let batch = import_into_records(
            &project_dir,
            &mut files,
            vec![path_to_string(&source)],
            "user-import",
            "source-provenance-test",
            None,
        )
        .unwrap();
        let file_id = files[0].id.clone();
        files[0].needs_user_confirmation = true;
        let source_path = path_to_string(&source);
        let manifest = ProjectManifest {
            schema_version: MANIFEST_SCHEMA_VERSION,
            project: ProjectSummary {
                id: "project-source-provenance".to_string(),
                name: "source provenance".to_string(),
                root_dir: path_to_string(&project_dir),
                manifest_path: path_to_string(&manifest_path(&project_dir)),
                last_opened_at: now_string(),
                next_step: String::new(),
                created_at: now_string(),
                description: String::new(),
                repository_path: String::new(),
            },
            files,
            location_decisions: batch.location_decisions,
            pending_reviews: vec![
                pending_item(
                    format!("source-missing|{file_id}|{}", normalize_key(&source_path)),
                    "source_missing",
                    "原始资料缺失：临时导入资料.txt".to_string(),
                    "原始导入目录已清理。".to_string(),
                    &file_id,
                    &source_path,
                    "",
                    "参考资料",
                ),
                pending_item(
                    format!("review-needed|{file_id}"),
                    "review_required",
                    "需要检查：临时导入资料.txt".to_string(),
                    "由原始来源缺失触发的旧提醒。".to_string(),
                    &file_id,
                    "",
                    "",
                    "参考资料",
                ),
            ],
            ..ProjectManifest::default()
        };
        persist_project(&project_dir, &manifest, None).unwrap();
        fs::remove_file(&source).unwrap();

        let (scanned, changed, _) = scan_project_workspace(&path_to_string(&project_dir)).unwrap();

        assert!(changed);
        assert!(scanned.files[0].still_exists);
        assert!(!scanned.files[0].needs_user_confirmation);
        assert!(!scanned.pending_reviews.iter().any(|item| {
            matches!(
                item.kind.as_str(),
                "source_missing" | "source_moved" | "review_required"
            )
        }));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn scan_workspace_keeps_managed_copy_missing_as_a_blocker() {
        let root = test_root("workspace-managed-copy-missing");
        let project_dir = root.join("project");
        let source_dir = root.join("source");
        fs::create_dir_all(&project_dir).unwrap();
        fs::create_dir_all(&source_dir).unwrap();
        ensure_project_dirs(&project_dir).unwrap();

        let source = source_dir.join("source.txt");
        fs::write(&source, "seed").unwrap();
        let mut files = Vec::new();
        let batch = import_into_records(
            &project_dir,
            &mut files,
            vec![path_to_string(&source)],
            "user-import",
            "managed-copy-missing-test",
            None,
        )
        .unwrap();
        let managed_path = PathBuf::from(&files[0].managed_path);
        let manifest = ProjectManifest {
            schema_version: MANIFEST_SCHEMA_VERSION,
            project: ProjectSummary {
                id: "project-managed-copy-missing".to_string(),
                name: "managed copy missing".to_string(),
                root_dir: path_to_string(&project_dir),
                manifest_path: path_to_string(&manifest_path(&project_dir)),
                last_opened_at: now_string(),
                next_step: String::new(),
                created_at: now_string(),
                description: String::new(),
                repository_path: String::new(),
            },
            files,
            location_decisions: batch.location_decisions,
            ..ProjectManifest::default()
        };
        persist_project(&project_dir, &manifest, None).unwrap();
        fs::remove_file(&managed_path).unwrap();
        fs::remove_file(&source).unwrap();

        let (scanned, _, _) = scan_project_workspace(&path_to_string(&project_dir)).unwrap();

        assert!(!scanned.files[0].still_exists);
        assert!(scanned.files[0].needs_user_confirmation);
        assert!(scanned
            .pending_reviews
            .iter()
            .any(|item| item.kind == "managed_missing"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn global_search_matches_project_file_message_and_codex_report() {
        let now = now_string();
        let manifest = ProjectManifest {
            schema_version: MANIFEST_SCHEMA_VERSION,
            project: ProjectSummary {
                id: "project-1".to_string(),
                name: "感冒院 Alpha".to_string(),
                root_dir: "D:/Projects/GMY-Alpha".to_string(),
                manifest_path: "D:/Projects/GMY-Alpha/.ganmaoyuan/project-location-manifest.json"
                    .to_string(),
                last_opened_at: now.clone(),
                next_step: "整理全局搜索".to_string(),
                created_at: now.clone(),
                description: "用于验证全局搜索可以命中项目理解和资料摘要。".to_string(),
                repository_path: String::new(),
            },
            files: vec![FileRecord {
                id: "file-1".to_string(),
                file_name: "需求说明.txt".to_string(),
                managed_path:
                    "D:/Projects/GMY-Alpha/.ganmaoyuan/managed/02_requirements/需求说明.txt"
                        .to_string(),
                managed_relative_path: "managed/02_requirements/需求说明.txt".to_string(),
                content_hash: "project-file-hash".to_string(),
                content_summary: "包含全局搜索验收范围与关键词".to_string(),
                recommended_category: "需求文档".to_string(),
                category: "需求文档".to_string(),
                file_type: "txt".to_string(),
                still_exists: true,
                modified_at: now.clone(),
                ..FileRecord::default()
            }],
            messages: vec![WorkspaceMessage {
                id: "message-1".to_string(),
                author: "user".to_string(),
                kind: "requirement".to_string(),
                text: "今天继续做全局搜索与 Codex 协同闭环回归".to_string(),
                created_at: now.clone(),
                ..WorkspaceMessage::default()
            }],
            tasks: vec![TaskRecord {
                id: "task-1".to_string(),
                title: "完成全局搜索".to_string(),
                status: "active".to_string(),
                source_message_id: "message-1".to_string(),
                created_at: now.clone(),
                updated_at: now.clone(),
            }],
            decisions: Vec::new(),
            artifacts: Vec::new(),
            codex_prompts: Vec::new(),
            codex_reports: vec![CodexReportRecord {
                id: "report-1".to_string(),
                source_label: "codex-report.md".to_string(),
                summary: "已完成全局搜索并验证重启恢复".to_string(),
                test_results: vec!["npm run build 通过".to_string()],
                unresolved_issues: vec!["待人工点击回归".to_string()],
                created_at: now.clone(),
                ..CodexReportRecord::default()
            }],
            daily_sessions: Vec::new(),
            location_decisions: Vec::new(),
            pending_reviews: Vec::new(),
            monitoring: MonitoringState::default(),
            atlas: waiting_atlas(),
            project_analysis: ProjectAnalysis {
                status: "success".to_string(),
                project_definition: "感冒院负责项目连续工作和资料掌控。".to_string(),
                next_steps: vec!["继续回归全局搜索".to_string()],
                updated_at: now.clone(),
                ..ProjectAnalysis::default()
            },
            audit: Vec::new(),
            draft: WorkspaceDraft::default(),
            recovery_points: Vec::new(),
            deepseek_authorization: DeepSeekAuthorization::default(),
            ..ProjectManifest::default()
        };

        let mut results = Vec::new();
        collect_project_search_results(
            &manifest,
            &normalize_search_text("全局 搜索"),
            &mut results,
        );

        assert!(results.iter().any(|item| item.content_type == "project"));
        let file_result = results
            .iter()
            .find(|item| item.content_type == "file")
            .expect("project file should enter search through FileProjection");
        assert_eq!(file_result.file_id, "file-1");
        assert_eq!(file_result.hash, "project-file-hash");
        assert_eq!(file_result.category, "需求文档");
        assert_eq!(file_result.lifecycle_status, "managed");
        assert_eq!(file_result.ownership_type, "existingProjectMaterial");
        assert!(results.iter().any(|item| item.content_type == "message"));
        assert!(results
            .iter()
            .any(|item| item.content_type == "codexReport"));
    }

    #[test]
    fn project_matching_prefers_explicit_project_evidence_and_returns_top_three() {
        let item = MaterialInboxItem {
            file_name: "感冒院需求说明-v3.docx".to_string(),
            source_path: r"D:\资料 收件\感冒院需求说明-v3.docx".to_string(),
            file_type: "docx".to_string(),
            suggested_category: "需求文档".to_string(),
            ..MaterialInboxItem::default()
        };
        let analysis = FileAnalysis {
            content_summary: "感冒院本地优先文件归位和 Inbox 工作流需求".to_string(),
            main_fields_or_sections: vec!["项目文件位置管理".to_string()],
            ..FileAnalysis::default()
        };
        let projects = vec![
            (
                ProjectSummary {
                    id: "gmy".to_string(),
                    name: "感冒院".to_string(),
                    description: "本地优先 AI 项目工作台与文件位置管理".to_string(),
                    ..ProjectSummary::default()
                },
                ProjectManifest {
                    project_analysis: ProjectAnalysis {
                        project_definition: "统一管理 Inbox 和受管文件位置".to_string(),
                        ..ProjectAnalysis::default()
                    },
                    ..ProjectManifest::default()
                },
            ),
            (
                ProjectSummary {
                    id: "erp".to_string(),
                    name: "ERP".to_string(),
                    ..ProjectSummary::default()
                },
                ProjectManifest::default(),
            ),
            (
                ProjectSummary {
                    id: "wts".to_string(),
                    name: "WTS".to_string(),
                    ..ProjectSummary::default()
                },
                ProjectManifest::default(),
            ),
            (
                ProjectSummary {
                    id: "atlas".to_string(),
                    name: "Atlas".to_string(),
                    ..ProjectSummary::default()
                },
                ProjectManifest::default(),
            ),
        ];

        let candidates = rank_project_candidates(&projects, &item, &analysis);

        assert_eq!(candidates.len(), 3);
        assert_eq!(candidates[0].candidate_project_id, "gmy");
        assert!(candidates[0].score >= 45);
        assert!(candidates[0]
            .matched_signals
            .contains(&"projectName".to_string()));
    }

    #[test]
    fn inbox_location_is_confined_and_uses_controlled_semantic_directory() {
        let root = test_root("inbox-location");
        fs::create_dir_all(&root).unwrap();
        let plan = decide_inbox_file_location(
            &root,
            &[],
            r"..\..\需求：一期?.docx",
            "docx",
            "Windows SendTo",
            "资料收件箱",
            "requirements",
        )
        .unwrap();

        assert!(plan.destination.starts_with(&root));
        assert!(plan.relative_path.contains("02_requirements"));
        assert!(
            !plan.destination.to_string_lossy().contains(".."),
            "{}",
            plan.destination.display()
        );
        assert!(!plan
            .destination
            .file_name()
            .unwrap()
            .to_string_lossy()
            .contains(':'));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn similar_content_can_form_a_version_family_without_final_name_only() {
        let root = test_root("content-version");
        fs::create_dir_all(&root).unwrap();
        let existing = FileRecord {
            id: "file-v1".to_string(),
            file_name: "项目范围说明.docx".to_string(),
            file_type: "docx".to_string(),
            category: "需求文档".to_string(),
            content_summary: "感冒院文件收件箱、项目匹配、位置归位和撤销流程".to_string(),
            version_group_id: "family-1".to_string(),
            current_version: "v1".to_string(),
            ..FileRecord::default()
        };
        let project = ProjectSummary {
            id: "project-1".to_string(),
            name: "感冒院".to_string(),
            root_dir: path_to_string(&root),
            ..ProjectSummary::default()
        };
        let manifest = ProjectManifest {
            project: project.clone(),
            files: vec![existing],
            ..ProjectManifest::default()
        };
        let item = MaterialInboxItem {
            file_name: "工作流调整稿.docx".to_string(),
            file_type: "docx".to_string(),
            suggested_category: "需求文档".to_string(),
            ..MaterialInboxItem::default()
        };
        let analysis = FileAnalysis {
            content_summary: "感冒院文件收件箱、项目匹配、位置归位和撤销流程更新".to_string(),
            ..FileAnalysis::default()
        };

        let candidate =
            find_new_version_candidate(&[(project, manifest)], &item, &analysis).unwrap();

        assert!(candidate.is_some());
        assert_eq!(candidate.unwrap().1.version_group_id, "family-1");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn duplicate_or_version_candidates_do_not_cross_projects_on_ties() {
        let shared_hash = "same-sha256".to_string();
        let item = MaterialInboxItem {
            file_name: "需求说明-v2.docx".to_string(),
            file_type: "docx".to_string(),
            source_hash: shared_hash.clone(),
            suggested_category: "需求文档".to_string(),
            ..MaterialInboxItem::default()
        };
        let analysis = FileAnalysis {
            content_summary: "共同的项目需求和验收说明".to_string(),
            ..FileAnalysis::default()
        };
        let roots = [test_root("candidate-alpha"), test_root("candidate-beta")];
        for root in &roots {
            fs::create_dir_all(root).unwrap();
        }
        let projects = ["alpha", "beta"]
            .into_iter()
            .zip(roots.iter())
            .map(|(id, root)| {
                let project = ProjectSummary {
                    id: id.to_string(),
                    name: id.to_string(),
                    root_dir: path_to_string(root),
                    ..ProjectSummary::default()
                };
                let file = FileRecord {
                    id: format!("file-{id}"),
                    file_name: "需求说明.docx".to_string(),
                    file_type: "docx".to_string(),
                    category: "需求文档".to_string(),
                    content_hash: shared_hash.clone(),
                    content_summary: analysis.content_summary.clone(),
                    current_version: "v1".to_string(),
                    version_group_id: format!("family-{id}"),
                    ..FileRecord::default()
                };
                (
                    project.clone(),
                    ProjectManifest {
                        project,
                        files: vec![file],
                        ..ProjectManifest::default()
                    },
                )
            })
            .collect::<Vec<_>>();

        assert!(find_duplicate_candidate(&projects, &shared_hash).is_none());
        assert!(find_new_version_candidate(&projects, &item, &analysis)
            .unwrap()
            .is_none());
        let ranked = rank_project_candidates(&projects, &item, &analysis);
        assert_eq!(ranked[0].score, ranked[1].score);
        assert!(ranked[0].matched_signals.contains(&"sha256".to_string()));
        for root in roots {
            fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn parses_bounded_ai_project_match_json() {
        let parsed = parse_ai_project_match(
            "```json\n{\"candidateProjectId\":\"p1\",\"score\":120,\"reasons\":[\"name\"]}\n```",
        )
        .unwrap();
        assert_eq!(parsed.candidate_project_id, "p1");
        assert_eq!(parsed.score, 100);
    }

    // ===== 第16~20步闭环测试 =====

    fn setup_project(label: &str) -> PathBuf {
        let root = test_root(label);
        ensure_project_dirs(&root).unwrap();
        let mut manifest = ProjectManifest::default();
        manifest.schema_version = MANIFEST_SCHEMA_VERSION;
        manifest.project = ProjectSummary {
            id: "p-test".to_string(),
            name: "测试项目".to_string(),
            ..ProjectSummary::default()
        };
        write_json_atomic(&manifest_path(&root), &manifest).unwrap();
        root
    }

    fn setup_test_git_repository(label: &str) -> PathBuf {
        let repo = test_root(label);
        fs::create_dir_all(&repo).unwrap();
        for args in [
            vec!["init"],
            vec!["config", "user.email", "test@example.local"],
            vec!["config", "user.name", "Ganmaoyuan Test"],
        ] {
            assert!(std::process::Command::new("git")
                .arg("-C")
                .arg(&repo)
                .args(args)
                .status()
                .unwrap()
                .success());
        }
        fs::write(repo.join("README.md"), "initial\n").unwrap();
        assert!(std::process::Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(["add", "README.md"])
            .status()
            .unwrap()
            .success());
        assert!(std::process::Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(["commit", "-m", "initial"])
            .status()
            .unwrap()
            .success());
        repo
    }

    #[test]
    fn continue_project_queue_prioritizes_action_and_excludes_resolved_focus() {
        let manifests = [
            ProjectManifest {
                project: ProjectSummary {
                    id: "project-a".to_string(),
                    name: "项目 A".to_string(),
                    root_dir: "D:\\Projects\\A".to_string(),
                    ..ProjectSummary::default()
                },
                ..ProjectManifest::default()
            },
            ProjectManifest {
                project: ProjectSummary {
                    id: "project-b".to_string(),
                    name: "项目 B".to_string(),
                    root_dir: "D:\\Projects\\B".to_string(),
                    ..ProjectSummary::default()
                },
                ..ProjectManifest::default()
            },
            ProjectManifest {
                project: ProjectSummary {
                    id: "project-c".to_string(),
                    name: "项目 C".to_string(),
                    root_dir: "D:\\Projects\\C".to_string(),
                    ..ProjectSummary::default()
                },
                ..ProjectManifest::default()
            },
        ];
        let focuses = manifests
            .iter()
            .map(|manifest| TodayProjectFocus {
                project_id: manifest.project.id.clone(),
                project_name: manifest.project.name.clone(),
                project_root: manifest.project.root_dir.clone(),
                ..TodayProjectFocus::default()
            })
            .collect::<Vec<_>>();
        let pending = vec![PendingActionProjection {
            id: "codex-review-a".to_string(),
            action_type: "codexAcceptance".to_string(),
            project_id: "project-a".to_string(),
            title: "验收 Codex 结果".to_string(),
            reason: "等待人工验收".to_string(),
            priority: "high".to_string(),
            status: "open".to_string(),
            ..PendingActionProjection::default()
        }];
        let activities = vec![ActivityProjection {
            id: "git-b".to_string(),
            project_id: "project-b".to_string(),
            summary: "Git 最近有变化".to_string(),
            ..ActivityProjection::default()
        }];
        let recommendations = build_continue_project_recommendations(
            &manifests,
            &focuses,
            &pending,
            &activities,
            &[],
            &[],
        );
        assert_eq!(recommendations.len(), 2);
        assert_eq!(recommendations[0].project_id, "project-a");
        assert_eq!(recommendations[1].project_id, "project-b");
        assert!(recommendations
            .iter()
            .all(|item| item.project_id != "project-c"));
        assert!(recommendations[0]
            .score_basis
            .iter()
            .any(|item| item.contains("有效待处理")));

        let snoozed = build_continue_project_recommendations(
            &manifests,
            &focuses,
            &pending,
            &activities,
            &[],
            &[ContinueProjectPreference {
                project_id: "project-a".to_string(),
                snoozed_until: (now_string().parse::<u128>().unwrap() + 86_400_000).to_string(),
                ..ContinueProjectPreference::default()
            }],
        );
        assert!(snoozed.iter().all(|item| item.project_id != "project-a"));

        let pinned = build_continue_project_recommendations(
            &manifests,
            &focuses,
            &[],
            &[],
            &[],
            &[ContinueProjectPreference {
                project_id: "project-c".to_string(),
                pinned: true,
                ..ContinueProjectPreference::default()
            }],
        );
        assert!(pinned
            .iter()
            .any(|item| item.project_id == "project-c" && item.pinned));
    }

    #[test]
    fn stable_manifest_reads_do_not_rewrite_derived_project_files() {
        let root = setup_project("stable-manifest-read");
        read_and_repair_manifest(&root).unwrap();
        let manifest_before = fs::read(manifest_path(&root)).unwrap();
        let file_analysis = file_analysis_path(&root);
        let file_analysis_before = fs::read(&file_analysis).unwrap();

        read_and_repair_manifest(&root).unwrap();

        assert_eq!(fs::read(manifest_path(&root)).unwrap(), manifest_before);
        assert_eq!(fs::read(file_analysis).unwrap(), file_analysis_before);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn unchanged_workspace_scan_does_not_persist_monitoring_timestamps() {
        let root = setup_project("stable-workspace-scan");
        read_and_repair_manifest(&root).unwrap();
        let manifest_before = fs::read(manifest_path(&root)).unwrap();

        let (_, changed, _) = scan_project_workspace(&path_to_string(&root)).unwrap();

        assert!(!changed);
        assert_eq!(fs::read(manifest_path(&root)).unwrap(), manifest_before);
        fs::remove_dir_all(root).unwrap();
    }

    fn make_file(id: &str, name: &str, summary: &str) -> FileRecord {
        FileRecord {
            id: id.to_string(),
            file_name: name.to_string(),
            content_summary: summary.to_string(),
            content_hash: format!("hash-{id}"),
            category: "other".to_string(),
            ..FileRecord::default()
        }
    }

    fn test_pending_action(
        id: &str,
        action_type: &str,
        project_id: &str,
        title: &str,
        reason: &str,
        source_ref: &str,
        priority: &str,
        status: &str,
        created_at: &str,
        evidence_refs: Vec<EvidenceRef>,
    ) -> PendingActionProjection {
        PendingActionProjection {
            id: id.to_string(),
            action_type: action_type.to_string(),
            project_id: project_id.to_string(),
            title: title.to_string(),
            reason: reason.to_string(),
            source_ref: source_ref.to_string(),
            priority: priority.to_string(),
            status: status.to_string(),
            created_at: created_at.to_string(),
            evidence_refs,
        }
    }

    // 场景1：需求文档 → 需求/需求变化发现
    #[test]
    fn requirement_document_yields_requirement_and_change_findings() {
        let root = setup_project("req-doc");
        let file = make_file(
            "f1",
            "产品需求文档.md",
            "本文档描述系统需求，要求支持导出，必须完成鉴权，新增报表模块。",
        );
        analyze_routed_file_impact(&path_to_string(&root), &file).unwrap();
        let manifest = read_and_repair_manifest(&root).unwrap();
        let types: Vec<&str> = manifest.project_impact_analyses[0]
            .findings
            .iter()
            .map(|f| f.finding_type.as_str())
            .collect();
        assert!(types.contains(&"requirement"), "应识别需求: {:?}", types);
        assert!(
            types.contains(&"requirementChange"),
            "应识别需求变化: {:?}",
            types
        );
        fs::remove_dir_all(&root).unwrap();
    }

    // 场景2：会议纪要 → 任务/决定候选
    #[test]
    fn meeting_notes_yield_task_and_decision_candidates() {
        let root = setup_project("meeting");
        let file = make_file(
            "f2",
            "会议纪要.md",
            "会议决定采用新架构，确认由张三负责，结论下周完成跟进整改。",
        );
        analyze_routed_file_impact(&path_to_string(&root), &file).unwrap();
        let manifest = read_and_repair_manifest(&root).unwrap();
        let types: Vec<&str> = manifest
            .project_action_candidates
            .iter()
            .map(|c| c.candidate_type.as_str())
            .collect();
        assert!(types.contains(&"task"), "应生成任务候选: {:?}", types);
        assert!(types.contains(&"decision"), "应生成决定候选: {:?}", types);
        assert!(manifest
            .project_action_candidates
            .iter()
            .all(|candidate| !candidate.decision_trace.id.is_empty()));
        assert!(!manifest.project_impact_analyses[0]
            .decision_trace
            .id
            .is_empty());
        fs::remove_dir_all(&root).unwrap();
    }

    // 场景3：测试报告 → 结果/风险/阻塞发现
    #[test]
    fn test_report_yields_outcome_risk_blocker_findings() {
        let root = setup_project("test-report");
        let file = make_file(
            "f3",
            "测试报告.md",
            "测试报告显示风险较高，存在失败用例，阻塞发布，但已通过验收交付结果。",
        );
        analyze_routed_file_impact(&path_to_string(&root), &file).unwrap();
        let manifest = read_and_repair_manifest(&root).unwrap();
        let types: Vec<&str> = manifest.project_impact_analyses[0]
            .findings
            .iter()
            .map(|f| f.finding_type.as_str())
            .collect();
        assert!(types.contains(&"blocker"), "应识别阻塞: {:?}", types);
        assert!(types.contains(&"outcome"), "应识别结果: {:?}", types);
        fs::remove_dir_all(&root).unwrap();
    }

    // 场景4：普通参考资料 → 不生成任务候选
    #[test]
    fn plain_reference_generates_no_task_candidate() {
        let root = setup_project("plain-ref");
        let file = make_file(
            "f4",
            "接口说明.md",
            "本文件为接口说明参考资料，供团队成员查阅理解。",
        );
        analyze_routed_file_impact(&path_to_string(&root), &file).unwrap();
        let manifest = read_and_repair_manifest(&root).unwrap();
        assert!(
            manifest.project_action_candidates.is_empty(),
            "参考资料不应生成行动候选: {:?}",
            manifest.project_action_candidates
        );
        fs::remove_dir_all(&root).unwrap();
    }

    // 场景5：重复分析 → 不重复影响分析与候选
    #[test]
    fn repeated_analysis_does_not_duplicate_impact_or_candidates() {
        let root = setup_project("dup");
        let file = make_file("f1", "需求.md", "需求：完成导出模块，新增报表。");
        analyze_routed_file_impact(&path_to_string(&root), &file).unwrap();
        analyze_routed_file_impact(&path_to_string(&root), &file).unwrap();
        let manifest = read_and_repair_manifest(&root).unwrap();
        assert_eq!(
            manifest.project_impact_analyses.len(),
            1,
            "影响分析不应重复"
        );
        let first_count = manifest.project_action_candidates.len();
        analyze_routed_file_impact(&path_to_string(&root), &file).unwrap();
        let manifest = read_and_repair_manifest(&root).unwrap();
        assert_eq!(
            manifest.project_action_candidates.len(),
            first_count,
            "候选不应重复"
        );
        fs::remove_dir_all(&root).unwrap();
    }

    // 场景6：DeepSeek 不可用 → 本地规则仍完成路由分析
    #[test]
    fn impact_analysis_works_without_deepseek() {
        let root = setup_project("no-deepseek");
        let file = make_file("f1", "需求.md", "需求：必须完成登录模块。");
        analyze_routed_file_impact(&path_to_string(&root), &file).unwrap();
        let manifest = read_and_repair_manifest(&root).unwrap();
        assert_eq!(manifest.project_impact_analyses.len(), 1);
        assert!(!manifest.project_impact_analyses[0].findings.is_empty());
        assert_eq!(manifest.project_impact_analyses[0].status, "needsReview");
        fs::remove_dir_all(&root).unwrap();
    }

    // 场景7：两个相似项目 → 互不污染
    #[test]
    fn separate_projects_do_not_pollute_each_other() {
        let root_a = setup_project("proj-a");
        let root_b = setup_project("proj-b");
        let file = make_file("f1", "需求.md", "需求：完成导出。");
        analyze_routed_file_impact(&path_to_string(&root_a), &file).unwrap();
        let a = read_and_repair_manifest(&root_a).unwrap();
        let b = read_and_repair_manifest(&root_b).unwrap();
        assert_eq!(a.project_impact_analyses.len(), 1);
        assert_eq!(b.project_impact_analyses.len(), 0, "项目B不应被污染");
        fs::remove_dir_all(&root_a).unwrap();
        fs::remove_dir_all(&root_b).unwrap();
    }

    // 场景8：用户忽略候选 → 不再重复生成待确认
    #[test]
    fn ignored_candidate_does_not_regenerate_pending_review() {
        let mut manifest = ProjectManifest::default();
        manifest.schema_version = MANIFEST_SCHEMA_VERSION;
        manifest.project = ProjectSummary {
            id: "p".to_string(),
            name: "t".to_string(),
            ..ProjectSummary::default()
        };
        let file = make_file("f1", "需求.md", "需求：完成登录。");
        let impact = build_local_project_impact(&manifest, &file);
        let candidates = build_action_candidates_from_impact(&impact);
        let proposal = build_project_state_proposal(&manifest, &file, &impact);
        upsert_pending_reviews_for_impact(&mut manifest, &impact, &candidates, &proposal);
        let before = manifest.pending_reviews.len();
        for review in manifest.pending_reviews.iter_mut() {
            review.status = "ignored".to_string();
        }
        upsert_pending_reviews_for_impact(&mut manifest, &impact, &candidates, &proposal);
        assert_eq!(
            manifest.pending_reviews.len(),
            before,
            "已忽略的待确认不应重复生成"
        );
    }

    // 场景9：状态提案应用后再撤销 → 恢复下一步
    #[test]
    fn state_proposal_apply_then_revert_restores_next_step() {
        let mut manifest = ProjectManifest::default();
        manifest.schema_version = MANIFEST_SCHEMA_VERSION;
        manifest.project = ProjectSummary {
            id: "p".to_string(),
            name: "t".to_string(),
            next_step: "原状态".to_string(),
            ..ProjectSummary::default()
        };
        let file = make_file("f1", "需求.md", "需求：调整目标。");
        let impact = build_local_project_impact(&manifest, &file);
        let proposal = build_project_state_proposal(&manifest, &file, &impact);
        assert_eq!(
            proposal.decision_trace.decision_type,
            "projectStateProposal"
        );
        apply_state_proposal_changes(&mut manifest, &proposal);
        assert_ne!(manifest.project.next_step, "原状态", "应用后应改变下一步");
        revert_state_proposal_changes(&mut manifest, &proposal);
        assert_eq!(manifest.project.next_step, "原状态", "撤销后应恢复下一步");
    }

    // 场景10：重启后数据持久化（从磁盘重新读取）
    #[test]
    fn impact_analysis_persists_across_reload() {
        let root = setup_project("persist");
        let file = make_file("f1", "需求.md", "需求：完成导出模块。");
        analyze_routed_file_impact(&path_to_string(&root), &file).unwrap();
        let manifest = read_and_repair_manifest(&root).unwrap();
        assert_eq!(manifest.project_impact_analyses.len(), 1);
        assert!(!manifest.project_action_candidates.is_empty());
        assert_eq!(manifest.project_state_proposals.len(), 1);
        assert!(manifest
            .pending_reviews
            .iter()
            .all(|item| item.kind != "projectState.proposal"));
        assert!(manifest.project.next_step.is_empty());
        assert!(!manifest.project_state_proposals[0]
            .decision_trace
            .id
            .is_empty());
        fs::remove_dir_all(&root).unwrap();
    }

    // 场景19：每日继续快照反映待确认与阻塞状态
    #[test]
    fn daily_continue_snapshot_reflects_pending_state() {
        let mut manifest = ProjectManifest::default();
        manifest.schema_version = MANIFEST_SCHEMA_VERSION;
        manifest.project = ProjectSummary {
            id: "p".to_string(),
            name: "测试项目".to_string(),
            ..ProjectSummary::default()
        };
        manifest.pending_reviews.push(PendingReviewItem {
            id: "r1".to_string(),
            status: "pending".to_string(),
            ..PendingReviewItem::default()
        });
        manifest
            .project_action_candidates
            .push(ProjectActionCandidate {
                id: "c1".to_string(),
                candidate_type: "blocker".to_string(),
                title: "阻塞项A".to_string(),
                status: "pending".to_string(),
                ..ProjectActionCandidate::default()
            });
        update_daily_continue_snapshot(&mut manifest);
        assert_eq!(manifest.daily_continue_snapshots.len(), 1);
        let snap = &manifest.daily_continue_snapshots[0];
        assert!(
            !snap.pending_confirmations.is_empty() || !snap.blockers.is_empty(),
            "快照应反映待确认或阻塞"
        );
    }

    #[test]
    fn pending_review_status_update_persists_and_updates_today_source() {
        let root = setup_project("pending-review-status");
        let mut manifest = read_and_repair_manifest(&root).unwrap();
        manifest.pending_reviews.push(PendingReviewItem {
            id: "review-1".to_string(),
            key: "review-key-1".to_string(),
            title: "确认新增资料位置".to_string(),
            detail: "文件归位建议需要人工确认。".to_string(),
            status: "pending".to_string(),
            detected_at: "100".to_string(),
            ..PendingReviewItem::default()
        });
        persist_project(&root, &manifest, None).unwrap();

        let updated = update_pending_review_status(
            path_to_string(&root),
            "review-1".to_string(),
            "resolved".to_string(),
        )
        .unwrap();

        assert_eq!(updated.pending_reviews[0].status, "resolved");
        assert_eq!(updated.monitoring.pending_count, 0);
        assert!(!updated
            .daily_continue_snapshots
            .last()
            .unwrap()
            .pending_confirmations
            .contains(&"确认新增资料位置".to_string()));

        let reloaded = read_and_repair_manifest(&root).unwrap();
        assert_eq!(reloaded.pending_reviews[0].status, "resolved");
        assert!(reloaded
            .audit
            .iter()
            .any(|event| event.action == "pendingReview.status"));
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn pending_review_status_rejects_invalid_state() {
        let root = setup_project("pending-review-invalid-status");
        let mut manifest = read_and_repair_manifest(&root).unwrap();
        manifest.pending_reviews.push(PendingReviewItem {
            id: "review-1".to_string(),
            key: "review-key-1".to_string(),
            title: "确认新增资料位置".to_string(),
            status: "pending".to_string(),
            detected_at: "100".to_string(),
            ..PendingReviewItem::default()
        });
        persist_project(&root, &manifest, None).unwrap();

        let result = update_pending_review_status(
            path_to_string(&root),
            "review-1".to_string(),
            "done".to_string(),
        );

        assert!(result.is_err());
        let reloaded = read_and_repair_manifest(&root).unwrap();
        assert_eq!(reloaded.pending_reviews[0].status, "pending");
        fs::remove_dir_all(&root).unwrap();
    }

    // ===== 第21~25步主动工作助手测试 =====

    #[test]
    fn proactive_workspace_uses_real_history_and_trace() {
        let mut manifest = ProjectManifest::default();
        manifest.project = ProjectSummary {
            id: "history-project".to_string(),
            name: "历史项目".to_string(),
            next_step: "完成回归测试".to_string(),
            ..ProjectSummary::default()
        };
        manifest.recovery_points.push(RecoveryPoint {
            id: "rp-1".to_string(),
            project_id: manifest.project.id.clone(),
            completed: "已完成资料分类".to_string(),
            next_step: "完成回归测试".to_string(),
            created_at: "100".to_string(),
            ..RecoveryPoint::default()
        });
        update_daily_continue_snapshot(&mut manifest);
        let snapshot = manifest.daily_continue_snapshots.last().unwrap();
        assert_eq!(snapshot.last_progress[0], "已完成资料分类");
        assert_eq!(snapshot.recommended_actions[0].action, "完成回归测试");
        assert!(!snapshot.recommended_actions[0].decision_trace.id.is_empty());
        assert_eq!(
            snapshot.recommended_actions[0].decision_trace.project_id,
            "history-project"
        );
    }

    #[test]
    fn proactive_workspace_with_no_history_creates_no_fake_action() {
        let mut manifest = ProjectManifest::default();
        manifest.project = ProjectSummary {
            id: "empty-project".to_string(),
            name: "空项目".to_string(),
            ..ProjectSummary::default()
        };
        update_daily_continue_snapshot(&mut manifest);
        let snapshot = manifest.daily_continue_snapshots.last().unwrap();
        assert!(snapshot.last_progress.is_empty());
        assert!(snapshot.recommended_actions.is_empty());
        assert!(manifest.project_state_summary.facts.is_empty());
    }

    #[test]
    fn cleared_scan_reviews_do_not_survive_as_continue_focus() {
        let mut manifest = ProjectManifest::default();
        manifest.project = ProjectSummary {
            id: "cleared-scan-project".to_string(),
            name: "已清理扫描项目".to_string(),
            next_step: "请先处理待确认事项：发现新文件：.gitignore".to_string(),
            ..ProjectSummary::default()
        };

        update_daily_continue_snapshot(&mut manifest);

        let snapshot = manifest.daily_continue_snapshots.last().unwrap();
        assert!(snapshot.pending_confirmations.is_empty());
        assert!(snapshot
            .recommended_actions
            .iter()
            .all(|item| !item.action.contains(".gitignore")));
        assert!(!manifest.project.next_step.contains("发现新文件"));
    }

    #[test]
    fn project_analysis_suggestions_do_not_become_current_next_step() {
        let mut manifest = ProjectManifest::default();
        manifest.project = ProjectSummary {
            id: "analysis-only-project".to_string(),
            next_step: "接入只读 Atlas Adapter 并评估 Atlas。".to_string(),
            ..ProjectSummary::default()
        };
        manifest.project_analysis.next_steps = vec![
            "接入只读 Atlas Adapter 并评估 Atlas。".to_string(),
            "补充正式验收标准。".to_string(),
        ];

        update_daily_continue_snapshot(&mut manifest);

        assert!(manifest.project.next_step.is_empty());
        assert_eq!(manifest.project_analysis.next_steps.len(), 2);

        manifest.project.next_step = FACTS_ONLY_NEXT_STEP_PLACEHOLDER.to_string();
        update_daily_continue_snapshot(&mut manifest);
        assert!(manifest.project.next_step.is_empty());
    }

    #[test]
    fn requirement_messages_do_not_create_active_project_tasks() {
        let mut manifest = ProjectManifest {
            project: ProjectSummary {
                id: "context-only".to_string(),
                ..ProjectSummary::default()
            },
            ..ProjectManifest::default()
        };
        let message = WorkspaceMessage {
            id: "message-1".to_string(),
            author: "user".to_string(),
            kind: "requirement".to_string(),
            text: "请根据项目资料生成下一步建议。".to_string(),
            created_at: "100".to_string(),
            ..WorkspaceMessage::default()
        };

        record_message_derivatives(&mut manifest, &message);

        assert!(manifest.tasks.is_empty());
    }

    #[test]
    fn legacy_message_derived_task_does_not_become_today_focus() {
        let mut manifest = ProjectManifest {
            project: ProjectSummary {
                id: "legacy-context".to_string(),
                ..ProjectSummary::default()
            },
            messages: vec![WorkspaceMessage {
                id: "message-1".to_string(),
                author: "user".to_string(),
                kind: "requirement".to_string(),
                text: "请根据项目资料生成下一步建议。".to_string(),
                created_at: "1".to_string(),
                ..WorkspaceMessage::default()
            }],
            tasks: vec![TaskRecord {
                id: "legacy-task".to_string(),
                title: "请根据项目资料生成下一步建议。".to_string(),
                status: "active".to_string(),
                source_message_id: "message-1".to_string(),
                created_at: "1".to_string(),
                updated_at: "1".to_string(),
            }],
            ..ProjectManifest::default()
        };

        update_daily_continue_snapshot(&mut manifest);

        assert!(manifest
            .daily_continue_snapshots
            .last()
            .expect("snapshot")
            .recommended_actions
            .is_empty());
        assert!(!manifest
            .project_attentions
            .iter()
            .any(|attention| attention.attention_type == "staleTask"));
    }

    #[test]
    fn continue_focus_requires_fresh_facts_before_business_focus() {
        let project = TodayProjectFocus {
            project_id: "p".to_string(),
            project_name: "项目".to_string(),
            project_root: "D:\\Project".to_string(),
            ..TodayProjectFocus::default()
        };
        let freshness = TodayFreshnessCheck {
            project_id: "p".to_string(),
            project_root: "D:\\Project".to_string(),
            status: "stale".to_string(),
            reason: "Git HEAD 不是最新。".to_string(),
            evidence_refs: vec![EvidenceRef {
                kind: "git".to_string(),
                label: "旧 Git snapshot".to_string(),
                ..EvidenceRef::default()
            }],
        };
        let action = test_pending_action(
            "codex-review:t1",
            "codexReview",
            "p",
            "Codex 任务需要检查：修复滚动",
            "结果证据需要人工检查。",
            "t1",
            "high",
            "pending",
            "200",
            vec![EvidenceRef {
                kind: "codexTask".to_string(),
                id: "t1".to_string(),
                label: "Codex 任务".to_string(),
                ..EvidenceRef::default()
            }],
        );

        let focus = build_continue_work_focus(
            &[],
            &[project],
            &[],
            &[action],
            &CodexTaskTodaySummary::default(),
            &WorkspaceActivitySummary::default(),
            &[freshness],
        )
        .expect("focus");

        assert_eq!(focus.title, "项目事实需要刷新");
        assert_eq!(focus.primary_action.action_type, "refreshFacts");
        assert_eq!(focus.freshness_status, "stale");
    }

    #[test]
    fn continue_focus_prefers_codex_review_over_old_file_noise() {
        let project = TodayProjectFocus {
            project_id: "p".to_string(),
            project_name: "项目".to_string(),
            project_root: "D:\\Project".to_string(),
            ..TodayProjectFocus::default()
        };
        let noisy = test_pending_action(
            "pending-review:app",
            "projectReview",
            "p",
            "原始资料缺失: app.exe",
            "历史缺失文件。",
            "app",
            "medium",
            "pending",
            "300",
            vec![EvidenceRef::default()],
        );
        let important = test_pending_action(
            "codex-review:t1",
            "codexReview",
            "p",
            "Codex 任务需要检查：结果证据",
            "结果证据需要人工检查。",
            "t1",
            "high",
            "pending",
            "100",
            vec![EvidenceRef {
                kind: "codexTask".to_string(),
                id: "t1".to_string(),
                label: "Codex 任务".to_string(),
                ..EvidenceRef::default()
            }],
        );
        let actions = govern_pending_actions_for_today(vec![noisy, important]);
        let focus = build_continue_work_focus(
            &[],
            &[project],
            &[],
            &actions,
            &CodexTaskTodaySummary::default(),
            &WorkspaceActivitySummary::default(),
            &[],
        )
        .expect("focus");

        assert_eq!(actions.len(), 1);
        assert_eq!(focus.title, "检查 Codex 任务结果");
        assert_eq!(focus.primary_action.action_type, "reviewCodexTask");
        assert_eq!(focus.evidence_refs.len(), 1);
    }

    #[test]
    fn continue_focus_prefers_awaiting_acceptance_before_general_review() {
        let acceptance = test_pending_action(
            "codex-acceptance:t1",
            "codexAcceptance",
            "p",
            "Codex 任务待验收：完成 Search 收口",
            "1 项人工验收尚未完成。",
            "t1",
            "high",
            "pending",
            "100",
            vec![EvidenceRef {
                kind: "codexTask".to_string(),
                id: "t1".to_string(),
                label: "待验收 Codex 任务".to_string(),
                ..EvidenceRef::default()
            }],
        );
        let general = test_pending_action(
            "project-review:r1",
            "projectReview",
            "p",
            "需要检查：旧资料",
            "普通待检查。",
            "r1",
            "medium",
            "pending",
            "200",
            vec![EvidenceRef {
                kind: "pendingReview".to_string(),
                id: "r1".to_string(),
                label: "旧资料检查".to_string(),
                ..EvidenceRef::default()
            }],
        );

        let focus = build_continue_work_focus(
            &[],
            &[TodayProjectFocus {
                project_id: "p".to_string(),
                project_root: "D:\\Project".to_string(),
                ..TodayProjectFocus::default()
            }],
            &[],
            &[general, acceptance],
            &CodexTaskTodaySummary::default(),
            &WorkspaceActivitySummary::default(),
            &[],
        )
        .expect("focus");

        assert_eq!(focus.title, "验收 Codex 交付结果");
        assert_eq!(focus.primary_action.action_type, "acceptCodexResult");
    }

    #[test]
    fn continue_focus_dedupes_pending_actions_and_requires_evidence() {
        let first = test_pending_action(
            "review:1",
            "projectReview",
            "p",
            "确认项目状态",
            "需要用户确认。",
            "r1",
            "medium",
            "pending",
            "100",
            Vec::new(),
        );
        let duplicate = test_pending_action(
            "review:2",
            "projectReview",
            "p",
            "确认项目状态",
            "重复提醒。",
            "r2",
            "medium",
            "pending",
            "200",
            Vec::new(),
        );

        let actions = govern_pending_actions_for_today(vec![first, duplicate]);
        let focus = build_continue_work_focus(
            &[],
            &[TodayProjectFocus {
                project_id: "p".to_string(),
                project_root: "D:\\Project".to_string(),
                ..TodayProjectFocus::default()
            }],
            &[],
            &actions,
            &CodexTaskTodaySummary::default(),
            &WorkspaceActivitySummary::default(),
            &[],
        )
        .expect("focus");

        assert_eq!(actions.len(), 1);
        assert_eq!(focus.source_action_ids.len(), 1);
        assert_eq!(focus.evidence_refs.len(), 1);
    }

    #[test]
    fn continue_focus_does_not_guess_without_evidence() {
        let focus = build_continue_work_focus(
            &[],
            &[],
            &[],
            &[],
            &CodexTaskTodaySummary::default(),
            &WorkspaceActivitySummary::default(),
            &[],
        )
        .expect("focus");

        assert_eq!(focus.title, "当前没有足够事实确定唯一下一步");
        assert_eq!(focus.primary_action.action_type, "openPendingAction");
        assert!(focus.evidence_refs.is_empty());
    }

    #[test]
    fn continue_focus_git_refresh_updates_stale_snapshot() {
        let root = setup_project("continue-focus-git-refresh-project");
        let repo = test_root("continue-focus-git-refresh-repo");
        fs::create_dir_all(&repo).unwrap();
        assert!(std::process::Command::new("git")
            .arg("-C")
            .arg(&repo)
            .arg("init")
            .status()
            .unwrap()
            .success());
        for args in [
            vec!["config", "user.email", "test@example.local"],
            vec!["config", "user.name", "Ganmaoyuan Test"],
        ] {
            assert!(std::process::Command::new("git")
                .arg("-C")
                .arg(&repo)
                .args(args)
                .status()
                .unwrap()
                .success());
        }
        fs::write(repo.join("README.md"), "initial").unwrap();
        assert!(std::process::Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(["add", "README.md"])
            .status()
            .unwrap()
            .success());
        assert!(std::process::Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(["commit", "-m", "initial"])
            .status()
            .unwrap()
            .success());
        let first = capture_git_snapshot("p-test", &repo).unwrap();
        write_git_snapshot(&root, &first).unwrap();
        fs::write(repo.join("README.md"), "changed").unwrap();
        assert!(std::process::Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(["add", "README.md"])
            .status()
            .unwrap()
            .success());
        assert!(std::process::Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(["commit", "-m", "changed"])
            .status()
            .unwrap()
            .success());

        let manifest = read_and_repair_manifest(&root).unwrap();
        let freshness = refresh_project_git_facts_for_today(&root, &manifest);
        let refreshed = read_git_snapshot(&root).unwrap().unwrap();

        assert_eq!(freshness.status, "fresh");
        assert_ne!(refreshed.head, first.head);
        assert_eq!(read_work_ledger(&root).unwrap().events.len(), 1);
        fs::remove_dir_all(root).unwrap();
        fs::remove_dir_all(repo).unwrap();
    }

    #[test]
    fn today_read_uses_persisted_git_fact_without_refreshing_repository() {
        let root = setup_project("today-read-persisted-git");
        let repository = test_root("today-read-persisted-git-repository");
        fs::create_dir_all(&repository).unwrap();
        let mut manifest = read_and_repair_manifest(&root).unwrap();
        manifest.project.repository_path = path_to_string(&repository);
        persist_project(&root, &manifest, None).unwrap();
        write_git_snapshot(
            &root,
            &GitSnapshot {
                project_id: manifest.project.id.clone(),
                repository_path: path_to_string(&repository),
                head: "abc123def456".to_string(),
                head_short: "abc123d".to_string(),
                ..GitSnapshot::default()
            },
        )
        .unwrap();

        let check = read_project_git_facts_for_today(&root, &manifest);

        assert_eq!(check.status, "fresh");
        assert_eq!(check.evidence_refs[0].id, "abc123def456");
        assert_eq!(read_work_ledger(&root).unwrap().events.len(), 0);
        fs::remove_dir_all(root).unwrap();
        fs::remove_dir_all(repository).unwrap();
    }

    #[test]
    fn continue_focus_non_git_project_does_not_block_business_focus() {
        let root = setup_project("continue-focus-non-git-project");
        let manifest = read_and_repair_manifest(&root).unwrap();
        let freshness = refresh_project_git_facts_for_today(&root, &manifest);
        let action = test_pending_action(
            "codex-review:t1",
            "codexReview",
            &manifest.project.id,
            "Codex 任务需要检查：结果证据",
            "结果证据需要人工检查。",
            "t1",
            "high",
            "pending",
            "200",
            vec![EvidenceRef {
                kind: "codexTask".to_string(),
                id: "t1".to_string(),
                label: "Codex 任务".to_string(),
                ..EvidenceRef::default()
            }],
        );

        let focus = build_continue_work_focus(
            std::slice::from_ref(&manifest),
            &[TodayProjectFocus {
                project_id: manifest.project.id.clone(),
                project_root: path_to_string(&root),
                ..TodayProjectFocus::default()
            }],
            &[],
            &[action],
            &CodexTaskTodaySummary::default(),
            &WorkspaceActivitySummary::default(),
            &[freshness.clone()],
        )
        .expect("focus");

        assert_eq!(freshness.status, "notApplicable");
        assert_eq!(focus.title, "检查 Codex 任务结果");
        assert_eq!(focus.primary_action.action_type, "reviewCodexTask");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn continue_focus_uses_repository_snapshot_when_project_root_is_not_repo() {
        let root = setup_project("continue-focus-associated-project");
        let repo = test_root("continue-focus-associated-repo");
        fs::create_dir_all(&repo).unwrap();
        assert!(std::process::Command::new("git")
            .arg("-C")
            .arg(&repo)
            .arg("init")
            .status()
            .unwrap()
            .success());
        for args in [
            vec!["config", "user.email", "test@example.local"],
            vec!["config", "user.name", "Ganmaoyuan Test"],
        ] {
            assert!(std::process::Command::new("git")
                .arg("-C")
                .arg(&repo)
                .args(args)
                .status()
                .unwrap()
                .success());
        }
        fs::write(repo.join("README.md"), "initial").unwrap();
        assert!(std::process::Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(["add", "README.md"])
            .status()
            .unwrap()
            .success());
        assert!(std::process::Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(["commit", "-m", "initial"])
            .status()
            .unwrap()
            .success());
        let initial = capture_git_snapshot("p-test", &repo).unwrap();
        write_git_snapshot(&root, &initial).unwrap();

        let manifest = read_and_repair_manifest(&root).unwrap();
        let freshness = refresh_project_git_facts_for_today(&root, &manifest);
        let refreshed = read_git_snapshot(&root).unwrap().unwrap();

        assert_eq!(freshness.status, "fresh");
        assert_eq!(
            refreshed.repository_path,
            path_to_string(&repo.canonicalize().unwrap())
        );
        assert_eq!(refreshed.head, initial.head);
        fs::remove_dir_all(root).unwrap();
        fs::remove_dir_all(repo).unwrap();
    }

    #[test]
    fn git_refresh_persists_repository_link_for_project_and_new_codex_tasks() {
        let root = setup_project("repository-link-project");
        let repo = test_root("repository-link-repo");
        fs::create_dir_all(&repo).unwrap();
        assert!(std::process::Command::new("git")
            .arg("-C")
            .arg(&repo)
            .arg("init")
            .status()
            .unwrap()
            .success());
        for args in [
            vec!["config", "user.email", "test@example.local"],
            vec!["config", "user.name", "Ganmaoyuan Test"],
        ] {
            assert!(std::process::Command::new("git")
                .arg("-C")
                .arg(&repo)
                .args(args)
                .status()
                .unwrap()
                .success());
        }
        fs::write(repo.join("README.md"), "initial").unwrap();
        assert!(std::process::Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(["add", "README.md"])
            .status()
            .unwrap()
            .success());
        assert!(std::process::Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(["commit", "-m", "initial"])
            .status()
            .unwrap()
            .success());

        let snapshot =
            refresh_git_snapshot(path_to_string(&root), Some(path_to_string(&repo))).unwrap();
        let linked_manifest = read_and_repair_manifest(&root).unwrap();
        let expected = path_to_string(&repo.canonicalize().unwrap());
        assert_eq!(snapshot.repository_path, expected);
        assert_eq!(linked_manifest.project.repository_path, expected);

        fs::remove_file(git_snapshot_path(&root)).unwrap();
        let freshness = refresh_project_git_facts_for_today(&root, &linked_manifest);
        assert_eq!(freshness.status, "fresh");
        let task_id = generate_codex_prompt(path_to_string(&root))
            .unwrap()
            .prompt
            .id;
        assert_eq!(
            read_codex_task(&root, &task_id).unwrap().repository_path,
            expected
        );

        fs::remove_dir_all(root).unwrap();
        fs::remove_dir_all(repo).unwrap();
    }

    #[test]
    fn invalid_repository_refresh_does_not_create_a_project_link() {
        let root = setup_project("repository-link-invalid");
        let invalid = test_root("repository-link-invalid-directory");
        fs::create_dir_all(&invalid).unwrap();

        assert!(
            refresh_git_snapshot(path_to_string(&root), Some(path_to_string(&invalid))).is_err()
        );
        assert!(read_and_repair_manifest(&root)
            .unwrap()
            .project
            .repository_path
            .is_empty());

        fs::remove_dir_all(root).unwrap();
        fs::remove_dir_all(invalid).unwrap();
    }

    #[test]
    fn legacy_same_project_git_snapshot_migrates_to_a_repository_link() {
        let root = setup_project("repository-link-legacy");
        let repository = test_root("repository-link-legacy-repo");
        fs::create_dir_all(&repository).unwrap();
        let legacy: ProjectManifest = read_json(&manifest_path(&root)).unwrap();
        write_git_snapshot(
            &root,
            &GitSnapshot {
                project_id: legacy.project.id.clone(),
                repository_path: path_to_string(&repository),
                ..GitSnapshot::default()
            },
        )
        .unwrap();

        let repaired = read_and_repair_manifest(&root).unwrap();
        assert_eq!(
            repaired.project.repository_path,
            path_to_string(&repository)
        );

        fs::remove_dir_all(root).unwrap();
        fs::remove_dir_all(repository).unwrap();
    }

    #[test]
    fn legacy_git_snapshot_for_another_project_is_not_migrated() {
        let root = setup_project("repository-link-other-project");
        let repository = test_root("repository-link-other-project-repo");
        fs::create_dir_all(&repository).unwrap();
        write_git_snapshot(
            &root,
            &GitSnapshot {
                project_id: "another-project".to_string(),
                repository_path: path_to_string(&repository),
                ..GitSnapshot::default()
            },
        )
        .unwrap();

        assert!(read_and_repair_manifest(&root)
            .unwrap()
            .project
            .repository_path
            .is_empty());

        fs::remove_dir_all(root).unwrap();
        fs::remove_dir_all(repository).unwrap();
    }

    #[test]
    fn project_blocker_is_visible_in_today_snapshot() {
        let mut manifest = ProjectManifest::default();
        manifest.project.id = "blocked-project".to_string();
        manifest.project.name = "阻塞项目".to_string();
        manifest
            .project_action_candidates
            .push(ProjectActionCandidate {
                id: "blocker-1".to_string(),
                candidate_type: "blocker".to_string(),
                title: "等待测试环境".to_string(),
                status: "pending".to_string(),
                ..ProjectActionCandidate::default()
            });
        update_daily_continue_snapshot(&mut manifest);
        assert!(manifest
            .project_state_summary
            .blockers
            .contains(&"等待测试环境".to_string()));
        assert!(manifest
            .daily_continue_snapshots
            .last()
            .unwrap()
            .blockers
            .contains(&"等待测试环境".to_string()));
    }

    #[test]
    fn file_added_after_recovery_creates_attention() {
        let mut manifest = ProjectManifest::default();
        manifest.project.id = "file-change".to_string();
        manifest.recovery_points.push(RecoveryPoint {
            id: "rp-old".to_string(),
            created_at: "100".to_string(),
            ..RecoveryPoint::default()
        });
        manifest.files.push(FileRecord {
            id: "file-new".to_string(),
            file_name: "新增需求.docx".to_string(),
            content_summary: "新增验收要求".to_string(),
            modified_at: "200".to_string(),
            ..FileRecord::default()
        });
        update_project_attentions(&mut manifest);
        let attention = manifest
            .project_attentions
            .iter()
            .find(|item| item.attention_type == "fileChange")
            .expect("new file attention");
        assert_eq!(attention.status, "pending");
        assert_eq!(attention.decision_trace.decision_type, "riskCandidate");
    }

    #[test]
    fn proactive_rules_work_without_deepseek_configuration() {
        let mut manifest = ProjectManifest::default();
        manifest.project.id = "offline-project".to_string();
        manifest.tasks.push(TaskRecord {
            id: "offline-task".to_string(),
            title: "离线整理资料".to_string(),
            status: "active".to_string(),
            source_message_id: "message-1".to_string(),
            ..TaskRecord::default()
        });
        update_daily_continue_snapshot(&mut manifest);
        assert_eq!(
            manifest
                .daily_continue_snapshots
                .last()
                .unwrap()
                .recommended_actions[0]
                .action,
            "离线整理资料"
        );
    }

    #[test]
    fn proactive_state_does_not_cross_projects() {
        let mut first = ProjectManifest::default();
        first.project.id = "project-a".to_string();
        first.project.name = "A".to_string();
        first.tasks.push(TaskRecord {
            id: "task-a".to_string(),
            title: "完成 A 项目验证".to_string(),
            status: "active".to_string(),
            ..TaskRecord::default()
        });
        let mut second = ProjectManifest::default();
        second.project.id = "project-b".to_string();
        second.project.name = "B".to_string();
        update_daily_continue_snapshot(&mut first);
        update_daily_continue_snapshot(&mut second);
        assert_eq!(
            first
                .daily_continue_snapshots
                .last()
                .unwrap()
                .recommended_actions
                .len(),
            1
        );
        assert!(second
            .daily_continue_snapshots
            .last()
            .unwrap()
            .recommended_actions
            .is_empty());
        assert!(second.project_state_summary.facts.is_empty());
    }

    #[test]
    fn proactive_state_persists_across_reload() {
        let root = setup_project("proactive-persist");
        let mut manifest = read_and_repair_manifest(&root).unwrap();
        manifest.project.root_dir = path_to_string(&root);
        manifest.tasks.push(TaskRecord {
            id: "persist-task".to_string(),
            title: "重启后继续".to_string(),
            status: "active".to_string(),
            ..TaskRecord::default()
        });
        update_daily_continue_snapshot(&mut manifest);
        persist_project(&root, &manifest, None).unwrap();
        let reloaded = read_and_repair_manifest(&root).unwrap();
        assert_eq!(reloaded.project_state_summary.current_phase, "任务推进");
        assert_eq!(
            reloaded
                .daily_continue_snapshots
                .last()
                .unwrap()
                .recommended_actions[0]
                .action,
            "重启后继续"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn work_pattern_candidate_requires_repeated_real_records() {
        let mut manifest = ProjectManifest::default();
        manifest.project.id = "pattern-project".to_string();
        manifest.daily_sessions = vec![
            DailySession {
                id: "d1".to_string(),
                ..DailySession::default()
            },
            DailySession {
                id: "d2".to_string(),
                ..DailySession::default()
            },
        ];
        manifest.recovery_points = vec![
            RecoveryPoint {
                id: "r1".to_string(),
                completed: "整理资料".to_string(),
                next_step: "分析需求".to_string(),
                ..RecoveryPoint::default()
            },
            RecoveryPoint {
                id: "r2".to_string(),
                completed: "分析需求".to_string(),
                next_step: "执行任务".to_string(),
                ..RecoveryPoint::default()
            },
        ];
        update_work_pattern_candidates(&mut manifest);
        assert_eq!(manifest.work_pattern_candidates.len(), 1);
        assert!(manifest.work_pattern_candidates[0].review_required);
        assert_eq!(
            manifest.work_pattern_candidates[0]
                .decision_trace
                .decision_type,
            "skillCandidate"
        );
    }

    #[test]
    fn ownership_project_name_hit_is_existing_project() {
        let projects = vec![(
            ProjectSummary {
                id: "p1".to_string(),
                name: "感冒院".to_string(),
                ..ProjectSummary::default()
            },
            ProjectManifest::default(),
        )];
        let mut item = MaterialInboxItem::default();
        item.file_name = "感冒院项目规划.md".to_string();
        let analysis = FileAnalysis {
            file_name: "感冒院项目规划.md".to_string(),
            content_summary: "感冒院工作台规划说明".to_string(),
            ..FileAnalysis::default()
        };
        assert_eq!(
            classify_ownership(&item, &analysis, &projects),
            inbox_routing::OWNERSHIP_EXISTING_PROJECT
        );
    }

    #[test]
    fn stale_same_hash_analysis_is_rebuilt_without_copying() {
        let root = test_root("stale-inbox-analysis");
        fs::create_dir_all(&root).unwrap();
        let source = root.join("车间生产订单报表.txt");
        fs::write(&source, "车间 订单 工单 排产 完工数量 交期").unwrap();
        let source_hash = hash_file(&source).unwrap();
        let previous = MaterialInboxItem {
            id: "inbox-old".to_string(),
            file_name: "车间生产订单报表.txt".to_string(),
            source_path: path_to_string(&source),
            source_hash: source_hash.clone(),
            received_count: 1,
            source_history: vec![InboxSourceEvent {
                source_path: path_to_string(&source),
                hash: source_hash.clone(),
                ..InboxSourceEvent::default()
            }],
            analysis_version: MATERIAL_INBOX_ANALYSIS_VERSION - 1,
            ..MaterialInboxItem::default()
        };

        assert!(should_refresh_inbox_analysis(previous.analysis_version));
        let refreshed = process_repeated_inbox_file(
            &previous,
            &[],
            &[],
            &source,
            &source_hash,
            fs::metadata(&source).unwrap().len(),
            "testImport",
        )
        .unwrap();

        assert_eq!(refreshed.id, previous.id);
        assert_eq!(refreshed.received_count, 2);
        assert_eq!(refreshed.source_history.len(), 2);
        assert_eq!(refreshed.analysis_version, MATERIAL_INBOX_ANALYSIS_VERSION);
        assert_eq!(refreshed.document_type, "车间生产/订单报表");
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn current_same_hash_analysis_reuses_cache() {
        let root = test_root("current-inbox-analysis");
        fs::create_dir_all(&root).unwrap();
        let source = root.join("车间生产订单报表.txt");
        fs::write(&source, "车间 订单 工单 排产 完工数量 交期").unwrap();
        let source_hash = hash_file(&source).unwrap();
        let previous = MaterialInboxItem {
            id: "inbox-current".to_string(),
            source_hash: source_hash.clone(),
            document_type: "已缓存判断".to_string(),
            received_count: 1,
            analysis_version: MATERIAL_INBOX_ANALYSIS_VERSION,
            ..MaterialInboxItem::default()
        };

        let reused = process_repeated_inbox_file(
            &previous,
            &[],
            &[],
            &source,
            &source_hash,
            fs::metadata(&source).unwrap().len(),
            "testImport",
        )
        .unwrap();

        assert_eq!(reused.document_type, "已缓存判断");
        assert_eq!(reused.received_count, 2);
        assert_eq!(reused.source_history.len(), 1);
        assert!(reused.result_note.contains("复用缓存"));
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn manual_reanalysis_updates_semantics_without_new_record() {
        let root = test_root("manual-inbox-analysis");
        fs::create_dir_all(&root).unwrap();
        let source = root.join("VAVE申请单.txt");
        fs::write(&source, "VAVE 降本申请 预计收益 审批").unwrap();
        let source_hash = hash_file(&source).unwrap();
        let previous = MaterialInboxItem {
            id: "stable-inbox-id".to_string(),
            file_name: "VAVE申请单.txt".to_string(),
            source_path: path_to_string(&source),
            source_hash: source_hash.clone(),
            document_type: "文本资料".to_string(),
            received_count: 3,
            analysis_version: MATERIAL_INBOX_ANALYSIS_VERSION,
            ..MaterialInboxItem::default()
        };

        let refreshed = rebuild_material_inbox_analysis(
            &previous,
            &[],
            &[],
            &source,
            &source_hash,
            fs::metadata(&source).unwrap().len(),
        )
        .unwrap();

        assert_eq!(refreshed.id, "stable-inbox-id");
        assert_eq!(refreshed.received_count, 3);
        assert_eq!(refreshed.document_type, "VAVE/降本申请单");
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn new_inbox_file_runs_current_full_analysis() {
        let root = test_root("new-inbox-analysis");
        fs::create_dir_all(&root).unwrap();
        let source = root.join("周工作计划.txt");
        fs::write(
            &source,
            "本周工作安排 周计划 负责人 开始日期 截止日期 下周任务",
        )
        .unwrap();
        let source_hash = hash_file(&source).unwrap();
        let (analysis, _) = analyze_managed_file(
            "new-inbox-id",
            "周工作计划.txt",
            &source,
            "txt",
            &source_hash,
        );
        let item = build_material_inbox_item(
            &[],
            &path_to_string(&source),
            &source_hash,
            fs::metadata(&source).unwrap().len(),
            &analysis,
            "test",
        )
        .unwrap();

        assert_eq!(item.analysis_version, MATERIAL_INBOX_ANALYSIS_VERSION);
        assert_eq!(item.document_type, "工作计划");
        assert_eq!(item.document_purpose, "plan");
        assert!(item
            .decision_traces
            .iter()
            .any(|trace| trace.decision_type == "documentPurpose"));
        assert!(!item.business_purpose.is_empty());
        assert!(!item.ownership_type.is_empty());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn ownership_general_business_material() {
        let projects = vec![(
            ProjectSummary {
                id: "p1".to_string(),
                name: "感冒院".to_string(),
                ..ProjectSummary::default()
            },
            ProjectManifest::default(),
        )];
        let mut item = MaterialInboxItem::default();
        item.file_name = "车间订单报表.xlsx".to_string();
        let analysis = FileAnalysis {
            file_name: "车间订单报表.xlsx".to_string(),
            content_summary: "订单号 工序 完成数量 报废数量".to_string(),
            ..FileAnalysis::default()
        };
        assert_eq!(
            classify_ownership(&item, &analysis, &projects),
            inbox_routing::OWNERSHIP_GENERAL_WORK
        );
    }

    #[test]
    fn ownership_reference_material_enters_general_classification() {
        let mut item = MaterialInboxItem::default();
        item.file_name = "参考资料-行业规范.pdf".to_string();
        let analysis = FileAnalysis {
            file_name: "参考资料-行业规范.pdf".to_string(),
            content_summary: "参考资料".to_string(),
            ..FileAnalysis::default()
        };
        assert_eq!(
            classify_ownership(&item, &analysis, &[]),
            inbox_routing::OWNERSHIP_GENERAL_WORK
        );
    }

    #[test]
    fn general_workbook_build_does_not_match_projects() {
        let projects = vec![(
            ProjectSummary {
                id: "p1".to_string(),
                name: "感冒院".to_string(),
                ..ProjectSummary::default()
            },
            ProjectManifest::default(),
        )];
        let mut analysis = FileAnalysis::default();
        analysis.file_id = new_id();
        analysis.file_name = "车间订单报表.xlsx".to_string();
        analysis.parse_status = "success".to_string();
        analysis.recommended_category = "数据表格".to_string();
        analysis.document_type = "车间生产/订单报表".to_string();
        analysis.business_domain = "制造/生产".to_string();
        analysis.content_summary = "订单号 工序 完成数量 报废数量 报废率".to_string();
        let item = build_material_inbox_item(
            &projects,
            r"C:\tmp\车间订单报表.xlsx",
            &format!("hash-{}", new_id()),
            1024,
            &analysis,
            "test",
        )
        .unwrap();
        assert_eq!(item.ownership_type, inbox_routing::OWNERSHIP_GENERAL_WORK);
        assert!(
            item.project_candidates.is_empty(),
            "通用资料不应生成项目候选"
        );
        assert!(item.target_project_id.is_empty(), "通用资料不应绑定项目");
        assert_eq!(item.global_destination, inbox_routing::GLOBAL_DEST_GENERAL);
        assert_eq!(item.general_material_domain, "manufacturing");
        assert_eq!(item.general_material_category, "productionData");
        assert_eq!(item.recommended_location, "生产数据");
    }

    #[test]
    fn vave_excel_routes_to_engineering_vave_with_high_confidence() {
        let analysis = FileAnalysis {
            file_id: new_id(),
            file_name: "瑞尔-VAVE申请单.xlsx".to_string(),
            parse_status: "success".to_string(),
            recommended_category: "数据表格".to_string(),
            document_type: "VAVE/降本申请单".to_string(),
            business_domain: "研发/质量/降本".to_string(),
            business_purpose: "VAVE 申请流程与审批".to_string(),
            content_summary: "VAVE申请确认单，包含预计收益与审批状态。".to_string(),
            main_fields_or_sections: vec![
                "VAVE申请确认单：20 行 × 12 列；表头：项目、预计收益、审批状态".to_string(),
            ],
            ..FileAnalysis::default()
        };
        let item = build_material_inbox_item(
            &[],
            r"D:\资料\瑞尔-VAVE申请单.xlsx",
            "hash-vave",
            1024,
            &analysis,
            "test",
        )
        .unwrap();

        assert_eq!(item.ownership_type, inbox_routing::OWNERSHIP_GENERAL_WORK);
        assert_eq!(item.general_material_domain, "engineering");
        assert_eq!(item.general_material_category, "vave");
        assert_eq!(
            item.recommended_relative_location,
            "general/engineering/vave"
        );
        assert_eq!(item.recommended_location, "改善申请");
        assert!(item.confidence_score >= 78);
        assert!(should_auto_route_general_material(&item));
    }

    #[test]
    fn workshop_order_excel_routes_to_manufacturing_production_data() {
        let analysis = FileAnalysis {
            file_id: new_id(),
            file_name: "车间订单报表.xlsx".to_string(),
            parse_status: "success".to_string(),
            recommended_category: "数据表格".to_string(),
            document_type: "车间生产/订单报表".to_string(),
            business_domain: "制造/生产".to_string(),
            business_purpose: "订单进度与完工状态跟踪".to_string(),
            content_summary: "车间生产订单、工序、完工数量与交期。".to_string(),
            main_fields_or_sections: vec![
                "生产订单：100 行 × 8 列；表头：订单号、工序、完工数量、交期".to_string(),
            ],
            ..FileAnalysis::default()
        };
        let item = build_material_inbox_item(
            &[],
            r"D:\资料\车间订单报表.xlsx",
            "hash-production",
            2048,
            &analysis,
            "test",
        )
        .unwrap();

        assert_eq!(item.ownership_type, inbox_routing::OWNERSHIP_GENERAL_WORK);
        assert_eq!(item.general_material_domain, "manufacturing");
        assert_eq!(item.general_material_category, "productionData");
        assert_eq!(item.recommended_location, "生产数据");
        assert!(item.confidence_score >= 78);
    }

    #[test]
    fn work_plan_excel_routes_to_management_work_plan() {
        let analysis = FileAnalysis {
            file_id: new_id(),
            file_name: "周工作计划.xlsx".to_string(),
            parse_status: "success".to_string(),
            recommended_category: "数据表格".to_string(),
            document_type: "工作计划".to_string(),
            business_domain: "管理".to_string(),
            business_purpose: "人员与部门工作安排".to_string(),
            content_summary: "本周工作安排与下周任务计划。".to_string(),
            main_fields_or_sections: vec![
                "周工作计划：15 行 × 5 列；表头：日期、任务、责任人、状态".to_string(),
            ],
            ..FileAnalysis::default()
        };
        let item = build_material_inbox_item(
            &[],
            r"D:\资料\周工作计划.xlsx",
            "hash-plan",
            1024,
            &analysis,
            "test",
        )
        .unwrap();

        assert_eq!(item.ownership_type, inbox_routing::OWNERSHIP_GENERAL_WORK);
        assert_eq!(item.general_material_domain, "management");
        assert_eq!(item.general_material_category, "workPlan");
        assert_eq!(item.recommended_location, "工作计划");
        assert!(item.confidence_score >= 78);
    }

    #[test]
    fn unknown_excel_stays_pending_without_auto_route() {
        let analysis = FileAnalysis {
            file_id: new_id(),
            file_name: "未命名资料.xlsx".to_string(),
            parse_status: "success".to_string(),
            recommended_category: "数据表格".to_string(),
            document_type: "数据表格".to_string(),
            business_domain: "未分类".to_string(),
            content_summary: "识别到一个工作表。".to_string(),
            main_fields_or_sections: vec!["Sheet1：3 行 × 2 列；表头：A、B".to_string()],
            ..FileAnalysis::default()
        };
        let item = build_material_inbox_item(
            &[],
            r"D:\资料\未命名资料.xlsx",
            "hash-unknown",
            512,
            &analysis,
            "test",
        )
        .unwrap();

        assert_eq!(item.ownership_type, inbox_routing::OWNERSHIP_NEEDS_REVIEW);
        assert!(!should_auto_route_general_material(&item));
        assert_eq!(item.processing_status, inbox_routing::PENDING_REVIEW);
    }

    fn build_test_inbox_item(file_name: &str, summary: &str, fields: &[&str]) -> MaterialInboxItem {
        let analysis = FileAnalysis {
            file_id: new_id(),
            file_name: file_name.to_string(),
            parse_status: "success".to_string(),
            recommended_category: "数据表格".to_string(),
            document_type: "数据表格".to_string(),
            content_summary: summary.to_string(),
            main_fields_or_sections: fields.iter().map(|value| value.to_string()).collect(),
            ..FileAnalysis::default()
        };
        build_material_inbox_item(
            &[],
            &format!(r"D:\候选判断测试\{file_name}"),
            &format!("hash-{}", new_id()),
            512,
            &analysis,
            "test",
        )
        .unwrap()
    }

    #[test]
    fn account_import_template_is_master_data_not_project_candidate() {
        let item = build_test_inbox_item(
            "account_import_template.xlsx",
            "用户账号批量导入模板",
            &["账号、姓名、部门、角色"],
        );
        assert_eq!(item.ownership_type, inbox_routing::OWNERSHIP_GENERAL_WORK);
        assert_eq!(item.material_semantic_type, "masterDataOrTemplate");
        assert_eq!(item.general_material_domain, "management");
        assert_eq!(item.general_material_category, "masterData");
        assert_ne!(item.ownership_type, inbox_routing::OWNERSHIP_NEW_PROJECT);
    }

    #[test]
    fn employee_and_material_code_are_master_data() {
        for (file_name, summary) in [
            ("员工信息.xlsx", "员工编号、姓名、部门和岗位"),
            ("物料编码.xlsx", "物料编码、物料名称、规格和单位"),
        ] {
            let item = build_test_inbox_item(file_name, summary, &[summary]);
            assert_eq!(item.material_semantic_type, "masterDataOrTemplate");
            assert_eq!(item.general_material_category, "masterData");
            assert_eq!(item.ownership_type, inbox_routing::OWNERSHIP_GENERAL_WORK);
        }
    }

    #[test]
    fn ric_single_file_is_only_suspected_project_material() {
        let item = build_test_inbox_item(
            "RIC-复制零件.xlsx",
            "零件号、复制状态和备注",
            &["零件号、名称、状态"],
        );
        assert_eq!(item.ownership_type, inbox_routing::OWNERSHIP_NEEDS_REVIEW);
        assert_eq!(item.judgement_status, "project_candidate_low");
        assert!(item.project_candidate_score >= 12);
        assert!(item.project_candidate_score < 75);
        assert!(item
            .project_candidate_reasons
            .iter()
            .any(|reason| reason.contains("项目代号")));
    }

    #[test]
    fn complete_related_material_set_becomes_new_project_candidate() {
        let mut items = vec![
            build_test_inbox_item(
                "项目说明.docx",
                "项目目标、项目范围、需求和交付物",
                &["项目目标", "项目范围"],
            ),
            build_test_inbox_item(
                "计划.xlsx",
                "任务、负责人、开始时间、结束时间和里程碑",
                &["任务、负责人、计划日期"],
            ),
            build_test_inbox_item(
                "会议纪要.docx",
                "参会人员讨论项目需求和下一步任务",
                &["参会人员", "会议结论"],
            ),
        ];
        let changed_ids = items.iter().map(|item| item.id.clone()).collect();
        reassess_batch_project_candidates(&mut items, &changed_ids, &[]);

        assert!(items.iter().all(|item| {
            item.ownership_type == inbox_routing::OWNERSHIP_NEW_PROJECT
                && item.project_candidate_score >= 75
                && item.judgement_status == "project_candidate_high"
        }));
    }

    #[test]
    fn inbox_classification_and_location_create_decision_traces() {
        let item = build_test_inbox_item(
            "车间订单报表.xlsx",
            "生产订单、工序、完工数量和交期",
            &["订单号、工序、完工数量、交期"],
        );
        assert!(item
            .decision_traces
            .iter()
            .any(|trace| trace.decision_type == "fileClassification"));
        let location = item
            .decision_traces
            .iter()
            .find(|trace| trace.decision_type == "locationRecommendation")
            .expect("位置建议应有追踪记录");
        assert!(!location.input_evidence.is_empty());
        assert!(!location.recommendation.is_empty());
        assert_eq!(location.confidence.level, "high");
        assert!(location.confidence.display.starts_with("高 "));
    }

    #[test]
    fn project_match_creates_decision_trace() {
        let projects = vec![(
            ProjectSummary {
                id: "project-1".to_string(),
                name: "感冒院".to_string(),
                root_dir: r"D:\GanMaoYuan\SelfProject".to_string(),
                ..ProjectSummary::default()
            },
            ProjectManifest::default(),
        )];
        let analysis = FileAnalysis {
            file_id: new_id(),
            file_name: "感冒院项目说明.md".to_string(),
            parse_status: "success".to_string(),
            recommended_category: "需求资料".to_string(),
            document_type: "项目说明".to_string(),
            content_summary: "感冒院项目目标和范围".to_string(),
            main_fields_or_sections: vec!["项目目标".to_string()],
            ..FileAnalysis::default()
        };
        let item = build_material_inbox_item(
            &projects,
            r"D:\资料\感冒院项目说明.md",
            "trace-project-hash",
            100,
            &analysis,
            "test",
        )
        .unwrap();
        assert!(item
            .decision_traces
            .iter()
            .any(|trace| trace.decision_type == "projectMatch"));
    }

    #[test]
    fn decision_trace_records_approved_modified_rejected_and_cancelled() {
        let mut traces = vec![new_decision_trace(
            "locationRecommendation",
            "subject-1",
            "project-1",
            Vec::new(),
            "判断".to_string(),
            "建议".to_string(),
            trace_confidence(86),
        )];
        update_trace_outcome(
            &mut traces,
            &["locationRecommendation"],
            "modified",
            "pending",
            "用户修改位置。",
        );
        assert_eq!(traces[0].user_decision, "modified");

        update_trace_outcome(
            &mut traces,
            &["locationRecommendation"],
            "approved",
            "executed",
            "用户确认归位。",
        );
        assert_eq!(traces[0].user_decision, "approved");
        assert_eq!(traces[0].execution, "executed");

        update_trace_outcome(
            &mut traces,
            &["locationRecommendation"],
            "rejected",
            "cancelled",
            "用户拒绝。",
        );
        assert_eq!(traces[0].user_decision, "rejected");
        assert_eq!(traces[0].execution, "cancelled");
    }

    #[test]
    fn decision_trace_survives_json_reload_and_legacy_data_stays_readable() {
        let mut item = MaterialInboxItem {
            id: "trace-item".to_string(),
            ..MaterialInboxItem::default()
        };
        item.decision_traces.push(new_decision_trace(
            "fileClassification",
            &item.id,
            "",
            Vec::new(),
            "识别为测试资料".to_string(),
            "归入测试资料".to_string(),
            trace_confidence(80),
        ));
        let encoded = serde_json::to_string(&item).unwrap();
        let restored: MaterialInboxItem = serde_json::from_str(&encoded).unwrap();
        assert_eq!(restored.decision_traces.len(), 1);
        assert_eq!(restored.decision_traces[0].confidence.display, "高 80%");

        let legacy: MaterialInboxItem = serde_json::from_str(r#"{"id":"legacy-item"}"#).unwrap();
        assert!(legacy.decision_traces.is_empty());
    }

    #[test]
    fn matching_general_history_increases_confidence() {
        let analysis = FileAnalysis {
            file_id: new_id(),
            file_name: "VAVE申请.xlsx".to_string(),
            parse_status: "success".to_string(),
            recommended_category: "数据表格".to_string(),
            document_type: "VAVE/降本申请单".to_string(),
            business_domain: "研发/质量/降本".to_string(),
            content_summary: "降本申请。".to_string(),
            ..FileAnalysis::default()
        };
        let without_history = build_material_inbox_item(
            &[],
            r"D:\资料\VAVE申请.xlsx",
            "hash-history-1",
            512,
            &analysis,
            "test",
        )
        .unwrap();
        let history = vec![GlobalManagedFile {
            file_name: "历史VAVE申请.xlsx".to_string(),
            general_material_domain: "engineering".to_string(),
            general_material_category: "vave".to_string(),
            ..GlobalManagedFile::default()
        }];
        let with_history = build_material_inbox_item_with_history(
            &[],
            &history,
            r"D:\资料\VAVE申请.xlsx",
            "hash-history-2",
            512,
            &analysis,
            "test",
        )
        .unwrap();

        assert!(with_history.confidence_score > without_history.confidence_score);
        assert!(with_history
            .confidence_reasons
            .iter()
            .any(|reason| reason.contains("历史归档")));
    }

    #[test]
    fn text_with_project_name_matches_project() {
        let projects = vec![(
            ProjectSummary {
                id: "p1".to_string(),
                name: "感冒院".to_string(),
                ..ProjectSummary::default()
            },
            ProjectManifest::default(),
        )];
        let mut analysis = FileAnalysis::default();
        analysis.file_id = new_id();
        analysis.file_name = "需求说明.txt".to_string();
        analysis.parse_status = "success".to_string();
        analysis.recommended_category = "需求文档".to_string();
        analysis.content_summary = "感冒院工作台需求说明与验收标准".to_string();
        let item = build_material_inbox_item(
            &projects,
            r"C:\tmp\需求说明.txt",
            &format!("hash-{}", new_id()),
            2048,
            &analysis,
            "test",
        )
        .unwrap();
        assert_eq!(
            item.ownership_type,
            inbox_routing::OWNERSHIP_EXISTING_PROJECT
        );
        assert!(
            !item.project_candidates.is_empty(),
            "项目名命中应生成项目候选"
        );
    }

    #[test]
    fn parse_failed_item_never_matches_project() {
        let projects = vec![(
            ProjectSummary {
                id: "p1".to_string(),
                name: "感冒院".to_string(),
                ..ProjectSummary::default()
            },
            ProjectManifest::default(),
        )];
        let mut analysis = FileAnalysis::default();
        analysis.file_id = new_id();
        analysis.file_name = "setup.exe".to_string();
        analysis.parse_status = "failed".to_string();
        analysis.recommended_category = "待检查".to_string();
        analysis.parse_failure_reason = "暂不支持解析 .exe 文件，已进入待检查事项。".to_string();
        let item = build_material_inbox_item(
            &projects,
            r"C:\tmp\setup.exe",
            &format!("hash-{}", new_id()),
            4096,
            &analysis,
            "test",
        )
        .unwrap();
        assert_eq!(item.ownership_type, inbox_routing::OWNERSHIP_UNSUPPORTED);
        assert!(item.project_candidates.is_empty());
        assert!(item.target_project_id.is_empty());
        assert_eq!(item.confidence_score, 0);
    }

    #[test]
    fn ocr_failure_item_keeps_friendly_error_and_detail() {
        let projects: Vec<(ProjectSummary, ProjectManifest)> = Vec::new();
        let mut analysis = FileAnalysis::default();
        analysis.file_id = new_id();
        analysis.file_name = "扫描件.png".to_string();
        analysis.parse_status = "failed".to_string();
        analysis.recommended_category = "待检查".to_string();
        analysis.parse_failure_reason = "未能从文件中识别出文字，可重试或手动归类。".to_string();
        analysis.technical_detail =
            "图片 OCR 失败：Windows OCR 未识别到文字 (0x80004005)".to_string();
        let item = build_material_inbox_item(
            &projects,
            r"C:\tmp\扫描件.png",
            &format!("hash-{}", new_id()),
            512,
            &analysis,
            "test",
        )
        .unwrap();
        assert_eq!(item.ownership_type, inbox_routing::OWNERSHIP_UNSUPPORTED);
        assert!(item.project_candidates.is_empty());
        assert!(item.parse_failure_reason.contains("未能从文件中识别出文字"));
        assert!(item.technical_detail.contains("0x80004005"));
    }

    #[test]
    fn global_route_copies_and_undo_removes_copy() {
        let root = test_root("global-route");
        let source = root.join("车间订单报表.xlsx");
        fs::create_dir_all(root.join("source")).unwrap();
        fs::write(&source, b"order data columns").unwrap();
        let mut item = MaterialInboxItem::default();
        item.id = new_id();
        item.file_name = "车间订单报表.xlsx".to_string();
        item.source_path = path_to_string(&source);
        item.source_hash = hash_file(&source).unwrap();
        item.ownership_type = inbox_routing::OWNERSHIP_GENERAL_WORK.to_string();
        item.document_type = "车间生产/订单报表".to_string();
        item.business_domain = "制造/生产".to_string();
        let dest_dir = root.join("managed/general");
        fs::create_dir_all(&dest_dir).unwrap();
        let audit = root.join("managed/audit/operations.jsonl");
        fs::create_dir_all(audit.parent().unwrap()).unwrap();
        let record =
            perform_global_route(&dest_dir, &audit, &item, inbox_routing::GLOBAL_DEST_GENERAL)
                .unwrap();
        assert!(Path::new(&record.managed_path).is_file(), "受管副本应存在");
        assert!(audit.exists(), "审计日志应写入");
        assert_eq!(record.destination, inbox_routing::GLOBAL_DEST_GENERAL);
        // 原文件未被移动
        assert!(source.is_file(), "原文件不应被移动");
        // undo 删除受管副本
        let mut document = GlobalFilesDocument {
            files: vec![record.clone()],
            ..GlobalFilesDocument::default()
        };
        let undone = perform_global_undo(&audit, &mut document, &record.id).unwrap();
        assert!(!undone.undone_at.is_empty());
        assert!(
            !Path::new(&record.managed_path).exists(),
            "撤销后受管副本应删除"
        );
        assert_eq!(document.files[0].undone_at, undone.undone_at);
        assert!(source.is_file(), "撤销不应影响原文件");
    }

    #[test]
    fn general_material_route_records_semantic_nested_location() {
        let root = test_root("global-semantic-route");
        let source = root.join("瑞尔-VAVE申请单.xlsx");
        fs::create_dir_all(&root).unwrap();
        fs::write(&source, b"vave application").unwrap();
        let mut item = MaterialInboxItem::default();
        item.id = new_id();
        item.file_name = "瑞尔-VAVE申请单.xlsx".to_string();
        item.source_path = path_to_string(&source);
        item.source_hash = hash_file(&source).unwrap();
        item.ownership_type = inbox_routing::OWNERSHIP_GENERAL_WORK.to_string();
        item.general_material_domain = "engineering".to_string();
        item.general_material_category = "vave".to_string();
        let dest_dir = root.join("managed/general/engineering/vave");
        fs::create_dir_all(&dest_dir).unwrap();
        let audit = root.join("managed/audit/operations.jsonl");
        fs::create_dir_all(audit.parent().unwrap()).unwrap();

        let record =
            perform_global_route(&dest_dir, &audit, &item, inbox_routing::GLOBAL_DEST_GENERAL)
                .unwrap();

        assert_eq!(record.general_material_domain, "engineering");
        assert_eq!(record.general_material_category, "vave");
        assert_eq!(
            record.managed_relative_path,
            "engineering/vave/瑞尔-VAVE申请单.xlsx"
        );
        assert_eq!(
            Path::new(&record.managed_path).parent(),
            Some(dest_dir.as_path())
        );
        assert!(source.is_file(), "原文件不应被移动或删除");
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn global_undo_rejects_modified_copy() {
        let root = test_root("global-undo-conflict");
        let source = root.join("data.txt");
        fs::create_dir_all(root.join("source")).unwrap();
        fs::write(&source, b"original").unwrap();
        let mut item = MaterialInboxItem::default();
        item.id = new_id();
        item.file_name = "data.txt".to_string();
        item.source_path = path_to_string(&source);
        item.source_hash = hash_file(&source).unwrap();
        item.ownership_type = inbox_routing::OWNERSHIP_GENERAL_WORK.to_string();
        let dest_dir = root.join("managed/general");
        fs::create_dir_all(&dest_dir).unwrap();
        let audit = root.join("managed/audit/operations.jsonl");
        fs::create_dir_all(audit.parent().unwrap()).unwrap();
        let record =
            perform_global_route(&dest_dir, &audit, &item, inbox_routing::GLOBAL_DEST_GENERAL)
                .unwrap();
        // 修改受管副本
        fs::write(&record.managed_path, b"tampered").unwrap();
        let mut document = GlobalFilesDocument {
            files: vec![record.clone()],
            ..GlobalFilesDocument::default()
        };
        let result = perform_global_undo(&audit, &mut document, &record.id);
        assert!(result.is_err(), "受管副本被修改后不应自动删除");
        assert!(
            Path::new(&record.managed_path).is_file(),
            "冲突时副本应保留"
        );
    }

    #[test]
    fn garbled_recovery_point_is_excluded_from_today() {
        let mut manifest = ProjectManifest::default();
        manifest.project.id = "health-recovery".to_string();
        manifest.project.next_step = "\u{fffd}\u{fffd}\u{fffd}".to_string();
        manifest.recovery_points.push(RecoveryPoint {
            id: "bad-rp".to_string(),
            completed: "\u{fffd}\u{fffd}".to_string(),
            next_step: "\u{fffd}\u{fffd}\u{fffd}".to_string(),
            ..RecoveryPoint::default()
        });
        update_daily_continue_snapshot(&mut manifest);
        let snapshot = manifest.daily_continue_snapshots.last().unwrap();
        assert!(snapshot.last_progress.is_empty());
        assert!(snapshot.recommended_actions.is_empty());
        assert!(manifest.data_health_records.iter().any(|record| {
            record.health_type == "suspect" && record.source == "recoveryPoint:bad-rp"
        }));
    }

    #[test]
    fn executable_is_excluded_from_project_analysis_context() {
        let mut manifest = ProjectManifest::default();
        manifest.project.id = "health-file".to_string();
        manifest.files.push(FileRecord {
            id: "binary".to_string(),
            file_name: "app.exe".to_string(),
            file_type: "exe".to_string(),
            managed_path: r"D:\project\.ganmaoyuan\managed\build\app.exe".to_string(),
            content_summary: "binary application".to_string(),
            ..FileRecord::default()
        });
        update_data_health_records(&mut manifest);
        let context = build_project_analysis_context(&manifest)
            .unwrap()
            .to_string();
        assert!(!context.contains("app.exe"));
    }

    #[test]
    fn low_quality_task_does_not_create_today_action() {
        let mut manifest = ProjectManifest::default();
        manifest.project.id = "health-task".to_string();
        manifest.tasks.push(TaskRecord {
            id: "bad-task".to_string(),
            title: "这是项目的背景说明，包含很多上下文但没有明确动作，也不应直接成为任务标题。"
                .repeat(4),
            status: "active".to_string(),
            ..TaskRecord::default()
        });
        update_daily_continue_snapshot(&mut manifest);
        assert!(manifest
            .daily_continue_snapshots
            .last()
            .unwrap()
            .recommended_actions
            .is_empty());
    }

    #[test]
    fn ai_context_with_only_project_label_refuses_to_guess() {
        let mut manifest = ProjectManifest::default();
        manifest.project.id = "thin-project".to_string();
        manifest.project.name = "只有名称".to_string();
        manifest.project.description = "简单项目说明".to_string();
        manifest.messages.push(WorkspaceMessage {
            id: "assistant-pending".to_string(),
            author: "ganmaoyuan".to_string(),
            kind: "assistant".to_string(),
            evidence_items: vec![MessageEvidenceItem {
                id: "project-evidence".to_string(),
                title: "项目说明".to_string(),
                content_type: "project".to_string(),
                basis_kind: "fact".to_string(),
                summary: "简单项目说明".to_string(),
                source_record_id: "thin-project".to_string(),
                ..MessageEvidenceItem::default()
            }],
            ..WorkspaceMessage::default()
        });
        let context = build_deepseek_context(&manifest, "assistant-pending", "deepseek-chat")
            .unwrap()
            .to_string();
        assert!(context.contains("当前项目资料不足，无法确认"));
        assert!(context.contains("不得猜测或补全背景"));
    }

    #[test]
    fn ai_context_includes_explicit_reference_file_excerpt() {
        let root = setup_project("ai-reference-context");
        let file_path = root
            .join(".ganmaoyuan")
            .join("managed")
            .join("02_requirements")
            .join("WTS_Production_Migration_Manifest.md");
        fs::create_dir_all(file_path.parent().unwrap()).unwrap();
        fs::write(
            &file_path,
            "# WTS Production Migration Manifest\n\n用于指导 IT/DBA 按顺序执行数据库结构初始化和迁移。\n排除业务数据导入和生产数据改写。\n",
        )
        .unwrap();

        let mut manifest = ProjectManifest::default();
        manifest.project.id = "reference-project".to_string();
        manifest.project.name = "WTS".to_string();
        manifest.project.root_dir = path_to_string(&root);
        manifest.files.push(FileRecord {
            id: "reference-file".to_string(),
            file_name: "WTS_Production_Migration_Manifest.md".to_string(),
            managed_path: path_to_string(&file_path),
            file_type: "md".to_string(),
            parse_status: "success".to_string(),
            content_summary: "WTS Production Migration Manifest，数据库迁移执行清单。".to_string(),
            still_exists: true,
            ..FileRecord::default()
        });
        manifest.messages.push(WorkspaceMessage {
            id: "question".to_string(),
            author: "user".to_string(),
            text: "刚才加入的 WTS_Production_Migration_Manifest.md 主要是做什么的？".to_string(),
            ..WorkspaceMessage::default()
        });
        manifest.messages.push(WorkspaceMessage {
            id: "assistant".to_string(),
            author: "ganmaoyuan".to_string(),
            ..WorkspaceMessage::default()
        });

        let context = build_deepseek_context(&manifest, "assistant", "deepseek-chat")
            .unwrap()
            .to_string();

        assert!(context.contains("WTS_Production_Migration_Manifest.md"));
        assert!(context.contains("数据库结构初始化和迁移"));
        assert!(context.contains("排除业务数据导入"));
        assert!(!context.contains(&path_to_string(&root)));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn chat_image_attachment_persists_without_becoming_project_material() {
        let root = setup_project("chat-image-persist");
        let input = ChatImageAttachmentInput {
            file_name: "screen.png".to_string(),
            content_type: "image/png".to_string(),
            data_url: "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=".to_string(),
        };
        let attachments =
            persist_chat_image_attachments(&root, "message-image", vec![input.clone()]).unwrap();
        assert_eq!(attachments.len(), 1);
        assert_eq!(attachments[0].attachment_type, "image");
        assert_eq!(attachments[0].content_type, "image/png");
        assert!(attachments[0].managed_path.is_empty());
        assert!(root.join(&attachments[0].relative_path).is_file());

        let loaded = read_chat_attachment_data(&root, &attachments[0].relative_path).unwrap();
        assert_eq!(loaded.content_type, "image/png");
        assert_eq!(loaded.data_url, input.data_url);

        let mut manifest = ProjectManifest::default();
        record_message_derivatives(
            &mut manifest,
            &WorkspaceMessage {
                id: "message-image".to_string(),
                author: "user".to_string(),
                kind: "requirement".to_string(),
                attachments,
                ..WorkspaceMessage::default()
            },
        );
        assert!(manifest.artifacts.is_empty());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn chat_image_context_uses_current_user_attachment_only_for_vision_models() {
        let root = setup_project("chat-image-context");
        let attachments = persist_chat_image_attachments(
            &root,
            "question",
            vec![ChatImageAttachmentInput {
                file_name: "screen.png".to_string(),
                content_type: "image/png".to_string(),
                data_url: "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=".to_string(),
            }],
        )
        .unwrap();
        let mut manifest = ProjectManifest::default();
        manifest.project.root_dir = path_to_string(&root);
        manifest.project.name = "图片上下文测试".to_string();
        manifest.messages.push(WorkspaceMessage {
            id: "question".to_string(),
            author: "user".to_string(),
            text: "请看这张截图".to_string(),
            attachments,
            ..WorkspaceMessage::default()
        });
        manifest.messages.push(WorkspaceMessage {
            id: "assistant".to_string(),
            author: "ganmaoyuan".to_string(),
            ..WorkspaceMessage::default()
        });

        let vision_context = build_deepseek_context(&manifest, "assistant", "deepseek-flash")
            .unwrap()
            .to_string();
        assert!(vision_context.contains("image_url"));
        assert!(vision_context.contains("data:image/png;base64"));

        let text_context = build_deepseek_context(&manifest, "assistant", "deepseek-chat")
            .unwrap()
            .to_string();
        assert!(!text_context.contains("image_url"));
        assert!(text_context.contains("不支持视觉输入"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn data_health_status_persists_and_legacy_manifest_loads() {
        let root = setup_project("data-health-persist");
        let mut manifest = read_and_repair_manifest(&root).unwrap();
        manifest.recovery_points.push(RecoveryPoint {
            id: "legacy-bad".to_string(),
            completed: "\u{fffd}\u{fffd}".to_string(),
            next_step: String::new(),
            ..RecoveryPoint::default()
        });
        update_data_health_records(&mut manifest);
        let id = manifest.data_health_records[0].id.clone();
        manifest.data_health_records[0].status = "ignored".to_string();
        persist_project(&root, &manifest, None).unwrap();
        let loaded = read_and_repair_manifest(&root).unwrap();
        assert_eq!(
            loaded
                .data_health_records
                .iter()
                .find(|record| record.id == id)
                .unwrap()
                .status,
            "ignored"
        );
        let legacy: ProjectManifest = serde_json::from_value(serde_json::json!({
            "project": {"id":"legacy","name":"旧项目","rootDir":"","manifestPath":"","lastOpenedAt":"","nextStep":""}
        }))
        .unwrap();
        assert!(legacy.data_health_records.is_empty());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn read_repair_replaces_garbled_display_text_without_deleting_history() {
        let root = setup_project("repair-garbled-display");
        let mut manifest = read_and_repair_manifest(&root).unwrap();
        manifest.project.next_step = "\u{fffd}\u{fffd}\u{fffd}".to_string();
        manifest.pending_reviews.push(PendingReviewItem {
            id: "review-bad".to_string(),
            key: "new-file|bad".to_string(),
            kind: "new_file".to_string(),
            title: "\u{fffd}\u{fffd}\u{fffd}".to_string(),
            detail: "\u{fffd}\u{fffd}\u{fffd}".to_string(),
            path: path_to_string(&root.join("新增资料.txt")),
            status: "open".to_string(),
            ..PendingReviewItem::default()
        });
        manifest.recovery_points.push(RecoveryPoint {
            id: "rp-bad".to_string(),
            completed: "\u{fffd}\u{fffd}".to_string(),
            next_step: "\u{fffd}\u{fffd}\u{fffd}".to_string(),
            ..RecoveryPoint::default()
        });
        persist_project(&root, &manifest, None).unwrap();

        let loaded = read_and_repair_manifest(&root).unwrap();

        assert!(!looks_garbled_or_meaningless(&loaded.project.next_step));
        assert!(!looks_garbled_or_meaningless(
            &loaded.pending_reviews[0].title
        ));
        assert!(!looks_garbled_or_meaningless(
            &loaded.pending_reviews[0].detail
        ));
        assert!(loaded
            .data_health_records
            .iter()
            .any(|record| record.source == "recoveryPoint:rp-bad"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn v1_foundation_generates_atlas_skill_and_library_outputs() {
        let root = setup_project("v1-foundation");
        let mut manifest = read_and_repair_manifest(&root).unwrap();
        manifest.project.name = "感冒院".to_string();
        manifest.daily_sessions.push(DailySession {
            id: "day-1".to_string(),
            date_key: "2026-08-18".to_string(),
            ..DailySession::default()
        });
        manifest.daily_sessions.push(DailySession {
            id: "day-2".to_string(),
            date_key: "2026-08-19".to_string(),
            ..DailySession::default()
        });
        manifest.recovery_points.push(RecoveryPoint {
            id: "rp-1".to_string(),
            completed: "整理文件入口与位置归位".to_string(),
            next_step: "继续用恢复点推进".to_string(),
            ..RecoveryPoint::default()
        });
        manifest.recovery_points.push(RecoveryPoint {
            id: "rp-2".to_string(),
            completed: "导入 Codex 报告并验收".to_string(),
            next_step: "继续用恢复点推进".to_string(),
            ..RecoveryPoint::default()
        });

        update_daily_continue_snapshot(&mut manifest);
        update_v1_foundation(&root, &mut manifest).unwrap();

        assert!(!manifest.work_pattern_candidates.is_empty());
        assert!(!manifest.skill_library.is_empty());
        assert_eq!(manifest.skill_library[0].status, "candidate");
        assert!(!manifest.atlas_skill_assessments.is_empty());
        assert!(manifest.atlas_skill_assessments[0].review_required);
        assert!(PathBuf::from(&manifest.v1_readiness.usage_guide_path).exists());
        assert!(manifest
            .v1_readiness
            .capability_overview
            .iter()
            .any(|item| item.contains("Decision Trace")));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn v1_foundation_discovers_knowledge_and_improvement_candidates() {
        let root = setup_project("v1-patterns");
        let mut manifest = read_and_repair_manifest(&root).unwrap();
        manifest.project.id = "self".to_string();
        manifest.codex_prompts.push(CodexPromptRecord {
            id: "prompt-1".to_string(),
            title: "交给 Codex".to_string(),
            ..CodexPromptRecord::default()
        });
        manifest.codex_prompts.push(CodexPromptRecord {
            id: "prompt-2".to_string(),
            title: "交给 Codex".to_string(),
            ..CodexPromptRecord::default()
        });
        manifest.codex_reports.push(CodexReportRecord {
            id: "report-1".to_string(),
            summary: "完成文件入口验收".to_string(),
            ..CodexReportRecord::default()
        });
        manifest.codex_reports.push(CodexReportRecord {
            id: "report-2".to_string(),
            summary: "完成搜索验收".to_string(),
            ..CodexReportRecord::default()
        });
        manifest.data_health_records.push(DataHealthRecord {
            id: "health-1".to_string(),
            health_type: "suspect".to_string(),
            severity: "medium".to_string(),
            source: "task:a".to_string(),
            description: "低质量任务".to_string(),
            suggested_action: "人工处理".to_string(),
            status: "open".to_string(),
            created_at: now_string(),
        });
        manifest.data_health_records.push(DataHealthRecord {
            id: "health-2".to_string(),
            health_type: "notSuitableForAIContext".to_string(),
            severity: "medium".to_string(),
            source: "file:b".to_string(),
            description: "构建产物".to_string(),
            suggested_action: "排除上下文".to_string(),
            status: "open".to_string(),
            created_at: now_string(),
        });

        update_v1_foundation(&root, &mut manifest).unwrap();

        assert!(manifest
            .knowledge_pattern_candidates
            .iter()
            .any(|item| item.pattern_name == "Codex 执行闭环"));
        assert!(manifest
            .improvement_candidates
            .iter()
            .any(|item| item.problem.contains("历史数据质量")));
        assert!(manifest
            .v1_readiness
            .current_limitations
            .iter()
            .any(|item| item.contains("云同步")));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn workspace_config_builds_standard_directories() {
        let root = test_root("workspace-config-shape");
        let config = build_workspace_config(root.clone(), "missing".to_string());

        assert_eq!(config.schema_version, WORKSPACE_CONFIG_SCHEMA_VERSION);
        assert_eq!(config.workspace_root, path_to_string(&root));
        assert_eq!(config.inbox_root, path_to_string(&root.join("00_Inbox")));
        assert_eq!(
            config.projects_root,
            path_to_string(&root.join("10_Projects"))
        );
        assert_eq!(
            config.general_root,
            path_to_string(&root.join("20_General"))
        );
        assert_eq!(
            config.temporary_root,
            path_to_string(&root.join("30_Temporary"))
        );
        assert_eq!(
            config.archive_root,
            path_to_string(&root.join("40_Archive"))
        );
        assert_eq!(
            config.system_root,
            path_to_string(&root.join(".ganmaoyuan"))
        );
        assert_eq!(config.status, "missing");
    }

    #[test]
    fn workspace_config_status_detects_missing_and_ready() {
        let root = test_root("workspace-config-status");
        let config = build_workspace_config(root.clone(), "missing".to_string());
        assert_eq!(
            verify_workspace_config_status(config.clone()).status,
            "missing"
        );

        for directory in workspace_standard_dirs(&config) {
            fs::create_dir_all(directory).unwrap();
        }

        let verified = verify_workspace_config_status(config);
        assert_eq!(verified.status, "ready");
        assert!(!verified.last_verified_at.is_empty());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn workspace_initialization_does_not_touch_legacy_project_manifest() {
        let root = setup_project("workspace-legacy-project");
        let before = fs::read_to_string(manifest_path(&root)).unwrap();
        let workspace_root = test_root("workspace-side-by-side");
        let config = build_workspace_config(workspace_root.clone(), "missing".to_string());
        for directory in workspace_standard_dirs(&config) {
            fs::create_dir_all(directory).unwrap();
        }

        let after = fs::read_to_string(manifest_path(&root)).unwrap();
        assert_eq!(before, after);
        assert!(workspace_root.join("00_Inbox").is_dir());
        fs::remove_dir_all(root).unwrap();
        fs::remove_dir_all(workspace_root).unwrap();
    }

    #[test]
    fn work_ledger_dedupes_source_ref_and_event_type() {
        let root = setup_project("work-ledger-dedupe");
        let first = append_work_event_if_new(
            &root,
            fact_event(
                "p-test",
                "git",
                "abc123",
                "git.headChanged",
                "Git HEAD 更新。".to_string(),
                vec!["repo:test".to_string()],
                98,
            ),
        )
        .unwrap();
        let duplicate = append_work_event_if_new(
            &root,
            fact_event(
                "p-test",
                "git",
                "abc123",
                "git.headChanged",
                "Git HEAD 更新。".to_string(),
                vec!["repo:test".to_string()],
                98,
            ),
        )
        .unwrap();

        assert!(first.is_some());
        assert!(duplicate.is_none());
        assert_eq!(read_work_ledger(&root).unwrap().events.len(), 1);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn git_snapshot_records_commit_and_dirty_without_completion_claim() {
        let root = setup_project("git-observer-ledger");
        let repo = test_root("git-observer-repo");
        fs::create_dir_all(&repo).unwrap();
        assert!(std::process::Command::new("git")
            .arg("-C")
            .arg(&repo)
            .arg("init")
            .status()
            .unwrap()
            .success());
        for args in [
            vec!["config", "user.email", "test@example.local"],
            vec!["config", "user.name", "Ganmaoyuan Test"],
        ] {
            assert!(std::process::Command::new("git")
                .arg("-C")
                .arg(&repo)
                .args(args)
                .status()
                .unwrap()
                .success());
        }
        fs::write(repo.join("README.md"), "initial").unwrap();
        assert!(std::process::Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(["add", "README.md"])
            .status()
            .unwrap()
            .success());
        assert!(std::process::Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(["commit", "-m", "initial"])
            .status()
            .unwrap()
            .success());

        let clean = capture_git_snapshot("p-test", &repo).unwrap();
        record_git_snapshot_events(&root, "p-test", None, &clean).unwrap();
        assert!(!clean.head.is_empty());
        assert!(!clean.recent_commits.is_empty());
        fs::write(repo.join("README.md"), "dirty").unwrap();
        let dirty = capture_git_snapshot("p-test", &repo).unwrap();
        record_git_snapshot_events(&root, "p-test", Some(&clean), &dirty).unwrap();
        let ledger = read_work_ledger(&root).unwrap();

        assert!(ledger
            .events
            .iter()
            .any(|event| event.event_type == "git.headChanged"));
        let dirty_event = ledger
            .events
            .iter()
            .find(|event| event.event_type == "git.workingTreeDirty")
            .expect("dirty event");
        assert!(dirty_event.summary.contains("未提交改动"));
        assert!(!dirty_event.summary.contains("完成"));
        fs::remove_dir_all(root).unwrap();
        fs::remove_dir_all(repo).unwrap();
    }

    #[test]
    fn codex_report_import_writes_external_result_and_is_idempotent() {
        let root = setup_project("codex-ledger");
        let report = "完成：实现 Work Ledger\n测试：cargo test 通过\ncommit: abc1234\n遗留问题：需要人工验收";

        let first = import_codex_report_text(path_to_string(&root), report.to_string()).unwrap();
        let second = import_codex_report_text(path_to_string(&root), report.to_string()).unwrap();
        let ledger = read_work_ledger(&root).unwrap();
        let results = read_codex_external_results(&root).unwrap();

        assert!(!first.duplicate);
        assert!(second.duplicate);
        assert_eq!(results.results.len(), 1);
        assert_eq!(
            ledger
                .events
                .iter()
                .filter(|event| event.event_type == "codex.reportImported")
                .count(),
            1
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn codex_prompt_creates_bridge_task_id_and_result_directory() {
        let root = setup_project("codex-bridge-prompt");

        let result = generate_codex_prompt(path_to_string(&root)).unwrap();

        assert!(!result.prompt.id.is_empty());
        assert!(codex_result_bridge_dir(&root).exists());
        assert!(result.prompt.prompt_text.contains(&format!(
            ".ganmaoyuan/codex/results/{}-result.json",
            result.prompt.id
        )));
        assert!(result
            .prompt
            .prompt_text
            .contains(&codex_result_bridge_dir(&root).to_string_lossy().to_string()));
        assert!(result
            .prompt
            .prompt_text
            .contains("resultSource 写为 \"codex\""));
        assert!(result.prompt.prompt_text.contains("taskId"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn explicit_codex_task_keeps_prompt_scope_isolated() {
        let root = setup_project("explicit-codex-task");
        let task = create_codex_task(
            path_to_string(&root),
            CodexTaskCreateRequest {
                title: "验证结果桥接".to_string(),
                task_type: "fileOperation".to_string(),
                instructions: "仅创建 docs/proof.md，内容为 OK。不要修改其它文件。".to_string(),
            },
        )
        .unwrap();

        assert_eq!(task.status, "ready");
        assert_eq!(task.task_type, "fileOperation");
        assert!(task.prompt.contains("docs/proof.md"));
        assert!(task.prompt.contains(&task.task_id));
        assert!(task.prompt.contains(&task.expected_result_path));
        assert!(!task.prompt.contains("D:\\Atlas"));
        assert_eq!(read_codex_task(&root, &task.task_id).unwrap(), task);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn explicit_coding_task_persists_type_for_reload_and_runner() {
        let root = setup_project("explicit-coding-task-type");
        let task = create_codex_task(
            path_to_string(&root),
            CodexTaskCreateRequest {
                title: "优化设置后台右侧内容区留白".to_string(),
                task_type: "coding".to_string(),
                instructions: "只调整右侧内容区的 padding 和 section spacing，不修改其它功能。"
                    .to_string(),
            },
        )
        .unwrap();
        let reloaded = read_codex_task(&root, &task.task_id).unwrap();
        let run_prompt = codex_prompt_for_run(
            &reloaded,
            &CodexRun {
                id: "run-coding-type".to_string(),
                git_head_before: "abc123".to_string(),
                git_dirty_files_before: vec![GitChangedFile {
                    status: "M".to_string(),
                    path: "unrelated.md".to_string(),
                }],
                ..CodexRun::default()
            },
        );

        assert_eq!(task.task_type, "coding");
        assert_eq!(reloaded.task_type, "coding");
        assert!(reloaded.prompt.contains("taskType：coding"));
        assert!(run_prompt.contains("taskType：coding"));
        assert!(codex_task_git_required(&reloaded));
        assert!(run_prompt.contains("git add -- <本任务文件路径>"));
        assert!(run_prompt.contains("禁止 `git add .`"));
        assert!(run_prompt.contains("unrelated.md"));
        assert!(run_prompt.contains("gitHead"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn consecutive_codex_tasks_keep_prompt_and_result_paths_isolated() {
        let root = setup_project("codex-task-prompt-isolation");

        let first = generate_codex_prompt(path_to_string(&root)).unwrap();
        let second = generate_codex_prompt(path_to_string(&root)).unwrap();
        let first_task = read_codex_task(&root, &first.prompt.id).unwrap();
        let second_task = read_codex_task(&root, &second.prompt.id).unwrap();

        assert_ne!(first_task.task_id, second_task.task_id);
        assert_eq!(
            second_task.prompt.lines().next().unwrap(),
            format!("taskId：{}", second_task.task_id)
        );
        assert!(second_task
            .prompt
            .contains(&format!("{}-result.json", second_task.task_id)));
        assert!(second_task
            .prompt
            .contains(&second_task.expected_result_path));
        assert!(!second_task.prompt.contains(&first_task.task_id));
        assert!(second_task
            .expected_result_path
            .contains(&format!("{}-result.json", second_task.task_id)));
        validate_codex_task_prompt_binding(&first_task).unwrap();
        validate_codex_task_prompt_binding(&second_task).unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn codex_task_start_validation_rejects_stale_prompt_binding() {
        let task = CodexTask {
            task_id: "task-current".to_string(),
            prompt: "taskId：task-old\n请写入 .ganmaoyuan/codex/results/task-old-result.json"
                .to_string(),
            expected_result_path:
                "D:\\project\\.ganmaoyuan\\codex\\results\\task-current-result.json".to_string(),
            ..CodexTask::default()
        };

        let error = validate_codex_task_prompt_binding(&task).unwrap_err();
        assert!(error.contains("taskId 不一致"));
    }

    #[test]
    fn failed_codex_task_can_rebind_same_task_id_and_reload() {
        let root = setup_project("codex-task-rebind");
        let generated = generate_codex_prompt(path_to_string(&root)).unwrap();
        let task_id = generated.prompt.id;
        let mut task = read_codex_task(&root, &task_id).unwrap();
        task.status = "failed".to_string();
        task.prompt = "taskId：stale-task\n旧结果 stale-task-result.json".to_string();
        write_codex_task(&root, &task).unwrap();

        let rebound = rebind_codex_task_prompt(path_to_string(&root), task_id.clone()).unwrap();
        let reloaded = read_codex_task(&root, &task_id).unwrap();

        assert_eq!(rebound.task_id, task_id);
        assert_eq!(rebound.status, "ready");
        assert_eq!(reloaded, rebound);
        validate_codex_task_prompt_binding(&reloaded).unwrap();
        assert!(reloaded
            .expected_result_path
            .contains(&format!("{}-result.json", reloaded.task_id)));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn codex_result_bridge_imports_json_once_into_work_ledger() {
        let root = setup_project("codex-bridge-json");
        ensure_project_dirs(&root).unwrap();
        let result_path = codex_result_bridge_dir(&root).join("task-123-result.json");
        fs::write(
            &result_path,
            r#"{
  "taskId": "task-123",
  "status": "completed",
  "summary": "完成 Codex Result Bridge v1",
  "changedFiles": ["src/app/AppState.tsx", "src-tauri/src/project_service.rs"],
  "commits": ["abc1234"],
  "tests": ["cargo test 通过", "npm run build 通过"],
  "remainingIssues": ["需要桌面人工验收"],
  "manualAcceptance": ["确认自动入账"],
  "completedAt": "2026-08-25T12:00:00Z"
}"#,
        )
        .unwrap();

        let first = scan_codex_result_bridge(path_to_string(&root)).unwrap();
        let second = scan_codex_result_bridge(path_to_string(&root)).unwrap();
        let ledger = read_work_ledger(&root).unwrap();
        let results = read_codex_external_results(&root).unwrap();

        assert_eq!(first.scanned_count, 1);
        assert_eq!(first.imported_count, 1);
        assert_eq!(second.imported_count, 0);
        assert_eq!(results.results.len(), 1);
        assert_eq!(results.results[0].task_id, "task-123");
        assert!(results.results[0]
            .changed_files
            .iter()
            .any(|file| file.contains("AppState.tsx")));
        assert_eq!(
            ledger
                .events
                .iter()
                .filter(|event| event.event_type == "codex.reportImported")
                .count(),
            1
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn codex_result_bridge_preserves_active_written_result_over_fallback() {
        let root = setup_project("codex-bridge-active-result");
        ensure_project_dirs(&root).unwrap();
        let task = CodexTask {
            task_id: "task-active".to_string(),
            project_id: "project-1".to_string(),
            status: "handedOff".to_string(),
            expected_result_path: path_to_string(
                &codex_result_bridge_dir(&root).join("task-active-result.json"),
            ),
            ..CodexTask::default()
        };
        write_codex_task(&root, &task).unwrap();
        upsert_codex_run(
            &root,
            CodexRun {
                id: "run-active".to_string(),
                task_id: task.task_id.clone(),
                project_id: task.project_id.clone(),
                status: "exited".to_string(),
                exit_code: Some(0),
                ended_at: now_string(),
                ..CodexRun::default()
            },
        )
        .unwrap();
        fs::write(
            codex_result_bridge_dir(&root).join("task-active-result.json"),
            r#"{
  "taskId": "task-active",
  "status": "completed",
  "summary": "Codex 主动写入结果",
  "changedFiles": ["src/pages/WorkPage.tsx"],
  "commits": [],
  "tests": ["npm test 通过"],
  "remainingIssues": [],
  "manualAcceptance": [],
  "completedAt": "2026-08-26T00:00:00Z",
  "resultSource": "codex"
}"#,
        )
        .unwrap();

        let result = scan_codex_result_bridge(path_to_string(&root)).unwrap();
        let external = read_codex_external_results(&root).unwrap();

        assert_eq!(result.imported_count, 1);
        assert_eq!(external.results[0].task_id, "task-active");
        assert_ne!(external.results[0].result_source, "runnerFallback");
        assert!(external.results[0]
            .changed_files
            .iter()
            .any(|file| file.contains("WorkPage.tsx")));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn exited_codex_run_without_result_does_not_create_fallback_during_scan() {
        let root = setup_project("codex-run-fallback");
        ensure_project_dirs(&root).unwrap();
        let task = CodexTask {
            task_id: "task-fallback".to_string(),
            project_id: "project-1".to_string(),
            status: "handedOff".to_string(),
            expected_result_path: path_to_string(
                &codex_result_bridge_dir(&root).join("task-fallback-result.json"),
            ),
            ..CodexTask::default()
        };
        let last_message = codex_run_output_path(&root, "run-fallback");
        write_codex_task(&root, &task).unwrap();
        write_text_atomic(&last_message, "我已经完成检查，但没有写入 result 文件。").unwrap();
        upsert_codex_run(
            &root,
            CodexRun {
                id: "run-fallback".to_string(),
                task_id: task.task_id.clone(),
                project_id: task.project_id.clone(),
                status: "exited".to_string(),
                exit_code: Some(0),
                output_last_message_path: path_to_string(&last_message),
                ended_at: now_string(),
                ..CodexRun::default()
            },
        )
        .unwrap();

        let first = scan_codex_result_bridge(path_to_string(&root)).unwrap();
        let second = scan_codex_result_bridge(path_to_string(&root)).unwrap();
        let fallback_path = codex_result_bridge_dir(&root).join("task-fallback-result.json");
        let external = read_codex_external_results(&root).unwrap();
        let task = read_codex_task(&root, "task-fallback").unwrap();

        assert!(!fallback_path.exists());
        assert_eq!(first.imported_count, 0);
        assert_eq!(second.imported_count, 0);
        assert!(external.results.is_empty());
        assert_eq!(task.status, "awaitingResult");
        refresh_codex_task_git_verifications(&root, "").unwrap();
        assert_eq!(
            read_codex_task(&root, "task-fallback").unwrap().status,
            "awaitingResult"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn missing_last_message_does_not_create_fake_result_during_scan() {
        let root = setup_project("codex-run-fallback-missing-message");
        ensure_project_dirs(&root).unwrap();
        let task = CodexTask {
            task_id: "task-no-message".to_string(),
            project_id: "project-1".to_string(),
            status: "handedOff".to_string(),
            expected_result_path: path_to_string(
                &codex_result_bridge_dir(&root).join("task-no-message-result.json"),
            ),
            ..CodexTask::default()
        };
        write_codex_task(&root, &task).unwrap();
        upsert_codex_run(
            &root,
            CodexRun {
                id: "run-no-message".to_string(),
                task_id: task.task_id.clone(),
                project_id: task.project_id.clone(),
                status: "exited".to_string(),
                exit_code: Some(0),
                output_last_message_path: path_to_string(&codex_run_output_path(
                    &root,
                    "run-no-message",
                )),
                ended_at: now_string(),
                ..CodexRun::default()
            },
        )
        .unwrap();

        let result = scan_codex_result_bridge(path_to_string(&root)).unwrap();
        let external = read_codex_external_results(&root).unwrap();
        let task = read_codex_task(&root, "task-no-message").unwrap();

        assert_eq!(result.imported_count, 0);
        assert!(external.results.is_empty());
        assert_eq!(task.status, "awaitingResult");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn codex_result_bridge_imports_markdown_result() {
        let root = setup_project("codex-bridge-md");
        ensure_project_dirs(&root).unwrap();
        fs::write(
            codex_result_bridge_dir(&root).join("task-md-result.md"),
            "taskId：task-md\n完成：实现自动结果扫描\n测试：npm test 通过\ncommit: def5678\n遗留问题：等待人工验收",
        )
        .unwrap();

        let result = scan_codex_result_bridge(path_to_string(&root)).unwrap();
        let results = read_codex_external_results(&root).unwrap();

        assert_eq!(result.imported_count, 1);
        assert_eq!(results.results[0].task_id, "task-md");
        assert_eq!(results.results[0].commits, vec!["def5678".to_string()]);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn list_codex_tasks_scans_existing_result_and_advances_task() {
        let root = setup_project("codex-list-scans-result");
        let prompt = generate_codex_prompt(path_to_string(&root)).unwrap();
        let task_id = prompt.prompt.id.clone();
        let mut task = read_codex_task(&root, &task_id).unwrap();
        task.status = "running".to_string();
        write_codex_task(&root, &task).unwrap();
        fs::write(
            codex_result_bridge_dir(&root).join(format!("{task_id}-result.json")),
            format!(
                r#"{{
  "taskId": "{task_id}",
  "taskType": "analysis",
  "status": "completed",
  "summary": "完成安全分析任务",
  "resultText": "真实结果文件已经写入。",
  "resultSource": "codex",
  "remainingIssues": [],
  "manualAcceptance": [],
  "completedAt": "2026-08-27T09:00:00+08:00"
}}"#
            ),
        )
        .unwrap();

        let list = list_codex_tasks(path_to_string(&root)).unwrap();
        let advanced = list
            .tasks
            .iter()
            .find(|item| item.task_id == task_id)
            .unwrap();

        assert_eq!(advanced.status, "completed");
        assert_eq!(advanced.result_source, "codex");
        assert!(!advanced.result_id.is_empty());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn restarted_codex_process_waits_for_result_without_fallback() {
        let root = setup_project("codex-process-lost");
        let task_id = "lost-task";
        write_codex_task(
            &root,
            &CodexTask {
                task_id: task_id.to_string(),
                project_id: "p1".to_string(),
                title: "失联任务".to_string(),
                task_type: "analysis".to_string(),
                status: "running".to_string(),
                prompt: format!("taskId：{task_id}"),
                expected_result_path: path_to_string(
                    &codex_result_bridge_dir(&root).join(format!("{task_id}-result.json")),
                ),
                updated_at: now_string(),
                ..CodexTask::default()
            },
        )
        .unwrap();
        upsert_codex_run(
            &root,
            CodexRun {
                id: "run-lost".to_string(),
                task_id: task_id.to_string(),
                project_id: "p1".to_string(),
                status: "running".to_string(),
                started_at: "1".to_string(),
                pid: 0,
                ..CodexRun::default()
            },
        )
        .unwrap();

        list_codex_tasks(path_to_string(&root)).unwrap();

        let task = read_codex_task(&root, task_id).unwrap();
        let run = read_codex_run(&root, "run-lost").unwrap();
        assert_eq!(task.status, "awaitingResult");
        assert_eq!(run.status, "unknownAfterRestart");
        assert!(run.error.contains("未检测到"));
        assert!(!codex_result_bridge_dir(&root)
            .join(format!("{task_id}-result.json"))
            .exists());

        write_analysis_bridge_result(&root, task_id, &[]);
        list_codex_tasks(path_to_string(&root)).unwrap();
        let recovered = read_codex_task(&root, task_id).unwrap();
        assert_eq!(recovered.status, "completed");
        assert_eq!(recovered.result_source, "codex");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn restarted_codex_process_needs_review_only_after_recovery_window() {
        let root = setup_project("codex-process-recovery-expired");
        let task_id = "expired-task";
        write_codex_task(
            &root,
            &CodexTask {
                task_id: task_id.to_string(),
                project_id: "p1".to_string(),
                title: "过期恢复任务".to_string(),
                task_type: "analysis".to_string(),
                status: "awaitingResult".to_string(),
                prompt: format!("taskId：{task_id}"),
                expected_result_path: path_to_string(
                    &codex_result_bridge_dir(&root).join(format!("{task_id}-result.json")),
                ),
                updated_at: now_string(),
                ..CodexTask::default()
            },
        )
        .unwrap();
        upsert_codex_run(
            &root,
            CodexRun {
                id: "run-expired".to_string(),
                task_id: task_id.to_string(),
                project_id: "p1".to_string(),
                status: "unknownAfterRestart".to_string(),
                started_at: "1".to_string(),
                ended_at: "1".to_string(),
                pid: 0,
                ..CodexRun::default()
            },
        )
        .unwrap();

        sync_failed_codex_runs_to_tasks(&root).unwrap();

        let task = read_codex_task(&root, task_id).unwrap();
        assert_eq!(task.status, "needsReview");
        assert!(task
            .remaining_issues
            .iter()
            .any(|item| item.contains("恢复窗口已结束")));
        assert!(!codex_result_bridge_dir(&root)
            .join(format!("{task_id}-result.json"))
            .exists());
        fs::remove_dir_all(root).unwrap();
    }

    fn write_analysis_bridge_result(root: &Path, task_id: &str, acceptance: &[&str]) {
        write_json_atomic(
            &codex_result_bridge_dir(root).join(format!("{task_id}-result.json")),
            &json!({
                "taskId": task_id,
                "taskType": "analysis",
                "status": "completed",
                "summary": "完成项目分析",
                "resultText": "完成分析并提出下一步建议，等待用户检查。",
                "findings": ["当前项目资料已整理"],
                "manualAcceptance": acceptance,
                "remainingIssues": [],
                "resultSource": "codex"
            }),
        )
        .unwrap();
    }

    fn wait_for_live_codex_result(root: &Path, task_id: &str) -> (CodexTask, CodexRun) {
        let run = start_codex_task_run(path_to_string(root), task_id.to_string()).unwrap();
        assert_eq!(run.status, "starting");
        let deadline = std::time::Instant::now() + Duration::from_secs(240);
        loop {
            let current = read_codex_run(root, &run.id).unwrap();
            if !matches!(current.status.as_str(), "starting" | "running") {
                assert!(
                    current.exit_code == Some(0)
                        || current.error.contains("已收到本次真实 Codex 结果"),
                    "CLI failed; inspect the isolated CodexRun record"
                );
                break;
            }
            if std::time::Instant::now() >= deadline {
                cancel_codex_task_run(path_to_string(root), task_id.to_string()).unwrap();
                panic!("Live test exceeded four minutes; only its child process was cancelled");
            }
            std::thread::sleep(Duration::from_secs(2));
        }
        let task = (0..30)
            .find_map(|_| {
                let current = read_codex_task(root, task_id).unwrap();
                if !current.result_received_at.is_empty() && current.result_run_id == run.id {
                    Some(current)
                } else {
                    std::thread::sleep(Duration::from_millis(500));
                    None
                }
            })
            .expect("Runner did not automatically import the Codex-written result");
        (task, read_codex_run(root, &run.id).unwrap())
    }

    #[test]
    #[ignore = "Opt-in: runs the installed Codex CLI using local authentication"]
    fn live_codex_cli_writes_result_and_bridge_receives_it() {
        let root = setup_project("codex-live-handoff");
        let repo = root.join("repository");
        fs::create_dir_all(&repo).unwrap();
        assert!(hidden_command("git")
            .arg("-C")
            .arg(&repo)
            .arg("init")
            .output()
            .unwrap()
            .status
            .success());
        let id = generate_codex_prompt(path_to_string(&root))
            .unwrap()
            .prompt
            .id;
        let mut task = read_codex_task(&root, &id).unwrap();
        task.task_type = "fileOperation".to_string();
        task.repository_path = path_to_string(&repo);
        let proof_path = repo.join("codex-live-runner-proof.md");
        task.prompt = format!(
            "taskId: {id}\nThis is an isolated file-operation and result-handoff smoke test. Do not read any other project, memory, network, or user files. Do not change Git. Create this UTF-8 text file with exactly `CODEX_LIVE_RUNNER_OK`: {}. Then write UTF-8 JSON to this absolute path: {}\nJSON must contain taskId={id}, taskType=fileOperation, status=completed, summary='Isolated runner handoff verified', resultText='The isolated test file and result file were written by Codex.', changedFiles=['codex-live-runner-proof.md'], artifacts=['codex-live-runner-proof.md'], targetFiles=['codex-live-runner-proof.md'], commits=[], tests=[], remainingIssues=[], manualAcceptance=['Confirm the isolated runner file'], resultSource=codex, completedAt=actual current time. Use a dedicated file-writing tool, then finish immediately. Do not merely print the JSON. Do not use a fallback. Final reply: Handoff file written.",
            path_to_string(&proof_path), task.expected_result_path
        );
        write_codex_task(&root, &task).unwrap();
        assert_eq!(task.status, "ready");
        let mut previous_result = String::new();
        for attempt in 0..2 {
            if attempt > 0 {
                reject_codex_task(
                    path_to_string(&root),
                    id.clone(),
                    previous_result.clone(),
                    Some("Isolated retry verification".to_string()),
                )
                .unwrap();
            }
            let run = start_codex_task_run(path_to_string(&root), id.clone()).unwrap();
            assert_eq!(run.status, "starting");
            eprintln!(
                "Live handoff started; isolated artifacts: {}",
                root.display()
            );
            let deadline = std::time::Instant::now() + Duration::from_secs(900);
            loop {
                let current = read_codex_run(&root, &run.id).unwrap();
                if !matches!(current.status.as_str(), "starting" | "running") {
                    assert!(
                        current.exit_code == Some(0)
                            || current.error.contains("已收到本次真实 Codex 结果"),
                        "CLI failed; inspect isolated run record"
                    );
                    break;
                }
                if std::time::Instant::now() >= deadline {
                    cancel_codex_task_run(path_to_string(&root), id.clone()).unwrap();
                    panic!("Live test exceeded 15 minutes; only its child process was cancelled");
                }
                std::thread::sleep(Duration::from_secs(2));
            }
            let received = (0..30)
                .find_map(|_| {
                    let current = read_codex_task(&root, &id).unwrap();
                    if !current.result_received_at.is_empty() && current.result_run_id == run.id {
                        Some(current)
                    } else {
                        std::thread::sleep(Duration::from_millis(500));
                        None
                    }
                })
                .expect("Runner did not automatically import the result");
            let result: serde_json::Value =
                read_json(Path::new(&task.expected_result_path)).unwrap();
            assert_eq!(result["taskId"], id);
            assert_eq!(result["resultSource"], "codex");
            assert_eq!(result["resultRunId"], run.id);
            assert_eq!(
                fs::read_to_string(&proof_path).unwrap(),
                "CODEX_LIVE_RUNNER_OK\n"
            );
            assert_eq!(received.result_source, "codex");
            assert_eq!(received.result_run_id, run.id);
            assert_eq!(received.status, "awaitingAcceptance");
            assert_ne!(received.result_id, previous_result);
            previous_result = received.result_id.clone();
            assert!(received.reported_commits.is_empty());
            assert!(received.tests.is_empty());
            assert!(Path::new(&run.output_last_message_path).is_file());
            let listed = list_codex_tasks(path_to_string(&root)).unwrap();
            assert_eq!(
                listed
                    .tasks
                    .iter()
                    .find(|item| item.task_id == id)
                    .unwrap()
                    .result_id,
                received.result_id
            );
            assert_eq!(
                scan_codex_result_bridge(path_to_string(&root))
                    .unwrap()
                    .imported_count,
                0
            );
            eprintln!("PASS attempt {}: running -> Codex-written run-bound result -> resultReceived -> awaitingAcceptance; fallback excluded", attempt + 1);
        }
        assert_eq!(
            read_and_repair_manifest(&root).unwrap().codex_reports.len(),
            2
        );
        let reports = read_and_repair_manifest(&root).unwrap().codex_reports;
        let evidence_runs = reports
            .iter()
            .map(|report| codex_result_contract_from_report(report).result_run_id)
            .collect::<HashSet<_>>();
        assert_eq!(evidence_runs.len(), 2);
        assert!(!evidence_runs.contains(""));
        assert_ne!(
            reports[0].evidence_managed_path,
            reports[1].evidence_managed_path
        );
        let received = read_codex_task(&root, &id).unwrap();
        let accepted =
            accept_codex_task(path_to_string(&root), id.clone(), received.result_id).unwrap();
        assert_eq!(accepted.status, "completed");
        assert!(list_codex_tasks(path_to_string(&root))
            .unwrap()
            .tasks
            .iter()
            .any(|task| task.task_id == id && task.status == "completed"));
        eprintln!("PASS: result-bound backend acceptance persists after reload; isolated test, not user desktop acceptance");
    }

    #[test]
    #[ignore = "Opt-in: runs a real, isolated Codex analysis task using local authentication"]
    fn live_codex_cli_analysis_uses_cmd_and_returns_a_structured_result() {
        let root = setup_project("codex-live-analysis");
        let repo = root.join("repository");
        fs::create_dir_all(&repo).unwrap();
        assert!(hidden_command("git")
            .arg("-C")
            .arg(&repo)
            .arg("init")
            .output()
            .unwrap()
            .status
            .success());
        fs::write(
            repo.join("package.json"),
            r#"{"name":"isolated-analysis","version":"1.2.3"}"#,
        )
        .unwrap();
        let id = generate_codex_prompt(path_to_string(&root))
            .unwrap()
            .prompt
            .id;
        let mut task = read_codex_task(&root, &id).unwrap();
        task.task_type = "analysis".to_string();
        task.repository_path = path_to_string(&repo);
        task.prompt = format!(
            "taskId: {id}\nThis is an isolated read-only analysis and result-handoff test. Read only {} and report its JSON name and version. Do not create, modify, delete, stage, or commit any repository file. Do not access any other project, memory, network, or user files. Then write UTF-8 JSON to {} with taskId={id}, taskType=analysis, status=completed, summary='Isolated analysis completed', resultText='The package metadata was read in the isolated repository.', findings=['package name isolated-analysis', 'version 1.2.3'], recommendations=['No repository changes are needed'], questions=[], changedFiles=[], commits=[], tests=[], remainingIssues=[], manualAcceptance=[], resultSource=codex, completedAt=actual current time. Use cmd.exe for the read command as required by the execution constraints. Do not merely print the JSON. Final reply: Analysis result file written.",
            path_to_string(&repo.join("package.json")),
            task.expected_result_path
        );
        write_codex_task(&root, &task).unwrap();

        let (received, run) = wait_for_live_codex_result(&root, &id);
        let result: serde_json::Value = read_json(Path::new(&task.expected_result_path)).unwrap();
        assert_eq!(run.status, "exited");
        assert_eq!(result["taskId"], id);
        assert_eq!(result["taskType"], "analysis");
        assert_eq!(result["resultSource"], "codex");
        assert_eq!(result["resultRunId"], run.id);
        assert!(result["findings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item
                .as_str()
                .unwrap_or_default()
                .contains("isolated-analysis")));
        assert_eq!(received.result_source, "codex");
        assert_eq!(received.status, "completed");
        assert_eq!(received.git_verification.status, "notApplicable");
    }

    #[test]
    #[ignore = "Opt-in: runs a real, isolated Codex coding task using local authentication"]
    fn live_codex_cli_coding_task_commits_and_is_git_verified() {
        let root = setup_project("codex-live-coding");
        let repo = root.join("repository");
        fs::create_dir_all(&repo).unwrap();
        assert!(hidden_command("git")
            .arg("-C")
            .arg(&repo)
            .args(["init", "-b", "main"])
            .output()
            .unwrap()
            .status
            .success());
        assert!(hidden_command("git")
            .arg("-C")
            .arg(&repo)
            .args(["config", "user.email", "codex-live@example.invalid"])
            .output()
            .unwrap()
            .status
            .success());
        assert!(hidden_command("git")
            .arg("-C")
            .arg(&repo)
            .args(["config", "user.name", "Codex Live Test"])
            .output()
            .unwrap()
            .status
            .success());
        fs::write(repo.join("README.md"), "# Isolated Codex coding test\n").unwrap();
        assert!(hidden_command("git")
            .arg("-C")
            .arg(&repo)
            .args(["add", "README.md"])
            .output()
            .unwrap()
            .status
            .success());
        assert!(hidden_command("git")
            .arg("-C")
            .arg(&repo)
            .args(["commit", "-m", "test: initialize isolated coding task"])
            .output()
            .unwrap()
            .status
            .success());
        let id = generate_codex_prompt(path_to_string(&root))
            .unwrap()
            .prompt
            .id;
        let mut task = read_codex_task(&root, &id).unwrap();
        task.task_type = "coding".to_string();
        task.repository_path = path_to_string(&repo);
        let proof_path = repo.join("codex-live-coding-proof.md");
        task.prompt = format!(
            "taskId: {id}\nThis is an isolated coding and Git-verification test. In this isolated repository only, create {} with exactly `CODEX_LIVE_CODING_OK` followed by one newline. Use cmd.exe for every Git or test command as required by the execution constraints. Run `git add codex-live-coding-proof.md`, commit with message `test: verify Codex coding bridge`, then obtain the full commit SHA with `git rev-parse HEAD`. Do not access any other project, memory, network, or user files. Then write UTF-8 JSON to {} with taskId={id}, taskType=coding, status=completed, summary='Isolated coding task completed', resultText='The proof file was created and committed in the isolated repository.', changedFiles=['codex-live-coding-proof.md'], commits=[the full SHA from git rev-parse HEAD], tests=['git status --porcelain returned clean'], remainingIssues=[], manualAcceptance=['Confirm the isolated coding proof file'], resultSource=codex, completedAt=actual current time. Do not merely print the JSON. Final reply: Coding result file written.",
            path_to_string(&proof_path),
            task.expected_result_path
        );
        write_codex_task(&root, &task).unwrap();

        let (received, run) = wait_for_live_codex_result(&root, &id);
        let result: serde_json::Value = read_json(Path::new(&task.expected_result_path)).unwrap();
        assert_eq!(run.status, "exited");
        assert_eq!(result["taskId"], id);
        assert_eq!(result["taskType"], "coding");
        assert_eq!(result["resultSource"], "codex");
        assert_eq!(result["resultRunId"], run.id);
        assert_eq!(
            fs::read_to_string(&proof_path).unwrap(),
            "CODEX_LIVE_CODING_OK\n"
        );
        assert_eq!(received.result_source, "codex");
        assert_eq!(received.status, "awaitingAcceptance");
        assert_eq!(received.git_verification.status, "verified");
        assert_eq!(received.reported_commits.len(), 1);
        let accepted = accept_codex_task(path_to_string(&root), id, received.result_id).unwrap();
        assert_eq!(accepted.status, "completed");
    }

    #[test]
    fn codex_result_rescan_preserves_acceptance_and_task_timestamp() {
        let root = setup_project("codex-rescan-acceptance");
        let prompt = generate_codex_prompt(path_to_string(&root)).unwrap();
        let id = prompt.prompt.id;
        write_analysis_bridge_result(&root, &id, &["确认分析结论"]);
        scan_codex_result_bridge(path_to_string(&root)).unwrap();
        let accepted = accept_codex_task(
            path_to_string(&root),
            id.clone(),
            read_codex_task(&root, &id).unwrap().result_id,
        )
        .unwrap();
        let before = fs::read(codex_tasks_dir(&root).join(format!("{id}.json"))).unwrap();
        let events_before = read_work_ledger(&root).unwrap().events.len();

        for _ in 0..2 {
            assert_eq!(
                scan_codex_result_bridge(path_to_string(&root))
                    .unwrap()
                    .imported_count,
                0
            );
            let current = read_codex_task(&root, &id).unwrap();
            assert_eq!(current.status, "completed");
            assert_eq!(current.updated_at, accepted.updated_at);
            assert_eq!(current.acceptance.status, "approved");
        }
        assert_eq!(
            fs::read(codex_tasks_dir(&root).join(format!("{id}.json"))).unwrap(),
            before
        );
        assert_eq!(read_work_ledger(&root).unwrap().events.len(), events_before);
        let mut legacy = read_codex_task(&root, &id).unwrap();
        legacy.status = "awaitingAcceptance".to_string();
        write_codex_task(&root, &legacy).unwrap();
        scan_codex_result_bridge(path_to_string(&root)).unwrap();
        assert_eq!(read_codex_task(&root, &id).unwrap().status, "completed");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn codex_needs_review_can_use_the_canonical_acceptance_path() {
        let root = setup_project("codex-needs-review-acceptance");
        let prompt = generate_codex_prompt(path_to_string(&root)).unwrap();
        let id = prompt.prompt.id;
        write_analysis_bridge_result(&root, &id, &["确认分析结论"]);
        scan_codex_result_bridge(path_to_string(&root)).unwrap();

        let mut review_task = read_codex_task(&root, &id).unwrap();
        assert_eq!(review_task.status, "awaitingAcceptance");
        review_task.status = "needsReview".to_string();
        write_codex_task(&root, &review_task).unwrap();

        let accepted =
            accept_codex_task(path_to_string(&root), id.clone(), review_task.result_id).unwrap();
        assert_eq!(accepted.status, "completed");
        assert_eq!(accepted.acceptance.status, "approved");
        assert!(read_work_ledger(&root)
            .unwrap()
            .events
            .iter()
            .any(|event| event.event_type == "codex.taskAccepted"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn codex_exit_after_result_does_not_restore_waiting_state() {
        let root = setup_project("codex-result-before-exit");
        let id = generate_codex_prompt(path_to_string(&root))
            .unwrap()
            .prompt
            .id;
        for status in [
            "needsReview",
            "failed",
            "cancelled",
            "verifying",
            "resultReceived",
            "awaitingAcceptance",
            "completed",
        ] {
            let mut task = read_codex_task(&root, &id).unwrap();
            task.status = status.to_string();
            write_codex_task(&root, &task).unwrap();
            let before = fs::read(codex_tasks_dir(&root).join(format!("{id}.json"))).unwrap();
            mark_codex_task_awaiting_result(&root, &id).unwrap();
            assert_eq!(read_codex_task(&root, &id).unwrap().status, status);
            assert_eq!(
                fs::read(codex_tasks_dir(&root).join(format!("{id}.json"))).unwrap(),
                before
            );
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn codex_exit_waits_for_result_transaction() {
        let root = setup_project("codex-result-exit-race");
        let id = generate_codex_prompt(path_to_string(&root))
            .unwrap()
            .prompt
            .id;
        let mut task = read_codex_task(&root, &id).unwrap();
        task.status = "running".to_string();
        write_codex_task(&root, &task).unwrap();

        let guard = project_write_lock().lock().unwrap();
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let callback_root = root.clone();
        let callback_id = id.clone();
        let callback = std::thread::spawn(move || {
            started_tx.send(()).unwrap();
            let outcome = mark_codex_task_awaiting_result(&callback_root, &callback_id);
            done_tx.send(outcome).unwrap();
        });
        started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let early = done_rx.recv_timeout(Duration::from_millis(100));
        // Simulate the result transaction completing while the exit callback waits.
        task.status = "resultReceived".to_string();
        task.result_id = "received-result".to_string();
        write_codex_task(&root, &task).unwrap();
        drop(guard);
        callback.join().unwrap();
        assert!(matches!(
            early,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout)
        ));
        done_rx
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .unwrap();
        assert_eq!(
            read_codex_task(&root, &id).unwrap().status,
            "resultReceived"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn codex_review_requires_current_project_result_and_pending_state() {
        let root = setup_project("codex-review-gate");
        let id = generate_codex_prompt(path_to_string(&root))
            .unwrap()
            .prompt
            .id;
        assert!(accept_codex_task(path_to_string(&root), id.clone(), String::new()).is_err());
        write_analysis_bridge_result(&root, &id, &["检查结论"]);
        scan_codex_result_bridge(path_to_string(&root)).unwrap();
        let original = read_codex_task(&root, &id).unwrap();
        for status in [
            "ready",
            "running",
            "awaitingResult",
            "failed",
            "cancelled",
            "completed",
        ] {
            let mut task = original.clone();
            task.status = status.to_string();
            write_codex_task(&root, &task).unwrap();
            assert!(
                accept_codex_task(path_to_string(&root), id.clone(), task.result_id.clone())
                    .is_err(),
                "{status}"
            );
            assert_eq!(read_codex_task(&root, &id).unwrap().status, status);
        }
        let mut task = original.clone();
        task.project_id = "other-project".to_string();
        write_codex_task(&root, &task).unwrap();
        assert!(
            accept_codex_task(path_to_string(&root), id.clone(), task.result_id)
                .unwrap_err()
                .contains("不属于")
        );
        write_codex_task(&root, &original).unwrap();
        assert!(accept_codex_task(
            path_to_string(&root),
            id.clone(),
            "stale-result".to_string()
        )
        .is_err());
        assert!(!read_work_ledger(&root)
            .unwrap()
            .events
            .iter()
            .any(|event| event.event_type == "codex.taskAccepted"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn codex_review_rejects_missing_or_changed_evidence() {
        let root = setup_project("codex-review-evidence");
        let id = generate_codex_prompt(path_to_string(&root))
            .unwrap()
            .prompt
            .id;
        write_analysis_bridge_result(&root, &id, &["检查结论"]);
        scan_codex_result_bridge(path_to_string(&root)).unwrap();
        let task = read_codex_task(&root, &id).unwrap();
        let manifest = read_and_repair_manifest(&root).unwrap();
        let report = manifest
            .codex_reports
            .iter()
            .find(|report| report.id == task.result_id)
            .unwrap();
        let path = PathBuf::from(&report.evidence_managed_path);
        let original = fs::read(&path).unwrap();
        fs::remove_file(&path).unwrap();
        assert!(
            accept_codex_task(path_to_string(&root), id.clone(), task.result_id.clone())
                .unwrap_err()
                .contains("无法读取")
        );
        write_analysis_bridge_result(&root, &id, &["不同的验收事项"]);
        fs::write(&path, fs::read(&task.expected_result_path).unwrap()).unwrap();
        assert!(
            accept_codex_task(path_to_string(&root), id.clone(), task.result_id.clone())
                .unwrap_err()
                .contains("已变化")
        );
        fs::write(path, original).unwrap();
        assert_eq!(
            read_codex_task(&root, &id).unwrap().status,
            "awaitingAcceptance"
        );
        assert_eq!(
            accept_codex_task(path_to_string(&root), id, task.result_id)
                .unwrap()
                .status,
            "completed"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn codex_review_is_result_bound_idempotent_and_persistent() {
        let root = setup_project("codex-review-idempotency");
        let id = generate_codex_prompt(path_to_string(&root))
            .unwrap()
            .prompt
            .id;
        write_analysis_bridge_result(&root, &id, &["结论一"]);
        scan_codex_result_bridge(path_to_string(&root)).unwrap();
        let a = read_codex_task(&root, &id).unwrap().result_id;
        let rejected = reject_codex_task(
            path_to_string(&root),
            id.clone(),
            a.clone(),
            Some("补充证据".to_string()),
        )
        .unwrap();
        assert_eq!(
            reject_codex_task(path_to_string(&root), id.clone(), a.clone(), None)
                .unwrap()
                .updated_at,
            rejected.updated_at
        );
        write_analysis_bridge_result(&root, &id, &["结论二"]);
        scan_codex_result_bridge(path_to_string(&root)).unwrap();
        let b = read_codex_task(&root, &id).unwrap().result_id;
        assert_ne!(a, b);
        assert!(accept_codex_task(path_to_string(&root), id.clone(), a.clone()).is_err());
        assert!(reject_codex_task(path_to_string(&root), id.clone(), a, None).is_err());
        let workers = (0..2)
            .map(|_| {
                let (root, id, b) = (root.clone(), id.clone(), b.clone());
                std::thread::spawn(move || accept_codex_task(path_to_string(&root), id, b).unwrap())
            })
            .collect::<Vec<_>>();
        let results = workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(results[0].updated_at, results[1].updated_at);
        let listed = list_codex_tasks(path_to_string(&root)).unwrap();
        assert_eq!(
            listed
                .tasks
                .iter()
                .find(|task| task.task_id == id)
                .unwrap()
                .status,
            "completed"
        );
        let events = read_work_ledger(&root).unwrap().events;
        assert_eq!(
            events
                .iter()
                .filter(|event| event.event_type == "codex.taskAccepted")
                .count(),
            1
        );
        assert_eq!(
            events
                .iter()
                .filter(|event| event.event_type == "codex.taskRejected")
                .count(),
            1
        );
        assert!(events
            .iter()
            .any(|event| event.event_type == "codex.taskAccepted"
                && event.evidence_refs.contains(&format!("codexReport:{b}"))));
        write_analysis_bridge_result(&root, &id, &["结论三"]);
        scan_codex_result_bridge(path_to_string(&root)).unwrap();
        let c = read_codex_task(&root, &id).unwrap().result_id;
        accept_codex_task(path_to_string(&root), id.clone(), c).unwrap();
        assert_eq!(
            read_work_ledger(&root)
                .unwrap()
                .events
                .iter()
                .filter(|event| event.event_type == "codex.taskAccepted")
                .count(),
            2
        );
        assert!(start_codex_task_run(path_to_string(&root), id)
            .unwrap_err()
            .contains("不允许启动"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn codex_review_rechecks_git_and_rejects_fallback_or_failed_contract() {
        for mode in ["git", "fallback", "blocked"] {
            let root = setup_project(&format!("codex-review-{mode}"));
            let id = generate_codex_prompt(path_to_string(&root))
                .unwrap()
                .prompt
                .id;
            write_analysis_bridge_result(&root, &id, &["人工检查"]);
            let path = codex_result_bridge_dir(&root).join(format!("{id}-result.json"));
            let mut value: serde_json::Value = read_json(&path).unwrap();
            if mode == "git" {
                value["taskType"] = json!("coding");
                value["commits"] = json!(["deadbeef"]);
            }
            if mode == "fallback" {
                value["resultSource"] = json!("runnerFallback");
            }
            if mode == "blocked" {
                value["status"] = json!("blocked");
            }
            write_json_atomic(&path, &value).unwrap();
            scan_codex_result_bridge(path_to_string(&root)).unwrap();
            let mut legacy = read_codex_task(&root, &id).unwrap();
            assert_ne!(legacy.status, "awaitingAcceptance");
            legacy.status = "awaitingAcceptance".to_string();
            legacy.git_verification.status = "verified".to_string();
            write_codex_task(&root, &legacy).unwrap();
            assert!(
                accept_codex_task(path_to_string(&root), id, legacy.result_id).is_err(),
                "{mode}"
            );
            fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn codex_review_rejects_active_and_other_runs() {
        let root = setup_project("codex-review-run-binding");
        let id = generate_codex_prompt(path_to_string(&root))
            .unwrap()
            .prompt
            .id;
        write_analysis_bridge_result(&root, &id, &["人工检查"]);
        let path = codex_result_bridge_dir(&root).join(format!("{id}-result.json"));
        let mut value: serde_json::Value = read_json(&path).unwrap();
        value["resultRunId"] = json!("current");
        write_json_atomic(&path, &value).unwrap();
        scan_codex_result_bridge(path_to_string(&root)).unwrap();
        let task = read_codex_task(&root, &id).unwrap();
        for (run_id, status, exit_code) in [
            ("current", "running", None),
            ("current", "failed", Some(1)),
            ("other", "exited", Some(0)),
        ] {
            write_codex_runs(
                &root,
                vec![CodexRun {
                    id: run_id.to_string(),
                    task_id: id.clone(),
                    status: status.to_string(),
                    exit_code,
                    ..CodexRun::default()
                }],
            )
            .unwrap();
            assert!(
                accept_codex_task(path_to_string(&root), id.clone(), task.result_id.clone())
                    .is_err()
            );
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn codex_review_accepts_result_after_handoff_cleanup_exit() {
        let root = setup_project("codex-review-handoff-cleanup");
        let id = generate_codex_prompt(path_to_string(&root))
            .unwrap()
            .prompt
            .id;
        write_analysis_bridge_result(&root, &id, &["人工检查"]);
        let path = codex_result_bridge_dir(&root).join(format!("{id}-result.json"));
        let mut value: serde_json::Value = read_json(&path).unwrap();
        value["resultRunId"] = json!("handoff");
        write_json_atomic(&path, &value).unwrap();
        scan_codex_result_bridge(path_to_string(&root)).unwrap();
        let task = read_codex_task(&root, &id).unwrap();
        upsert_codex_run(
            &root,
            CodexRun {
                id: "handoff".to_string(),
                task_id: id.clone(),
                status: "exited".to_string(),
                exit_code: Some(1),
                error:
                    "已收到本次真实 Codex 结果；Codex CLI 未在宽限期内自行退出，已停止残留进程。"
                        .to_string(),
                ..CodexRun::default()
            },
        )
        .unwrap();

        let accepted =
            accept_codex_task(path_to_string(&root), id.clone(), task.result_id).unwrap();
        assert_eq!(accepted.status, "completed");
        assert!(get_work_ledger(path_to_string(&root))
            .unwrap()
            .events
            .iter()
            .any(|event| event.event_type == "codex.taskAccepted"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn codex_review_failed_ledger_write_restores_pending_task() {
        let root = setup_project("codex-review-ledger-failure");
        let id = generate_codex_prompt(path_to_string(&root))
            .unwrap()
            .prompt
            .id;
        write_analysis_bridge_result(&root, &id, &["人工检查"]);
        scan_codex_result_bridge(path_to_string(&root)).unwrap();
        let task = read_codex_task(&root, &id).unwrap();
        let original = fs::read(codex_task_path(&root, &id)).unwrap();
        let ledger = work_ledger_path(&root);
        fs::rename(&ledger, root.join("saved-ledger.json")).unwrap();
        fs::create_dir(&ledger).unwrap();
        assert!(
            accept_codex_task(path_to_string(&root), id.clone(), task.result_id)
                .unwrap_err()
                .contains("恢复待验收")
        );
        assert_eq!(fs::read(codex_task_path(&root, &id)).unwrap(), original);
        fs::write(codex_task_path(&root, &id), "corrupt-test-json").unwrap();
        assert_eq!(
            read_codex_task(&root, &id).unwrap().status,
            "awaitingAcceptance"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn concurrent_work_events_preserve_git_and_acceptance_facts() {
        let root = setup_project("ledger-concurrent-append");
        let project_id = read_and_repair_manifest(&root).unwrap().project.id;
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
        let workers = (0..8)
            .map(|index| {
                let (root, project_id, barrier) =
                    (root.clone(), project_id.clone(), barrier.clone());
                std::thread::spawn(move || {
                    let event = fact_event(
                        &project_id,
                        if index % 2 == 0 { "git" } else { "codex" },
                        &format!("source-{index}"),
                        if index % 2 == 0 {
                            "git.headChanged"
                        } else {
                            "codex.taskAccepted"
                        },
                        "Isolated concurrent fact".to_string(),
                        vec![],
                        100,
                    );
                    barrier.wait();
                    assert!(append_work_event_if_new(&root, event.clone())
                        .unwrap()
                        .is_some());
                    assert!(append_work_event_if_new(&root, event).unwrap().is_none());
                })
            })
            .collect::<Vec<_>>();
        for worker in workers {
            worker.join().unwrap();
        }
        let events = read_work_ledger(&root).unwrap().events;
        assert_eq!(events.len(), 8);
        assert_eq!(
            events
                .iter()
                .filter(|event| event.event_type == "codex.taskAccepted")
                .count(),
            4
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn codex_handoff_write_error_is_visible_and_needs_review() {
        let root = setup_project("codex-handoff-write-error");
        let id = generate_codex_prompt(path_to_string(&root))
            .unwrap()
            .prompt
            .id;
        let mut task = read_codex_task(&root, &id).unwrap();
        task.status = "running".to_string();
        let blocked_parent = root.join("not-a-directory");
        fs::write(&blocked_parent, "preserve").unwrap();
        task.expected_result_path =
            path_to_string(&blocked_parent.join(format!("{id}-result.json")));
        write_codex_task(&root, &task).unwrap();
        let run = CodexRun {
            id: "write-failed".to_string(),
            task_id: id.clone(),
            status: "exited".to_string(),
            exit_code: Some(0),
            ..CodexRun::default()
        };
        upsert_codex_run(&root, run.clone()).unwrap();
        let error = finish_codex_result_handoff(&root, &run).unwrap_err();
        let task = read_codex_task(&root, &id).unwrap();
        assert_eq!(task.status, "needsReview");
        assert!(task.remaining_issues.contains(&error));
        assert!(error.contains("结果回流失败"));
        assert_eq!(fs::read_to_string(blocked_parent).unwrap(), "preserve");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn codex_git_refresh_preserves_completed_analysis() {
        let root = setup_project("codex-analysis-git-refresh");
        let id = generate_codex_prompt(path_to_string(&root))
            .unwrap()
            .prompt
            .id;
        write_analysis_bridge_result(&root, &id, &[]);
        scan_codex_result_bridge(path_to_string(&root)).unwrap();
        assert_eq!(read_codex_task(&root, &id).unwrap().status, "completed");
        refresh_codex_task_git_verifications(&root, "").unwrap();
        let task = read_codex_task(&root, &id).unwrap();
        assert_eq!(task.git_verification.status, "notApplicable");
        assert_eq!(task.status, "completed");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn codex_retry_is_not_overwritten_by_previously_imported_result() {
        let root = setup_project("codex-retry-result-replay");
        let id = generate_codex_prompt(path_to_string(&root))
            .unwrap()
            .prompt
            .id;
        write_analysis_bridge_result(&root, &id, &[]);
        scan_codex_result_bridge(path_to_string(&root)).unwrap();
        let mut task = read_codex_task(&root, &id).unwrap();
        task.status = "running".to_string();
        write_codex_task(&root, &task).unwrap();
        scan_codex_result_bridge(path_to_string(&root)).unwrap();
        assert_eq!(read_codex_task(&root, &id).unwrap().status, "running");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn codex_rejected_acceptance_survives_rescan_and_git_refresh() {
        let root = setup_project("codex-rejected-rescan");
        let id = generate_codex_prompt(path_to_string(&root))
            .unwrap()
            .prompt
            .id;
        write_analysis_bridge_result(&root, &id, &["确认结论"]);
        scan_codex_result_bridge(path_to_string(&root)).unwrap();
        reject_codex_task(
            path_to_string(&root),
            id.clone(),
            read_codex_task(&root, &id).unwrap().result_id,
            Some("结论不准确".to_string()),
        )
        .unwrap();
        scan_codex_result_bridge(path_to_string(&root)).unwrap();
        refresh_codex_task_git_verifications(&root, "").unwrap();
        let task = read_codex_task(&root, &id).unwrap();
        assert_eq!(task.status, "failed");
        assert_eq!(task.acceptance.status, "rejected");
        assert!(task.remaining_issues.contains(&"结论不准确".to_string()));
        let mut legacy = task;
        legacy.status = "awaitingAcceptance".to_string();
        write_codex_task(&root, &legacy).unwrap();
        scan_codex_result_bridge(path_to_string(&root)).unwrap();
        assert_eq!(read_codex_task(&root, &id).unwrap().status, "failed");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn codex_blocked_result_does_not_become_completed_or_awaiting_acceptance() {
        let root = setup_project("codex-blocked-contract");
        let id = generate_codex_prompt(path_to_string(&root))
            .unwrap()
            .prompt
            .id;
        write_analysis_bridge_result(&root, &id, &["环境恢复后重试"]);
        let path = codex_result_bridge_dir(&root).join(format!("{id}-result.json"));
        let mut value: serde_json::Value = read_json(&path).unwrap();
        value["status"] = json!("blocked");
        write_json_atomic(&path, &value).unwrap();
        scan_codex_result_bridge(path_to_string(&root)).unwrap();
        refresh_codex_task_git_verifications(&root, "").unwrap();
        assert_eq!(read_codex_task(&root, &id).unwrap().status, "failed");
        let mut legacy = read_codex_task(&root, &id).unwrap();
        legacy.status = "awaitingAcceptance".to_string();
        write_codex_task(&root, &legacy).unwrap();
        scan_codex_result_bridge(path_to_string(&root)).unwrap();
        assert_eq!(read_codex_task(&root, &id).unwrap().status, "failed");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn codex_old_run_result_and_recovery_cannot_replace_live_retry() {
        let root = setup_project("codex-old-run-result");
        let id = generate_codex_prompt(path_to_string(&root))
            .unwrap()
            .prompt
            .id;
        let mut task = read_codex_task(&root, &id).unwrap();
        task.status = "running".to_string();
        write_codex_task(&root, &task).unwrap();
        for (run_id, started_at, pid) in [("old", "1", 0), ("new", "2", std::process::id())] {
            upsert_codex_run(
                &root,
                CodexRun {
                    id: run_id.to_string(),
                    task_id: id.clone(),
                    status: "running".to_string(),
                    started_at: started_at.to_string(),
                    pid,
                    ..CodexRun::default()
                },
            )
            .unwrap();
        }
        recover_stale_codex_runs(&root).unwrap();
        assert_eq!(read_codex_task(&root, &id).unwrap().status, "running");
        write_analysis_bridge_result(&root, &id, &[]);
        let path = codex_result_bridge_dir(&root).join(format!("{id}-result.json"));
        let mut value: serde_json::Value = read_json(&path).unwrap();
        value["resultRunId"] = json!("old");
        write_json_atomic(&path, &value).unwrap();
        scan_codex_result_bridge(path_to_string(&root)).unwrap();
        let current = read_codex_task(&root, &id).unwrap();
        assert_eq!(current.status, "running");
        assert!(current.result_id.is_empty());
        assert_eq!(read_codex_run(&root, "new").unwrap().status, "running");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn codex_retry_does_not_treat_old_file_as_new_result_after_exit_or_restart() {
        for status in ["exited", "unknownAfterRestart"] {
            let root = setup_project("codex-stale-retry-file");
            let id = generate_codex_prompt(path_to_string(&root))
                .unwrap()
                .prompt
                .id;
            write_analysis_bridge_result(&root, &id, &["Check previous analysis"]);
            scan_codex_result_bridge(path_to_string(&root)).unwrap();
            let path = codex_result_bridge_dir(&root).join(format!("{id}-result.json"));
            let before = fs::read(&path).unwrap();
            let reports_before = read_and_repair_manifest(&root).unwrap().codex_reports.len();
            let mut task = read_codex_task(&root, &id).unwrap();
            task.status = "running".to_string();
            write_codex_task(&root, &task).unwrap();
            let run = CodexRun {
                id: "retry".to_string(),
                task_id: id.clone(),
                status: status.to_string(),
                started_at: (now_string().parse::<u128>().unwrap() + 1000).to_string(),
                exit_code: if status == "exited" { Some(0) } else { None },
                error: "Original process no longer exists".to_string(),
                ..CodexRun::default()
            };
            upsert_codex_run(&root, run.clone()).unwrap();
            assert!(!codex_expected_result_exists(&root, &id));
            if status == "exited" {
                assert!(finish_codex_result_handoff(&root, &run).is_err());
            }
            let listed = list_codex_tasks(path_to_string(&root)).unwrap();
            let current = listed.tasks.iter().find(|task| task.task_id == id).unwrap();
            assert_eq!(
                current.status,
                if status == "exited" {
                    "needsReview"
                } else {
                    "awaitingResult"
                }
            );
            if status == "exited" {
                assert!(!current.remaining_issues.is_empty());
            }
            assert_eq!(fs::read(&path).unwrap(), before);
            assert_eq!(
                read_and_repair_manifest(&root).unwrap().codex_reports.len(),
                reports_before
            );
            fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn codex_reports_keep_independent_evidence_after_result_overwrite() {
        let root = setup_project("codex-history-evidence");
        let id = generate_codex_prompt(path_to_string(&root))
            .unwrap()
            .prompt
            .id;
        write_analysis_bridge_result(&root, &id, &["First conclusion"]);
        let first = scan_codex_result_bridge(path_to_string(&root))
            .unwrap()
            .reports
            .remove(0);
        let first_bytes = fs::read(&first.evidence_managed_path).unwrap();
        write_analysis_bridge_result(&root, &id, &["Second conclusion"]);
        let second = scan_codex_result_bridge(path_to_string(&root))
            .unwrap()
            .reports
            .remove(0);
        assert_ne!(first.evidence_managed_path, second.evidence_managed_path);
        assert_eq!(fs::read(&first.evidence_managed_path).unwrap(), first_bytes);
        assert_eq!(
            codex_result_contract_from_report(&first).manual_acceptance,
            vec!["First conclusion"]
        );
        assert_eq!(
            codex_result_contract_from_report(&second).manual_acceptance,
            vec!["Second conclusion"]
        );
        assert_eq!(
            scan_codex_result_bridge(path_to_string(&root))
                .unwrap()
                .imported_count,
            0
        );
        let current = read_codex_task(&root, &id).unwrap();
        fs::remove_file(&current.expected_result_path).unwrap();
        let reloaded = read_and_repair_manifest(&root).unwrap();
        assert_eq!(reloaded.codex_reports.len(), 2);
        assert_eq!(
            codex_result_contract_from_report(&reloaded.codex_reports[0]).manual_acceptance,
            vec!["First conclusion"]
        );
        assert_eq!(
            codex_result_contract_from_report(&reloaded.codex_reports[1]).manual_acceptance,
            vec!["Second conclusion"]
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn codex_legacy_overwritten_evidence_is_not_reinterpreted_as_old_report() {
        let root = setup_project("codex-legacy-evidence");
        let id = generate_codex_prompt(path_to_string(&root))
            .unwrap()
            .prompt
            .id;
        write_analysis_bridge_result(&root, &id, &["Original acceptance"]);
        let mut legacy = scan_codex_result_bridge(path_to_string(&root))
            .unwrap()
            .reports
            .remove(0);
        let task = read_codex_task(&root, &id).unwrap();
        legacy.evidence_managed_path = task.expected_result_path.clone();
        assert_eq!(
            codex_result_contract_from_report(&legacy).status,
            "completed"
        );
        let mut next: serde_json::Value = read_json(Path::new(&task.expected_result_path)).unwrap();
        next["summary"] = json!("New run content must not appear in old report");
        next["resultSource"] = json!("runnerFallback");
        next["resultRunId"] = json!("new-run");
        next["findings"] = json!(["New run finding"]);
        write_json_atomic(Path::new(&task.expected_result_path), &next).unwrap();
        let contract = codex_result_contract_from_report(&legacy);
        assert_eq!(contract.status, "needsReview");
        assert!(!contract.summary.contains("New run"));
        assert!(contract.findings.is_empty());
        assert!(contract.result_run_id.is_empty());
        assert_ne!(contract.result_source, "runnerFallback");
        assert!(contract
            .remaining_issues
            .iter()
            .any(|issue| issue.contains("证据缺失或已变化")));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn codex_evidence_snapshot_is_never_overwritten_and_missing_evidence_requires_review() {
        let root = setup_project("codex-snapshot-safety");
        let id = generate_codex_prompt(path_to_string(&root))
            .unwrap()
            .prompt
            .id;
        write_analysis_bridge_result(&root, &id, &["Check snapshot"]);
        let task = read_codex_task(&root, &id).unwrap();
        let source = Path::new(&task.expected_result_path);
        let raw = fs::read_to_string(source).unwrap();
        let report = scan_codex_result_bridge(path_to_string(&root))
            .unwrap()
            .reports
            .remove(0);
        let snapshot = PathBuf::from(&report.evidence_managed_path);
        fs::write(&snapshot, "tampered snapshot").unwrap();
        assert!(bridge_result_evidence_file(&root, source, &raw).is_err());
        assert_eq!(fs::read_to_string(&snapshot).unwrap(), "tampered snapshot");
        fs::remove_file(&snapshot).unwrap();
        scan_codex_result_bridge(path_to_string(&root)).unwrap();
        let current = read_codex_task(&root, &id).unwrap();
        assert_eq!(current.status, "needsReview");
        assert!(accept_codex_task(path_to_string(&root), id, current.result_id).is_err());
        assert_eq!(fs::read_to_string(source).unwrap(), raw);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn codex_same_task_retry_binds_new_result_and_ignores_late_old_result() {
        let root = setup_project("codex-retry-result-binding");
        let id = generate_codex_prompt(path_to_string(&root))
            .unwrap()
            .prompt
            .id;
        let path = codex_result_bridge_dir(&root).join(format!("{id}-result.json"));
        let original_prompt = read_codex_task(&root, &id).unwrap().prompt;
        let mut results = Vec::new();
        for (run_id, started_at) in [("old", "1"), ("new", "2")] {
            let mut task = read_codex_task(&root, &id).unwrap();
            task.status = "running".to_string();
            task.result_received_at = "1".to_string();
            write_codex_task(&root, &task).unwrap();
            let prompt = codex_prompt_for_run(
                &task,
                &CodexRun {
                    id: run_id.to_string(),
                    ..CodexRun::default()
                },
            );
            assert!(prompt.contains(&format!("resultRunId=\"{run_id}\"")));
            assert!(prompt.contains(&task.expected_result_path));
            upsert_codex_run(
                &root,
                CodexRun {
                    id: run_id.to_string(),
                    task_id: id.clone(),
                    status: "running".to_string(),
                    started_at: started_at.to_string(),
                    pid: std::process::id(),
                    ..CodexRun::default()
                },
            )
            .unwrap();
            write_analysis_bridge_result(&root, &id, &["Confirm this attempt"]);
            let mut value: serde_json::Value = read_json(&path).unwrap();
            value["resultRunId"] = json!(run_id);
            write_json_atomic(&path, &value).unwrap();
            assert_eq!(
                scan_codex_result_bridge(path_to_string(&root))
                    .unwrap()
                    .imported_count,
                1
            );
            let current = read_codex_task(&root, &id).unwrap();
            assert_eq!(current.result_run_id, run_id);
            assert_eq!(current.status, "awaitingAcceptance");
            assert_ne!(current.result_received_at, "1");
            assert_eq!(current.prompt, original_prompt);
            results.push((current.result_id, fs::read(&path).unwrap()));
        }
        assert_ne!(results[0].0, results[1].0);
        fs::write(&path, &results[0].1).unwrap();
        assert_eq!(
            scan_codex_result_bridge(path_to_string(&root))
                .unwrap()
                .imported_count,
            0
        );
        assert_eq!(read_codex_task(&root, &id).unwrap().result_id, results[1].0);
        assert_eq!(
            read_and_repair_manifest(&root).unwrap().codex_reports.len(),
            2
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn codex_markdown_result_keeps_run_binding_and_legacy_result_still_loads() {
        let root = setup_project("codex-markdown-run-binding");
        let id = generate_codex_prompt(path_to_string(&root))
            .unwrap()
            .prompt
            .id;
        let path = codex_result_bridge_dir(&root).join(format!("{id}-result.md"));
        let mut run = CodexRun {
            id: "current".to_string(),
            task_id: id.clone(),
            status: "running".to_string(),
            started_at: "1".to_string(),
            pid: std::process::id(),
            ..CodexRun::default()
        };
        upsert_codex_run(&root, run.clone()).unwrap();
        fs::write(&path, format!("taskId: {id}\nresultRunId: current\n完成状态：completed\n完成摘要：完成分析并提出下一步建议\nresultSource: codex\n")).unwrap();
        assert!(codex_result_file_matches_run(&path, &run));
        scan_codex_result_bridge(path_to_string(&root)).unwrap();
        assert_eq!(
            read_codex_task(&root, &id).unwrap().result_run_id,
            "current"
        );
        run.id = "other".to_string();
        assert!(!codex_result_file_matches_run(&path, &run));
        fs::write(
            &path,
            format!("taskId: {id}\n完成状态：completed\n完成摘要：兼容旧版结果\n"),
        )
        .unwrap();
        assert!(codex_result_file_matches_run(&path, &run));
        let json_path = path.with_extension("json");
        write_json_atomic(&json_path, &json!({"taskId": id, "resultRunId": ""})).unwrap();
        let report = CodexReportRecord {
            evidence_managed_path: path_to_string(&json_path),
            ..CodexReportRecord::default()
        };
        assert!(codex_result_contract_from_report(&report)
            .result_run_id
            .is_empty());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn codex_exit_without_result_never_generates_runner_fallback() {
        let root = setup_project("codex-exit-without-result");
        let id = generate_codex_prompt(path_to_string(&root))
            .unwrap()
            .prompt
            .id;
        let run = CodexRun {
            id: "run-without-result".to_string(),
            task_id: id.clone(),
            status: "exited".to_string(),
            started_at: now_string(),
            exit_code: Some(0),
            ..CodexRun::default()
        };
        upsert_codex_run(&root, run.clone()).unwrap();

        assert!(wait_for_matching_codex_result(&root, &id, &run, false).is_err());
        mark_codex_task_missing_result(&root, &id, "Codex 已退出，但结果缺失。").unwrap();

        assert!(!codex_expected_result_exists(&root, &id));
        assert_eq!(read_codex_task(&root, &id).unwrap().status, "needsReview");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn codex_task_lifecycle_requires_real_result_git_and_acceptance() {
        let root = setup_project("codex-task-lifecycle");
        let repo = test_root("codex-task-lifecycle-repo");
        fs::create_dir_all(&repo).unwrap();
        assert!(std::process::Command::new("git")
            .arg("-C")
            .arg(&repo)
            .arg("init")
            .status()
            .unwrap()
            .success());
        for args in [
            vec!["config", "user.email", "test@example.local"],
            vec!["config", "user.name", "Ganmaoyuan Test"],
        ] {
            assert!(std::process::Command::new("git")
                .arg("-C")
                .arg(&repo)
                .args(args)
                .status()
                .unwrap()
                .success());
        }
        fs::write(repo.join("README.md"), "initial").unwrap();
        assert!(std::process::Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(["add", "README.md"])
            .status()
            .unwrap()
            .success());
        assert!(std::process::Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(["commit", "-m", "initial"])
            .status()
            .unwrap()
            .success());
        let snapshot =
            refresh_git_snapshot(path_to_string(&root), Some(path_to_string(&repo))).unwrap();

        let prompt = generate_codex_prompt(path_to_string(&root)).unwrap();
        let task_id = prompt.prompt.id.clone();
        assert_eq!(read_codex_task(&root, &task_id).unwrap().status, "ready");

        let handed_off =
            mark_codex_task_handed_off(path_to_string(&root), task_id.clone()).unwrap();
        assert_eq!(handed_off.status, "awaitingResult");

        fs::write(
            codex_result_bridge_dir(&root).join(format!("{task_id}-result.json")),
            format!(
                r#"{{
  "taskId": "{task_id}",
  "status": "completed",
  "summary": "完成 CodexTask 生命周期测试",
  "changedFiles": ["README.md"],
  "commits": ["{}"],
  "tests": ["cargo test 通过"],
  "remainingIssues": [],
  "manualAcceptance": ["确认界面状态"],
  "completedAt": "2026-08-25T12:00:00Z"
}}"#,
                snapshot.head
            ),
        )
        .unwrap();
        let bridge = scan_codex_result_bridge(path_to_string(&root)).unwrap();
        assert_eq!(bridge.imported_count, 1);
        let task = read_codex_task(&root, &task_id).unwrap();
        assert_eq!(task.status, "awaitingAcceptance");
        assert_eq!(task.git_verification.status, "verified");
        assert_eq!(task.verified_commits, vec![snapshot.head.clone()]);
        assert_eq!(task.manual_acceptance, vec!["确认界面状态".to_string()]);

        let accepted = accept_codex_task(
            path_to_string(&root),
            task_id.clone(),
            task.result_id.clone(),
        )
        .unwrap();
        assert_eq!(accepted.status, "completed");
        assert_eq!(accepted.acceptance.status, "approved");
        let reloaded = get_work_ledger(path_to_string(&root)).unwrap();
        assert!(reloaded
            .codex_tasks
            .iter()
            .any(|item| item.task_id == task_id && item.status == "completed"));
        assert!(reloaded
            .events
            .iter()
            .any(|event| event.event_type == "codex.taskAccepted"));
        fs::remove_dir_all(root).unwrap();
        fs::remove_dir_all(repo).unwrap();
    }

    #[test]
    fn nested_coding_result_records_commit_and_can_be_accepted() {
        let root = setup_project("nested-coding-result");
        let repo = setup_test_git_repository("nested-coding-result-repo");
        refresh_git_snapshot(path_to_string(&root), Some(path_to_string(&repo))).unwrap();
        let task = create_codex_task(
            path_to_string(&root),
            CodexTaskCreateRequest {
                title: "只修改本次代码".to_string(),
                task_type: "coding".to_string(),
                instructions: "创建 src/task-owned.txt 并提交。".to_string(),
            },
        )
        .unwrap();
        let (head_before, worktree_status_before, dirty_files_before) =
            capture_codex_run_git_baseline(&repo, &task).unwrap();
        assert_eq!(worktree_status_before, "clean");
        assert!(dirty_files_before.is_empty());
        let run = CodexRun {
            id: "run-nested-coding".to_string(),
            task_id: task.task_id.clone(),
            project_id: task.project_id.clone(),
            repository_path: path_to_string(&repo),
            status: "exited".to_string(),
            started_at: "2026-09-24T08:00:00Z".to_string(),
            ended_at: "2026-09-24T08:01:00Z".to_string(),
            exit_code: Some(0),
            git_head_before: head_before,
            git_worktree_status_before: worktree_status_before,
            git_dirty_files_before: dirty_files_before,
            ..CodexRun::default()
        };
        upsert_codex_run(&root, run.clone()).unwrap();
        fs::create_dir_all(repo.join("src")).unwrap();
        fs::write(repo.join("src/task-owned.txt"), "owned by this task\n").unwrap();
        assert!(std::process::Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(["add", "--", "src/task-owned.txt"])
            .status()
            .unwrap()
            .success());
        assert!(std::process::Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(["commit", "-m", "test: commit coding task"])
            .status()
            .unwrap()
            .success());
        let commit = run_git(&repo, &["rev-parse", "HEAD"])
            .unwrap()
            .trim()
            .to_string();
        write_json_atomic(
            &PathBuf::from(&task.expected_result_path),
            &json!({
                "taskId": task.task_id,
                "taskType": "coding",
                "resultRunId": run.id,
                "status": "completed",
                "summary": "已提交本次代码修改",
                "resultSource": "codex",
                "manualAcceptance": ["请人工核对"],
                "coding": {
                    "changedFiles": ["src/task-owned.txt"],
                    "commits": [commit],
                    "tests": [{"name": "git diff --check", "status": "passed", "details": "无空白错误"}],
                    "gitHead": commit,
                    "gitStatus": "clean",
                    "gitModifiedFiles": []
                }
            }),
        )
        .unwrap();

        scan_codex_result_bridge(path_to_string(&root)).unwrap();
        let received = read_codex_task(&root, &task.task_id).unwrap();
        assert_eq!(received.status, "awaitingAcceptance");
        assert_eq!(received.reported_commits, vec![commit.clone()]);
        assert_eq!(
            received.changed_files,
            vec!["src/task-owned.txt".to_string()]
        );
        assert_eq!(received.git_verification.status, "verified");
        assert!(received
            .tests
            .iter()
            .any(|test| test.contains("git diff --check")));

        let accepted = accept_codex_task(
            path_to_string(&root),
            task.task_id.clone(),
            received.result_id.clone(),
        )
        .unwrap();
        assert_eq!(accepted.status, "completed");
        assert_eq!(accepted.acceptance.status, "approved");
        fs::remove_dir_all(root).unwrap();
        fs::remove_dir_all(repo).unwrap();
    }

    #[test]
    fn coding_result_rejects_commit_that_includes_startup_dirty_file() {
        let root = setup_project("coding-dirty-baseline");
        let repo = setup_test_git_repository("coding-dirty-baseline-repo");
        refresh_git_snapshot(path_to_string(&root), Some(path_to_string(&repo))).unwrap();
        let task = create_codex_task(
            path_to_string(&root),
            CodexTaskCreateRequest {
                title: "仅提交任务文件".to_string(),
                task_type: "coding".to_string(),
                instructions: "创建 src/task-only.txt。".to_string(),
            },
        )
        .unwrap();
        fs::write(repo.join("unrelated.md"), "pre-existing user change\n").unwrap();
        let (head_before, worktree_status_before, dirty_files_before) =
            capture_codex_run_git_baseline(&repo, &task).unwrap();
        assert_eq!(worktree_status_before, "dirty");
        assert_eq!(dirty_files_before[0].path, "unrelated.md");
        let run = CodexRun {
            id: "run-dirty-baseline".to_string(),
            task_id: task.task_id.clone(),
            project_id: task.project_id.clone(),
            repository_path: path_to_string(&repo),
            status: "exited".to_string(),
            started_at: "2026-09-24T08:00:00Z".to_string(),
            exit_code: Some(0),
            git_head_before: head_before,
            git_worktree_status_before: worktree_status_before,
            git_dirty_files_before: dirty_files_before,
            ..CodexRun::default()
        };
        upsert_codex_run(&root, run.clone()).unwrap();
        fs::create_dir_all(repo.join("src")).unwrap();
        fs::write(repo.join("src/task-only.txt"), "task change\n").unwrap();
        assert!(std::process::Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(["add", "--", "unrelated.md", "src/task-only.txt"])
            .status()
            .unwrap()
            .success());
        assert!(std::process::Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(["commit", "-m", "test: unsafe mixed commit"])
            .status()
            .unwrap()
            .success());
        let commit = run_git(&repo, &["rev-parse", "HEAD"])
            .unwrap()
            .trim()
            .to_string();
        write_json_atomic(
            &PathBuf::from(&task.expected_result_path),
            &json!({
                "taskId": task.task_id,
                "taskType": "coding",
                "resultRunId": run.id,
                "status": "completed",
                "summary": "不应接受混合提交",
                "resultSource": "codex",
                "manualAcceptance": ["请人工核对"],
                "coding": {
                    "changedFiles": ["src/task-only.txt"],
                    "commits": [commit],
                    "tests": ["passed"]
                }
            }),
        )
        .unwrap();

        scan_codex_result_bridge(path_to_string(&root)).unwrap();
        let received = read_codex_task(&root, &task.task_id).unwrap();
        assert_eq!(received.status, "needsReview");
        assert_eq!(received.git_verification.status, "mismatch");
        assert!(received.git_verification.reason.contains("unrelated.md"));
        assert!(
            accept_codex_task(path_to_string(&root), task.task_id, received.result_id).is_err()
        );
        fs::remove_dir_all(root).unwrap();
        fs::remove_dir_all(repo).unwrap();
    }

    #[test]
    fn coding_result_without_commit_remains_unacceptable() {
        let root = setup_project("coding-no-commit");
        let repo = setup_test_git_repository("coding-no-commit-repo");
        refresh_git_snapshot(path_to_string(&root), Some(path_to_string(&repo))).unwrap();
        let task = create_codex_task(
            path_to_string(&root),
            CodexTaskCreateRequest {
                title: "没有提交的代码任务".to_string(),
                task_type: "coding".to_string(),
                instructions: "仅用于验证无 commit 不能验收。".to_string(),
            },
        )
        .unwrap();
        let run = CodexRun {
            id: "run-no-commit".to_string(),
            task_id: task.task_id.clone(),
            project_id: task.project_id.clone(),
            repository_path: path_to_string(&repo),
            status: "exited".to_string(),
            started_at: "2026-09-24T08:00:00Z".to_string(),
            exit_code: Some(0),
            git_head_before: run_git(&repo, &["rev-parse", "HEAD"])
                .unwrap()
                .trim()
                .to_string(),
            git_worktree_status_before: "clean".to_string(),
            ..CodexRun::default()
        };
        upsert_codex_run(&root, run.clone()).unwrap();
        write_json_atomic(
            &PathBuf::from(&task.expected_result_path),
            &json!({
                "taskId": task.task_id,
                "taskType": "coding",
                "resultRunId": run.id,
                "status": "completed",
                "summary": "没有 commit",
                "resultSource": "codex",
                "coding": {"changedFiles": ["src/no-commit.txt"], "commits": [], "tests": ["passed"]}
            }),
        )
        .unwrap();

        scan_codex_result_bridge(path_to_string(&root)).unwrap();
        let received = read_codex_task(&root, &task.task_id).unwrap();
        assert_eq!(received.status, "needsReview");
        assert_eq!(received.git_verification.status, "unavailable");
        assert!(
            accept_codex_task(path_to_string(&root), task.task_id, received.result_id).is_err()
        );
        fs::remove_dir_all(root).unwrap();
        fs::remove_dir_all(repo).unwrap();
    }

    #[test]
    fn codex_task_mismatch_and_duplicate_result_do_not_complete_or_duplicate_events() {
        let root = setup_project("codex-task-mismatch");
        let repo = test_root("codex-task-mismatch-repo");
        fs::create_dir_all(&repo).unwrap();
        assert!(std::process::Command::new("git")
            .arg("-C")
            .arg(&repo)
            .arg("init")
            .status()
            .unwrap()
            .success());
        for args in [
            vec!["config", "user.email", "test@example.local"],
            vec!["config", "user.name", "Ganmaoyuan Test"],
        ] {
            assert!(std::process::Command::new("git")
                .arg("-C")
                .arg(&repo)
                .args(args)
                .status()
                .unwrap()
                .success());
        }
        fs::write(repo.join("README.md"), "initial").unwrap();
        assert!(std::process::Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(["add", "README.md"])
            .status()
            .unwrap()
            .success());
        assert!(std::process::Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(["commit", "-m", "initial"])
            .status()
            .unwrap()
            .success());
        let _ = refresh_git_snapshot(path_to_string(&root), Some(path_to_string(&repo))).unwrap();
        let prompt = generate_codex_prompt(path_to_string(&root)).unwrap();
        let task_id = prompt.prompt.id.clone();
        fs::write(
            codex_result_bridge_dir(&root).join(format!("{task_id}-result.json")),
            format!(
                r#"{{
  "taskId": "{task_id}",
  "status": "completed",
  "summary": "报告声称完成但 commit 不存在",
  "changedFiles": ["src/lib.rs"],
  "commits": ["deadbeef"],
  "tests": ["npm run build 通过"],
  "remainingIssues": [],
  "manualAcceptance": [],
  "completedAt": "2026-08-25T12:00:00Z"
}}"#,
            ),
        )
        .unwrap();

        let first = scan_codex_result_bridge(path_to_string(&root)).unwrap();
        let second = scan_codex_result_bridge(path_to_string(&root)).unwrap();
        let task = read_codex_task(&root, &task_id).unwrap();
        let ledger = read_work_ledger(&root).unwrap();

        assert_eq!(first.imported_count, 1);
        assert_eq!(second.imported_count, 0);
        assert_eq!(task.status, "needsReview");
        assert_ne!(task.status, "completed");
        assert_eq!(task.git_verification.status, "mismatch");
        assert_eq!(
            ledger
                .events
                .iter()
                .filter(|event| event.event_type == "codex.reportImported")
                .count(),
            1
        );
        fs::remove_dir_all(root).unwrap();
        fs::remove_dir_all(repo).unwrap();
    }

    #[test]
    fn analysis_codex_result_completes_without_git_commit_or_tests() {
        let root = setup_project("codex-analysis-no-git");
        ensure_project_dirs(&root).unwrap();
        let task_id = "task-analysis";
        write_codex_task(
            &root,
            &CodexTask {
                task_id: task_id.to_string(),
                project_id: "project-1".to_string(),
                title: "生成项目理解".to_string(),
                task_type: "analysis".to_string(),
                status: "awaitingResult".to_string(),
                expected_result_path: path_to_string(
                    &codex_result_bridge_dir(&root).join(format!("{task_id}-result.json")),
                ),
                ..CodexTask::default()
            },
        )
        .unwrap();
        fs::write(
            codex_result_bridge_dir(&root).join(format!("{task_id}-result.json")),
            r#"{
  "taskId": "task-analysis",
  "taskType": "analysis",
  "status": "completed",
  "summary": "完成项目理解",
  "resultText": "已经基于当前项目资料整理项目定义、缺口和下一步建议。",
  "findings": ["项目资料已经形成闭环"],
  "recommendations": ["下一步继续做真实验收"],
  "questions": [],
  "remainingIssues": [],
  "manualAcceptance": [],
  "completedAt": "2026-08-26T00:00:00Z"
}"#,
        )
        .unwrap();

        scan_codex_result_bridge(path_to_string(&root)).unwrap();
        let task = read_codex_task(&root, task_id).unwrap();

        assert_eq!(task.task_type, "analysis");
        assert_eq!(task.status, "completed");
        assert_eq!(task.git_verification.status, "notApplicable");
        assert!(task.reported_commits.is_empty());
        assert!(task.tests.is_empty());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn infer_codex_task_type_prefers_project_understanding_over_git_boilerplate() {
        let task_type = infer_codex_task_type(
            "生成项目理解与下一步建议\nGit 要求：若本任务没有 Git 改动，请明确说明 Git 不适用。\n完成后不要虚构 commit/tests。",
        );

        assert_eq!(task_type, "analysis");
    }

    #[test]
    fn result_file_task_id_mismatch_requires_review_without_updating_wrong_task() {
        let root = setup_project("codex-result-task-id-mismatch");
        ensure_project_dirs(&root).unwrap();
        write_codex_task(
            &root,
            &CodexTask {
                task_id: "task-a".to_string(),
                project_id: "project-1".to_string(),
                task_type: "analysis".to_string(),
                status: "awaitingResult".to_string(),
                expected_result_path: path_to_string(
                    &codex_result_bridge_dir(&root).join("task-a-result.json"),
                ),
                ..CodexTask::default()
            },
        )
        .unwrap();
        fs::write(
            codex_result_bridge_dir(&root).join("task-a-result.json"),
            r#"{
  "taskId": "task-b",
  "taskType": "analysis",
  "status": "completed",
  "summary": "这是错误 taskId 的结果",
  "resultText": "不应该更新 task-b。"
}"#,
        )
        .unwrap();

        scan_codex_result_bridge(path_to_string(&root)).unwrap();
        let task_a = read_codex_task(&root, "task-a").unwrap();
        let task_b = read_codex_task(&root, "task-b");

        assert_eq!(task_a.status, "needsReview");
        assert!(task_a
            .remaining_issues
            .iter()
            .any(|item| item.contains("taskId 不一致")));
        assert!(task_b.is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn exited_run_output_never_becomes_a_synthetic_result_during_scan() {
        let root = setup_project("codex-fallback-current-run");
        ensure_project_dirs(&root).unwrap();
        let task_id = "task-current-output";
        write_codex_task(
            &root,
            &CodexTask {
                task_id: task_id.to_string(),
                project_id: "project-1".to_string(),
                task_type: "analysis".to_string(),
                status: "awaitingResult".to_string(),
                expected_result_path: path_to_string(
                    &codex_result_bridge_dir(&root).join(format!("{task_id}-result.json")),
                ),
                ..CodexTask::default()
            },
        )
        .unwrap();
        upsert_codex_run(
            &root,
            CodexRun {
                id: "run-old".to_string(),
                task_id: task_id.to_string(),
                status: "failed".to_string(),
                stderr_summary: "旧 run 的错误不应污染新结果".to_string(),
                started_at: "1".to_string(),
                ..CodexRun::default()
            },
        )
        .unwrap();
        let last_message = codex_run_output_path(&root, "run-new");
        write_text_atomic(&last_message, "当前 run 完成了项目分析并留下了可用结论。").unwrap();
        upsert_codex_run(
            &root,
            CodexRun {
                id: "run-new".to_string(),
                task_id: task_id.to_string(),
                status: "exited".to_string(),
                exit_code: Some(0),
                output_last_message_path: path_to_string(&last_message),
                started_at: "2".to_string(),
                ended_at: "3".to_string(),
                ..CodexRun::default()
            },
        )
        .unwrap();

        scan_codex_result_bridge(path_to_string(&root)).unwrap();
        let external = read_codex_external_results(&root).unwrap();
        let task = read_codex_task(&root, task_id).unwrap();

        assert!(external.results.is_empty());
        assert_eq!(task.status, "awaitingResult");
        assert!(!task
            .remaining_issues
            .iter()
            .any(|item| item.contains("旧 run")));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn later_successful_run_is_not_overwritten_by_older_failed_run() {
        let root = setup_project("codex-multi-run-isolation");
        write_codex_task(
            &root,
            &CodexTask {
                task_id: "task-multi".to_string(),
                project_id: "project-1".to_string(),
                task_type: "analysis".to_string(),
                status: "completed".to_string(),
                result_id: "result-1".to_string(),
                updated_at: "3".to_string(),
                ..CodexTask::default()
            },
        )
        .unwrap();
        upsert_codex_run(
            &root,
            CodexRun {
                id: "run-old".to_string(),
                task_id: "task-multi".to_string(),
                status: "failed".to_string(),
                error: "旧失败".to_string(),
                started_at: "1".to_string(),
                ..CodexRun::default()
            },
        )
        .unwrap();
        upsert_codex_run(
            &root,
            CodexRun {
                id: "run-new".to_string(),
                task_id: "task-multi".to_string(),
                status: "exited".to_string(),
                exit_code: Some(0),
                started_at: "2".to_string(),
                ..CodexRun::default()
            },
        )
        .unwrap();

        sync_failed_codex_runs_to_tasks(&root).unwrap();
        let task = read_codex_task(&root, "task-multi").unwrap();

        assert_eq!(task.status, "completed");
        assert!(!task
            .remaining_issues
            .iter()
            .any(|item| item.contains("旧失败")));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn codex_exec_args_are_repo_scoped_and_not_full_access() {
        let repo = std::env::temp_dir().join("ganmaoyuan-test-repository");
        let project = std::env::temp_dir().join("ganmaoyuan-test-project");
        let result_directory = project.join(".ganmaoyuan/codex/results");
        let output = project.join(".ganmaoyuan/codex/runs/run-last-message.txt");
        let probe = CodexCliCapabilityProbe {
            available: true,
            supports_cd: true,
            supports_add_dir: true,
            supports_json: true,
            supports_output_last_message: true,
            supports_sandbox: true,
            supports_stdin: true,
            ..CodexCliCapabilityProbe::default()
        };

        let args = codex_exec_args(&repo, &result_directory, &output, &probe, false).unwrap();
        let repo_arg = path_to_string(&repo);
        let result_arg = path_to_string(&result_directory);

        assert!(args.windows(2).any(|pair| pair == ["-C", &repo_arg]));
        assert!(args
            .windows(2)
            .any(|pair| pair == ["--add-dir", &result_arg]));
        assert!(!args.iter().any(|arg| arg == &path_to_string(&project)));
        assert!(args
            .windows(2)
            .any(|pair| pair == ["--sandbox", "workspace-write"]));
        assert!(!args.iter().any(|arg| arg == "--ask-for-approval"));
        assert!(!args.iter().any(|arg| arg.contains("danger")));
        assert!(!args.iter().any(|arg| arg.contains("full-access")));
    }

    #[test]
    fn native_codex_exec_args_use_safe_auto_approval_without_conflicting_sandbox_flag() {
        let repo = std::env::temp_dir().join("ganmaoyuan-test-repository");
        let project = std::env::temp_dir().join("ganmaoyuan-test-project");
        let result_directory = project.join(".ganmaoyuan/codex/results");
        let output = project.join(".ganmaoyuan/codex/runs/run-last-message.txt");
        let probe = CodexCliCapabilityProbe {
            available: true,
            supports_cd: true,
            supports_add_dir: true,
            supports_json: true,
            supports_output_last_message: true,
            supports_stdin: true,
            ..CodexCliCapabilityProbe::default()
        };

        let args = codex_exec_args(&repo, &result_directory, &output, &probe, true).unwrap();

        assert!(args.iter().any(|arg| arg == "--approve-for-me"));
        assert!(args.iter().any(|arg| arg == "--ignore-user-config"));
        assert!(!args.iter().any(|arg| arg == "--sandbox"));
        assert!(!args.iter().any(|arg| arg.contains("danger")));
        assert!(!args.iter().any(|arg| arg.contains("full-access")));
    }

    #[test]
    fn file_operation_run_prompt_avoids_the_unreliable_shell_path() {
        let prompt = codex_prompt_for_run(
            &CodexTask {
                task_id: "file-task".to_string(),
                task_type: "fileOperation".to_string(),
                expected_result_path: "D:/project/.ganmaoyuan/codex/results/file-task-result.json"
                    .to_string(),
                prompt: "taskId：file-task\n执行文件操作".to_string(),
                ..CodexTask::default()
            },
            &CodexRun {
                id: "run-file".to_string(),
                ..CodexRun::default()
            },
        );

        assert!(prompt.contains("优先使用 Codex 专用 apply_patch 文件工具"));
        assert!(prompt.contains("不要使用 terminal、shell、PowerShell 或 cmd"));
        assert!(prompt.contains("resultRunId=\"run-file\""));
    }

    #[test]
    fn non_file_codex_runs_require_cmd_on_windows() {
        let prompt = codex_prompt_for_run(
            &CodexTask {
                task_id: "analysis-task".to_string(),
                task_type: "analysis".to_string(),
                expected_result_path:
                    "D:/project/.ganmaoyuan/codex/results/analysis-task-result.json".to_string(),
                prompt: "taskId：analysis-task\n读取项目状态".to_string(),
                ..CodexTask::default()
            },
            &CodexRun {
                id: "run-analysis".to_string(),
                ..CodexRun::default()
            },
        );

        if cfg!(target_os = "windows") {
            assert!(prompt.contains("C:\\Windows\\System32\\cmd.exe"));
            assert!(prompt.contains("不要使用 PowerShell、pwsh"));
        }
        assert!(prompt.contains("resultRunId=\"run-analysis\""));
    }

    #[test]
    fn file_operation_without_reported_commit_does_not_require_git_verification() {
        let task = CodexTask {
            task_type: "fileOperation".to_string(),
            changed_files: vec!["docs/codex-golden-path-test.md".to_string()],
            target_files: vec!["docs/codex-golden-path-test.md".to_string()],
            ..CodexTask::default()
        };

        assert!(!codex_task_git_required(&task));
    }

    #[test]
    fn codex_task_result_directory_is_limited_to_the_bridge_directory() {
        let root = setup_project("codex-result-directory-scope");
        let task = CodexTask {
            task_id: "isolated-task".to_string(),
            expected_result_path: path_to_string(
                &codex_result_bridge_dir(&root).join("isolated-task-result.json"),
            ),
            ..CodexTask::default()
        };

        assert_eq!(
            codex_task_result_directory(&root, &task).unwrap(),
            codex_result_bridge_dir(&root)
        );

        let mut outside = task;
        outside.expected_result_path =
            path_to_string(&root.join("other/isolated-task-result.json"));
        assert!(codex_task_result_directory(&root, &outside).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn codex_task_result_directory_accepts_equivalent_windows_separator_forms() {
        let root = setup_project("codex-result-directory-separators");
        let task_id = "separator-task";
        let expected = path_to_string(&canonical_codex_result_path(&root, task_id));
        let task = CodexTask {
            task_id: task_id.to_string(),
            expected_result_path: expected.replace('\\', "/"),
            ..CodexTask::default()
        };

        assert_eq!(
            codex_task_result_directory(&root, &task).unwrap(),
            codex_result_bridge_dir(&root)
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn codex_cli_capability_probe_detects_supported_exec_flags() {
        let probe = parse_codex_cli_capability_probe(
            "codex-cli 0.145.0",
            "Usage: codex exec [OPTIONS] [PROMPT]\n  -C, --cd <DIR>\n      --add-dir <DIR>\n      --json\n  -o, --output-last-message <FILE>\n  -s, --sandbox <MODE>",
            "2026-08-26T00:00:00Z".to_string(),
        );

        assert!(probe.available);
        assert!(probe.supports_cd);
        assert!(probe.supports_add_dir);
        assert!(probe.supports_json);
        assert!(probe.supports_output_last_message);
        assert!(probe.supports_sandbox);
        assert!(probe.supports_stdin);
    }

    #[test]
    fn codex_run_failure_reason_explains_invalid_cli_argument() {
        let reason = codex_run_failure_reason(
            Some(2),
            "error: unexpected argument '--ask-for-approval' found",
            "",
        );

        assert!(reason.contains("Codex CLI 参数无效"));
        assert!(reason.contains("--ask-for-approval"));
    }

    #[test]
    fn codex_runs_persist_and_active_run_blocks_same_task() {
        let root = setup_project("codex-run-persist");
        let run = CodexRun {
            id: "run-1".to_string(),
            task_id: "task-1".to_string(),
            project_id: "project-1".to_string(),
            repository_path: "D:/repo".to_string(),
            status: "running".to_string(),
            started_at: now_string(),
            pid: std::process::id(),
            ..CodexRun::default()
        };

        upsert_codex_run(&root, run.clone()).unwrap();
        let active = active_codex_run_for_task(&root, "task-1").unwrap();
        let reloaded = read_codex_runs(&root).unwrap();

        assert_eq!(reloaded.len(), 1);
        assert_eq!(reloaded[0].id, "run-1");
        assert!(active.is_some());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn stale_codex_run_is_marked_exited_after_restart() {
        let root = setup_project("codex-run-stale");
        upsert_codex_run(
            &root,
            CodexRun {
                id: "run-stale".to_string(),
                task_id: "task-1".to_string(),
                status: "running".to_string(),
                started_at: now_string(),
                pid: u32::MAX,
                ..CodexRun::default()
            },
        )
        .unwrap();

        recover_stale_codex_runs(&root).unwrap();
        let runs = read_codex_runs(&root).unwrap();

        assert_eq!(runs[0].status, "unknownAfterRestart");
        assert!(!runs[0].error.is_empty());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn matching_result_closes_stale_run_without_restart_warning() {
        let root = setup_project("codex-run-result-recovery");
        let task = create_codex_task(
            path_to_string(&root),
            CodexTaskCreateRequest {
                title: "恢复真实结果".to_string(),
                task_type: "fileOperation".to_string(),
                instructions: "仅创建证明文件。".to_string(),
            },
        )
        .unwrap();
        let run = CodexRun {
            id: "run-with-result".to_string(),
            task_id: task.task_id.clone(),
            project_id: task.project_id.clone(),
            status: "running".to_string(),
            started_at: now_string(),
            pid: u32::MAX,
            ..CodexRun::default()
        };
        upsert_codex_run(&root, run.clone()).unwrap();
        fs::write(
            &task.expected_result_path,
            format!(
                "{{\"taskId\":\"{}\",\"resultRunId\":\"{}\",\"status\":\"completed\"}}",
                task.task_id, run.id
            ),
        )
        .unwrap();

        recover_stale_codex_runs(&root).unwrap();
        let recovered = read_codex_run(&root, &run.id).unwrap();

        assert_eq!(recovered.status, "exited");
        assert!(recovered.error.is_empty());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn starting_codex_run_is_active_without_being_recovered_immediately() {
        let root = setup_project("codex-run-starting");
        let run = CodexRun {
            id: "run-starting".to_string(),
            task_id: "task-1".to_string(),
            status: "starting".to_string(),
            started_at: now_string(),
            ..CodexRun::default()
        };
        upsert_codex_run(&root, run).unwrap();

        recover_stale_codex_runs(&root).unwrap();

        assert_eq!(
            read_codex_run(&root, "run-starting").unwrap().status,
            "starting"
        );
        assert!(active_codex_run_for_task(&root, "task-1")
            .unwrap()
            .is_some());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn coding_task_running_past_three_minutes_stays_within_its_long_task_window() {
        let now = now_string().parse::<u128>().unwrap();
        let run = CodexRun {
            status: "running".to_string(),
            started_at: (now - CODEX_STANDARD_RUN_TIMEOUT.as_millis() - 1).to_string(),
            ..CodexRun::default()
        };

        assert!(!codex_run_execution_timed_out(&run, "coding"));
        assert!(codex_run_execution_timed_out(&run, "analysis"));
        let coding_expired = CodexRun {
            started_at: (now - CODEX_CODING_RUN_TIMEOUT.as_millis() - 1).to_string(),
            ..run
        };
        assert!(codex_run_execution_timed_out(&coding_expired, "coding"));
        assert!(codex_timeout_error("coding").contains("30 分钟"));
        assert!(codex_timeout_error("analysis").contains("3 分钟"));
    }

    #[test]
    fn live_coding_task_past_standard_timeout_is_not_recovered_as_failed() {
        let root = setup_project("codex-coding-long-run-recovery");
        let task = create_codex_task(
            path_to_string(&root),
            CodexTaskCreateRequest {
                title: "长时间代码修改".to_string(),
                task_type: "coding".to_string(),
                instructions: "执行一项需要较长时间的代码修改。".to_string(),
            },
        )
        .unwrap();
        let mut running_task = task.clone();
        running_task.status = "running".to_string();
        write_codex_task(&root, &running_task).unwrap();
        upsert_codex_run(
            &root,
            CodexRun {
                id: "run-long-coding".to_string(),
                task_id: task.task_id.clone(),
                project_id: task.project_id.clone(),
                status: "running".to_string(),
                started_at: (now_string().parse::<u128>().unwrap()
                    - CODEX_STANDARD_RUN_TIMEOUT.as_millis()
                    - 1)
                .to_string(),
                pid: std::process::id(),
                ..CodexRun::default()
            },
        )
        .unwrap();

        recover_stale_codex_runs(&root).unwrap();

        assert_eq!(
            read_codex_run(&root, "run-long-coding").unwrap().status,
            "running"
        );
        assert_eq!(
            read_codex_task(&root, &task.task_id).unwrap().status,
            "running"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn user_cancel_terminates_codex_process_and_marks_task_cancelled() {
        let root = setup_project("codex-user-cancel");
        let task = create_codex_task(
            path_to_string(&root),
            CodexTaskCreateRequest {
                title: "可停止的代码任务".to_string(),
                task_type: "coding".to_string(),
                instructions: "等待用户主动停止。".to_string(),
            },
        )
        .unwrap();
        let mut child = hidden_command("cmd.exe")
            .args(["/d", "/s", "/c", "ping 127.0.0.1 -n 60 >NUL"])
            .spawn()
            .unwrap();
        let pid = child.id();
        upsert_codex_run(
            &root,
            CodexRun {
                id: "run-user-cancel".to_string(),
                task_id: task.task_id.clone(),
                project_id: task.project_id.clone(),
                status: "running".to_string(),
                started_at: now_string(),
                pid,
                ..CodexRun::default()
            },
        )
        .unwrap();

        let cancelled = cancel_codex_task_run(path_to_string(&root), task.task_id.clone()).unwrap();
        assert_eq!(cancelled.status, "cancelled");
        assert!(!process_exists(pid));
        assert_eq!(
            read_codex_task(&root, &task.task_id).unwrap().status,
            "cancelled"
        );
        let _ = child.wait();
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn failed_codex_run_keeps_task_current_and_retryable() {
        let root = setup_project("codex-run-failed-task");
        let task = CodexTask {
            task_id: "task-new".to_string(),
            project_id: "project-1".to_string(),
            title: "新任务".to_string(),
            prompt: "测试 prompt".to_string(),
            status: "handedOff".to_string(),
            updated_at: "2".to_string(),
            ..CodexTask::default()
        };
        write_codex_task(&root, &task).unwrap();

        mark_codex_task_run_failed(&root, "task-new", "Codex CLI 参数无效").unwrap();
        let reloaded = read_codex_task(&root, "task-new").unwrap();

        assert_eq!(reloaded.status, "failed");
        assert!(reloaded
            .remaining_issues
            .iter()
            .any(|item| item.contains("Codex CLI 参数无效")));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn abnormal_codex_exit_is_recorded_as_failure_not_timeout() {
        let root = setup_project("codex-run-abnormal-exit");
        let task = create_codex_task(
            path_to_string(&root),
            CodexTaskCreateRequest {
                title: "异常退出任务".to_string(),
                task_type: "coding".to_string(),
                instructions: "执行代码修改。".to_string(),
            },
        )
        .unwrap();
        let mut running_task = task.clone();
        running_task.status = "running".to_string();
        write_codex_task(&root, &running_task).unwrap();
        upsert_codex_run(
            &root,
            CodexRun {
                id: "run-abnormal-exit".to_string(),
                task_id: task.task_id.clone(),
                project_id: task.project_id.clone(),
                status: "failed".to_string(),
                exit_code: Some(1),
                error: "Codex CLI 未登录或认证失败：需要登录".to_string(),
                started_at: now_string(),
                ended_at: now_string(),
                ..CodexRun::default()
            },
        )
        .unwrap();

        sync_failed_codex_runs_to_tasks(&root).unwrap();

        let failed = read_codex_task(&root, &task.task_id).unwrap();
        assert_eq!(failed.status, "failed");
        assert!(failed
            .remaining_issues
            .iter()
            .any(|issue| issue.contains("未登录或认证失败")));
        assert!(!failed
            .remaining_issues
            .iter()
            .any(|issue| issue.contains("执行超过")));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn list_codex_tasks_repairs_legacy_failed_run_reason_and_task_status() {
        let root = setup_project("codex-run-repair");
        write_codex_task(
            &root,
            &CodexTask {
                task_id: "task-legacy".to_string(),
                project_id: "project-1".to_string(),
                title: "旧失败任务".to_string(),
                prompt: "测试 prompt".to_string(),
                status: "handedOff".to_string(),
                updated_at: "2".to_string(),
                ..CodexTask::default()
            },
        )
        .unwrap();
        upsert_codex_run(
            &root,
            CodexRun {
                id: "run-legacy".to_string(),
                task_id: "task-legacy".to_string(),
                status: "failed".to_string(),
                started_at: "3".to_string(),
                exit_code: Some(2),
                stderr_summary: "error: unexpected argument '--ask-for-approval' found".to_string(),
                error: "codex exec 非 0 退出：Some(2)".to_string(),
                ..CodexRun::default()
            },
        )
        .unwrap();

        let list = list_codex_tasks(path_to_string(&root)).unwrap();

        assert_eq!(list.tasks[0].status, "failed");
        assert!(list.runs[0].error.contains("Codex CLI 参数无效"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn user_decision_record_creates_manifest_decision_and_traceable_event() {
        let root = setup_project("user-decision-ledger");

        let event = record_user_decision_event(
            path_to_string(&root),
            "暂不做 ChatGPT MCP".to_string(),
            Some("先验证 Phase A 价值。".to_string()),
        )
        .unwrap();
        let manifest = read_and_repair_manifest(&root).unwrap();
        let ledger = read_work_ledger(&root).unwrap();

        assert!(manifest
            .decisions
            .iter()
            .any(|decision| decision.summary == "暂不做 ChatGPT MCP"));
        assert_eq!(event.fact_kind, "fact");
        assert_eq!(event.decision_trace.decision_type, "decisionCandidate");
        assert!(ledger
            .events
            .iter()
            .any(|item| item.event_type == "user.decisionRecorded"));
        let reloaded = get_work_ledger(path_to_string(&root)).unwrap();
        assert!(reloaded.events.iter().any(|item| item.id == event.id));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn user_decision_record_is_idempotent_for_duplicate_submit() {
        let root = setup_project("user-decision-ledger-dedupe");

        let first = record_user_decision_event(
            path_to_string(&root),
            "暂不做 ChatGPT MCP".to_string(),
            Some("先验证 Phase A 价值。".to_string()),
        )
        .unwrap();
        let second = record_user_decision_event(
            path_to_string(&root),
            "暂不做 ChatGPT MCP".to_string(),
            Some("先验证 Phase A 价值。".to_string()),
        )
        .unwrap();
        let manifest = read_and_repair_manifest(&root).unwrap();
        let ledger = read_work_ledger(&root).unwrap();

        assert_eq!(first.id, second.id);
        assert_eq!(first.source_ref, second.source_ref);
        assert_eq!(
            manifest
                .decisions
                .iter()
                .filter(|decision| decision.summary == "暂不做 ChatGPT MCP")
                .count(),
            1
        );
        assert_eq!(
            ledger
                .events
                .iter()
                .filter(|event| event.event_type == "user.decisionRecorded")
                .count(),
            1
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn chat_decision_capture_requires_explicit_recording_intent() {
        assert_eq!(
            extract_explicit_recorded_decision("我决定采用新的迁移门禁，但先不记录。"),
            None
        );
        assert_eq!(
            extract_explicit_recorded_decision("请记下来：以后先做备份。"),
            None
        );
        assert_eq!(
            extract_explicit_recorded_decision(
                "根据资料，我决定：以后正式库迁移前必须完成备份。请把这个决定记下来。"
            ),
            Some("以后正式库迁移前必须完成备份".to_string())
        );
        assert_eq!(
            extract_explicit_recorded_decision("我确认以后使用影子库，帮我记住这个决定。"),
            Some("以后使用影子库".to_string())
        );
    }

    #[test]
    fn chat_next_step_capture_requires_explicit_recording_intent() {
        let text = "下一步我准备先核对 WTS 当前工作区里的未提交改动分别是什么，再决定是否提交。请把这个下一步记下来。";
        assert_eq!(
            extract_explicit_recorded_next_step(text),
            Some("先核对 WTS 当前工作区里的未提交改动分别是什么，再决定是否提交".to_string())
        );
        assert_eq!(
            extract_explicit_recorded_next_step("下一步先核对工作区，但暂时不记录。"),
            None
        );
        assert_eq!(
            extract_explicit_recorded_next_step("建议下一步核对工作区，请记下来。"),
            None
        );
        assert_eq!(
            extract_explicit_recorded_next_step("AI建议：下一步核对工作区，请记下来。"),
            None
        );
        assert_eq!(
            extract_explicit_recorded_decision(text),
            None,
            "下一步中的‘再决定是否提交’不能触发用户决定记录"
        );
    }

    #[test]
    fn chat_next_step_capture_persists_current_fact_without_decision_or_review() {
        let root = setup_project("chat-next-step-capture");
        let mut manifest = read_and_repair_manifest(&root).unwrap();
        let user_message = workspace_message(
            "user",
            "requirement",
            "下一步我准备先核对 WTS 当前工作区里的未提交改动分别是什么，再决定是否提交。请把这个下一步记下来。",
            "user",
        );
        let next_step = extract_explicit_recorded_next_step(&user_message.text).unwrap();
        manifest.messages.push(user_message.clone());
        let write = record_user_next_step_event_in_manifest(
            &root,
            &mut manifest,
            &next_step,
            None,
            Some(&user_message.id),
        )
        .unwrap();
        assert!(write.is_new);
        persist_project(&root, &manifest, None).unwrap();
        append_work_event_if_new(&root, write.event.clone())
            .unwrap()
            .expect("canonical next-step event should be written");

        let duplicate = record_user_next_step_event_in_manifest(
            &root,
            &mut manifest,
            &next_step,
            None,
            Some(&user_message.id),
        )
        .unwrap();
        assert!(!duplicate.is_new);
        persist_project(&root, &manifest, None).unwrap();

        let reloaded = read_and_repair_manifest(&root).unwrap();
        assert_eq!(reloaded.project.next_step, next_step);
        assert!(reloaded.decisions.is_empty());
        assert!(reloaded.pending_reviews.is_empty());
        let ledger = read_work_ledger(&root).unwrap();
        assert_eq!(
            ledger
                .events
                .iter()
                .filter(|event| event.event_type == "user.nextActionRecorded")
                .count(),
            1
        );

        let packet = get_project_context_packet(path_to_string(&root)).unwrap();
        assert_eq!(packet.next_step, next_step);
        assert!(packet
            .decisions
            .iter()
            .all(|decision| !decision.summary.contains("决定是否提交")));
        assert_eq!(
            packet
                .pending_actions
                .iter()
                .filter(|action| action.title.contains("先核对 WTS"))
                .count(),
            1
        );

        capture_project_fact(
            path_to_string(&root),
            ProjectFactCaptureRequest {
                capture_type: "resolveAction".to_string(),
                action_source_ref: write.event.source_ref.clone(),
                ..ProjectFactCaptureRequest::default()
            },
        )
        .unwrap();
        let cleared = read_and_repair_manifest(&root).unwrap();
        assert!(cleared.project.next_step.is_empty());
        let cleared_packet = get_project_context_packet(path_to_string(&root)).unwrap();
        assert!(cleared_packet.pending_actions.is_empty());
        assert!(cleared_packet.next_step.is_empty());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn finishing_work_replaces_or_clears_current_next_step_without_deleting_history() {
        let root = setup_project("finish-work-next-step");
        let mut manifest = read_and_repair_manifest(&root).unwrap();
        manifest.project.next_step = "旧的下一步".to_string();
        write_json_atomic(&manifest_path(&root), &manifest).unwrap();

        let replaced = finish_project_work(
            path_to_string(&root),
            "完成旧事项".to_string(),
            "新的下一步".to_string(),
        )
        .unwrap();
        assert_eq!(replaced.recovery_point.next_step, "新的下一步");
        assert_eq!(
            read_and_repair_manifest(&root).unwrap().project.next_step,
            "新的下一步"
        );

        let cleared = finish_project_work(
            path_to_string(&root),
            "完成新的下一步".to_string(),
            String::new(),
        )
        .unwrap();
        let reloaded = read_and_repair_manifest(&root).unwrap();
        assert!(reloaded.project.next_step.is_empty());
        assert!(cleared.recovery_point.next_step.is_empty());
        assert_eq!(reloaded.recovery_points.len(), 2);
        assert_eq!(reloaded.recovery_points[0].next_step, "新的下一步");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn chat_decision_capture_persists_canonical_facts_without_current_actions() {
        let root = setup_project("chat-decision-capture");
        let mut manifest = read_and_repair_manifest(&root).unwrap();
        let user_message = workspace_message(
            "user",
            "requirement",
            "根据这份资料，我决定：正式库迁移前必须确认目标库不在禁止列表里，并且已经完成备份或快照。请把这个决定记下来。",
            "user",
        );
        manifest.messages.push(user_message.clone());
        let decision = extract_explicit_recorded_decision(&user_message.text).unwrap();
        let write = record_user_decision_event_in_manifest(
            &root,
            &mut manifest,
            &decision,
            None,
            Some(&user_message.id),
        )
        .unwrap();
        assert!(write.is_new);
        persist_project(&root, &manifest, None).unwrap();
        append_work_event_if_new(&root, write.event.clone())
            .unwrap()
            .expect("canonical decision event should be written");

        let reloaded = read_and_repair_manifest(&root).unwrap();
        let decision_record = reloaded
            .decisions
            .iter()
            .find(|item| item.summary == decision)
            .expect("decision record should survive reload");
        assert_eq!(decision_record.source_message_id, user_message.id);
        assert!(reloaded.project.next_step.is_empty());
        assert!(reloaded.pending_reviews.is_empty());

        let ledger = read_work_ledger(&root).unwrap();
        let event = ledger
            .events
            .iter()
            .find(|item| item.event_type == "user.decisionRecorded")
            .expect("decision ledger event should survive reload");
        assert_eq!(event.project_id, reloaded.project.id);
        assert_eq!(event.decision_trace.recommendation, decision);

        let packet = get_project_context_packet(path_to_string(&root)).unwrap();
        assert!(packet.decisions.iter().any(|item| item.summary == decision));
        assert_eq!(packet.next_step, "");
        assert!(packet.pending_actions.is_empty());

        let mut assistant_manifest = reloaded.clone();
        record_message_derivatives(
            &mut assistant_manifest,
            &workspace_message(
                "ganmaoyuan",
                "assistant",
                "建议确认后再决定是否继续，并记录这次结果。",
                "deepseek",
            ),
        );
        assert_eq!(
            assistant_manifest
                .decisions
                .iter()
                .filter(|item| item.summary.contains("建议确认后再决定"))
                .count(),
            0
        );

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn context_bridge_excludes_legacy_system_scan_decisions() {
        let root = setup_project("context-legacy-system-decision");
        let mut manifest = read_and_repair_manifest(&root).unwrap();
        manifest.decisions.push(DecisionRecord {
            id: "legacy-scan".to_string(),
            summary: "项目目录扫描发现 16091 个待确认事项。".to_string(),
            source_message_id: "system-monitor".to_string(),
            created_at: now_string(),
        });
        manifest.decisions.push(DecisionRecord {
            id: "user-decision".to_string(),
            summary: "保留项目资料目录与代码仓库分离。".to_string(),
            source_message_id: String::new(),
            created_at: now_string(),
        });
        manifest.decisions.push(DecisionRecord {
            id: "legacy-unlinked-decision".to_string(),
            summary: "历史记录中没有用户来源的文本".to_string(),
            source_message_id: String::new(),
            created_at: now_string(),
        });
        manifest.messages.push(workspace_message(
            "ganmaoyuan",
            "assistant",
            "## 已知事实\n\n**AI总结**\n- 这是分析结果。",
            "deepseek",
        ));
        let assistant_message_id = manifest.messages.last().unwrap().id.clone();
        manifest.decisions.push(DecisionRecord {
            id: "legacy-ai-summary".to_string(),
            summary: "## 已知事实".to_string(),
            source_message_id: assistant_message_id,
            created_at: now_string(),
        });
        for (index, summary) in [
            "## 已知事实（依据：WTS_Production_Migration_Manifest.md）",
            "**已知事实**",
            "- **已知事实**",
            "AI总结",
            "建议：把迁移门禁写入 checklist。",
        ]
        .into_iter()
        .enumerate()
        {
            manifest.decisions.push(DecisionRecord {
                id: format!("legacy-analysis-{index}"),
                summary: summary.to_string(),
                source_message_id: String::new(),
                created_at: now_string(),
            });
        }
        let user_message = workspace_message(
            "user",
            "requirement",
            "我决定保留当前目录结构，请把这个决定记下来。",
            "user",
        );
        let user_message_id = user_message.id.clone();
        manifest.messages.push(user_message);
        manifest.decisions.push(DecisionRecord {
            id: "user-message-decision".to_string(),
            summary: "保留当前目录结构".to_string(),
            source_message_id: user_message_id,
            created_at: now_string(),
        });
        write_json_atomic(&manifest_path(&root), &manifest).unwrap();
        append_work_event_if_new(
            &root,
            fact_event(
                &manifest.project.id,
                "user",
                "legacy-user-decision",
                "user.decisionRecorded",
                "用户确认决定：保留项目资料目录与代码仓库分离。".to_string(),
                vec!["decision:user-decision".to_string()],
                100,
            ),
        )
        .unwrap();
        append_work_event_if_new(
            &root,
            fact_event(
                &manifest.project.id,
                "user",
                "legacy-analysis-decision",
                "user.decisionRecorded",
                "用户确认决定：已知事实".to_string(),
                vec!["decision:legacy-analysis-0".to_string()],
                100,
            ),
        )
        .unwrap();

        let packet = get_project_context_packet(path_to_string(&root)).unwrap();

        assert!(packet
            .decisions
            .iter()
            .all(|item| !item.summary.contains("16091")));
        assert!(packet
            .decisions
            .iter()
            .any(|item| item.summary.contains("资料目录")));
        assert!(packet
            .decisions
            .iter()
            .all(|item| !item.summary.contains("没有用户来源")));
        assert!(packet
            .decisions
            .iter()
            .any(|item| item.summary == "保留当前目录结构"));
        assert!(packet
            .decisions
            .iter()
            .all(|item| !item.summary.contains("已知事实")));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn project_context_packet_is_factual_bounded_and_private() {
        let root = setup_project("project-context-packet");
        let mut manifest = read_and_repair_manifest(&root).unwrap();
        manifest.project.name = "青岚工作台".to_string();
        manifest.files.push(FileRecord {
            id: "file-1".to_string(),
            file_name: "验收记录.xlsx".to_string(),
            original_source_path: r"D:\Company\客户资料\验收记录.xlsx".to_string(),
            managed_relative_path: "10_Projects/青岚工作台/验收记录.xlsx".to_string(),
            content_summary: "测试摘要 token=not-for-context".to_string(),
            still_exists: true,
            ..FileRecord::default()
        });
        write_json_atomic(&manifest_path(&root), &manifest).unwrap();
        append_work_event_if_new(
            &root,
            fact_event(
                "p-test",
                "git",
                "head-1",
                "git.headChanged",
                "Git HEAD 已更新，等待验收。".to_string(),
                vec!["commit:1234567".to_string()],
                96,
            ),
        )
        .unwrap();
        let task = CodexTask {
            task_id: "168eb63d-778d-4dcd-8571-984cb92a41c6".to_string(),
            project_id: "p-test".to_string(),
            title: "检查 Workspace 扫描结果".to_string(),
            task_type: "analysis".to_string(),
            status: "awaitingAcceptance".to_string(),
            summary: "已完成扫描结果分析。".to_string(),
            manual_acceptance: vec!["确认桌面扫描结果".to_string()],
            ..CodexTask::default()
        };
        write_codex_task(&root, &task).unwrap();

        let packet = get_project_context_packet(path_to_string(&root)).unwrap();

        assert_eq!(packet.project_name, "青岚工作台");
        assert!(packet.focus.is_some());
        assert!(packet
            .recent_activity
            .iter()
            .any(|item| item.summary.contains("Git HEAD")));
        assert_eq!(packet.files.len(), 1);
        assert_eq!(
            packet.files[0].location,
            "10_Projects/青岚工作台/验收记录.xlsx"
        );
        assert!(!packet.files[0].summary.contains("token="));
        assert!(packet.codex_result.is_some());
        assert!(packet.markdown.contains("当前焦点"));
        assert!(!packet.markdown.contains(r"D:\Company"));
        assert!(!packet
            .markdown
            .contains("168eb63d-778d-4dcd-8571-984cb92a41c6"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn project_context_packet_keeps_projects_isolated_and_handles_sparse_history() {
        let root = setup_project("project-context-packet-sparse");
        let packet = get_project_context_packet(path_to_string(&root)).unwrap();

        assert_eq!(packet.project_id, "p-test");
        assert!(packet.sparse);
        assert!(packet.focus.is_none());
        assert!(packet.markdown.contains("不会据此虚构下一步"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn user_captured_facts_are_idempotent_and_pending_actions_close_by_source() {
        let root = setup_project("user-captured-facts");
        let progress_request = ProjectFactCaptureRequest {
            capture_type: "progress".to_string(),
            content: "已完成真实工作记录闭环验证".to_string(),
            reason: "本机验收".to_string(),
            ..ProjectFactCaptureRequest::default()
        };

        let first = capture_project_fact(path_to_string(&root), progress_request.clone()).unwrap();
        let duplicate = capture_project_fact(path_to_string(&root), progress_request).unwrap();
        assert_eq!(first.id, duplicate.id);

        let blocker = capture_project_fact(
            path_to_string(&root),
            ProjectFactCaptureRequest {
                capture_type: "blocker".to_string(),
                content: "等待业务方确认验收范围".to_string(),
                ..ProjectFactCaptureRequest::default()
            },
        )
        .unwrap();
        let snapshot = get_work_ledger(path_to_string(&root)).unwrap();
        assert!(snapshot
            .activity_timeline
            .iter()
            .any(|activity| activity.activity_type == "userProgress"));
        assert!(snapshot
            .pending_actions
            .iter()
            .any(|action| action.action_type == "userBlocker"));

        capture_project_fact(
            path_to_string(&root),
            ProjectFactCaptureRequest {
                capture_type: "resolveAction".to_string(),
                action_source_ref: blocker.source_ref,
                ..ProjectFactCaptureRequest::default()
            },
        )
        .unwrap();
        let refreshed = get_work_ledger(path_to_string(&root)).unwrap();
        assert!(!refreshed
            .pending_actions
            .iter()
            .any(|action| action.action_type == "userBlocker"));
        assert!(refreshed
            .activity_timeline
            .iter()
            .any(|activity| activity.activity_type == "userActionResolved"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn user_captured_facts_remain_project_scoped() {
        let first_root = setup_project("user-captured-facts-first");
        let second_root = setup_project("user-captured-facts-second");
        let mut second_manifest: ProjectManifest = read_json(&manifest_path(&second_root)).unwrap();
        second_manifest.project.id = "p-second".to_string();
        write_json_atomic(&manifest_path(&second_root), &second_manifest).unwrap();
        let first = capture_project_fact(
            path_to_string(&first_root),
            ProjectFactCaptureRequest {
                capture_type: "nextAction".to_string(),
                content: "完成第一项目验收".to_string(),
                ..ProjectFactCaptureRequest::default()
            },
        )
        .unwrap();
        let second = capture_project_fact(
            path_to_string(&second_root),
            ProjectFactCaptureRequest {
                capture_type: "nextAction".to_string(),
                content: "完成第二项目验收".to_string(),
                ..ProjectFactCaptureRequest::default()
            },
        )
        .unwrap();

        let first_snapshot = get_work_ledger(path_to_string(&first_root)).unwrap();
        let second_snapshot = get_work_ledger(path_to_string(&second_root)).unwrap();
        let first_manifest = read_and_repair_manifest(&first_root).unwrap();
        let second_manifest = read_and_repair_manifest(&second_root).unwrap();
        assert_eq!(first_manifest.project.next_step, "完成第一项目验收");
        assert_eq!(second_manifest.project.next_step, "完成第二项目验收");
        assert!(first_snapshot
            .pending_actions
            .iter()
            .any(|action| action.source_ref == first.source_ref));
        assert!(!first_snapshot
            .pending_actions
            .iter()
            .any(|action| action.source_ref == second.source_ref));
        assert!(second_snapshot
            .pending_actions
            .iter()
            .any(|action| action.source_ref == second.source_ref));
        fs::remove_dir_all(first_root).unwrap();
        fs::remove_dir_all(second_root).unwrap();
    }
}
