use crate::models::{
    ActivityProjection, CleanupExecutionBatch, CleanupPlan, CodexRun, CodexTask, EvidenceRef,
    ExecutionRecord, FileIdentity, FileLocation, FileMetadata, FileProjection, FileRecord,
    GlobalManagedFile, LocationPreview, MaterialInboxItem, PendingActionProjection,
    ProjectManifest, SearchIndexEntry, WorkEvent, WorkspaceScanFile,
};
use sha2::{Digest, Sha256};
use std::path::Path;

pub struct FileProjectionAdapter;

impl FileProjectionAdapter {
    pub fn from_inbox(item: &MaterialInboxItem) -> FileProjection {
        let managed_path = item
            .route_operation
            .as_ref()
            .map(|operation| operation.target_path.clone())
            .unwrap_or_default();
        build_file_projection(
            &item.id,
            &item.source_hash,
            &item.file_name,
            merged_source_paths(
                &item.source_path,
                item.source_history
                    .iter()
                    .map(|entry| entry.source_path.as_str()),
            ),
            &item.created_at,
            &item.file_type,
            &item.document_type,
            &item.document_purpose,
            &item.business_domain,
            first_non_empty(&item.general_material_category, &item.recommended_category),
            inbox_lifecycle(item),
            &item.duplicate_of_file_id,
            &item.document_family_id,
            item.version_number,
            &item.source_path,
            &managed_path,
            first_non_empty(
                &item.recommended_relative_location,
                &item.suggested_managed_path,
            ),
            first_non_empty(&item.target_project_id, &item.target_project_name),
            &item.ownership_type,
            &item.general_material_category,
            &item.content_summary,
            "materialInbox",
        )
    }

    pub fn from_scan(file: &WorkspaceScanFile) -> FileProjection {
        build_file_projection(
            &file.id,
            &file.hash,
            &file.file_name,
            vec![file.path.clone()],
            &file.modified_at,
            &file.extension,
            &file.document_type,
            &file.document_purpose,
            "",
            &file.recommended_category,
            if file.status == "failed" {
                "missing"
            } else {
                "understood"
            },
            "",
            "",
            0,
            &file.path,
            "",
            &file.recommended_location,
            &file.recommended_project_name,
            &file.ownership_type,
            &file.recommended_category,
            &file.content_summary,
            "workspaceScan",
        )
    }

    pub fn from_project_file(file: &FileRecord, project_id: &str) -> FileProjection {
        let mut sources = file.source_paths.clone();
        if !file.original_source_path.trim().is_empty() {
            sources.push(file.original_source_path.clone());
        }
        build_file_projection(
            &file.id,
            &file.content_hash,
            first_non_empty(&file.original_file_name, &file.file_name),
            unique_non_empty(sources),
            first_non_empty(&file.modified_at, &file.created_at),
            &file.file_type,
            &file.recommended_category,
            "",
            "",
            &file.category,
            if file.still_exists {
                "managed"
            } else {
                "missing"
            },
            file.duplicate_of_file_id.as_deref().unwrap_or(""),
            first_non_empty(&file.document_family_id, &file.version_group_id),
            file.version_number,
            &file.original_source_path,
            &file.managed_path,
            &file.managed_relative_path,
            project_id,
            "existingProjectMaterial",
            "",
            &file.content_summary,
            "projectFile",
        )
    }

    pub fn from_global_file(file: &GlobalManagedFile) -> FileProjection {
        let lifecycle_status = if !file.lifecycle_status.trim().is_empty() {
            file.lifecycle_status.as_str()
        } else if !file.undone_at.trim().is_empty() {
            "superseded"
        } else if !file.managed_path.trim().is_empty() && !Path::new(&file.managed_path).exists() {
            "missing"
        } else if file.content_hash.trim().is_empty() {
            "received"
        } else {
            "managed"
        };
        build_file_projection(
            &file.id,
            &file.content_hash,
            &file.file_name,
            vec![file.source_path.clone()],
            &file.created_at,
            &file.file_type,
            &file.document_type,
            &file.document_purpose,
            &file.business_domain,
            first_non_empty(&file.general_material_category, &file.category),
            lifecycle_status,
            "",
            "",
            0,
            &file.source_path,
            &file.managed_path,
            &file.managed_relative_path,
            &file.related_project,
            &file.ownership_type,
            &file.general_material_category,
            first_non_empty(&file.business_summary, &file.content_summary),
            "globalManagedFile",
        )
    }

    pub fn from_search_entry(entry: &SearchIndexEntry) -> FileProjection {
        let mut projection = build_file_projection(
            first_non_empty(&entry.file_id, &entry.id),
            &entry.hash,
            &entry.title,
            Vec::new(),
            &entry.updated_at,
            &entry.file_type,
            &entry.document_type,
            &entry.document_purpose,
            &entry.business_domain,
            &entry.category,
            first_non_empty(&entry.lifecycle_status, &entry.recent_status),
            &entry.duplicate_of,
            &entry.version_group_id,
            entry.version_number,
            "",
            &entry.managed_path,
            &entry.workspace_relative_path,
            &entry.project_id,
            &entry.ownership_type,
            &entry.category,
            &entry.summary,
            "searchIndex",
        );
        if !entry.evidence_refs.is_empty() {
            projection.evidence_refs = entry.evidence_refs.clone();
        }
        projection
    }
}

pub struct LocationPreviewService;

impl LocationPreviewService {
    pub fn preview(
        source_path: &str,
        target_path: &Path,
        workspace_root: Option<&Path>,
        project_id: &str,
        ownership_type: &str,
        category: &str,
        reason: &str,
    ) -> LocationPreview {
        let target = normalize_path(target_path);
        let workspace_relative_path = workspace_root
            .and_then(|root| target_path.strip_prefix(root).ok())
            .map(normalize_path)
            .unwrap_or_default();
        let fingerprint = [
            source_path,
            target.as_str(),
            workspace_relative_path.as_str(),
            project_id,
            ownership_type,
            category,
        ]
        .join("\n");
        LocationPreview {
            preview_id: format!("location:{}", sha256_text(&fingerprint)),
            source_path: source_path.to_string(),
            target_path: target,
            workspace_relative_path,
            project_id: project_id.to_string(),
            ownership_type: ownership_type.to_string(),
            category: category.to_string(),
            reason: reason.to_string(),
        }
    }

    pub fn matches(preview_id: &str, preview: &LocationPreview) -> bool {
        !preview_id.trim().is_empty() && preview_id == preview.preview_id
    }
}

pub struct ActivityProjectionAdapter;

impl ActivityProjectionAdapter {
    pub fn from_work_event(event: &WorkEvent) -> ActivityProjection {
        let user_visible = user_visible_work_event(&event.event_type);
        ActivityProjection {
            id: event.id.clone(),
            project_id: event.project_id.clone(),
            activity_type: activity_type(&event.event_type).to_string(),
            source_type: event.source_type.clone(),
            occurred_at: event.occurred_at.clone(),
            summary: event.summary.clone(),
            evidence_refs: event
                .evidence_refs
                .iter()
                .map(|value| evidence_ref_from_legacy(value))
                .collect(),
            actor: activity_actor(&event.source_type).to_string(),
            confidence: event.confidence.clone(),
            user_visible,
        }
    }

    pub fn visible_timeline(events: &[WorkEvent]) -> Vec<ActivityProjection> {
        let mut timeline = events
            .iter()
            .map(Self::from_work_event)
            .filter(|activity| activity.user_visible)
            .collect::<Vec<_>>();
        timeline.sort_by(|left, right| right.occurred_at.cmp(&left.occurred_at));
        timeline
    }
}

pub struct PendingActionAdapter;

impl PendingActionAdapter {
    pub fn from_project(
        manifest: &ProjectManifest,
        codex_tasks: &[CodexTask],
        work_events: &[WorkEvent],
    ) -> Vec<PendingActionProjection> {
        Self::from_project_with_runs(manifest, codex_tasks, &[], work_events)
    }

    pub fn from_project_with_runs(
        manifest: &ProjectManifest,
        codex_tasks: &[CodexTask],
        codex_runs: &[CodexRun],
        work_events: &[WorkEvent],
    ) -> Vec<PendingActionProjection> {
        let project_id = manifest.project.id.as_str();
        let mut actions = Vec::new();
        actions.extend(manifest.pending_reviews.iter().filter_map(|review| {
            if matches!(review.status.as_str(), "resolved" | "ignored") {
                return None;
            }
            Some(pending_action(
                &format!("pending-review:{}", review.id),
                "projectReview",
                project_id,
                &review.title,
                &review.detail,
                &review.id,
                "medium",
                normalize_pending_status(&review.status),
                first_non_empty(&review.detected_at, &review.updated_at),
                vec![EvidenceRef {
                    kind: "pendingReview".to_string(),
                    id: review.id.clone(),
                    path_snapshot: review.path.clone(),
                    hash_snapshot: String::new(),
                    label: review.title.clone(),
                }],
            ))
        }));
        // Project-state proposals are analysis suggestions, not user-confirmed
        // work. Keep them available to the impact-analysis panel, but do not
        // promote them into the current PendingAction projection.
        actions.extend(manifest.project_attentions.iter().filter_map(|attention| {
            if !matches!(attention.status.as_str(), "pending" | "later") {
                return None;
            }
            if attention_is_legacy_message_task(attention, manifest) {
                return None;
            }
            Some(pending_action(
                &format!("project-attention:{}", attention.id),
                "projectAttention",
                project_id,
                &attention.title,
                &attention.reason,
                &attention.id,
                confidence_priority(attention.confidence.score),
                "pending",
                first_non_empty(&attention.updated_at, &attention.created_at),
                attention
                    .evidence
                    .iter()
                    .map(|evidence| EvidenceRef {
                        kind: evidence.kind.clone(),
                        id: evidence.source_id.clone(),
                        path_snapshot: String::new(),
                        hash_snapshot: String::new(),
                        label: evidence.label.clone(),
                    })
                    .collect(),
            ))
        }));
        let current_next_step = current_user_next_step(manifest, work_events);
        let resolved_sources = work_events
            .iter()
            .filter(|event| {
                event.project_id == project_id
                    && event.source_type == "user"
                    && event.event_type == "user.actionResolved"
            })
            .map(|event| event.source_ref.as_str())
            .collect::<std::collections::HashSet<_>>();
        let next_step_has_ledger_event = work_events.iter().any(|event| {
            event.project_id == project_id
                && event.source_type == "user"
                && event.event_type == "user.nextActionRecorded"
                && user_next_action_is_current(event, current_next_step)
                && !resolved_sources.contains(event.source_ref.as_str())
        });
        let next_step_echoes_accepted_task = current_next_step_is_accepted_task_echo(
            manifest,
            codex_tasks,
            work_events,
            current_next_step,
        );
        if !current_next_step.is_empty()
            && !next_step_has_ledger_event
            && !next_step_echoes_accepted_task
        {
            let source_ref = format!("manifest-next-step:{project_id}");
            actions.push(pending_action(
                &source_ref,
                "userNextAction",
                project_id,
                current_next_step,
                "来自项目清单中当前有效的下一步；历史建议不会覆盖它。",
                &source_ref,
                "medium",
                "pending",
                &manifest.project.last_opened_at,
                vec![EvidenceRef {
                    kind: "projectManifest".to_string(),
                    id: project_id.to_string(),
                    label: "项目当前 nextStep".to_string(),
                    ..EvidenceRef::default()
                }],
            ));
        }
        actions.extend(manifest.tasks.iter().filter_map(|task| {
            if !matches!(task.status.as_str(), "active" | "todo")
                || task.title.trim().is_empty()
                || manifest.messages.iter().any(|message| {
                    message.id == task.source_message_id
                        && message.author == "user"
                        && message.kind == "requirement"
                })
            {
                return None;
            }
            Some(pending_action(
                &format!("project-task:{}", task.id),
                "projectTask",
                project_id,
                &task.title,
                "这是项目中尚未完成的已确认任务。",
                &task.id,
                "medium",
                "pending",
                &task.updated_at,
                vec![EvidenceRef {
                    kind: "task".to_string(),
                    id: task.id.clone(),
                    label: "项目任务记录".to_string(),
                    ..EvidenceRef::default()
                }],
            ))
        }));
        actions.extend(codex_tasks.iter().filter_map(|task| {
            if !task.project_id.is_empty() && task.project_id != project_id {
                return None;
            }
            if codex_task_is_superseded_by_accepted_target(task, codex_tasks) {
                return None;
            }
            match task.status.as_str() {
                "awaitingAcceptance" if codex_task_has_bound_result(task) => Some(pending_action(
                    &format!("codex-acceptance:{}", task.task_id),
                    "codexAcceptance",
                    project_id,
                    &format!(
                        "Codex 任务待验收：{}",
                        first_non_empty(&task.title, &task.task_id)
                    ),
                    &format!("{} 项人工验收尚未完成。", task.manual_acceptance.len()),
                    &task.task_id,
                    "high",
                    "pending",
                    codex_task_action_timestamp(task),
                    task.evidence_refs
                        .iter()
                        .map(|value| evidence_ref_from_legacy(value))
                        .collect(),
                )),
                "needsReview" | "resultReceived" | "verifying"
                    if codex_task_has_bound_result(task) =>
                {
                    Some(pending_action(
                        &format!("codex-review:{}", task.task_id),
                        "codexReview",
                        project_id,
                        &format!(
                            "Codex 任务需要检查：{}",
                            first_non_empty(&task.title, &task.task_id)
                        ),
                        &codex_task_review_reason(task),
                        &task.task_id,
                        "high",
                        "pending",
                        codex_task_action_timestamp(task),
                        task.evidence_refs
                            .iter()
                            .map(|value| evidence_ref_from_legacy(value))
                            .collect(),
                    ))
                }
                "ready" => Some(pending_action(
                    &format!("codex-ready:{}", task.task_id),
                    "codexReady",
                    project_id,
                    &format!(
                        "待启动 Codex 任务：{}",
                        first_non_empty(&task.title, &task.task_id)
                    ),
                    "这是尚未启动的当前项目任务。",
                    &task.task_id,
                    "high",
                    "pending",
                    codex_task_action_timestamp(task),
                    vec![EvidenceRef {
                        kind: "codexTask".to_string(),
                        id: task.task_id.clone(),
                        label: "待启动 Codex 任务".to_string(),
                        ..EvidenceRef::default()
                    }],
                )),
                "starting" | "running" | "awaitingResult"
                    if codex_task_has_live_run(task, project_id, codex_runs) =>
                {
                    Some(pending_action(
                        &format!("codex-active:{}", task.task_id),
                        "codexActive",
                        project_id,
                        &format!(
                            "继续 Codex 任务：{}",
                            first_non_empty(&task.title, &task.task_id)
                        ),
                        "该任务仍处于执行或结果核验流程中。",
                        &task.task_id,
                        "high",
                        "pending",
                        codex_task_action_timestamp(task),
                        vec![EvidenceRef {
                            kind: "codexTask".to_string(),
                            id: task.task_id.clone(),
                            label: "当前 Codex 任务".to_string(),
                            ..EvidenceRef::default()
                        }],
                    ))
                }
                _ => None,
            }
        }));
        actions.extend(Self::from_user_work_events(
            project_id,
            current_next_step,
            work_events,
        ));
        unique_pending_actions(actions)
    }

    fn from_user_work_events(
        project_id: &str,
        current_next_step: &str,
        work_events: &[WorkEvent],
    ) -> Vec<PendingActionProjection> {
        let resolved_sources = work_events
            .iter()
            .filter(|event| {
                event.project_id == project_id
                    && event.source_type == "user"
                    && event.event_type == "user.actionResolved"
            })
            .map(|event| event.source_ref.clone())
            .collect::<std::collections::HashSet<_>>();
        let current_next_step_event = work_events
            .iter()
            .filter(|event| {
                event.project_id == project_id
                    && event.source_type == "user"
                    && event.event_type == "user.nextActionRecorded"
                    && user_next_action_is_current(event, current_next_step)
                    && !resolved_sources.contains(&event.source_ref)
            })
            .max_by(|left, right| left.occurred_at.cmp(&right.occurred_at))
            .map(|event| event.id.as_str());

        work_events
            .iter()
            .filter(|event| {
                event.project_id == project_id
                    && event.source_type == "user"
                    && matches!(
                        event.event_type.as_str(),
                        "user.blockerRecorded" | "user.nextActionRecorded"
                    )
                    && (event.event_type == "user.blockerRecorded"
                        || current_next_step_event == Some(event.id.as_str()))
                    && !resolved_sources.contains(&event.source_ref)
            })
            .map(|event| {
                let is_blocker = event.event_type == "user.blockerRecorded";
                let title = if is_blocker {
                    event.summary.as_str()
                } else {
                    event
                        .summary
                        .strip_prefix("用户记录下一步：")
                        .unwrap_or(&event.summary)
                };
                pending_action(
                    &format!("user-capture:{}", event.id),
                    if is_blocker {
                        "userBlocker"
                    } else {
                        "userNextAction"
                    },
                    project_id,
                    title,
                    if is_blocker {
                        "用户明确记录当前阻塞；处理后可继续推进。"
                    } else {
                        "用户明确记录的下一步；完成后请记录进展或关闭事项。"
                    },
                    &event.source_ref,
                    if is_blocker { "medium" } else { "low" },
                    "pending",
                    &event.occurred_at,
                    vec![EvidenceRef {
                        kind: "workEvent".to_string(),
                        id: event.id.clone(),
                        label: "用户确认的工作记录".to_string(),
                        ..EvidenceRef::default()
                    }],
                )
            })
            .collect()
    }

    pub fn from_cleanup(plans: &[CleanupPlan]) -> Vec<PendingActionProjection> {
        unique_pending_actions(
            plans
                .iter()
                .filter(|plan| matches!(plan.status.as_str(), "draft" | "reviewing"))
                .flat_map(|plan| {
                    plan.items.iter().filter_map(move |item| {
                        if !matches!(item.review_status.as_str(), "" | "pending" | "modified") {
                            return None;
                        }
                        Some(pending_action(
                            &format!("cleanup:{}:{}", plan.id, item.id),
                            "cleanupReview",
                            &item.recommended_project,
                            &format!("确认整理位置：{}", item.file_name),
                            &format!("建议位置：{}", item.recommended_target_path),
                            &item.id,
                            confidence_priority(item.confidence.score),
                            "pending",
                            &plan.created_at,
                            item.evidence
                                .iter()
                                .map(|evidence| EvidenceRef {
                                    kind: evidence.kind.clone(),
                                    id: evidence.source_id.clone(),
                                    path_snapshot: item.source_path.clone(),
                                    hash_snapshot: item.hash.clone(),
                                    label: evidence.label.clone(),
                                })
                                .collect(),
                        ))
                    })
                })
                .collect(),
        )
    }

    pub fn from_inbox(items: &[MaterialInboxItem]) -> Vec<PendingActionProjection> {
        unique_pending_actions(
            items
                .iter()
                .filter(|item| {
                    matches!(item.processing_status.as_str(), "pendingReview" | "failed")
                })
                .map(|item| {
                    pending_action(
                        &format!("inbox:{}", item.id),
                        "inboxReview",
                        &item.target_project_id,
                        &format!("Inbox 待确认：{}", item.file_name),
                        first_non_empty(&item.error_message, &item.location_reason),
                        &item.id,
                        confidence_priority(item.confidence_score),
                        "pending",
                        first_non_empty(&item.updated_at, &item.received_at),
                        vec![EvidenceRef {
                            kind: "file".to_string(),
                            id: item.id.clone(),
                            path_snapshot: item.source_path.clone(),
                            hash_snapshot: item.source_hash.clone(),
                            label: item.file_name.clone(),
                        }],
                    )
                })
                .collect(),
        )
    }
}

fn current_user_next_step<'a>(manifest: &'a ProjectManifest, work_events: &[WorkEvent]) -> &'a str {
    let value = manifest.project.next_step.trim();
    if value.is_empty() || !has_independent_user_next_step_evidence(manifest, work_events, value) {
        ""
    } else {
        value
    }
}

fn current_next_step_is_accepted_task_echo(
    manifest: &ProjectManifest,
    codex_tasks: &[CodexTask],
    work_events: &[WorkEvent],
    current_next_step: &str,
) -> bool {
    let current_next_step = current_next_step.trim();
    if current_next_step.is_empty()
        || has_independent_user_next_step_evidence(manifest, work_events, current_next_step)
    {
        return false;
    }

    codex_tasks.iter().any(|task| {
        (task.project_id.is_empty() || task.project_id == manifest.project.id)
            && task.status == "completed"
            && task.acceptance.status == "approved"
            && task.title.trim() == current_next_step
    })
}

fn has_independent_user_next_step_evidence(
    manifest: &ProjectManifest,
    work_events: &[WorkEvent],
    value: &str,
) -> bool {
    let normalized = normalize_user_text(value);
    manifest.audit.iter().any(|event| {
        event.action == "workLedger.userNextStep"
            && event.target == manifest.project.id
            && event.outcome == "success"
            && event.confirmed
            && normalize_user_text(&event.details) == normalized
    }) || work_events.iter().any(|event| {
        event.project_id == manifest.project.id
            && event.source_type == "user"
            && event.event_type == "user.nextActionRecorded"
            && user_next_action_is_current(event, value)
    }) || manifest.recovery_points.iter().any(|point| {
        point.project_id == manifest.project.id
            && !point.completed.trim().is_empty()
            && point.completed.trim() != "完成项目目录扫描"
            && normalize_user_text(&point.next_step) == normalized
    })
}

fn codex_task_has_bound_result(task: &CodexTask) -> bool {
    !task.result_id.trim().is_empty()
        && !task.result_source.eq_ignore_ascii_case("runnerFallback")
        && !task
            .evidence_refs
            .iter()
            .any(|reference| reference.starts_with("runnerFallback:"))
}

fn codex_task_action_timestamp(task: &CodexTask) -> &str {
    let timestamps = match task.status.as_str() {
        "awaitingAcceptance" | "needsReview" | "resultReceived" | "verifying" => [
            task.result_received_at.as_str(),
            task.completed_at.as_str(),
            task.handed_off_at.as_str(),
            task.created_at.as_str(),
        ],
        "starting" | "running" | "awaitingResult" => [
            task.handed_off_at.as_str(),
            task.created_at.as_str(),
            task.result_received_at.as_str(),
            task.completed_at.as_str(),
        ],
        _ => [
            task.created_at.as_str(),
            task.handed_off_at.as_str(),
            task.result_received_at.as_str(),
            task.completed_at.as_str(),
        ],
    };
    timestamps
        .into_iter()
        .find(|timestamp| !timestamp.trim().is_empty())
        .unwrap_or_default()
}

fn codex_task_review_reason(task: &CodexTask) -> String {
    if task.task_type.eq_ignore_ascii_case("coding")
        && task.git_verification.status == "unavailable"
        && task
            .git_verification
            .reason
            .to_ascii_lowercase()
            .contains("commit")
    {
        return "代码任务结果已回流，但没有报告可核验的 Git commit，当前不能验收。".to_string();
    }
    if matches!(
        task.git_verification.status.as_str(),
        "mismatch" | "partial"
    ) && !task.git_verification.reason.trim().is_empty()
    {
        return format!("Git 核验未通过：{}", task.git_verification.reason.trim());
    }
    task.remaining_issues
        .iter()
        .find(|issue| !issue.trim().is_empty())
        .cloned()
        .unwrap_or_else(|| "已有绑定结果证据需要人工检查。".to_string())
}

fn codex_task_has_live_run(task: &CodexTask, project_id: &str, runs: &[CodexRun]) -> bool {
    task.project_id == project_id
        && runs.iter().any(|run| {
            run.task_id == task.task_id
                && run.project_id == project_id
                && run.status == "running"
                && run.pid != 0
                && run.process_alive
        })
}

fn user_next_action_is_current(event: &WorkEvent, current_next_step: &str) -> bool {
    let current_next_step = current_next_step.trim();
    if current_next_step.is_empty() {
        return false;
    }
    event
        .summary
        .strip_prefix("用户记录下一步：")
        .map(str::trim)
        .is_some_and(|summary| {
            normalize_user_text(summary) == normalize_user_text(current_next_step)
        })
}

fn normalize_user_text(value: &str) -> String {
    value
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect()
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

fn attention_is_legacy_message_task(
    attention: &crate::models::ProjectAttention,
    manifest: &ProjectManifest,
) -> bool {
    attention.attention_type == "staleTask"
        && attention.evidence.iter().any(|evidence| {
            evidence.kind == "task"
                && manifest.tasks.iter().any(|task| {
                    task.id == evidence.source_id
                        && manifest.messages.iter().any(|message| {
                            message.id == task.source_message_id
                                && message.author == "user"
                                && message.kind == "requirement"
                        })
                })
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

pub struct ExecutionProjectionAdapter;

impl ExecutionProjectionAdapter {
    pub fn from_codex_run(
        run: &CodexRun,
        task: Option<&CodexTask>,
        retry_count: u32,
    ) -> ExecutionRecord {
        let status = match run.status.as_str() {
            "starting" => "queued",
            "running" => "running",
            "cancelled" => "cancelled",
            "failed" => "failed",
            "exited" if run.exit_code == Some(0) => "succeeded",
            "exited" => "failed",
            _ => "needsReview",
        };
        let requires_approval = task
            .map(|value| {
                !value.manual_acceptance.is_empty() || value.status == "awaitingAcceptance"
            })
            .unwrap_or(false);
        let approval_status = task
            .map(|value| {
                if value.acceptance.status.trim().is_empty() {
                    if requires_approval {
                        "pending"
                    } else {
                        "notRequired"
                    }
                } else {
                    value.acceptance.status.as_str()
                }
            })
            .unwrap_or("notRequired");
        ExecutionRecord {
            id: run.id.clone(),
            execution_type: "codexRun".to_string(),
            source_id: run.task_id.clone(),
            project_id: run.project_id.clone(),
            status: status.to_string(),
            started_at: run.started_at.clone(),
            ended_at: run.ended_at.clone(),
            retry_count,
            error: first_non_empty(&run.error, &run.stderr_summary).to_string(),
            requires_approval,
            approval_status: approval_status.to_string(),
            evidence_refs: vec![EvidenceRef {
                kind: "codexRun".to_string(),
                id: run.id.clone(),
                path_snapshot: run.repository_path.clone(),
                hash_snapshot: String::new(),
                label: task
                    .map(|value| first_non_empty(&value.title, &value.task_id))
                    .unwrap_or(run.task_id.as_str())
                    .to_string(),
            }],
        }
    }

    pub fn from_codex_runs(runs: &[CodexRun], tasks: &[CodexTask]) -> Vec<ExecutionRecord> {
        let mut seen = std::collections::HashMap::<&str, u32>::new();
        runs.iter()
            .map(|run| {
                let count = seen.entry(run.task_id.as_str()).or_insert(0);
                let retry_count = *count;
                *count += 1;
                let task = tasks.iter().find(|task| task.task_id == run.task_id);
                Self::from_codex_run(run, task, retry_count)
            })
            .collect()
    }

    pub fn from_cleanup_batch(batch: &CleanupExecutionBatch, retry_count: u32) -> ExecutionRecord {
        let has_review_item = batch.items.iter().any(|item| {
            matches!(item.status.as_str(), "failed" | "undoConflict")
                || !item.error.trim().is_empty()
        });
        let status = match batch.status.as_str() {
            "running" => "running",
            "completed" if has_review_item => "needsReview",
            "completed" => "succeeded",
            "failed" => "failed",
            "cancelled" | "undone" => "cancelled",
            _ => "needsReview",
        };
        let error = batch
            .items
            .iter()
            .filter_map(|item| (!item.error.trim().is_empty()).then_some(item.error.as_str()))
            .collect::<Vec<_>>()
            .join("；");
        ExecutionRecord {
            id: batch.id.clone(),
            execution_type: "cleanupExecution".to_string(),
            source_id: batch.cleanup_plan_id.clone(),
            status: status.to_string(),
            started_at: batch.started_at.clone(),
            ended_at: batch.completed_at.clone(),
            retry_count,
            error,
            requires_approval: true,
            approval_status: "approved".to_string(),
            evidence_refs: vec![EvidenceRef {
                kind: "cleanupExecution".to_string(),
                id: batch.id.clone(),
                path_snapshot: String::new(),
                hash_snapshot: String::new(),
                label: format!("整理方案 {}", batch.cleanup_plan_id),
            }],
            ..ExecutionRecord::default()
        }
    }

    pub fn from_cleanup_batches(batches: &[CleanupExecutionBatch]) -> Vec<ExecutionRecord> {
        let mut seen = std::collections::HashMap::<&str, u32>::new();
        batches
            .iter()
            .map(|batch| {
                let count = seen.entry(batch.cleanup_plan_id.as_str()).or_insert(0);
                let retry_count = *count;
                *count += 1;
                Self::from_cleanup_batch(batch, retry_count)
            })
            .collect()
    }
}

fn pending_action(
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

fn unique_pending_actions(actions: Vec<PendingActionProjection>) -> Vec<PendingActionProjection> {
    let mut unique = Vec::new();
    for action in actions {
        if !unique
            .iter()
            .any(|existing: &PendingActionProjection| existing.id == action.id)
        {
            unique.push(action);
        }
    }
    unique.sort_by(|left, right| right.created_at.cmp(&left.created_at));
    unique
}

fn evidence_ref_from_legacy(value: &str) -> EvidenceRef {
    let (kind, id) = value.split_once(':').unwrap_or(("record", value));
    EvidenceRef {
        kind: kind.to_string(),
        id: id.to_string(),
        label: value.to_string(),
        ..EvidenceRef::default()
    }
}

fn user_visible_work_event(event_type: &str) -> bool {
    matches!(
        event_type,
        "file.import"
            | "file.generated"
            | "global.route"
            | "global.undo"
            | "codex.reportImported"
            | "codex.reportAccepted"
            | "codex.taskAccepted"
            | "codex.taskRejected"
            | "git.headChanged"
            | "user.decisionRecorded"
            | "user.progressRecorded"
            | "user.blockerRecorded"
            | "user.nextActionRecorded"
            | "user.actionResolved"
            | "projectState.applied"
            | "workspace.finish"
    ) || event_type.contains("failed")
        || event_type.contains("error")
}

fn activity_type(event_type: &str) -> &str {
    match event_type {
        "file.import" => "fileReceived",
        "file.generated" | "global.route" => "fileManaged",
        "global.undo" => "fileUndo",
        "git.headChanged" => "gitCommit",
        "user.decisionRecorded" => "userDecision",
        "user.progressRecorded" => "userProgress",
        "user.blockerRecorded" => "userBlocker",
        "user.nextActionRecorded" => "userNextAction",
        "user.actionResolved" => "userActionResolved",
        "codex.taskAccepted" => "manualAcceptance",
        "codex.reportImported" | "codex.reportAccepted" => "codexCompleted",
        "projectState.applied" => "projectStateChanged",
        "workspace.finish" => "workSessionCompleted",
        value if value.contains("failed") || value.contains("error") => "importantError",
        _ => "technical",
    }
}

fn activity_actor(source_type: &str) -> &str {
    match source_type {
        "user" => "localUser",
        "codex" => "codex",
        "git" => "git",
        _ => "system",
    }
}

fn normalize_pending_status(status: &str) -> &str {
    match status {
        "approved" | "confirmed" => "approved",
        "rejected" => "rejected",
        "resolved" => "resolved",
        "ignored" => "ignored",
        _ => "pending",
    }
}

fn confidence_priority(score: u8) -> &'static str {
    if score >= 78 {
        "high"
    } else if score >= 45 {
        "medium"
    } else {
        "low"
    }
}

fn build_file_projection(
    file_id: &str,
    hash: &str,
    original_name: &str,
    source_paths: Vec<String>,
    created_at: &str,
    file_type: &str,
    document_type: &str,
    document_purpose: &str,
    business_domain: &str,
    category: &str,
    lifecycle_status: &str,
    duplicate_of: &str,
    version_group_id: &str,
    version_number: u32,
    original_path: &str,
    managed_path: &str,
    workspace_relative_path: &str,
    project_id: &str,
    ownership_type: &str,
    general_category: &str,
    summary: &str,
    source_model: &str,
) -> FileProjection {
    let identity = FileIdentity {
        file_id: file_id.to_string(),
        hash: hash.to_string(),
        original_name: original_name.to_string(),
        source_paths: unique_non_empty(source_paths),
        created_at: created_at.to_string(),
    };
    let metadata = FileMetadata {
        file_type: file_type.to_string(),
        document_type: document_type.to_string(),
        document_purpose: document_purpose.to_string(),
        business_domain: business_domain.to_string(),
        category: category.to_string(),
        lifecycle_status: lifecycle_status.to_string(),
        duplicate_of: duplicate_of.to_string(),
        version_group_id: version_group_id.to_string(),
        version_number,
    };
    let location = FileLocation {
        original_path: original_path.to_string(),
        managed_path: managed_path.to_string(),
        workspace_relative_path: workspace_relative_path.to_string(),
        project_id: project_id.to_string(),
        ownership_type: ownership_type.to_string(),
        general_category: general_category.to_string(),
    };
    let evidence_refs = vec![EvidenceRef {
        kind: "file".to_string(),
        id: file_id.to_string(),
        path_snapshot: first_non_empty(managed_path, original_path).to_string(),
        hash_snapshot: hash.to_string(),
        label: original_name.to_string(),
    }];
    FileProjection {
        identity,
        metadata,
        location,
        summary: summary.to_string(),
        source_model: source_model.to_string(),
        evidence_refs,
    }
}

fn inbox_lifecycle(item: &MaterialInboxItem) -> &'static str {
    match item.processing_status.as_str() {
        "completed" | "routed" => "managed",
        "failed" => "missing",
        "pendingReview" => "understood",
        _ => "received",
    }
}

fn merged_source_paths<'a>(primary: &str, history: impl Iterator<Item = &'a str>) -> Vec<String> {
    unique_non_empty(
        std::iter::once(primary.to_string())
            .chain(history.map(str::to_string))
            .collect(),
    )
}

fn unique_non_empty(values: Vec<String>) -> Vec<String> {
    let mut output = Vec::new();
    for value in values {
        if !value.trim().is_empty() && !output.iter().any(|existing| existing == &value) {
            output.push(value);
        }
    }
    output
}

fn first_non_empty<'a>(primary: &'a str, fallback: &'a str) -> &'a str {
    if primary.trim().is_empty() {
        fallback
    } else {
        primary
    }
}

fn normalize_path(path: &Path) -> String {
    path.to_string_lossy().replace('/', "\\")
}

fn sha256_text(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{
        GlobalManagedFile, InboxSourceEvent, MaterialInboxItem, WorkspaceScanFile,
    };

    #[test]
    fn same_file_projects_consistently_across_inbox_scan_and_global_models() {
        let hash = "same-hash";
        let path = r"D:\资料\VAVE申请单.xlsx";
        let inbox = MaterialInboxItem {
            id: "file-1".to_string(),
            file_name: "VAVE申请单.xlsx".to_string(),
            source_path: path.to_string(),
            source_hash: hash.to_string(),
            file_type: "xlsx".to_string(),
            document_type: "VAVE降本申请单".to_string(),
            document_purpose: "requirement".to_string(),
            business_domain: "研发/质量/降本".to_string(),
            ownership_type: "generalWorkMaterial".to_string(),
            general_material_category: "vave".to_string(),
            content_summary: "VAVE申请确认单".to_string(),
            source_history: vec![InboxSourceEvent {
                source_path: path.to_string(),
                ..InboxSourceEvent::default()
            }],
            ..MaterialInboxItem::default()
        };
        let scan = WorkspaceScanFile {
            id: "file-1".to_string(),
            path: path.to_string(),
            file_name: inbox.file_name.clone(),
            extension: "xlsx".to_string(),
            hash: hash.to_string(),
            document_type: inbox.document_type.clone(),
            document_purpose: inbox.document_purpose.clone(),
            ownership_type: inbox.ownership_type.clone(),
            recommended_category: "vave".to_string(),
            content_summary: inbox.content_summary.clone(),
            ..WorkspaceScanFile::default()
        };
        let global = GlobalManagedFile {
            id: "file-1".to_string(),
            file_name: inbox.file_name.clone(),
            source_path: path.to_string(),
            managed_path: r"D:\GanMaoYuan_Workspace\20_General\engineering\vave\VAVE申请单.xlsx"
                .to_string(),
            managed_relative_path: r"20_General\engineering\vave\VAVE申请单.xlsx".to_string(),
            content_hash: hash.to_string(),
            file_type: "xlsx".to_string(),
            document_type: inbox.document_type.clone(),
            document_purpose: inbox.document_purpose.clone(),
            business_domain: inbox.business_domain.clone(),
            ownership_type: inbox.ownership_type.clone(),
            general_material_category: "vave".to_string(),
            content_summary: inbox.content_summary.clone(),
            ..GlobalManagedFile::default()
        };

        let inbox_projection = FileProjectionAdapter::from_inbox(&inbox);
        let scan_projection = FileProjectionAdapter::from_scan(&scan);
        let global_projection = FileProjectionAdapter::from_global_file(&global);
        assert_eq!(
            inbox_projection.identity.hash,
            scan_projection.identity.hash
        );
        assert_eq!(
            scan_projection.identity.hash,
            global_projection.identity.hash
        );
        assert_eq!(
            inbox_projection.metadata.document_type,
            global_projection.metadata.document_type
        );
        assert_eq!(
            scan_projection.location.ownership_type,
            global_projection.location.ownership_type
        );
    }

    #[test]
    fn location_preview_id_changes_when_target_changes() {
        let first = LocationPreviewService::preview(
            r"D:\资料\a.txt",
            Path::new(r"D:\GanMaoYuan_Workspace\20_General\a.txt"),
            Some(Path::new(r"D:\GanMaoYuan_Workspace")),
            "",
            "generalWorkMaterial",
            "reference",
            "测试",
        );
        let second = LocationPreviewService::preview(
            r"D:\资料\a.txt",
            Path::new(r"D:\GanMaoYuan_Workspace\30_Temporary\a.txt"),
            Some(Path::new(r"D:\GanMaoYuan_Workspace")),
            "",
            "temporaryOrReference",
            "reference",
            "测试",
        );
        assert!(LocationPreviewService::matches(&first.preview_id, &first));
        assert_ne!(first.preview_id, second.preview_id);
    }

    #[test]
    fn work_ledger_timeline_hides_technical_events() {
        let events = vec![
            WorkEvent {
                id: "visible".to_string(),
                event_type: "git.headChanged".to_string(),
                summary: "新增提交 abc123".to_string(),
                ..WorkEvent::default()
            },
            WorkEvent {
                id: "technical".to_string(),
                event_type: "codex.runStarted".to_string(),
                summary: "pid 123".to_string(),
                ..WorkEvent::default()
            },
        ];
        let timeline = ActivityProjectionAdapter::visible_timeline(&events);
        assert_eq!(timeline.len(), 1);
        assert_eq!(timeline[0].activity_type, "gitCommit");
    }

    #[test]
    fn codex_acceptance_and_cleanup_review_become_pending_actions() {
        let manifest = ProjectManifest {
            project: crate::models::ProjectSummary {
                id: "project-1".to_string(),
                next_step: "整理验收结论".to_string(),
                ..crate::models::ProjectSummary::default()
            },
            ..ProjectManifest::default()
        };
        let task = CodexTask {
            task_id: "task-1".to_string(),
            title: "修复扫描".to_string(),
            status: "awaitingAcceptance".to_string(),
            result_id: "result-1".to_string(),
            result_source: "codex".to_string(),
            manual_acceptance: vec!["确认扫描结果".to_string()],
            ..CodexTask::default()
        };
        let task_actions = PendingActionAdapter::from_project(&manifest, &[task], &[]);
        assert!(task_actions
            .iter()
            .any(|action| action.action_type == "codexAcceptance"));

        let plan = CleanupPlan {
            id: "plan-1".to_string(),
            status: "reviewing".to_string(),
            items: vec![crate::models::CleanupPlanItem {
                id: "item-1".to_string(),
                file_name: "资料.xlsx".to_string(),
                review_status: "pending".to_string(),
                recommended_target_path: r"D:\GanMaoYuan_Workspace\20_General\资料.xlsx"
                    .to_string(),
                ..crate::models::CleanupPlanItem::default()
            }],
            ..CleanupPlan::default()
        };
        let cleanup_actions = PendingActionAdapter::from_cleanup(&[plan]);
        assert_eq!(cleanup_actions[0].action_type, "cleanupReview");
    }

    #[test]
    fn accepted_codex_task_title_does_not_become_an_unrecorded_current_next_step() {
        let title = "验收会话超时后的重新认证和输入恢复提示";
        let manifest = ProjectManifest {
            project: crate::models::ProjectSummary {
                id: "project-1".to_string(),
                next_step: title.to_string(),
                ..crate::models::ProjectSummary::default()
            },
            pending_reviews: vec![crate::models::PendingReviewItem {
                id: "scan-review".to_string(),
                title: "确认新发现的文件".to_string(),
                status: "open".to_string(),
                ..crate::models::PendingReviewItem::default()
            }],
            ..ProjectManifest::default()
        };
        let accepted_task = CodexTask {
            task_id: "accepted-task".to_string(),
            project_id: "project-1".to_string(),
            title: title.to_string(),
            status: "completed".to_string(),
            acceptance: crate::models::CodexTaskAcceptance {
                status: "approved".to_string(),
                ..crate::models::CodexTaskAcceptance::default()
            },
            ..CodexTask::default()
        };

        let actions = PendingActionAdapter::from_project(&manifest, &[accepted_task], &[]);

        assert!(!actions.iter().any(|action| {
            action.action_type == "userNextAction" || action.action_type == "codexAcceptance"
        }));
        assert_eq!(
            actions
                .iter()
                .filter(|action| action.action_type == "projectReview")
                .count(),
            1,
            "file review remains pending independently of the accepted Codex task"
        );
    }

    #[test]
    fn independently_recorded_next_step_survives_matching_accepted_task_title() {
        let title = "验收会话超时后的重新认证和输入恢复提示";
        let manifest = ProjectManifest {
            project: crate::models::ProjectSummary {
                id: "project-1".to_string(),
                next_step: title.to_string(),
                ..crate::models::ProjectSummary::default()
            },
            audit: vec![crate::models::AuditEvent {
                action: "workLedger.userNextStep".to_string(),
                target: "project-1".to_string(),
                outcome: "success".to_string(),
                details: title.to_string(),
                confirmed: true,
                ..crate::models::AuditEvent::default()
            }],
            ..ProjectManifest::default()
        };
        let accepted_task = CodexTask {
            task_id: "accepted-task".to_string(),
            project_id: "project-1".to_string(),
            title: title.to_string(),
            status: "completed".to_string(),
            acceptance: crate::models::CodexTaskAcceptance {
                status: "approved".to_string(),
                ..crate::models::CodexTaskAcceptance::default()
            },
            ..CodexTask::default()
        };

        let actions = PendingActionAdapter::from_project(&manifest, &[accepted_task], &[]);

        assert!(actions
            .iter()
            .any(|action| { action.action_type == "userNextAction" && action.title == title }));
    }

    #[test]
    fn accepted_task_from_another_project_cannot_suppress_current_project_next_step() {
        let title = "继续核对项目恢复结果";
        let manifest = ProjectManifest {
            project: crate::models::ProjectSummary {
                id: "project-1".to_string(),
                next_step: title.to_string(),
                ..crate::models::ProjectSummary::default()
            },
            audit: vec![crate::models::AuditEvent {
                action: "workLedger.userNextStep".to_string(),
                target: "project-1".to_string(),
                outcome: "success".to_string(),
                details: title.to_string(),
                confirmed: true,
                ..crate::models::AuditEvent::default()
            }],
            ..ProjectManifest::default()
        };
        let other_project_task = CodexTask {
            task_id: "other-project-task".to_string(),
            project_id: "project-2".to_string(),
            title: title.to_string(),
            status: "completed".to_string(),
            acceptance: crate::models::CodexTaskAcceptance {
                status: "approved".to_string(),
                ..crate::models::CodexTaskAcceptance::default()
            },
            ..CodexTask::default()
        };

        let actions = PendingActionAdapter::from_project(&manifest, &[other_project_task], &[]);

        assert!(actions
            .iter()
            .any(|action| { action.action_type == "userNextAction" && action.title == title }));
    }

    #[test]
    fn accepted_newer_task_suppresses_only_the_same_stale_codex_target() {
        let manifest = ProjectManifest {
            project: crate::models::ProjectSummary {
                id: "project-1".to_string(),
                ..crate::models::ProjectSummary::default()
            },
            ..ProjectManifest::default()
        };
        let stale = CodexTask {
            task_id: "stale".to_string(),
            title: "旧的黄金文件检查".to_string(),
            status: "needsReview".to_string(),
            created_at: "100".to_string(),
            target_files: vec!["docs/codex-golden-path-test.md".to_string()],
            ..CodexTask::default()
        };
        let accepted = CodexTask {
            task_id: "accepted".to_string(),
            title: "新的黄金文件检查".to_string(),
            status: "completed".to_string(),
            created_at: "200".to_string(),
            target_files: vec![
                "D:/GanMaoYuan/Website-Clone/docs/codex-golden-path-test.md".to_string()
            ],
            acceptance: crate::models::CodexTaskAcceptance {
                status: "approved".to_string(),
                ..crate::models::CodexTaskAcceptance::default()
            },
            ..CodexTask::default()
        };
        let unrelated = CodexTask {
            task_id: "unrelated".to_string(),
            title: "仍需检查的不同文件".to_string(),
            status: "needsReview".to_string(),
            result_id: "result-unrelated".to_string(),
            result_source: "codex".to_string(),
            created_at: "100".to_string(),
            target_files: vec!["docs/another-file.md".to_string()],
            ..CodexTask::default()
        };

        let actions =
            PendingActionAdapter::from_project(&manifest, &[stale, accepted, unrelated], &[]);

        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].source_ref, "unrelated");
    }

    #[test]
    fn legacy_message_derived_attention_is_not_a_current_pending_action() {
        let legacy_task = crate::models::TaskRecord {
            id: "legacy-task".to_string(),
            title: "历史需求消息".to_string(),
            status: "active".to_string(),
            source_message_id: "message-1".to_string(),
            created_at: "100".to_string(),
            updated_at: "100".to_string(),
        };
        let manifest = ProjectManifest {
            project: crate::models::ProjectSummary {
                id: "project-1".to_string(),
                ..crate::models::ProjectSummary::default()
            },
            messages: vec![crate::models::WorkspaceMessage {
                id: "message-1".to_string(),
                author: "user".to_string(),
                kind: "requirement".to_string(),
                ..crate::models::WorkspaceMessage::default()
            }],
            tasks: vec![legacy_task],
            project_attentions: vec![crate::models::ProjectAttention {
                id: "attention-1".to_string(),
                attention_type: "staleTask".to_string(),
                title: "任务长期未关闭".to_string(),
                status: "pending".to_string(),
                evidence: vec![crate::models::DecisionTraceEvidence {
                    kind: "task".to_string(),
                    source_id: "legacy-task".to_string(),
                    ..crate::models::DecisionTraceEvidence::default()
                }],
                ..crate::models::ProjectAttention::default()
            }],
            ..ProjectManifest::default()
        };

        let actions = PendingActionAdapter::from_project(&manifest, &[], &[]);

        assert!(actions.is_empty());
    }

    #[test]
    fn analysis_state_proposals_are_not_current_pending_actions() {
        let manifest = ProjectManifest {
            project: crate::models::ProjectSummary {
                id: "project-1".to_string(),
                ..crate::models::ProjectSummary::default()
            },
            project_state_proposals: vec![crate::models::ProjectStateProposal {
                id: "proposal-1".to_string(),
                status: "pending".to_string(),
                ..crate::models::ProjectStateProposal::default()
            }],
            ..ProjectManifest::default()
        };

        let actions = PendingActionAdapter::from_project(&manifest, &[], &[]);

        assert!(actions
            .iter()
            .all(|action| action.action_type != "projectStateConfirmation"));
    }

    #[test]
    fn user_blockers_and_next_actions_are_pending_until_resolved() {
        let manifest = ProjectManifest {
            project: crate::models::ProjectSummary {
                id: "project-1".to_string(),
                next_step: "整理验收结论".to_string(),
                ..crate::models::ProjectSummary::default()
            },
            ..ProjectManifest::default()
        };
        let events = vec![
            WorkEvent {
                id: "blocker".to_string(),
                project_id: "project-1".to_string(),
                source_type: "user".to_string(),
                source_ref: "blocker-source".to_string(),
                event_type: "user.blockerRecorded".to_string(),
                summary: "用户记录阻塞：等待确认".to_string(),
                occurred_at: "2026-09-16T10:00:00Z".to_string(),
                ..WorkEvent::default()
            },
            WorkEvent {
                id: "next".to_string(),
                project_id: "project-1".to_string(),
                source_type: "user".to_string(),
                source_ref: "next-source".to_string(),
                event_type: "user.nextActionRecorded".to_string(),
                summary: "用户记录下一步：整理验收结论".to_string(),
                occurred_at: "2026-09-16T11:00:00Z".to_string(),
                ..WorkEvent::default()
            },
        ];
        let pending = PendingActionAdapter::from_project(&manifest, &[], &events);
        assert_eq!(pending.len(), 2);
        assert!(pending
            .iter()
            .any(|action| action.action_type == "userBlocker"));

        let mut resolved = events;
        resolved.push(WorkEvent {
            id: "resolved".to_string(),
            project_id: "project-1".to_string(),
            source_type: "user".to_string(),
            source_ref: "blocker-source".to_string(),
            event_type: "user.actionResolved".to_string(),
            ..WorkEvent::default()
        });
        let refreshed = PendingActionAdapter::from_project(&manifest, &[], &resolved);
        assert_eq!(refreshed.len(), 1);
        assert_eq!(refreshed[0].action_type, "userNextAction");
    }

    #[test]
    fn stale_user_next_actions_remain_history_but_only_current_one_is_pending() {
        let manifest = ProjectManifest {
            project: crate::models::ProjectSummary {
                id: "project-1".to_string(),
                next_step: "第二个下一步".to_string(),
                ..crate::models::ProjectSummary::default()
            },
            ..ProjectManifest::default()
        };
        let events = vec![
            WorkEvent {
                id: "old-next".to_string(),
                project_id: "project-1".to_string(),
                source_type: "user".to_string(),
                source_ref: "old-source".to_string(),
                event_type: "user.nextActionRecorded".to_string(),
                summary: "用户记录下一步：第一个下一步".to_string(),
                occurred_at: "100".to_string(),
                ..WorkEvent::default()
            },
            WorkEvent {
                id: "current-next".to_string(),
                project_id: "project-1".to_string(),
                source_type: "user".to_string(),
                source_ref: "current-source".to_string(),
                event_type: "user.nextActionRecorded".to_string(),
                summary: "用户记录下一步：第二个下一步".to_string(),
                occurred_at: "200".to_string(),
                ..WorkEvent::default()
            },
        ];

        let actions = PendingActionAdapter::from_project(&manifest, &[], &events);

        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].source_ref, "current-source");
    }

    #[test]
    fn duplicate_events_for_the_same_current_next_step_project_only_once() {
        let manifest = ProjectManifest {
            project: crate::models::ProjectSummary {
                id: "project-1".to_string(),
                next_step: "完成连续性回归".to_string(),
                ..crate::models::ProjectSummary::default()
            },
            ..ProjectManifest::default()
        };
        let event = |id: &str, source_ref: &str, occurred_at: &str| WorkEvent {
            id: id.to_string(),
            project_id: "project-1".to_string(),
            source_type: "user".to_string(),
            source_ref: source_ref.to_string(),
            event_type: "user.nextActionRecorded".to_string(),
            summary: "用户记录下一步：完成连续性回归".to_string(),
            occurred_at: occurred_at.to_string(),
            ..WorkEvent::default()
        };

        let actions = PendingActionAdapter::from_project(
            &manifest,
            &[],
            &[
                event("first", "source-first", "100"),
                event("latest", "source-latest", "200"),
            ],
        );

        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].source_ref, "source-latest");
    }

    #[test]
    fn unbound_codex_history_and_cross_project_tasks_are_not_current_actions() {
        let manifest = ProjectManifest {
            project: crate::models::ProjectSummary {
                id: "project-1".to_string(),
                ..crate::models::ProjectSummary::default()
            },
            ..ProjectManifest::default()
        };
        let stale = CodexTask {
            task_id: "stale-review".to_string(),
            project_id: "project-1".to_string(),
            status: "needsReview".to_string(),
            ..CodexTask::default()
        };
        let failed = CodexTask {
            task_id: "failed".to_string(),
            project_id: "project-1".to_string(),
            status: "failed".to_string(),
            ..CodexTask::default()
        };
        let foreign = CodexTask {
            task_id: "foreign-ready".to_string(),
            project_id: "project-2".to_string(),
            status: "ready".to_string(),
            ..CodexTask::default()
        };

        let actions = PendingActionAdapter::from_project(&manifest, &[stale, failed, foreign], &[]);

        assert!(actions.is_empty());
    }

    #[test]
    fn codex_review_action_explains_missing_task_bound_commit() {
        let manifest = ProjectManifest {
            project: crate::models::ProjectSummary {
                id: "project-1".to_string(),
                ..crate::models::ProjectSummary::default()
            },
            ..ProjectManifest::default()
        };
        let task = CodexTask {
            task_id: "coding-review".to_string(),
            project_id: "project-1".to_string(),
            title: "已修改的代码任务".to_string(),
            task_type: "coding".to_string(),
            status: "needsReview".to_string(),
            result_id: "result-bound-to-task".to_string(),
            result_source: "codex".to_string(),
            git_verification: crate::models::CodexTaskGitVerification {
                status: "unavailable".to_string(),
                reason: "Codex 结果没有报告 commit。".to_string(),
                ..crate::models::CodexTaskGitVerification::default()
            },
            ..CodexTask::default()
        };

        let actions = PendingActionAdapter::from_project(&manifest, &[task], &[]);

        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].source_ref, "coding-review");
        assert!(actions[0].reason.contains("没有报告可核验的 Git commit"));
    }

    #[test]
    fn codex_pending_action_time_uses_result_lifecycle_not_git_refresh_time() {
        let manifest = ProjectManifest {
            project: crate::models::ProjectSummary {
                id: "project-1".to_string(),
                ..crate::models::ProjectSummary::default()
            },
            ..ProjectManifest::default()
        };
        let task = CodexTask {
            task_id: "old-review".to_string(),
            project_id: "project-1".to_string(),
            task_type: "coding".to_string(),
            status: "needsReview".to_string(),
            result_id: "result-1".to_string(),
            result_source: "codex".to_string(),
            result_received_at: "200".to_string(),
            updated_at: "9999".to_string(),
            ..CodexTask::default()
        };

        let actions = PendingActionAdapter::from_project(&manifest, &[task], &[]);

        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].created_at, "200");

        let running_retry = CodexTask {
            status: "awaitingResult".to_string(),
            result_received_at: "200".to_string(),
            handed_off_at: "300".to_string(),
            updated_at: "9999".to_string(),
            ..CodexTask::default()
        };
        assert_eq!(codex_task_action_timestamp(&running_retry), "300");
    }

    #[test]
    fn codex_execution_focus_requires_a_matching_live_run() {
        let manifest = ProjectManifest {
            project: crate::models::ProjectSummary {
                id: "project-1".to_string(),
                ..crate::models::ProjectSummary::default()
            },
            ..ProjectManifest::default()
        };
        let task = CodexTask {
            task_id: "task-1".to_string(),
            project_id: "project-1".to_string(),
            status: "awaitingResult".to_string(),
            ..CodexTask::default()
        };
        let stale_runs = vec![
            CodexRun {
                id: "unknown-run".to_string(),
                task_id: task.task_id.clone(),
                project_id: "project-1".to_string(),
                status: "unknownAfterRestart".to_string(),
                process_alive: false,
                ..CodexRun::default()
            },
            CodexRun {
                id: "exited-run".to_string(),
                task_id: task.task_id.clone(),
                project_id: "project-1".to_string(),
                status: "exited".to_string(),
                process_alive: false,
                ..CodexRun::default()
            },
            CodexRun {
                id: "foreign-run".to_string(),
                task_id: task.task_id.clone(),
                project_id: "project-2".to_string(),
                status: "running".to_string(),
                process_alive: true,
                ..CodexRun::default()
            },
            CodexRun {
                id: "pidless-run".to_string(),
                task_id: task.task_id.clone(),
                project_id: "project-1".to_string(),
                status: "running".to_string(),
                pid: 0,
                process_alive: true,
                ..CodexRun::default()
            },
        ];

        let actions =
            PendingActionAdapter::from_project_with_runs(&manifest, &[task], &stale_runs, &[]);

        assert!(actions
            .iter()
            .all(|action| action.action_type != "codexActive"));
    }

    #[test]
    fn live_codex_run_is_active_but_result_received_is_a_review_action() {
        let manifest = ProjectManifest {
            project: crate::models::ProjectSummary {
                id: "project-1".to_string(),
                ..crate::models::ProjectSummary::default()
            },
            ..ProjectManifest::default()
        };
        let running_task = CodexTask {
            task_id: "running-task".to_string(),
            project_id: "project-1".to_string(),
            status: "running".to_string(),
            ..CodexTask::default()
        };
        let running = CodexRun {
            id: "live-run".to_string(),
            task_id: running_task.task_id.clone(),
            project_id: "project-1".to_string(),
            status: "running".to_string(),
            pid: 42,
            process_alive: true,
            ..CodexRun::default()
        };
        let active = PendingActionAdapter::from_project_with_runs(
            &manifest,
            &[running_task],
            &[running],
            &[],
        );
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].action_type, "codexActive");

        let received_task = CodexTask {
            task_id: "received-task".to_string(),
            project_id: "project-1".to_string(),
            status: "resultReceived".to_string(),
            result_id: "result-1".to_string(),
            result_source: "codex".to_string(),
            ..CodexTask::default()
        };
        let received =
            PendingActionAdapter::from_project_with_runs(&manifest, &[received_task], &[], &[]);
        assert_eq!(received.len(), 1);
        assert_eq!(received[0].action_type, "codexReview");
    }

    #[test]
    fn current_next_step_does_not_promote_analysis_suggestions_or_defaults() {
        for value in [
            "查看资料分析结果，补充项目目标与当前任务。",
            "继续整理项目资料，确认下一步工作。",
            "从 project analysis 产生的建议",
        ] {
            let manifest = ProjectManifest {
                project: crate::models::ProjectSummary {
                    id: "project-1".to_string(),
                    next_step: value.to_string(),
                    ..crate::models::ProjectSummary::default()
                },
                project_analysis: crate::models::ProjectAnalysis {
                    next_steps: vec!["从 project analysis 产生的建议".to_string()],
                    ..crate::models::ProjectAnalysis::default()
                },
                ..ProjectManifest::default()
            };

            assert!(PendingActionAdapter::from_project(&manifest, &[], &[]).is_empty());
        }
    }

    #[test]
    fn unproven_manifest_next_step_is_not_a_user_action() {
        let manifest = ProjectManifest {
            project: crate::models::ProjectSummary {
                id: "project-1".to_string(),
                next_step: "重复文件：session-timeout-policy-parsed.txt".to_string(),
                ..crate::models::ProjectSummary::default()
            },
            ..ProjectManifest::default()
        };

        let actions = PendingActionAdapter::from_project(&manifest, &[], &[]);

        assert!(actions
            .iter()
            .all(|action| action.action_type != "userNextAction"));
    }

    #[test]
    fn explicitly_recorded_next_step_survives_matching_analysis_suggestion() {
        let value = "先完善项目恢复自动化验证";
        let manifest = ProjectManifest {
            project: crate::models::ProjectSummary {
                id: "project-1".to_string(),
                next_step: value.to_string(),
                ..crate::models::ProjectSummary::default()
            },
            project_analysis: crate::models::ProjectAnalysis {
                next_steps: vec![value.to_string()],
                ..crate::models::ProjectAnalysis::default()
            },
            ..ProjectManifest::default()
        };
        let event = WorkEvent {
            project_id: "project-1".to_string(),
            source_type: "user".to_string(),
            event_type: "user.nextActionRecorded".to_string(),
            summary: format!("用户记录下一步：{value}"),
            ..WorkEvent::default()
        };

        let actions = PendingActionAdapter::from_project(&manifest, &[], &[event]);

        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].title, value);
        assert_eq!(actions[0].action_type, "userNextAction");
    }

    #[test]
    fn manifest_next_step_remains_pending_when_its_legacy_ledger_event_was_resolved() {
        let value = "重新明确记录的工作事项";
        let manifest = ProjectManifest {
            project: crate::models::ProjectSummary {
                id: "project-1".to_string(),
                next_step: value.to_string(),
                ..crate::models::ProjectSummary::default()
            },
            audit: vec![crate::models::AuditEvent {
                action: "workLedger.userNextStep".to_string(),
                target: "project-1".to_string(),
                outcome: "success".to_string(),
                details: value.to_string(),
                confirmed: true,
                ..crate::models::AuditEvent::default()
            }],
            ..ProjectManifest::default()
        };
        let events = vec![
            WorkEvent {
                project_id: "project-1".to_string(),
                source_type: "user".to_string(),
                source_ref: "old-next-step".to_string(),
                event_type: "user.nextActionRecorded".to_string(),
                summary: format!("用户记录下一步：{value}"),
                ..WorkEvent::default()
            },
            WorkEvent {
                project_id: "project-1".to_string(),
                source_type: "user".to_string(),
                source_ref: "old-next-step".to_string(),
                event_type: "user.actionResolved".to_string(),
                ..WorkEvent::default()
            },
        ];

        let actions = PendingActionAdapter::from_project(&manifest, &[], &events);

        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].title, value);
        assert_eq!(actions[0].source_ref, "manifest-next-step:project-1");
    }

    #[test]
    fn codex_runs_share_execution_status_and_retry_semantics() {
        let task = CodexTask {
            task_id: "task-1".to_string(),
            status: "awaitingAcceptance".to_string(),
            manual_acceptance: vec!["打开桌面应用验收".to_string()],
            ..CodexTask::default()
        };
        let runs = vec![
            CodexRun {
                id: "run-1".to_string(),
                task_id: task.task_id.clone(),
                status: "failed".to_string(),
                exit_code: Some(2),
                error: "参数无效".to_string(),
                ..CodexRun::default()
            },
            CodexRun {
                id: "run-2".to_string(),
                task_id: task.task_id.clone(),
                status: "exited".to_string(),
                exit_code: Some(0),
                ..CodexRun::default()
            },
        ];
        let records = ExecutionProjectionAdapter::from_codex_runs(&runs, &[task]);
        assert_eq!(records[0].status, "failed");
        assert_eq!(records[0].retry_count, 0);
        assert_eq!(records[1].status, "succeeded");
        assert_eq!(records[1].retry_count, 1);
        assert!(records[1].requires_approval);
        assert_eq!(records[1].approval_status, "pending");
    }

    #[test]
    fn cleanup_execution_failure_and_cancel_use_shared_statuses() {
        let review_batch = CleanupExecutionBatch {
            id: "batch-1".to_string(),
            cleanup_plan_id: "plan-1".to_string(),
            status: "completed".to_string(),
            items: vec![crate::models::CleanupExecutionItem {
                status: "undoConflict".to_string(),
                error: "副本已被修改".to_string(),
                ..crate::models::CleanupExecutionItem::default()
            }],
            ..CleanupExecutionBatch::default()
        };
        let cancelled_batch = CleanupExecutionBatch {
            id: "batch-2".to_string(),
            cleanup_plan_id: "plan-1".to_string(),
            status: "undone".to_string(),
            ..CleanupExecutionBatch::default()
        };
        let records =
            ExecutionProjectionAdapter::from_cleanup_batches(&[review_batch, cancelled_batch]);
        assert_eq!(records[0].status, "needsReview");
        assert_eq!(records[1].status, "cancelled");
        assert_eq!(records[1].retry_count, 1);
    }

    #[test]
    fn evidence_reference_round_trips_without_full_source_content() {
        let evidence = EvidenceRef {
            kind: "file".to_string(),
            id: "file-1".to_string(),
            path_snapshot: r"D:\资料\需求.docx".to_string(),
            hash_snapshot: "hash-1".to_string(),
            label: "需求摘要".to_string(),
        };
        let json = serde_json::to_string(&evidence).unwrap();
        let restored: EvidenceRef = serde_json::from_str(&json).unwrap();
        assert_eq!(restored, evidence);
        assert!(!json.contains("原始正文"));
    }
}
