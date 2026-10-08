use crate::{
    models::{
        DataHealthRecord, ProjectActionCandidate, ProjectManifest, WeeklyHabitSummary,
        WeeklyProjectProgress, WeeklyRepeatedPattern, WeeklyReportExportResult, WeeklyReportRecord,
        WeeklyReportsDocument, WeeklyReviewDashboard, WeeklyReviewSettings, WeeklySkillCandidate,
        WeeklyTaskDistributionItem,
    },
    project_service,
    storage::{
        global_data_dir, new_id, now_string, path_to_string, read_json, write_json_atomic,
        write_text_atomic,
    },
};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};
use tauri::Runtime;

const SETTINGS_FILE: &str = "weekly-review-settings.json";
const GLOBAL_REPORTS_FILE: &str = "weekly-reports.json";

pub fn load_weekly_review_dashboard<R: Runtime>(
    app: &tauri::AppHandle<R>,
    project_root: Option<String>,
) -> Result<WeeklyReviewDashboard, String> {
    let settings = read_settings(app)?;
    let (week_start, week_end, current_week_key) = current_week_window();
    let global_reports = load_reports(read_global_doc(app)?);
    let (project_reports, active_project_root) = match project_root {
        Some(root) if !root.trim().is_empty() => {
            let root_path = PathBuf::from(&root);
            if root_path.exists() {
                let doc = read_project_doc(&root_path)?;
                (load_reports(doc), path_to_string(&root_path))
            } else {
                (Vec::new(), String::new())
            }
        }
        _ => (Vec::new(), String::new()),
    };

    Ok(WeeklyReviewDashboard {
        settings,
        current_week_key,
        week_start,
        week_end,
        generated_at: now_string(),
        generated_now: false,
        global_reports,
        project_reports,
        active_project_root,
    })
}

pub fn load_weekly_review_settings<R: Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<WeeklyReviewSettings, String> {
    read_settings(app)
}

pub fn save_weekly_review_settings<R: Runtime>(
    app: &tauri::AppHandle<R>,
    generation_weekday: u8,
) -> Result<WeeklyReviewSettings, String> {
    let settings = WeeklyReviewSettings {
        generation_weekday,
        last_auto_generated_week_key: read_settings(app)?.last_auto_generated_week_key,
        updated_at: now_string(),
    };
    write_json_atomic(&settings_path(app)?, &settings)?;
    Ok(settings)
}

pub fn generate_weekly_reviews<R: Runtime>(
    app: &tauri::AppHandle<R>,
    project_root: Option<String>,
    week_key: String,
    week_start: String,
    week_end: String,
    force: bool,
) -> Result<WeeklyReviewDashboard, String> {
    let _guard = project_service::project_write_lock()
        .lock()
        .map_err(|_| "项目存储锁已损坏，请重启感冒院。".to_string())?;
    let mut settings = read_settings(app)?;
    let (canonical_start, canonical_end, canonical_key) =
        canonical_week_window(&week_key, &week_start, &week_end);

    let global_report = generate_report_for_scope(
        app,
        "",
        "感冒院全局周报",
        &canonical_key,
        &canonical_start,
        &canonical_end,
        force,
    )?;
    let mut global_doc = read_global_doc(app)?;
    upsert_report(&mut global_doc, global_report.clone());
    write_json_atomic(&global_reports_path(app)?, &global_doc)?;

    let mut project_reports = Vec::new();
    if let Some(root) = project_root
        .clone()
        .filter(|value| !value.trim().is_empty())
    {
        let project_report = generate_report_for_scope(
            app,
            &root,
            "",
            &canonical_key,
            &canonical_start,
            &canonical_end,
            force,
        )?;
        let mut project_doc = read_project_doc(Path::new(&root))?;
        upsert_report(&mut project_doc, project_report.clone());
        write_json_atomic(&project_reports_path(Path::new(&root)), &project_doc)?;
        project_reports.push(project_report);
    }

    settings.last_auto_generated_week_key = canonical_key.clone();
    settings.updated_at = now_string();
    write_json_atomic(&settings_path(app)?, &settings)?;

    Ok(WeeklyReviewDashboard {
        settings,
        current_week_key: canonical_key,
        week_start: canonical_start,
        week_end: canonical_end,
        generated_at: now_string(),
        generated_now: true,
        global_reports: load_reports(read_global_doc(app)?),
        project_reports,
        active_project_root: project_root.unwrap_or_default(),
    })
}

pub fn update_weekly_report_markdown<R: Runtime>(
    app: &tauri::AppHandle<R>,
    project_root: String,
    report_id: String,
    markdown: String,
) -> Result<WeeklyReportRecord, String> {
    let _guard = project_service::project_write_lock()
        .lock()
        .map_err(|_| "项目存储锁已损坏，请重启感冒院。".to_string())?;
    let mut doc = read_doc_for_root(app, &project_root)?;
    let updated_report = {
        let report = doc
            .reports
            .iter_mut()
            .find(|item| item.id == report_id)
            .ok_or_else(|| "周报记录不存在。".to_string())?;
        report.markdown = markdown;
        report.updated_at = now_string();
        write_weekly_report_files(report)?;
        report.clone()
    };
    write_doc_for_root(app, &project_root, &doc)?;
    Ok(updated_report)
}

pub fn confirm_weekly_report<R: Runtime>(
    app: &tauri::AppHandle<R>,
    project_root: String,
    report_id: String,
) -> Result<WeeklyReportRecord, String> {
    let _guard = project_service::project_write_lock()
        .lock()
        .map_err(|_| "项目存储锁已损坏，请重启感冒院。".to_string())?;
    let mut doc = read_doc_for_root(app, &project_root)?;
    let updated_report = {
        let report = doc
            .reports
            .iter_mut()
            .find(|item| item.id == report_id)
            .ok_or_else(|| "周报记录不存在。".to_string())?;
        report.confirmed_at = now_string();
        report.updated_at = report.confirmed_at.clone();
        for candidate in &mut report.skill_candidates {
            if candidate.decision_trace.id.is_empty() {
                candidate.decision_trace = project_service::skill_candidate_decision_trace(
                    &report.project_id,
                    &candidate.name,
                    &candidate.scenario,
                    &candidate.evidence,
                    candidate.confidence,
                );
            }
            candidate.decision_trace.user_decision = "approved".to_string();
            candidate.decision_trace.user_decision_note =
                "用户确认周报中的技能候选，仅供后续人工评审。".to_string();
            candidate.decision_trace.execution = "pending".to_string();
            candidate.decision_trace.execution_note = "未训练、未写入 Atlas。".to_string();
            candidate.decision_trace.updated_at = report.updated_at.clone();
        }
        report.clone()
    };
    write_doc_for_root(app, &project_root, &doc)?;
    Ok(updated_report)
}

pub fn export_weekly_report<R: Runtime>(
    app: &tauri::AppHandle<R>,
    project_root: String,
    report_id: String,
    destination_dir: String,
) -> Result<WeeklyReportExportResult, String> {
    let doc = read_doc_for_root(app, &project_root)?;
    let report = doc
        .reports
        .iter()
        .find(|item| item.id == report_id)
        .ok_or_else(|| "周报记录不存在。".to_string())?;
    let destination = if destination_dir.trim().is_empty() {
        PathBuf::from(r"D:\GanMaoYuan\Exports")
    } else {
        PathBuf::from(destination_dir)
    };
    fs::create_dir_all(&destination)
        .map_err(|err| format!("创建导出目录失败：{}：{err}", path_to_string(&destination)))?;
    let export_dir = destination.join(format!(
        "weekly-review-{}-v{}",
        report.week_key, report.version
    ));
    fs::create_dir_all(&export_dir)
        .map_err(|err| format!("创建导出目录失败：{}：{err}", path_to_string(&export_dir)))?;
    let json_path = export_dir.join("weekly-report.json");
    let md_path = export_dir.join("weekly-report.md");
    let pdf_path = export_dir.join("weekly-report.pdf");
    write_json_atomic(&json_path, report)?;
    write_text_atomic(&md_path, &report.markdown)?;
    write_minimal_pdf(&pdf_path, &report.markdown)?;
    Ok(WeeklyReportExportResult {
        report_id: report.id.clone(),
        destination_dir: path_to_string(&export_dir),
        exported_paths: vec![
            path_to_string(&json_path),
            path_to_string(&md_path),
            path_to_string(&pdf_path),
        ],
        exported_at: now_string(),
    })
}

fn generate_report_for_scope<R: Runtime>(
    app: &tauri::AppHandle<R>,
    project_root: &str,
    project_name: &str,
    week_key: &str,
    week_start: &str,
    week_end: &str,
    force: bool,
) -> Result<WeeklyReportRecord, String> {
    let mut doc = read_doc_for_root(app, project_root)?;
    if !force {
        if let Some(existing) = doc
            .reports
            .iter()
            .filter(|item| item.week_key == week_key)
            .max_by_key(|item| item.version)
        {
            return Ok(existing.clone());
        }
    }
    let manifest = if project_root.trim().is_empty() {
        None
    } else {
        Some(project_service::read_and_repair_manifest(Path::new(
            project_root,
        ))?)
    };
    let version = doc
        .reports
        .iter()
        .filter(|item| item.week_key == week_key)
        .map(|item| item.version)
        .max()
        .unwrap_or(0)
        + 1;
    let report = build_report(
        project_root,
        project_name,
        week_key,
        week_start,
        week_end,
        version,
        manifest,
    )?;
    upsert_report(&mut doc, report.clone());
    write_doc_for_root(app, project_root, &doc)?;
    Ok(report)
}

fn build_report(
    project_root: &str,
    project_name: &str,
    week_key: &str,
    week_start: &str,
    week_end: &str,
    version: u32,
    manifest: Option<ProjectManifest>,
) -> Result<WeeklyReportRecord, String> {
    let manifest = manifest.unwrap_or_default();
    let has_real_evidence = !manifest.messages.is_empty()
        || !manifest.tasks.is_empty()
        || !manifest.decisions.is_empty()
        || !manifest.artifacts.is_empty()
        || !manifest.files.is_empty()
        || !manifest.recovery_points.is_empty()
        || !manifest.audit.is_empty();
    let scope = if project_root.trim().is_empty() {
        "global"
    } else {
        "project"
    };
    let name = if project_name.is_empty() {
        manifest.project.name.clone()
    } else {
        project_name.to_string()
    };
    let completed_items = if has_real_evidence {
        collect_completed_items(&manifest)
    } else {
        Vec::new()
    };
    let blockers = collect_blockers(&manifest);
    let unfinished_items = collect_unfinished_items(&manifest);
    let facts = if has_real_evidence {
        build_facts(&manifest)
    } else {
        vec!["本周没有足够的真实项目记录，未生成完成事项。".to_string()]
    };
    let analysis = if has_real_evidence {
        build_analysis(&manifest)
    } else {
        Vec::new()
    };
    let suggestions = if has_real_evidence {
        build_suggestions(&manifest)
    } else {
        Vec::new()
    };
    let pending_confirmations = manifest
        .pending_reviews
        .iter()
        .filter(|item| item.status != "resolved")
        .map(|item| format!("{}：{}", item.title, item.detail))
        .collect::<Vec<_>>();
    let task_distribution = vec![WeeklyTaskDistributionItem {
        label: "任务".to_string(),
        count: manifest.tasks.len(),
    }];
    let repeated_operations = count_patterns(
        manifest
            .audit
            .iter()
            .map(|item| item.action.as_str())
            .collect::<Vec<_>>()
            .as_slice(),
    );
    let repeated_issues = count_patterns(
        manifest
            .pending_reviews
            .iter()
            .map(|item| item.kind.as_str())
            .collect::<Vec<_>>()
            .as_slice(),
    );
    let habit_summary = if has_real_evidence {
        WeeklyHabitSummary {
            effective_habits: if manifest.messages.is_empty() {
                Vec::new()
            } else {
                vec![format!(
                    "本周共有 {} 条真实工作消息。",
                    manifest.messages.len()
                )]
            },
            inefficient_loops: repeated_operations
                .iter()
                .map(|item| format!("{} 重复 {} 次。", item.label, item.count))
                .collect(),
            frequent_switches: Vec::new(),
            repeated_rework: repeated_issues
                .iter()
                .map(|item| format!("{} 重复 {} 次。", item.label, item.count))
                .collect(),
        }
    } else {
        WeeklyHabitSummary::default()
    };
    let skill_candidates = if has_real_evidence {
        build_skill_candidates(&manifest, scope)
    } else {
        Vec::new()
    };
    let mut data_health_records = manifest.data_health_records.clone();
    if !has_real_evidence {
        data_health_records.push(DataHealthRecord {
            id: new_id(),
            health_type: "emptyWeeklyReport".to_string(),
            severity: "medium".to_string(),
            source: format!("weeklyReport:{week_key}:v{version}"),
            description: "该自然周没有足够真实证据。".to_string(),
            suggested_action: "补充真实任务、对话或成果后再重新生成。".to_string(),
            status: "open".to_string(),
            created_at: now_string(),
        });
    }
    let markdown = build_markdown(
        scope,
        &name,
        week_key,
        week_start,
        week_end,
        &completed_items,
        &blockers,
        &unfinished_items,
        &facts,
        &analysis,
        &suggestions,
        &pending_confirmations,
        &task_distribution,
        &repeated_operations,
        &repeated_issues,
        &habit_summary,
        &skill_candidates,
        &manifest,
    );
    let report_dir = build_report_dir(project_root, week_key, version);
    fs::create_dir_all(&report_dir)
        .map_err(|err| format!("创建周报目录失败：{}：{err}", path_to_string(&report_dir)))?;
    let json_path = report_dir.join("report.json");
    let md_path = report_dir.join("report.md");
    let pdf_path = report_dir.join("report.pdf");
    let now = now_string();
    let project_progress = if scope == "global" {
        Vec::new()
    } else {
        vec![WeeklyProjectProgress {
            project_id: manifest.project.id.clone(),
            project_name: manifest.project.name.clone(),
            completed_items: collect_completed_items(&manifest),
            blockers: collect_blockers(&manifest),
            unfinished_items: collect_unfinished_items(&manifest),
            next_steps: suggestions.clone(),
        }]
    };
    let report = WeeklyReportRecord {
        id: new_id(),
        scope: scope.to_string(),
        project_id: manifest.project.id.clone(),
        project_root: project_root.to_string(),
        project_name: name,
        week_key: week_key.to_string(),
        week_start: week_start.to_string(),
        week_end: week_end.to_string(),
        version,
        generated_at: now.clone(),
        updated_at: now.clone(),
        confirmed_at: String::new(),
        review_required: true,
        markdown: markdown.clone(),
        json_path: path_to_string(&json_path),
        markdown_path: path_to_string(&md_path),
        pdf_path: path_to_string(&pdf_path),
        completed_items: completed_items.clone(),
        achievements: completed_items,
        blockers,
        unfinished_items,
        facts,
        analysis,
        suggestions,
        pending_confirmations,
        task_distribution,
        repeated_operations,
        repeated_issues,
        habit_summary,
        skill_candidates,
        project_progress,
        evidence: manifest.project_analysis.evidence.clone(),
        data_health_records,
    };
    write_json_atomic(&json_path, &report)?;
    write_text_atomic(&md_path, &markdown)?;
    write_minimal_pdf(&pdf_path, &markdown)?;
    Ok(report)
}

fn build_markdown(
    scope: &str,
    project_name: &str,
    week_key: &str,
    week_start: &str,
    week_end: &str,
    completed_items: &[String],
    blockers: &[String],
    unfinished_items: &[String],
    facts: &[String],
    analysis: &[String],
    suggestions: &[String],
    pending_confirmations: &[String],
    task_distribution: &[WeeklyTaskDistributionItem],
    repeated_operations: &[WeeklyRepeatedPattern],
    repeated_issues: &[WeeklyRepeatedPattern],
    habit_summary: &WeeklyHabitSummary,
    skill_candidates: &[WeeklySkillCandidate],
    manifest: &ProjectManifest,
) -> String {
    let mut lines = Vec::new();
    lines.push("# 每周工作复盘与技能沉淀".to_string());
    lines.push(format!(
        "- 范围: {}",
        if scope == "global" {
            "全局"
        } else {
            "项目"
        }
    ));
    lines.push(format!("- 项目: {}", project_name));
    lines.push(format!("- 周标识: {}", week_key));
    lines.push(format!("- 周起止: {} ~ {}", week_start, week_end));
    lines.push(String::new());
    lines.push("## 本周完成事项与成果".to_string());
    push_list_lines(&mut lines, completed_items, "本周未提取到明确完成事项。");
    lines.push(String::new());
    lines.push("## 各项目进展、阻塞和未完成事项".to_string());
    if scope == "global" {
        lines.push("全局周报按项目汇总。".to_string());
    } else {
        lines.push(format!("当前项目：{}", manifest.project.name));
    }
    push_list_lines(&mut lines, blockers, "本周未提取到阻塞事项。");
    push_list_lines(&mut lines, unfinished_items, "本周未提取到未完成事项。");
    lines.push(String::new());
    lines.push("## 工作时间与任务分布概况".to_string());
    if task_distribution.is_empty() {
        lines.push("无任务分布数据。".to_string());
    } else {
        for item in task_distribution {
            lines.push(format!("- {}：{}", item.label, item.count));
        }
    }
    lines.push(String::new());
    lines.push("## 重复出现的操作和问题".to_string());
    push_pattern_lines(&mut lines, repeated_operations, "本周未识别到重复操作。");
    push_pattern_lines(&mut lines, repeated_issues, "本周未识别到重复问题。");
    lines.push(String::new());
    lines.push("## 工作习惯总结".to_string());
    push_list_lines(
        &mut lines,
        &habit_summary.effective_habits,
        "暂无有效习惯总结。",
    );
    push_list_lines(
        &mut lines,
        &habit_summary.inefficient_loops,
        "暂无低效环节总结。",
    );
    push_list_lines(
        &mut lines,
        &habit_summary.frequent_switches,
        "暂无频繁切换总结。",
    );
    push_list_lines(
        &mut lines,
        &habit_summary.repeated_rework,
        "暂无重复返工总结。",
    );
    lines.push(String::new());
    lines.push("## 下周建议和优先事项".to_string());
    push_list_lines(&mut lines, suggestions, "暂无下周建议。");
    lines.push(String::new());
    lines.push("## 可沉淀技能候选".to_string());
    if skill_candidates.is_empty() {
        lines.push("暂无技能候选。".to_string());
    } else {
        for candidate in skill_candidates {
            lines.push(format!(
                "- {} | 场景: {} | 次数: {} | confidence: {:.2} | reviewRequired: {}",
                candidate.name,
                candidate.scenario,
                candidate.occurrence_count,
                candidate.confidence,
                candidate.review_required
            ));
            lines.push(format!(
                "  - 本周真实证据: {}",
                candidate.evidence.join("；")
            ));
            lines.push(format!(
                "  - 可复用步骤: {}",
                candidate.reusable_steps.join("；")
            ));
            lines.push(format!("  - 预期价值: {}", candidate.expected_value));
            lines.push(format!("  - 当前缺口: {}", candidate.current_gap));
        }
    }
    lines.push(String::new());
    lines.push("## 事实 / 分析".to_string());
    push_list_lines(&mut lines, facts, "无可靠事实。");
    push_list_lines(&mut lines, analysis, "无额外分析。");
    lines.push(String::new());
    lines.push("## 待确认".to_string());
    push_list_lines(&mut lines, pending_confirmations, "暂无待确认内容。");
    lines.join("\n")
}

fn collect_completed_items(manifest: &ProjectManifest) -> Vec<String> {
    manifest
        .tasks
        .iter()
        .filter(|task| {
            matches!(
                task.status.as_str(),
                "done" | "completed" | "closed" | "resolved"
            )
        })
        .map(|task| task.title.clone())
        .take(8)
        .collect()
}

fn collect_blockers(manifest: &ProjectManifest) -> Vec<String> {
    let mut items: Vec<String> = manifest
        .pending_reviews
        .iter()
        .filter(|item| item.status != "resolved" && item.status != "ignored")
        .map(|item| item.title.clone())
        .collect();
    for candidate in &manifest.project_action_candidates {
        if (candidate.candidate_type == "blocker" || candidate.candidate_type == "risk")
            && candidate.status == "pending"
            && !items.iter().any(|existing| existing == &candidate.title)
        {
            items.push(candidate.title.clone());
        }
    }
    items.truncate(8);
    items
}

fn collect_unfinished_items(manifest: &ProjectManifest) -> Vec<String> {
    manifest
        .tasks
        .iter()
        .filter(|task| {
            !matches!(
                task.status.as_str(),
                "done" | "completed" | "closed" | "resolved"
            )
        })
        .map(|task| task.title.clone())
        .take(8)
        .collect()
}

fn build_facts(manifest: &ProjectManifest) -> Vec<String> {
    let mut lines = vec![
        format!("项目说明：{}", manifest.project.description),
        format!("项目下一步：{}", manifest.project.next_step),
        format!("对话条数：{}", manifest.messages.len()),
        format!("任务条数：{}", manifest.tasks.len()),
        format!("决策条数：{}", manifest.decisions.len()),
        format!("成果条数：{}", manifest.artifacts.len()),
        format!("文件条数：{}", manifest.files.len()),
        format!(
            "DeepSeek 对话条数：{}",
            manifest
                .messages
                .iter()
                .filter(|m| m.source == "deepseek")
                .count()
        ),
    ];
    if let Some(point) = manifest.recovery_points.last() {
        lines.push(format!(
            "最近恢复点：{} → 下一步：{}",
            point.completed, point.next_step
        ));
    }
    let impact_count = manifest.project_impact_analyses.len();
    if impact_count > 0 {
        lines.push(format!("文件影响分析累计：{} 份", impact_count));
        for analysis in manifest.project_impact_analyses.iter().rev().take(3) {
            lines.push(format!("影响摘要：{}", analysis.summary));
        }
    }
    let candidate_count = manifest.project_action_candidates.len();
    if candidate_count > 0 {
        lines.push(format!("行动候选累计：{} 项", candidate_count));
    }
    lines
}

fn build_analysis(manifest: &ProjectManifest) -> Vec<String> {
    let mut lines = Vec::new();
    if manifest.project_analysis.status == "success" {
        lines.push("项目理解已生成。".to_string());
    } else {
        lines.push(format!(
            "项目理解状态：{}",
            manifest.project_analysis.status
        ));
    }
    if manifest.atlas.status.is_empty() {
        lines.push("Atlas 状态未评估。".to_string());
    } else {
        lines.push(format!("Atlas 状态：{}", manifest.atlas.status));
    }
    if !manifest.pending_reviews.is_empty() {
        lines.push(format!("待检查事项：{} 项", manifest.pending_reviews.len()));
    }
    lines
}

fn build_suggestions(manifest: &ProjectManifest) -> Vec<String> {
    if !manifest.project_analysis.next_steps.is_empty() {
        return manifest.project_analysis.next_steps.clone();
    }
    if !manifest.project.next_step.trim().is_empty() {
        return vec![manifest.project.next_step.clone()];
    }
    Vec::new()
}

pub(crate) fn build_skill_candidates(
    manifest: &ProjectManifest,
    _scope: &str,
) -> Vec<WeeklySkillCandidate> {
    let deepseek_count = manifest
        .messages
        .iter()
        .filter(|m| m.source == "deepseek")
        .count();
    let mut candidates = Vec::new();
    // 从文件影响分析产生的行动候选中聚合重复模式：只有多次出现才视为可沉淀技能。
    let mut groups: HashMap<String, Vec<&ProjectActionCandidate>> = HashMap::new();
    for candidate in &manifest.project_action_candidates {
        if matches!(
            candidate.candidate_type.as_str(),
            "task" | "decision" | "requirementChange"
        ) {
            let key = candidate
                .title
                .to_lowercase()
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            groups.entry(key).or_default().push(candidate);
        }
    }
    for members in groups.values() {
        let occurrence = members.len();
        if occurrence < 2 {
            continue;
        }
        let sample = &members[0];
        let evidence: Vec<String> = members
            .iter()
            .take(5)
            .map(|member| {
                format!(
                    "候选「{}」来源文件 {} 置信 {:.2}",
                    member.title, member.related_file_id, member.confidence
                )
            })
            .collect();
        let reusable_steps = vec![
            format!("识别「{}」类事项", sample.title),
            "与项目已有任务/决定去重".to_string(),
            "确认后沉淀为可复用处理路径".to_string(),
        ];
        let confidence = if occurrence >= 2 { 0.78 } else { 0.55 };
        candidates.push(WeeklySkillCandidate {
            name: format!("重复事项：{}", sample.title),
            scenario: format!("在多个文件中反复出现「{}」类事项", sample.title),
            evidence,
            occurrence_count: occurrence,
            reusable_steps,
            expected_value: "把高频事项固化为标准处理动作，减少重复确认。".to_string(),
            current_gap: if occurrence >= 2 {
                "已观察到重复出现，建议沉淀为技能。".to_string()
            } else {
                "仅出现一次，暂不成熟，继续观察。".to_string()
            },
            confidence,
            review_required: true,
            decision_trace: Default::default(),
        });
    }
    if deepseek_count >= 2 {
        candidates.push(WeeklySkillCandidate {
            name: "DeepSeek 项目理解".to_string(),
            scenario: "用项目说明、摘要和关键历史生成项目理解".to_string(),
            evidence: vec![format!("DeepSeek 对话条数：{}", deepseek_count)],
            occurrence_count: deepseek_count,
            reusable_steps: vec![
                "收集项目说明".to_string(),
                "提取文件摘要".to_string(),
                "整理关键历史和待确认问题".to_string(),
            ],
            expected_value: "稳定项目理解和下一步建议。".to_string(),
            current_gap: "仍缺少更强的自动证据筛选。".to_string(),
            confidence: 0.74,
            review_required: true,
            decision_trace: Default::default(),
        });
    }
    if manifest.codex_reports.len() >= 2 {
        candidates.push(WeeklySkillCandidate {
            name: "Codex 报告收口".to_string(),
            scenario: "导入外部执行结果并更新项目状态".to_string(),
            evidence: vec![format!("Codex 报告数：{}", manifest.codex_reports.len())],
            occurrence_count: manifest.codex_reports.len(),
            reusable_steps: vec![
                "生成收敛提示词".to_string(),
                "导入执行结果".to_string(),
                "确认更新项目".to_string(),
            ],
            expected_value: "减少人工回看和状态遗漏。".to_string(),
            current_gap: "仍需进一步明确报告确认后的自动更新边界。".to_string(),
            confidence: 0.82,
            review_required: true,
            decision_trace: Default::default(),
        });
    }
    candidates.retain(|candidate| {
        candidate.occurrence_count >= 2
            && !candidate.evidence.is_empty()
            && !candidate.reusable_steps.is_empty()
            && candidate.review_required
    });
    for candidate in &mut candidates {
        candidate.decision_trace = project_service::skill_candidate_decision_trace(
            &manifest.project.id,
            &candidate.name,
            &candidate.scenario,
            &candidate.evidence,
            candidate.confidence,
        );
    }
    candidates
}

fn count_patterns(values: &[&str]) -> Vec<WeeklyRepeatedPattern> {
    let mut counts = HashMap::<String, usize>::new();
    for value in values {
        *counts.entry((*value).to_string()).or_insert(0) += 1;
    }
    counts
        .into_iter()
        .filter(|(_, count)| *count > 1)
        .map(|(label, count)| WeeklyRepeatedPattern {
            label,
            count,
            evidence: Vec::new(),
        })
        .collect()
}

fn load_reports(doc: WeeklyReportsDocument) -> Vec<WeeklyReportRecord> {
    let mut reports = doc.reports;
    reports.sort_by(|left, right| {
        right
            .updated_at
            .cmp(&left.updated_at)
            .then_with(|| right.version.cmp(&left.version))
    });
    reports
}

fn upsert_report(doc: &mut WeeklyReportsDocument, report: WeeklyReportRecord) {
    if let Some(existing) = doc.reports.iter_mut().find(|item| {
        item.scope == report.scope
            && item.week_key == report.week_key
            && item.version == report.version
    }) {
        *existing = report;
    } else {
        doc.reports.push(report);
    }
    doc.updated_at = now_string();
}

fn read_settings<R: Runtime>(app: &tauri::AppHandle<R>) -> Result<WeeklyReviewSettings, String> {
    let path = settings_path(app)?;
    if !path.exists() {
        return Ok(WeeklyReviewSettings {
            generation_weekday: 1,
            last_auto_generated_week_key: String::new(),
            updated_at: now_string(),
        });
    }
    read_json(&path)
}

fn read_global_doc<R: Runtime>(app: &tauri::AppHandle<R>) -> Result<WeeklyReportsDocument, String> {
    let path = global_reports_path(app)?;
    if !path.exists() {
        return Ok(WeeklyReportsDocument::default());
    }
    let mut doc: WeeklyReportsDocument = read_json(&path)?;
    let before = doc.clone();
    refresh_weekly_data_health(&mut doc);
    if doc != before {
        write_json_atomic(&path, &doc)?;
    }
    Ok(doc)
}

fn read_project_doc(root: &Path) -> Result<WeeklyReportsDocument, String> {
    let path = project_reports_path(root);
    if !path.exists() {
        return Ok(WeeklyReportsDocument::default());
    }
    let mut doc: WeeklyReportsDocument = read_json(&path)?;
    let before = doc.clone();
    refresh_weekly_data_health(&mut doc);
    if doc != before {
        write_json_atomic(&path, &doc)?;
    }
    Ok(doc)
}

fn refresh_weekly_data_health(doc: &mut WeeklyReportsDocument) {
    let mut counts = HashMap::<String, usize>::new();
    for report in &doc.reports {
        *counts
            .entry(format!(
                "{}|{}|{}",
                report.scope, report.week_key, report.version
            ))
            .or_default() += 1;
    }
    for report in &mut doc.reports {
        let prior = report
            .data_health_records
            .iter()
            .map(|record| (record.health_type.clone(), record.clone()))
            .collect::<HashMap<_, _>>();
        report.data_health_records.retain(|record| {
            !matches!(
                record.health_type.as_str(),
                "duplicateWeekVersion"
                    | "invalidWeekKey"
                    | "emptyWeeklyReport"
                    | "weeklyWithoutEvidence"
            )
        });
        let source = format!("weeklyReport:{}:v{}", report.week_key, report.version);
        let mut add = |health_type: &str, severity: &str, description: &str, action: &str| {
            report
                .data_health_records
                .push(
                    prior
                        .get(health_type)
                        .cloned()
                        .unwrap_or_else(|| DataHealthRecord {
                            id: new_id(),
                            health_type: health_type.to_string(),
                            severity: severity.to_string(),
                            source: source.clone(),
                            description: description.to_string(),
                            suggested_action: action.to_string(),
                            status: "open".to_string(),
                            created_at: now_string(),
                        }),
                );
        };
        if !report.week_key.contains("-W") {
            add(
                "invalidWeekKey",
                "medium",
                "历史周报使用了不稳定的动态周标识。",
                "保留历史版本，新生成报告使用 ISO 自然周标识。",
            );
        }
        if counts
            .get(&format!(
                "{}|{}|{}",
                report.scope, report.week_key, report.version
            ))
            .copied()
            .unwrap_or_default()
            > 1
        {
            add(
                "duplicateWeekVersion",
                "high",
                "同一自然周出现重复版本号。",
                "人工确认后保留一个版本；系统不会自动删除历史。",
            );
        }
        if report.completed_items.is_empty()
            && report.achievements.is_empty()
            && report.evidence.is_empty()
        {
            add(
                "emptyWeeklyReport",
                "medium",
                "周报没有真实完成事项或证据。",
                "补充真实记录后再重新生成。",
            );
        }
        report.skill_candidates.retain(|candidate| {
            candidate.occurrence_count >= 2
                && !candidate.evidence.is_empty()
                && !candidate.reusable_steps.is_empty()
                && candidate.review_required
        });
    }
}

fn read_doc_for_root<R: Runtime>(
    app: &tauri::AppHandle<R>,
    project_root: &str,
) -> Result<WeeklyReportsDocument, String> {
    if project_root.trim().is_empty() {
        read_global_doc(app)
    } else {
        read_project_doc(Path::new(project_root))
    }
}

fn write_doc_for_root<R: Runtime>(
    app: &tauri::AppHandle<R>,
    project_root: &str,
    doc: &WeeklyReportsDocument,
) -> Result<(), String> {
    if project_root.trim().is_empty() {
        write_json_atomic(&global_reports_path(app)?, doc)
    } else {
        write_json_atomic(&project_reports_path(Path::new(project_root)), doc)
    }
}

fn settings_path<R: Runtime>(app: &tauri::AppHandle<R>) -> Result<PathBuf, String> {
    Ok(global_data_dir(app)?.join(SETTINGS_FILE))
}

fn global_reports_path<R: Runtime>(app: &tauri::AppHandle<R>) -> Result<PathBuf, String> {
    Ok(global_data_dir(app)?.join(GLOBAL_REPORTS_FILE))
}

fn project_reports_path(root: &Path) -> PathBuf {
    root.join(".ganmaoyuan/weekly-reports/reports.json")
}

fn build_report_dir(project_root: &str, week_key: &str, version: u32) -> PathBuf {
    if project_root.trim().is_empty() {
        default_global_reports_root()
            .join(week_key)
            .join(format!("v{version}"))
    } else {
        PathBuf::from(project_root)
            .join(".ganmaoyuan/weekly-reports")
            .join(week_key)
            .join(format!("v{version}"))
    }
}

fn default_global_reports_root() -> PathBuf {
    if let Some(configured) = std::env::var_os("GANMAOYUAN_DATA_DIR") {
        return PathBuf::from(configured).join("weekly-reports");
    }
    let legacy = PathBuf::from(r"D:\GanMaoYuan\AppData");
    if legacy.exists() {
        return legacy.join("weekly-reports");
    }
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("com.ganmaoyuan.desktop")
        .join("weekly-reports")
}

fn current_week_window() -> (String, String, String) {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;
    let today = now_ms / 86_400_000;
    let weekday = (today + 3).rem_euclid(7);
    let current_monday = today - weekday;
    let week_start_days = current_monday - 7;
    let week_end_days = current_monday - 1;
    let (year, week) = iso_week_from_days(week_start_days);
    (
        format_civil_date(week_start_days),
        format_civil_date(week_end_days),
        format!("{year}-W{week:02}"),
    )
}

fn canonical_week_window(
    requested_key: &str,
    requested_start: &str,
    requested_end: &str,
) -> (String, String, String) {
    if requested_key.len() == 8
        && requested_key.as_bytes().get(4) == Some(&b'-')
        && requested_key.as_bytes().get(5) == Some(&b'W')
    {
        return (
            requested_start.to_string(),
            requested_end.to_string(),
            requested_key.to_string(),
        );
    }
    if let Ok(start_ms) = requested_start.parse::<i64>() {
        let start_days = start_ms / 86_400_000;
        let end_days = requested_end
            .parse::<i64>()
            .map(|value| value / 86_400_000)
            .unwrap_or(start_days + 6);
        let (year, week) = iso_week_from_days(start_days);
        return (
            format_civil_date(start_days),
            format_civil_date(end_days),
            format!("{year}-W{week:02}"),
        );
    }
    current_week_window()
}

fn civil_from_days(days_since_epoch: i64) -> (i32, u32, u32) {
    let z = days_since_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era as i32 + (era * 400) as i32;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    if month <= 2 {
        year += 1;
    }
    (year, month as u32, day as u32)
}

fn days_from_civil(year: i32, month: u32, day: u32) -> i64 {
    let year = year - i32::from(month <= 2);
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let adjusted_month = month as i32 + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * adjusted_month + 2) / 5 + day as i32 - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era as i64 * 146_097 + day_of_era as i64 - 719_468
}

fn iso_week_from_days(days: i64) -> (i32, u32) {
    let weekday = (days + 3).rem_euclid(7);
    let thursday = days + (3 - weekday);
    let (iso_year, _, _) = civil_from_days(thursday);
    let jan_four = days_from_civil(iso_year, 1, 4);
    let week_one_monday = jan_four - (jan_four + 3).rem_euclid(7);
    let week = ((days - week_one_monday) / 7 + 1) as u32;
    (iso_year, week)
}

fn format_civil_date(days: i64) -> String {
    let (year, month, day) = civil_from_days(days);
    format!("{year:04}-{month:02}-{day:02}")
}

fn push_list_lines(lines: &mut Vec<String>, items: &[String], empty: &str) {
    if items.is_empty() {
        lines.push(empty.to_string());
    } else {
        for item in items {
            lines.push(format!("- {item}"));
        }
    }
}

fn push_pattern_lines(lines: &mut Vec<String>, items: &[WeeklyRepeatedPattern], empty: &str) {
    if items.is_empty() {
        lines.push(empty.to_string());
    } else {
        for item in items {
            lines.push(format!("- {}（{} 次）", item.label, item.count));
        }
    }
}

fn write_weekly_report_files(report: &WeeklyReportRecord) -> Result<(), String> {
    write_json_atomic(Path::new(&report.json_path), report)?;
    write_text_atomic(Path::new(&report.markdown_path), &report.markdown)?;
    write_minimal_pdf(Path::new(&report.pdf_path), &report.markdown)
}

fn write_minimal_pdf(path: &Path, markdown: &str) -> Result<(), String> {
    let mut pdf = String::from("%PDF-1.4\n");
    let mut content = String::from("BT\n/F1 10 Tf\n72 760 Td\n");
    for line in markdown.lines().take(24) {
        content.push_str(&format!("({}) Tj\n0 -13 Td\n", escape_pdf_text(line)));
    }
    content.push_str("ET\n");
    let objects = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>".to_string(),
        format!("<< /Length {} >>\nstream\n{}endstream", content.len(), content),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
    ];
    let mut offsets = Vec::new();
    for (index, object) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.push_str(&format!("{} 0 obj\n{}\nendobj\n", index + 1, object));
    }
    let xref_offset = pdf.len();
    pdf.push_str("xref\n0 6\n0000000000 65535 f \n");
    for offset in offsets {
        pdf.push_str(&format!("{offset:010} 00000 n \n"));
    }
    pdf.push_str(&format!(
        "trailer << /Size 6 /Root 1 0 R >>\nstartxref\n{}\n%%EOF\n",
        xref_offset
    ));
    fs::write(path, pdf).map_err(|err| format!("写入 PDF 失败：{}：{err}", path_to_string(path)))
}

fn escape_pdf_text(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('(', "\\(")
        .replace(')', "\\)")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{ProjectActionCandidate, ProjectManifest, ProjectSummary};

    fn base_manifest() -> ProjectManifest {
        let mut manifest = ProjectManifest::default();
        manifest.project = ProjectSummary {
            id: "p".to_string(),
            name: "测试项目".to_string(),
            ..ProjectSummary::default()
        };
        manifest
    }

    fn candidate(title: &str) -> ProjectActionCandidate {
        ProjectActionCandidate {
            id: title.to_string(),
            candidate_type: "task".to_string(),
            title: title.to_string(),
            status: "pending".to_string(),
            related_file_id: "f1".to_string(),
            confidence: 0.7,
            ..ProjectActionCandidate::default()
        }
    }

    // 场景11：周报技能候选 —— 重复出现才沉淀，且默认 review_required
    #[test]
    fn repeated_action_candidate_becomes_skill_candidate() {
        let mut manifest = base_manifest();
        manifest
            .project_action_candidates
            .push(candidate("整理需求文档"));
        manifest
            .project_action_candidates
            .push(candidate("整理需求文档"));
        let candidates = build_skill_candidates(&manifest, "project");
        let found = candidates.iter().find(|c| c.name.contains("整理需求文档"));
        assert!(found.is_some(), "重复事项应生成技能候选");
        let sc = found.unwrap();
        assert_eq!(sc.occurrence_count, 2);
        assert!(
            (sc.confidence - 0.78).abs() < 0.001,
            "多次出现置信度应为0.78"
        );
        assert!(sc.review_required);
        assert_eq!(sc.decision_trace.decision_type, "skillCandidate");
        assert_eq!(sc.decision_trace.confidence.display, "高 78%");
    }

    #[test]
    fn single_occurrence_does_not_create_skill_candidate() {
        let mut manifest = base_manifest();
        manifest
            .project_action_candidates
            .push(candidate("一次性事项"));
        let candidates = build_skill_candidates(&manifest, "project");
        assert!(candidates.is_empty());
    }

    #[test]
    fn natural_week_key_is_stable_for_same_week() {
        let monday = days_from_civil(2026, 8, 17) * 86_400_000;
        let first = canonical_week_window(
            "week-dynamic-a",
            &monday.to_string(),
            &(monday + 6 * 86_400_000).to_string(),
        );
        let second = canonical_week_window(
            "week-dynamic-b",
            &(monday + 3_600_000).to_string(),
            &(monday + 6 * 86_400_000 + 3_600_000).to_string(),
        );
        assert_eq!(first.2, "2026-W34");
        assert_eq!(first.2, second.2);
    }

    #[test]
    fn duplicate_week_version_and_empty_report_are_flagged() {
        let report = WeeklyReportRecord {
            scope: "project".to_string(),
            week_key: "2026-W34".to_string(),
            version: 1,
            ..WeeklyReportRecord::default()
        };
        let mut doc = WeeklyReportsDocument {
            reports: vec![report.clone(), report],
            ..WeeklyReportsDocument::default()
        };
        refresh_weekly_data_health(&mut doc);
        assert!(doc.reports.iter().all(|item| item
            .data_health_records
            .iter()
            .any(|record| { record.health_type == "duplicateWeekVersion" })));
        assert!(doc.reports.iter().all(|item| item
            .data_health_records
            .iter()
            .any(|record| { record.health_type == "emptyWeeklyReport" })));
    }
}
