import { useEffect, useMemo, useState } from "react";
import { useNavigate } from "react-router-dom";
import { useAppState } from "../app/AppState";
import { searchProjects, type GlobalSearchResult } from "../features/project/desktopApi";
import { Badge, Button, Card, EmptyState, Feedback, TextInput } from "./ui";

export function GlobalSearchPanel({
  compact = false,
  autoFocus = false,
}: {
  compact?: boolean;
  autoFocus?: boolean;
}) {
  const navigate = useNavigate();
  const { openProject, openFilePath, openFolderPath, setError } = useAppState();
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<GlobalSearchResult[]>([]);
  const [isSearching, setIsSearching] = useState(false);

  useEffect(() => {
    const trimmed = query.trim();
    if (!trimmed) {
      setResults([]);
      setIsSearching(false);
      return;
    }
    const timer = window.setTimeout(() => {
      setIsSearching(true);
      void searchProjects(trimmed)
        .then((next) => {
          setResults(next);
          setError("");
        })
        .catch((err) => {
          setError(String(err));
        })
        .finally(() => setIsSearching(false));
    }, 220);
    return () => window.clearTimeout(timer);
  }, [query, setError]);

  const visibleResults = useMemo(() => results.slice(0, 12), [results]);

  async function openResult(result: GlobalSearchResult) {
    try {
      if (result.projectRoot) {
        await openProject(result.projectRoot);
        navigate("/work");
        return;
      }
      if (result.managedPath) {
        await openFilePath(result.managedPath);
      }
    } catch (err) {
      setError(String(err));
    }
  }

  return (
    <section className={`global-search-panel ${compact ? "compact" : ""}`}>
      <div className="global-search-head">
        <div>
          <span className="section-label">全局搜索</span>
          {!compact ? <strong>搜项目、资料摘要、对话、任务和分析结果</strong> : null}
        </div>
        {isSearching ? <Badge tone="pending">搜索中...</Badge> : null}
      </div>

      <TextInput
        variant="search"
        className="global-search-field"
        aria-label='全局搜索'
        autoFocus={autoFocus}
        value={query}
        onChange={(event) => setQuery(event.target.value)}
        placeholder="输入中文关键词，例如：全局搜索 / DeepSeek / PRD"
      />

      {query.trim() ? (
        <div className="global-search-results">
          {visibleResults.length ? (
            visibleResults.map((result) => (
              <Card
                as="article"
                key={result.id}
                className="global-search-result"
                onClick={() => void openResult(result)}
                role="button"
                tabIndex={0}
                onKeyDown={(event) => {
                  if (event.key === "Enter" || event.key === " ") {
                    event.preventDefault();
                    void openResult(result);
                  }
                }}
              >
                <div className="global-search-meta">
                  <strong>{result.title || result.projectName}</strong>
                  <Badge tone="neutral">{formatResultType(result.contentType)} / {result.projectName}</Badge>
                </div>
                {isWorkspaceFile(result) ? (
                  <div className="global-search-workspace-meta">
                    <span>位置：{result.workspaceRelativePath || result.managedPath || "Workspace"}</span>
                    <span>分类：{formatOwnership(result.ownershipType)}{result.category ? ` / ${result.category}` : ""}</span>
                    <span>用途：{formatDocumentPurpose(result.documentPurpose) || result.documentType || "待确认"}</span>
                  </div>
                ) : null}
                <p>{result.snippet || "已命中该项目记录。"}</p>
                {result.matchSnippet ? (
                  <div className="global-search-match">
                    命中{result.matchedField ? ` ${result.matchedField}` : ""}：{result.matchSnippet}
                  </div>
                ) : null}
                {result.recentStatus || result.decisionTraceId || result.confidenceDisplay ? (
                  <div className="global-search-workspace-meta subdued">
                    {result.recentStatus ? <span>{result.recentStatus}</span> : null}
                    {result.confidenceDisplay ? <span>{result.confidenceDisplay}</span> : null}
                    {result.decisionTraceId ? <span>Decision Trace：{result.decisionTraceId}</span> : null}
                  </div>
                ) : null}
                <div className="global-search-footer">
                  <span>{formatUpdatedAt(result.updatedAt)}</span>
                  {result.managedPath ? (
                    <div className="file-result-actions">
                      <Button
                        type="button"
                        onClick={(event) => {
                          event.stopPropagation();
                          void openFilePath(result.managedPath).catch(() => undefined);
                        }}
                      >
                        打开文件
                      </Button>
                      <Button
                        type="button"
                        onClick={(event) => {
                          event.stopPropagation();
                          void openFolderPath(result.managedPath).catch(() => undefined);
                        }}
                      >
                        打开所在位置
                      </Button>
                    </div>
                  ) : null}
                </div>
              </Card>
            ))
          ) : (
            <EmptyState kind="search" />
          )}
        </div>
      ) : (
        <Feedback>搜索范围包含项目名称、文件名、摘要、对话、任务、决定、成果、项目理解、Atlas 结果和 Codex 报告。</Feedback>
      )}
    </section>
  );
}

function formatResultType(contentType: string) {
  switch (contentType) {
    case "project":
      return "项目";
    case "file":
      return "文件";
    case "message":
      return "对话";
    case "task":
      return "任务";
    case "decision":
      return "决定";
    case "artifact":
      return "成果";
    case "projectAnalysis":
      return "项目理解";
    case "atlas":
      return "Atlas";
    case "codexReport":
      return "Codex 报告";
    case "global_file":
      return "Workspace 文件";
    default:
      return "记录";
  }
}

function isWorkspaceFile(result: GlobalSearchResult) {
  return result.contentType === "global_file" || Boolean(result.workspaceRelativePath);
}

function formatOwnership(value: string) {
  switch (value) {
    case "existingProject":
    case "existingProjectMaterial":
      return "项目资料";
    case "generalWorkMaterial":
      return "通用资料";
    case "temporaryOrReference":
      return "临时/参考";
    case "newProjectCandidate":
      return "新项目候选";
    case "unsupportedOrFailed":
      return "无法处理";
    default:
      return value || "待确认";
  }
}

function formatDocumentPurpose(value: string) {
  switch (value) {
    case "requirement":
      return "需求";
    case "plan":
      return "计划";
    case "report":
      return "报告";
    case "evidence":
      return "证据";
    case "reference":
      return "参考";
    case "data":
      return "数据";
    case "template":
      return "模板";
    case "communication":
      return "沟通";
    case "unknown":
      return "待确认";
    default:
      return value;
  }
}

function formatUpdatedAt(value: string) {
  if (!value) return "更新时间未知";
  const numeric = Number(value);
  const date = Number.isFinite(numeric) && numeric > 0 ? new Date(numeric) : new Date(value);
  if (Number.isNaN(date.getTime())) {
    return value;
  }
  return `更新于 ${date.toLocaleString("zh-CN", { hour12: false })}`;
}
