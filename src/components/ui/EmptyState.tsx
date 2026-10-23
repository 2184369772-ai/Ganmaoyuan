import type { ReactNode } from "react";

type EmptyStateKind = "data" | "search" | "project" | "pending";

const DEFAULT_TEXT: Record<EmptyStateKind, string> = {
  data: "暂无数据",
  search: "没有找到相关结果。",
  project: "暂无项目",
  pending: "暂无待处理事项",
};

export function EmptyState({
  kind = "data",
  title,
  action,
}: {
  kind?: EmptyStateKind;
  title?: string;
  action?: ReactNode;
}) {
  return (
    <div className={`ui-empty ui-empty-${kind}`}>
      <p>{title ?? DEFAULT_TEXT[kind]}</p>
      {action}
    </div>
  );
}
