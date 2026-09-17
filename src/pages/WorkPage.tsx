import { useEffect, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { useNavigate, useSearchParams } from "react-router-dom";
import { useAppState } from "../app/AppState";
import { AppLayout } from "../components/AppLayout";
import { GlobalSearchPanel } from "../components/GlobalSearchPanel";
import { DecisionTraceDetails, formatTraceConfidence } from "../components/DecisionTraceDetails";
import { Badge, Button, Feedback } from "../components/ui";
import {
  cancelCodexTaskRun,
  chooseImportFiles,
  getProjectContextPacket,
  listCodexTasks,
  rebindCodexTaskPrompt,
  scanCodexResultBridge,
  startCodexTaskRun,
} from "../features/project/desktopApi";
import { ProjectImpactPanel } from "../features/project/ProjectImpactPanel";
import type {
  AtlasAssessment,
  CodexExternalResult,
  CodexPromptRecord,
  CodexReportRecord,
  CodexRun,
  CodexTask,
  ManagedFile,
  PendingActionProjection,
  MonitoringState,
  PendingReviewItem,
  ProjectAnalysis,
  ProjectFactCaptureRequest,
  ProjectContextPacket,
  WorkLedgerSnapshot,
  WorkspaceMessage,
} from "../features/project/desktopApi";

type BrowserSpeechRecognition = {
  continuous: boolean;
  interimResults: boolean;
  lang: string;
  start: () => void;
  stop: () => void;
  onresult: ((event: SpeechRecognitionEventLike) => void) | null;
  onerror: ((event: { error: string }) => void) | null;
  onend: (() => void) | null;
};

type SpeechRecognitionEventLike = {
  results: ArrayLike<ArrayLike<{ transcript: string }>>;
};

declare global {
  interface Window {
    SpeechRecognition?: new () => BrowserSpeechRecognition;
    webkitSpeechRecognition?: new () => BrowserSpeechRecognition;
  }
}

export function WorkPage() {
  const { activeProject } = useAppState();
  return <ProjectWorkPage key={`${activeProject?.id ?? ""}:${activeProject?.rootDir ?? ""}`} />;
}

function ProjectWorkPage() {
  const navigate = useNavigate();
  const threadRef = useRef<HTMLElement | null>(null);
  const endRef = useRef<HTMLDivElement | null>(null);
  const recognitionRef = useRef<BrowserSpeechRecognition | null>(null);
  const mountedRef = useRef(false);
  const runRequestRef = useRef(0);
  const selectedTaskRef = useRef("");
  const previousCountsRef = useRef({
    messages: 0,
    files: 0,
    reviews: 0,
    reports: 0,
  });
  const shouldFollowBottomRef = useRef(true);
  const {
    activeProject,
    projects,
    activeManifest,
    workspaceMessages,
    managedFiles,
    codexPrompts,
    codexReports,
    codexTasks,
    pendingReviews,
    tasks,
    dailySessions,
    locationDecisions,
    memos,
    monitoring,
    atlas,
    projectAnalysis,
    openProject,
    sendProjectMessage,
    stopProjectMessage,
    saveProjectDraft,
    importFilesToActiveProject,
    finishProjectWork,
    generateCodexPrompt,
    acceptCodexTask,
    rejectCodexTask,
    importCodexReportText,
    importCodexReportFile,
    applyCodexReport,
    grantProjectDeepSeekAuthorization,
    openFilePath,
    openFolderPath,
    addMemo,
    updateMemo,
    deleteMemo,
    weeklyReviewDashboard,
    generateWeeklyReviews,
    confirmWeeklyReport,
    error,
    setError,
    impactAnalyses,
    actionCandidates,
    stateProposals,
    dailyContinue,
    projectStateAutoApply,
    refreshProjectImpact,
    refreshDailyContinue,
    regenerateDailyContinue,
    confirmActionCandidate,
    ignoreActionCandidate,
    updateActionCandidate,
    applyStateProposal,
    undoStateProposal,
    markTodayDone,
    setStateAutoApply,
    updateProjectAttention,
    updateDataHealthStatus,
    workLedger,
    refreshWorkLedger,
    refreshGitSnapshot,
    recordUserDecisionEvent,
    captureProjectFact,
  } = useAppState();
  const [draft, setDraft] = useState("");
  const [isWrapping, setIsWrapping] = useState(false);
  const [done, setDone] = useState("");
  const [nextStep, setNextStep] = useState("");
  const [isSending, setIsSending] = useState(false);
  const [isImporting, setIsImporting] = useState(false);
  const [isFinishing, setIsFinishing] = useState(false);
  const [isListening, setIsListening] = useState(false);
  const [showSearchPanel, setShowSearchPanel] = useState(false);
  const [showCodexPanel, setShowCodexPanel] = useState(false);
  const [showProjectContextPanel, setShowProjectContextPanel] = useState(false);
  const [showWorkCapturePanel, setShowWorkCapturePanel] = useState(false);
  const [projectContextPacket, setProjectContextPacket] = useState<ProjectContextPacket | null>(null);
  const [isLoadingProjectContext, setIsLoadingProjectContext] = useState(false);
  const [isCopyingProjectContext, setIsCopyingProjectContext] = useState(false);
  const [codexPromptText, setCodexPromptText] = useState("");
  const [codexReportInput, setCodexReportInput] = useState("");
  const [isGeneratingCodexPrompt, setIsGeneratingCodexPrompt] = useState(false);
  const [isImportingCodexReport, setIsImportingCodexReport] = useState(false);
  const [isApplyingCodexReport, setIsApplyingCodexReport] = useState(false);
  const [isUpdatingCodexTask, setIsUpdatingCodexTask] = useState(false);
  const [codexRuns, setCodexRuns] = useState<CodexRun[]>([]);
  const [isStartingCodexRun, setIsStartingCodexRun] = useState(false);
  const [isCancellingCodexRun, setIsCancellingCodexRun] = useState(false);
  const [gitRepositoryPath, setGitRepositoryPath] = useState("");
  const [isRefreshingGit, setIsRefreshingGit] = useState(false);
  const [decisionText, setDecisionText] = useState("");
  const [decisionReason, setDecisionReason] = useState("");
  const [isRecordingDecision, setIsRecordingDecision] = useState(false);
  const [captureType, setCaptureType] = useState<ProjectFactCaptureRequest["captureType"]>("progress");
  const [captureContent, setCaptureContent] = useState("");
  const [captureReason, setCaptureReason] = useState("");
  const [captureFeedback, setCaptureFeedback] = useState("");
  const [isCapturingFact, setIsCapturingFact] = useState(false);
  const [selectedManagedFileId, setSelectedManagedFileId] = useState<string | null>(null);
  const [sidebarOpen, setSidebarOpen] = useState(false);
  const [rightSidebarOpen, setRightSidebarOpen] = useState(true);
  const [memoDraft, setMemoDraft] = useState("");
  const [searchParams, setSearchParams] = useSearchParams();
  const [isGeneratingWeeklyReview, setIsGeneratingWeeklyReview] = useState(false);
  const [isConfirmingWeeklyReview, setIsConfirmingWeeklyReview] = useState(false);
  const targetCodexTaskId = searchParams.get("taskId") || "";
  const targetCaptureActionSourceRef = searchParams.get("actionId") || "";
  const showWeeklyReview = searchParams.get("panel") === "weekly-review";
  const showProjectImpact = searchParams.get("panel") === "project-impact";

  useEffect(() => {
    if (searchParams.get("panel") !== "codex") return;
    setShowSearchPanel(false);
    setShowCodexPanel(true);
  }, [searchParams]);

  useEffect(() => {
    if (searchParams.get("panel") !== "work-capture") return;
    setShowSearchPanel(false);
    setShowCodexPanel(false);
    setShowProjectContextPanel(false);
    setShowWorkCapturePanel(true);
  }, [searchParams]);

  useEffect(() => {
    if (!showProjectImpact || !activeProject) return;
    void refreshProjectImpact().catch((err) => setError(String(err)));
    void refreshDailyContinue().catch((err) => setError(String(err)));
  }, [showProjectImpact, activeProject?.id]);

  useEffect(() => {
    setDraft(activeManifest?.draft.text ?? "");
  }, [activeProject?.id, activeManifest?.draft.text]);

  const currentCodexTask = selectCurrentCodexTask(codexTasks, codexRuns, targetCodexTaskId);

  useLayoutEffect(() => {
    mountedRef.current = true;
    return () => { mountedRef.current = false; };
  }, []);

  useLayoutEffect(() => {
    selectedTaskRef.current = currentCodexTask?.taskId ?? "";
  }, [currentCodexTask?.taskId]);

  function setCodexError(message: string) {
    if (mountedRef.current) setError(message);
  }

  function closeWorkCapturePanel() {
    setShowWorkCapturePanel(false);
    setSearchParams((previous) => {
      if (previous.get("panel") !== "work-capture" && !previous.has("actionId")) return previous;
      const next = new URLSearchParams(previous);
      if (next.get("panel") === "work-capture") next.delete("panel");
      next.delete("actionId");
      return next;
    });
  }

  function selectCodexTask(taskId: string) {
    setSearchParams(buildCodexTaskSearchParams(searchParams, taskId));
  }

  function scrollCodexConsoleToTop() {
    window.requestAnimationFrame(() => {
      if (!mountedRef.current) return;
      document.getElementById("codex-console-top")?.scrollIntoView({ block: "start", behavior: "auto" });
    });
  }

  useEffect(() => {
    setCodexPromptText(resolveCodexPromptText(currentCodexTask, codexPrompts));
  }, [activeProject?.id, currentCodexTask?.taskId, currentCodexTask?.prompt, codexPrompts]);

  useEffect(() => {
    void refreshWorkLedger().catch((err) => setCodexError(String(err)));
  }, [activeProject?.id]);

  useEffect(() => {
    if (!activeProject) {
      setCodexRuns([]);
      return;
    }
    void refreshCodexRuns().catch((err) => setCodexError(String(err)));
  }, [activeProject?.id]);

  useEffect(() => {
    if (!activeProject || !codexRuns.some((run) => run.status === "running")) return;
    let busy = false;
    const timer = window.setInterval(() => {
      if (busy) return;
      busy = true;
      void refreshCodexRuns().catch((err) => setCodexError(String(err))).finally(() => { busy = false; });
    }, 2500);
    return () => window.clearInterval(timer);
  }, [activeProject?.id, codexRuns]);

  useEffect(() => {
    if (!activeProject) return;
    setGitRepositoryPath(
      workLedger?.gitSnapshot?.repositoryPath || activeProject.repositoryPath || activeProject.rootDir,
    );
  }, [activeProject?.id, activeProject?.rootDir, activeProject?.repositoryPath, workLedger?.gitSnapshot?.repositoryPath]);

  useEffect(() => {
    if (!managedFiles.length) {
      setSelectedManagedFileId(null);
      return;
    }
    setSelectedManagedFileId((current) =>
      current && managedFiles.some((file) => file.id === current) ? current : managedFiles[0].id,
    );
  }, [managedFiles]);

  useEffect(() => {
    if (!activeProject) return;
    const timer = window.setTimeout(() => {
      void saveProjectDraft(draft).catch((err) => setError(String(err)));
    }, 400);
    return () => window.clearTimeout(timer);
  }, [activeProject?.id, draft, saveProjectDraft, setError]);

  useLayoutEffect(() => {
    const thread = threadRef.current;
    if (!thread) return;
    thread.scrollTop = thread.scrollHeight;
    shouldFollowBottomRef.current = true;
    previousCountsRef.current = {
      messages: workspaceMessages.length,
      files: managedFiles.length,
      reviews: pendingReviews.length,
      reports: codexReports.length,
    };
  }, [activeProject?.id]);

  useEffect(() => {
    const nextCounts = {
      messages: workspaceMessages.length,
      files: managedFiles.length,
      reviews: pendingReviews.length,
      reports: codexReports.length,
    };
    const previous = previousCountsRef.current;
    const hasNewContent =
      nextCounts.messages > previous.messages ||
      nextCounts.files > previous.files ||
      nextCounts.reviews > previous.reviews;
    if (hasNewContent && (shouldFollowBottomRef.current || isNearBottom(threadRef.current))) {
      endRef.current?.scrollIntoView({ block: "end", behavior: "smooth" });
    }
    previousCountsRef.current = nextCounts;
  }, [workspaceMessages.length, managedFiles.length, pendingReviews.length, codexReports.length]);

  useEffect(
    () => () => {
      recognitionRef.current?.stop();
      recognitionRef.current = null;
    },
    [],
  );

  if (!activeProject) {
    return (
      <main className="workspace-empty">
        <button type="button" className="btn btn-primary" onClick={() => navigate("/")}>
          返回开始页
        </button>
      </main>
    );
  }

  async function addFiles() {
    if (isImporting) return;
    setIsImporting(true);
    try {
      const files = await chooseImportFiles();
      if (!files.length) return;
      shouldFollowBottomRef.current = true;
      await importFilesToActiveProject(files, draft || "补充资料");
      setError("");
    } catch (err) {
      setError(String(err));
    } finally {
      setIsImporting(false);
    }
  }

  async function submitMessage() {
    if (!draft.trim() || isSending) return;
    setIsSending(true);
    try {
      shouldFollowBottomRef.current = true;
      await sendProjectMessage(draft);
      setDraft("");
    } catch (err) {
      setError(String(err));
    } finally {
      setIsSending(false);
    }
  }

  function applyComposerSuggestion(text: string) {
    setDraft((current) => (current.trim() ? current : text));
  }

  async function confirmFinish() {
    if (isFinishing) return;
    setIsFinishing(true);
    try {
      await finishProjectWork(done, nextStep);
      navigate("/");
    } catch (err) {
      setError(String(err));
      setIsFinishing(false);
    }
  }

  function toggleVoiceInput() {
    if (isListening) {
      recognitionRef.current?.stop();
      return;
    }
    const SpeechRecognitionCtor = window.SpeechRecognition || window.webkitSpeechRecognition;
    if (!SpeechRecognitionCtor) {
      setError("当前桌面环境没有可用的中文语音输入接口。");
      return;
    }
    const recognition = new SpeechRecognitionCtor();
    recognition.lang = "zh-CN";
    recognition.continuous = false;
    recognition.interimResults = false;
    recognition.onresult = (event) => {
      const transcript = Array.from(event.results)
        .flatMap((result) => Array.from(result))
        .map((item) => item.transcript)
        .join("")
        .trim();
      if (transcript) {
        setDraft((previous) => `${previous}${previous.trim() ? "\n" : ""}${transcript}`.trimStart());
      }
      setError("");
    };
    recognition.onerror = (event) => {
      setError(`语音输入失败: ${event.error}`);
    };
    recognition.onend = () => {
      setIsListening(false);
      recognitionRef.current = null;
    };
    recognitionRef.current = recognition;
    setIsListening(true);
    recognition.start();
  }

  async function handleGenerateCodexPrompt() {
    if (isGeneratingCodexPrompt) return;
    setIsGeneratingCodexPrompt(true);
    try {
      const result = await generateCodexPrompt();
      if (!result || !mountedRef.current) return;
      setCodexPromptText(result.prompt.promptText);
      setShowCodexPanel(true);
      selectCodexTask(result.prompt.id);
      scrollCodexConsoleToTop();
      setCodexError("");
    } catch (err) {
      setCodexError(String(err));
    } finally {
      setIsGeneratingCodexPrompt(false);
    }
  }

  async function handleCopyCodexPrompt() {
    if (!codexPromptText.trim()) return;
    try {
      await navigator.clipboard.writeText(codexPromptText);
      setCodexError("");
    } catch (err) {
      setCodexError(String(err));
    }
  }

  async function handleOpenProjectContext() {
    if (!activeProject || isLoadingProjectContext) return;
    setIsLoadingProjectContext(true);
    try {
      const packet = await getProjectContextPacket(activeProject.rootDir);
      if (!mountedRef.current) return;
      setProjectContextPacket(packet);
      setShowSearchPanel(false);
      setShowCodexPanel(false);
      setShowWorkCapturePanel(false);
      setShowProjectContextPanel(true);
      setError("");
    } catch (err) {
      setError(String(err));
    } finally {
      setIsLoadingProjectContext(false);
    }
  }

  async function handleCopyProjectContext() {
    if (!projectContextPacket?.markdown.trim() || isCopyingProjectContext) return;
    setIsCopyingProjectContext(true);
    try {
      await navigator.clipboard.writeText(projectContextPacket.markdown);
      setError("");
    } catch (err) {
      setError(`复制项目上下文失败：${String(err)}`);
    } finally {
      setIsCopyingProjectContext(false);
    }
  }

  async function handleImportCodexReportText() {
    if (!codexReportInput.trim() || isImportingCodexReport) return;
    setIsImportingCodexReport(true);
    try {
      shouldFollowBottomRef.current = true;
      const result = await importCodexReportText(codexReportInput);
      if (!mountedRef.current) return;
      if (!result.duplicate) {
        setCodexReportInput("");
      }
      setShowCodexPanel(true);
      setCodexError("");
    } catch (err) {
      setCodexError(String(err));
    } finally {
      setIsImportingCodexReport(false);
    }
  }

  async function handleImportCodexReportFile() {
    if (isImportingCodexReport) return;
    setIsImportingCodexReport(true);
    try {
      const files = await chooseImportFiles();
      if (!mountedRef.current) return;
      const reportPath = files.find((file) => /\.(txt|md)$/i.test(file));
      if (!reportPath) {
        setCodexError("请选择 txt 或 md 格式的 Codex 报告。");
        return;
      }
      shouldFollowBottomRef.current = true;
      await importCodexReportFile(reportPath);
      if (!mountedRef.current) return;
      setShowCodexPanel(true);
      setCodexError("");
    } catch (err) {
      setCodexError(String(err));
    } finally {
      setIsImportingCodexReport(false);
    }
  }

  async function handleApplyCodexReport(reportId: string) {
    if (isApplyingCodexReport) return;
    setIsApplyingCodexReport(true);
    try {
      await applyCodexReport(reportId);
      setCodexError("");
    } catch (err) {
      setCodexError(String(err));
    } finally {
      setIsApplyingCodexReport(false);
    }
  }

  async function refreshCodexRuns() {
    if (!activeProject) return;
    if (!mountedRef.current) return;
    const request = ++runRequestRef.current;
    try {
      const result = await listCodexTasks(activeProject.rootDir);
      if (!mountedRef.current || request !== runRequestRef.current) return;
      setCodexRuns(result.runs ?? []);
      await refreshWorkLedger();
    } catch (err) {
      if (mountedRef.current && request === runRequestRef.current) throw err;
    }
  }

  async function handleRefreshCodexStatus() {
    if (!activeProject || isImportingCodexReport) return;
    setIsImportingCodexReport(true);
    try {
      await scanCodexResultBridge(activeProject.rootDir);
      if (!mountedRef.current) return;
      await refreshCodexRuns();
      setCodexError("");
    } catch (err) {
      setCodexError(String(err));
    } finally {
      setIsImportingCodexReport(false);
    }
  }

  async function handleStartCodexRun(taskId: string) {
    if (!activeProject || isStartingCodexRun) return;
    setIsStartingCodexRun(true);
    try {
      await startCodexTaskRun(activeProject.rootDir, taskId);
      if (!mountedRef.current) return;
      await refreshCodexRuns();
      setCodexError("");
    } catch (err) {
      setCodexError(String(err));
    } finally {
      setIsStartingCodexRun(false);
    }
  }

  async function handleRebindCodexPrompt(taskId: string) {
    if (!activeProject || isUpdatingCodexTask) return;
    setIsUpdatingCodexTask(true);
    try {
      const task = await rebindCodexTaskPrompt(activeProject.rootDir, taskId);
      if (!mountedRef.current) return;
      if (selectedTaskRef.current === taskId) setCodexPromptText(task.prompt);
      await refreshCodexRuns();
      setCodexError("");
    } catch (err) {
      setCodexError(String(err));
    } finally {
      setIsUpdatingCodexTask(false);
    }
  }

  async function handleCancelCodexRun(taskId: string) {
    if (!activeProject || isCancellingCodexRun) return;
    setIsCancellingCodexRun(true);
    try {
      await cancelCodexTaskRun(activeProject.rootDir, taskId);
      if (!mountedRef.current) return;
      await refreshCodexRuns();
      setCodexError("已停止 Codex 进程，仓库可能存在未提交修改，请查看 Git 事实。");
    } catch (err) {
      setCodexError(String(err));
    } finally {
      setIsCancellingCodexRun(false);
    }
  }

  async function handleAcceptCodexTask(taskId: string, resultId: string) {
    if (isUpdatingCodexTask) return;
    setIsUpdatingCodexTask(true);
    try {
      await acceptCodexTask(taskId, resultId);
      setCodexError("");
    } catch (err) {
      setCodexError(String(err));
    } finally {
      setIsUpdatingCodexTask(false);
    }
  }

  async function handleRejectCodexTask(taskId: string, resultId: string) {
    if (isUpdatingCodexTask) return;
    const reason = window.prompt("请简单说明验收失败原因。") ?? "";
    if (!reason.trim()) return;
    setIsUpdatingCodexTask(true);
    try {
      await rejectCodexTask(taskId, resultId, reason);
      setCodexError("");
    } catch (err) {
      setCodexError(String(err));
    } finally {
      setIsUpdatingCodexTask(false);
    }
  }

  async function handleRefreshGitSnapshot() {
    if (isRefreshingGit) return;
    setIsRefreshingGit(true);
    try {
      await refreshGitSnapshot(gitRepositoryPath.trim() || undefined);
      setCodexError("");
    } catch (err) {
      setCodexError(String(err));
    } finally {
      setIsRefreshingGit(false);
    }
  }

  async function handleRecordDecision() {
    if (!decisionText.trim() || isRecordingDecision) return;
    setIsRecordingDecision(true);
    try {
      await recordUserDecisionEvent(decisionText, decisionReason);
      setDecisionText("");
      setDecisionReason("");
      setCodexError("");
    } catch (err) {
      setCodexError(String(err));
    } finally {
      setIsRecordingDecision(false);
    }
  }

  async function handleCaptureProjectFact() {
    if (isCapturingFact) return;
    setIsCapturingFact(true);
    try {
      await captureProjectFact({
        captureType,
        content: captureContent,
        reason: captureReason,
      });
      setCaptureContent("");
      setCaptureReason("");
      setCaptureFeedback("已写入项目工作事实。继续工作时会自动使用这条记录。");
      setError("");
    } catch (err) {
      setError(String(err));
    } finally {
      setIsCapturingFact(false);
    }
  }

  async function handleResolveCapturedAction(actionSourceRef: string) {
    if (isCapturingFact) return;
    setIsCapturingFact(true);
    try {
      await captureProjectFact({
        captureType: "resolveAction",
        content: "",
        actionSourceRef,
      });
      setCaptureFeedback("已关闭该事项，历史记录会保留。");
      setError("");
    } catch (err) {
      setError(String(err));
    } finally {
      setIsCapturingFact(false);
    }
  }

  async function handleGenerateWeeklyReview(force = false) {
    if (isGeneratingWeeklyReview) return;
    setIsGeneratingWeeklyReview(true);
    try {
      await generateWeeklyReviews(force);
      setSearchParams((previous) => {
        const next = new URLSearchParams(previous);
        next.set("panel", "weekly-review");
        return next;
      });
      setError("");
    } catch (err) {
      setError(String(err));
    } finally {
      setIsGeneratingWeeklyReview(false);
    }
  }

  async function handleConfirmWeeklyReview(reportId: string) {
    if (isConfirmingWeeklyReview) return;
    setIsConfirmingWeeklyReview(true);
    try {
      await confirmWeeklyReport(reportId);
      setError("");
    } catch (err) {
      setError(String(err));
    } finally {
      setIsConfirmingWeeklyReview(false);
    }
  }

  const latestCodexReport = currentCodexTask
    ? codexReports.find((report) => codexReportTaskId(report) === currentCodexTask.taskId) ?? null
    : null;
  const selectedManagedFile = managedFiles.find((file) => file.id === selectedManagedFileId) ?? managedFiles[0] ?? null;
  const conversationMessages = workspaceMessages.filter((message, index) =>
    isConversationMessage(message, index, activeProject.description),
  );
  const activityMessages = workspaceMessages.filter((message, index) =>
    !isConversationMessage(message, index, activeProject.description),
  );

  return (
    <AppLayout className={`work-shell workspace-shell${sidebarOpen ? " left-sidebar-open" : ""}`}>
      <button
        type="button"
        className="sidebar-peek"
        onClick={() => setSidebarOpen((value) => !value)}
        aria-label="展开侧边栏"
      >
        {sidebarOpen ? "←" : "→"}
      </button>

      <aside className={`start-sidebar work-sidebar ${sidebarOpen ? "open" : ""}`}>
        <SidebarSection title="最近项目">
          {projects.length ? (
            projects.slice(0, 5).map((project) => (
              <button
                key={project.id}
                type="button"
                className="sidebar-item"
                onClick={() => void openProject(project.rootDir)}
              >
                {project.name}
              </button>
            ))
          ) : (
            <p>暂无历史项目</p>
          )}
        </SidebarSection>

        <SidebarSection title="历史工作台">
          {dailySessions.length ? (
            dailySessions
              .slice(-4)
              .reverse()
              .map((session) => <p key={session.id}>{session.dateKey} · {session.messageIds.length} 条记录</p>)
          ) : (
            <p>暂无历史记录</p>
          )}
        </SidebarSection>

        <SidebarSection title="待检查事项">
          {pendingReviews.length ? (
            pendingReviews.slice(0, 4).map((item) => <p key={item.id}>{item.title}</p>)
          ) : (
            <p>暂无待检查事项</p>
          )}
        </SidebarSection>

        <SidebarSection title="项目文件位置">
          {locationDecisions.length ? (
            locationDecisions.slice(-4).reverse().map((item) => (
              <p key={item.id}>
                {item.fileName} · {item.managedRelativePath}
              </p>
            ))
          ) : (
            <p>{managedFiles.length} 个文件已登记</p>
          )}
        </SidebarSection>

        <SidebarSection title="当前任务">
          {tasks.length ? (
            tasks.slice(-3).reverse().map((task) => <p key={task.id}>{task.title} · {task.status}</p>)
          ) : (
            <p>暂无当前任务</p>
          )}
        </SidebarSection>

        <SidebarSection title="备忘录">
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
        </SidebarSection>
      </aside>

      {sidebarOpen ? (
        <button
          type="button"
          className="workspace-sidebar-backdrop"
          onClick={() => setSidebarOpen(false)}
          aria-label="关闭左侧栏"
        />
      ) : null}

      <header className="workspace-topbar">
        <div className="workspace-brand">
          <span className="section-label">当前项目</span>
          <strong>{activeProject.name}</strong>
          <span>{activeProject.rootDir}</span>
        </div>
        <div className="workspace-top-actions">
          <div className="workspace-top-actions-main">
            <span className="workspace-actions-label">工作流</span>
            <button
              type="button"
              className={`btn ${showCodexPanel ? "btn-primary" : "btn-secondary"}`}
              onClick={() => {
                setShowProjectContextPanel(false);
                setShowWorkCapturePanel(false);
                setShowCodexPanel((value) => !value);
              }}
            >
              交给 Codex
            </button>
            <Button
              type="button"
              variant="ghost"
              disabled={isLoadingProjectContext}
              loading={isLoadingProjectContext}
              onClick={() => void handleOpenProjectContext()}
            >
              项目上下文
            </Button>
            <Button
              type="button"
              variant={showWorkCapturePanel ? "primary" : "ghost"}
              onClick={() => {
                setShowSearchPanel(false);
                setShowCodexPanel(false);
                setShowProjectContextPanel(false);
                if (showWorkCapturePanel) {
                  closeWorkCapturePanel();
                } else {
                  setShowWorkCapturePanel(true);
                }
                setCaptureFeedback("");
              }}
            >
              记录工作
            </Button>
            <button
              type="button"
              className={`btn ${showSearchPanel ? "btn-primary" : "btn-ghost"}`}
              onClick={() => {
                setShowProjectContextPanel(false);
                setShowWorkCapturePanel(false);
                setShowSearchPanel((value) => !value);
              }}
            >
              全局搜索
            </button>
            <button
              type="button"
              className={`btn ${showWeeklyReview ? "btn-primary" : "btn-ghost"}`}
              onClick={() => {
                setShowSearchPanel(false);
                setShowCodexPanel(false);
                setShowWorkCapturePanel(false);
                setSearchParams((previous) => {
                  const next = new URLSearchParams(previous);
                  if (next.get("panel") === "weekly-review") {
                    next.delete("panel");
                  } else {
                    next.set("panel", "weekly-review");
                  }
                  return next;
                });
              }}
            >
              本周复盘
            </button>
            <button
              type="button"
              className={`btn ${showProjectImpact ? "btn-primary" : "btn-ghost"}`}
              onClick={() => {
                setShowSearchPanel(false);
                setShowCodexPanel(false);
                setShowWorkCapturePanel(false);
                setSearchParams((previous) => {
                  const next = new URLSearchParams(previous);
                  if (next.get("panel") === "project-impact") {
                    next.delete("panel");
                  } else {
                    next.set("panel", "project-impact");
                  }
                  return next;
                });
              }}
            >
              项目影响与行动
            </button>
          </div>
          <div className="workspace-top-actions-side">
            <span className="workspace-actions-label">工具</span>
            <button type="button" className="btn btn-ghost" onClick={() => setRightSidebarOpen((value) => !value)}>
              {rightSidebarOpen ? "隐藏文件" : "显示文件"}
            </button>
            <button type="button" className="btn btn-ghost" onClick={() => navigate("/")}>
              切换项目
            </button>
            <button type="button" className="btn" onClick={() => setIsWrapping(true)}>
              收工
            </button>
            <button type="button" className="icon-btn" onClick={() => navigate("/settings")} aria-label="设置">
              ⚙
            </button>
          </div>
        </div>
      </header>

      {showSearchPanel || showCodexPanel || showProjectContextPanel || showWorkCapturePanel ? (
        <section className="workspace-top-panels">
          <div className="thread-inner workspace-top-panels-inner">
            {showSearchPanel ? <GlobalSearchPanel autoFocus /> : null}
            {showProjectContextPanel ? (
              <ProjectContextPacketPanel
                packet={projectContextPacket}
                isLoading={isLoadingProjectContext}
                isCopying={isCopyingProjectContext}
                onCopy={() => void handleCopyProjectContext()}
                onClose={() => setShowProjectContextPanel(false)}
              />
            ) : null}
            {showWorkCapturePanel ? (
              <ProjectFactCapturePanel
                captureType={captureType}
                content={captureContent}
                reason={captureReason}
                pendingActions={workLedger?.pendingActions ?? []}
                highlightedActionSourceRef={targetCaptureActionSourceRef}
                isSaving={isCapturingFact}
                feedback={captureFeedback}
                onCaptureTypeChange={setCaptureType}
                onContentChange={setCaptureContent}
                onReasonChange={setCaptureReason}
                onSubmit={() => void handleCaptureProjectFact()}
                onResolve={(sourceRef) => void handleResolveCapturedAction(sourceRef)}
                onClose={closeWorkCapturePanel}
              />
            ) : null}
            {showCodexPanel ? (
              <CodexCollaborationPanel
                promptText={codexPromptText}
                onPromptChange={setCodexPromptText}
                onGeneratePrompt={handleGenerateCodexPrompt}
                onCopyPrompt={handleCopyCodexPrompt}
                onRebindTaskPrompt={handleRebindCodexPrompt}
                onStartTaskRun={handleStartCodexRun}
                onCancelTaskRun={handleCancelCodexRun}
                onAcceptTask={handleAcceptCodexTask}
                onRejectTask={handleRejectCodexTask}
                onSelectTask={selectCodexTask}
                onRefreshCodexStatus={handleRefreshCodexStatus}
                reportInput={codexReportInput}
                onReportInputChange={setCodexReportInput}
                onImportReportText={handleImportCodexReportText}
                onImportReportFile={handleImportCodexReportFile}
                onApplyReport={handleApplyCodexReport}
                latestPrompt={codexPrompts[0] ?? null}
                latestReport={latestCodexReport}
                codexTasks={codexTasks}
                codexRuns={codexRuns}
                targetTaskId={targetCodexTaskId}
                repositoryPath={gitRepositoryPath}
                onRepositoryPathChange={setGitRepositoryPath}
                onRefreshGit={handleRefreshGitSnapshot}
                isRefreshingGit={isRefreshingGit}
                decisionText={decisionText}
                onDecisionTextChange={setDecisionText}
                decisionReason={decisionReason}
                onDecisionReasonChange={setDecisionReason}
                onRecordDecision={handleRecordDecision}
                isRecordingDecision={isRecordingDecision}
                workLedger={workLedger}
                isGeneratingPrompt={isGeneratingCodexPrompt}
                isImportingReport={isImportingCodexReport}
                isApplyingReport={isApplyingCodexReport}
                isUpdatingTask={isUpdatingCodexTask}
                isStartingRun={isStartingCodexRun}
                isCancellingRun={isCancellingCodexRun}
              />
            ) : null}
          </div>
        </section>
      ) : null}

      <section className={`workspace-body${rightSidebarOpen ? "" : " sidebar-hidden"}`}>
        <section
          ref={threadRef}
          className="workspace-thread"
          onScroll={(event) => {
            shouldFollowBottomRef.current = isNearBottom(event.currentTarget);
          }}
        >
          <div className="thread-inner">
            {error ? <div className="restore-note">{error}</div> : null}
            <div className="restore-note">
              <span>项目恢复信息</span>
              <strong>{activeProject.nextStep}</strong>
            </div>

            {showWeeklyReview ? (
              <section className="restore-note weekly-review-note">
                <div className="weekly-review-hero">
                  <div className="wrap-inline-head">
                    <span className="section-label">本周复盘</span>
                    <strong>{weeklyReviewDashboard?.generatedAt || "等待生成"}</strong>
                  </div>
                  {weeklyReviewDashboard ? (
                    <div className="weekly-review-conclusion">
                      <span className="section-label">本周结论</span>
                      <p>
                        {weeklyReviewDashboard.globalReports[0]?.suggestions[0] ||
                          weeklyReviewDashboard.projectReports[0]?.suggestions[0] ||
                          "当前还没有足够的真实记录形成明确结论。"}
                      </p>
                    </div>
                  ) : null}
                  {weeklyReviewDashboard ? (
                    <div className="weekly-review-summary">
                      <article className="weekly-review-summary-item">
                        <span>当前周</span>
                        <strong>{weeklyReviewDashboard.currentWeekKey}</strong>
                      </article>
                      <article className="weekly-review-summary-item">
                        <span>项目周报</span>
                        <strong>{weeklyReviewDashboard.projectReports.length} 份</strong>
                      </article>
                      <article className="weekly-review-summary-item">
                        <span>全局周报</span>
                        <strong>{weeklyReviewDashboard.globalReports.length} 份</strong>
                      </article>
                    </div>
                  ) : null}
                </div>
                <div className="settings-action-row">
                  <button
                    type="button"
                    className="btn btn-primary"
                    disabled={isGeneratingWeeklyReview}
                    onClick={() => void handleGenerateWeeklyReview(false)}
                  >
                    {isGeneratingWeeklyReview ? "生成中..." : "立即生成"}
                  </button>
                  <button
                    type="button"
                    className="btn"
                    disabled={isGeneratingWeeklyReview}
                    onClick={() => void handleGenerateWeeklyReview(true)}
                  >
                    强制重算
                  </button>
                  <button
                    type="button"
                    className="btn"
                    onClick={() =>
                      setSearchParams((previous) => {
                        const next = new URLSearchParams(previous);
                        next.delete("panel");
                        return next;
                      })
                    }
                  >
                    关闭
                  </button>
                </div>
                <div className="weekly-review-list">
                  {(weeklyReviewDashboard?.projectReports ?? []).slice(0, 3).map((report) => (
                    <article key={report.id} className="file-result">
                      <div>
                        <strong>{report.scope === "global" ? "全局周报" : report.projectName}</strong>
                        <p>周次：{report.weekKey}</p>
                        <p>本周完成：{report.achievements.join("；") || "暂无"}</p>
                        <p>重要变化：{report.blockers.join("；") || "暂无"}</p>
                        <p>下一步：{report.suggestions.join("；") || "暂无"}</p>
                      </div>
                      <div className="file-result-actions">
                        <button type="button" className="btn" onClick={() => void handleConfirmWeeklyReview(report.id)}>
                          确认收录
                        </button>
                      </div>
                    </article>
                      ))}
                </div>
                {(() => {
                  const candidates = [
                    ...(weeklyReviewDashboard?.projectReports[0]?.skillCandidates ?? []),
                    ...(weeklyReviewDashboard?.globalReports[0]?.skillCandidates ?? []),
                  ];
                  if (!candidates.length) return null;
                  return (
                    <div className="insight-block">
                      <span>技能候选（仅候选，需人工评估，不会自动训练）</span>
                      <div className="answer-evidence">
                        {candidates.map((candidate, index) => (
                          <div key={`${candidate.name}-${index}`} className="evidence-item">
                            <div className="evidence-item-head">
                              <span>{candidate.name}</span>
                              <span>
                                出现 {candidate.occurrenceCount} 次 · 置信度{" "}
                                {formatTraceConfidence(Math.round((candidate.confidence ?? 0) * 100))}
                                {candidate.reviewRequired ? " · 待确认" : ""}
                              </span>
                            </div>
                            <p>{candidate.scenario}</p>
                            <p className="inline-notice">预期价值：{candidate.expectedValue}</p>
                            {candidate.evidence.length ? (
                              <p className="inline-notice">证据：{candidate.evidence.join("；")}</p>
                            ) : null}
                            {candidate.currentGap ? (
                              <p className="inline-notice">当前缺口：{candidate.currentGap}</p>
                            ) : null}
                            <DecisionTraceDetails trace={candidate.decisionTrace} />
                          </div>
                        ))}
                      </div>
                    </div>
                  );
                })()}
              </section>
            ) : null}

            {showProjectImpact ? (
              <ProjectImpactPanel
                impactAnalyses={impactAnalyses}
                actionCandidates={actionCandidates}
                stateProposals={stateProposals}
                dailyContinue={dailyContinue}
                projectStateSummary={activeManifest?.projectStateSummary ?? null}
                projectAttentions={activeManifest?.projectAttentions ?? []}
                workPatternCandidates={activeManifest?.workPatternCandidates ?? []}
                dataHealthRecords={activeManifest?.dataHealthRecords ?? []}
                atlasSkillAssessments={activeManifest?.atlasSkillAssessments ?? []}
                skillLibrary={activeManifest?.skillLibrary ?? []}
                knowledgePatternCandidates={activeManifest?.knowledgePatternCandidates ?? []}
                improvementCandidates={activeManifest?.improvementCandidates ?? []}
                v1Readiness={activeManifest?.v1Readiness ?? null}
                projectStateAutoApply={projectStateAutoApply}
                onConfirmCandidate={confirmActionCandidate}
                onIgnoreCandidate={ignoreActionCandidate}
                onUpdateCandidate={updateActionCandidate}
                onApplyProposal={applyStateProposal}
                onUndoProposal={undoStateProposal}
                onMarkTodayDone={markTodayDone}
                onSetAutoApply={setStateAutoApply}
                onRegenerateDailyContinue={() => void regenerateDailyContinue().catch((err) => setError(String(err)))}
                onUpdateAttention={(attentionId, status) => void updateProjectAttention(attentionId, status)}
                onUpdateDataHealth={(recordId, status) => void updateDataHealthStatus(recordId, status)}
                onReload={() => {
                  void refreshProjectImpact().catch((err) => setError(String(err)));
                  void refreshDailyContinue().catch((err) => setError(String(err)));
                }}
                onClose={() =>
                  setSearchParams((previous) => {
                    const next = new URLSearchParams(previous);
                    next.delete("panel");
                    return next;
                  })
                }
              />
            ) : null}

            {conversationMessages.length ? (
              conversationMessages.map((message) => (
              <Message key={message.id} message={message} openFilePath={openFilePath} openFolderPath={openFolderPath} />
              ))
            ) : (
              <section className="conversation-empty-start">
                <span className="section-label">新的工作线程</span>
                <strong>项目已经准备好，先说今天要推进什么。</strong>
                <p>初始化、资料分析和待检查项已放到右侧「项目动态」。这里保持干净，只承接你和感冒院接下来的项目讨论。</p>
                <div className="conversation-start-hints">
                  <button type="button" onClick={() => applyComposerSuggestion("请基于当前项目资料，告诉我今天最值得继续做的 1 到 3 件事。")}>
                    继续上次任务
                  </button>
                  <button type="button" onClick={() => applyComposerSuggestion("请整理当前项目的下一步 Codex 执行提示词，要求收敛、可验收。")}>
                    整理 Codex 提示词
                  </button>
                  <button type="button" onClick={() => applyComposerSuggestion("请检查新增资料和待确认事项，告诉我哪些需要先处理。")}>
                    分析新增资料
                  </button>
                </div>
              </section>
            )}

            {isWrapping ? (
              <section className="wrap-inline">
                <div className="wrap-inline-head">
                  <span className="section-label">收工</span>
                  <strong>保存项目恢复点</strong>
                </div>
                <label>
                  <span>今天完成了什么</span>
                  <textarea rows={3} value={done} onChange={(event) => setDone(event.target.value)} />
                </label>
                <label>
                  <span>下一步是什么</span>
                  <textarea rows={3} value={nextStep} onChange={(event) => setNextStep(event.target.value)} />
                </label>
                <div className="wrap-actions">
                  <button type="button" className="btn" onClick={() => setIsWrapping(false)}>
                    继续工作
                  </button>
                  <button type="button" className="btn btn-primary" disabled={isFinishing} onClick={() => void confirmFinish()}>
                    {isFinishing ? "正在保存..." : "确认收工"}
                  </button>
                </div>
              </section>
            ) : null}
            <div ref={endRef} />
          </div>
        </section>

        <aside className={`workspace-sidebar${rightSidebarOpen ? "" : " hidden"}`}>
          <div className="workspace-sidebar-inner">
            <WorkspaceActivityPanel
              projectAnalysis={projectAnalysis}
              atlas={atlas}
              monitoring={monitoring}
              pendingReviews={pendingReviews}
              activityMessages={activityMessages}
              needsAuthorization={!activeManifest?.deepseekAuthorization?.grantedAt}
              onGrantAuthorization={grantProjectDeepSeekAuthorization}
            />
            <div className="workspace-sidebar-head">
              <span className="section-label">当前参考资料</span>
              <strong>{selectedManagedFile ? selectedManagedFile.fileName : `${managedFiles.length} 个已登记`}</strong>
              <p>{selectedManagedFile ? "当前只展开一份文件详情，避免右侧栏过重。" : "只保留当前需要参考的资料，详情点击后展开。"}</p>
            </div>
            <div className="workspace-sidebar-list">
              {managedFiles.length ? (
                <>
                  <div className="workspace-file-list">
                    {managedFiles.map((file) => (
                      <button
                        key={file.id}
                        type="button"
                        className={`workspace-file-item${selectedManagedFile?.id === file.id ? " active" : ""}`}
                        onClick={() => setSelectedManagedFileId(file.id)}
                      >
                        <strong>{file.fileName}</strong>
                        <span>{file.recommendedCategory || file.category || "未分类"} · {file.parseStatus === "success" ? "可用" : file.parseStatus === "failed" ? "待检查" : "处理中"}</span>
                      </button>
                    ))}
                  </div>
                  {selectedManagedFile ? (
                    <div className="workspace-selected-file-card">
                      <div className="workspace-selected-file-head">
                        <span className="section-label">当前展开</span>
                        <strong>{selectedManagedFile.recommendedCategory || selectedManagedFile.category || "未分类"}</strong>
                      </div>
                      <FileResult
                        file={selectedManagedFile}
                        openFilePath={openFilePath}
                        openFolderPath={openFolderPath}
                        compactPath
                      />
                    </div>
                  ) : null}
                </>
              ) : (
                <div className="workspace-sidebar-empty">暂无项目文件。</div>
              )}
            </div>
          </div>
        </aside>
      </section>

      <footer className={`workspace-composer${rightSidebarOpen ? "" : " sidebar-hidden"}`}>
        <div className="composer-inner">
          <div className="composer-context-strip">
            {!activeManifest?.deepseekAuthorization?.grantedAt ? (
              <button type="button" onClick={() => void grantProjectDeepSeekAuthorization()}>
                需要授权 DeepSeek 后才能真实回复
              </button>
            ) : (
              <span>DeepSeek 已授权</span>
            )}
            {pendingReviews.length ? <span>{pendingReviews.length} 个资料待确认</span> : <span>资料确认项清空</span>}
            <span>{managedFiles.length} 个项目文件</span>
          </div>
          <div className="composer-row">
            <button type="button" className="btn composer-file-button" disabled={isImporting} onClick={() => void addFiles()}>
              {isImporting ? "正在处理..." : "添加资料"}
            </button>
            <textarea
              rows={3}
              value={draft}
              onChange={(event) => setDraft(event.target.value)}
              placeholder="告诉感冒院你现在要做什么……"
              onKeyDown={(event) => {
                if (event.key === "Enter" && !event.shiftKey) {
                  event.preventDefault();
                  void submitMessage();
                }
              }}
            />
            <button type="button" className="btn composer-voice-button" onClick={toggleVoiceInput}>
              {isListening ? "停止语音" : "语音输入"}
            </button>
          </div>
          <div className="composer-actions">
            <button
              type="button"
              className="btn btn-primary composer-send"
              disabled={!draft.trim() || isSending}
              onClick={() => void submitMessage()}
            >
              {isSending ? "发送中..." : "发送"}
            </button>
            <button type="button" className="btn composer-stop" onClick={() => void stopProjectMessage()}>
              停止生成
            </button>
          </div>
        </div>
      </footer>
    </AppLayout>
  );
}

function ProjectContextPacketPanel({
  packet,
  isLoading,
  isCopying,
  onCopy,
  onClose,
}: {
  packet: ProjectContextPacket | null;
  isLoading: boolean;
  isCopying: boolean;
  onCopy: () => void;
  onClose: () => void;
}) {
  return (
    <section className="project-context-panel" aria-labelledby="project-context-title">
      <header className="project-context-head">
        <div>
          <span className="section-label">项目交接</span>
          <h2 id="project-context-title">项目上下文</h2>
          <p>仅整理当前项目的真实事实，可直接交给外部对话或代码代理。</p>
        </div>
        <div className="project-context-actions">
          <Button type="button" variant="primary" disabled={!packet?.markdown || isLoading} loading={isCopying} onClick={onCopy}>
            复制到剪贴板
          </Button>
          <Button type="button" variant="ghost" onClick={onClose}>
            关闭
          </Button>
        </div>
      </header>
      {isLoading ? <Feedback tone="loading" title="正在整理当前项目事实">不会读取原文件正文，也不会修改项目数据。</Feedback> : null}
      {!isLoading && !packet ? <Feedback title="暂无可复制的项目上下文">请稍后重试。</Feedback> : null}
      {!isLoading && packet ? (
        <div className="project-context-sheet">
          <p className="project-context-privacy">{packet.privacyNotice}</p>
          {packet.focus ? (
            <section className="project-context-section project-context-focus">
              <span className="section-label">当前焦点</span>
              <strong>{packet.focus.title}</strong>
              <p>{packet.focus.summary}</p>
              {packet.focus.evidence.length ? <small>依据：{packet.focus.evidence.join("；")}</small> : null}
            </section>
          ) : null}
          {packet.recentActivity.length ? (
            <ContextPacketList title="最近事实">
              {packet.recentActivity.map((activity) => (
                <li key={`${activity.occurredAt}-${activity.summary}`}>
                  <time>{activity.occurredAt}</time>
                  <span>{activity.summary}</span>
                </li>
              ))}
            </ContextPacketList>
          ) : null}
          {packet.pendingActions.length ? (
            <ContextPacketList title="待你处理">
              {packet.pendingActions.map((action) => (
                <li key={`${action.title}-${action.reason}`}>
                  <strong>{action.title}</strong>
                  <span>{action.reason}</span>
                </li>
              ))}
            </ContextPacketList>
          ) : null}
          {packet.files.length ? (
            <ContextPacketList title="相关资料">
              {packet.files.map((file) => (
                <li key={`${file.name}-${file.location}`}>
                  <strong>{file.name}</strong>
                  <span>{[file.documentPurpose, file.lifecycleStatus, file.location].filter(Boolean).join(" · ")}</span>
                </li>
              ))}
            </ContextPacketList>
          ) : null}
          {packet.codexResult ? (
            <section className="project-context-section">
              <span className="section-label">最近 Codex 结果</span>
              <strong>{packet.codexResult.title}</strong>
              <p>{packet.codexResult.summary || packet.codexResult.status}</p>
              {packet.codexResult.manualAcceptance.length ? (
                <ul className="project-context-inline-list">
                  {packet.codexResult.manualAcceptance.map((item) => <li key={item}>待验收：{item}</li>)}
                </ul>
              ) : null}
            </section>
          ) : null}
          {packet.sparse ? <Feedback title="当前项目可用事实较少">感冒院不会据此虚构下一步。</Feedback> : null}
        </div>
      ) : null}
    </section>
  );
}

function ContextPacketList({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="project-context-section">
      <span className="section-label">{title}</span>
      <ul className="project-context-list">{children}</ul>
    </section>
  );
}

function ProjectFactCapturePanel({
  captureType,
  content,
  reason,
  pendingActions,
  highlightedActionSourceRef,
  isSaving,
  feedback,
  onCaptureTypeChange,
  onContentChange,
  onReasonChange,
  onSubmit,
  onResolve,
  onClose,
}: {
  captureType: ProjectFactCaptureRequest["captureType"];
  content: string;
  reason: string;
  pendingActions: PendingActionProjection[];
  highlightedActionSourceRef: string;
  isSaving: boolean;
  feedback: string;
  onCaptureTypeChange: (value: ProjectFactCaptureRequest["captureType"]) => void;
  onContentChange: (value: string) => void;
  onReasonChange: (value: string) => void;
  onSubmit: () => void;
  onResolve: (sourceRef: string) => void;
  onClose: () => void;
}) {
  const mode = captureMode(captureType);
  const capturedActions = pendingActions
    .filter((action) => ["userBlocker", "userNextAction"].includes(action.type))
    .sort((left, right) => Number(right.sourceRef === highlightedActionSourceRef) - Number(left.sourceRef === highlightedActionSourceRef));

  return (
    <section className="project-context-panel work-capture-panel" aria-labelledby="work-capture-title">
      <header className="project-context-head">
        <div>
          <span className="section-label">项目事实</span>
          <h2 id="work-capture-title">记录工作</h2>
          <p>只保存你明确确认的事实，不调用 AI，也不会自动创建任务。</p>
        </div>
        <div className="project-context-actions">
          <Button type="button" variant="ghost" onClick={onClose}>
            关闭
          </Button>
        </div>
      </header>

      <div className="work-capture-sheet">
        <div className="work-capture-types" role="radiogroup" aria-label="工作事实类型">
          {(["progress", "decision", "blocker", "nextAction"] as const).map((value) => {
            const option = captureMode(value);
            return (
              <button
                key={value}
                type="button"
                className={captureType === value ? "is-selected" : ""}
                role="radio"
                aria-checked={captureType === value}
                onClick={() => onCaptureTypeChange(value)}
              >
                <strong>{option.label}</strong>
                <span>{option.description}</span>
              </button>
            );
          })}
        </div>

        <label className="work-capture-field">
          <span>{mode.prompt}</span>
          <textarea
            rows={4}
            value={content}
            onChange={(event) => onContentChange(event.target.value)}
            placeholder={mode.placeholder}
          />
        </label>
        <label className="work-capture-field">
          <span>原因或补充说明（可选）</span>
          <textarea
            rows={2}
            value={reason}
            onChange={(event) => onReasonChange(event.target.value)}
            placeholder="只记录必要背景，不会发送给外部模型。"
          />
        </label>

        {captureType === "blocker" || captureType === "nextAction" ? (
          <Feedback title="这条记录会成为待处理事项">
            它会出现在 Continue Work 中，直到你明确标记为已处理。
          </Feedback>
        ) : null}
        {feedback ? <Feedback tone="success" title="已记录">{feedback}</Feedback> : null}
        <div className="work-capture-actions">
          <Button type="button" variant="primary" loading={isSaving} disabled={!content.trim()} onClick={onSubmit}>
            记录{mode.label}
          </Button>
        </div>

        {capturedActions.length ? (
          <section className="project-context-section work-capture-open-actions">
            <span className="section-label">你记录的待处理事项</span>
            <ul className="project-context-list">
              {capturedActions.map((action) => (
                <li key={action.id}>
                  <div>
                    <strong>{action.title}</strong>
                    <span>{action.reason}</span>
                  </div>
                  <Button type="button" size="sm" variant="ghost" disabled={isSaving} onClick={() => onResolve(action.sourceRef)}>
                    标记已处理
                  </Button>
                </li>
              ))}
            </ul>
          </section>
        ) : null}
      </div>
    </section>
  );
}

function captureMode(type: ProjectFactCaptureRequest["captureType"]) {
  switch (type) {
    case "decision":
      return {
        label: "记录决定",
        description: "正式采纳的项目选择",
        prompt: "这次决定了什么？",
        placeholder: "例如：暂不接入 ChatGPT MCP，先验证项目事实捕获闭环。",
      };
    case "blocker":
      return {
        label: "记录阻塞",
        description: "需要处理才能继续的事情",
        prompt: "当前卡在哪里？",
        placeholder: "例如：等待业务方确认测试范围后才能继续验收。",
      };
    case "nextAction":
      return {
        label: "添加下一步",
        description: "你已经明确要做的下一件事",
        prompt: "下一步要做什么？",
        placeholder: "例如：整理验收结果并交给业务方确认。",
      };
    default:
      return {
        label: "完成进展",
        description: "已经真实完成的工作",
        prompt: "这次已经完成了什么？",
        placeholder: "例如：完成 Workspace 整理闭环的人工验收并记录结果。",
      };
  }
}

function CodexCollaborationPanel({
  promptText,
  onPromptChange,
  onGeneratePrompt,
  onCopyPrompt,
  onRebindTaskPrompt,
  onStartTaskRun,
  onCancelTaskRun,
  onAcceptTask,
  onRejectTask,
  onSelectTask,
  onRefreshCodexStatus,
  reportInput,
  onReportInputChange,
  onImportReportText,
  onImportReportFile,
  onApplyReport,
  latestPrompt,
  latestReport,
  codexTasks,
  codexRuns,
  targetTaskId,
  repositoryPath,
  onRepositoryPathChange,
  onRefreshGit,
  isRefreshingGit,
  decisionText,
  onDecisionTextChange,
  decisionReason,
  onDecisionReasonChange,
  onRecordDecision,
  isRecordingDecision,
  workLedger,
  isGeneratingPrompt,
  isImportingReport,
  isApplyingReport,
  isUpdatingTask,
  isStartingRun,
  isCancellingRun,
}: {
  promptText: string;
  onPromptChange: (value: string) => void;
  onGeneratePrompt: () => Promise<void>;
  onCopyPrompt: () => Promise<void>;
  onRebindTaskPrompt: (taskId: string) => Promise<void>;
  onStartTaskRun: (taskId: string) => Promise<void>;
  onCancelTaskRun: (taskId: string) => Promise<void>;
  onAcceptTask: (taskId: string, resultId: string) => Promise<void>;
  onRejectTask: (taskId: string, resultId: string) => Promise<void>;
  onSelectTask: (taskId: string) => void;
  onRefreshCodexStatus: () => Promise<void>;
  reportInput: string;
  onReportInputChange: (value: string) => void;
  onImportReportText: () => Promise<void>;
  onImportReportFile: () => Promise<void>;
  onApplyReport: (reportId: string) => Promise<void>;
  latestPrompt: CodexPromptRecord | null;
  latestReport: CodexReportRecord | null;
  codexTasks: CodexTask[];
  codexRuns: CodexRun[];
  targetTaskId: string;
  repositoryPath: string;
  onRepositoryPathChange: (value: string) => void;
  onRefreshGit: () => Promise<void>;
  isRefreshingGit: boolean;
  decisionText: string;
  onDecisionTextChange: (value: string) => void;
  decisionReason: string;
  onDecisionReasonChange: (value: string) => void;
  onRecordDecision: () => Promise<void>;
  isRecordingDecision: boolean;
  workLedger: WorkLedgerSnapshot | null;
  isGeneratingPrompt: boolean;
  isImportingReport: boolean;
  isApplyingReport: boolean;
  isUpdatingTask: boolean;
  isStartingRun: boolean;
  isCancellingRun: boolean;
}) {
  const currentTask = selectCurrentCodexTask(codexTasks, codexRuns, targetTaskId);
  const currentRun = currentTask ? latestRunForTask(codexRuns, currentTask.taskId) : null;
  const view = buildCodexTaskView(currentTask, currentRun);
  const currentTaskType = currentTask ? resolveCodexTaskDisplayType(currentTask) : "analysis";
  return (
    <section className="codex-panel codex-console" id="codex-console-top">
      <div className="codex-console-head">
        <div>
          <span className="section-label">Codex 协同</span>
          <strong>Codex 任务控制台</strong>
          <p className="codex-panel-hint">先看当前任务，再看进度和是否需要你操作。技术证据已收进详情里。</p>
        </div>
        <Button type="button" variant="ghost" disabled={isGeneratingPrompt} loading={isGeneratingPrompt} onClick={() => void onGeneratePrompt()}>
          生成新任务
        </Button>
      </div>

      {currentTask ? (
        <section className="codex-command-strip" aria-label="当前 Codex 任务">
          <div className="codex-current-task-main">
            <span className="section-label">当前任务</span>
            <div className="codex-current-title-line">
              <strong>{codexDisplayTaskTitle(currentTask)}</strong>
              <Badge tone="neutral">{codexTaskTypeText(currentTaskType)}</Badge>
            </div>
            <p>{codexDisplayTaskDescription(currentTask, view.description)}</p>
          </div>
          <div className="codex-current-task-side">
            <div className="codex-status-stack">
              <Badge tone={codexBadgeTone(view.tone)}>{view.statusLabel}</Badge>
            </div>
            <div className="codex-primary-actions">
              {view.primaryAction === "start" ? (
                <Button
                  type="button"
                  variant="primary"
                  disabled={!currentTask.prompt.trim() || isStartingRun}
                  loading={isStartingRun}
                  onClick={() => void onStartTaskRun(currentTask.taskId)}
                >
                  启动 Codex
                </Button>
              ) : null}
              {view.primaryAction === "stop" ? (
                <Button
                  type="button"
                  variant="danger"
                  disabled={isCancellingRun}
                  loading={isCancellingRun}
                  onClick={() => void onCancelTaskRun(currentTask.taskId)}
                >
                  停止执行
                </Button>
              ) : null}
              {view.primaryAction === "retry" ? (
                <Button
                  type="button"
                  variant="primary"
                  disabled={!currentTask.prompt.trim() || isStartingRun}
                  loading={isStartingRun}
                  onClick={() => void onStartTaskRun(currentTask.taskId)}
                >
                  重新执行
                </Button>
              ) : null}
              {view.primaryAction === "accept" ? (
                <Button type="button" variant="primary" disabled={isUpdatingTask} loading={isUpdatingTask} onClick={() => void onAcceptTask(currentTask.taskId, currentTask.resultId)}>
                  验收通过
                </Button>
              ) : null}
              {view.primaryAction === "checkResult" ? (
                <Button type="button" variant="secondary" disabled={isImportingReport} loading={isImportingReport} onClick={() => void onRefreshCodexStatus()}>
                  重新检查结果
                </Button>
              ) : null}
              {view.secondaryActions.includes("reject") ? (
                <Button type="button" variant="danger" disabled={isUpdatingTask} onClick={() => void onRejectTask(currentTask.taskId, currentTask.resultId)}>
                  验收失败
                </Button>
              ) : null}
              {view.secondaryActions.includes("rebindPrompt") ? (
                <Button
                  type="button"
                  variant="secondary"
                  disabled={isUpdatingTask}
                  onClick={() => void onRebindTaskPrompt(currentTask.taskId)}
                >
                  重新绑定提示词
                </Button>
              ) : null}
              {view.primaryAction === "review" ? (
                <Button
                  type="button"
                  variant="primary"
                  onClick={() => document.getElementById("codex-result-section")?.scrollIntoView({ block: "start" })}
                >
                  查看问题
                </Button>
              ) : null}
              {view.secondaryActions.includes("copyPrompt") ? (
                <Button type="button" variant="ghost" disabled={!promptText.trim()} onClick={() => void onCopyPrompt()}>
                  复制提示词
                </Button>
              ) : null}
            </div>
            {!currentTask.prompt.trim() && view.primaryAction === "start" ? (
              <small className="codex-action-hint">提示词未生成，暂时不能启动。</small>
            ) : null}
          </div>
        </section>
      ) : (
        <section className="codex-command-strip codex-empty-task" aria-label="当前 Codex 任务">
          <div className="codex-current-task-main">
            <span className="section-label">当前任务</span>
            <strong>还没有 Codex 任务</strong>
            <p>先生成一个任务，感冒院会自动带上结果回流位置和项目上下文。</p>
          </div>
          <Button type="button" variant="primary" disabled={isGeneratingPrompt} loading={isGeneratingPrompt} onClick={() => void onGeneratePrompt()}>
            生成 Codex 任务
          </Button>
        </section>
      )}

      <div className="codex-progress-panel">
        <div className="codex-section-title">
          <span className="section-label">执行进度</span>
          <strong>{view.progressTitle}</strong>
        </div>
        <div className="codex-steps" aria-label="Codex 任务生命周期">
          {codexTaskSteps(currentTask, currentRun).map((step) => (
            <div key={step.key} className={`codex-step ${step.state}`}>
              <span aria-hidden="true">{step.state === "done" ? "✓" : ""}</span>
              <em>{step.label}</em>
              {step.note ? <small>{step.note}</small> : null}
            </div>
          ))}
        </div>
      </div>

      <div className="codex-result-panel" id="codex-result-section">
        <div className="codex-section-title">
          <span className="section-label">结果 / 待验收</span>
          <strong>{currentTask ? codexResultTitle(currentTask) : "等待任务生成"}</strong>
        </div>
        {currentTask ? (
          <CodexTaskResultSummary
            task={currentTask}
            onAcceptTask={onAcceptTask}
            onRejectTask={onRejectTask}
            isUpdatingTask={isUpdatingTask}
          />
        ) : (
          <Feedback>生成任务后，Codex 结果会在这里显示。</Feedback>
        )}
      </div>

      {codexTasks.length ? (
        <div className="codex-history-panel">
          <div className="codex-section-title">
            <span className="section-label">最近 Codex 任务</span>
            <strong>{codexTasks.length} 个任务</strong>
          </div>
          {codexTasks.slice(0, 8).map((task) => (
            <details key={task.taskId} className="codex-task-row codex-history-row">
              <summary onClick={() => onSelectTask(task.taskId)}>
                <span>{codexDisplayTaskTitle(task)}</span>
                <small>{codexTaskTypeText(resolveCodexTaskDisplayType(task))}</small>
                <Badge tone={codexTaskStatusTone(task.status)}>{codexTaskStatusText(task.status)}</Badge>
                <small>{codexHistoryGitSummary(task)}</small>
                <small>{task.manualAcceptance?.length ? `${task.manualAcceptance.length} 项待验收` : "无需人工项"}</small>
              </summary>
              <p>taskId：{task.taskId}</p>
              <p>结果路径：{task.expectedResultPath || "未记录"}</p>
              <p>Git：{codexGitStatusText(task.gitVerification?.status)} {task.gitVerification?.reason || ""}</p>
              <InsightBlock title="测试" items={task.tests} empty="未记录测试。" />
              <InsightBlock title="遗留问题" items={task.remainingIssues} empty="未记录遗留问题。" />
              <InsightBlock title="证据" items={task.evidenceRefs} empty="未记录证据。" />
            </details>
          ))}
        </div>
      ) : null}

      <details className="codex-advanced-details">
        <summary>查看技术详情</summary>
        <div className="codex-advanced-grid">
          {currentTask ? (
            <div className="codex-technical-block">
              <strong>任务技术信息</strong>
              <p>任务类型：{codexTaskTypeText(currentTaskType)}</p>
              <p>taskId：{currentTask.taskId}</p>
              <p>结果路径：{currentTask.expectedResultPath || "未记录"}</p>
              <p>仓库：{currentTask.repositoryPath || "未记录"}</p>
              <p>
                Git：{codexGitStatusText(currentTask.gitVerification?.status)}
                {currentTask.gitVerification?.reason ? ` · ${currentTask.gitVerification.reason}` : ""}
              </p>
              {currentRun ? (
                <>
                  <p>
                    本机 Codex：{codexRunStatusText(currentRun.status)}
                    {currentRun.pid ? ` · pid ${currentRun.pid}` : ""}
                    {currentRun.exitCode !== null && currentRun.exitCode !== undefined ? ` · exit ${currentRun.exitCode}` : ""}
                  </p>
                  {currentRun.error ? <p>失败原因：{currentRun.error}</p> : null}
                  {currentRun.stderrSummary ? (
                    <details className="codex-task-row">
                      <summary>stderr</summary>
                      <pre>{currentRun.stderrSummary}</pre>
                    </details>
                  ) : null}
                </>
              ) : null}
            </div>
          ) : null}

          <div className="codex-technical-block">
            <div className="codex-technical-head">
              <strong>Codex 提示词</strong>
              <Button type="button" variant="secondary" disabled={!promptText.trim()} onClick={() => void onCopyPrompt()}>
                复制提示词
              </Button>
            </div>
            <p className="inline-notice">{promptText.trim() ? "已生成 Codex 任务提示词。" : "还没有生成提示词。"}</p>
            <details className="codex-task-row">
              <summary>查看提示词</summary>
              <textarea rows={10} value={promptText} onChange={(event) => onPromptChange(event.target.value)} />
            </details>
            {latestPrompt ? (
              <p className="inline-notice">归档位置：{latestPrompt.managedPath}</p>
            ) : null}
          </div>

          {latestReport ? (
            <div className="codex-technical-block">
              <CodexReportSummary report={latestReport} onApplyReport={onApplyReport} isApplyingReport={isApplyingReport} />
            </div>
          ) : null}

          <div className="codex-technical-block">
            <strong>手工导入报告</strong>
            <label className="codex-field">
              <span>回复文本</span>
              <textarea
                rows={6}
                value={reportInput}
                onChange={(event) => onReportInputChange(event.target.value)}
                placeholder="粘贴包含完成状态、修改内容、测试结果、commit 和遗留问题的回复文本。"
              />
            </label>
            <div className="file-result-actions">
              <Button type="button" variant="secondary" disabled={!reportInput.trim() || isImportingReport} loading={isImportingReport} onClick={() => void onImportReportText()}>
                导入文本
              </Button>
              <Button type="button" variant="secondary" disabled={isImportingReport} onClick={() => void onImportReportFile()}>
                导入 txt/md 报告
              </Button>
            </div>
          </div>

          <WorkLedgerPanel
            repositoryPath={repositoryPath}
            onRepositoryPathChange={onRepositoryPathChange}
            onRefreshGit={onRefreshGit}
            isRefreshingGit={isRefreshingGit}
            decisionText={decisionText}
            onDecisionTextChange={onDecisionTextChange}
            decisionReason={decisionReason}
            onDecisionReasonChange={onDecisionReasonChange}
            onRecordDecision={onRecordDecision}
            isRecordingDecision={isRecordingDecision}
            workLedger={workLedger}
          />
        </div>
      </details>
    </section>
  );
}

function CodexTaskResultSummary({
  task,
  onAcceptTask,
  onRejectTask,
  isUpdatingTask,
}: {
  task: CodexTask;
  onAcceptTask: (taskId: string, resultId: string) => Promise<void>;
  onRejectTask: (taskId: string, resultId: string) => Promise<void>;
  isUpdatingTask: boolean;
}) {
  if (!task.resultId && !["awaitingAcceptance", "completed", "resultReceived", "needsReview"].includes(task.status)) {
    return <p className="inline-notice">结果尚未回流。Codex 写入结果文件后，这里会自动更新。</p>;
  }
  const type = resolveCodexTaskDisplayType(task);
  const resultTitle = codexTaskResultBlockTitle(type);
  const resultGroups = codexTaskResultGroups(task);

  return (
    <div className="codex-result-summary">
      <p className="codex-result-lede">
        <strong>{resultTitle}：</strong>
        {codexUserResultSummary(task)}
      </p>
      <div className="codex-result-section">
        <span className="section-label">关键结果</span>
        {resultGroups.map((group) => (
          <InsightBlock key={group.title} title={group.title} items={group.items} empty={group.empty} defaultOpen={group.defaultOpen} />
        ))}
        {type === "coding" ? (
          <div className="codex-result-facts">
            <span>
              <strong>代码提交：</strong>
              {codexShortCommit(task.verifiedCommits?.[0] || task.reportedCommits?.[0]) || "本次未记录代码提交"}
            </span>
            <span>
              <strong>Git：</strong>
              {codexPublicGitStatusText(task.gitVerification?.status)}
            </span>
          </div>
        ) : null}
        {type !== "analysis" && type !== "coding" && task.gitVerification?.status ? (
          <div className="codex-result-facts">
            <span>
              <strong>Git：</strong>
              {codexGitStatusText(task.gitVerification.status)}
            </span>
          </div>
        ) : null}
      </div>
      <div className="codex-result-section codex-result-followups">
        <span className="section-label">待处理</span>
        <p>
          <strong>遗留问题：</strong>
          {codexPublicItems(task.remainingIssues).length ? codexPublicItems(task.remainingIssues).slice(0, 3).join("；") : "无"}
        </p>
      </div>
      {task.manualAcceptance.length ? (
        <div className="codex-acceptance-list">
          <strong>待人工验收</strong>
          {task.manualAcceptance.map((item, index) => (
            <span key={`${item}-${index}`}>{item}</span>
          ))}
          {task.status === "awaitingAcceptance" ? (
            <div className="file-result-actions">
              <Button
                type="button"
                variant="primary"
                disabled={isUpdatingTask}
                loading={isUpdatingTask}
                onClick={() => void onAcceptTask(task.taskId, task.resultId)}
              >
                验收通过
              </Button>
              <Button
                type="button"
                variant="danger"
                disabled={isUpdatingTask}
                onClick={() => void onRejectTask(task.taskId, task.resultId)}
              >
                验收失败
              </Button>
            </div>
          ) : null}
        </div>
      ) : null}
    </div>
  );
}

function CodexReportSummary({
  report,
  onApplyReport,
  isApplyingReport,
}: {
  report: CodexReportRecord;
  onApplyReport: (reportId: string) => Promise<void>;
  isApplyingReport: boolean;
}) {
  const reviewRequired = report.parseStatus !== "ready" || report.completedStatus === "uncertain";
  return (
    <div className="codex-report-summary">
      <div className="message-meta">
        <strong>最新 Codex 报告</strong>
        <span>{report.sourceLabel}</span>
      </div>
      <p>完成状态: {report.completedStatus || "uncertain"}</p>
      <p>解析状态: {report.parseStatus}</p>
      <p>摘要: {report.summary || "未提取到明确摘要。"}</p>
      <InsightBlock title="修改内容" items={report.modifiedContent} empty="未提取到修改内容。" />
      <InsightBlock title="测试和构建结果" items={report.testResults} empty="未提取到测试结果。" />
      <InsightBlock title="遗留问题" items={report.unresolvedIssues} empty="未提取到遗留问题。" />
      <InsightBlock title="下一步建议" items={report.nextStepSuggestions} empty="未提取到下一步建议。" />
      <p>commit: {report.commit || "未提取到 commit"}</p>
      <p>证据位置: {report.evidenceManagedPath || "未归档"}</p>
      {report.applied ? <p>已标记为已检查。</p> : null}
      {reviewRequired ? (
        <p>这份报告仍需人工检查，已进入待检查事项，不会自动更新项目状态。</p>
      ) : (
        <button
          type="button"
          className="btn btn-primary"
          disabled={report.applied || isApplyingReport}
          onClick={() => void onApplyReport(report.id)}
        >
          {report.applied ? "已检查" : isApplyingReport ? "处理中..." : "标记报告已检查"}
        </button>
      )}
    </div>
  );
}

export function codexReportTaskId(
  report: Pick<CodexReportRecord, "summary" | "sourceLabel" | "evidenceManagedPath">,
) {
  return (
    extractTaskIdFromText(report.summary) ||
    extractTaskIdFromResultPath(report.sourceLabel) ||
    extractTaskIdFromResultPath(report.evidenceManagedPath)
  );
}

function extractTaskIdFromText(text: string) {
  for (const line of text.split(/\r?\n/)) {
    if (!line.includes("taskId") && !line.includes("任务ID")) continue;
    const [, value = ""] = line.split(/[:：]/, 2);
    if (value.trim()) return value.trim().slice(0, 120);
  }
  return "";
}

function extractTaskIdFromResultPath(text: string) {
  const fileName = text.split(/[\\/]/).pop() || text;
  const match = fileName.match(/^(.+)-result\.(json|md)$/i);
  return match?.[1] ?? "";
}

function WorkLedgerPanel({
  repositoryPath,
  onRepositoryPathChange,
  onRefreshGit,
  isRefreshingGit,
  decisionText,
  onDecisionTextChange,
  decisionReason,
  onDecisionReasonChange,
  onRecordDecision,
  isRecordingDecision,
  workLedger,
}: {
  repositoryPath: string;
  onRepositoryPathChange: (value: string) => void;
  onRefreshGit: () => Promise<void>;
  isRefreshingGit: boolean;
  decisionText: string;
  onDecisionTextChange: (value: string) => void;
  decisionReason: string;
  onDecisionReasonChange: (value: string) => void;
  onRecordDecision: () => Promise<void>;
  isRecordingDecision: boolean;
  workLedger: WorkLedgerSnapshot | null;
}) {
  const snapshot = workLedger?.gitSnapshot ?? null;
  const activityTimeline = workLedger?.activityTimeline ?? [];
  const legacyActivities = (workLedger?.events ?? [])
    .filter((event) => legacyUserFacingWorkEvent(event.eventType))
    .slice(-6)
    .reverse()
    .map((event) => ({ id: event.id, summary: event.summary, confidence: event.confidence }));
  const workActivities = activityTimeline.length ? activityTimeline.slice(0, 6) : legacyActivities;
  const visibleActivityIds = new Set(
    activityTimeline.length ? activityTimeline.map((activity) => activity.id) : legacyActivities.map((activity) => activity.id),
  );
  const technicalEvents = workLedger?.events.filter((event) => !visibleActivityIds.has(event.id)).slice(-6).reverse() ?? [];
  const latestCodexResults = workLedger?.codexResults.slice(-3).reverse() ?? [];
  return (
    <section className="codex-panel work-ledger-panel">
      <div className="codex-panel-head">
        <div>
          <span className="section-label">真实事实</span>
          <strong>Git 状态、Codex 回流和用户决定</strong>
          <p className="codex-panel-hint">Phase A：让感冒院先掌握真实工作事实，再基于事实生成 Today 和项目状态。</p>
        </div>
      </div>

      <label className="codex-field">
        <span>关联 Git 仓库路径</span>
        <input value={repositoryPath} onChange={(event) => onRepositoryPathChange(event.target.value)} />
      </label>
      <div className="file-result-actions">
        <button type="button" className="btn" disabled={isRefreshingGit} onClick={() => void onRefreshGit()}>
          {isRefreshingGit ? "读取中..." : "刷新 Git 事实"}
        </button>
      </div>

      {snapshot ? (
        <div className="codex-report-summary">
          <p>
            Git：{snapshot.branch || "未知分支"} / {snapshot.headShort || "无 HEAD"} /{" "}
            {snapshot.isDirty ? `有 ${snapshot.changedFiles.length} 个未提交改动` : "工作区干净"}
          </p>
          {snapshot.recentCommits[0] ? <p>最近提交：{snapshot.recentCommits[0].shortHash} · {snapshot.recentCommits[0].subject}</p> : null}
          {snapshot.changedFiles.length ? (
            <InsightBlock
              title="未提交改动"
              items={snapshot.changedFiles.slice(0, 8).map((file) => `${file.status} ${file.path}`)}
              empty="暂无未提交改动。"
            />
          ) : null}
        </div>
      ) : null}

      {latestCodexResults.length ? (
        <div className="codex-report-summary">
          <strong>Codex 回流</strong>
          {latestCodexResults.map((result) => (
            <CodexExternalResultSummary key={result.id} result={result} />
          ))}
        </div>
      ) : null}

      <label className="codex-field">
        <span>记录决定</span>
        <input value={decisionText} onChange={(event) => onDecisionTextChange(event.target.value)} placeholder="例如：暂不做 ChatGPT MCP" />
      </label>
      <label className="codex-field">
        <span>原因（可选）</span>
        <input value={decisionReason} onChange={(event) => onDecisionReasonChange(event.target.value)} placeholder="说明这条决定的依据" />
      </label>
      <div className="file-result-actions">
        <button type="button" className="btn" disabled={!decisionText.trim() || isRecordingDecision} onClick={() => void onRecordDecision()}>
          {isRecordingDecision ? "记录中..." : "记录决定"}
        </button>
      </div>

      {workActivities.length ? (
        <div className="codex-report-summary">
          <strong>工作事实</strong>
          {workActivities.map((activity) => (
            <p key={activity.id}>
              {activity.summary}
              {activity.confidence?.display ? ` · ${activity.confidence.display}` : ""}
            </p>
          ))}
          {technicalEvents.length ? (
            <details className="codex-task-row">
              <summary>
                <span>技术事实</span>
                <em>{technicalEvents.length} 条</em>
                <small>默认隐藏</small>
                <small>可追溯</small>
              </summary>
              {technicalEvents.map((event) => (
                <p key={event.id}>{event.summary}</p>
              ))}
            </details>
          ) : null}
        </div>
      ) : (
        <div className="codex-report-summary">
          <strong>还没有事实记录</strong>
          <p>先点“刷新 Git 事实”读取当前仓库状态；导入 Codex 回复或记录决定后，这里会显示可追溯的工作事实。</p>
        </div>
      )}
    </section>
  );
}

function legacyUserFacingWorkEvent(eventType: string) {
  return [
    "git.headChanged",
    "codex.taskAccepted",
    "codex.taskRejected",
    "codex.reportAccepted",
    "user.decisionRecorded",
  ].includes(eventType);
}

function CodexExternalResultSummary({ result }: { result: CodexExternalResult }) {
  const statusText = result.status || "未提取状态";
  const commitText = result.commits.length ? result.commits.slice(0, 2).join("、") : "未提取 commit";
  const testText = result.tests.length ? result.tests.slice(0, 2).join("；") : "未提取测试结果";
  const pendingText = result.manualAcceptance.length || result.unresolvedItems.length
    ? [...result.manualAcceptance, ...result.unresolvedItems].slice(0, 2).join("；")
    : "暂无待人工验收项";

  return (
    <div className="work-ledger-result">
      <p>
        <strong>{statusText}</strong>
        <span>{result.sourceLabel || "Codex 回复"}</span>
      </p>
      <p>commit：{commitText}</p>
      <p>测试：{testText}</p>
      <p>待处理：{pendingText}</p>
    </div>
  );
}

type CodexPrimaryAction = "start" | "stop" | "retry" | "accept" | "checkResult" | "review" | "none";
type CodexSecondaryAction = "copyPrompt" | "reject" | "rebindPrompt";
type CodexStepState = "done" | "current" | "pending";
type CodexTaskType = "analysis" | "coding" | "verification" | "fileOperation";
type CodexBadgeTone = "success" | "warning" | "error" | "pending" | "high" | "medium" | "low" | "neutral";

const CODEX_TECHNICAL_TERMS = [
  "taskId",
  "resultPath",
  "expectedResultPath",
  "runnerFallback",
  "Result Bridge",
  "Codex CLI",
  "Git Observer",
  "fallback",
  "stderr",
  "stdout",
  "output-last-message",
  "pid",
  "exitCode",
];

const CODEX_UNNAMED_TASK_TITLE = "未命名 Codex 任务";

export function codexDisplayTaskTitle(
  task: Pick<CodexTask, "title" | "prompt" | "taskType"> & Partial<Pick<CodexTask, "taskId" | "resultId" | "summary" | "resultText">>,
) {
  const candidates = [task.title, firstMeaningfulPromptLine(task.prompt), task.summary, task.resultText];
  for (const candidate of candidates) {
    const source = normalizeWhitespace(candidate);
    if (!source || isUnsafeCodexDisplayTitle(source, task)) continue;
    const semantic = codexSemanticTaskTitle(source);
    if (!semantic || isUnsafeCodexDisplayTitle(semantic, task)) continue;
    return truncateDisplayText(semantic, 35);
  }
  return CODEX_UNNAMED_TASK_TITLE;
}

function codexDisplayTaskDescription(task: Pick<CodexTask, "summary" | "resultText">, fallback: string) {
  const source = normalizeWhitespace(task.summary || task.resultText || fallback);
  const publicText = sanitizeCodexPublicText(source) || fallback;
  return truncateDisplayText(publicText, 86);
}

export function codexUserResultSummary(
  task: Pick<CodexTask, "summary" | "resultText" | "taskType" | "changedFiles" | "reportedCommits" | "verifiedCommits"> &
    Partial<Pick<CodexTask, "title" | "prompt" | "tests">>,
) {
  const source = sanitizeCodexPublicText(normalizeWhitespace(task.summary || task.resultText));
  if (source) return truncateDisplayText(source, 150);
  if (resolveCodexTaskDisplayType(task) === "coding" && !task.changedFiles.length && !task.reportedCommits.length && !task.verifiedCommits.length) {
    return "Codex 已完成本次任务，不过还没有形成可确认的代码提交或结构化结果，因此需要检查实际执行内容。";
  }
  return "结果已回流，暂无摘要。";
}

function codexUserFacingRunError(error?: string) {
  if (!error) return "";
  const text = error.toLowerCase();
  if (text.includes("not found") || text.includes("找不到") || text.includes("无法将") || text.includes("not recognized")) {
    return "没有找到可用的本机 Codex 命令，请检查 Codex 安装。";
  }
  if (text.includes("login") || text.includes("auth") || text.includes("unauthorized")) {
    return "Codex 认证没有通过，请检查本机 Codex 登录状态。";
  }
  if (text.includes("unexpected argument") || text.includes("unknown option") || text.includes("invalid")) {
    return "Codex 启动参数不兼容，需要检查本机 CLI 版本。";
  }
  return "Codex 执行失败，可以查看技术详情或重新执行。";
}

function sanitizeCodexPublicText(value: string) {
  const text = normalizeWhitespace(value);
  if (!text) return "";
  if (CODEX_TECHNICAL_TERMS.some((term) => text.toLowerCase().includes(term.toLowerCase()))) return "";
  if (isTechnicalIdentifier(text)) return "";
  return text.replace(/\b[0-9a-f]{40}\b/gi, (match) => codexShortCommit(match) || match);
}

function codexPublicItems(items: string[]) {
  return items.map((item) => sanitizeCodexPublicText(item)).filter(Boolean);
}

function codexSemanticTaskTitle(source: string) {
  if (/项目理解|下一步建议|项目分析/.test(source)) return "生成项目理解与下一步建议";
  if (/result bridge|结果回流|回流/i.test(source)) return "验证 Codex 结果回流";
  if (/runner|cli|本机 codex|启动/i.test(source)) return "验证本机 Codex 执行";
  if (/修复|fix/i.test(source)) return firstClause(source);
  if (/生成|整理|导入|核验|验证|实现/.test(source)) return firstClause(source);
  return firstClause(source);
}

function isUnsafeCodexDisplayTitle(
  value: string,
  task?: Partial<Pick<CodexTask, "taskId" | "resultId">>,
) {
  const text = normalizeWhitespace(value);
  if (!text) return true;
  if (task?.taskId && text === task.taskId) return true;
  if (task?.resultId && text === task.resultId) return true;
  if (/^(taskId|runId|resultId)\s*[:：]/i.test(text)) return true;
  if (isTechnicalIdentifier(text)) return true;
  if (/^[a-zA-Z]:[\\/]/.test(text) || /^\\\\/.test(text)) return true;
  if (/[\\/].+/.test(text)) return true;
  if (/-result\.(json|md)$/i.test(text)) return true;
  return false;
}

function isTechnicalIdentifier(value: string) {
  const text = normalizeWhitespace(value);
  return (
    /^[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i.test(text) ||
    /^[0-9a-f]{16,40}$/i.test(text)
  );
}

function firstMeaningfulPromptLine(prompt?: string) {
  if (!prompt) return "";
  return (
    prompt
      .split(/\r?\n/)
      .map((line) => line.trim())
      .find((line) => line && !/^taskId[:：]/i.test(line) && !/^result/i.test(line)) || ""
  );
}

function firstClause(value: string) {
  return normalizeWhitespace(value).split(/[。；;，,：:\n\r]/)[0] || value;
}

function normalizeWhitespace(value?: string) {
  return (value || "").replace(/\s+/g, " ").trim();
}

function truncateDisplayText(value: string, maxLength: number) {
  const chars = [...value];
  return chars.length > maxLength ? `${chars.slice(0, maxLength).join("")}...` : value;
}

export function codexShortCommit(commit?: string) {
  if (!commit) return "";
  const normalized = commit.trim();
  const hash = normalized.match(/[0-9a-f]{7,40}/i)?.[0];
  return hash ? hash.slice(0, 7) : truncateDisplayText(normalized, 16);
}

function codexPublicGitStatusText(status?: string) {
  if (status === "unavailable") return "暂时无法确认代码变更";
  return codexGitStatusText(status);
}

export function buildCodexTaskView(
  task: Pick<CodexTask, "status" | "summary" | "manualAcceptance" | "gitVerification"> | null,
  run: Pick<CodexRun, "status" | "exitCode" | "error"> | null,
) {
  if (!task) {
    return {
      statusLabel: "未生成",
      description: "还没有 Codex 任务。先生成任务提示词，再决定是否启动本机 Codex。",
      progressTitle: "等待创建任务",
      primaryAction: "none" as CodexPrimaryAction,
      secondaryActions: [] as CodexSecondaryAction[],
      tone: "pending",
    };
  }

  const hasTaskOutcome = ["resultReceived", "verifying", "awaitingAcceptance", "completed", "needsReview", "failed", "cancelled"].includes(task.status);
  if (!hasTaskOutcome && (run?.status === "running" || run?.status === "starting")) {
    return {
      statusLabel: "Codex 执行中",
      description: "本机 Codex 正在执行这项任务。现在只需要等待，必要时可以停止执行。",
      progressTitle: "Codex 正在执行",
      primaryAction: "stop" as CodexPrimaryAction,
      secondaryActions: [] as CodexSecondaryAction[],
      tone: "info",
    };
  }

  if (run?.status === "exited" && run.exitCode === 0 && ["running", "awaitingResult", "handedOff"].includes(task.status)) {
    return {
      statusLabel: "等待结果回流",
      description: "Codex 已执行完成，正在整理结果。现在还不能把任务判定为完成。",
      progressTitle: "等待结果整理",
      primaryAction: "checkResult" as CodexPrimaryAction,
      secondaryActions: [] as CodexSecondaryAction[],
      tone: "pending",
    };
  }

  if (task.status === "needsReview") {
    return {
      statusLabel: "需要检查",
      description: "结果已经返回，但还需要确认它是否能作为本次任务的有效结果。",
      progressTitle: "等待人工检查",
      primaryAction: "review" as CodexPrimaryAction,
      secondaryActions: [] as CodexSecondaryAction[],
      tone: "warning",
    };
  }

  if (task.gitVerification?.status === "mismatch") {
    return {
      statusLabel: "结果与 Git 不一致",
      description: "报告内容和代码记录不一致，需要先重新检查结果。",
      progressTitle: "Git 核验需要处理",
      primaryAction: "checkResult" as CodexPrimaryAction,
      secondaryActions: [] as CodexSecondaryAction[],
      tone: "warning",
    };
  }

  if (task.status === "failed" || (!hasTaskOutcome && run?.status === "failed")) {
    const needsRebind = /taskId|提示词|result/i.test(`${run?.error || ""} ${task.summary || ""}`);
    return {
      statusLabel: "执行失败",
      description: codexUserFacingRunError(run?.error) || "Codex 执行没有通过。可以在保留同一任务的前提下重新执行。",
      progressTitle: "等待重新执行",
      primaryAction: "retry" as CodexPrimaryAction,
      secondaryActions: needsRebind ? (["rebindPrompt"] as CodexSecondaryAction[]) : [],
      tone: "danger",
    };
  }

  if (task.status === "awaitingAcceptance") {
    return {
      statusLabel: "等待验收",
      description: "结果已经返回，现在需要你确认人工验收项。",
      progressTitle: "等待人工验收",
      primaryAction: "accept" as CodexPrimaryAction,
      secondaryActions: ["reject"] as CodexSecondaryAction[],
      tone: "warning",
    };
  }

  if (task.status === "completed") {
    return {
      statusLabel: "已完成",
      description: task.summary || "这项 Codex 任务已经完成并入账。",
      progressTitle: "任务已完成",
      primaryAction: "none" as CodexPrimaryAction,
      secondaryActions: [] as CodexSecondaryAction[],
      tone: "success",
    };
  }

  if (task.status === "cancelled" || run?.status === "cancelled") {
    return {
      statusLabel: "已停止",
      description: "本次 Codex 执行已停止。可能仍有未提交修改，需要检查代码记录后再决定是否重新执行。",
      progressTitle: "执行已停止",
      primaryAction: "retry" as CodexPrimaryAction,
      secondaryActions: [] as CodexSecondaryAction[],
      tone: "warning",
    };
  }

  if (task.status === "resultReceived" || task.status === "verifying") {
    return {
      statusLabel: task.status === "verifying" ? "正在核验" : "结果已回流",
      description: task.manualAcceptance?.length
        ? "结果已经回流，下一步会进入人工验收。"
        : "结果已经回流，正在确认它是否可以作为本次任务的结果。",
      progressTitle: task.status === "verifying" ? "正在核验证据" : "结果已经回流",
      primaryAction: "none" as CodexPrimaryAction,
      secondaryActions: [] as CodexSecondaryAction[],
      tone: "info",
    };
  }

  if (task.status === "awaitingResult") {
    return {
      statusLabel: "等待结果回流",
      description: "任务已经交给 Codex，正在等待结果整理完成。",
      progressTitle: "等待结果整理",
      primaryAction: "checkResult" as CodexPrimaryAction,
      secondaryActions: [] as CodexSecondaryAction[],
      tone: "pending",
    };
  }

  return {
    statusLabel: "待启动",
    description: "任务已准备好，可以启动本机 Codex 执行；复制提示词作为备用方式。",
    progressTitle: "任务已创建",
    primaryAction: "start" as CodexPrimaryAction,
    secondaryActions: ["copyPrompt"] as CodexSecondaryAction[],
    tone: "pending",
  };
}

export function codexTaskSteps(task: CodexTask | null, run: CodexRun | null) {
  const status = task?.status ?? "";
  const runStarted = Boolean(run);
  const runFinished = Boolean(run && ["exited", "failed", "cancelled", "unknownAfterRestart"].includes(run.status));
  const resultReceived = ["resultReceived", "verifying", "awaitingAcceptance", "completed", "needsReview"].includes(status);
  const gitNotApplicable = task?.gitVerification?.status === "notApplicable" || resolveCodexTaskDisplayType(task) === "analysis";
  const gitDone =
    ["awaitingAcceptance", "completed"].includes(status) ||
    ["verified", "notApplicable"].includes(task?.gitVerification?.status ?? "");
  const acceptanceDone = status === "completed" || task?.acceptance?.status === "approved";
  const needsAcceptance = Boolean(task?.manualAcceptance?.length);

  const steps = [
    { key: "created", label: "任务已创建", done: Boolean(task), current: !task },
    { key: "run", label: "Codex 执行", done: runFinished || resultReceived, current: runStarted && !runFinished },
    { key: "result", label: "结果回流", done: resultReceived, current: Boolean(task) && !resultReceived && runFinished },
    { key: "git", label: "Git 核验", note: gitNotApplicable ? "不适用" : "", done: gitDone, current: resultReceived && !gitDone },
    {
      key: "acceptance",
      label: "人工验收",
      done: acceptanceDone || (gitDone && !needsAcceptance),
      current: status === "awaitingAcceptance",
    },
    { key: "done", label: "完成", done: status === "completed", current: status === "completed" },
  ];

  return steps.map(({ key, label, note, done, current }) => ({
    key,
    label,
    note,
    state: (done ? "done" : current ? "current" : "pending") as CodexStepState,
  }));
}

function codexResultTitle(task: CodexTask) {
  if (task.status === "awaitingAcceptance") return "需要你验收";
  if (task.status === "completed") return "结果已完成";
  if (task.resultId) return "结果已回流";
  if (task.gitVerification?.status === "mismatch") return "代码记录不一致";
  return "等待结果回流";
}

function normalizeCodexTaskType(type?: string): CodexTaskType {
  if (type === "analysis" || type === "coding" || type === "verification" || type === "fileOperation") {
    return type;
  }
  return "analysis";
}

export function resolveCodexTaskDisplayType(
  task:
    | (Pick<CodexTask, "taskType" | "changedFiles" | "reportedCommits" | "verifiedCommits"> &
        Partial<Pick<CodexTask, "title" | "prompt" | "summary" | "resultText" | "tests">>)
    | null
    | undefined,
): CodexTaskType {
  if (!task) return "analysis";
  const storedType = normalizeCodexTaskType(task.taskType);
  const text = normalizeWhitespace([task.title, task.prompt, task.summary, task.resultText].filter(Boolean).join(" "));
  const hasCodeEvidence = Boolean(task.changedFiles?.length || task.reportedCommits?.length || task.verifiedCommits?.length);
  const hasAnalysisSignal = /项目理解|下一步建议|项目分析|分析结果|待确认问题|资料不足|recommendations|findings/i.test(text);
  if (storedType === "coding" && hasAnalysisSignal && !hasCodeEvidence) return "analysis";
  return storedType;
}

export function codexTaskResultGroups(task: CodexTask) {
  const type = resolveCodexTaskDisplayType(task);
  if (type === "analysis") {
    return [
      { title: "分析发现", items: codexPublicItems(task.findings), empty: "未记录分析发现。", defaultOpen: true },
      { title: "建议", items: codexPublicItems(task.recommendations), empty: "未记录建议。", defaultOpen: false },
      { title: "待确认问题", items: codexPublicItems(task.questions), empty: "未记录待确认问题。", defaultOpen: false },
    ];
  }
  if (type === "coding") {
    return [
      { title: "修改文件", items: codexPublicItems(task.changedFiles), empty: "本次未记录文件修改。", defaultOpen: true },
      { title: "测试", items: codexPublicItems(task.tests), empty: "未记录测试。", defaultOpen: false },
    ];
  }
  if (type === "verification") {
    return [
      { title: "检查项", items: codexPublicItems(task.checks), empty: "未记录检查项。", defaultOpen: true },
      { title: "通过", items: codexPublicItems(task.passed), empty: "未记录通过项。", defaultOpen: false },
      { title: "未通过", items: codexPublicItems(task.failed), empty: "未记录未通过项。", defaultOpen: false },
    ];
  }
  return [
    { title: "产物", items: codexPublicItems(task.artifacts), empty: "未记录产物。", defaultOpen: true },
    { title: "目标文件", items: codexPublicItems(task.targetFiles), empty: "未记录目标文件。", defaultOpen: false },
    { title: "失败项", items: codexPublicItems(task.failed), empty: "未记录失败项。", defaultOpen: false },
  ];
}

function codexTaskTypeText(type?: string) {
  const map: Record<string, string> = {
    analysis: "分析任务",
    coding: "代码任务",
    verification: "验收任务",
    fileOperation: "文件任务",
    legacy: "旧任务",
    unknown: "未知类型",
  };
  return map[type || ""] ?? "分析任务";
}

function codexTaskResultBlockTitle(type?: string) {
  const map: Record<string, string> = {
    analysis: "分析结果",
    coding: "完成内容",
    verification: "验收结果",
    fileOperation: "文件结果",
  };
  return map[normalizeCodexTaskType(type)] ?? "结果";
}

function codexTaskStatusText(status: string) {
  const map: Record<string, string> = {
    draft: "草稿",
    ready: "待交给 Codex",
    running: "执行中",
    awaitingResult: "等待结果回流",
    resultReceived: "结果已回流",
    verifying: "正在核验",
    awaitingAcceptance: "等待人工验收",
    completed: "已完成",
    failed: "执行失败",
    cancelled: "已取消",
    needsReview: "需要检查",
  };
  return map[status] ?? status ?? "未知";
}

function codexGitStatusText(status?: string) {
  const map: Record<string, string> = {
    verified: "已验证",
    partial: "部分验证",
    mismatch: "结果与 Git 不一致",
    notApplicable: "不适用",
    unavailable: "暂时无法验证",
  };
  return map[status || ""] ?? status ?? "未核验";
}

function codexBadgeTone(tone: string): CodexBadgeTone {
  if (tone === "success") return "success";
  if (tone === "warning") return "warning";
  if (tone === "danger") return "error";
  if (tone === "pending") return "pending";
  return "neutral";
}

function codexTaskStatusTone(status?: string): CodexBadgeTone {
  if (status === "completed") return "success";
  if (status === "failed" || status === "needsReview") return "error";
  if (status === "awaitingAcceptance" || status === "cancelled") return "warning";
  if (status === "running" || status === "resultReceived" || status === "verifying") return "pending";
  return "neutral";
}

export function codexHistoryGitSummary(task: CodexTask) {
  if (resolveCodexTaskDisplayType(task) !== "coding") return "";
  const commit = task.verifiedCommits?.[0] || task.reportedCommits?.[0];
  return commit ? `Git ${codexShortCommit(commit)}` : codexGitStatusText(task.gitVerification?.status);
}

function codexRunStatusText(status?: string) {
  const map: Record<string, string> = {
    starting: "启动中",
    running: "运行中",
    exited: "已退出，等待结果回流",
    failed: "执行失败",
    cancelled: "已停止",
    unknownAfterRestart: "重启后状态未知",
  };
  return map[status || ""] ?? status ?? "未启动";
}

export function selectCurrentCodexTask(tasks: CodexTask[], runs: CodexRun[], targetTaskId = "") {
  if (!tasks.length) return null;
  const taskById = new Map(tasks.map((task) => [task.taskId, task]));
  const targetTask = targetTaskId ? taskById.get(targetTaskId) : null;
  if (targetTask && !["completed", "cancelled"].includes(targetTask.status)) {
    return targetTask;
  }
  for (const run of [...runs].sort((left, right) => right.startedAt.localeCompare(left.startedAt))) {
    const task = taskById.get(run.taskId);
    if (task && !["completed", "cancelled"].includes(task.status)) return task;
  }
  return tasks.find((task) => !["completed", "cancelled"].includes(task.status)) ?? tasks[0];
}

export function buildCodexTaskSearchParams(current: URLSearchParams, taskId: string) {
  const next = new URLSearchParams(current);
  next.set("panel", "codex");
  next.set("taskId", taskId);
  return next;
}

export function resolveCodexPromptText(
  currentTask: Pick<CodexTask, "prompt"> | null,
  prompts: Array<Pick<CodexPromptRecord, "promptText">>,
) {
  return currentTask?.prompt ?? prompts[0]?.promptText ?? "";
}

function latestRunForTask(runs: CodexRun[], taskId: string) {
  return (
    runs
      .filter((run) => run.taskId === taskId)
      .sort((left, right) => right.startedAt.localeCompare(left.startedAt))[0] ?? null
  );
}

function atlasStatusText(atlas: AtlasAssessment | null) {
  if (!atlas) return "Atlas 评估尚未生成。";
  const version = atlas.atlasVersion || "未知版本";
  const status = atlas.status || "unavailable";
  return atlas.failureReason ? `${version} / ${status} / ${atlas.failureReason}` : `${version} / ${status}`;
}

function WorkspaceActivityPanel({
  projectAnalysis,
  atlas,
  monitoring,
  pendingReviews,
  activityMessages,
  needsAuthorization,
  onGrantAuthorization,
}: {
  projectAnalysis: ProjectAnalysis | null;
  atlas: AtlasAssessment | null;
  monitoring: MonitoringState | null;
  pendingReviews: PendingReviewItem[];
  activityMessages: WorkspaceMessage[];
  needsAuthorization: boolean;
  onGrantAuthorization: () => Promise<void>;
}) {
  const projectDefinition =
    projectAnalysis?.status === "success"
      ? projectAnalysis.projectDefinition || "项目理解仍在整理中。"
      : projectAnalysis?.status === "failed"
        ? `项目理解生成失败：${projectAnalysis.failureReason}`
        : "项目理解等待资料和授权后生成。";
  const latestActivity = activityMessages.slice(-4).reverse();

  return (
    <section className="workspace-activity-panel">
      <div className="workspace-sidebar-head workspace-activity-head">
        <span className="section-label">项目动态</span>
        <strong>初始化、理解和系统记录</strong>
        <p>这些内容不再占用聊天主线程，需要时在这里展开查看。</p>
      </div>

      <div className="workspace-activity-body">
        {needsAuthorization ? (
          <div className="workspace-activity-callout">
            <strong>DeepSeek 授权</strong>
            <p>首次发送前，需要你明确授权只发送项目说明、摘要、关键历史和当前问题，不上传原始文件。</p>
            <button type="button" className="btn btn-primary" onClick={() => void onGrantAuthorization()}>
              我已授权
            </button>
          </div>
        ) : null}

        <details className="workspace-activity-details" open>
          <summary>
            <span>项目理解</span>
            <strong>{projectAnalysis?.status || "等待"}</strong>
          </summary>
          <p>{projectDefinition}</p>
          <div className="workspace-activity-metrics">
            <span>待确认 {projectAnalysis?.questions?.filter(Boolean).length ?? 0}</span>
            <span>下一步 {projectAnalysis?.nextSteps?.filter(Boolean).length ?? 0}</span>
            <span>待检查 {pendingReviews.length}</span>
          </div>
        </details>

        <details className="workspace-activity-details">
          <summary>
            <span>系统记录</span>
            <strong>{activityMessages.length}</strong>
          </summary>
          {latestActivity.length ? (
            <div className="workspace-activity-list">
              {latestActivity.map((message) => (
                <article key={message.id} className="workspace-activity-item">
                  <span>{message.kind}{message.status ? ` / ${message.status}` : ""}</span>
                  <p>{message.text}</p>
                </article>
              ))}
            </div>
          ) : (
            <p>暂无系统记录。</p>
          )}
        </details>

        <details className="workspace-activity-details">
          <summary>
            <span>Atlas 与监视</span>
            <strong>{atlas?.status || monitoring?.status || "等待"}</strong>
          </summary>
          <p>{atlasStatusText(atlas)}</p>
          <p>{monitoring?.lastScannedAt ? `最近扫描：${monitoring.lastScannedAt}` : "主动监视尚未产生扫描结果。"}</p>
        </details>
      </div>
    </section>
  );
}

function InsightBlock({
  title,
  items,
  empty,
  defaultOpen = false,
}: {
  title: string;
  items?: string[];
  empty: string;
  defaultOpen?: boolean;
}) {
  const visible = items?.filter(Boolean).slice(0, 4) ?? [];
  return (
    <details className="insight-block" open={defaultOpen}>
      <summary>
        <span>{title}</span>
        <strong>{visible.length ? `${visible.length} 条` : "暂无"}</strong>
      </summary>
      {visible.length ? (
        <ul>
          {visible.map((item) => (
            <li key={item}>{item}</li>
          ))}
        </ul>
      ) : (
        <p>{empty}</p>
      )}
    </details>
  );
}

function isConversationMessage(message: WorkspaceMessage, index: number, projectDescription: string) {
  if (message.source === "local_rule" || message.source === "project_monitor") return false;
  if (message.author === "system") return false;
  if (message.kind === "analysis" || message.kind === "review" || message.kind === "monitor") return false;

  const trimmedText = message.text.trim();
  const trimmedDescription = projectDescription.trim();
  const isInitialProjectDescription =
    index === 0 &&
    message.author === "user" &&
    message.kind === "requirement" &&
    (trimmedText === trimmedDescription || trimmedText === "已创建项目，等待补充项目需求。");

  return !isInitialProjectDescription;
}

function Message({
  message,
  openFilePath,
  openFolderPath,
}: {
  message: WorkspaceMessage;
  openFilePath: (path: string) => Promise<void>;
  openFolderPath: (path: string) => Promise<void>;
}) {
  const facts = message.evidenceItems.filter((item) => item.basisKind === "fact");
  const inference = message.evidenceItems.filter((item) => item.basisKind === "inference");
  const pending = message.evidenceItems.filter((item) => item.basisKind === "pending");
  const evidenceCount = facts.length + inference.length + pending.length;
  return (
    <article className={`thread-message message-${message.author === "user" ? "user" : "ganmaoyuan"}`}>
      <div className="message-meta">
        <strong>{message.author === "user" ? "你" : "感冒院"}</strong>
        <span>{message.status ? `${message.kind} / ${message.status}` : message.kind}</span>
      </div>
      <p>{message.text}</p>
      {message.author !== "user" ? (
        <details className="answer-evidence">
          <summary>
            <span>回答依据</span>
            <strong>
              {evidenceCount
                ? `事实 ${facts.length} / 推断 ${inference.length} / 待确认 ${pending.length}`
                : "无项目资料依据"}
            </strong>
          </summary>
          <EvidenceGroup title="资料事实" items={facts} empty="无项目资料依据" openFilePath={openFilePath} openFolderPath={openFolderPath} />
          <EvidenceGroup title="AI 推断" items={inference} empty="本条回答未记录额外推断说明。" openFilePath={openFilePath} openFolderPath={openFolderPath} />
          <EvidenceGroup title="待确认内容" items={pending} empty="暂无待确认内容。" openFilePath={openFilePath} openFolderPath={openFolderPath} />
        </details>
      ) : null}
    </article>
  );
}

function EvidenceGroup({
  title,
  items,
  empty,
  openFilePath,
  openFolderPath,
}: {
  title: string;
  items: WorkspaceMessage["evidenceItems"];
  empty: string;
  openFilePath: (path: string) => Promise<void>;
  openFolderPath: (path: string) => Promise<void>;
}) {
  return (
    <div className="evidence-group">
      <strong>{title}</strong>
      {items.length ? (
        items.map((item) => (
          <div key={item.id} className="evidence-item">
            <div className="evidence-item-head">
              <span>{item.title || "未命名记录"}</span>
              <span>{formatEvidenceType(item.contentType)}</span>
            </div>
            <p>{item.summary || "无摘要"}</p>
            {item.managedPath ? (
              <div className="file-result-actions">
                <button type="button" className="btn" onClick={() => void openFilePath(item.managedPath).catch(() => undefined)}>
                  打开文件
                </button>
                <button type="button" className="btn" onClick={() => void openFolderPath(item.managedPath).catch(() => undefined)}>
                  打开所在位置
                </button>
              </div>
            ) : null}
          </div>
        ))
      ) : (
        <p>{empty}</p>
      )}
    </div>
  );
}

function formatEvidenceType(contentType: string) {
  switch (contentType) {
    case "project":
      return "项目说明";
    case "file":
      return "文件摘要";
    case "message":
      return "对话记录";
    case "decision":
      return "项目决定";
    case "projectAnalysis":
      return "项目分析";
    case "pendingReview":
      return "待检查事项";
    case "inference":
      return "模型推断";
    default:
      return "记录";
  }
}

function FileResult({
  file,
  openFilePath,
  openFolderPath,
  compactPath = false,
}: {
  file: ManagedFile;
  openFilePath: (path: string) => Promise<void>;
  openFolderPath: (path: string) => Promise<void>;
  compactPath?: boolean;
}) {
  const parseLabel =
    file.parseStatus === "success"
      ? "已解析"
      : file.parseStatus === "partial"
        ? "部分解析"
        : file.parseStatus === "failed"
          ? "解析失败"
          : file.parseStatus === "review_required"
            ? "待检查"
            : "待解析";
  const summary = file.parseStatus === "failed" ? file.parseFailureReason || "解析失败，等待人工检查。" : file.contentSummary || "暂无摘要。";
  const sections = file.mainFieldsOrSections?.length ? file.mainFieldsOrSections.slice(0, 3).join("、") : "未识别到主要字段或章节。";

  return (
    <article className="file-result">
      <div>
        <strong>{file.fileName}</strong>
        <span className={compactPath ? "file-result-path compact" : "file-result-path"}>{file.managedPath}</span>
        <p>
          系统判断: {parseLabel}; {file.fileType || "未知格式"} 文件。推荐分类: {file.recommendedCategory || file.category}。
          {file.duplicateOfFileId ? " 内容与已登记文件重复，未再次复制。" : ""}
        </p>
        <p>简短摘要: {summary}</p>
        <p>主要字段或章节: {sections}</p>
        <p>保存原因: {file.locationReason || "已通过统一位置服务归入当前受管目录。"}</p>
        {file.analysisWarnings?.length ? <p>解析提示: {file.analysisWarnings.join("；")}</p> : null}
      </div>
      <div className="file-result-actions">
        <button type="button" className="btn" onClick={() => void openFilePath(file.managedPath).catch(() => undefined)}>
          打开文件
        </button>
        <button type="button" className="btn" onClick={() => void openFolderPath(file.managedPath).catch(() => undefined)}>
          打开所在位置
        </button>
      </div>
    </article>
  );
}

function SidebarSection({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="sidebar-section">
      <h3>{title}</h3>
      {children}
    </section>
  );
}

function isNearBottom(element: HTMLElement | null) {
  if (!element) return true;
  return element.scrollHeight - element.scrollTop - element.clientHeight < 120;
}
