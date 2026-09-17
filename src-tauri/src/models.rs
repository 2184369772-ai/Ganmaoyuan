use serde::{Deserialize, Serialize};

pub const MANIFEST_SCHEMA_VERSION: u32 = 6;
pub const ANALYSIS_SCHEMA_VERSION: u32 = 3;
pub const MATERIAL_INBOX_SCHEMA_VERSION: u32 = 2;
pub const MATERIAL_INBOX_ANALYSIS_VERSION: u32 = 6;
pub const WORKSPACE_CONFIG_SCHEMA_VERSION: u32 = 1;
pub const SEARCH_INDEX_SCHEMA_VERSION: u32 = 2;

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceRef {
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub path_snapshot: String,
    #[serde(default)]
    pub hash_snapshot: String,
    #[serde(default)]
    pub label: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FileIdentity {
    #[serde(default)]
    pub file_id: String,
    #[serde(default)]
    pub hash: String,
    #[serde(default)]
    pub original_name: String,
    #[serde(default)]
    pub source_paths: Vec<String>,
    #[serde(default)]
    pub created_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FileMetadata {
    #[serde(default)]
    pub file_type: String,
    #[serde(default)]
    pub document_type: String,
    #[serde(default)]
    pub document_purpose: String,
    #[serde(default)]
    pub business_domain: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub lifecycle_status: String,
    #[serde(default)]
    pub duplicate_of: String,
    #[serde(default)]
    pub version_group_id: String,
    #[serde(default)]
    pub version_number: u32,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FileLocation {
    #[serde(default)]
    pub original_path: String,
    #[serde(default)]
    pub managed_path: String,
    #[serde(default)]
    pub workspace_relative_path: String,
    #[serde(default)]
    pub project_id: String,
    #[serde(default)]
    pub ownership_type: String,
    #[serde(default)]
    pub general_category: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FileProjection {
    #[serde(default)]
    pub identity: FileIdentity,
    #[serde(default)]
    pub metadata: FileMetadata,
    #[serde(default)]
    pub location: FileLocation,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub source_model: String,
    #[serde(default)]
    pub evidence_refs: Vec<EvidenceRef>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LocationPreview {
    #[serde(default)]
    pub preview_id: String,
    #[serde(default)]
    pub source_path: String,
    #[serde(default)]
    pub target_path: String,
    #[serde(default)]
    pub workspace_relative_path: String,
    #[serde(default)]
    pub project_id: String,
    #[serde(default)]
    pub ownership_type: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub reason: String,
}

fn default_schema_version() -> u32 {
    1
}

fn default_workspace_config_schema_version() -> u32 {
    WORKSPACE_CONFIG_SCHEMA_VERSION
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceConfig {
    #[serde(default = "default_workspace_config_schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub workspace_root: String,
    #[serde(default)]
    pub inbox_root: String,
    #[serde(default)]
    pub projects_root: String,
    #[serde(default)]
    pub general_root: String,
    #[serde(default)]
    pub temporary_root: String,
    #[serde(default)]
    pub archive_root: String,
    #[serde(default)]
    pub system_root: String,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub last_verified_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceScanFile {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub file_name: String,
    #[serde(default)]
    pub extension: String,
    #[serde(default)]
    pub size_bytes: u64,
    #[serde(default)]
    pub modified_at: String,
    #[serde(default)]
    pub hash: String,
    #[serde(default)]
    pub scan_batch_id: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub not_suitable_for_ai_context: bool,
    #[serde(default)]
    pub parse_status: String,
    #[serde(default)]
    pub content_summary: String,
    #[serde(default)]
    pub document_type: String,
    #[serde(default)]
    pub document_purpose: String,
    #[serde(default)]
    pub ownership_type: String,
    #[serde(default)]
    pub recommended_project_name: String,
    #[serde(default)]
    pub recommended_location: String,
    #[serde(default)]
    pub recommended_category: String,
    #[serde(default)]
    pub confidence_level: String,
    #[serde(default)]
    pub confidence_score: u8,
    #[serde(default)]
    pub needs_confirmation: bool,
    #[serde(default)]
    pub failure_reason: String,
    #[serde(default)]
    pub decision_traces: Vec<DecisionTrace>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceScanBatch {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub source_directory: String,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub completed_at: String,
    #[serde(default)]
    pub file_count: u64,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub failure_reason: String,
    #[serde(default)]
    pub organizable_count: u64,
    #[serde(default)]
    pub needs_confirmation_count: u64,
    #[serde(default)]
    pub unknown_count: u64,
    #[serde(default)]
    pub files: Vec<WorkspaceScanFile>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceScanDocument {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub batches: Vec<WorkspaceScanBatch>,
    #[serde(default)]
    pub updated_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CleanupPlanItem {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub source_path: String,
    #[serde(default)]
    pub file_name: String,
    #[serde(default)]
    pub hash: String,
    #[serde(default)]
    pub current_location: String,
    #[serde(default)]
    pub recommended_ownership: String,
    #[serde(default)]
    pub recommended_project: String,
    #[serde(default)]
    pub recommended_target_path: String,
    #[serde(default)]
    pub document_purpose: String,
    #[serde(default)]
    pub confidence: DecisionTraceConfidence,
    #[serde(default)]
    pub evidence: Vec<DecisionTraceEvidence>,
    #[serde(default)]
    pub decision_trace_id: String,
    #[serde(default)]
    pub location_preview_id: String,
    #[serde(default)]
    pub required_action: String,
    #[serde(default)]
    pub decision_trace: DecisionTrace,
    #[serde(default)]
    pub decision_traces: Vec<DecisionTrace>,
    #[serde(default)]
    pub review_status: String,
    #[serde(default)]
    pub reviewed_at: String,
    #[serde(default)]
    pub reviewer: String,
    #[serde(default)]
    pub user_reason: String,
    #[serde(default)]
    pub final_project: String,
    #[serde(default)]
    pub final_category: String,
    #[serde(default)]
    pub final_target_path: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CleanupPlan {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub scan_batch_id: String,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub items: Vec<CleanupPlanItem>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CleanupPlanDocument {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub plans: Vec<CleanupPlan>,
    #[serde(default)]
    pub updated_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CleanupPlanReviewFilter {
    #[serde(default)]
    pub recommended_target_path_prefix: String,
    #[serde(default)]
    pub recommended_ownership: String,
    #[serde(default)]
    pub document_purpose: String,
    #[serde(default)]
    pub final_category: String,
    #[serde(default)]
    pub confidence_level: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CleanupExecutionItem {
    #[serde(default)]
    pub cleanup_plan_item_id: String,
    #[serde(default)]
    pub source_path: String,
    #[serde(default)]
    pub target_path: String,
    #[serde(default)]
    pub hash_before: String,
    #[serde(default)]
    pub hash_after: String,
    #[serde(default)]
    pub operation_id: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub error: String,
    #[serde(default)]
    pub global_file_id: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CleanupExecutionBatch {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub cleanup_plan_id: String,
    #[serde(default)]
    pub started_at: String,
    #[serde(default)]
    pub completed_at: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub items: Vec<CleanupExecutionItem>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CleanupExecutionDocument {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub batches: Vec<CleanupExecutionBatch>,
    #[serde(default)]
    pub updated_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSummary {
    pub id: String,
    pub name: String,
    pub root_dir: String,
    pub manifest_path: String,
    pub last_opened_at: String,
    pub next_step: String,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub description: String,
    /// A project can keep its working material outside the code repository.
    /// This is the verified repository association used by Git and Codex flows.
    #[serde(default)]
    pub repository_path: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FileRecord {
    pub id: String,
    pub file_name: String,
    pub original_source_path: String,
    pub managed_path: String,
    pub file_type: String,
    pub category: String,
    pub created_source: String,
    pub related_task: String,
    pub current_version: String,
    #[serde(default)]
    pub history_versions: Vec<String>,
    pub created_at: String,
    pub modified_at: String,
    pub still_exists: bool,
    pub needs_user_confirmation: bool,
    #[serde(default)]
    pub parse_status: String,
    #[serde(default)]
    pub content_summary: String,
    #[serde(default)]
    pub main_fields_or_sections: Vec<String>,
    #[serde(default)]
    pub recommended_category: String,
    #[serde(default)]
    pub parse_failure_reason: String,
    #[serde(default)]
    pub parser: String,
    #[serde(default)]
    pub extracted_text_path: String,
    #[serde(default)]
    pub page_count: Option<u32>,
    #[serde(default)]
    pub sheet_count: Option<u32>,
    #[serde(default)]
    pub row_count: Option<u64>,
    #[serde(default)]
    pub column_count: Option<u64>,
    #[serde(default)]
    pub analysis_warnings: Vec<String>,
    #[serde(default)]
    pub content_hash: String,
    #[serde(default)]
    pub size_bytes: u64,
    #[serde(default)]
    pub version_group_id: String,
    #[serde(default)]
    pub duplicate_of_file_id: Option<String>,
    #[serde(default)]
    pub previous_version_id: Option<String>,
    #[serde(default)]
    pub source_paths: Vec<String>,
    #[serde(default)]
    pub managed_relative_path: String,
    #[serde(default)]
    pub location_reason: String,
    #[serde(default)]
    pub import_transaction_id: String,
    #[serde(default)]
    pub last_verified_at: String,
    #[serde(default)]
    pub original_file_name: String,
    #[serde(default)]
    pub document_family_id: String,
    #[serde(default)]
    pub version_id: String,
    #[serde(default)]
    pub version_number: u32,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FileAnalysis {
    pub file_id: String,
    pub file_name: String,
    pub managed_path: String,
    pub parse_status: String,
    pub content_summary: String,
    #[serde(default)]
    pub main_fields_or_sections: Vec<String>,
    pub recommended_category: String,
    pub parse_failure_reason: String,
    pub extracted_at: String,
    #[serde(default)]
    pub parser: String,
    #[serde(default)]
    pub content_hash: String,
    #[serde(default)]
    pub extracted_text_path: String,
    #[serde(default)]
    pub page_count: Option<u32>,
    #[serde(default)]
    pub sheet_count: Option<u32>,
    #[serde(default)]
    pub row_count: Option<u64>,
    #[serde(default)]
    pub column_count: Option<u64>,
    #[serde(default)]
    pub warnings: Vec<String>,
    // 业务语义识别（V3 inbox 冲刺新增；旧数据缺省为空串，向后兼容）
    #[serde(default)]
    pub document_type: String,
    #[serde(default)]
    pub business_domain: String,
    #[serde(default)]
    pub business_purpose: String,
    #[serde(default)]
    pub business_summary: String,
    #[serde(default)]
    pub document_purpose: String,
    #[serde(default)]
    pub document_purpose_confidence: u8,
    #[serde(default)]
    pub document_purpose_evidence: Vec<String>,
    #[serde(default)]
    pub technical_detail: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DataHealthRecord {
    pub id: String,
    #[serde(default, rename = "type")]
    pub health_type: String,
    #[serde(default)]
    pub severity: String,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub suggested_action: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub created_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct FileAnalysisDocument {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub files: Vec<FileAnalysis>,
    #[serde(default)]
    pub updated_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MessageAttachment {
    pub file_id: String,
    pub file_name: String,
    pub managed_path: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MessageEvidenceItem {
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub content_type: String,
    #[serde(default)]
    pub basis_kind: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub managed_path: String,
    #[serde(default)]
    pub source_record_id: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceMessage {
    pub id: String,
    pub author: String,
    pub kind: String,
    pub text: String,
    pub created_at: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub attachments: Vec<MessageAttachment>,
    #[serde(default)]
    pub related_task: String,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub model_id: String,
    #[serde(default)]
    pub evidence_items: Vec<MessageEvidenceItem>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TaskRecord {
    pub id: String,
    pub title: String,
    pub status: String,
    pub source_message_id: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DecisionRecord {
    pub id: String,
    pub summary: String,
    pub source_message_id: String,
    pub created_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactRecord {
    pub id: String,
    pub title: String,
    pub artifact_type: String,
    pub source: String,
    pub source_message_id: String,
    pub file_id: String,
    pub managed_path: String,
    pub related_task: String,
    pub created_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CodexPromptRecord {
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub related_task: String,
    #[serde(default)]
    pub prompt_text: String,
    #[serde(default)]
    pub file_id: String,
    #[serde(default)]
    pub managed_path: String,
    #[serde(default)]
    pub content_hash: String,
    #[serde(default)]
    pub created_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CodexReportRecord {
    pub id: String,
    #[serde(default)]
    pub source_kind: String,
    #[serde(default)]
    pub source_label: String,
    #[serde(default)]
    pub report_hash: String,
    #[serde(default)]
    pub evidence_file_id: String,
    #[serde(default)]
    pub evidence_managed_path: String,
    #[serde(default)]
    pub parse_status: String,
    #[serde(default)]
    pub completed_status: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub modified_content: Vec<String>,
    #[serde(default)]
    pub test_results: Vec<String>,
    #[serde(default)]
    pub commit: String,
    #[serde(default)]
    pub unresolved_issues: Vec<String>,
    #[serde(default)]
    pub next_step_suggestions: Vec<String>,
    #[serde(default)]
    pub parse_failure_reason: String,
    #[serde(default)]
    pub duplicate_of_report_id: Option<String>,
    #[serde(default)]
    pub applied: bool,
    #[serde(default)]
    pub confirmed_at: String,
    #[serde(default)]
    pub created_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DailySession {
    pub id: String,
    pub date_key: String,
    pub started_at: String,
    pub updated_at: String,
    #[serde(default)]
    pub message_ids: Vec<String>,
    #[serde(default)]
    pub task_ids: Vec<String>,
    #[serde(default)]
    pub decision_ids: Vec<String>,
    #[serde(default)]
    pub artifact_ids: Vec<String>,
    #[serde(default)]
    pub imported_file_ids: Vec<String>,
    #[serde(default)]
    pub recovery_point_ids: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LocationDecision {
    pub id: String,
    pub file_name: String,
    pub source_path: String,
    pub managed_path: String,
    pub managed_relative_path: String,
    pub category: String,
    pub file_type: String,
    pub created_source: String,
    pub related_task: String,
    pub purpose: String,
    pub version: String,
    pub reason: String,
    pub requires_confirmation: bool,
    pub created_at: String,
    #[serde(default)]
    pub operation_id: String,
    #[serde(default)]
    pub location_preview_id: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InboxStatusTransition {
    #[serde(default)]
    pub from: String,
    #[serde(default)]
    pub to: String,
    #[serde(default)]
    pub changed_at: String,
    #[serde(default)]
    pub reason: String,
    #[serde(default)]
    pub actor: String,
    #[serde(default)]
    pub error_code: String,
    #[serde(default)]
    pub error_message: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InboxSourceEvent {
    #[serde(default)]
    pub source_path: String,
    #[serde(default)]
    pub received_from: String,
    #[serde(default)]
    pub received_at: String,
    #[serde(default)]
    pub hash: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InboxProjectCandidate {
    #[serde(default)]
    pub candidate_project_id: String,
    #[serde(default)]
    pub candidate_project_name: String,
    #[serde(default)]
    pub candidate_project_root: String,
    #[serde(default)]
    pub score: u8,
    #[serde(default)]
    pub confidence: String,
    #[serde(default)]
    pub reasons: Vec<String>,
    #[serde(default)]
    pub evidence: Vec<String>,
    #[serde(default)]
    pub matched_signals: Vec<String>,
    #[serde(default)]
    pub rule_score: u8,
    #[serde(default)]
    pub ai_score: Option<u8>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InboxRouteOperation {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub project_id: String,
    #[serde(default)]
    pub project_root: String,
    #[serde(default)]
    pub source_path: String,
    #[serde(default)]
    pub target_path: String,
    #[serde(default)]
    pub managed_file_id: String,
    #[serde(default)]
    pub source_hash: String,
    #[serde(default)]
    pub target_hash_at_route: String,
    #[serde(default)]
    pub created_managed_copy: bool,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub undone_at: String,
    #[serde(default)]
    pub failure_reason: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InboxRoutingSettings {
    #[serde(default)]
    pub auto_route_high_confidence: bool,
    #[serde(default)]
    pub updated_at: String,
}

impl Default for InboxRoutingSettings {
    fn default() -> Self {
        Self {
            auto_route_high_confidence: false,
            updated_at: String::new(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PendingReviewItem {
    pub id: String,
    pub key: String,
    pub kind: String,
    pub title: String,
    pub detail: String,
    #[serde(default)]
    pub file_id: String,
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub suggested_managed_path: String,
    #[serde(default)]
    pub suggested_category: String,
    #[serde(default)]
    pub status: String,
    pub detected_at: String,
    #[serde(default)]
    pub updated_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MonitoringState {
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub last_scanned_at: String,
    #[serde(default)]
    pub last_event_at: String,
    #[serde(default)]
    pub last_error: String,
    #[serde(default)]
    pub watch_started_at: String,
    #[serde(default)]
    pub pending_count: usize,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AtlasAssessment {
    #[serde(default)]
    pub atlas_version: String,
    #[serde(default = "default_atlas_status")]
    pub status: String,
    #[serde(default)]
    pub reusable_parts: Vec<String>,
    #[serde(default)]
    pub uncovered_parts: Vec<String>,
    #[serde(default)]
    pub training_candidates: Vec<String>,
    #[serde(default)]
    pub evidence_references: Vec<String>,
    #[serde(default = "default_true")]
    pub review_required: bool,
    #[serde(default)]
    pub assessed_at: String,
    #[serde(default)]
    pub failure_reason: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AtlasSkillAssessment {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub skill_name: String,
    #[serde(default)]
    pub matched_capability: String,
    #[serde(default)]
    pub reusable_evidence: Vec<String>,
    #[serde(default)]
    pub uncovered_part: String,
    #[serde(default)]
    pub similarity: f32,
    #[serde(default)]
    pub confidence: DecisionTraceConfidence,
    #[serde(default = "default_true")]
    pub review_required: bool,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub assessed_at: String,
    #[serde(default)]
    pub evidence_references: Vec<String>,
    #[serde(default)]
    pub decision_trace: DecisionTrace,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectAnalysis {
    #[serde(default = "default_analysis_status")]
    pub status: String,
    #[serde(default)]
    pub project_definition: String,
    #[serde(default)]
    pub goals: Vec<String>,
    #[serde(default)]
    pub roles: Vec<String>,
    #[serde(default)]
    pub material_usage: Vec<String>,
    #[serde(default)]
    pub known_requirements: Vec<String>,
    #[serde(default)]
    pub gaps: Vec<String>,
    #[serde(default)]
    pub questions: Vec<String>,
    #[serde(default)]
    pub constraints: Vec<String>,
    #[serde(default)]
    pub evidence: Vec<String>,
    #[serde(default)]
    pub next_steps: Vec<String>,
    #[serde(default)]
    pub updated_at: String,
    #[serde(default)]
    pub model_id: String,
    #[serde(default)]
    pub failure_reason: String,
}

fn default_analysis_status() -> String {
    "unavailable".to_string()
}

fn default_atlas_status() -> String {
    "waiting_for_integration".to_string()
}

fn default_true() -> bool {
    true
}

fn default_basis_kind() -> String {
    "inference".to_string()
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AuditEvent {
    pub id: String,
    pub action: String,
    pub target: String,
    pub outcome: String,
    pub created_at: String,
    #[serde(default)]
    pub details: String,
    #[serde(default)]
    pub requires_confirmation: bool,
    #[serde(default)]
    pub confirmed: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DecisionTraceEvidence {
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub source_id: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DecisionTraceConfidence {
    #[serde(default)]
    pub score: u8,
    #[serde(default)]
    pub level: String,
    #[serde(default)]
    pub display: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DecisionTrace {
    pub id: String,
    #[serde(rename = "type", default)]
    pub decision_type: String,
    #[serde(default)]
    pub subject_id: String,
    #[serde(default)]
    pub project_id: String,
    #[serde(default)]
    pub input_evidence: Vec<DecisionTraceEvidence>,
    #[serde(default)]
    pub ai_understanding: String,
    #[serde(default)]
    pub recommendation: String,
    #[serde(default)]
    pub confidence: DecisionTraceConfidence,
    #[serde(default = "default_pending_decision")]
    pub user_decision: String,
    #[serde(default)]
    pub user_decision_note: String,
    #[serde(default = "default_pending_execution")]
    pub execution: String,
    #[serde(default)]
    pub execution_note: String,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
}

fn default_pending_decision() -> String {
    "pending".to_string()
}

fn default_pending_execution() -> String {
    "pending".to_string()
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct ProjectManifest {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    pub project: ProjectSummary,
    #[serde(default)]
    pub files: Vec<FileRecord>,
    #[serde(default)]
    pub messages: Vec<WorkspaceMessage>,
    #[serde(default)]
    pub tasks: Vec<TaskRecord>,
    #[serde(default)]
    pub decisions: Vec<DecisionRecord>,
    #[serde(default)]
    pub artifacts: Vec<ArtifactRecord>,
    #[serde(default)]
    pub codex_prompts: Vec<CodexPromptRecord>,
    #[serde(default)]
    pub codex_reports: Vec<CodexReportRecord>,
    #[serde(default)]
    pub daily_sessions: Vec<DailySession>,
    #[serde(default)]
    pub location_decisions: Vec<LocationDecision>,
    #[serde(default)]
    pub pending_reviews: Vec<PendingReviewItem>,
    #[serde(default)]
    pub monitoring: MonitoringState,
    #[serde(default)]
    pub atlas: AtlasAssessment,
    #[serde(default)]
    pub project_analysis: ProjectAnalysis,
    #[serde(default)]
    pub audit: Vec<AuditEvent>,
    #[serde(default)]
    pub draft: WorkspaceDraft,
    #[serde(default)]
    pub recovery_points: Vec<RecoveryPoint>,
    #[serde(default)]
    pub deepseek_authorization: DeepSeekAuthorization,
    #[serde(default)]
    pub project_impact_analyses: Vec<ProjectImpactAnalysis>,
    #[serde(default)]
    pub project_action_candidates: Vec<ProjectActionCandidate>,
    #[serde(default)]
    pub project_state_proposals: Vec<ProjectStateProposal>,
    #[serde(default)]
    pub daily_continue_snapshots: Vec<DailyContinueSnapshot>,
    #[serde(default)]
    pub project_state_summary: ProjectStateSummary,
    #[serde(default)]
    pub project_attentions: Vec<ProjectAttention>,
    #[serde(default)]
    pub work_pattern_candidates: Vec<WeeklySkillCandidate>,
    #[serde(default)]
    pub project_state_auto_apply: bool,
    #[serde(default)]
    pub data_health_records: Vec<DataHealthRecord>,
    #[serde(default)]
    pub atlas_skill_assessments: Vec<AtlasSkillAssessment>,
    #[serde(default)]
    pub skill_library: Vec<PersonalSkill>,
    #[serde(default)]
    pub knowledge_pattern_candidates: Vec<KnowledgePatternCandidate>,
    #[serde(default)]
    pub improvement_candidates: Vec<ImprovementCandidate>,
    #[serde(default)]
    pub v1_readiness: V1Readiness,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AppRegistry {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub projects: Vec<ProjectSummary>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MaterialInboxItem {
    pub id: String,
    #[serde(default)]
    pub file_name: String,
    #[serde(default)]
    pub source_path: String,
    #[serde(default)]
    pub received_at: String,
    #[serde(default)]
    pub received_from: String,
    #[serde(default)]
    pub source_hash: String,
    #[serde(default)]
    pub file_type: String,
    #[serde(default)]
    pub size_bytes: u64,
    #[serde(default)]
    pub parse_status: String,
    #[serde(default)]
    pub content_summary: String,
    #[serde(default)]
    pub analysis_result: String,
    #[serde(default)]
    pub main_fields_or_sections: Vec<String>,
    #[serde(default)]
    pub recommended_category: String,
    #[serde(default)]
    pub recommended_project_name: String,
    #[serde(default)]
    pub recommended_location: String,
    #[serde(default)]
    pub parse_failure_reason: String,
    #[serde(default)]
    pub parser: String,
    #[serde(default)]
    pub judgement_status: String,
    #[serde(default)]
    pub target_project_id: String,
    #[serde(default)]
    pub target_project_root: String,
    #[serde(default)]
    pub target_project_name: String,
    #[serde(default)]
    pub suggested_managed_path: String,
    #[serde(default)]
    pub suggested_category: String,
    #[serde(default)]
    pub duplicate_of_file_id: String,
    #[serde(default)]
    pub previous_version_file_id: String,
    #[serde(default)]
    pub proposed_project_name: String,
    #[serde(default)]
    pub decision_basis: Vec<String>,
    #[serde(default)]
    pub processing_status: String,
    #[serde(default)]
    pub result_note: String,
    #[serde(default)]
    pub managed_file_id: String,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
    #[serde(default)]
    pub status_history: Vec<InboxStatusTransition>,
    #[serde(default)]
    pub last_transition_at: String,
    #[serde(default)]
    pub error_code: String,
    #[serde(default)]
    pub error_message: String,
    #[serde(default)]
    pub failed_stage: String,
    #[serde(default)]
    pub project_candidates: Vec<InboxProjectCandidate>,
    #[serde(default)]
    pub confidence_level: String,
    #[serde(default)]
    pub confidence_score: u8,
    #[serde(default)]
    pub confidence_reasons: Vec<String>,
    #[serde(default)]
    pub recommended_relative_location: String,
    #[serde(default)]
    pub location_reason: String,
    #[serde(default)]
    pub suggested_file_name: String,
    #[serde(default)]
    pub duplicate_kind: String,
    #[serde(default)]
    pub document_family_id: String,
    #[serde(default)]
    pub version_id: String,
    #[serde(default)]
    pub version_number: u32,
    #[serde(default)]
    pub received_count: u32,
    #[serde(default)]
    pub source_history: Vec<InboxSourceEvent>,
    #[serde(default)]
    pub route_operation: Option<InboxRouteOperation>,
    #[serde(default)]
    pub auto_routed: bool,
    #[serde(default)]
    pub can_undo: bool,
    #[serde(default)]
    pub ai_match_status: String,
    #[serde(default)]
    pub ai_match_model: String,
    // V3 inbox 冲刺新增：一级归属类型（existingProject/generalWorkMaterial/
    // newProjectCandidate/temporaryOrReference/needsReview/unsupportedOrFailed）
    #[serde(default)]
    pub ownership_type: String,
    #[serde(default)]
    pub document_type: String,
    #[serde(default)]
    pub business_domain: String,
    #[serde(default)]
    pub business_purpose: String,
    #[serde(default)]
    pub business_summary: String,
    #[serde(default)]
    pub document_purpose: String,
    #[serde(default)]
    pub document_purpose_confidence: u8,
    #[serde(default)]
    pub document_purpose_evidence: Vec<String>,
    #[serde(default)]
    pub technical_detail: String,
    #[serde(default)]
    pub analysis_version: u32,
    #[serde(default)]
    pub general_material_domain: String,
    #[serde(default)]
    pub general_material_category: String,
    #[serde(default)]
    pub material_semantic_type: String,
    #[serde(default)]
    pub project_candidate_score: u8,
    #[serde(default)]
    pub project_candidate_reasons: Vec<String>,
    #[serde(default)]
    pub project_candidate_evidence: Vec<String>,
    #[serde(default)]
    pub project_candidate_confidence: String,
    #[serde(default)]
    pub decision_traces: Vec<DecisionTrace>,
    // 全局归位信息（routeInboxItemToGlobal 使用）
    #[serde(default)]
    pub global_destination: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct MaterialInboxDocument {
    #[serde(default = "default_material_inbox_schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub items: Vec<MaterialInboxItem>,
    #[serde(default)]
    pub updated_at: String,
}

fn default_material_inbox_schema_version() -> u32 {
    MATERIAL_INBOX_SCHEMA_VERSION
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ProjectCreateResult {
    pub project: ProjectSummary,
    pub files: Vec<FileRecord>,
    pub messages: Vec<WorkspaceMessage>,
    pub atlas: AtlasAssessment,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ImportResult {
    pub files: Vec<FileRecord>,
    pub messages: Vec<WorkspaceMessage>,
    pub atlas: AtlasAssessment,
    #[serde(default)]
    pub duplicates: Vec<FileRecord>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct MaterialInboxRouteResult {
    pub item: MaterialInboxItem,
    #[serde(default)]
    pub project: Option<ProjectSummary>,
    #[serde(default)]
    pub files: Vec<FileRecord>,
}

// 全局（非项目）受管文件登记。原文件从不移动；受管副本位于全局数据目录下。
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct GlobalManagedFile {
    pub id: String,
    #[serde(default)]
    pub file_name: String,
    #[serde(default)]
    pub source_path: String,
    #[serde(default)]
    pub managed_path: String,
    #[serde(default)]
    pub managed_relative_path: String,
    #[serde(default)]
    pub ownership_type: String,
    #[serde(default)]
    pub destination: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub related_project: String,
    #[serde(default)]
    pub general_material_domain: String,
    #[serde(default)]
    pub general_material_category: String,
    #[serde(default)]
    pub material_semantic_type: String,
    #[serde(default)]
    pub file_type: String,
    #[serde(default)]
    pub content_hash: String,
    #[serde(default)]
    pub document_type: String,
    #[serde(default)]
    pub business_domain: String,
    #[serde(default)]
    pub business_purpose: String,
    #[serde(default)]
    pub business_summary: String,
    #[serde(default)]
    pub document_purpose: String,
    #[serde(default)]
    pub document_purpose_confidence: u8,
    #[serde(default)]
    pub document_purpose_evidence: Vec<String>,
    #[serde(default)]
    pub content_summary: String,
    #[serde(default)]
    pub inbox_item_id: String,
    #[serde(default)]
    pub operation_id: String,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub can_undo: bool,
    #[serde(default)]
    pub undone_at: String,
    #[serde(default)]
    pub lifecycle_status: String,
    #[serde(default)]
    pub decision_trace_id: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct GlobalFilesDocument {
    #[serde(default)]
    pub schema_version: u32,
    #[serde(default)]
    pub files: Vec<GlobalManagedFile>,
    #[serde(default)]
    pub updated_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GeneratedFileResult {
    pub file: FileRecord,
    pub message: WorkspaceMessage,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CodexPromptResult {
    pub prompt: CodexPromptRecord,
    pub manifest: ProjectManifest,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CodexReportImportResult {
    pub report: CodexReportRecord,
    pub manifest: ProjectManifest,
    #[serde(default)]
    pub duplicate: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CodexResultBridgeScanResult {
    pub result_dir: String,
    pub scanned_count: usize,
    pub imported_count: usize,
    #[serde(default)]
    pub reports: Vec<CodexReportRecord>,
    pub manifest: ProjectManifest,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CodexReportApplyResult {
    pub report: CodexReportRecord,
    pub manifest: ProjectManifest,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GlobalSearchResult {
    pub id: String,
    #[serde(default)]
    pub file_id: String,
    #[serde(default)]
    pub hash: String,
    #[serde(default)]
    pub project_id: String,
    #[serde(default)]
    pub project_root: String,
    #[serde(default)]
    pub project_name: String,
    #[serde(default)]
    pub content_type: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub snippet: String,
    #[serde(default)]
    pub updated_at: String,
    #[serde(default)]
    pub managed_path: String,
    #[serde(default)]
    pub workspace_relative_path: String,
    #[serde(default)]
    pub file_type: String,
    #[serde(default)]
    pub ownership_type: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub document_type: String,
    #[serde(default)]
    pub document_purpose: String,
    #[serde(default)]
    pub business_domain: String,
    #[serde(default)]
    pub lifecycle_status: String,
    #[serde(default)]
    pub duplicate_of: String,
    #[serde(default)]
    pub version_group_id: String,
    #[serde(default)]
    pub version_number: u32,
    #[serde(default)]
    pub evidence_refs: Vec<EvidenceRef>,
    #[serde(default)]
    pub decision_trace_id: String,
    #[serde(default)]
    pub recent_status: String,
    #[serde(default)]
    pub confidence_display: String,
    #[serde(default)]
    pub matched_field: String,
    #[serde(default)]
    pub match_snippet: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SearchIndexEntry {
    pub id: String,
    #[serde(default)]
    pub file_id: String,
    #[serde(default)]
    pub hash: String,
    #[serde(default)]
    pub project_id: String,
    #[serde(default)]
    pub project_root: String,
    #[serde(default)]
    pub project_name: String,
    #[serde(default)]
    pub content_type: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub updated_at: String,
    #[serde(default)]
    pub managed_path: String,
    #[serde(default)]
    pub workspace_relative_path: String,
    #[serde(default)]
    pub file_type: String,
    #[serde(default)]
    pub ownership_type: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub document_type: String,
    #[serde(default)]
    pub document_purpose: String,
    #[serde(default)]
    pub business_domain: String,
    #[serde(default)]
    pub lifecycle_status: String,
    #[serde(default)]
    pub duplicate_of: String,
    #[serde(default)]
    pub version_group_id: String,
    #[serde(default)]
    pub version_number: u32,
    #[serde(default)]
    pub evidence_refs: Vec<EvidenceRef>,
    #[serde(default)]
    pub decision_trace_id: String,
    #[serde(default)]
    pub recent_status: String,
    #[serde(default)]
    pub confidence_display: String,
    #[serde(default)]
    pub haystack: String,
    #[serde(default)]
    pub indexed_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SearchIndexDocument {
    #[serde(default)]
    pub schema_version: u32,
    #[serde(default)]
    pub entries: Vec<SearchIndexEntry>,
    #[serde(default)]
    pub updated_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct MessageResult {
    pub project: ProjectSummary,
    pub messages: Vec<WorkspaceMessage>,
    #[serde(default)]
    pub stream_message_id: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FinishWorkResult {
    pub project: ProjectSummary,
    pub message: WorkspaceMessage,
    pub recovery_point: RecoveryPoint,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MemoItem {
    pub id: String,
    pub text: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceDraft {
    pub project_id: String,
    pub text: String,
    #[serde(default)]
    pub pending_file_paths: Vec<String>,
    pub updated_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryPoint {
    pub id: String,
    pub project_id: String,
    pub completed: String,
    pub next_step: String,
    pub created_at: String,
    pub message_count: usize,
    pub file_count: usize,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectImpactEvidence {
    #[serde(default)]
    pub basis_kind: String,
    #[serde(default)]
    pub file_id: String,
    #[serde(default)]
    pub file_name: String,
    #[serde(default)]
    pub section: String,
    #[serde(default)]
    pub excerpt: String,
    #[serde(default)]
    pub source_hash: String,
    #[serde(default)]
    pub granularity: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectImpactFinding {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub finding_type: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub confidence: f32,
    #[serde(default)]
    pub evidence: Vec<ProjectImpactEvidence>,
    #[serde(default)]
    pub source_reference: String,
    #[serde(default)]
    pub suggested_action: String,
    #[serde(default = "default_basis_kind")]
    pub basis_kind: String,
    #[serde(default = "default_true")]
    pub review_required: bool,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub decision_trace: DecisionTrace,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectImpactAnalysis {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub source_file_id: String,
    #[serde(default)]
    pub source_hash: String,
    #[serde(default)]
    pub project_id: String,
    #[serde(default)]
    pub generated_at: String,
    #[serde(default)]
    pub analysis_version: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub relevance: String,
    #[serde(default)]
    pub impact_level: String,
    #[serde(default)]
    pub findings: Vec<ProjectImpactFinding>,
    #[serde(default)]
    pub evidence: Vec<ProjectImpactEvidence>,
    #[serde(default)]
    pub confidence: f32,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub failure_reason: String,
    #[serde(default)]
    pub decision_trace: DecisionTrace,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectActionCandidate {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub candidate_type: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub suggested_priority: String,
    #[serde(default)]
    pub suggested_due_date: String,
    #[serde(default)]
    pub related_file_id: String,
    #[serde(default)]
    pub evidence: Vec<ProjectImpactEvidence>,
    #[serde(default)]
    pub confidence: f32,
    #[serde(default = "default_true")]
    pub review_required: bool,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub source_impact_id: String,
    #[serde(default)]
    pub source_hash: String,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
    #[serde(default)]
    pub applied_record_id: String,
    #[serde(default)]
    pub decision_trace: DecisionTrace,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectStateChange {
    #[serde(default)]
    pub field: String,
    #[serde(default)]
    pub before: String,
    #[serde(default)]
    pub after: String,
    #[serde(default)]
    pub reason: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectStateProposal {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub before: Vec<ProjectStateChange>,
    #[serde(default)]
    pub proposed_changes: Vec<ProjectStateChange>,
    #[serde(default)]
    pub evidence: Vec<ProjectImpactEvidence>,
    #[serde(default)]
    pub confidence: f32,
    #[serde(default)]
    pub source_file_ids: Vec<String>,
    #[serde(default)]
    pub source_hashes: Vec<String>,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub applied_at: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub previous_next_step: String,
    #[serde(default)]
    pub decision_trace: DecisionTrace,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DailyContinueItem {
    #[serde(default)]
    pub action: String,
    #[serde(default)]
    pub reason: String,
    #[serde(default)]
    pub evidence: Vec<String>,
    #[serde(default)]
    pub related_project: String,
    #[serde(default)]
    pub related_files: Vec<String>,
    #[serde(default)]
    pub priority: String,
    #[serde(default)]
    pub item_type: String,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub confidence: DecisionTraceConfidence,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub decision_trace: DecisionTrace,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectStateSummary {
    #[serde(default)]
    pub project_id: String,
    #[serde(default)]
    pub current_phase: String,
    #[serde(default)]
    pub recent_progress: Vec<String>,
    #[serde(default)]
    pub current_risks: Vec<String>,
    #[serde(default)]
    pub blockers: Vec<String>,
    #[serde(default)]
    pub next_milestone: String,
    #[serde(default)]
    pub recent_changes: Vec<String>,
    #[serde(default)]
    pub facts: Vec<String>,
    #[serde(default)]
    pub inferences: Vec<String>,
    #[serde(default)]
    pub suggestions: Vec<String>,
    #[serde(default)]
    pub evidence: Vec<DecisionTraceEvidence>,
    #[serde(default)]
    pub generated_at: String,
    #[serde(default)]
    pub decision_trace: DecisionTrace,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectAttention {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub project_id: String,
    #[serde(default)]
    pub attention_type: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub reason: String,
    #[serde(default)]
    pub evidence: Vec<DecisionTraceEvidence>,
    #[serde(default)]
    pub suggested_action: String,
    #[serde(default)]
    pub confidence: DecisionTraceConfidence,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
    #[serde(default)]
    pub decision_trace: DecisionTrace,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TodayProjectFocus {
    #[serde(default)]
    pub project_id: String,
    #[serde(default)]
    pub project_name: String,
    #[serde(default)]
    pub project_root: String,
    #[serde(default)]
    pub current_status: String,
    #[serde(default)]
    pub recent_change: String,
    #[serde(default)]
    pub next_step: String,
    #[serde(default)]
    pub updated_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TodayBlocker {
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub related_project: String,
    #[serde(default)]
    pub evidence: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TodayPendingItem {
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub related_project: String,
    #[serde(default)]
    pub source_id: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ContinueWorkPrimaryAction {
    #[serde(rename = "type", default)]
    pub action_type: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub project_id: String,
    #[serde(default)]
    pub project_root: String,
    #[serde(default)]
    pub source_id: String,
    #[serde(default)]
    pub panel: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ContinueWorkFocus {
    #[serde(default)]
    pub project_id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub reason: String,
    #[serde(default)]
    pub priority: String,
    #[serde(default)]
    pub confidence: DecisionTraceConfidence,
    #[serde(default)]
    pub freshness_status: String,
    #[serde(default)]
    pub evidence_refs: Vec<EvidenceRef>,
    #[serde(default)]
    pub source_action_ids: Vec<String>,
    #[serde(default)]
    pub primary_action: ContinueWorkPrimaryAction,
    #[serde(default)]
    pub generated_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceActivityItem {
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub project_name: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub managed_path: String,
    #[serde(default)]
    pub decision_trace_id: String,
    #[serde(default)]
    pub occurred_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceActivitySummary {
    #[serde(default)]
    pub new_managed_files: usize,
    #[serde(default)]
    pub project_files: usize,
    #[serde(default)]
    pub general_files: usize,
    #[serde(default)]
    pub temporary_files: usize,
    #[serde(default)]
    pub pending_cleanup_plans: usize,
    #[serde(default)]
    pub failed_items: usize,
    #[serde(default)]
    pub conflicts: usize,
    #[serde(default)]
    pub versioned_files: usize,
    #[serde(default)]
    pub recent_items: Vec<WorkspaceActivityItem>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TodayWorkspace {
    #[serde(default)]
    pub generated_at: String,
    #[serde(default)]
    pub continue_work_focus: Option<ContinueWorkFocus>,
    #[serde(default)]
    pub last_progress: Vec<String>,
    #[serde(default)]
    pub focus_projects: Vec<TodayProjectFocus>,
    #[serde(default)]
    pub recommended_actions: Vec<DailyContinueItem>,
    #[serde(default)]
    pub blockers: Vec<TodayBlocker>,
    #[serde(default)]
    pub pending_items: Vec<TodayPendingItem>,
    #[serde(default)]
    pub workspace_activity: WorkspaceActivitySummary,
    #[serde(default)]
    pub codex_task_summary: CodexTaskTodaySummary,
    #[serde(default)]
    pub activity_timeline: Vec<ActivityProjection>,
    #[serde(default)]
    pub pending_actions: Vec<PendingActionProjection>,
    #[serde(default)]
    pub executions: Vec<ExecutionRecord>,
    #[serde(default)]
    pub status: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CodexTaskTodaySummary {
    #[serde(default)]
    pub active_or_delivered: usize,
    #[serde(default)]
    pub awaiting_acceptance: usize,
    #[serde(default)]
    pub abnormal: usize,
    #[serde(default)]
    pub latest_tasks: Vec<CodexTaskTodayItem>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CodexTaskTodayItem {
    #[serde(default)]
    pub task_id: String,
    #[serde(default)]
    pub project_id: String,
    #[serde(default)]
    pub project_name: String,
    #[serde(default)]
    pub project_root: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub git_status: String,
    #[serde(default)]
    pub manual_acceptance_count: usize,
    #[serde(default)]
    pub updated_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WorkEvent {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub project_id: String,
    #[serde(default)]
    pub source_type: String,
    #[serde(default)]
    pub source_ref: String,
    #[serde(default)]
    pub event_type: String,
    #[serde(default)]
    pub fact_kind: String,
    #[serde(default)]
    pub occurred_at: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    #[serde(default)]
    pub confidence: DecisionTraceConfidence,
    #[serde(default)]
    pub decision_trace_id: String,
    #[serde(default)]
    pub decision_trace: DecisionTrace,
    #[serde(default)]
    pub created_at: String,
}

/// User-confirmed project facts are appended to the existing work ledger.
/// This request is transient; it intentionally does not create another task or todo record.
#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectFactCaptureRequest {
    #[serde(default)]
    pub capture_type: String,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub reason: String,
    #[serde(default)]
    pub action_source_ref: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ActivityProjection {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub project_id: String,
    #[serde(rename = "type", default)]
    pub activity_type: String,
    #[serde(default)]
    pub source_type: String,
    #[serde(default)]
    pub occurred_at: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub evidence_refs: Vec<EvidenceRef>,
    #[serde(default)]
    pub actor: String,
    #[serde(default)]
    pub confidence: DecisionTraceConfidence,
    #[serde(default)]
    pub user_visible: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PendingActionProjection {
    #[serde(default)]
    pub id: String,
    #[serde(rename = "type", default)]
    pub action_type: String,
    #[serde(default)]
    pub project_id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub reason: String,
    #[serde(default)]
    pub source_ref: String,
    #[serde(default)]
    pub priority: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub evidence_refs: Vec<EvidenceRef>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionRecord {
    #[serde(default)]
    pub id: String,
    #[serde(rename = "type", default)]
    pub execution_type: String,
    #[serde(default)]
    pub source_id: String,
    #[serde(default)]
    pub project_id: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub started_at: String,
    #[serde(default)]
    pub ended_at: String,
    #[serde(default)]
    pub retry_count: u32,
    #[serde(default)]
    pub error: String,
    #[serde(default)]
    pub requires_approval: bool,
    #[serde(default)]
    pub approval_status: String,
    #[serde(default)]
    pub evidence_refs: Vec<EvidenceRef>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CodexTaskGitVerification {
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub repository_path: String,
    #[serde(default)]
    pub checked_at: String,
    #[serde(default)]
    pub reason: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CodexTaskAcceptance {
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub reason: String,
    #[serde(default)]
    pub decided_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CodexCliCapabilityProbe {
    #[serde(default)]
    pub available: bool,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub supports_cd: bool,
    #[serde(default)]
    pub supports_add_dir: bool,
    #[serde(default)]
    pub supports_json: bool,
    #[serde(default)]
    pub supports_output_last_message: bool,
    #[serde(default)]
    pub supports_sandbox: bool,
    #[serde(default)]
    pub supports_stdin: bool,
    #[serde(default)]
    pub checked_at: String,
    #[serde(default)]
    pub failure_reason: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CodexTask {
    #[serde(default)]
    pub task_id: String,
    #[serde(default)]
    pub project_id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub task_type: String,
    #[serde(default)]
    pub prompt: String,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub handed_off_at: String,
    #[serde(default)]
    pub result_received_at: String,
    #[serde(default)]
    pub completed_at: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub repository_path: String,
    #[serde(default)]
    pub expected_result_path: String,
    #[serde(default)]
    pub result_id: String,
    #[serde(default)]
    pub result_source: String,
    #[serde(default)]
    pub result_run_id: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub result_text: String,
    #[serde(default)]
    pub changed_files: Vec<String>,
    #[serde(default)]
    pub reported_commits: Vec<String>,
    #[serde(default)]
    pub verified_commits: Vec<String>,
    #[serde(default)]
    pub git_verification: CodexTaskGitVerification,
    #[serde(default)]
    pub tests: Vec<String>,
    #[serde(default)]
    pub findings: Vec<String>,
    #[serde(default)]
    pub recommendations: Vec<String>,
    #[serde(default)]
    pub questions: Vec<String>,
    #[serde(default)]
    pub checks: Vec<String>,
    #[serde(default)]
    pub passed: Vec<String>,
    #[serde(default)]
    pub failed: Vec<String>,
    #[serde(default)]
    pub artifacts: Vec<String>,
    #[serde(default)]
    pub target_files: Vec<String>,
    #[serde(default)]
    pub remaining_issues: Vec<String>,
    #[serde(default)]
    pub manual_acceptance: Vec<String>,
    #[serde(default)]
    pub acceptance: CodexTaskAcceptance,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    #[serde(default)]
    pub updated_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CodexTaskList {
    #[serde(default)]
    pub tasks: Vec<CodexTask>,
    #[serde(default)]
    pub runs: Vec<CodexRun>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CodexRun {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub task_id: String,
    #[serde(default)]
    pub project_id: String,
    #[serde(default)]
    pub repository_path: String,
    #[serde(default)]
    pub command_executable: String,
    #[serde(default)]
    pub command_args: Vec<String>,
    #[serde(default)]
    pub capability_probe: CodexCliCapabilityProbe,
    #[serde(default)]
    pub working_directory: String,
    #[serde(default)]
    pub stdin_prompt_summary: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub started_at: String,
    #[serde(default)]
    pub ended_at: String,
    #[serde(default)]
    pub pid: u32,
    #[serde(default)]
    pub exit_code: Option<i32>,
    #[serde(default)]
    pub output_last_message_path: String,
    #[serde(default)]
    pub stdout_summary: String,
    #[serde(default)]
    pub stderr_summary: String,
    #[serde(default)]
    pub error: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CodexRunDocument {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub runs: Vec<CodexRun>,
    #[serde(default)]
    pub updated_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WorkLedgerDocument {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub events: Vec<WorkEvent>,
    #[serde(default)]
    pub updated_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GitCommitInfo {
    #[serde(default)]
    pub hash: String,
    #[serde(default)]
    pub short_hash: String,
    #[serde(default)]
    pub subject: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub timestamp: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GitChangedFile {
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub path: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GitSnapshot {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub project_id: String,
    #[serde(default)]
    pub repository_path: String,
    #[serde(default)]
    pub branch: String,
    #[serde(default)]
    pub head: String,
    #[serde(default)]
    pub head_short: String,
    #[serde(default)]
    pub recent_commits: Vec<GitCommitInfo>,
    #[serde(default)]
    pub is_dirty: bool,
    #[serde(default)]
    pub changed_files: Vec<GitChangedFile>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub failure_reason: String,
    #[serde(default)]
    pub captured_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CodexExternalResult {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub project_id: String,
    #[serde(default)]
    pub source_hash: String,
    #[serde(default)]
    pub source_label: String,
    #[serde(default)]
    pub result_source: String,
    #[serde(default)]
    pub task_id: String,
    #[serde(default)]
    pub task_type: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub result_text: String,
    #[serde(default)]
    pub completed_content: Vec<String>,
    #[serde(default)]
    pub changed_files: Vec<String>,
    #[serde(default)]
    pub commits: Vec<String>,
    #[serde(default)]
    pub tests: Vec<String>,
    #[serde(default)]
    pub findings: Vec<String>,
    #[serde(default)]
    pub recommendations: Vec<String>,
    #[serde(default)]
    pub questions: Vec<String>,
    #[serde(default)]
    pub checks: Vec<String>,
    #[serde(default)]
    pub passed: Vec<String>,
    #[serde(default)]
    pub failed: Vec<String>,
    #[serde(default)]
    pub artifacts: Vec<String>,
    #[serde(default)]
    pub target_files: Vec<String>,
    #[serde(default)]
    pub unresolved_items: Vec<String>,
    #[serde(default)]
    pub manual_acceptance: Vec<String>,
    #[serde(default)]
    pub external_thread_id: String,
    #[serde(default)]
    pub result_run_id: String,
    #[serde(default)]
    pub raw_evidence_path: String,
    #[serde(default)]
    pub created_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CodexExternalResultsDocument {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub results: Vec<CodexExternalResult>,
    #[serde(default)]
    pub updated_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WorkLedgerSnapshot {
    #[serde(default)]
    pub events: Vec<WorkEvent>,
    #[serde(default)]
    pub git_snapshot: Option<GitSnapshot>,
    #[serde(default)]
    pub codex_results: Vec<CodexExternalResult>,
    #[serde(default)]
    pub codex_tasks: Vec<CodexTask>,
    #[serde(default)]
    pub activity_timeline: Vec<ActivityProjection>,
    #[serde(default)]
    pub pending_actions: Vec<PendingActionProjection>,
    #[serde(default)]
    pub executions: Vec<ExecutionRecord>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectContextFocus {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub reason: String,
    #[serde(default)]
    pub evidence: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectContextActivity {
    #[serde(default)]
    pub occurred_at: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub evidence: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectContextPendingAction {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub reason: String,
    #[serde(default)]
    pub priority: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectContextFile {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub document_purpose: String,
    #[serde(default)]
    pub lifecycle_status: String,
    #[serde(default)]
    pub location: String,
    #[serde(default)]
    pub summary: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectContextCodexResult {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub task_type: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub manual_acceptance: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectContextPacket {
    #[serde(default)]
    pub project_id: String,
    #[serde(default)]
    pub project_name: String,
    #[serde(default)]
    pub generated_at: String,
    #[serde(default)]
    pub privacy_notice: String,
    #[serde(default)]
    pub focus: Option<ProjectContextFocus>,
    #[serde(default)]
    pub recent_activity: Vec<ProjectContextActivity>,
    #[serde(default)]
    pub pending_actions: Vec<ProjectContextPendingAction>,
    #[serde(default)]
    pub files: Vec<ProjectContextFile>,
    #[serde(default)]
    pub codex_result: Option<ProjectContextCodexResult>,
    #[serde(default)]
    pub sparse: bool,
    #[serde(default)]
    pub markdown: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DailyContinueSnapshot {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub project_id: String,
    #[serde(default)]
    pub generated_at: String,
    #[serde(default)]
    pub last_progress: Vec<String>,
    #[serde(default)]
    pub recommended_actions: Vec<DailyContinueItem>,
    #[serde(default)]
    pub blockers: Vec<String>,
    #[serde(default)]
    pub pending_confirmations: Vec<String>,
    #[serde(default)]
    pub recent_changes: Vec<String>,
    #[serde(default)]
    pub status: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DeepSeekAuthorization {
    #[serde(default)]
    pub granted_at: String,
    #[serde(default)]
    pub granted_by: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DeepSeekModelInfo {
    pub id: String,
    #[serde(default)]
    pub owned_by: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DeepSeekSettings {
    #[serde(default)]
    pub has_api_key: bool,
    #[serde(default)]
    pub selected_model_id: String,
    #[serde(default)]
    pub last_tested_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DeepSeekConnectionResult {
    pub settings: DeepSeekSettings,
    #[serde(default)]
    pub models: Vec<DeepSeekModelInfo>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DeepSeekStreamEvent {
    pub project_root: String,
    pub message_id: String,
    pub status: String,
    #[serde(default)]
    pub delta: String,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub error: String,
    #[serde(default)]
    pub model_id: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WeeklyTaskDistributionItem {
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub count: usize,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WeeklyRepeatedPattern {
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub count: usize,
    #[serde(default)]
    pub evidence: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WeeklyHabitSummary {
    #[serde(default)]
    pub effective_habits: Vec<String>,
    #[serde(default)]
    pub inefficient_loops: Vec<String>,
    #[serde(default)]
    pub frequent_switches: Vec<String>,
    #[serde(default)]
    pub repeated_rework: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WeeklySkillCandidate {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub scenario: String,
    #[serde(default)]
    pub evidence: Vec<String>,
    #[serde(default)]
    pub occurrence_count: usize,
    #[serde(default)]
    pub reusable_steps: Vec<String>,
    #[serde(default)]
    pub expected_value: String,
    #[serde(default)]
    pub current_gap: String,
    #[serde(default)]
    pub confidence: f32,
    #[serde(default = "default_true")]
    pub review_required: bool,
    #[serde(default)]
    pub decision_trace: DecisionTrace,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WeeklyProjectProgress {
    #[serde(default)]
    pub project_id: String,
    #[serde(default)]
    pub project_name: String,
    #[serde(default)]
    pub completed_items: Vec<String>,
    #[serde(default)]
    pub blockers: Vec<String>,
    #[serde(default)]
    pub unfinished_items: Vec<String>,
    #[serde(default)]
    pub next_steps: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WeeklyReportRecord {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub scope: String,
    #[serde(default)]
    pub project_id: String,
    #[serde(default)]
    pub project_root: String,
    #[serde(default)]
    pub project_name: String,
    #[serde(default)]
    pub week_key: String,
    #[serde(default)]
    pub week_start: String,
    #[serde(default)]
    pub week_end: String,
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub generated_at: String,
    #[serde(default)]
    pub updated_at: String,
    #[serde(default)]
    pub confirmed_at: String,
    #[serde(default)]
    pub review_required: bool,
    #[serde(default)]
    pub markdown: String,
    #[serde(default)]
    pub json_path: String,
    #[serde(default)]
    pub markdown_path: String,
    #[serde(default)]
    pub pdf_path: String,
    #[serde(default)]
    pub completed_items: Vec<String>,
    #[serde(default)]
    pub achievements: Vec<String>,
    #[serde(default)]
    pub blockers: Vec<String>,
    #[serde(default)]
    pub unfinished_items: Vec<String>,
    #[serde(default)]
    pub facts: Vec<String>,
    #[serde(default)]
    pub analysis: Vec<String>,
    #[serde(default)]
    pub suggestions: Vec<String>,
    #[serde(default)]
    pub pending_confirmations: Vec<String>,
    #[serde(default)]
    pub task_distribution: Vec<WeeklyTaskDistributionItem>,
    #[serde(default)]
    pub repeated_operations: Vec<WeeklyRepeatedPattern>,
    #[serde(default)]
    pub repeated_issues: Vec<WeeklyRepeatedPattern>,
    #[serde(default)]
    pub habit_summary: WeeklyHabitSummary,
    #[serde(default)]
    pub skill_candidates: Vec<WeeklySkillCandidate>,
    #[serde(default)]
    pub project_progress: Vec<WeeklyProjectProgress>,
    #[serde(default)]
    pub evidence: Vec<String>,
    #[serde(default)]
    pub data_health_records: Vec<DataHealthRecord>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WeeklyReviewSettings {
    #[serde(default = "default_generation_weekday")]
    pub generation_weekday: u8,
    #[serde(default)]
    pub last_auto_generated_week_key: String,
    #[serde(default)]
    pub updated_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WeeklyReportsDocument {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub reports: Vec<WeeklyReportRecord>,
    #[serde(default)]
    pub updated_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WeeklyReviewDashboard {
    #[serde(default)]
    pub settings: WeeklyReviewSettings,
    #[serde(default)]
    pub current_week_key: String,
    #[serde(default)]
    pub week_start: String,
    #[serde(default)]
    pub week_end: String,
    #[serde(default)]
    pub generated_at: String,
    #[serde(default)]
    pub generated_now: bool,
    #[serde(default)]
    pub global_reports: Vec<WeeklyReportRecord>,
    #[serde(default)]
    pub project_reports: Vec<WeeklyReportRecord>,
    #[serde(default)]
    pub active_project_root: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WeeklyReportExportResult {
    #[serde(default)]
    pub report_id: String,
    #[serde(default)]
    pub destination_dir: String,
    #[serde(default)]
    pub exported_paths: Vec<String>,
    #[serde(default)]
    pub exported_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WeeklyReviewSyncResult {
    #[serde(default)]
    pub target_week_key: String,
    #[serde(default)]
    pub generated_global_report_id: String,
    #[serde(default)]
    pub generated_project_report_ids: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PersonalSkill {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub scenario: String,
    #[serde(default)]
    pub evidence: Vec<String>,
    #[serde(default)]
    pub frequency: usize,
    #[serde(default)]
    pub steps: Vec<String>,
    #[serde(default)]
    pub examples: Vec<String>,
    #[serde(default)]
    pub confidence: DecisionTraceConfidence,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
    #[serde(default)]
    pub decision_trace: DecisionTrace,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgePatternCandidate {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub pattern_name: String,
    #[serde(default)]
    pub related_projects: Vec<String>,
    #[serde(default)]
    pub evidence: Vec<String>,
    #[serde(default)]
    pub common_steps: Vec<String>,
    #[serde(default)]
    pub reusable_value: String,
    #[serde(default)]
    pub confidence: DecisionTraceConfidence,
    #[serde(default = "default_true")]
    pub review_required: bool,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
    #[serde(default)]
    pub decision_trace: DecisionTrace,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ImprovementCandidate {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub problem: String,
    #[serde(default)]
    pub evidence: Vec<String>,
    #[serde(default)]
    pub impact: String,
    #[serde(default)]
    pub suggested_improvement: String,
    #[serde(default)]
    pub confidence: DecisionTraceConfidence,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
    #[serde(default)]
    pub decision_trace: DecisionTrace,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct V1Readiness {
    #[serde(default)]
    pub generated_at: String,
    #[serde(default)]
    pub overview_path: String,
    #[serde(default)]
    pub usage_guide_path: String,
    #[serde(default)]
    pub limitations_path: String,
    #[serde(default)]
    pub roadmap_path: String,
    #[serde(default)]
    pub capability_overview: Vec<String>,
    #[serde(default)]
    pub current_limitations: Vec<String>,
    #[serde(default)]
    pub roadmap_candidates: Vec<String>,
    #[serde(default)]
    pub security_checks: Vec<String>,
    #[serde(default)]
    pub installer_paths: Vec<String>,
}

fn default_generation_weekday() -> u8 {
    1
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LocalBackupResult {
    pub backup_dir: String,
    #[serde(default)]
    pub global_files: Vec<String>,
    #[serde(default)]
    pub project_roots: Vec<String>,
    #[serde(default)]
    pub excluded_entries: Vec<String>,
    pub checksum_manifest_path: String,
    pub created_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BackupRestoreResult {
    pub backup_dir: String,
    pub restored_global_dir: String,
    #[serde(default)]
    pub restored_projects: Vec<ProjectSummary>,
    #[serde(default)]
    pub restored_global_files: Vec<String>,
    #[serde(default)]
    pub warnings: Vec<String>,
    pub restored_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectMigrationResult {
    pub project: ProjectSummary,
    pub old_root_dir: String,
    pub new_root_dir: String,
    pub retained_old_root: bool,
    #[serde(default)]
    pub copied_entries: usize,
    pub migrated_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SafeExportResult {
    pub export_dir: String,
    #[serde(default)]
    pub included_files: Vec<String>,
    pub exported_file_count: usize,
    pub redacted_message_count: usize,
    pub exported_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PrivacyArtifactsResult {
    pub privacy_notice_path: String,
    pub third_party_notices_path: String,
    pub diagnostic_log_path: String,
    pub generated_at: String,
}
