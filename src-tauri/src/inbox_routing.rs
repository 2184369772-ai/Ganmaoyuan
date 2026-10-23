use crate::{
    models::{InboxStatusTransition, MaterialInboxItem},
    storage::now_string,
};
use std::path::Path;

pub const RECEIVED: &str = "received";
pub const ANALYZING: &str = "analyzing";
pub const PENDING_REVIEW: &str = "pendingReview";
pub const READY_TO_ROUTE: &str = "readyToRoute";
pub const ROUTING: &str = "routing";
pub const ROUTED: &str = "routed";
pub const PROJECT_CREATED: &str = "projectCreated";
pub const IGNORED: &str = "ignored";
pub const FAILED: &str = "failed";

// 一级归属类型（V3 inbox 冲刺）。判断发生在项目匹配之前：
// 只有 existingProject 才继续执行项目匹配。
pub const OWNERSHIP_EXISTING_PROJECT: &str = "existingProject";
pub const OWNERSHIP_GENERAL_WORK: &str = "generalWorkMaterial";
pub const OWNERSHIP_NEW_PROJECT: &str = "newProjectCandidate";
pub const OWNERSHIP_TEMPORARY: &str = "temporaryOrReference";
pub const OWNERSHIP_NEEDS_REVIEW: &str = "needsReview";
pub const OWNERSHIP_UNSUPPORTED: &str = "unsupportedOrFailed";

/// 全局归位目标目录代码。
pub const GLOBAL_DEST_GENERAL: &str = "general";
pub const GLOBAL_DEST_TEMPORARY: &str = "temporary";

pub fn transition(
    item: &mut MaterialInboxItem,
    to: &str,
    reason: impl Into<String>,
    actor: &str,
    error: Option<(&str, &str)>,
) -> Result<(), String> {
    let from = item.processing_status.clone();
    if !is_valid_status(to) {
        return Err(format!("未知 Inbox 状态：{to}"));
    }
    if !from.is_empty() && from != to && !is_legal_transition(&from, to) {
        let message = format!("拒绝非法 Inbox 状态跳转：{from} -> {to}");
        item.status_history.push(InboxStatusTransition {
            from,
            to: to.to_string(),
            changed_at: now_string(),
            reason: reason.into(),
            actor: actor.to_string(),
            error_code: "invalidTransition".to_string(),
            error_message: message.clone(),
        });
        return Err(message);
    }
    let changed_at = now_string();
    let (error_code, error_message) = error
        .map(|(code, message)| (code.to_string(), message.to_string()))
        .unwrap_or_default();
    item.status_history.push(InboxStatusTransition {
        from,
        to: to.to_string(),
        changed_at: changed_at.clone(),
        reason: reason.into(),
        actor: actor.to_string(),
        error_code: error_code.clone(),
        error_message: error_message.clone(),
    });
    item.processing_status = to.to_string();
    item.last_transition_at = changed_at.clone();
    item.updated_at = changed_at;
    item.error_code = error_code;
    item.error_message = error_message;
    Ok(())
}

pub fn migrate_legacy_item(item: &mut MaterialInboxItem) -> bool {
    let legacy = item.processing_status.clone();
    let current = match legacy.as_str() {
        "pending_review" => PENDING_REVIEW,
        "pending_confirmation" => {
            if item.target_project_root.is_empty() {
                PENDING_REVIEW
            } else {
                READY_TO_ROUTE
            }
        }
        "imported" => ROUTED,
        "created_project" => PROJECT_CREATED,
        "" => RECEIVED,
        value if is_valid_status(value) => value,
        _ => PENDING_REVIEW,
    };
    let mut changed = legacy != current;
    item.processing_status = current.to_string();
    if item.received_count == 0 {
        item.received_count = 1;
        changed = true;
    }
    if item.source_history.is_empty() && !item.source_path.is_empty() {
        item.source_history.push(crate::models::InboxSourceEvent {
            source_path: item.source_path.clone(),
            received_from: item.received_from.clone(),
            received_at: item.received_at.clone(),
            hash: item.source_hash.clone(),
        });
        changed = true;
    }
    if item.suggested_file_name.is_empty() && !item.file_name.is_empty() {
        item.suggested_file_name = normalize_managed_file_name(&item.file_name, None);
        changed = true;
    }
    if item.recommended_relative_location.is_empty() && !item.suggested_category.is_empty() {
        item.recommended_relative_location =
            semantic_location(&item.suggested_category).to_string();
        changed = true;
    }
    if item.confidence_level.is_empty() {
        item.confidence_level = if item.target_project_root.is_empty() {
            "low"
        } else {
            "medium"
        }
        .to_string();
        changed = true;
    }
    if item.status_history.is_empty() {
        let received_at = if item.received_at.is_empty() {
            item.created_at.clone()
        } else {
            item.received_at.clone()
        };
        item.status_history.push(InboxStatusTransition {
            from: String::new(),
            to: RECEIVED.to_string(),
            changed_at: received_at,
            reason: "兼容迁移：恢复既有 Inbox 接收记录。".to_string(),
            actor: "system".to_string(),
            ..InboxStatusTransition::default()
        });
        if current != RECEIVED {
            item.status_history.push(InboxStatusTransition {
                from: RECEIVED.to_string(),
                to: current.to_string(),
                changed_at: item.updated_at.clone(),
                reason: "兼容迁移：映射旧处理状态。".to_string(),
                actor: "system".to_string(),
                ..InboxStatusTransition::default()
            });
        }
        changed = true;
    }
    if item.last_transition_at.is_empty() {
        item.last_transition_at = item.updated_at.clone();
        changed = true;
    }
    changed
}

pub fn confidence_level(score: u8, conflicting_signals: bool) -> &'static str {
    if conflicting_signals || score < 45 {
        "low"
    } else if score < 78 {
        "medium"
    } else {
        "high"
    }
}

pub fn semantic_location(category: &str) -> &'static str {
    match category {
        "需求文档" | "项目资料" => "requirements",
        "会议记录" => "meetings",
        "方案文档" | "方案演示" | "图片资料" => "design",
        "测试资料" => "test",
        "交付文件" => "delivery",
        "数据表格" => "data",
        "产出文件" | "报告" => "reports",
        "开发文件" | "提示词" => "development",
        "参考资料" => "reference",
        _ => "other",
    }
}

pub fn normalize_managed_file_name(original_name: &str, suggested_stem: Option<&str>) -> String {
    let path = Path::new(original_name);
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("");
    let original_stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("file");
    let generic = ["新建文档", "最终版", "最新版", "未命名", "document", "file"]
        .iter()
        .any(|value| original_stem.eq_ignore_ascii_case(value));
    let preferred = if generic {
        suggested_stem
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(original_stem)
    } else {
        original_stem
    };
    let mut stem = preferred
        .chars()
        .map(|ch| {
            if matches!(ch, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') {
                '_'
            } else {
                ch
            }
        })
        .collect::<String>();
    stem = stem.trim().trim_end_matches(['.', ' ']).to_string();
    if stem.is_empty() {
        stem = "file".to_string();
    }
    if is_reserved_windows_name(&stem) {
        stem.push('_');
    }
    stem = stem.chars().take(100).collect();
    if extension.is_empty() {
        stem
    } else {
        format!("{stem}.{extension}")
    }
}

fn is_valid_status(value: &str) -> bool {
    matches!(
        value,
        RECEIVED
            | ANALYZING
            | PENDING_REVIEW
            | READY_TO_ROUTE
            | ROUTING
            | ROUTED
            | PROJECT_CREATED
            | IGNORED
            | FAILED
    )
}

fn is_legal_transition(from: &str, to: &str) -> bool {
    matches!(
        (from, to),
        (RECEIVED, ANALYZING)
            | (RECEIVED, FAILED)
            | (ANALYZING, PENDING_REVIEW)
            | (ANALYZING, READY_TO_ROUTE)
            | (ANALYZING, FAILED)
            | (PENDING_REVIEW, READY_TO_ROUTE)
            | (PENDING_REVIEW, IGNORED)
            | (PENDING_REVIEW, FAILED)
            | (READY_TO_ROUTE, ROUTING)
            | (READY_TO_ROUTE, PENDING_REVIEW)
            | (READY_TO_ROUTE, IGNORED)
            | (READY_TO_ROUTE, FAILED)
            | (ROUTING, ROUTED)
            | (ROUTING, PROJECT_CREATED)
            | (ROUTING, FAILED)
            | (FAILED, ANALYZING)
            | (FAILED, READY_TO_ROUTE)
            | (FAILED, PENDING_REVIEW)
            | (ROUTED, READY_TO_ROUTE)
            | (ROUTED, PENDING_REVIEW)
            | (PROJECT_CREATED, READY_TO_ROUTE)
            | (PROJECT_CREATED, PENDING_REVIEW)
    )
}

fn is_reserved_windows_name(value: &str) -> bool {
    let upper = value.to_ascii_uppercase();
    matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (upper.len() == 4
            && (upper.starts_with("COM") || upper.starts_with("LPT"))
            && upper[3..].chars().all(|ch| ('1'..='9').contains(&ch)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_legal_and_rejects_illegal_transitions() {
        let mut item = MaterialInboxItem::default();
        transition(&mut item, RECEIVED, "received", "system", None).unwrap();
        transition(&mut item, ANALYZING, "analyzing", "system", None).unwrap();
        transition(&mut item, READY_TO_ROUTE, "ready", "system", None).unwrap();
        assert!(transition(&mut item, ROUTED, "skip routing", "system", None).is_err());
        assert_eq!(item.processing_status, READY_TO_ROUTE);
        assert_eq!(
            item.status_history.last().unwrap().error_code,
            "invalidTransition"
        );
    }

    #[test]
    fn confidence_policy_handles_boundaries_and_conflicts() {
        assert_eq!(confidence_level(90, false), "high");
        assert_eq!(confidence_level(60, false), "medium");
        assert_eq!(confidence_level(90, true), "low");
        assert_eq!(confidence_level(20, false), "low");
    }

    #[test]
    fn managed_name_is_windows_safe_and_keeps_extension() {
        assert_eq!(
            normalize_managed_file_name("新建文档.docx", Some("项目需求：一期")),
            "项目需求：一期.docx"
        );
        assert_eq!(normalize_managed_file_name("CON.txt", None), "CON_.txt");
    }

    #[test]
    fn managed_name_supports_chinese_spaces_and_bounds_long_names() {
        assert_eq!(
            normalize_managed_file_name("项目 需求说明.md", None),
            "项目 需求说明.md"
        );
        let long_name = format!("{}.pdf", "很长的中文文件名".repeat(40));
        let normalized = normalize_managed_file_name(&long_name, None);
        assert!(normalized.ends_with(".pdf"));
        assert!(normalized.trim_end_matches(".pdf").chars().count() <= 100);
    }

    #[test]
    fn migrates_legacy_inbox_status_and_preserves_source_evidence() {
        let mut item = MaterialInboxItem {
            file_name: "旧资料.txt".to_string(),
            source_path: r"D:\旧资料\旧资料.txt".to_string(),
            received_from: "appImport".to_string(),
            received_at: "2026-08-01T10:00:00Z".to_string(),
            processing_status: "pending_confirmation".to_string(),
            target_project_root: r"D:\Projects\Alpha".to_string(),
            suggested_category: "需求文档".to_string(),
            ..MaterialInboxItem::default()
        };

        assert!(migrate_legacy_item(&mut item));
        assert_eq!(item.processing_status, READY_TO_ROUTE);
        assert_eq!(item.received_count, 1);
        assert_eq!(item.source_history.len(), 1);
        assert_eq!(item.recommended_relative_location, "requirements");
        assert!(!item.status_history.is_empty());
    }
}
