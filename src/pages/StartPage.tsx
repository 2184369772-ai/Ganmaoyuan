import { useEffect, useState, type ReactNode } from "react";
import { useNavigate, useSearchParams } from "react-router-dom";
import { useAppState } from "../app/AppState";
import { AppLayout } from "../components/AppLayout";
import { GlobalSearchPanel } from "../components/GlobalSearchPanel";
import { DecisionTraceDetails } from "../components/DecisionTraceDetails";
import { Feedback } from "../components/ui";
import {
  chooseImportFiles,
  chooseProjectRoot,
  type MaterialInboxItem,
  type WorkspaceActivitySummary,
} from "../features/project/desktopApi";

type StartMode = "idle" | "continue" | "new" | "inbox";

const semanticLocationOptions = [
  { value: "requirements", label: "需求资料" },
  { value: "meetings", label: "会议资料" },
  { value: "design", label: "设计资料" },
  { value: "test", label: "测试资料" },
  { value: "reports", label: "报告资料" },
  { value: "data", label: "数据资料" },
  { value: "reference", label: "参考资料" },
  { value: "delivery", label: "交付资料" },
  { value: "development", label: "开发资料" },
  { value: "other", label: "其他待检查" },
];

const documentPurposeLabels: Record<string, string> = {
  requirement: "需求",
  plan: "计划",
  report: "报告",
  evidence: "证据",
  reference: "参考资料",
  data: "数据",
  template: "模板",
  communication: "沟通记录",
  unknown: "待确认",
};

export function StartPage() {
  const navigate = useNavigate();
  const [searchParams] = useSearchParams();
  const {
    isDesktopReady,
    isStartupHydrated,
    projects,
    activeProject,
    openProject,
    createProject,
    memos,
    addMemo,
    updateMemo,
    deleteMemo,
    materialInbox,
    receiveInboxFiles,
    confirmInboxEntry,
    updateInboxRouteDecision,
    ignoreInboxEntry,
    retryInboxEntry,
    reanalyzeInboxEntry,
    undoInboxRoute,
    routeInboxItemToGlobal,
    openFilePath,
    openFolderPath,
    createProjectFromInbox,
    managedFiles,
    pendingReviews,
    tasks,
    dailySessions,
    locationDecisions,
    todayWorkspace,
    refreshTodayWorkspace,
    error,
    setError,
  } = useAppState();
  const [sidebarOpen, setSidebarOpen] = useState(false);
  const [mode, setMode] = useState<StartMode>("idle");
  const [selectedRoot, setSelectedRoot] = useState("");
  const [projectName, setProjectName] = useState("");
  const [projectRoot, setProjectRoot] = useState("");
  const [filePaths, setFilePaths] = useState<string[]>([]);
  const [description, setDescription] = useState("");
  const [memoDraft, setMemoDraft] = useState("");
  const [isBusy, setIsBusy] = useState(false);
  const [inboxCreateItemId, setInboxCreateItemId] = useState("");
  const [inboxProjectName, setInboxProjectName] = useState("");
  const [inboxProjectRoot, setInboxProjectRoot] = useState("");
  const [inboxDescription, setInboxDescription] = useState("");

  const inboxPendingReviews = materialInbox.filter((item) => item.processingStatus === "pendingReview" || item.processingStatus === "failed");
  const currentHistoryRoot = activeProject?.rootDir || selectedRoot || projects[0]?.rootDir || "";

  useEffect(() => {
    if (searchParams.get("mode") === "inbox") {
      setMode("inbox");
    }
  }, [searchParams]);

  async function openProjectAndEnter(projectRoot: string) {
    if (!projectRoot || isBusy) return;
    setIsBusy(true);
    try {
      await openProject(projectRoot);
      setSidebarOpen(false);
      navigate("/work");
    } catch (err) {
      setError(String(err));
    } finally {
      setIsBusy(false);
    }
  }

  async function runHomepagePrimaryAction() {
    setMode(projects.length ? "continue" : "new");
  }

  function homepagePrimaryLabel() {
    return projects.length ? "开始工作" : "新建项目";
  }

  if (isDesktopReady && !isStartupHydrated) {
    return (
      <AppLayout className="start-shell project-start-shell">
        <section className="page-shell page-shell-home">
          <div className="start-center project-start-center">
            <h1>感冒院</h1>
            <Feedback tone="warning">正在恢复项目状态…</Feedback>
          </div>
        </section>
      </AppLayout>
    );
  }

  async function continueProject() {
    if (!selectedRoot) return;
    await openProjectAndEnter(selectedRoot);
  }

  async function createNewProject() {
    if (!projectName.trim() || !projectRoot) return;
    setIsBusy(true);
    try {
      await createProject(projectName.trim(), projectRoot, filePaths, description);
      navigate("/work");
    } catch (err) {
      setError(String(err));
    } finally {
      setIsBusy(false);
    }
  }

  async function importToInbox(paths: string[]) {
    if (!paths.length) return;
    setIsBusy(true);
    try {
      await receiveInboxFiles(paths);
      setMode("inbox");
    } catch (err) {
      setError(String(err));
    } finally {
      setIsBusy(false);
    }
  }

  async function confirmInbox(item: MaterialInboxItem) {
    if (!item.targetProjectRoot) return;
    setIsBusy(true);
    try {
      await confirmInboxEntry(item.id, item.targetProjectRoot);
    } catch (err) {
      setError(String(err));
    } finally {
      setIsBusy(false);
    }
  }

  async function runInboxAction(action: () => Promise<void>) {
    if (isBusy) return;
    setIsBusy(true);
    try {
      await action();
    } catch (err) {
      setError(String(err));
    } finally {
      setIsBusy(false);
    }
  }

  async function createInboxProject() {
    if (!inboxCreateItemId || !inboxProjectName.trim() || !inboxProjectRoot) return;
    setIsBusy(true);
    try {
      await createProjectFromInbox(
        inboxCreateItemId,
        inboxProjectName.trim(),
        inboxProjectRoot,
        inboxDescription,
      );
      setInboxCreateItemId("");
      setInboxProjectName("");
      setInboxProjectRoot("");
      setInboxDescription("");
      navigate("/work");
    } catch (err) {
      setError(String(err));
    } finally {
      setIsBusy(false);
    }
  }

  function beginCreateInboxProject(item: MaterialInboxItem) {
    setInboxCreateItemId(item.id);
    setInboxProjectName(item.proposedProjectName || item.fileName);
    setInboxProjectRoot("");
    setInboxDescription("");
  }

  return (
    <AppLayout
      className="start-shell project-start-shell"
      sidebarOpen={sidebarOpen}
      sidebar={(
        <>
          <button
            type="button"
            className="sidebar-peek"
            onClick={() => setSidebarOpen((value) => !value)}
            aria-label="展开侧边栏"
          >
            {sidebarOpen ? "←" : "→"}
          </button>

          <aside className={`start-sidebar ${sidebarOpen ? "open" : ""}`}>
        <Section title="全局搜索">
          <GlobalSearchPanel compact />
        </Section>

        <Section title="最近项目">
          {projects.length ? (
            projects.slice(0, 5).map((project) => (
              <button
                key={project.id}
                type="button"
                className="sidebar-item"
                onClick={() => void openProjectAndEnter(project.rootDir)}
              >
                {project.name}
              </button>
            ))
          ) : (
            <p>暂无历史项目</p>
          )}
        </Section>

        <Section title="历史工作台">
          {dailySessions.length ? (
            dailySessions
              .slice(-4)
              .reverse()
              .map((session) => (
                <button
                  key={session.id}
                  type="button"
                  className="sidebar-item"
                  disabled={!currentHistoryRoot}
                  onClick={() => void openProjectAndEnter(currentHistoryRoot)}
                >
                  {session.dateKey} · {session.messageIds.length} 条记录
                </button>
              ))
          ) : (
            projects.slice(0, 3).map((project) => (
              <button
                key={project.id}
                type="button"
                className="sidebar-item"
                onClick={() => void openProjectAndEnter(project.rootDir)}
              >
                {project.name}
              </button>
            ))
          )}
        </Section>

        <Section title="待检查事项">
          {inboxPendingReviews.length ? (
            inboxPendingReviews.slice(0, 4).map((item) => <p key={item.id}>{item.fileName}</p>)
          ) : pendingReviews.length ? (
            pendingReviews.slice(0, 4).map((item) => <p key={item.id}>{item.title}</p>)
          ) : (
            <p>暂无待检查资料</p>
          )}
        </Section>

        <Section title="项目文件位置">
          {locationDecisions.length ? (
            locationDecisions.slice(-4).reverse().map((item) => (
              <p key={item.id}>
                {item.fileName} · {item.managedRelativePath}
              </p>
            ))
          ) : (
            <p>{managedFiles.length} 个文件已登记</p>
          )}
        </Section>

        <Section title="当前任务">
          {tasks.length ? (
            tasks.slice(-3).reverse().map((task) => (
              <p key={task.id}>
                {task.title} · {task.status}
              </p>
            ))
          ) : (
            <p>暂无当前任务</p>
          )}
        </Section>

        <Section title="备忘录">
          <div className="memo-editor">
            <textarea value={memoDraft} onChange={(event) => setMemoDraft(event.target.value)} placeholder="保存网址或临时文字" />
            <button
              type="button"
              className="btn"
              onClick={() => {
                if (!memoDraft.trim()) return;
                void addMemo(memoDraft.trim());
                setMemoDraft("");
              }}
            >
              保存
            </button>
          </div>
          {memos.map((memo) => (
            <div key={memo.id} className="memo-item">
              <textarea value={memo.text} onChange={(event) => void updateMemo(memo.id, event.target.value)} />
              <button type="button" className="btn" onClick={() => void deleteMemo(memo.id)}>
                删除
              </button>
            </div>
          ))}
        </Section>
          </aside>
        </>
      )}
      header={(
        <header className="start-topbar">
          <button type="button" className="icon-btn" onClick={() => navigate("/settings")} aria-label="设置">
            ⚙
          </button>
        </header>
      )}
    >

      <section className="page-shell page-shell-home">
        <div className={`start-center project-start-center ${mode === "inbox" ? "project-start-center-inbox" : ""}`}>
        <h1>感冒院</h1>
        {!isDesktopReady ? (
          <Feedback tone="warning">请从 Tauri 桌面应用启动，浏览器模式无法掌控 Windows 文件位置。</Feedback>
        ) : null}
        {error ? <Feedback tone="error">{error}</Feedback> : null}

        {mode === "idle" ? (
          <div className="today-entry">
            <div className="today-entry-head">
              <div>
                <span className="section-label">今天应该做什么</span>
                <h2>{todayWorkspace?.status === "ready" ? "从真实进度继续" : "先建立项目记录"}</h2>
              </div>
              <button
                type="button"
                className="btn"
                onClick={() => void refreshTodayWorkspace({ refreshFacts: true })}
              >
                刷新
              </button>
            </div>
            <div className="start-idle-actions start-idle-actions-hero">
              <div className="start-idle-primary">
                <button
                  type="button"
                  className="btn btn-primary btn-large start-primary"
                  disabled={isBusy}
                  onClick={() => void runHomepagePrimaryAction()}
                >
                  {homepagePrimaryLabel()}
                </button>
              </div>
              <div className="start-idle-secondary">
                <button
                  type="button"
                  className="btn btn-large"
                  onClick={() => void chooseImportFiles().then((paths) => void importToInbox(paths))}
                >
                  导入资料
                </button>
              </div>
            </div>
            <div className="today-layout">
              <section className="today-primary-column">
                <div className="today-summary-card">
                  <span className="section-label">上次做到哪里</span>
                  {todayWorkspace?.lastProgress.length ? (
                    <p className="today-last-progress">上次做到：{todayWorkspace.lastProgress.filter(Boolean).join("；")}</p>
                  ) : (
                    <p className="inline-placeholder">暂无恢复点，不生成虚假进度或建议。</p>
                  )}
                </div>

                <div className="today-section-block">
                  <div className="today-section-head">
                    <div>
                      <span className="section-label">最近项目</span>
                      <strong>{projects.length ? "最近进入的项目" : "还没有已注册项目"}</strong>
                    </div>
                    {projects.length > 5 ? <button type="button" className="btn" onClick={() => setMode("continue")}>查看全部项目</button> : null}
                  </div>
                  {projects.length ? (
                    <div className="today-focus-list">
                      {projects.slice(0, 5).map((project) => (
                        <button
                          key={project.id}
                          type="button"
                          className="today-focus-item"
                          onClick={() => void openProjectAndEnter(project.rootDir)}
                        >
                          <div className="today-focus-topline">
                            <strong>{project.name}</strong>
                            <span>{project.lastOpenedAt ? new Date(Number(project.lastOpenedAt)).toLocaleString("zh-CN") : "尚未进入"}</span>
                          </div>
                          <p>{project.nextStep || "打开项目查看当前 Focus"}</p>
                        </button>
                      ))}
                    </div>
                  ) : (
                    <div className="ui-empty">
                      <p>还没有可恢复的项目记录。</p>
                    </div>
                  )}
                </div>

                <details className="today-more-details">
                  <summary>
                    <span>更多动态</span>
                    <strong>建议、待关注与文件变化</strong>
                  </summary>
                  <div className="today-more-content">
                <div className="today-section-block today-workspace-activity">
                  <div className="today-section-head">
                    <div>
                      <span className="section-label">文件空间变化</span>
                      <strong>{formatWorkspaceActivityTitle(todayWorkspace?.workspaceActivity)}</strong>
                    </div>
                  </div>
                  {todayWorkspace?.workspaceActivity?.recentItems?.length ? (
                    <div className="today-tertiary-list">
                      {todayWorkspace.workspaceActivity.recentItems.slice(0, 4).map((item) => (
                        <button
                          key={`${item.kind}-${item.title}-${item.occurredAt}`}
                          type="button"
                          className="today-tertiary-item"
                          disabled={!item.managedPath}
                          onClick={() => item.managedPath && void openFolderPath(item.managedPath)}
                        >
                          <span>{formatWorkspaceActivityKind(item.kind)} · {item.category || "Workspace"}</span>
                          <strong>{item.title}</strong>
                          {item.summary ? <small>{item.summary}</small> : null}
                        </button>
                      ))}
                    </div>
                  ) : (
                    <div className="ui-empty">
                      <p>暂无新的 Workspace 整理记录。</p>
                    </div>
                  )}
                </div>

              <section className="today-secondary-column">
                <div className="today-section-block">
                  <div className="today-section-head">
                    <div>
                      <span className="section-label">今天建议</span>
                      <strong>优先处理 1 到 3 件真实事项</strong>
                    </div>
                  </div>
                  {todayWorkspace?.recommendedActions.length ? (
                    <div className="today-actions-list">
                      {todayWorkspace.recommendedActions.map((item, index) => (
                        <article key={`${item.relatedProject}-${index}`} className="today-action-item">
                          <div className="today-action-topline">
                            <div className="today-action-main">
                              <strong>{item.action}</strong>
                              <small>{item.relatedProject} · 来源：{item.source || "现有记录"}</small>
                            </div>
                            <span className="today-action-state">
                              {item.itemType === "fact" ? "事实" : item.itemType === "suggestion" ? "建议" : "待确认"}
                              {item.confidence?.display ? ` · ${item.confidence.display}` : ""}
                            </span>
                          </div>
                          <p>{item.reason}</p>
                          <div className="today-action-footer">
                            <small>{item.category || "工作项"}</small>
                            {item.evidence.length ? <p className="inline-placeholder">依据：{item.evidence.join("；")}</p> : null}
                          </div>
                          {item.decisionTrace ? (
                            <details className="today-action-details">
                              <summary>查看原因</summary>
                              <DecisionTraceDetails trace={item.decisionTrace} />
                            </details>
                          ) : null}
                        </article>
                      ))}
                    </div>
                  ) : (
                    <div className="ui-empty">
                      <p>当前资料不足，暂不生成额外建议。</p>
                    </div>
                  )}
                </div>

                <div className="today-section-block today-tertiary-block">
                  <div className="today-section-head">
                    <div>
                      <span className="section-label">待关注</span>
                      <strong>阻塞与待确认</strong>
                    </div>
                  </div>
                  <div className="today-tertiary-list">
                    <article className="today-tertiary-item today-tertiary-reminder">
                      <span>阻塞提醒</span>
                      <strong>{todayWorkspace?.blockers.length ? `有 ${todayWorkspace.blockers.length} 项需要处理` : "当前没有阻塞"}</strong>
                    </article>
                    <article className="today-tertiary-item today-tertiary-reminder">
                      <span>待确认</span>
                      <strong>{todayWorkspace?.pendingItems.length ? `还有 ${todayWorkspace.pendingItems.length} 项待处理` : "当前没有待确认项"}</strong>
                    </article>
                  </div>
                </div>
              </section>
                  </div>
                </details>
              </section>
            </div>
          </div>
        ) : null}

        {mode === "continue" ? (
          <div className="start-flow-panel">
            <h2>继续历史任务</h2>
            <select value={selectedRoot} onChange={(event) => setSelectedRoot(event.target.value)}>
              <option value="">选择已有项目</option>
              {projects.map((project) => (
                <option key={project.id} value={project.rootDir}>
                  {project.name}
                </option>
              ))}
            </select>
            <div className="start-flow-actions">
              <button type="button" className="btn btn-ghost" onClick={() => setMode("new")}>
                新建项目
              </button>
              <button type="button" className="btn" onClick={() => setMode("inbox")}>
                查看 Inbox
              </button>
              <button type="button" className="btn btn-primary" disabled={!selectedRoot || isBusy} onClick={continueProject}>
                继续
              </button>
            </div>
          </div>
        ) : null}

        {mode === "new" ? (
          <div className="start-flow-panel start-new-project-panel">
            <h2>新建项目</h2>
            <input value={projectName} onChange={(event) => setProjectName(event.target.value)} placeholder="项目名称" />
            <button type="button" className="btn" onClick={() => void chooseProjectRoot().then((path) => path && setProjectRoot(path))}>
              {projectRoot || "选择项目根目录"}
            </button>
            <button type="button" className="btn" onClick={() => void chooseImportFiles().then(setFilePaths)}>
              {filePaths.length ? `已选择 ${filePaths.length} 个资料文件` : "导入已有资料"}
            </button>
            <textarea
              value={description}
              onChange={(event) => setDescription(event.target.value)}
              placeholder="说明这个项目是做什么的，本轮语音入口先预留。"
            />
            <div className="start-flow-actions">
              <button type="button" className="btn" onClick={() => setMode("idle")}>
                取消
              </button>
              <button
                type="button"
                className="btn btn-primary"
                disabled={!projectName.trim() || !projectRoot || isBusy}
                onClick={createNewProject}
              >
                创建并进入工作台
              </button>
            </div>
          </div>
        ) : null}

        {mode === "inbox" ? (
          <div
            className="start-flow-panel inbox-panel"
            onDragOver={(event) => event.preventDefault()}
            onDrop={(event) => {
              event.preventDefault();
              const dropped = Array.from(event.dataTransfer.files)
                .map((file) => (file as File & { path?: string }).path ?? "")
                .filter(Boolean);
              void importToInbox(dropped);
            }}
          >
            <h2>Inbox</h2>
            <p>先导入，再确认归位。判断不明确的会留在待检查，不会自动移动、覆盖或删除原文件。</p>
            <div className="start-flow-actions">
              <button type="button" className="btn" onClick={() => setMode("idle")}>
                返回
              </button>
              <button type="button" className="btn" onClick={() => void chooseImportFiles().then((paths) => void importToInbox(paths))}>
                继续导入到 Inbox
              </button>
            </div>
            <div className="inbox-dropzone">拖入资料到这里，或使用“继续导入到 Inbox”批量选择文件。</div>
            <div className="inbox-list">
              {materialInbox.length ? (
                materialInbox.map((item) => (
                  <article key={item.id} className="inbox-item">
                    <div className="inbox-item-head">
                      <div className="inbox-item-title-group">
                        <strong>{item.fileName}</strong>
                        <p className="inbox-item-summary">{item.contentSummary || "暂无摘要"}</p>
                      </div>
                      <span>{formatInboxStatus(item)}</span>
                    </div>
                    <div className="inbox-hero-row">
                      <div className="inbox-hero-main">
                        <span className="section-label">系统结论</span>
                        <strong>
                          {item.documentType ||
                            item.businessPurpose ||
                            item.businessSummary ||
                            formatCategoryLabel(item.recommendedCategory) ||
                            "待检查"}
                        </strong>
                        <p>{formatInboxNextStep(item)}</p>
                      </div>
                      <div className="inbox-hero-side">
                        <span>{item.ownershipType ? ownershipLabel(item.ownershipType) : "待确认"}</span>
                        <strong>{formatConfidence(item.confidenceLevel, item.confidenceScore)}</strong>
                      </div>
                    </div>
                    <div className="inbox-meta">
                      <span>归属：{item.ownershipType ? ownershipLabel(item.ownershipType) : "待确认"}</span>
                      <span>位置：{formatSemanticLocation(item.recommendedRelativeLocation || item.recommendedLocation)}</span>
                    </div>
                    <div className="inbox-next-step">
                      <span className="section-label">下一步</span>
                      <strong>{formatInboxNextStep(item)}</strong>
                    </div>
                    {item.errorMessage ? (
                      <p className="inbox-error">
                        错误：{item.errorMessage}
                        {item.technicalDetail ? (
                          <details className="technical-detail">
                            <summary>技术详情</summary>
                            <code>{item.technicalDetail}</code>
                          </details>
                        ) : null}
                      </p>
                    ) : null}
                    <details className="inbox-details">
                      <summary>查看原因</summary>
                      <div className="inbox-details-body">
                        <DecisionTraceDetails trace={primaryDecisionTrace(item)} />
                      </div>
                    </details>
                    <details className="inbox-details">
                      <summary>查看判断依据</summary>
                      <div className="inbox-details-body">
                        <p>收到时间：{formatReceivedTime(item.receivedAt || item.createdAt)}</p>
                        <p>分析结果：{formatInboxAnalysis(item)}</p>
                        <p>用途：{documentPurposeLabels[item.documentPurpose] || item.documentPurpose || "待确认"}</p>
                        <p>用途置信度：{item.documentPurposeConfidence || 0}%</p>
                        {item.businessDomain ? <p>业务：{item.businessDomain}</p> : null}
                        {item.ownershipType !== "generalWorkMaterial" ? (
                          <p>推荐项目：{item.recommendedProjectName || item.targetProjectName || "待确认"}</p>
                        ) : null}
                        <p>当前状态：{formatLifecycleStatus(item.processingStatus)}</p>
                        <p>来源：{item.receivedFrom || "appImport"}</p>
                        {item.suggestedFileName && item.suggestedFileName !== item.fileName ? (
                          <p>managed 命名建议：{item.suggestedFileName}</p>
                        ) : null}
                        {item.locationReason ? <p>位置原因：{item.locationReason}</p> : null}
                        {item.projectCandidateScore >= 12 && item.projectCandidateReasons.length ? (
                          <p>项目候选原因：{item.projectCandidateReasons.join("；")}</p>
                        ) : null}
                        {item.statusHistory.length ? (
                          <p>最近变化：{item.statusHistory[item.statusHistory.length - 1]?.reason}</p>
                        ) : null}
                        {item.versionNumber > 1 ? <p>检测到新版本：v{item.versionNumber}</p> : null}
                        {item.receivedCount > 1 ? <p>已收到 {item.receivedCount} 次</p> : null}
                        {item.projectCandidates.filter((candidate) => candidate.score > 0).length ? (
                          <div className="inbox-candidates">
                            <span>候选项目</span>
                            {item.projectCandidates
                              .filter((candidate) => candidate.score > 0)
                              .sort((left, right) => right.score - left.score)
                              .map((candidate) => (
                                <span key={`${item.id}-${candidate.candidateProjectId}`}>
                                  {candidate.candidateProjectName} · {formatConfidence(candidate.confidence, candidate.score)}
                                </span>
                              ))}
                          </div>
                        ) : item.ownershipType === "existingProject" ? (
                          <p className="inline-placeholder">未找到可信的已有项目关联。</p>
                        ) : null}
                        {item.decisionBasis.length ? (
                          <ul className="inbox-basis">
                            {item.decisionBasis.map((basis, index) => (
                              <li key={`${item.id}-${index}`}>{basis}</li>
                            ))}
                          </ul>
                        ) : null}
                        {item.resultNote ? <p>{item.resultNote}</p> : null}
                      </div>
                    </details>
                    {item.processingStatus === "pendingReview" || item.processingStatus === "readyToRoute" || item.processingStatus === "failed" ? (
                      item.ownershipType === "unsupportedOrFailed" ? (
                        <p className="inline-placeholder">
                          该文件无法可靠解析，不会归入任何项目；可重试或留待人工检查。
                        </p>
                      ) : item.ownershipType === "existingProject" ? (
                        <div className="inbox-route-editor">
                          <div className="inbox-route-editor-head">
                            <span className="section-label">确认归档</span>
                            <strong>确认项目和位置后即可归位</strong>
                          </div>
                          <label>
                            <span>项目</span>
                            <select
                              value={item.targetProjectRoot}
                              onChange={(event) => {
                                const root = event.target.value;
                                if (!root) return;
                                void runInboxAction(() =>
                                  updateInboxRouteDecision(
                                    item.id,
                                    root,
                                    item.recommendedRelativeLocation || "other",
                                  ),
                                );
                              }}
                            >
                              <option value="">选择项目</option>
                              {projects.map((project) => (
                                <option key={project.id} value={project.rootDir}>{project.name}</option>
                              ))}
                            </select>
                          </label>
                          <label>
                            <span>位置</span>
                            <select
                              value={item.recommendedRelativeLocation || "other"}
                              disabled={!item.targetProjectRoot}
                              onChange={(event) =>
                                void runInboxAction(() =>
                                  updateInboxRouteDecision(item.id, item.targetProjectRoot, event.target.value),
                                )
                              }
                            >
                              {semanticLocationOptions.map((option) => (
                                <option key={option.value} value={option.value}>{option.label}</option>
                              ))}
                            </select>
                          </label>
                        </div>
                      ) : null
                    ) : null}
                    <div className="start-flow-actions">
                      {item.targetProjectRoot &&
                      (item.processingStatus === "pendingReview" || item.processingStatus === "readyToRoute") &&
                      item.ownershipType !== "unsupportedOrFailed" ? (
                        <button type="button" className="btn btn-primary" disabled={isBusy} onClick={() => void confirmInbox(item)}>
                          确认归位
                        </button>
                      ) : null}
                      {(item.ownershipType === "generalWorkMaterial" ||
                        item.ownershipType === "temporaryOrReference" ||
                        item.ownershipType === "needsReview") &&
                      item.processingStatus !== "routed" &&
                      item.processingStatus !== "projectCreated" &&
                      item.processingStatus !== "ignored" ? (
                        <>
                          <button
                            type="button"
                            className="btn"
                            disabled={isBusy}
                            onClick={() => void runInboxAction(() => routeInboxItemToGlobal(item.id, "general"))}
                          >
                            {item.ownershipType === "generalWorkMaterial"
                              ? `归入${formatSemanticLocation(item.recommendedRelativeLocation)}`
                              : "归入通用资料区"}
                          </button>
                          <button
                            type="button"
                            className="btn"
                            disabled={isBusy}
                            onClick={() => void runInboxAction(() => routeInboxItemToGlobal(item.id, "temporary"))}
                          >
                            归入临时资料区
                          </button>
                        </>
                      ) : null}
                      {item.processingStatus === "failed" ? (
                        <button type="button" className="btn" disabled={isBusy} onClick={() => void runInboxAction(() => retryInboxEntry(item.id))}>
                          重试
                        </button>
                      ) : null}
                      {item.processingStatus !== "analyzing" ? (
                        <button
                          type="button"
                          className="btn"
                          disabled={isBusy}
                          onClick={() => void runInboxAction(() => reanalyzeInboxEntry(item.id))}
                        >
                          重新分析
                        </button>
                      ) : null}
                      {item.processingStatus === "pendingReview" || item.processingStatus === "readyToRoute" ? (
                        <button type="button" className="btn" disabled={isBusy} onClick={() => void runInboxAction(() => ignoreInboxEntry(item.id))}>
                          暂不处理
                        </button>
                      ) : null}
                      {item.canUndo && (item.processingStatus === "routed" || item.processingStatus === "projectCreated") ? (
                        <button type="button" className="btn" disabled={isBusy} onClick={() => void runInboxAction(() => undoInboxRoute(item.id))}>
                          撤销
                        </button>
                      ) : null}
                      {item.suggestedManagedPath && (item.processingStatus === "routed" || item.processingStatus === "projectCreated") ? (
                        <>
                          <button type="button" className="btn" onClick={() => void openFilePath(item.suggestedManagedPath)}>查看</button>
                          <button type="button" className="btn" onClick={() => void openFolderPath(item.suggestedManagedPath)}>所在位置</button>
                        </>
                      ) : null}
                      {item.processingStatus !== "routed" &&
                      item.processingStatus !== "projectCreated" &&
                      item.processingStatus !== "ignored" &&
                      item.ownershipType === "newProjectCandidate" &&
                      item.projectCandidateScore >= 75 ? (
                        <button type="button" className="btn" onClick={() => beginCreateInboxProject(item)}>
                          从收件箱新建项目
                        </button>
                      ) : null}
                    </div>
                    {inboxCreateItemId === item.id ? (
                      <div className="inbox-create-form">
                        <input
                          value={inboxProjectName}
                          onChange={(event) => setInboxProjectName(event.target.value)}
                          placeholder="新项目名称"
                        />
                        <button
                          type="button"
                          className="btn"
                          onClick={() => void chooseProjectRoot().then((path) => path && setInboxProjectRoot(path))}
                        >
                          {inboxProjectRoot || "选择项目根目录"}
                        </button>
                        <textarea
                          value={inboxDescription}
                          onChange={(event) => setInboxDescription(event.target.value)}
                          placeholder="补充这个新项目的说明"
                        />
                        <div className="start-flow-actions">
                          <button type="button" className="btn" onClick={() => setInboxCreateItemId("")}>
                            取消
                          </button>
                          <button
                            type="button"
                            className="btn btn-primary"
                            disabled={!inboxProjectName.trim() || !inboxProjectRoot || isBusy}
                            onClick={() => void createInboxProject()}
                          >
                            创建并归位
                          </button>
                        </div>
                      </div>
                    ) : null}
                  </article>
                ))
              ) : (
                <p>暂时还没有收到文件。</p>
              )}
            </div>
          </div>
        ) : null}
        </div>
      </section>
    </AppLayout>
  );
}

function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="sidebar-section">
      <h3>{title}</h3>
      {children}
    </section>
  );
}

function formatInboxStatus(item: MaterialInboxItem) {
  const { judgementStatus, processingStatus } = item;
  if (processingStatus === "routed") return "已归位";
  if (processingStatus === "projectCreated") return "已新建项目";
  if (judgementStatus === "duplicate") return "重复文件";
  if (judgementStatus === "new_version") return "新版本";
  if (judgementStatus === "matched_project") return "可识别项目";
  if (judgementStatus === "project_candidate_high" || item.projectCandidateScore >= 75) return "建议创建新项目";
  if (judgementStatus === "project_candidate_medium" || item.projectCandidateScore >= 45) return "可能形成独立项目";
  if (judgementStatus === "project_candidate_low" || item.projectCandidateScore >= 12) return "疑似项目资料";
  if (judgementStatus === "general_material") return "通用工作资料";
  if (judgementStatus === "temporary_reference") return "临时/参考";
  if (judgementStatus === "unsupported") return "不支持/解析失败";
  return "待检查";
}

function primaryDecisionTrace(item: MaterialInboxItem) {
  const preferred = ["autoRoute", "locationRecommendation", "projectMatch", "ownershipDecision", "fileClassification"];
  for (const type of preferred) {
    const trace = [...(item.decisionTraces ?? [])].reverse().find((candidate) => candidate.type === type);
    if (trace) return trace;
  }
  return undefined;
}

function formatInboxNextStep(item: MaterialInboxItem) {
  if (item.processingStatus === "routed") {
    return "已按当前建议归位，可直接打开文件或所在位置。";
  }
  if (item.processingStatus === "projectCreated") {
    return "已基于当前资料建立项目，后续可继续补充。";
  }
  if (item.processingStatus === "failed" || item.ownershipType === "unsupportedOrFailed") {
    return "本次解析不可靠，建议重试或留待人工检查。";
  }
  if (item.processingStatus === "analyzing" || item.processingStatus === "routing") {
    return "系统正在处理，完成后会更新推荐归位结果。";
  }
  if (item.ownershipType === "existingProject") {
    return `确认项目与位置后即可归位到 ${item.recommendedProjectName || item.targetProjectName || "对应项目"}。`;
  }
  if (item.ownershipType === "generalWorkMaterial") {
    return `可直接归入 ${formatSemanticLocation(item.recommendedRelativeLocation || item.recommendedLocation)}。`;
  }
  if (item.ownershipType === "temporaryOrReference") {
    return "建议先放入临时资料区，后续再决定是否长期归档。";
  }
  if (item.ownershipType === "newProjectCandidate") {
    return item.projectCandidateScore >= 75 ? "证据较强，建议从当前资料建立新项目。" : "当前证据还不够强，先继续补充资料更稳妥。";
  }
  return "当前判断还不够稳定，建议人工检查后再决定归位。";
}

function formatLifecycleStatus(status: string) {
  const labels: Record<string, string> = {
    received: "已接收",
    analyzing: "分析中",
    pendingReview: "待检查",
    readyToRoute: "可归位",
    routing: "归位中",
    routed: "已归位",
    projectCreated: "已新建项目",
    ignored: "已忽略",
    failed: "处理失败",
  };
  return labels[status] || status || "待检查";
}

function formatConfidence(_level: string, score: number) {
  if (!score || score <= 0) {
    return "未确认";
  }
  const label = score >= 78 ? "高" : score >= 45 ? "中" : "低";
  return `${label} ${score}%`;
}

function formatSemanticLocation(location: string) {
  if (!location) return "待确认项目后生成";
  const generalLabels: Record<string, string> = {
    "general/manufacturing/productionData": "生产数据",
    "general/manufacturing/productionReport": "生产报表",
    "general/manufacturing/orderData": "订单数据",
    "general/manufacturing/qualityData": "质量数据",
    "general/engineering/vave": "改善申请",
    "general/engineering/improvement": "改善资料",
    "general/engineering/designReference": "设计参考",
    "general/engineering/technicalDocument": "技术文档",
    "general/management/workPlan": "工作计划",
    "general/management/masterData": "主数据/模板",
    "general/management/meeting": "会议资料",
    "general/management/notice": "通知公告",
    "general/management/report": "管理报告",
    "general/business/quotation": "报价资料",
    "general/business/supplier": "供应商资料",
    "general/business/customer": "客户资料",
    "general/reference/template": "模板",
    "general/reference/learning": "学习资料",
    "general/reference/other": "其他参考资料",
  };
  if (generalLabels[location]) return generalLabels[location];
  return semanticLocationOptions.find((option) => option.value === location)?.label || location;
}

function formatCategoryLabel(category: string) {
  if (!category) return "";
  return semanticLocationOptions.find((option) => option.value === category)?.label || category;
}

function formatInboxAnalysis(item: MaterialInboxItem) {
  const status = item.parseStatus === "success" ? "解析完成" : formatLifecycleStatus(item.processingStatus);
  const judgement = item.documentType || item.businessPurpose || formatCategoryLabel(item.recommendedCategory);
  return judgement ? `${status} / ${judgement}` : status;
}

function formatReceivedTime(timestamp: string) {
  const numeric = Number(timestamp);
  if (!timestamp || !Number.isFinite(numeric) || numeric <= 0) {
    return "未知";
  }
  const date = new Date(numeric);
  if (Number.isNaN(date.getTime())) {
    return "未知";
  }
  const pad = (value: number) => String(value).padStart(2, "0");
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${pad(date.getHours())}:${pad(date.getMinutes())}`;
}

function formatWorkspaceActivityTitle(activity?: WorkspaceActivitySummary) {
  if (!activity || activity.newManagedFiles === 0) {
    return "暂无新的整理记录";
  }
  const parts = [
    activity.projectFiles ? `项目资料 ${activity.projectFiles}` : "",
    activity.generalFiles ? `通用资料 ${activity.generalFiles}` : "",
    activity.temporaryFiles ? `临时资料 ${activity.temporaryFiles}` : "",
  ].filter(Boolean);
  return parts.length ? parts.join(" · ") : `已整理 ${activity.newManagedFiles} 个文件`;
}

function formatWorkspaceActivityKind(kind: string) {
  const labels: Record<string, string> = {
    managed: "已整理",
    active: "活跃",
    versioned: "新版本",
    archived: "归档",
    missing: "缺失",
    superseded: "已撤销",
    failed: "失败",
    undoConflict: "撤销冲突",
    received: "已接收",
    understood: "已理解",
  };
  return labels[kind] || kind || "记录";
}

const OWNERSHIP_LABELS: Record<string, string> = {
  existingProject: "已有项目资料",
  generalWorkMaterial: "通用工作资料",
  newProjectCandidate: "建议创建新项目",
  temporaryOrReference: "临时/参考",
  needsReview: "待检查",
  unsupportedOrFailed: "无法处理",
};

function ownershipLabel(ownershipType: string) {
  return OWNERSHIP_LABELS[ownershipType] || "";
}
