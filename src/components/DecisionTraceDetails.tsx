import type { DecisionTrace } from "../features/project/desktopApi";
import { Badge, confidenceTone, EmptyState } from "./ui";

const DECISION_LABELS: Record<string, string> = {
  approved: "已确认",
  rejected: "已拒绝",
  modified: "已修改",
  pending: "待确认",
};

const EXECUTION_LABELS: Record<string, string> = {
  executed: "已执行",
  failed: "执行失败",
  cancelled: "已取消",
  pending: "未执行",
};

export function DecisionTraceDetails({ trace }: { trace?: DecisionTrace | null }) {
  if (!trace?.id) return null;
  return (
    <details className="technical-detail decision-trace-details">
      <summary>查看原因</summary>
      <div className="evidence-group">
        <strong>事实依据</strong>
        {trace.inputEvidence.length ? (
          <ul className="inbox-basis">
            {trace.inputEvidence.map((evidence, index) => (
              <li key={`${trace.id}-${evidence.kind}-${index}`}>
                {evidence.label || evidence.kind}：{evidence.summary || "无摘要"}
              </li>
            ))}
          </ul>
        ) : (
          <EmptyState kind="data" title="没有可引用的事实依据。" />
        )}
        <p><strong>AI 理解：</strong>{trace.aiUnderstanding || "未形成明确理解"}</p>
        <p><strong>建议：</strong>{trace.recommendation || "待人工判断"}</p>
        <p>
          <strong>置信度：</strong>
          <Badge tone={confidenceTone(trace.confidence.level)}>
            {trace.confidence.display || formatTraceConfidence(trace.confidence.score)}
          </Badge>
        </p>
        <p>
          <strong>最终：</strong>
          <Badge tone={trace.userDecision === "approved" ? "success" : trace.userDecision === "rejected" ? "error" : "pending"}>
            {DECISION_LABELS[trace.userDecision] || trace.userDecision || "待确认"}
          </Badge>{" "}
          <Badge tone={trace.execution === "failed" ? "error" : trace.execution === "executed" ? "success" : "pending"}>
            {EXECUTION_LABELS[trace.execution] || trace.execution || "未执行"}
          </Badge>
        </p>
      </div>
    </details>
  );
}

export function formatTraceConfidence(score: number) {
  const normalized = Math.max(0, Math.min(100, Math.round(score || 0)));
  const label = normalized >= 78 ? "高" : normalized >= 45 ? "中" : "低";
  return `${label} ${normalized}%`;
}
