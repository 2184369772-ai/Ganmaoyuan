//! Read-only, project-scoped reader for Keep the Why context Markdown.
//!
//! This module deliberately does not write project files or promote Markdown
//! claims into canonical user decisions. The caller supplies verified links to
//! the existing Work Ledger, Git repository, and Codex task records.

use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Component, Path, PathBuf},
};

const MAX_CONTEXT_FILES: usize = 64;
const MAX_FILE_BYTES: usize = 32 * 1024;
const MAX_TOTAL_BYTES: usize = 256 * 1024;
const MAX_ENTRY_BODY_CHARS: usize = 4_000;
const SKIP_FILES: &[&str] = &["index.md", "readme.md", "agents.md", "claude.md"];

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProjectRationaleContext {
    /// `available`, `empty`, `missing`, `invalid`, or `unavailable`.
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub entries: Vec<ProjectRationaleEntry>,
    #[serde(default)]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProjectRationaleEntry {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub types: Vec<String>,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub evidence: String,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub see: Vec<String>,
    #[serde(default)]
    pub superseded_by: String,
    #[serde(default)]
    pub verification: String,
    #[serde(default)]
    pub reason: String,
    #[serde(default)]
    pub rejected_alternatives: Vec<String>,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub source_file: String,
    #[serde(default)]
    pub line_start: usize,
    #[serde(default)]
    pub line_end: usize,
    #[serde(default)]
    pub decision_refs: Vec<String>,
    #[serde(default)]
    pub git_refs: Vec<String>,
    #[serde(default)]
    pub task_refs: Vec<String>,
    #[serde(default)]
    pub verified_links: Vec<String>,
    #[serde(default)]
    pub unresolved_conflict: bool,
}

#[derive(Debug, Clone, Default)]
struct RawEntry {
    title: String,
    line_start: usize,
    lines: Vec<(usize, String)>,
}

/// Reads only flat `context/*.md` files inside the configured repository.
/// Absence is a valid state and never causes project files to be created.
pub(crate) fn read_project_rationale(repository_path: &Path) -> ProjectRationaleContext {
    let Ok(repository_root) = fs::canonicalize(repository_path) else {
        return ProjectRationaleContext {
            status: "unavailable".to_string(),
            warnings: vec!["关联仓库当前不可读取；未读取理由资料。".to_string()],
            ..ProjectRationaleContext::default()
        };
    };
    if !repository_root.is_dir() {
        return ProjectRationaleContext {
            status: "unavailable".to_string(),
            warnings: vec!["关联仓库不是可读取目录；未读取理由资料。".to_string()],
            ..ProjectRationaleContext::default()
        };
    }

    let context_relative = match configured_context_path(&repository_root) {
        Ok(path) => path,
        Err(message) => {
            return ProjectRationaleContext {
                status: "invalid".to_string(),
                warnings: vec![message],
                ..ProjectRationaleContext::default()
            }
        }
    };
    let context_path = repository_root.join(&context_relative);
    let Ok(context_root) = fs::canonicalize(&context_path) else {
        return ProjectRationaleContext {
            status: "missing".to_string(),
            warnings: vec!["当前项目仓库没有可读取的 context/ 理由目录。".to_string()],
            ..ProjectRationaleContext::default()
        };
    };
    if !context_root.starts_with(&repository_root) || !context_root.is_dir() {
        return ProjectRationaleContext {
            status: "invalid".to_string(),
            warnings: vec!["理由目录越出当前关联仓库或不是目录；已拒绝读取。".to_string()],
            ..ProjectRationaleContext::default()
        };
    }

    let mut warnings = Vec::new();
    let Ok(read_dir) = fs::read_dir(&context_root) else {
        return ProjectRationaleContext {
            status: "unavailable".to_string(),
            warnings: vec!["读取当前项目 context/ 目录失败。".to_string()],
            ..ProjectRationaleContext::default()
        };
    };
    let mut files = Vec::<(String, PathBuf)>::new();
    for item in read_dir {
        let Ok(item) = item else {
            warnings.push("有 context/ 目录项无法读取，已跳过。".to_string());
            continue;
        };
        let name = item.file_name().to_string_lossy().to_string();
        if SKIP_FILES
            .iter()
            .any(|skip| name.eq_ignore_ascii_case(skip))
            || !name.to_ascii_lowercase().ends_with(".md")
        {
            continue;
        }
        let Ok(file_type) = item.file_type() else {
            continue;
        };
        if !file_type.is_file() {
            continue;
        }
        files.push((name, item.path()));
    }
    files.sort_by(|left, right| left.0.to_lowercase().cmp(&right.0.to_lowercase()));
    if files.len() > MAX_CONTEXT_FILES {
        warnings.push(format!(
            "context/ 理由文件超过读取上限 {MAX_CONTEXT_FILES}，其余文件未读取。"
        ));
        files.truncate(MAX_CONTEXT_FILES);
    }

    let mut total_bytes = 0_usize;
    let mut entries = Vec::new();
    for (file_name, path) in files {
        let Ok(canonical_file) = fs::canonicalize(&path) else {
            warnings.push(format!("context/{file_name} 无法解析，已跳过。"));
            continue;
        };
        if !canonical_file.starts_with(&context_root)
            || !canonical_file.starts_with(&repository_root)
        {
            warnings.push(format!("context/{file_name} 指向仓库外部，已拒绝读取。"));
            continue;
        }
        let Ok(metadata) = fs::metadata(&canonical_file) else {
            continue;
        };
        let size = metadata.len() as usize;
        if size > MAX_FILE_BYTES || total_bytes.saturating_add(size) > MAX_TOTAL_BYTES {
            warnings.push(format!(
                "context/{file_name} 超出理由资料读取大小上限，已跳过。"
            ));
            continue;
        }
        let Ok(bytes) = fs::read(&canonical_file) else {
            warnings.push(format!("context/{file_name} 无法读取，已跳过。"));
            continue;
        };
        total_bytes += bytes.len();
        let Ok(content) = String::from_utf8(bytes) else {
            warnings.push(format!("context/{file_name} 不是有效 UTF-8，已跳过。"));
            continue;
        };
        let relative = format!("{}/{}", path_component_string(&context_relative), file_name);
        entries.extend(parse_topic_file(&content, &relative));
    }

    mark_same_title_conflicts(&mut entries);
    let status = if entries.is_empty() {
        "empty"
    } else {
        "available"
    };
    ProjectRationaleContext {
        status: status.to_string(),
        entries,
        warnings,
    }
}

pub(crate) fn retrieve_for_query<'a>(
    context: &'a ProjectRationaleContext,
    query: &str,
    limit: usize,
) -> Vec<&'a ProjectRationaleEntry> {
    let terms = query_terms(query);
    let rationale_question = is_rationale_question(query);
    let mut scored = context
        .entries
        .iter()
        .filter_map(|entry| {
            let searchable = format!(
                "{} {} {} {} {} {}",
                entry.title,
                entry.types.join(" "),
                entry.source,
                entry.reason,
                entry.rejected_alternatives.join(" "),
                entry.body
            )
            .to_lowercase();
            let score = terms
                .iter()
                .filter(|term| searchable.contains(term.as_str()))
                .count();
            let broad_match = rationale_question
                && terms.is_empty()
                && matches!(entry.status.as_str(), "active" | "superseded");
            (score > 0 || broad_match).then_some((entry, score))
        })
        .collect::<Vec<_>>();
    scored.sort_by(|(left, left_score), (right, right_score)| {
        right_score
            .cmp(left_score)
            .then_with(|| evidence_rank(&left.evidence).cmp(&evidence_rank(&right.evidence)))
            .then_with(|| status_rank(&left.status).cmp(&status_rank(&right.status)))
            .then_with(|| left.source_file.cmp(&right.source_file))
            .then_with(|| left.line_start.cmp(&right.line_start))
    });
    scored
        .into_iter()
        .take(limit)
        .map(|(entry, _)| entry)
        .collect()
}

pub(crate) fn retrieve_for_codex<'a>(
    context: &'a ProjectRationaleContext,
    task_text: &str,
    limit: usize,
) -> Vec<&'a ProjectRationaleEntry> {
    let terms = query_terms(task_text);
    let needle = task_text.to_lowercase();
    let mut scored = context
        .entries
        .iter()
        .filter(|entry| {
            entry.status == "active"
                && entry.evidence == "confirmed"
                && !entry
                    .verification
                    .to_lowercase()
                    .starts_with("contradicted")
                && !entry.unresolved_conflict
        })
        .filter_map(|entry| {
            let searchable = format!(
                "{} {} {} {} {}",
                entry.title,
                entry.types.join(" "),
                entry.reason,
                entry.rejected_alternatives.join(" "),
                entry.body
            )
            .to_lowercase();
            let mut score = terms
                .iter()
                .filter(|term| searchable.contains(term.as_str()))
                .count();
            if !needle.trim().is_empty() && searchable.contains(needle.trim()) {
                score += 2;
            }
            (score > 0).then_some((entry, score))
        })
        .collect::<Vec<_>>();
    scored.sort_by(|(left, left_score), (right, right_score)| {
        right_score
            .cmp(left_score)
            .then_with(|| left.source_file.cmp(&right.source_file))
            .then_with(|| left.line_start.cmp(&right.line_start))
    });
    scored
        .into_iter()
        .take(limit)
        .map(|(entry, _)| entry)
        .collect()
}

fn configured_context_path(repository_root: &Path) -> Result<PathBuf, String> {
    let marker = repository_root.join(".keep-the-why");
    if !marker.exists() {
        return Ok(PathBuf::from("context"));
    }
    let canonical_marker = fs::canonicalize(&marker)
        .map_err(|_| "Keep the Why 配置无法解析；已拒绝读取理由资料。".to_string())?;
    if !canonical_marker.starts_with(repository_root) || !canonical_marker.is_file() {
        return Err("Keep the Why 配置不属于当前仓库；已拒绝读取理由资料。".to_string());
    }
    let metadata = fs::metadata(&canonical_marker)
        .map_err(|_| "Keep the Why 配置无法读取；已拒绝读取理由资料。".to_string())?;
    if metadata.len() as usize > 16 * 1024 {
        return Err("Keep the Why 配置超出安全读取上限；已拒绝读取理由资料。".to_string());
    }
    let bytes = fs::read(canonical_marker)
        .map_err(|_| "Keep the Why 配置无法读取；已拒绝读取理由资料。".to_string())?;
    let content = String::from_utf8(bytes)
        .map_err(|_| "Keep the Why 配置不是有效 UTF-8；已拒绝读取理由资料。".to_string())?;
    let mut in_config = false;
    let mut saw_open = false;
    let mut saw_close = false;
    let mut context_values = Vec::new();
    for line in content.lines().map(str::trim) {
        match line {
            "<!-- keep-the-why:config -->" if !in_config && !saw_open => {
                in_config = true;
                saw_open = true;
            }
            "<!-- /keep-the-why:config -->" if in_config => {
                in_config = false;
                saw_close = true;
            }
            "<!-- keep-the-why:config -->" => {
                return Err("Keep the Why 配置块重复或格式无效；已拒绝读取。".to_string());
            }
            _ if in_config => {
                if let Some(value) = line.strip_prefix("- context:") {
                    context_values.push(value.trim().trim_matches('`').to_string());
                }
            }
            _ => {}
        }
    }
    if in_config || !saw_open || !saw_close || context_values.len() != 1 {
        return Err("Keep the Why 配置缺少唯一、完整的 context 字段；已拒绝读取。".to_string());
    }
    let value = context_values.remove(0);
    let relative = PathBuf::from(value);
    if relative.as_os_str().is_empty()
        || relative.is_absolute()
        || relative.components().any(|part| {
            matches!(
                part,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err("Keep the Why context 路径必须是仓库内相对路径；已拒绝读取。".to_string());
    }
    Ok(relative)
}

fn path_component_string(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

fn parse_topic_file(content: &str, source_file: &str) -> Vec<ProjectRationaleEntry> {
    let mut raw_entries = Vec::new();
    let mut current: Option<RawEntry> = None;
    let mut in_fence = false;
    for (index, line) in content.lines().enumerate() {
        let line_number = index + 1;
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        if let Some(title) = trimmed.strip_prefix("## ") {
            if let Some(entry) = current.take() {
                raw_entries.push(entry);
            }
            current = Some(RawEntry {
                title: title.trim().to_string(),
                line_start: line_number,
                lines: Vec::new(),
            });
            continue;
        }
        if trimmed.starts_with("# ") {
            if let Some(entry) = current.take() {
                raw_entries.push(entry);
            }
            continue;
        }
        if let Some(entry) = &mut current {
            entry.lines.push((line_number, line.to_string()));
        }
    }
    if let Some(entry) = current {
        raw_entries.push(entry);
    }
    raw_entries
        .into_iter()
        .filter_map(|entry| parse_raw_entry(entry, source_file))
        .collect()
}

fn parse_raw_entry(raw: RawEntry, source_file: &str) -> Option<ProjectRationaleEntry> {
    let mut fields = std::collections::HashMap::<String, Vec<String>>::new();
    let mut body_lines = Vec::new();
    let mut in_header = true;
    for (line_number, line) in &raw.lines {
        let trimmed = line.trim();
        if in_header {
            if trimmed.is_empty() {
                continue;
            }
            if let Some((field, value)) = parse_header_field(trimmed) {
                fields.entry(field).or_default().push(value);
                continue;
            }
            in_header = false;
        }
        body_lines.push((*line_number, line.as_str()));
    }
    let one = |field: &str| {
        fields
            .get(field)
            .and_then(|values| (values.len() == 1).then(|| values[0].trim().to_string()))
            .unwrap_or_default()
    };
    let status = one("status").to_ascii_lowercase();
    let evidence = one("evidence").to_ascii_lowercase();
    if !matches!(
        status.as_str(),
        "active" | "superseded" | "open" | "needs-review" | "pending-confirmation"
    ) || !matches!(evidence.as_str(), "confirmed" | "inferred" | "unknown")
        || raw.title.trim().is_empty()
    {
        return None;
    }
    let body = truncate_chars(
        &body_lines
            .iter()
            .map(|(_, line)| *line)
            .collect::<Vec<_>>()
            .join("\n")
            .trim()
            .to_string(),
        MAX_ENTRY_BODY_CHARS,
    );
    let reason = labeled_paragraphs(&body, "Reason").join("\n");
    let rejected_alternatives = labeled_paragraphs(&body, "Rejected alternative");
    let all_text = format!(
        "{} {} {} {} {} {}",
        one("source"),
        one("see"),
        one("superseded by"),
        raw.title,
        body,
        fields
            .values()
            .flatten()
            .cloned()
            .collect::<Vec<_>>()
            .join(" ")
    );
    let line_end = body_lines
        .last()
        .map(|(line_number, _)| *line_number)
        .unwrap_or(raw.line_start);
    Some(ProjectRationaleEntry {
        id: one("id"),
        title: raw.title,
        types: fields.get("type").cloned().unwrap_or_default(),
        status,
        evidence,
        source: one("source"),
        see: fields.get("see").cloned().unwrap_or_default(),
        superseded_by: one("superseded by"),
        verification: one("verification"),
        reason,
        rejected_alternatives,
        body,
        source_file: source_file.to_string(),
        line_start: raw.line_start,
        line_end,
        decision_refs: labelled_ids(&all_text, &["decision:", "decisionrecord:"]),
        git_refs: labelled_git_refs(&all_text),
        task_refs: labelled_ids(&all_text, &["task:", "taskid:", "codextask:"]),
        ..ProjectRationaleEntry::default()
    })
}

fn parse_header_field(line: &str) -> Option<(String, String)> {
    let inner = line.strip_prefix("**")?;
    let (field, value) = inner.split_once(":**")?;
    let field = field.trim().to_ascii_lowercase();
    if !matches!(
        field.as_str(),
        "id" | "type"
            | "status"
            | "evidence"
            | "source"
            | "verification"
            | "revisit when"
            | "see"
            | "superseded by"
    ) {
        return None;
    }
    Some((field, value.trim().to_string()))
}

fn labeled_paragraphs(body: &str, label: &str) -> Vec<String> {
    let prefix = format!("**{label}:**").to_ascii_lowercase();
    let mut values = Vec::new();
    let mut current: Option<String> = None;
    for line in body.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if let Some(value) = current.take() {
                values.push(value);
            }
            continue;
        }
        let lower = trimmed.to_ascii_lowercase();
        if let Some(rest) = lower.strip_prefix(&prefix) {
            if let Some(value) = current.take() {
                values.push(value);
            }
            let value_start = trimmed.len().saturating_sub(rest.len());
            current = Some(trimmed[value_start..].trim().to_string());
        } else if current.is_some() && !trimmed.starts_with("**") {
            current.as_mut().unwrap().push(' ');
            current.as_mut().unwrap().push_str(trimmed);
        } else if current.is_some() {
            values.push(current.take().unwrap());
        }
    }
    if let Some(value) = current {
        values.push(value);
    }
    values
        .into_iter()
        .map(|value| truncate_chars(value.trim(), 600))
        .filter(|value| !value.is_empty())
        .collect()
}

fn labelled_ids(text: &str, prefixes: &[&str]) -> Vec<String> {
    let lower = text.to_ascii_lowercase();
    let mut ids = Vec::new();
    for prefix in prefixes {
        let mut cursor = 0;
        while let Some(offset) = lower[cursor..].find(prefix) {
            let start = cursor + offset + prefix.len();
            let candidate = lower[start..]
                .trim_start_matches(|ch: char| !ch.is_ascii_hexdigit())
                .chars()
                .take(36)
                .collect::<String>();
            let id = candidate.trim_matches(|ch: char| !ch.is_ascii_hexdigit() && ch != '-');
            if is_uuid_shape(id) && !ids.iter().any(|existing| existing == id) {
                ids.push(id.to_string());
            }
            cursor = start;
            if cursor >= lower.len() {
                break;
            }
        }
    }
    ids
}

fn labelled_git_refs(text: &str) -> Vec<String> {
    let lower = text.to_ascii_lowercase();
    let mut refs = Vec::new();
    for label in ["commit", "git"] {
        let mut cursor = 0;
        while let Some(offset) = lower[cursor..].find(label) {
            let after_label = cursor + offset + label.len();
            let rest = lower[after_label..].trim_start_matches(|ch: char| {
                ch.is_whitespace() || matches!(ch, ':' | '`' | '#' | '=')
            });
            let token = rest
                .chars()
                .take_while(|ch| ch.is_ascii_hexdigit())
                .collect::<String>();
            if (7..=40).contains(&token.len()) && !refs.iter().any(|existing| existing == &token) {
                refs.push(token);
            }
            cursor = after_label;
            if cursor >= lower.len() {
                break;
            }
        }
    }
    refs
}

fn is_uuid_shape(value: &str) -> bool {
    value.len() == 36
        && value.chars().enumerate().all(|(index, ch)| {
            if [8, 13, 18, 23].contains(&index) {
                ch == '-'
            } else {
                ch.is_ascii_hexdigit()
            }
        })
}

fn mark_same_title_conflicts(entries: &mut [ProjectRationaleEntry]) {
    let active_titles = entries
        .iter()
        .filter(|entry| entry.status == "active")
        .map(|entry| normalize_title(&entry.title))
        .filter(|title| !title.is_empty())
        .fold(
            std::collections::HashMap::<String, usize>::new(),
            |mut counts, title| {
                *counts.entry(title).or_default() += 1;
                counts
            },
        );
    for entry in entries.iter_mut().filter(|entry| entry.status == "active") {
        if active_titles
            .get(&normalize_title(&entry.title))
            .copied()
            .unwrap_or(0)
            > 1
        {
            entry.unresolved_conflict = true;
        }
    }
}

fn normalize_title(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn query_terms(query: &str) -> Vec<String> {
    let chars = query.to_lowercase().chars().collect::<Vec<_>>();
    let mut terms = Vec::new();
    let mut latin = String::new();
    let mut cjk = String::new();
    let flush_latin = |latin: &mut String, terms: &mut Vec<String>| {
        if latin.chars().count() >= 3
            && ![
                "why", "what", "how", "was", "were", "this", "that", "with", "from",
            ]
            .contains(&latin.as_str())
            && !terms.contains(latin)
        {
            terms.push(std::mem::take(latin));
        } else {
            latin.clear();
        }
    };
    let flush_cjk = |cjk: &mut String, terms: &mut Vec<String>| {
        let run = std::mem::take(cjk);
        let run_chars = run.chars().collect::<Vec<_>>();
        if run_chars.len() >= 2
            && ![
                "为什么",
                "怎么",
                "如何",
                "以前",
                "哪些",
                "以及",
                "这个",
                "那个",
                "哪些方案",
            ]
            .contains(&run.as_str())
        {
            if run_chars.len() <= 8 && !terms.contains(&run) {
                terms.push(run.clone());
            }
            for size in [4_usize, 3, 2] {
                if run_chars.len() >= size {
                    for window in run_chars.windows(size) {
                        let term = window.iter().collect::<String>();
                        if ![
                            "为什么",
                            "为何",
                            "怎么",
                            "如何",
                            "理由",
                            "原因",
                            "设计",
                            "方案",
                            "以前",
                            "当前",
                            "用户",
                            "项目",
                            "记录",
                            "以及",
                            "这个",
                            "那个",
                            "哪些",
                            "什么",
                            "可以",
                            "是否",
                            "应该",
                            "要求",
                            "不能",
                        ]
                        .contains(&term.as_str())
                            && !terms.contains(&term)
                        {
                            terms.push(term);
                        }
                    }
                }
            }
        }
    };
    for ch in chars {
        if ch.is_ascii_alphanumeric() {
            flush_cjk(&mut cjk, &mut terms);
            latin.push(ch);
        } else if ('\u{3400}'..='\u{9fff}').contains(&ch) {
            flush_latin(&mut latin, &mut terms);
            cjk.push(ch);
        } else {
            flush_latin(&mut latin, &mut terms);
            flush_cjk(&mut cjk, &mut terms);
        }
    }
    flush_latin(&mut latin, &mut terms);
    flush_cjk(&mut cjk, &mut terms);
    terms
}

pub(crate) fn is_rationale_question(query: &str) -> bool {
    [
        "为什么",
        "为何",
        "理由",
        "原因",
        "否决",
        "拒绝过",
        "不能违反",
        "约束",
        "设计依据",
        "why",
        "rationale",
        "rejected",
        "constraint",
    ]
    .iter()
    .any(|marker| query.to_lowercase().contains(&marker.to_lowercase()))
}

fn evidence_rank(value: &str) -> u8 {
    match value {
        "confirmed" => 0,
        "inferred" => 1,
        _ => 2,
    }
}

fn status_rank(value: &str) -> u8 {
    match value {
        "active" => 0,
        "superseded" => 1,
        "needs-review" => 2,
        "open" => 3,
        "pending-confirmation" => 4,
        _ => 5,
    }
}

fn truncate_chars(value: &str, limit: usize) -> String {
    let mut chars = value.chars();
    let text = chars.by_ref().take(limit).collect::<String>();
    if chars.next().is_some() {
        format!("{text}…")
    } else {
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::test_root;

    fn record(title: &str, status: &str, evidence: &str) -> ProjectRationaleEntry {
        ProjectRationaleEntry {
            title: title.to_string(),
            status: status.to_string(),
            evidence: evidence.to_string(),
            ..ProjectRationaleEntry::default()
        }
    }

    #[test]
    fn reads_only_scoped_markdown_and_preserves_provenance_fields() {
        let root = test_root("why-reader");
        let context = root.join("context");
        fs::create_dir_all(&context).unwrap();
        fs::write(
            root.join(".keep-the-why"),
            "<!-- keep-the-why:config -->\n- context: `context/`\n<!-- /keep-the-why:config -->\n",
        )
        .unwrap();
        fs::write(
            context.join("storage.md"),
            "# Storage\n\n## Snapshot ordering\n\n**Id:** 550e8400-e29b-41d4-a716-446655440000\n**Type:** decision\n**Status:** active\n**Evidence:** confirmed\n**Source:** verified test fixture\n\n**Reason:** preserve recoverability.\n\n**Rejected alternative:** skip snapshots.\n",
        )
        .unwrap();
        fs::write(context.join("index.md"), "# Context index\n").unwrap();
        fs::write(root.join("docs.md"), "## Not context\n**Status:** active\n").unwrap();

        let result = read_project_rationale(&root);
        assert_eq!(result.status, "available");
        assert_eq!(result.entries.len(), 1);
        let entry = &result.entries[0];
        assert_eq!(entry.source_file, "context/storage.md");
        assert_eq!(entry.status, "active");
        assert_eq!(entry.evidence, "confirmed");
        assert_eq!(entry.reason, "preserve recoverability.");
        assert_eq!(entry.rejected_alternatives, vec!["skip snapshots."]);
        assert_eq!(entry.line_start, 3);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_context_path_escape_and_does_not_fall_back() {
        let root = test_root("why-escape");
        fs::create_dir_all(root.join("context")).unwrap();
        fs::write(
            root.join(".keep-the-why"),
            "<!-- keep-the-why:config -->\n- context: `../outside`\n<!-- /keep-the-why:config -->\n",
        )
        .unwrap();
        let result = read_project_rationale(&root);
        assert_eq!(result.status, "invalid");
        assert!(result.entries.is_empty());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn query_retrieval_keeps_history_and_marks_same_title_active_conflict() {
        let mut context = ProjectRationaleContext {
            status: "available".to_string(),
            entries: vec![
                record("Database migration guard", "active", "confirmed"),
                record("Database migration guard", "active", "confirmed"),
                record("Database migration guard", "superseded", "confirmed"),
            ],
            warnings: Vec::new(),
        };
        mark_same_title_conflicts(&mut context.entries);
        let matches = retrieve_for_query(&context, "为什么数据库 migration guard", 5);
        assert_eq!(matches.len(), 3);
        assert!(matches[0].unresolved_conflict);
        assert_eq!(matches[2].status, "superseded");
        assert!(retrieve_for_codex(&context, "database migration guard", 5).is_empty());
    }

    #[test]
    fn codex_retrieval_excludes_inferred_historical_and_unverified_records() {
        let context = ProjectRationaleContext {
            status: "available".to_string(),
            entries: vec![
                ProjectRationaleEntry {
                    title: "File import boundary".to_string(),
                    status: "active".to_string(),
                    evidence: "confirmed".to_string(),
                    reason: "Only explicit imports enter Inbox.".to_string(),
                    source_file: "context/file-import.md".to_string(),
                    line_start: 3,
                    ..ProjectRationaleEntry::default()
                },
                ProjectRationaleEntry {
                    title: "File import boundary".to_string(),
                    status: "active".to_string(),
                    evidence: "inferred".to_string(),
                    reason: "Likely exclude repository scans.".to_string(),
                    ..ProjectRationaleEntry::default()
                },
                ProjectRationaleEntry {
                    title: "File import boundary".to_string(),
                    status: "superseded".to_string(),
                    evidence: "confirmed".to_string(),
                    reason: "An earlier rejected approach.".to_string(),
                    ..ProjectRationaleEntry::default()
                },
                ProjectRationaleEntry {
                    title: "File import boundary".to_string(),
                    status: "active".to_string(),
                    evidence: "confirmed".to_string(),
                    verification: "contradicted — unresolved".to_string(),
                    reason: "Contradicted record.".to_string(),
                    ..ProjectRationaleEntry::default()
                },
            ],
            warnings: Vec::new(),
        };
        let selected = retrieve_for_codex(&context, "file import Inbox", 8);
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].reason, "Only explicit imports enter Inbox.");
    }

    #[test]
    fn separate_project_roots_never_share_rationale_files() {
        let project_a = test_root("why-isolation-a");
        let project_b = test_root("why-isolation-b");
        for (root, title) in [
            (&project_a, "Project A boundary"),
            (&project_b, "Project B boundary"),
        ] {
            let context = root.join("context");
            fs::create_dir_all(&context).unwrap();
            fs::write(
                context.join("boundary.md"),
                format!("# Boundary\n\n## {title}\n\n**Status:** active\n**Evidence:** confirmed\n\n**Reason:** project-local record.\n"),
            )
            .unwrap();
        }
        let a = read_project_rationale(&project_a);
        let b = read_project_rationale(&project_b);
        assert_eq!(a.entries[0].title, "Project A boundary");
        assert_eq!(b.entries[0].title, "Project B boundary");
        assert!(!a
            .entries
            .iter()
            .any(|entry| entry.title == "Project B boundary"));
        assert!(!b
            .entries
            .iter()
            .any(|entry| entry.title == "Project A boundary"));
        fs::remove_dir_all(project_a).unwrap();
        fs::remove_dir_all(project_b).unwrap();
    }

    #[test]
    fn reloading_context_after_restart_reads_the_same_disk_evidence() {
        let root = test_root("why-restart");
        let context = root.join("context");
        fs::create_dir_all(&context).unwrap();
        fs::write(
            context.join("constraint.md"),
            "# Constraints\n\n## No data writes\n\n**Status:** active\n**Evidence:** confirmed\n\n**Reason:** the first phase is read only.\n",
        )
        .unwrap();
        let first = read_project_rationale(&root);
        let after_restart = read_project_rationale(&root);
        assert_eq!(first, after_restart);
        assert_eq!(
            after_restart.entries[0].source_file,
            "context/constraint.md"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn missing_why_context_is_explicit_and_read_only() {
        let root = test_root("why-missing");
        fs::create_dir_all(&root).unwrap();
        let result = read_project_rationale(&root);
        assert_eq!(result.status, "missing");
        assert!(result.entries.is_empty());
        assert!(!root.join(".keep-the-why").exists());
        assert!(!root.join("context").exists());
        fs::remove_dir_all(root).unwrap();
    }
}
