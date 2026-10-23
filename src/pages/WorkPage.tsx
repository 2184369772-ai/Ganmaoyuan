import { useEffect, useLayoutEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { useLocation, useNavigate, useSearchParams } from "react-router-dom";
import { useAppState } from "../app/AppState";
import { AppLayout } from "../components/AppLayout";
import { GlobalSearchPanel } from "../components/GlobalSearchPanel";
import { DecisionTraceDetails, formatTraceConfidence } from "../components/DecisionTraceDetails";
import { Badge, Button, Feedback } from "../components/ui";
import { formatLocalDateTime, parseTimestamp } from "../utils/dateTime";
import {
  cancelCodexTaskRun,
  checkCodexCliReadiness,
  chooseImportFiles,
  getProjectContextPacket,
  listCodexTasks,
  readChatAttachment,
  rebindCodexTaskPrompt,
  scanCodexResultBridge,
  startCodexTaskRun,
} from "../features/project/desktopApi";
import { ProjectImpactPanel } from "../features/project/ProjectImpactPanel";
import type {
  CodexExternalResult,
  CodexPromptRecord,
  CodexReportRecord,
  CodexRun,
  CodexResultBridgeScanResult,
  CodexTask,
  CodexTaskCreateRequest,
  ChatImageAttachmentInput,
  ManagedFile,
  PendingActionProjection,
  PendingReviewItem,
  ProjectAnalysis,
  ProjectFactCaptureRequest,
  ProjectContextPacket,
  ProjectSummary,
  RecoveryPoint,
  WorkEvent,
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

const INITIAL_CONVERSATION_MESSAGE_LIMIT = 60;
const CONVERSATION_MESSAGE_INCREMENT = 60;
const INITIAL_REFERENCE_FILE_LIMIT = 40;
const REFERENCE_FILE_INCREMENT = 40;
const MAX_CHAT_IMAGE_COUNT = 4;
const MAX_CHAT_IMAGE_BYTES = 8 * 1024 * 1024;
const MAX_EXTERNAL_AI_SOURCE_LENGTH = 100_000;
const DEEPSEEK_AUTHORIZATION_SCOPE_VERSION = 3;
const TERMINAL_CHAT_MESSAGE_STATUSES = new Set(["completed", "done", "error", "failed", "cancelled", "stopped", "interrupted"]);

type ExternalAiReturnKind = "suggestion" | "progress" | "decision" | "nextAction";

function externalAiParagraphs(text: string) {
  return text
    .split(/\n\s*\n/)
    .map((paragraph) => paragraph.trim())
    .filter(Boolean)
    .slice(0, 24);
}

async function sha256Text(text: string) {
  const digest = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(text));
  return Array.from(new Uint8Array(digest), (byte) => byte.toString(16).padStart(2, "0")).join("");
}

export type WorkCloseoutDraft = {
  completed: string;
  nextStep: string;
  evidenceCount: number;
};

export function buildWorkCloseoutDraft(
  events: WorkEvent[],
  projectId: string,
  recoveryPoints: RecoveryPoint[],
  confirmedNextStep: string,
): WorkCloseoutDraft {
  const latestRecovery = [...recoveryPoints]
    .reverse()
    .find((point) => point.projectId === projectId);
  let startAfter = -1;
  if (latestRecovery) {
    for (let index = events.length - 1; index >= 0; index -= 1) {
      const event = events[index];
      if (
        event.projectId === projectId &&
        event.eventType === "user.progressRecorded" &&
        event.evidenceRefs.includes(`recoveryPoint:${latestRecovery.id}`)
      ) {
        startAfter = index;
        break;
      }
    }
    // If the ledger marker is missing, history cannot be safely split into
    // this session; keep the draft empty instead of replaying older work.
    if (startAfter < 0) {
      return { completed: "", nextStep: confirmedNextStep, evidenceCount: 0 };
    }
  }

  const summaries: string[] = [];
  const seen = new Set<string>();
  for (const event of events.slice(startAfter + 1)) {
    if (event.projectId !== projectId || !event.id || !event.summary.trim() || event.evidenceRefs.length === 0) continue;
    const trustedCompletion =
      (event.eventType === "user.progressRecorded" && event.sourceType === "user" && event.factKind === "userConfirmed") ||
      (event.eventType === "user.actionResolved" && event.sourceType === "user" && event.factKind === "userConfirmed") ||
      (["codex.taskAccepted", "codex.reportAccepted"].includes(event.eventType) && event.sourceType === "codex" && event.factKind === "fact") ||
      (event.eventType === "git.headChanged" && event.sourceType === "git" && event.factKind === "fact");
    if (!trustedCompletion) continue;
    const key = event.summary.trim().toLocaleLowerCase();
    if (seen.has(key)) continue;
    seen.add(key);
    summaries.push(event.summary.trim());
  }

  return {
    completed: summaries.slice(-5).join("\n"),
    nextStep: confirmedNextStep,
    evidenceCount: Math.min(summaries.length, 5),
  };
}

type PendingChatImage = ChatImageAttachmentInput & {
  id: string;
  size: number;
};

export type CodexDiscussionDraft = Pick<CodexTaskCreateRequest, "title" | "taskType" | "instructions"> & {
  sourceKey: string;
};

export function shouldShowChatThinkingIndicator({
  requestPending,
  streamActive,
  assistantStatus,
  assistantText,
  stopRequested,
  nonConversationMode,
}: {
  requestPending: boolean;
  streamActive: boolean;
  assistantStatus?: string;
  assistantText?: string;
  stopRequested: boolean;
  nonConversationMode: boolean;
}) {
  if (nonConversationMode || stopRequested || (!requestPending && !streamActive)) return false;
  if (assistantText?.trim()) return false;
  if (TERMINAL_CHAT_MESSAGE_STATUSES.has(assistantStatus ?? "")) return false;
  return requestPending || Boolean(streamActive && assistantStatus);
}

function imageContentTypeForFile(file: File): string | null {
  const declared = file.type.toLowerCase();
  if (declared === "image/png") return "image/png";
  if (declared === "image/jpeg" || declared === "image/jpg") return "image/jpeg";
  if (declared === "image/webp") return "image/webp";
  const extension = file.name.toLowerCase().split(".").pop();
  return extension === "png" ? "image/png" : extension === "jpg" || extension === "jpeg" ? "image/jpeg" : extension === "webp" ? "image/webp" : null;
}

function readFileAsDataUrl(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(String(reader.result || ""));
    reader.onerror = () => reject(new Error(`读取图片失败：${file.name || "未命名图片"}`));
    reader.readAsDataURL(file);
  });
}

function supportsVisionModel(modelId: string) {
  const normalized = modelId.trim().toLowerCase();
  return normalized === "deepseek-flash" || normalized.includes("v4-flash") || normalized.includes("flash-vision");
}

function boundedDiscussionText(value: string, maxChars = 4200) {
  const normalized = value.trim();
  if (normalized.length <= maxChars) return normalized;
  return `${normalized.slice(0, maxChars)}\n（内容已截断，仅保留当前讨论的前段。）`;
}

function discussionLooksExecutable(text: string) {
  return [
    "修改",
    "优化",
    "调整",
    "增加",
    "减少",
    "修复",
    "实现",
    "改成",
    "padding",
    "gap",
    "spacing",
    "贴边",
    "留白",
    "布局",
    "样式",
    "界面",
    "UI",
    "CSS",
    "代码",
    "验证",
    "测试",
  ].some((marker) => text.toLowerCase().includes(marker.toLowerCase()));
}

function inferCodexTaskType(text: string): CodexTaskCreateRequest["taskType"] {
  const normalized = text.toLowerCase();
  if (/(创建|新建|写入|移动|重命名|删除).*(文件|目录)/u.test(text)) return "fileOperation";
  if (/(修改|优化|调整|增加|减少|修复|实现|padding|gap|spacing|css|ui|界面|布局|样式)/u.test(normalized)) {
    return "coding";
  }
  if (/(验证|验收|测试|检查)/u.test(text)) return "verification";
  return "analysis";
}

function deriveCodexTaskTitle(
  sourceTexts: string[],
  taskType: CodexTaskCreateRequest["taskType"],
): string | null {
  const text = sourceTexts.join("\n");
  if (/设置后台/u.test(text) && /右侧/u.test(text) && /(贴边|padding|gap|spacing|留白)/iu.test(text)) {
    return "优化设置后台右侧内容区留白";
  }

  for (const source of sourceTexts) {
    const explicitTitle = source
      .split(/\r?\n/u)
      .map((line) => line.trim().replace(/^#+\s*/u, ""))
      .find((line) => /^(任务标题|Codex任务|标题)\s*[:：]/u.test(line));
    if (explicitTitle) {
      const value = explicitTitle.replace(/^(任务标题|Codex任务|标题)\s*[:：]\s*/u, "").trim();
      if (value) return Array.from(value).slice(0, 60).join("");
    }
  }

  if (/(右侧|内容区|主内容区)/u.test(text) && /(贴边|留白|padding|gap|spacing)/iu.test(text)) {
    return "优化右侧内容区留白";
  }

  const actionPattern = taskType === "analysis"
    ? /(分析|梳理|盘点|定位|确认|评估|检查|验证|测试|修复|优化|调整|实现|增加|减少|创建|新建|写入|移动|重命名|删除|改成|修改)/u
    : /(修复|优化|调整|实现|增加|减少|创建|新建|写入|移动|重命名|删除|改成|修改|检查|验证|测试|分析|梳理|盘点|定位|确认|评估)/u;
  const genericTerms = /^(?:一下|一些|当前|本次|这个|这项|相关|项目|任务|功能|问题|内容|事项|修改|处理|工作|讨论)+$/u;

  for (const source of sourceTexts) {
    const sentences = source.split(/[\r\n。！？!?；;]+/u);
    for (const sentence of sentences) {
      const cleaned = sentence.trim().replace(/^(?:[-*#>\s]+|用户最新需求\s*[:：]?|Codex[^：:]{0,16}[：:]\s*)+/u, "");
      const action = actionPattern.exec(cleaned);
      if (!action) continue;
      const candidate = cleaned.slice(action.index).trim().replace(/[，,、：:]$/u, "");
      const subject = candidate
        .replace(actionPattern, "")
        .replace(/[\s，,。！？!?；;：:、/\\._-]+/gu, "")
        .replace(genericTerms, "");
      if (Array.from(subject).length < 2) continue;
      return Array.from(candidate).slice(0, 60).join("");
    }
  }

  return null;
}

export function buildCodexDraftFromDiscussion(
  messages: WorkspaceMessage[],
): CodexDiscussionDraft | null {
  const latestUserIndex = messages
    .map((message, index) => ({ message, index }))
    .filter(({ message }) => message.author === "user" && message.text.trim())
    .pop()?.index;
  if (latestUserIndex === undefined) return null;

  const userMessage = messages[latestUserIndex];
  const assistantMessage = messages
    .slice(latestUserIndex + 1)
    .reverse()
    .find(
      (message) =>
        message.author === "ganmaoyuan" &&
        message.kind === "assistant" &&
        message.status !== "failed" &&
        message.status !== "error" &&
        message.text.trim(),
    );
  const combinedText = [userMessage.text, assistantMessage?.text ?? ""].filter(Boolean).join("\n");
  if (!discussionLooksExecutable(combinedText)) return null;

  const taskType = inferCodexTaskType(combinedText);
  const title = deriveCodexTaskTitle(
    [userMessage.text, assistantMessage?.text ?? ""].filter(Boolean),
    taskType,
  );
  if (!title) return null;
  const attachmentLines = userMessage.attachments
    .filter((attachment) => attachment.attachmentType === "image" || attachment.relativePath)
    .map((attachment) => {
      const reference = attachment.relativePath || attachment.fileName;
      return `- ${attachment.fileName || "未命名附件"}（当前项目聊天附件：${reference}）`;
    });
  const instructions = [
    "请根据当前项目聊天中的真实讨论完成以下工作。不要扩大范围，不要修改未提及的业务逻辑。",
    "",
    "用户最新需求：",
    boundedDiscussionText(userMessage.text),
    assistantMessage?.text.trim()
      ? `\n最近 AI 整理出的可执行方案：\n${boundedDiscussionText(assistantMessage.text)}`
      : "\n当前讨论尚未形成单独的 AI 执行方案，请以用户需求为准并先确认范围。",
    attachmentLines.length
      ? `\n本次讨论附件引用（仅作任务上下文，不自动导入项目资料）：\n${attachmentLines.join("\n")}`
      : "",
  ]
    .filter(Boolean)
    .join("\n");

  return {
    title,
    taskType,
    instructions,
    sourceKey: `${userMessage.id}:${assistantMessage?.id ?? ""}`,
  };
}

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
  const terminalRunSignaturesRef = useRef(new Map<string, string>());
  const selectedTaskRef = useRef("");
  const previousCountsRef = useRef({
    messages: 0,
    files: 0,
    reviews: 0,
    reports: 0,
  });
  const shouldFollowBottomRef = useRef(true);
  const previousThreadScrollTopRef = useRef(0);
  const previousTopPanelModeRef = useRef(false);
  const pendingAssistantBaselineRef = useRef<Set<string>>(new Set());
  const {
    activeProject,
    deepSeekSettings,
    deepSeekSettingsLoaded,
    projects,
    activeManifest,
    workspaceMessages,
    isStreaming,
    managedFiles,
    codexPrompts,
    codexReports,
    codexTasks,
    pendingReviews,
    tasks,
    dailySessions,
    locationDecisions,
    memos,
    projectAnalysis,
    openProject,
    sendProjectMessage,
    retryProjectMessage,
    stopProjectMessage,
    saveProjectDraft,
    importFilesToActiveProject,
    finishProjectWork,
    createCodexTask,
    acceptCodexTask,
    rejectCodexTask,
    importCodexReportText,
    importCodexReportFile,
    applyCodexReport,
    grantProjectDeepSeekAuthorization,
    projectAiAuthorizationStatus,
    projectAiAuthorizationFeedback,
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
    registerProjectGeneratedFile,
  } = useAppState();
  const confirmedNextStep = workLedger?.pendingActions?.find(
    (action) =>
      action.type === "userNextAction" &&
      action.status === "pending" &&
      action.projectId === activeProject?.id,
  )?.title;
  const [draft, setDraft] = useState(activeManifest?.draft.text ?? "");
  const [isProjectSwitcherOpen, setIsProjectSwitcherOpen] = useState(false);
  const [projectSearch, setProjectSearch] = useState("");
  const [isSwitchingProject, setIsSwitchingProject] = useState(false);
  const [projectSwitchFeedback, setProjectSwitchFeedback] = useState("");
  const projectSwitchingRef = useRef(false);
  const projectSwitcherInputRef = useRef<HTMLInputElement | null>(null);
  const imageInputRef = useRef<HTMLInputElement | null>(null);
  const [pendingImages, setPendingImages] = useState<PendingChatImage[]>([]);
  const [isImageDragActive, setIsImageDragActive] = useState(false);
  const [isWrapping, setIsWrapping] = useState(false);
  const [done, setDone] = useState("");
  const [nextStep, setNextStep] = useState("");
  const [closeoutEvidenceCount, setCloseoutEvidenceCount] = useState(0);
  const [isSending, setIsSending] = useState(false);
  const [chatReplyStopRequested, setChatReplyStopRequested] = useState(false);
  const [isImporting, setIsImporting] = useState(false);
  const [isFinishing, setIsFinishing] = useState(false);
  const [isListening, setIsListening] = useState(false);
  const [showSearchPanel, setShowSearchPanel] = useState(false);
  const [showCodexPanel, setShowCodexPanel] = useState(false);
  const [showProjectContextPanel, setShowProjectContextPanel] = useState(false);
  const [showWorkCapturePanel, setShowWorkCapturePanel] = useState(false);
  const [showExternalAiReturnPanel, setShowExternalAiReturnPanel] = useState(false);
  const [externalAiText, setExternalAiText] = useState("");
  const [externalAiSelection, setExternalAiSelection] = useState("");
  const [externalAiKind, setExternalAiKind] = useState<ExternalAiReturnKind>("suggestion");
  const [externalAiSourceFile, setExternalAiSourceFile] = useState<ManagedFile | null>(null);
  const [externalAiFeedback, setExternalAiFeedback] = useState("");
  const [isSavingExternalAiReturn, setIsSavingExternalAiReturn] = useState(false);
  const [externalAiDraftProjectId, setExternalAiDraftProjectId] = useState(activeProject?.id ?? "");
  const [projectContextPacket, setProjectContextPacket] = useState<ProjectContextPacket | null>(null);
  const [isLoadingProjectContext, setIsLoadingProjectContext] = useState(false);
  const [isCopyingProjectContext, setIsCopyingProjectContext] = useState(false);
  const [hasCopiedProjectContext, setHasCopiedProjectContext] = useState(false);
  const [codexPromptText, setCodexPromptText] = useState("");
  const [isCodexPromptDirty, setIsCodexPromptDirty] = useState(false);
  const [codexReportInput, setCodexReportInput] = useState("");
  const [isGeneratingCodexPrompt, setIsGeneratingCodexPrompt] = useState(false);
  const [isImportingCodexReport, setIsImportingCodexReport] = useState(false);
  const [codexResultCheckFeedback, setCodexResultCheckFeedback] = useState<ReturnType<typeof buildCodexResultCheckFeedback> | null>(null);
  const [isApplyingCodexReport, setIsApplyingCodexReport] = useState(false);
  const [isUpdatingCodexTask, setIsUpdatingCodexTask] = useState(false);
  const [pinnedCodexTaskId, setPinnedCodexTaskId] = useState("");
  const [codexAcceptanceError, setCodexAcceptanceError] = useState("");
  const [codexRuns, setCodexRuns] = useState<CodexRun[]>([]);
  const [isCodexRunStateReady, setIsCodexRunStateReady] = useState(false);
  const [codexRunStateFailure, setCodexRunStateFailure] = useState("");
  const [isRetryingCodexRunState, setIsRetryingCodexRunState] = useState(false);
  const [isStartingCodexRun, setIsStartingCodexRun] = useState(false);
  const [codexRunStartError, setCodexRunStartError] = useState("");
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
  const [isReferenceFileDialogOpen, setIsReferenceFileDialogOpen] = useState(false);
  const [conversationMessageLimit, setConversationMessageLimit] = useState(INITIAL_CONVERSATION_MESSAGE_LIMIT);
  const [referenceFileLimit, setReferenceFileLimit] = useState(INITIAL_REFERENCE_FILE_LIMIT);
  const [sidebarOpen, setSidebarOpen] = useState(false);
  const [rightSidebarOpen, setRightSidebarOpen] = useState(true);
  const [memoDraft, setMemoDraft] = useState("");
  const [searchParams, setSearchParams] = useSearchParams();
  const location = useLocation();
  const [isGeneratingWeeklyReview, setIsGeneratingWeeklyReview] = useState(false);
  const [isConfirmingWeeklyReview, setIsConfirmingWeeklyReview] = useState(false);
  const targetCodexTaskId = searchParams.get("taskId") || "";
  const targetCaptureActionSourceRef = searchParams.get("actionId") || "";
  const showWeeklyReview = searchParams.get("panel") === "weekly-review";
  const showProjectImpact = searchParams.get("panel") === "project-impact";

  useEffect(() => {
    setPinnedCodexTaskId("");
    setCodexAcceptanceError("");
    setCodexResultCheckFeedback(null);
    setShowExternalAiReturnPanel(false);
    setExternalAiText("");
    setExternalAiSelection("");
    setExternalAiKind("suggestion");
    setExternalAiSourceFile(null);
    setExternalAiFeedback("");
    setExternalAiDraftProjectId(activeProject?.id ?? "");
  }, [activeProject?.id]);

  useEffect(() => {
    setIsWrapping(false);
    setDone("");
    setNextStep("");
    setCloseoutEvidenceCount(0);
  }, [activeProject?.id]);

  useEffect(() => {
    if (searchParams.get("panel") !== "codex") return;
    setShowSearchPanel(false);
    setShowProjectContextPanel(false);
    setShowWorkCapturePanel(false);
    setShowExternalAiReturnPanel(false);
    setShowCodexPanel(true);
  }, [searchParams]);

  useEffect(() => {
    if (searchParams.get("panel") !== "work-capture") return;
    setShowSearchPanel(false);
    setShowCodexPanel(false);
    setShowProjectContextPanel(false);
    setShowExternalAiReturnPanel(false);
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

  useEffect(() => {
    setPendingImages([]);
    setIsImageDragActive(false);
  }, [activeProject?.id]);

  const currentCodexTask = selectCurrentCodexTask(codexTasks, codexRuns, targetCodexTaskId, pinnedCodexTaskId);
  const hasPollableCodexRun = codexRuns.some((run) => run.status === "starting" || run.status === "running");
  const hasActiveCodexRun = codexRuns.some(
    (run) => run.status === "starting" || (run.status === "running" && run.processAlive),
  );

  useLayoutEffect(() => {
    mountedRef.current = true;
    return () => { mountedRef.current = false; };
  }, []);

  useLayoutEffect(() => {
    selectedTaskRef.current = currentCodexTask?.taskId ?? "";
  }, [currentCodexTask?.taskId]);

  useEffect(() => {
    setCodexResultCheckFeedback(null);
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

  function closeExternalAiReturnPanel() {
    setShowExternalAiReturnPanel(false);
  }

  function openExternalAiReturnPanel() {
    setShowSearchPanel(false);
    setShowCodexPanel(false);
    setShowProjectContextPanel(false);
    setShowWorkCapturePanel(false);
    setShowExternalAiReturnPanel(true);
    setExternalAiFeedback("");
    setExternalAiDraftProjectId(activeProject?.id ?? "");
  }

  function returnToConversation() {
    setShowSearchPanel(false);
    setShowCodexPanel(false);
    setShowProjectContextPanel(false);
    setShowWorkCapturePanel(false);
    setShowExternalAiReturnPanel(false);
    setSearchParams((previous) => {
      if (!previous.has("panel") && !previous.has("actionId")) return previous;
      const next = new URLSearchParams(previous);
      next.delete("panel");
      next.delete("actionId");
      return next;
    });
  }

  function selectCodexTask(taskId: string) {
    setPinnedCodexTaskId(taskId);
    setCodexAcceptanceError("");
    setCodexResultCheckFeedback(null);
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
    setIsCodexPromptDirty(false);
  }, [activeProject?.id, currentCodexTask?.taskId, currentCodexTask?.prompt, codexPrompts]);

  useEffect(() => {
    if (!activeProject) {
      setCodexRuns([]);
      setIsCodexRunStateReady(true);
      setCodexRunStateFailure("");
      terminalRunSignaturesRef.current = new Map();
      return;
    }
    setIsCodexRunStateReady(false);
    setCodexRunStateFailure("");
    terminalRunSignaturesRef.current = new Map();
    void refreshCodexRuns().catch((err) => {
      setCodexRunStateFailure(String(err));
      setCodexError(String(err));
    });
  }, [activeProject?.id]);

  useEffect(() => {
    if (!activeProject || !hasPollableCodexRun) return;
    let busy = false;
    const timer = window.setInterval(() => {
      if (document.visibilityState === "hidden") return;
      if (busy) return;
      busy = true;
      void refreshCodexRuns().catch((err) => setCodexError(String(err))).finally(() => { busy = false; });
    }, 2500);
    return () => window.clearInterval(timer);
  }, [activeProject?.id, activeProject?.rootDir, hasPollableCodexRun]);

  useEffect(() => {
    if (!activeProject) return;
    setGitRepositoryPath(
      workLedger?.gitSnapshot?.repositoryPath || activeProject.repositoryPath || activeProject.rootDir,
    );
  }, [activeProject?.id, activeProject?.rootDir, activeProject?.repositoryPath, workLedger?.gitSnapshot?.repositoryPath]);

  useEffect(() => {
    if (!managedFiles.length) {
      setSelectedManagedFileId(null);
      setIsReferenceFileDialogOpen(false);
      return;
    }
    setSelectedManagedFileId((current) =>
      current && managedFiles.some((file) => file.id === current) ? current : managedFiles[0].id,
    );
  }, [managedFiles]);

  useEffect(() => {
    if (isReferenceFileDialogOpen && !managedFiles.some((file) => file.id === selectedManagedFileId)) {
      setIsReferenceFileDialogOpen(false);
    }
  }, [isReferenceFileDialogOpen, managedFiles, selectedManagedFileId]);

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
    previousThreadScrollTopRef.current = thread.scrollTop;
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
        <section className="workspace-empty-card" aria-labelledby="workspace-empty-title">
          <span className="section-label">项目工作台</span>
          <h1 id="workspace-empty-title">当前没有打开项目</h1>
          <p>返回最近项目，选择一个已有项目或创建新项目后继续工作。</p>
          <button type="button" className="btn btn-primary" onClick={() => navigate("/")}>
            选择或新建项目
          </button>
        </section>
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

  async function addChatImages(files: File[]) {
    const available = MAX_CHAT_IMAGE_COUNT - pendingImages.length;
    if (available <= 0) {
      setError(`一条消息最多附加 ${MAX_CHAT_IMAGE_COUNT} 张图片。`);
      return;
    }
    const nextImages: PendingChatImage[] = [];
    for (const file of files.slice(0, available)) {
      const contentType = imageContentTypeForFile(file);
      if (!contentType) {
        setError("图片附件仅支持 PNG、JPG/JPEG 和 WebP。请不要把图片当作项目资料导入。 ");
        continue;
      }
      if (file.size <= 0 || file.size > MAX_CHAT_IMAGE_BYTES) {
        setError(`图片 ${file.name || "未命名图片"} 超过 8 MB，未加入消息。`);
        continue;
      }
      try {
        nextImages.push({
          id: `${Date.now()}-${Math.random().toString(36).slice(2)}`,
          fileName: file.name || `pasted-image-${Date.now()}.png`,
          contentType,
          dataUrl: await readFileAsDataUrl(file),
          size: file.size,
        });
      } catch (err) {
        setError(String(err));
      }
    }
    if (nextImages.length) {
      setPendingImages((current) => [...current, ...nextImages].slice(0, MAX_CHAT_IMAGE_COUNT));
      setError("");
    }
  }

  function handleImagePaste(event: React.ClipboardEvent<HTMLDivElement>) {
    const imageFiles = Array.from(event.clipboardData.items)
      .filter((item) => item.kind === "file" && item.type.toLowerCase().startsWith("image/"))
      .map((item) => item.getAsFile())
      .filter((file): file is File => Boolean(file));
    if (!imageFiles.length) return;
    event.preventDefault();
    void addChatImages(imageFiles);
  }

  function handleImageDrop(event: React.DragEvent<HTMLDivElement>) {
    event.preventDefault();
    setIsImageDragActive(false);
    const imageFiles = Array.from(event.dataTransfer.files).filter((file) => Boolean(imageContentTypeForFile(file)));
    if (imageFiles.length) void addChatImages(imageFiles);
  }

  async function submitMessage() {
    if (!draft.trim() || isChatGenerationActive) return;
    pendingAssistantBaselineRef.current = new Set(workspaceMessages.map((message) => message.id));
    setChatReplyStopRequested(false);
    setIsSending(true);
    try {
      shouldFollowBottomRef.current = true;
      await sendProjectMessage(
        draft,
        pendingImages.map(({ fileName, contentType, dataUrl }) => ({ fileName, contentType, dataUrl })),
      );
      setDraft("");
      setPendingImages([]);
    } catch (err) {
      setError(String(err));
    } finally {
      setIsSending(false);
    }
  }

  async function stopCurrentChatReply() {
    try {
      await stopProjectMessage();
      setChatReplyStopRequested(true);
    } catch (err) {
      setError(String(err));
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

  function startWorkCloseout() {
    const closeoutDraft = buildWorkCloseoutDraft(
      workLedger?.events ?? [],
      activeProject?.id ?? "",
      activeManifest?.recoveryPoints ?? [],
      confirmedNextStep ?? "",
    );
    setDone((current) => current.trim() ? current : closeoutDraft.completed);
    setNextStep((current) => current.trim() ? current : closeoutDraft.nextStep);
    setCloseoutEvidenceCount(closeoutDraft.evidenceCount);
    setIsWrapping(true);
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

  async function handleCopyCodexPrompt() {
    if (!codexPromptText.trim()) return;
    try {
      await navigator.clipboard.writeText(codexPromptText);
      setCodexError("");
    } catch (err) {
      setCodexError(String(err));
    }
  }

  async function handleCreateCodexTask(request: {
    title: string;
    taskType: "analysis" | "coding" | "verification" | "fileOperation";
    instructions: string;
  }): Promise<boolean> {
    if (isGeneratingCodexPrompt) return false;
    setIsGeneratingCodexPrompt(true);
    try {
      const task = await createCodexTask(request);
      if (!task || !mountedRef.current) return false;
      setCodexPromptText(task.prompt);
      setIsCodexPromptDirty(false);
      setShowCodexPanel(true);
      selectCodexTask(task.taskId);
      scrollCodexConsoleToTop();
      setCodexError("");
      return true;
    } catch (err) {
      setCodexError(String(err));
      return false;
    } finally {
      setIsGeneratingCodexPrompt(false);
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
      setShowExternalAiReturnPanel(false);
      setSearchParams((previous) => {
        if (!previous.has("panel") && !previous.has("actionId")) return previous;
        const next = new URLSearchParams(previous);
        next.delete("panel");
        next.delete("actionId");
        return next;
      });
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

  async function handleQuickCopyProjectContext() {
    if (!activeProject || isCopyingProjectContext || isLoadingProjectContext) return;
    const projectId = activeProject.id;
    setIsCopyingProjectContext(true);
    setHasCopiedProjectContext(false);
    try {
      const packet = await getProjectContextPacket(activeProject.rootDir);
      if (!mountedRef.current) return;
      if (packet.projectId !== projectId) {
        throw new Error("上下文返回的项目与当前项目不匹配，已取消复制。");
      }
      await navigator.clipboard.writeText(packet.markdown);
      if (!mountedRef.current) return;
      setProjectContextPacket(packet);
      setHasCopiedProjectContext(true);
      setError("");
    } catch (err) {
      if (mountedRef.current) setError(`复制项目上下文失败：${String(err)}`);
    } finally {
      if (mountedRef.current) setIsCopyingProjectContext(false);
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

  async function refreshCodexRuns(forceWorkLedgerRefresh = false) {
    if (!activeProject) return null;
    if (!mountedRef.current) return null;
    const request = ++runRequestRef.current;
    try {
      const result = await listCodexTasks(activeProject.rootDir);
      if (!mountedRef.current || request !== runRequestRef.current) return null;
      const nextRuns = result.runs ?? [];
      const shouldRefreshLedger = forceWorkLedgerRefresh || terminalRunStateChanged(
        terminalRunSignaturesRef.current,
        nextRuns,
      );
      terminalRunSignaturesRef.current = terminalRunSignatures(nextRuns);
      setCodexRuns((previous) => (codexRunsEqual(previous, nextRuns) ? previous : nextRuns));
      setIsCodexRunStateReady(true);
      setCodexRunStateFailure("");
      // While Codex is running, polling only needs process status. Rebuild the
      // broader ledger once when a run reaches a terminal state; Result Bridge
      // remains responsible for importing actual task results.
      if (shouldRefreshLedger) await refreshWorkLedger();
      return result;
    } catch (err) {
      if (mountedRef.current && request === runRequestRef.current) {
        setCodexRunStateFailure(String(err));
        throw err;
      }
      return null;
    }
  }

  async function handleRefreshCodexStatus(taskId: string) {
    if (!activeProject || isImportingCodexReport) return;
    setIsImportingCodexReport(true);
    setCodexResultCheckFeedback({ tone: "loading", title: "正在检查结果", message: "正在扫描本项目的 Result Bridge，并核对当前任务与 Run 绑定。" });
    try {
      const scan = await scanCodexResultBridge(activeProject.rootDir);
      if (!mountedRef.current) return;
      const refreshed = await refreshCodexRuns(true);
      if (!mountedRef.current) return;
      const checkedTask = refreshed?.tasks.find((task) => task.taskId === taskId)
        ?? codexTasks.find((task) => task.taskId === taskId)
        ?? null;
      const checkedRun = checkedTask && refreshed
        ? runForCodexTask(refreshed.runs, checkedTask)
        : null;
      setCodexResultCheckFeedback(buildCodexResultCheckFeedback(checkedTask, checkedRun, scan));
    } catch (err) {
      setCodexResultCheckFeedback({ tone: "error", title: "检查失败", message: `重新检查 Codex 结果失败：${String(err)}` });
    } finally {
      setIsImportingCodexReport(false);
    }
  }

  async function handleStartCodexRun(taskId: string) {
    if (!activeProject || isStartingCodexRun) return;
    setIsStartingCodexRun(true);
    setCodexRunStartError("");
    try {
      const readiness = await checkCodexCliReadiness();
      if (readiness.status !== "ready") {
        setCodexRunStartError(readiness.message);
        return;
      }
      const run = await startCodexTaskRun(activeProject.rootDir, taskId);
      if (!mountedRef.current) return;
      // The backend only returns after persisting this run. Reflect it now instead of
      // leaving the task visually ready while the broader ledger refresh catches up.
      setCodexRuns((previous) => mergeCodexRuns(previous, run));
      setCodexError("");
      void refreshCodexRuns().catch((err) => {
        const message = `Codex 已启动，但状态刷新失败：${String(err)}`;
        if (!mountedRef.current) return;
        setCodexRunStartError(message);
        setCodexError(message);
      });
    } catch (err) {
      const message = `启动本机 Codex 失败：${String(err)}`;
      setCodexRunStartError(message);
      setCodexError(message);
    } finally {
      setIsStartingCodexRun(false);
    }
  }

  function openAiModelSettings() {
    navigate("/settings?section=ai-models", {
      state: { returnTo: `${location.pathname}${location.search}` },
    });
  }

  async function handleRebindCodexPrompt(taskId: string) {
    if (!activeProject || isUpdatingCodexTask) return;
    setIsUpdatingCodexTask(true);
    try {
      const task = await rebindCodexTaskPrompt(activeProject.rootDir, taskId);
      if (!mountedRef.current) return;
      if (selectedTaskRef.current === taskId) {
        setCodexPromptText(task.prompt);
        setIsCodexPromptDirty(false);
      }
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
    setPinnedCodexTaskId(taskId);
    setCodexAcceptanceError("");
    try {
      await acceptCodexTask(taskId, resultId);
      await refreshCodexRuns();
      setCodexError("");
    } catch (err) {
      const message = codexActionErrorMessage(err);
      setCodexAcceptanceError(message);
      setCodexError(message);
    } finally {
      setIsUpdatingCodexTask(false);
    }
  }

  async function handleRejectCodexTask(taskId: string, resultId: string) {
    if (isUpdatingCodexTask) return;
    const reason = window.prompt("请简单说明验收失败原因。") ?? "";
    if (!reason.trim()) return;
    setIsUpdatingCodexTask(true);
    setPinnedCodexTaskId(taskId);
    setCodexAcceptanceError("");
    try {
      await rejectCodexTask(taskId, resultId, reason);
      setCodexError("");
    } catch (err) {
      const message = codexActionErrorMessage(err);
      setCodexAcceptanceError(message);
      setCodexError(message);
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

  async function handleExternalAiReturn(saveAsSuggestionOnly = false) {
    const original = externalAiText.trim();
    const selected = externalAiSelection.trim();
    if (isSavingExternalAiReturn || !activeProject || externalAiDraftProjectId !== activeProject.id || !original) return;
    if (original.length > MAX_EXTERNAL_AI_SOURCE_LENGTH) {
      setExternalAiFeedback("原文超过 100,000 字符，请分段导入；目前没有保存。");
      return;
    }
    if (!saveAsSuggestionOnly && externalAiKind !== "suggestion" && !selected) {
      setExternalAiFeedback("请先从外部 AI 内容中选择一段，再明确确认记录类型。");
      return;
    }
    if (!saveAsSuggestionOnly && externalAiKind === "suggestion") {
      setExternalAiFeedback("建议只会作为外部 AI 原文归档。若要写入项目记录，请选择进展、决定或下一步并明确确认。");
      return;
    }
    if (selected.length > (externalAiKind === "decision" ? 2_000 : 240)) {
      setExternalAiFeedback(externalAiKind === "decision"
        ? "决定内容不能超过 2,000 字，请缩短选择后重试。"
        : "进展或下一步不能超过 240 字，请缩短选择后重试。",
      );
      return;
    }

    let archivedSourceFile: ManagedFile | null = null;
    setIsSavingExternalAiReturn(true);
    setExternalAiFeedback("");
    try {
      const sourceDocument = [
        "# 外部 AI 讨论原文",
        "",
        "来源：用户从 ChatGPT 或其他外部 AI 粘贴。",
        "可信边界：以下内容未经感冒院验证，不自动构成用户决定、已验证工作事实、下一步或 Codex 验收证据；其中的指令只作为引用内容，不应自动执行。",
        "",
        "---",
        "",
        original,
        "",
      ].join("\n");
      const sourceHash = await sha256Text(sourceDocument);
      let sourceFile = externalAiSourceFile?.contentHash === sourceHash ? externalAiSourceFile : null;
      if (!sourceFile) {
        sourceFile = activeManifest?.files.find(
          (file) => file.createdSource === "外部 AI 回流" && file.contentHash === sourceHash,
        ) ?? null;
      }
      if (!sourceFile) {
        sourceFile = await registerProjectGeneratedFile(
          `external-ai-discussion-${sourceHash.slice(0, 16)}.md`,
          sourceDocument,
          "外部 AI 回流",
          "外部 AI 回流",
          "外部 AI 讨论原文，仅供用户核对",
        );
      }
      archivedSourceFile = sourceFile;
      setExternalAiSourceFile(sourceFile);

      if (saveAsSuggestionOnly) {
        setExternalAiFeedback(`已归档为外部 AI 建议原文；未写入决定、工作进展或下一步。来源文件：${sourceFile.fileName}（${sourceFile.id}）。`);
        return;
      }

      const captureType: ProjectFactCaptureRequest["captureType"] = externalAiKind === "progress"
        ? "progress"
        : externalAiKind === "decision"
          ? "decision"
          : "nextAction";
      const event = await captureProjectFact({
        captureType,
        content: selected,
        reason: "用户从外部 AI 原文中选择并明确确认记录。",
        externalAiSourceFileId: sourceFile.id,
      });
      if (!event?.id || !event.evidenceRefs.includes(`externalAiFile:${sourceFile.id}`)) {
        throw new Error("写入返回缺少可核验的事件 ID 或原文来源关联；请检查项目记录，勿重复确认成功。原文已归档。");
      }
      const decisionId = captureType === "decision"
        ? event.evidenceRefs.find((reference) => reference.startsWith("decision:"))
        : undefined;
      setExternalAiFeedback(
        `已写入${externalAiKind === "progress" ? "工作进展" : externalAiKind === "decision" ? "用户确认决定" : "用户确认下一步"}。真实 Work Ledger 事件 ID：${event.id}${decisionId ? `；DecisionRecord ID：${decisionId.slice("decision:".length)}` : ""}。来源文件：${sourceFile.fileName}（${sourceFile.id}）。`,
      );
    } catch (err) {
      setExternalAiFeedback(`未能确认写入成功：${err instanceof Error ? err.message : String(err)}${archivedSourceFile ? ` 原文来源仍已归档：${archivedSourceFile.fileName}（${archivedSourceFile.id}）。` : ""}`);
    } finally {
      setIsSavingExternalAiReturn(false);
    }
  }

  function handleExternalAiTextChange(value: string) {
    setExternalAiText(value);
    setExternalAiSelection("");
    setExternalAiSourceFile(null);
    setExternalAiFeedback("");
    setExternalAiDraftProjectId(activeProject?.id ?? "");
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
  const selectedManagedFile = managedFiles.find((file) => file.id === selectedManagedFileId) ?? null;
  const conversationMessages = useConversationMessages(workspaceMessages, activeProject.description);
  const codexDiscussionDraft = useMemo(() => buildCodexDraftFromDiscussion(conversationMessages), [conversationMessages]);
  const visibleConversationMessages = useMemo(
    () => latestWorkspaceItems(conversationMessages, conversationMessageLimit),
    [conversationMessages, conversationMessageLimit],
  );
  const hiddenConversationMessageCount = conversationMessages.length - visibleConversationMessages.length;
  const visibleManagedFiles = useMemo(
    () => visibleReferenceFiles(managedFiles, selectedManagedFile?.id ?? null, referenceFileLimit),
    [managedFiles, selectedManagedFile?.id, referenceFileLimit],
  );
  const hiddenManagedFileCount = managedFiles.length - visibleManagedFiles.length;

  const isExternalAiReturnPanelOpen = showExternalAiReturnPanel && externalAiDraftProjectId === activeProject?.id;
  const isTopPanelMode = showSearchPanel || showCodexPanel || showProjectContextPanel || showWorkCapturePanel || isExternalAiReturnPanelOpen;
  const isReportMode = showWeeklyReview || showProjectImpact;
  const pendingAssistantMessage = [...workspaceMessages].reverse().find(
    (message) => message.author !== "user" && !pendingAssistantBaselineRef.current.has(message.id),
  );
  const isCurrentReplyStreaming = Boolean(
    isStreaming && pendingAssistantMessage && !TERMINAL_CHAT_MESSAGE_STATUSES.has(pendingAssistantMessage.status),
  );
  const isChatGenerationActive = isSending || isCurrentReplyStreaming;
  const needsProjectAiAuthorization = Boolean(
    !activeManifest?.deepseekAuthorization?.grantedAt ||
    activeManifest.deepseekAuthorization.scopeVersion !== DEEPSEEK_AUTHORIZATION_SCOPE_VERSION,
  );
  const needsAiConfiguration = deepSeekSettingsLoaded && !deepSeekSettings.hasApiKey;
  const isProjectAiAuthorizationBusy =
    projectAiAuthorizationStatus === "saving" || projectAiAuthorizationStatus === "refreshing";
  const hasUnsavedWorkspaceInput = Boolean(
    isCodexPromptDirty || codexReportInput.trim() || decisionText.trim() || decisionReason.trim() ||
    memoDraft.trim() ||
    (showWorkCapturePanel && (captureContent.trim() || captureReason.trim())) ||
    (externalAiText.trim() && !externalAiSourceFile) || isSavingExternalAiReturn,
  );
  const projectSwitchRestriction = !isCodexRunStateReady
    ? codexRunStateFailure
      ? "无法确认本项目的 Codex 运行状态，检查成功前暂不能切换。"
      : "正在确认本项目的 Codex 运行状态，请稍候。"
    : isProjectAiAuthorizationBusy
      ? "正在保存或更新本项目的 AI 授权，请完成后再切换。"
      : hasActiveCodexRun || isStartingCodexRun || isCancellingCodexRun
        ? "Codex 任务正在启动或运行，结束后再切换项目。"
        : isChatGenerationActive
          ? "AI 正在回复，结束或停止回复后再切换项目。"
          : isImporting
            ? "资料正在导入，完成后再切换项目。"
            : pendingImages.length > 0
              ? "还有待发送图片；请先发送或移除图片再切换项目。"
              : isListening
                ? "语音输入正在进行，请先结束语音输入再切换项目。"
                : isWrapping || isFinishing
                  ? "收工记录正在编辑或保存，请先完成或取消收工操作。"
                  : isGeneratingCodexPrompt || isImportingCodexReport || isApplyingCodexReport || isUpdatingCodexTask
                    ? "Codex 任务正在处理中，请完成当前操作后再切换项目。"
                    : isRecordingDecision || isCapturingFact
                      ? "项目记录正在保存，请完成后再切换项目。"
                      : hasUnsavedWorkspaceInput
                        ? "当前页面有尚未保存的输入，请先保存、提交或清空后再切换。"
                        : "";
  const normalizedProjectSearch = projectSearch.trim().toLocaleLowerCase();
  const matchingProjects = useMemo(() => {
    if (!normalizedProjectSearch) return projects;
    return projects.filter((item) =>
      `${item.name}\n${item.rootDir}\n${item.description}`.toLocaleLowerCase().includes(normalizedProjectSearch),
    );
  }, [projects, normalizedProjectSearch]);
  const isChatThinking = shouldShowChatThinkingIndicator({
    requestPending: isSending,
    streamActive: isCurrentReplyStreaming,
    assistantStatus: pendingAssistantMessage?.status,
    assistantText: pendingAssistantMessage?.text,
    stopRequested: chatReplyStopRequested,
    nonConversationMode: isReportMode || isTopPanelMode,
  });

  useEffect(() => {
    if (isChatThinking && shouldFollowBottomRef.current) {
      endRef.current?.scrollIntoView?.({ block: "end", behavior: "smooth" });
    }
  }, [isChatThinking]);

  useEffect(() => {
    if (!isProjectSwitcherOpen) return;
    projectSwitcherInputRef.current?.focus();
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape" && !isSwitchingProject) setIsProjectSwitcherOpen(false);
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [isProjectSwitcherOpen, isSwitchingProject]);

  function openProjectSwitcher() {
    setProjectSearch("");
    setProjectSwitchFeedback("");
    setIsProjectSwitcherOpen(true);
  }

  async function switchWorkspaceProject(target: ProjectSummary) {
    if (!activeProject) return;
    if (target.id === activeProject.id) {
      if (!isSwitchingProject) setIsProjectSwitcherOpen(false);
      return;
    }
    if (projectSwitchingRef.current || projectSwitchRestriction) return;
    projectSwitchingRef.current = true;
    setProjectSwitchFeedback("");
    setIsSwitchingProject(true);
    try {
      // Flush the debounce-backed composer draft before changing the active project.
      await saveProjectDraft(draft);
      await openProject(target.rootDir);
      setIsProjectSwitcherOpen(false);
      setProjectSearch("");
    } catch (err) {
      setProjectSwitchFeedback(`切换项目失败，当前项目未更改：${err instanceof Error ? err.message : String(err)}`);
    } finally {
      projectSwitchingRef.current = false;
      setIsSwitchingProject(false);
    }
  }

  async function retryCodexRunStateCheck() {
    if (isRetryingCodexRunState) return;
    setIsRetryingCodexRunState(true);
    try {
      await refreshCodexRuns();
    } catch (err) {
      setCodexError(String(err));
    } finally {
      setIsRetryingCodexRunState(false);
    }
  }

  useLayoutEffect(() => {
    const returnedToConversation = previousTopPanelModeRef.current && !isTopPanelMode;
    previousTopPanelModeRef.current = isTopPanelMode;
    if (!returnedToConversation) return;

    const thread = threadRef.current;
    if (!thread) return;
    thread.scrollTop = resolveConversationScrollTop({
      scrollHeight: thread.scrollHeight,
      clientHeight: thread.clientHeight,
      previousScrollTop: previousThreadScrollTopRef.current,
      followBottom: shouldFollowBottomRef.current,
    });
    previousThreadScrollTopRef.current = thread.scrollTop;
  }, [isTopPanelMode]);

  return (
    <AppLayout className={`work-shell workspace-shell${sidebarOpen ? " left-sidebar-open" : ""}${isTopPanelMode ? " panel-mode" : ""}${isReportMode ? " report-mode" : ""}`}>
      <button
        type="button"
        className="sidebar-peek"
        onClick={() => setSidebarOpen((value) => !value)}
        aria-label="展开侧边栏"
      >
        {sidebarOpen ? "←" : "→"}
      </button>

      <aside className={`start-sidebar work-sidebar ${sidebarOpen ? "open" : ""}`}>
        {sidebarOpen ? <>
          <SidebarSection title="最近项目">
          {projects.length ? (
            projects.slice(0, 5).map((project) => (
              <button
                key={project.id}
                type="button"
                className="sidebar-item"
                disabled={Boolean(projectSwitchRestriction) || isSwitchingProject}
                title={projectSwitchRestriction || undefined}
                onClick={() => void switchWorkspaceProject(project)}
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
        </> : null}
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
          <span title={activeProject.rootDir}>{activeProject.rootDir}</span>
        </div>
        <div className="workspace-top-actions">
          <div className="workspace-top-actions-main">
            <button
              type="button"
              className={`btn ${!isTopPanelMode && !isReportMode ? "btn-secondary" : "btn-ghost"}`}
              aria-pressed={!isTopPanelMode && !isReportMode}
              onClick={returnToConversation}
            >
              项目对话
            </button>
            <button
              type="button"
              className={`btn ${showCodexPanel ? "btn-primary" : "btn-secondary"}`}
              aria-pressed={showCodexPanel}
              onClick={() => {
                setShowSearchPanel(false);
                setShowProjectContextPanel(false);
                setShowWorkCapturePanel(false);
                setShowExternalAiReturnPanel(false);
                if (showCodexPanel) {
                  returnToConversation();
                } else {
                  setShowCodexPanel(true);
                  setSearchParams((previous) => {
                    const next = new URLSearchParams(previous);
                    next.set("panel", "codex");
                    next.delete("actionId");
                    return next;
                  });
                }
              }}
            >
              Codex 任务
            </button>
            <details className="workspace-more-actions" open={showWeeklyReview || showProjectImpact}>
              <summary className={`btn ${showProjectContextPanel || showWorkCapturePanel || isExternalAiReturnPanelOpen || showSearchPanel || showWeeklyReview || showProjectImpact ? "btn-secondary" : "btn-ghost"}`}>
                更多工具
              </summary>
              <div className="workspace-more-actions-menu">
                <button
                  type="button"
                  className={`btn ${showSearchPanel ? "btn-primary" : "btn-ghost"}`}
                  aria-pressed={showSearchPanel}
                  onClick={() => {
                    const nextOpen = !showSearchPanel;
                    setShowCodexPanel(false);
                    setShowProjectContextPanel(false);
                    setShowWorkCapturePanel(false);
                    setShowExternalAiReturnPanel(false);
                    setShowSearchPanel(nextOpen);
                    setSearchParams((previous) => {
                      const next = new URLSearchParams(previous);
                      next.delete("panel");
                      next.delete("actionId");
                      return next;
                    });
                  }}
                >
                  全局搜索
                </button>
                <button
                  type="button"
                  className={`btn ${showProjectContextPanel ? "btn-primary" : "btn-ghost"}`}
                  aria-pressed={showProjectContextPanel}
                  disabled={isLoadingProjectContext}
                  onClick={() => void handleOpenProjectContext()}
                >
                  项目上下文
                </button>
                <button
                  type="button"
                  className="btn btn-ghost"
                  disabled={isLoadingProjectContext || isCopyingProjectContext}
                  aria-live="polite"
                  onClick={() => void handleQuickCopyProjectContext()}
                >
                  {isCopyingProjectContext ? "正在复制…" : hasCopiedProjectContext ? "已复制项目上下文" : "复制项目上下文"}
                </button>
                <button
                  type="button"
                  className={`btn ${showWorkCapturePanel ? "btn-primary" : "btn-ghost"}`}
                  aria-pressed={showWorkCapturePanel}
                  onClick={() => {
                    if (showWorkCapturePanel) {
                      closeWorkCapturePanel();
                      return;
                    }
                    setShowSearchPanel(false);
                    setShowCodexPanel(false);
                    setShowProjectContextPanel(false);
                    setShowExternalAiReturnPanel(false);
                    setShowWorkCapturePanel(true);
                    setCaptureFeedback("");
                    setSearchParams((previous) => {
                      const next = new URLSearchParams(previous);
                      next.set("panel", "work-capture");
                      return next;
                    });
                  }}
                >
                  记录工作
                </button>
                <button
                  type="button"
                  className={`btn ${isExternalAiReturnPanelOpen ? "btn-primary" : "btn-ghost"}`}
                  aria-pressed={isExternalAiReturnPanelOpen}
                  onClick={() => isExternalAiReturnPanelOpen ? closeExternalAiReturnPanel() : openExternalAiReturnPanel()}
                >
                  外部 AI 回流
                </button>
                <button
                  type="button"
                  className={`btn ${showWeeklyReview ? "btn-primary" : "btn-ghost"}`}
                  aria-pressed={showWeeklyReview}
                  onClick={() => {
                    setShowSearchPanel(false);
                    setShowCodexPanel(false);
                    setShowProjectContextPanel(false);
                    setShowWorkCapturePanel(false);
                    setShowExternalAiReturnPanel(false);
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
                  aria-pressed={showProjectImpact}
                  onClick={() => {
                    setShowSearchPanel(false);
                    setShowCodexPanel(false);
                    setShowProjectContextPanel(false);
                    setShowWorkCapturePanel(false);
                    setShowExternalAiReturnPanel(false);
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
                <button type="button" className="btn btn-ghost" onClick={startWorkCloseout}>
                  收工并保存进度
                </button>
              </div>
            </details>
          </div>
          <div className="workspace-top-actions-side">
            <button type="button" className="btn btn-ghost workspace-files-toggle" aria-expanded={rightSidebarOpen} onClick={() => setRightSidebarOpen((value) => !value)}>
              {rightSidebarOpen ? "收起侧栏" : "项目资料"}
            </button>
            <button type="button" className="btn btn-ghost" onClick={openProjectSwitcher}>
              切换项目
            </button>
            <button
              type="button"
              className="icon-btn"
              title="设置"
              onClick={() => navigate("/settings", { state: { returnTo: `${location.pathname}${location.search}` } })}
              aria-label="设置"
            >
              ⚙
            </button>
          </div>
        </div>
      </header>

      {isProjectSwitcherOpen ? (
        <div className="project-switcher-overlay">
          <button
            type="button"
            className="project-switcher-backdrop"
            aria-label="关闭项目选择器"
            disabled={isSwitchingProject}
            onClick={() => setIsProjectSwitcherOpen(false)}
          />
          <section className="project-switcher-dialog" role="dialog" aria-modal="true" aria-labelledby="project-switcher-title">
            <header className="project-switcher-header">
              <div>
                <span className="section-label">工作台</span>
                <h2 id="project-switcher-title">切换项目</h2>
              </div>
              <button type="button" className="btn btn-ghost" disabled={isSwitchingProject} onClick={() => setIsProjectSwitcherOpen(false)}>
                取消
              </button>
            </header>
            <label className="sr-only" htmlFor="project-switcher-search">搜索已登记项目</label>
            <input
              ref={projectSwitcherInputRef}
              id="project-switcher-search"
              className="project-switcher-search"
              type="search"
              placeholder="搜索全部已登记项目…"
              value={projectSearch}
              disabled={isSwitchingProject}
              onChange={(event) => setProjectSearch(event.target.value)}
            />
            {projectSwitchRestriction ? (
              <div className="project-switcher-notice-row">
                <p className="project-switcher-notice" role="status">{projectSwitchRestriction}</p>
                {codexRunStateFailure ? (
                  <button type="button" className="btn btn-ghost" disabled={isRetryingCodexRunState} onClick={() => void retryCodexRunStateCheck()}>
                    {isRetryingCodexRunState ? "正在重试…" : "重试检查"}
                  </button>
                ) : null}
              </div>
            ) : null}
            {projectSwitchFeedback ? (
              <p className="project-switcher-error" role="alert">{projectSwitchFeedback}</p>
            ) : null}
            <div className="project-switcher-list" aria-label="已登记项目">
              {matchingProjects.length ? matchingProjects.map((item) => {
                const isCurrent = item.id === activeProject.id;
                return (
                  <button
                    key={item.id}
                    type="button"
                    className="project-switcher-item"
                    disabled={isSwitchingProject || (!isCurrent && Boolean(projectSwitchRestriction))}
                    aria-current={isCurrent ? "page" : undefined}
                    onClick={() => void switchWorkspaceProject(item)}
                  >
                    <span className="project-switcher-item-copy">
                      <strong>{item.name}</strong>
                      <small title={item.rootDir}>{item.rootDir}</small>
                    </span>
                    <span className="project-switcher-item-state">
                      {isSwitchingProject && !isCurrent ? "正在切换…" : isCurrent ? "当前项目" : "打开"}
                    </span>
                  </button>
                );
              }) : (
                <p className="project-switcher-empty">没有找到匹配的已登记项目。</p>
              )}
            </div>
          </section>
        </div>
      ) : null}

      {showSearchPanel || showCodexPanel || showProjectContextPanel || showWorkCapturePanel || isExternalAiReturnPanelOpen ? (
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
            {isExternalAiReturnPanelOpen ? (
              <ExternalAiReturnPanel
                key={activeProject.id}
                text={externalAiText}
                selection={externalAiSelection}
                kind={externalAiKind}
                sourceFile={externalAiSourceFile}
                isSaving={isSavingExternalAiReturn}
                feedback={externalAiFeedback}
                onTextChange={handleExternalAiTextChange}
                onSelectParagraph={(value) => {
                  setExternalAiSelection(value);
                  setExternalAiFeedback("");
                }}
                onSelectionChange={setExternalAiSelection}
                onKindChange={setExternalAiKind}
                onSaveSuggestion={() => void handleExternalAiReturn(true)}
                onConfirm={() => void handleExternalAiReturn(false)}
                onClose={closeExternalAiReturnPanel}
              />
            ) : null}
            {showCodexPanel ? (
              <CodexCollaborationPanel
                promptText={codexPromptText}
                discussionDraft={codexDiscussionDraft}
                onPromptChange={(text) => {
                  setCodexPromptText(text);
                  setIsCodexPromptDirty(true);
                }}
                onCreateTask={handleCreateCodexTask}
                onCopyPrompt={handleCopyCodexPrompt}
                onRebindTaskPrompt={handleRebindCodexPrompt}
                onStartTaskRun={handleStartCodexRun}
                onCancelTaskRun={handleCancelCodexRun}
                onAcceptTask={handleAcceptCodexTask}
                onRejectTask={handleRejectCodexTask}
                onSelectTask={selectCodexTask}
                onRefreshCodexStatus={handleRefreshCodexStatus}
                resultCheckFeedback={codexResultCheckFeedback}
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
                pinnedTaskId={pinnedCodexTaskId}
                actionError={codexAcceptanceError}
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
                isCreatingTask={isGeneratingCodexPrompt}
                isImportingReport={isImportingCodexReport}
                isApplyingReport={isApplyingCodexReport}
                isUpdatingTask={isUpdatingCodexTask}
                isStartingRun={isStartingCodexRun}
                startRunError={codexRunStartError}
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
            if (isTopPanelMode || isReportMode) return;
            previousThreadScrollTopRef.current = event.currentTarget.scrollTop;
            shouldFollowBottomRef.current = isNearBottom(event.currentTarget);
          }}
        >
          <div className="thread-inner">
            {error ? <div className="restore-note">{error}</div> : null}
            {!isReportMode ? (
              <div className="restore-note">
                <span>项目恢复信息</span>
                <strong>{confirmedNextStep || "暂无用户确认的下一步"}</strong>
              </div>
            ) : null}

            {showWeeklyReview ? (
              <section className="restore-note weekly-review-note">
                <div className="weekly-review-hero">
                  <div className="wrap-inline-head">
                    <span className="section-label">本周复盘</span>
                    <strong>{weeklyReviewDashboard?.generatedAt ? formatLocalDateTime(weeklyReviewDashboard.generatedAt) : "等待生成"}</strong>
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

            {!isReportMode && conversationMessages.length ? (
              <>
                {hiddenConversationMessageCount ? (
                  <div className="thread-history-control">
                    <Button type="button" variant="ghost" onClick={() => setConversationMessageLimit((limit) => limit + CONVERSATION_MESSAGE_INCREMENT)}>
                      加载更早的 {Math.min(hiddenConversationMessageCount, CONVERSATION_MESSAGE_INCREMENT)} 条记录
                    </Button>
                  </div>
                ) : null}
                {visibleConversationMessages.map((message) => (
                  <Message
                    key={message.id}
                    message={message}
                    projectRoot={activeProject.rootDir}
                    readChatAttachment={readChatAttachment}
                    openFilePath={openFilePath}
                    openFolderPath={openFolderPath}
                    onRetry={retryProjectMessage}
                    readOnlyRecovery={Boolean(activeManifest?.recoveredFromBackup)}
                  />
                ))}
              </>
            ) : !isReportMode ? (
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
            ) : null}

            {isWrapping ? (
              <section className="wrap-inline">
                <div className="wrap-inline-head">
                  <span className="section-label">收工</span>
                  <strong>保存项目恢复点</strong>
                </div>
                <p className="codex-panel-hint">
                  {closeoutEvidenceCount > 0
                    ? "已从本项目的用户进展、已验收 Codex 结果和 Git 记录整理草稿，可编辑后保存；普通聊天和 AI 建议不会自动计为事实。"
                    : "没有找到本轮可自动整理的已核验进展；聊天仍会保留。你可以补充本次完成内容，或先继续工作。"}
                </p>
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
            {isChatThinking ? (
              <div className="chat-thinking-indicator" role="status" aria-label="感冒院正在回复">
                <span aria-hidden="true" />
                <span aria-hidden="true" />
                <span aria-hidden="true" />
              </div>
            ) : null}
            <div ref={endRef} />
          </div>
        </section>

        <aside className={`workspace-sidebar${rightSidebarOpen ? "" : " hidden"}`}>
          <div className="workspace-sidebar-inner">
            <WorkspaceActivityPanel
              projectAnalysis={projectAnalysis}
              pendingReviews={pendingReviews}
              workLedger={workLedger}
              activeNextStep={activeManifest?.project.nextStep ?? ""}
              activeTasks={tasks}
              needsAuthorization={needsProjectAiAuthorization}
              authorizationStatus={projectAiAuthorizationStatus}
              authorizationFeedback={projectAiAuthorizationFeedback}
              authorizationBusy={isProjectAiAuthorizationBusy}
              needsAiConfiguration={needsAiConfiguration}
              onOpenAiSettings={openAiModelSettings}
              onGrantAuthorization={grantProjectDeepSeekAuthorization}
            />
            <div className="workspace-sidebar-head workspace-reference-head">
              <span className="section-label">当前参考资料</span>
              <strong>{managedFiles.length ? `${managedFiles.length} 个已登记` : "暂无资料"}</strong>
              <p>只展示速览，详细内容在弹层中查看。</p>
            </div>
            <div className="workspace-sidebar-list">
              {managedFiles.length ? (
                <>
                  <div className="workspace-file-list">
                    {visibleManagedFiles.map((file) => {
                      const category = file.recommendedCategory || file.category || "未分类";
                      const status = file.parseStatus === "success" ? "可用" : file.parseStatus === "failed" ? "待检查" : "处理中";
                      const summary = file.parseStatus === "failed"
                        ? file.parseFailureReason || "解析失败，等待人工检查。"
                        : file.contentSummary || "暂无摘要。";
                      return (
                        <article
                          key={file.id}
                          className={`workspace-reference-card${selectedManagedFile?.id === file.id ? " active" : ""}`}
                        >
                          <div className="workspace-reference-card-head">
                            <strong title={file.fileName}>{file.fileName}</strong>
                            <span className="workspace-reference-tag">{category}</span>
                          </div>
                          <span className="workspace-reference-meta">
                            {status}{file.fileType ? ` · ${file.fileType}` : ""}
                          </span>
                          <p className="workspace-reference-summary" title={summary}>{summary}</p>
                          <div className="workspace-reference-actions">
                            <button
                              type="button"
                              className="btn btn-ghost"
                              onClick={() => {
                                setSelectedManagedFileId(file.id);
                                setIsReferenceFileDialogOpen(true);
                              }}
                            >
                              展开详情
                            </button>
                            <button type="button" className="btn btn-ghost" onClick={() => void openFilePath(file.managedPath).catch(() => undefined)}>
                              打开文件
                            </button>
                            <button type="button" className="btn btn-ghost" onClick={() => void openFolderPath(file.managedPath).catch(() => undefined)}>
                              打开所在位置
                            </button>
                          </div>
                        </article>
                      );
                    })}
                    {hiddenManagedFileCount ? (
                      <Button type="button" variant="ghost" onClick={() => setReferenceFileLimit((limit) => limit + REFERENCE_FILE_INCREMENT)}>
                        显示更多文件（还剩 {hiddenManagedFileCount} 个）
                      </Button>
                    ) : null}
                  </div>
                </>
              ) : (
                <div className="workspace-sidebar-empty">暂无项目文件。</div>
              )}
            </div>
          </div>
        </aside>
      </section>

      {isReferenceFileDialogOpen && selectedManagedFile ? (
        <ReferenceFileDialog
          file={selectedManagedFile}
          onClose={() => setIsReferenceFileDialogOpen(false)}
          openFilePath={openFilePath}
          openFolderPath={openFolderPath}
        />
      ) : null}

      <footer className={`workspace-composer${rightSidebarOpen ? "" : " sidebar-hidden"}`}>
        <div className="composer-inner">
          {(needsProjectAiAuthorization || needsAiConfiguration || projectAiAuthorizationStatus !== "idle") ? (
            <div className="composer-context-strip">
              {needsProjectAiAuthorization ? (
                <button
                  type="button"
                  disabled={isProjectAiAuthorizationBusy}
                  aria-busy={isProjectAiAuthorizationBusy}
                  onClick={() => void grantProjectDeepSeekAuthorization()}
                >
                  {isProjectAiAuthorizationBusy ? "正在启用 AI 回复…" : "启用 AI 回复"}
                </button>
              ) : null}
              {needsAiConfiguration ? (
                <>
                  <span>未配置 AI 连接</span>
                  <button type="button" onClick={openAiModelSettings}>配置 AI</button>
                </>
              ) : null}
              {projectAiAuthorizationFeedback ? (
                <span className={`project-authorization-feedback status-${projectAiAuthorizationStatus}`} role="status" aria-live="polite">
                  {projectAiAuthorizationFeedback}
                </span>
              ) : (
                <span>先确认项目资料与聊天记录的发送范围；旧授权需重新确认</span>
              )}
            </div>
          ) : null}
          {pendingImages.length ? (
            <div className="composer-image-previews" aria-label="待发送图片">
              {pendingImages.map((image) => (
                <figure key={image.id} className="composer-image-preview">
                  <img src={image.dataUrl} alt={image.fileName} />
                  <figcaption title={image.fileName}>{image.fileName}</figcaption>
                  <button type="button" className="composer-image-remove" aria-label={`移除 ${image.fileName}`} onClick={() => setPendingImages((current) => current.filter((item) => item.id !== image.id))}>
                    ×
                  </button>
                </figure>
              ))}
              <span className="composer-image-note">
                {supportsVisionModel(deepSeekSettings.selectedModelId)
                  ? "图片会随本条消息发送给视觉模型。"
                  : "当前模型不支持视觉，仅保存本条图片附件，不会假装看图。"}
              </span>
            </div>
          ) : null}
          <input
            ref={imageInputRef}
            className="sr-only"
            aria-label="选择要附加到消息的图片"
            tabIndex={-1}
            type="file"
            accept="image/png,image/jpeg,image/webp"
            multiple
            onChange={(event) => {
              const files = Array.from(event.target.files ?? []);
              event.target.value = "";
              void addChatImages(files);
            }}
          />
          <div
            className={`composer-row${isImageDragActive ? " image-drag-active" : ""}`}
            onPaste={handleImagePaste}
            onDragOver={(event) => {
              if (Array.from(event.dataTransfer.items).some((item) => item.kind === "file" && item.type.startsWith("image/"))) {
                event.preventDefault();
                setIsImageDragActive(true);
              }
            }}
            onDragLeave={() => setIsImageDragActive(false)}
            onDrop={handleImageDrop}
          >
            <button type="button" className="btn composer-file-button" disabled={isImporting} onClick={() => void addFiles()}>
              {isImporting ? "正在处理..." : "添加资料"}
            </button>
            <button type="button" className="btn composer-image-button" onClick={() => imageInputRef.current?.click()}>
              图片
            </button>
            <textarea
              rows={2}
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
              {isListening ? "停止语音" : "语音"}
            </button>
            <div className="composer-actions">
              <button
                type="button"
                className="btn btn-primary composer-send"
                disabled={!draft.trim() || isChatGenerationActive}
                onClick={() => void submitMessage()}
              >
                {isChatGenerationActive ? "生成中..." : "发送"}
              </button>
              {isChatGenerationActive ? (
                <button type="button" className="btn composer-stop" disabled={chatReplyStopRequested} onClick={() => void stopCurrentChatReply()}>
                  {chatReplyStopRequested ? "正在停止..." : "停止"}
                </button>
              ) : null}
            </div>
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
          {packet.readWarnings.length ? (
            <ContextPacketList title="读取限制">
              {packet.readWarnings.map((warning) => <li key={warning}>{warning}</li>)}
            </ContextPacketList>
          ) : null}
          <section className="project-context-section">
            <span className="section-label">用户确认的下一步</span>
            <strong>{packet.nextStep || "暂无用户确认的下一步"}</strong>
            {packet.focus?.title === packet.nextStep && packet.nextStep ? (
              <p>{packet.focus.summary}</p>
            ) : null}
          </section>
          {packet.focus && packet.focus.title !== packet.nextStep ? (
            <section className="project-context-section project-context-focus">
              <span className="section-label">当前焦点</span>
              <strong>{packet.focus.title}</strong>
              <p>{packet.focus.summary}</p>
              {packet.focus.evidence.length ? <small>依据：{packet.focus.evidence.join("；")}</small> : null}
            </section>
          ) : !packet.focus ? (
            <section className="project-context-section project-context-focus">
              <span className="section-label">当前焦点</span>
              <strong>暂无可优先执行的事项</strong>
              <p>文件和目录提醒仍保留在“待你处理”中，不会自动成为下一步。</p>
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
          <ContextPacketList title="项目理由" className="project-context-rationale-list">
            {packet.rationaleContext.entries.length ? (
              packet.rationaleContext.entries.map((entry) => (
                <li className="project-context-rationale" key={`${entry.sourceFile}-${entry.lineStart}-${entry.title}`}>
                  <strong>{entry.title}</strong>
                  <div className="project-context-rationale-meta">
                    <span>类型：{entry.types.join(" / ") || "未分类"}</span>
                    <span>状态：{rationaleStatusLabel(entry.status)}</span>
                    <span>依据：{rationaleEvidenceLabel(entry.evidence)}</span>
                    {entry.unresolvedConflict ? <span>同题记录冲突，未自动裁定</span> : null}
                  </div>
                  {entry.reason ? <p className="project-context-rationale-text"><strong>理由</strong>{entry.reason}</p> : null}
                  {entry.rejectedAlternatives.length ? (
                    <div className="project-context-rationale-notes">
                      {entry.rejectedAlternatives.map((alternative) => (
                        <p className="project-context-rationale-text" key={alternative}>
                          <strong>曾否决</strong>{alternative}
                        </p>
                      ))}
                    </div>
                  ) : null}
                  <div className="project-context-rationale-evidence">
                    <small>来源：{entry.sourceFile}:L{entry.lineStart}-L{entry.lineEnd}</small>
                    {entry.verifiedLinks.map((link) => <small key={link}>关联依据：{link}</small>)}
                  </div>
                </li>
              ))
            ) : (
              <li>
                <span>
                  {packet.rationaleContext.status === "invalid" || packet.rationaleContext.status === "unavailable"
                    ? "理由来源当前不可用；不会从 Git 或 AI 分析补出原因。"
                    : "当前项目没有 Keep the Why 理由记录；可在项目 AI 对话中查询现有决定，但不会猜测决定动机。"}
                </span>
              </li>
            )}
            {packet.rationaleContext.warnings.map((warning) => <li key={warning}><span>{warning}</span></li>)}
          </ContextPacketList>
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

function rationaleStatusLabel(status: string) {
  const labels: Record<string, string> = {
    active: "当前记录",
    superseded: "历史已替代",
    open: "尚无结论",
    "needs-review": "需要复核",
    "pending-confirmation": "待确认",
  };
  return labels[status] ?? status;
}

function rationaleEvidenceLabel(evidence: string) {
  const labels: Record<string, string> = {
    confirmed: "记录标注为已确认",
    inferred: "记录标注为推断",
    unknown: "证据未知",
  };
  return labels[evidence] ?? evidence;
}

function ContextPacketList({ title, children, className = "" }: { title: string; children: ReactNode; className?: string }) {
  return (
    <section className="project-context-section">
      <span className="section-label">{title}</span>
      <ul className={`project-context-list${className ? ` ${className}` : ""}`}>{children}</ul>
    </section>
  );
}

function ExternalAiReturnPanel({
  text,
  selection,
  kind,
  sourceFile,
  isSaving,
  feedback,
  onTextChange,
  onSelectParagraph,
  onSelectionChange,
  onKindChange,
  onSaveSuggestion,
  onConfirm,
  onClose,
}: {
  text: string;
  selection: string;
  kind: ExternalAiReturnKind;
  sourceFile: ManagedFile | null;
  isSaving: boolean;
  feedback: string;
  onTextChange: (value: string) => void;
  onSelectParagraph: (value: string) => void;
  onSelectionChange: (value: string) => void;
  onKindChange: (value: ExternalAiReturnKind) => void;
  onSaveSuggestion: () => void;
  onConfirm: () => void;
  onClose: () => void;
}) {
  const paragraphs = externalAiParagraphs(text);
  const confirmLabel = kind === "progress"
    ? "确认并记录已核实进展"
    : kind === "decision"
      ? "确认并记录为我的决定"
      : "确认并设为项目下一步";

  return (
    <section className="project-context-panel work-capture-panel" aria-labelledby="external-ai-return-title">
      <header className="project-context-head">
        <div>
          <span className="section-label">外部 AI · 用户确认后才写入</span>
          <h2 id="external-ai-return-title">带回外部讨论结果</h2>
          <p>粘贴 ChatGPT 等外部 AI 的回复。原文会作为未验证来源保存；不会自动变成事实、决定、下一步或 Codex 验收结果。</p>
        </div>
        <div className="project-context-actions">
          <Button type="button" variant="ghost" disabled={isSaving} onClick={onClose}>关闭</Button>
        </div>
      </header>

      <div className="work-capture-sheet">
        <label className="work-capture-field">
          <span>外部 AI 回复原文</span>
          <textarea
            rows={7}
            maxLength={MAX_EXTERNAL_AI_SOURCE_LENGTH}
            value={text}
            disabled={isSaving}
            onChange={(event) => onTextChange(event.target.value)}
            placeholder="在这里粘贴外部 AI 的回复……"
          />
          <small>{text.length.toLocaleString()} / {MAX_EXTERNAL_AI_SOURCE_LENGTH.toLocaleString()} 字符；内容只保存在当前项目，不会自动发送给 AI。</small>
        </label>

        {paragraphs.length ? (
          <section className="project-context-section" aria-label="选择要保留的内容">
            <span className="section-label">从原文选择一段</span>
            <div className="external-ai-paragraph-list">
              {paragraphs.map((paragraph, index) => (
                <button
                  key={`${index}-${paragraph.slice(0, 32)}`}
                  type="button"
                  className={`btn ${selection === paragraph ? "btn-secondary" : "btn-ghost"}`}
                  disabled={isSaving}
                  aria-pressed={selection === paragraph}
                  onClick={() => onSelectParagraph(paragraph)}
                >
                  {paragraph.length > 220 ? `${paragraph.slice(0, 220)}…` : paragraph}
                </button>
              ))}
            </div>
          </section>
        ) : null}

        <label className="work-capture-field">
          <span>准备保留的结论（可在确认前编辑）</span>
          <textarea
            rows={3}
            maxLength={kind === "decision" ? 2_000 : 240}
            value={selection}
            disabled={isSaving}
            onChange={(event) => onSelectionChange(event.target.value)}
            placeholder="先粘贴原文，再从上方选择一段内容。"
          />
          <small>{selection.length} / {kind === "decision" ? "2,000" : "240"} 字符</small>
        </label>

        <label className="work-capture-field">
          <span>如何处理这段内容</span>
          <select
            value={kind}
            disabled={isSaving}
            onChange={(event) => onKindChange(event.target.value as ExternalAiReturnKind)}
          >
            <option value="suggestion">仅作为外部 AI 建议，不写入工作记录</option>
            <option value="progress">我已核实：记录为工作进展</option>
            <option value="decision">我确认采纳：记录为项目决定</option>
            <option value="nextAction">我确认：设为项目下一步</option>
          </select>
        </label>

        {sourceFile ? (
          <p className="section-label" role="note">原文来源已归档：{sourceFile.fileName} · {sourceFile.id}</p>
        ) : null}
        {feedback ? <Feedback tone={feedback.startsWith("已") ? "success" : "warning"} title="回流结果">{feedback}</Feedback> : null}
        <div className="work-capture-actions">
          <Button type="button" variant="secondary" loading={isSaving} disabled={!text.trim()} onClick={onSaveSuggestion}>
            仅归档外部 AI 原文
          </Button>
          <Button
            type="button"
            variant="primary"
            loading={isSaving}
            disabled={!text.trim() || !selection.trim() || kind === "suggestion"}
            onClick={onConfirm}
          >
            {confirmLabel}
          </Button>
        </div>
      </div>
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
  discussionDraft,
  onPromptChange,
  onCreateTask,
  onCopyPrompt,
  onRebindTaskPrompt,
  onStartTaskRun,
  onCancelTaskRun,
  onAcceptTask,
  onRejectTask,
  onSelectTask,
  onRefreshCodexStatus,
  resultCheckFeedback,
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
  pinnedTaskId,
  actionError,
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
  isCreatingTask,
  isImportingReport,
  isApplyingReport,
  isUpdatingTask,
  isStartingRun,
  startRunError,
  isCancellingRun,
}: {
  promptText: string;
  discussionDraft: CodexDiscussionDraft | null;
  onPromptChange: (value: string) => void;
  onCreateTask: (request: {
    title: string;
    taskType: "analysis" | "coding" | "verification" | "fileOperation";
    instructions: string;
  }) => Promise<boolean>;
  onCopyPrompt: () => Promise<void>;
  onRebindTaskPrompt: (taskId: string) => Promise<void>;
  onStartTaskRun: (taskId: string) => Promise<void>;
  onCancelTaskRun: (taskId: string) => Promise<void>;
  onAcceptTask: (taskId: string, resultId: string) => Promise<void>;
  onRejectTask: (taskId: string, resultId: string) => Promise<void>;
  onSelectTask: (taskId: string) => void;
  onRefreshCodexStatus: (taskId: string) => Promise<void>;
  resultCheckFeedback: ReturnType<typeof buildCodexResultCheckFeedback> | null;
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
  pinnedTaskId: string;
  actionError: string;
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
  isCreatingTask: boolean;
  isImportingReport: boolean;
  isApplyingReport: boolean;
  isUpdatingTask: boolean;
  isStartingRun: boolean;
  startRunError: string;
  isCancellingRun: boolean;
}) {
  const currentTask = selectCurrentCodexTask(codexTasks, codexRuns, targetTaskId, pinnedTaskId);
  const currentRun = currentTask ? runForCodexTask(codexRuns, currentTask) : null;
  const view = buildCodexTaskView(currentTask, currentRun);
  const currentTaskType = currentTask ? resolveCodexTaskDisplayType(currentTask) : "analysis";
  const [showCreateTaskForm, setShowCreateTaskForm] = useState(false);
  const [newTaskTitle, setNewTaskTitle] = useState(discussionDraft?.title ?? "");
  const [newTaskType, setNewTaskType] = useState<"analysis" | "coding" | "verification" | "fileOperation">(discussionDraft?.taskType ?? "analysis");
  const [newTaskInstructions, setNewTaskInstructions] = useState(discussionDraft?.instructions ?? "");
  const [historyVisibleCount, setHistoryVisibleCount] = useState(8);
  const sortedTaskHistory = useMemo(() => sortCodexTasksByRecency(codexTasks, codexRuns), [codexTasks, codexRuns]);

  function applyDiscussionDraft() {
    if (!discussionDraft) return;
    setNewTaskTitle(discussionDraft.title);
    setNewTaskType(discussionDraft.taskType);
    setNewTaskInstructions(discussionDraft.instructions);
  }

  async function submitCreateTask() {
    const created = await onCreateTask({
      title: newTaskTitle,
      taskType: newTaskType,
      instructions: newTaskInstructions,
    });
    if (!created) return;
    setNewTaskTitle("");
    setNewTaskType("analysis");
    setNewTaskInstructions("");
    setShowCreateTaskForm(false);
  }
  return (
    <section className="codex-panel codex-console" id="codex-console-top">
      <div className="codex-console-head">
        <div>
          <span className="section-label">Codex 协同</span>
          <strong>交给 Codex</strong>
          <p className="codex-panel-hint">查看 Codex 正在做什么、结果是否需要你确认。</p>
        </div>
        <Button
          type="button"
          variant="ghost"
          aria-label="生成新任务"
          disabled={isCreatingTask}
          onClick={() => {
            if (!showCreateTaskForm) applyDiscussionDraft();
            setShowCreateTaskForm((value) => !value);
          }}
        >
          新任务
        </Button>
      </div>

      {showCreateTaskForm ? (
        <form
          className="codex-technical-block"
          onSubmit={(event) => {
            event.preventDefault();
            void submitCreateTask();
          }}
        >
          <strong>新建 Codex 任务</strong>
          <p className="inline-notice">
            {discussionDraft
              ? "已根据当前项目最近一次讨论预填，可在创建前审阅和修改。"
              : "当前讨论还没有足够明确的可执行方案，请先补充需求。"}{" "}
            任务说明只会用于本次任务，不会自动拼接旧任务或其它项目上下文。
          </p>
          <label className="codex-field">
            <span>任务标题</span>
            <input aria-label="任务标题" value={newTaskTitle} maxLength={160} onChange={(event) => setNewTaskTitle(event.target.value)} placeholder="例如：验证 Codex 黄金闭环" />
          </label>
          <label className="codex-field">
            <span>任务类型</span>
            <select aria-label="任务类型" value={newTaskType} onChange={(event) => setNewTaskType(event.target.value as typeof newTaskType)}>
              <option value="analysis">分析</option>
              <option value="coding">代码修改</option>
              <option value="verification">验证</option>
              <option value="fileOperation">文件操作</option>
            </select>
          </label>
          <label className="codex-field">
            <span>任务说明</span>
            <textarea aria-label="任务说明" rows={5} value={newTaskInstructions} onChange={(event) => setNewTaskInstructions(event.target.value)} placeholder="说明需要完成什么、允许修改哪些文件，以及验收条件。" />
          </label>
          <div className="file-result-actions">
            <Button type="submit" variant="primary" disabled={!newTaskTitle.trim() || !newTaskInstructions.trim() || isCreatingTask} loading={isCreatingTask}>
              创建任务
            </Button>
            <Button type="button" variant="ghost" disabled={isCreatingTask} onClick={() => setShowCreateTaskForm(false)}>
              取消
            </Button>
          </div>
        </form>
      ) : null}

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
                <Button type="button" variant="secondary" disabled={isImportingReport} loading={isImportingReport} onClick={() => void onRefreshCodexStatus(currentTask.taskId)}>
                  重新检查结果
                </Button>
              ) : null}
              {view.secondaryActions.includes("reject") ? (
                <Button type="button" variant="danger" disabled={isUpdatingTask} onClick={() => void onRejectTask(currentTask.taskId, currentTask.resultId)}>
                  需要修改
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
          <Button type="button" variant="primary" disabled={isCreatingTask} onClick={() => setShowCreateTaskForm(true)}>
            生成 Codex 任务
          </Button>
        </section>
      )}

      {actionError ? <Feedback tone="error" title="验收未完成">{actionError}</Feedback> : null}
      {resultCheckFeedback ? (
        <Feedback tone={resultCheckFeedback.tone} title={resultCheckFeedback.title}>
          {resultCheckFeedback.message}
        </Feedback>
      ) : null}

      {currentRun && (currentRun.status === "starting" || currentRun.status === "running") ? (
        <CodexRunObservability run={currentRun} />
      ) : null}

      {startRunError ? <Feedback tone="error" title="无法启动本机 Codex">{startRunError}</Feedback> : null}

      <details className="codex-progress-panel">
        <summary className="codex-section-title">
          <span className="section-label">执行过程</span>
          <strong>{view.progressTitle}</strong>
        </summary>
        <div className="codex-steps" aria-label="Codex 任务生命周期">
          {codexTaskSteps(currentTask, currentRun).map((step) => (
            <div key={step.key} className={`codex-step ${step.state}`}>
              <span aria-hidden="true">{step.state === "done" ? "✓" : ""}</span>
              <em>{step.label}</em>
              {step.note ? <small>{step.note}</small> : null}
            </div>
          ))}
        </div>
      </details>

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
          {sortedTaskHistory.slice(0, historyVisibleCount).map((task) => (
            <details key={task.taskId} className="codex-task-row codex-history-row">
              <summary onClick={() => onSelectTask(task.taskId)}>
                <span className="codex-history-title">
                  <strong>{codexDisplayTaskTitle(task)}</strong>
                  <small className="codex-history-time">{codexTaskTimeSummary(task, latestRunForTask(codexRuns, task.taskId))}</small>
                </span>
                <small>{codexTaskTypeText(resolveCodexTaskDisplayType(task))}</small>
                <Badge tone={codexTaskStatusTone(task.status)}>{codexTaskStatusText(task.status)}</Badge>
                <small>{codexHistoryGitSummary(task)}</small>
                <small>{task.manualAcceptance?.length ? `${task.manualAcceptance.length} 项待验收` : "无需人工项"}</small>
              </summary>
              <p className="codex-history-outcome">{codexHistoryOutcome(task)}</p>
              <details className="codex-technical-detail">
                <summary>查看技术详情</summary>
                <p>taskId：{task.taskId}</p>
                <p>结果路径：{task.expectedResultPath || "未记录"}</p>
                <p>Git：{codexGitStatusText(task.gitVerification?.status)} {task.gitVerification?.reason || ""}</p>
              </details>
              <InsightBlock title="测试" items={task.tests} empty="未记录测试。" />
              <InsightBlock title="遗留问题" items={task.remainingIssues} empty="未记录遗留问题。" />
              <InsightBlock title="证据" items={task.evidenceRefs} empty="未记录证据。" />
            </details>
          ))}
          {historyVisibleCount < sortedTaskHistory.length ? (
            <Button type="button" variant="ghost" onClick={() => setHistoryVisibleCount((count) => count + 8)}>
              查看更早的 {Math.min(8, sortedTaskHistory.length - historyVisibleCount)} 个任务
            </Button>
          ) : null}
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
          {["awaitingAcceptance", "needsReview"].includes(task.status) ? (
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
  task: Pick<CodexTask, "status" | "summary" | "manualAcceptance" | "gitVerification">
    & Partial<Pick<CodexTask, "resultId" | "resultReceivedAt" | "remainingIssues" | "expectedResultPath">>
    | null,
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
  const hasResult = Boolean(task.resultId?.trim());
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

  const runEndedWithoutResult = !hasResult
    && (["needsReview", "awaitingAcceptance", "resultReceived", "verifying"].includes(task.status)
      || (task.status === "awaitingResult"
        && Boolean(run && ["exited", "failed", "cancelled", "unknownAfterRestart"].includes(run.status))));
  if (runEndedWithoutResult) {
    const reason = task.remainingIssues?.find((item) => item.trim()) || run?.error;
    return {
      statusLabel: "结果未回流",
      description: reason
        ? `没有收到绑定本次任务的结果文件。${reason}重新检查只会扫描结果目录，不会重新执行任务。`
        : "没有收到绑定本次任务的结果文件。重新检查只会扫描结果目录，不会重新执行任务。",
      progressTitle: "等待检查结果文件",
      primaryAction: "checkResult" as CodexPrimaryAction,
      secondaryActions: [] as CodexSecondaryAction[],
      tone: "warning",
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
      description: "结果已经返回，请检查结果后通过人工验收；通过后任务才会完成。",
      progressTitle: "等待人工检查",
      primaryAction: "accept" as CodexPrimaryAction,
      secondaryActions: ["reject"] as CodexSecondaryAction[],
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

export function buildCodexResultCheckFeedback(
  task: Pick<CodexTask, "title" | "status" | "resultId" | "resultReceivedAt" | "expectedResultPath"> | null,
  run: Pick<CodexRun, "status" | "error"> | null,
  scan: Pick<CodexResultBridgeScanResult, "scannedCount" | "importedCount">,
): CodexResultCheckFeedback {
  if (!task) {
    return {
      tone: "error" as const,
      title: "无法确认当前任务",
      message: "检查完成，但持久化任务列表中找不到这条任务。没有重新执行任务或更改验收状态。",
    };
  }
  if (task.resultId.trim()) {
    return {
      tone: "success" as const,
      title: "已找到本次任务结果",
      message: `已重新读取“${task.title}”的持久化结果；当前任务状态为 ${task.status}。`,
    };
  }

  const expectedPath = task.expectedResultPath.trim()
    ? `预期结果文件：${task.expectedResultPath}。`
    : "该任务没有记录预期结果文件路径。";
  const runState = run?.status ? `最近一次运行状态：${run.status}。` : "没有找到该任务的运行记录。";
  if (scan.scannedCount === 0) {
    return {
      tone: "warning" as const,
      title: "未找到结果文件",
      message: `检查完成：结果目录中没有可读取的结果文件。${expectedPath}${runState}任务保持未回流状态，没有重新执行或标记完成。`,
    };
  }
  if (scan.importedCount > 0) {
    return {
      tone: "warning" as const,
      title: "未找到当前任务的结果",
      message: `检查完成：项目中导入了 ${scan.importedCount} 条结果，但没有一条绑定到“${task.title}”。${expectedPath}${runState}任务保持未回流状态。`,
    };
  }
  return {
    tone: "warning" as const,
    title: "未找到当前任务的结果",
    message: `检查完成：扫描了 ${scan.scannedCount} 个结果文件，但没有找到绑定到“${task.title}”及其运行记录的结果。${expectedPath}${runState}任务保持未回流状态。`,
  };
}

type CodexResultCheckFeedback = {
  tone: "info" | "success" | "warning" | "error" | "loading";
  title: string;
  message: string;
};

function parseCodexTimestamp(value?: string) {
  return parseTimestamp(value);
}

export function formatCodexElapsed(startedAt: string, now = Date.now()) {
  const started = parseCodexTimestamp(startedAt);
  if (!started) return "时间未知";
  const seconds = Math.max(0, Math.floor((now - started) / 1000));
  return `${Math.floor(seconds / 60)}分${String(seconds % 60).padStart(2, "0")}秒`;
}

export function formatCodexLastActivity(lastActivityAt: string, now = Date.now()) {
  const lastActivity = parseCodexTimestamp(lastActivityAt);
  if (!lastActivity) return "暂无记录";
  const seconds = Math.max(0, Math.floor((now - lastActivity) / 1000));
  if (seconds < 5) return "刚刚";
  if (seconds < 60) return `${seconds}秒前`;
  return `${Math.floor(seconds / 60)}分${String(seconds % 60).padStart(2, "0")}秒前`;
}

export function formatCodexTaskDate(value: string, now = Date.now()) {
  return formatLocalDateTime(value, now);
}

function formatCodexDuration(startedAt: string, endedAt: string, now = Date.now()) {
  const started = parseCodexTimestamp(startedAt);
  const ended = parseCodexTimestamp(endedAt) ?? now;
  if (!started || ended < started) return "";
  const seconds = Math.floor((ended - started) / 1000);
  return `${Math.floor(seconds / 60)}分${String(seconds % 60).padStart(2, "0")}秒`;
}

export function codexTaskTimeSummary(
  task: Pick<CodexTask, "createdAt" | "handedOffAt" | "resultReceivedAt" | "completedAt">,
  run: Pick<CodexRun, "startedAt" | "endedAt"> | null,
  now = Date.now(),
) {
  const created = task.createdAt ? `${formatCodexTaskDate(task.createdAt, now)} 创建` : "创建时间未知";
  const startedAt = run?.startedAt || task.handedOffAt;
  const endedAt = run?.endedAt || task.completedAt || task.resultReceivedAt;
  const parts = [created];
  if (startedAt) parts.push(`${formatCodexTaskDate(startedAt, now)} 启动`);
  if (endedAt) parts.push(`${formatCodexTaskDate(endedAt, now)} 结束`);
  const duration = startedAt && endedAt ? formatCodexDuration(startedAt, endedAt, now) : "";
  if (duration) parts.push(`用时 ${duration}`);
  return parts.join(" · ");
}

export function sortCodexTasksByRecency(tasks: CodexTask[], runs: CodexRun[]) {
  return [...tasks].sort((left, right) => {
    const leftCreated = parseCodexTimestamp(left.createdAt) ?? parseCodexTimestamp(left.updatedAt) ?? 0;
    const rightCreated = parseCodexTimestamp(right.createdAt) ?? parseCodexTimestamp(right.updatedAt) ?? 0;
    if (rightCreated !== leftCreated) return rightCreated - leftCreated;
    const leftRun = latestRunForTask(runs, left.taskId);
    const rightRun = latestRunForTask(runs, right.taskId);
    const leftStarted = parseCodexTimestamp(leftRun?.startedAt) ?? 0;
    const rightStarted = parseCodexTimestamp(rightRun?.startedAt) ?? 0;
    if (rightStarted !== leftStarted) return rightStarted - leftStarted;
    return right.taskId.localeCompare(left.taskId);
  });
}

export function codexActivityStatus(activity: string) {
  const event = activity.replace(/^(stdout|stderr)\s*[:：]\s*/i, "").trim();
  if (/[\u3400-\u9fff]/u.test(event)) return event;
  const labels: Record<string, string> = {
    "thread.started": "Codex 已建立任务会话",
    "turn.started": "Codex 已开始处理任务",
    "item.started": "Codex 已开始一项操作",
    "item.updated": "Codex 更新了一项操作的执行信息",
    "item.completed": "Codex 已结束一项操作",
    "turn.completed": "Codex 本轮处理已结束",
    "turn.failed": "Codex 本轮处理失败",
    "error": "Codex 报告了执行错误",
  };
  // The backend may retain an event name followed by its message text.
  const type = event.split(/\s|[:：]/, 1)[0];
  if (labels[type]) return labels[type];
  if (event === "Codex CLI 进程已启动。") return "Codex 已启动";
  if (event === "已创建 CodexRun，等待 Codex CLI 启动。") return "正在等待 Codex 启动";
  return "Codex 收到新的执行活动";
}

export function buildCodexRunObservability(run: Pick<CodexRun, "status" | "startedAt" | "processAlive" | "lastActivityAt" | "recentActivity"> & Partial<Pick<CodexRun, "rawActivity">>, now = Date.now()) {
  const lastActivity = parseCodexTimestamp(run.lastActivityAt);
  const quiet = run.status === "running" && run.processAlive && !!lastActivity && now - lastActivity >= 3 * 60 * 1000;
  const processText = run.status === "starting"
    ? "进程正在启动"
    : run.processAlive
      ? "进程正常"
      : "进程状态待确认";
  const latest = run.recentActivity?.[run.recentActivity.length - 1];
  const latestActivity = latest ? codexActivityStatus(latest) : "等待 Codex 输出";
  return {
    headline: run.status === "starting" ? "Codex 正在启动" : "Codex 正在执行",
    elapsedText: formatCodexElapsed(run.startedAt, now),
    processText,
    lastActivityText: formatCodexLastActivity(run.lastActivityAt, now),
    latestActivity,
    recentActivity: run.recentActivity ?? [],
    rawActivity: run.rawActivity ?? [],
    quiet,
  };
}

function CodexRunObservability({ run }: { run: CodexRun }) {
  const observability = buildCodexRunObservability(run);
  return (
    <section className="codex-live-status" aria-live="polite" aria-label="Codex 实时执行状态">
      <div className="codex-live-status-head">
        <strong>{observability.headline} · 已运行 {observability.elapsedText}</strong>
        <span>{observability.processText} · 最近活动 {observability.lastActivityText}</span>
      </div>
      <p>当前：{observability.latestActivity}</p>
      {observability.quiet ? <small>Codex 仍在运行，但已经数分钟没有新的 CLI 输出。</small> : null}
      <details className="codex-live-activity">
        <summary>查看执行详情</summary>
        {observability.recentActivity.length ? (
          <ol>
            {observability.recentActivity.map((activity, index) => <li key={`${activity}-${index}`}>{activity}</li>)}
          </ol>
        ) : (
          <p>暂时没有可展示的 Codex 输出。</p>
        )}
        <details className="codex-live-raw-activity">
          <summary>查看近期原始 CLI 事件</summary>
          {observability.rawActivity.length ? (
            <ol>
              {observability.rawActivity.map((activity, index) => <li key={`${activity}-${index}`}><code>{activity}</code></li>)}
            </ol>
          ) : (
            <p>当前运行尚无可展开的原始事件；旧运行可能只保留整理后的活动摘要。</p>
          )}
        </details>
      </details>
    </section>
  );
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
  if (["awaitingAcceptance", "needsReview"].includes(task.status)) return "需要你验收";
  if (task.status === "completed") return "结果已完成";
  if (task.resultId) return "结果已回流";
  if (task.gitVerification?.status === "mismatch") return "代码记录不一致";
  return "等待结果回流";
}

function codexHistoryOutcome(task: CodexTask) {
  if (task.status === "completed") return "Codex 已完成这项工作。";
  if (task.status === "awaitingAcceptance") return "结果已返回，需要你确认。";
  if (task.status === "needsReview") return "结果需要进一步检查。";
  if (task.status === "failed") return "这次执行没有完成，可以查看原因或重新执行。";
  if (task.status === "running" || task.status === "awaitingResult") return "Codex 正在处理这项工作。";
  if (task.status === "ready") return "任务已准备好，可以交给 Codex。";
  return "这项工作尚未完成。";
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
  // taskType is the persisted execution contract. Prompt/result fields contain
  // generic bridge vocabulary for every task type, so they must not override it.
  return normalizeCodexTaskType(task.taskType);
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

function runForCodexTask(runs: CodexRun[], task: Pick<CodexTask, "taskId" | "resultRunId">) {
  if (task.resultRunId) {
    return runs.find((run) => run.id === task.resultRunId && run.taskId === task.taskId) ?? latestRunForTask(runs, task.taskId);
  }
  return latestRunForTask(runs, task.taskId);
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

export function selectCurrentCodexTask(tasks: CodexTask[], runs: CodexRun[], targetTaskId = "", pinnedTaskId = "") {
  if (!tasks.length) return null;
  const taskById = new Map(tasks.map((task) => [task.taskId, task]));
  const pinnedTask = pinnedTaskId && pinnedTaskId === targetTaskId ? taskById.get(pinnedTaskId) : null;
  if (pinnedTask) return pinnedTask;
  const targetTask = targetTaskId ? taskById.get(targetTaskId) : null;
  const recency = (task: CodexTask) =>
    parseCodexTimestamp(task.updatedAt) ?? parseCodexTimestamp(task.createdAt) ?? 0;
  const hasBoundResult = (task: CodexTask) =>
    Boolean(task.resultId.trim()) &&
    task.resultSource.toLowerCase() !== "runnerfallback" &&
    !task.evidenceRefs.some((reference) => reference.startsWith("runnerFallback:"));
  const liveRuns = [...runs]
    .filter((run) => run.processAlive && ["starting", "running"].includes(run.status))
    .sort((left, right) => (parseCodexTimestamp(right.startedAt) ?? 0) - (parseCodexTimestamp(left.startedAt) ?? 0));
  const hasMatchingLiveRun = (task: CodexTask) =>
    liveRuns.some(
      (run) =>
        run.taskId === task.taskId &&
        (!task.projectId || !run.projectId || task.projectId === run.projectId),
    );
  const taskHasCurrentEvidence = (task: CodexTask) =>
    task.status === "ready" ||
    (["starting", "running", "handedOff", "awaitingResult"].includes(task.status) && hasMatchingLiveRun(task)) ||
    (["resultReceived", "verifying", "needsReview", "awaitingAcceptance"].includes(task.status) &&
      hasBoundResult(task));
  const targetStillActionable =
    targetTask &&
    taskHasCurrentEvidence(targetTask);
  if (targetTask && targetStillActionable) {
    return targetTask;
  }
  for (const liveRun of liveRuns) {
    const runningTask = taskById.get(liveRun.taskId);
    if (
      runningTask &&
      hasMatchingLiveRun(runningTask) &&
      ["starting", "running", "handedOff", "awaitingResult"].includes(runningTask.status) &&
      !["completed", "cancelled"].includes(runningTask.status)
    ) {
      return runningTask;
    }
  }
  const actionable = tasks
    .filter(taskHasCurrentEvidence)
    .sort((left, right) => recency(right) - recency(left));
  if (actionable.length) return actionable[0];

  const resultRecovery = tasks
    .filter(
      (task) =>
        ["starting", "running", "handedOff", "awaitingResult", "resultReceived", "verifying", "needsReview"].includes(task.status) &&
        !hasBoundResult(task) &&
        runs.some(
          (run) =>
            run.taskId === task.taskId &&
            ["exited", "failed", "unknownAfterRestart"].includes(run.status),
        ),
    )
    .sort((left, right) => recency(right) - recency(left))[0];
  if (resultRecovery) return resultRecovery;

  const completed = tasks
    .filter((task) => task.status === "completed")
    .sort((left, right) => recency(right) - recency(left))[0];
  if (completed) return completed;

  return null;
}

function codexActionErrorMessage(error: unknown) {
  const message = String(error).replace(/^Error:\s*/i, "").trim();
  return message || "操作未完成，请刷新当前任务后重试。";
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

export function terminalRunSignatures(runs: CodexRun[]) {
  return new Map(
    runs
      .filter((run) => isTerminalCodexRun(run))
      .map((run) => [run.id, `${run.status}:${run.endedAt}:${run.exitCode ?? ""}`]),
  );
}

export function terminalRunStateChanged(previous: Map<string, string>, nextRuns: CodexRun[]) {
  return nextRuns.some((run) => {
    if (!isTerminalCodexRun(run)) return false;
    return previous.get(run.id) !== `${run.status}:${run.endedAt}:${run.exitCode ?? ""}`;
  });
}

function isTerminalCodexRun(run: CodexRun) {
  return ["exited", "failed", "cancelled"].includes(run.status);
}

function latestRunForTask(runs: CodexRun[], taskId: string) {
  return (
    runs
      .filter((run) => run.taskId === taskId)
      .sort((left, right) => (parseCodexTimestamp(right.startedAt) ?? 0) - (parseCodexTimestamp(left.startedAt) ?? 0))[0] ?? null
  );
}

export function mergeCodexRuns(existing: CodexRun[], next: CodexRun) {
  const withoutCurrent = existing.filter((run) => run.id !== next.id);
  return [next, ...withoutCurrent].sort((left, right) => (parseCodexTimestamp(right.startedAt) ?? 0) - (parseCodexTimestamp(left.startedAt) ?? 0));
}

export function codexRunsEqual(left: CodexRun[], right: CodexRun[]) {
  if (left.length !== right.length) return false;
  return left.every((run, index) => {
    const next = right[index];
    if (!next) return false;
    const sameState =
      run.id === next.id &&
      run.taskId === next.taskId &&
      run.status === next.status &&
      run.startedAt === next.startedAt &&
      run.endedAt === next.endedAt &&
      run.pid === next.pid &&
      run.processAlive === next.processAlive &&
      run.exitCode === next.exitCode &&
      run.lastActivityAt === next.lastActivityAt &&
      run.outputLastMessagePath === next.outputLastMessagePath &&
      run.stdoutSummary === next.stdoutSummary &&
      run.stderrSummary === next.stderrSummary &&
      run.error === next.error &&
      sameRecentActivity(run.recentActivity, next.recentActivity);

    // Raw output is append-only in the runner. Compare its size and visible tail
    // instead of serializing the complete log on every status poll.
    const previousRaw = run.rawActivity ?? [];
    const nextRaw = next.rawActivity ?? [];
    return sameState && previousRaw.length === nextRaw.length && sameRecentActivity(previousRaw.slice(-4), nextRaw.slice(-4));
  });
}

function sameRecentActivity(left: string[] = [], right: string[] = []) {
  return left.length === right.length && left.every((entry, index) => entry === right[index]);
}

function WorkspaceActivityPanel({
  projectAnalysis,
  pendingReviews,
  workLedger,
  activeNextStep,
  activeTasks,
  needsAuthorization,
  authorizationStatus,
  authorizationFeedback,
  authorizationBusy,
  needsAiConfiguration,
  onOpenAiSettings,
  onGrantAuthorization,
}: {
  projectAnalysis: ProjectAnalysis | null;
  pendingReviews: PendingReviewItem[];
  workLedger: WorkLedgerSnapshot | null;
  activeNextStep: string;
  activeTasks: Array<{ status: string; title: string }>;
  needsAuthorization: boolean;
  authorizationStatus: string;
  authorizationFeedback: string;
  authorizationBusy: boolean;
  needsAiConfiguration: boolean;
  onOpenAiSettings: () => void;
  onGrantAuthorization: () => Promise<void>;
}) {
  const projectDefinition =
    projectAnalysis?.status === "success"
      ? projectAnalysis.projectDefinition || "项目理解仍在整理中。"
      : projectAnalysis?.status === "failed"
        ? `项目理解生成失败：${projectAnalysis.failureReason}`
        : "项目理解等待资料和授权后生成。";
  const recentFacts = buildRecoveryFacts(workLedger);
  const currentPendingActions = (workLedger?.pendingActions ?? []).filter((item) => item.status === "pending");
  const currentNextStep = isGeneratedNextStep(activeNextStep) ? "" : activeNextStep.trim();
  const currentActionCount = currentPendingActions.length + (currentNextStep ? 1 : 0) + activeTasks.filter((task) => task.status === "active").length;
  const currentPendingCount = pendingReviews.length + currentPendingActions.length;

  return (
    <section className="workspace-activity-panel">
      <div className="workspace-sidebar-head workspace-activity-head">
        <span className="section-label">项目动态</span>
        <strong>项目理解与最近变化</strong>
        <p>先看当前项目是什么，再按需回顾最近发生的事。</p>
      </div>

      <div className="workspace-activity-body">
        {needsAuthorization || needsAiConfiguration ? (
          <div className="workspace-activity-callout">
            <strong>{needsAiConfiguration ? "需要配置 AI 连接" : "需要授权或重新确认数据范围"}</strong>
            {needsAiConfiguration ? (
              <p>尚未配置 DeepSeek API Key。请先保存 Key 并测试连接；这只配置 AI 连接，不会授权发送本项目数据。</p>
            ) : null}
            {needsAuthorization ? (
              <>
                <p>启用会立即把当前项目的限量说明、资料摘要/章节、近期对话和只读工程状态摘要发送给外部 AI 服务生成项目理解。之后每次聊天会发送本轮问题、按问题筛选的项目记录，以及最多 5 轮历史对话（最近 3 轮，另加最多 2 轮相关历史；合计不超过 4,800 字，每条历史文本最多 700 字）；历史对话仅用于延续当前讨论，不作为已确认事实或决定。命中资料时还会发送最多 3 份资料的摘要与正文片段，本轮图片仅在所选模型支持视觉时发送。不会发送整份资料文件、完整聊天历史或历史图片。</p>
                {needsAiConfiguration ? (
                  <p className="workspace-authorization-setup-note">保存 Key 并测试连接后，返回此处仍需单独确认本项目授权。</p>
                ) : null}
              </>
            ) : null}
            {needsAiConfiguration ? (
              <button type="button" className="btn btn-ghost" onClick={onOpenAiSettings}>
                配置/测试 AI 连接
              </button>
            ) : null}
            {needsAuthorization ? (
              <details className="workspace-authorization-details">
                <summary>查看发送范围</summary>
                <p>项目理解：项目名称/说明和下一步、最多 24 份登记资料的名称/类别/摘要/章节、最多 18 条近期消息（每条最多 1,000 字），以及当前只读 Atlas 状态摘要；不含文件正文、本地路径或完整聊天历史。聊天：本轮问题最多 8,000 字；按问题选取当前项目的可信记录；最多 5 轮历史对话（最近 3 轮，另加最多 2 轮相关历史），合计不超过 4,800 字，每条历史文本最多 700 字。历史对话仅用于理解当前讨论，不作为事实或已确认决定。命中时最多 3 份登记资料，每份正文最多 1,800 字。最多 4 张本轮图片（单张不超过 8 MB、总计不超过 20 MB）仅发送给支持视觉的模型。Inbox 匹配会发送待处理资料的限量摘要，以及最多 3 个分别授权候选项目的说明、项目理解、资料/任务/决定摘要和规则匹配理由。原文件、本地来源路径、完整聊天历史和历史图片不发送；疑似密钥与本地路径会脱敏。API Key 保存在 Windows Credential Manager，仅用于请求授权头。</p>
              </details>
            ) : null}
            {authorizationFeedback ? (
              <p className={`project-authorization-feedback status-${authorizationStatus}`} role="status" aria-live="polite">
                {authorizationFeedback}
              </p>
            ) : null}
            {needsAuthorization ? (
              <button
                type="button"
                className="btn btn-primary"
                disabled={authorizationBusy}
                aria-busy={authorizationBusy}
                onClick={() => void onGrantAuthorization()}
              >
                {authorizationBusy ? "正在启用 AI 回复…" : "启用 AI 回复"}
              </button>
            ) : null}
          </div>
        ) : authorizationFeedback ? (
          <div className={`workspace-activity-callout authorization-status-callout status-${authorizationStatus}`}>
            <strong>AI 回复授权状态</strong>
            <p role="status" aria-live="polite">{authorizationFeedback}</p>
          </div>
        ) : null}

        <details className="workspace-activity-details workspace-activity-understanding" open>
          <summary>
            <span>项目理解</span>
            <strong>{projectAnalysisStatusLabel(projectAnalysis?.status)}</strong>
          </summary>
          <p>{projectDefinition}</p>
          <div className="workspace-activity-metrics">
            <div><span>待确认</span><strong>{currentPendingCount}</strong></div>
            <div><span>下一步</span><strong>{currentActionCount}</strong></div>
            <div><span>待检查</span><strong>{pendingReviews.length}</strong></div>
          </div>
        </details>

        <details className="workspace-activity-details workspace-activity-recent">
          <summary>
            <span>最近变化</span>
            <strong>{recentFacts.length}</strong>
          </summary>
          {recentFacts.length ? (
            <div className="workspace-activity-list">
              {recentFacts.map((fact) => (
                <article key={fact} className="workspace-activity-item">
                  <p>{fact}</p>
                </article>
              ))}
            </div>
          ) : (
            <p>暂无新的项目变化。</p>
          )}
        </details>
      </div>
    </section>
  );
}

function isGeneratedNextStep(value: string) {
  const normalized = value.trim();
  return !normalized
    || normalized === "查看资料分析结果，补充项目目标与当前任务。"
    || normalized === "继续整理项目资料，确认下一步工作。"
    || normalized === "根据当前项目事实继续下一步工作。";
}

function buildRecoveryFacts(workLedger: WorkLedgerSnapshot | null) {
  const facts = (workLedger?.activityTimeline ?? [])
    .filter((activity) => activity.userVisible !== false && activity.summary.trim())
    .map((activity) => activity.summary.trim());
  const codexFacts = (workLedger?.codexResults ?? [])
    .map((result) => result.resultText.trim() ? `Codex结果：${result.resultText.trim()}` : `Codex任务：${result.status}`)
    .filter(Boolean);
  const git = workLedger?.gitSnapshot;
  const gitFacts = git
    ? [`Git HEAD ${git.headShort}：${git.recentCommits[0]?.subject ?? "当前仓库事实已同步"}${git.isDirty ? "；工作区存在未提交改动" : "；工作区 clean"}`]
    : [];
  return [...facts, ...codexFacts, ...gitFacts].filter((fact, index, all) => all.indexOf(fact) === index).slice(-4).reverse();
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

export function workspaceActivityLabel(message: Pick<WorkspaceMessage, "kind" | "status">) {
  if (message.status === "interrupted") return "生成已中断";
  if (message.status === "failed" || message.status === "error") return "需要检查";
  if (message.kind === "review") return "待确认";
  if (message.status === "pending" || message.status === "running") return "处理中";
  if (message.kind === "analysis") return "项目理解";
  if (message.kind === "monitor") return "资料变化";
  return "项目记录";
}

export function projectAnalysisStatusLabel(status?: ProjectAnalysis["status"] | string | null) {
  if (status === "success") return "已整理";
  if (status === "failed") return "需要检查";
  if (status === "pending" || status === "running") return "整理中";
  return "等待整理";
}

export function workspaceMessageDisplayLabel(message: Pick<WorkspaceMessage, "kind" | "status">) {
  if (message.status === "interrupted") return "生成已中断";
  if (message.status === "failed" || message.status === "error") return "需要检查";
  if (message.status === "pending" || message.status === "running") return "处理中";
  if (message.kind === "fileLocation") return "文件已整理";
  if (message.kind === "analysis") return "项目理解已更新";
  if (message.kind === "monitor") return "资料变化已记录";
  if (message.kind === "review") return "待确认";
  if (message.kind === "codex" || message.kind === "task") return "Codex 工作记录";
  return "项目记录";
}

export function sanitizeWorkspacePublicText(text: string, kind?: string) {
  const trimmed = text.trim();
  if (!trimmed) return kind === "fileLocation" ? "文件已整理。" : "项目变化已记录。";
  if (/codex-task-[\w-]+\.md/i.test(trimmed)) return "已保存 Codex 任务记录。";
  if (/result bridge|runnerfallback|taskid|runid|resultpath|repositorypath|filelocation/i.test(trimmed)) {
    if (kind === "fileLocation") return "文件已整理。";
    if (kind === "codex" || kind === "task") return "Codex 工作记录已更新。";
    return "项目变化已记录。";
  }
  if (/^[A-Z]:\\|^\\\\|^\//i.test(trimmed)) return "相关资料已更新。";
  return trimmed;
}

export function latestWorkspaceItems<T>(items: T[], limit: number) {
  return items.slice(-Math.max(1, limit));
}

export function resolveConversationScrollTop({
  scrollHeight,
  clientHeight,
  previousScrollTop,
  followBottom,
}: {
  scrollHeight: number;
  clientHeight: number;
  previousScrollTop: number;
  followBottom: boolean;
}) {
  const maxScrollTop = Math.max(0, scrollHeight - clientHeight);
  return followBottom ? maxScrollTop : Math.min(Math.max(0, previousScrollTop), maxScrollTop);
}

export function useConversationMessages(messages: WorkspaceMessage[], projectDescription: string) {
  return useMemo(
    () => messages.filter((message, index) => isConversationMessage(message, index, projectDescription)),
    [messages, projectDescription],
  );
}

export function visibleReferenceFiles<T extends { id: string }>(items: T[], selectedId: string | null, limit: number) {
  const visible = items.slice(0, Math.max(1, limit));
  if (!selectedId || visible.some((item) => item.id === selectedId)) return visible;

  const selected = items.find((item) => item.id === selectedId);
  return selected ? [selected, ...visible.slice(0, -1)] : visible;
}

function Message({
  message,
  projectRoot,
  readChatAttachment: loadChatAttachment,
  openFilePath,
  openFolderPath,
  onRetry,
  readOnlyRecovery,
}: {
  message: WorkspaceMessage;
  projectRoot: string;
  readChatAttachment: typeof readChatAttachment;
  openFilePath: (path: string) => Promise<void>;
  openFolderPath: (path: string) => Promise<void>;
  onRetry: (messageId: string) => Promise<void>;
  readOnlyRecovery: boolean;
}) {
  const [imageUrls, setImageUrls] = useState<Record<string, string>>({});
  const operationReceipts = message.evidenceItems.filter((item) => item.contentType === "operationReceipt");
  const facts = message.evidenceItems.filter(
    (item) => item.basisKind === "fact" && item.contentType !== "operationReceipt",
  );
  const inference = message.evidenceItems.filter((item) => item.basisKind === "inference");
  const pending = message.evidenceItems.filter((item) => item.basisKind === "pending");
  const evidenceCount = facts.length + inference.length + pending.length;
  const attachmentSignature = message.attachments.map((item) => `${item.fileId}:${item.relativePath ?? ""}`).join("|");

  useEffect(() => {
    let active = true;
    const imageAttachments = message.attachments.filter(
      (attachment) => attachment.attachmentType === "image" && attachment.relativePath,
    );
    if (!imageAttachments.length) {
      setImageUrls({});
      return () => {
        active = false;
      };
    }
    void Promise.all(
      imageAttachments.map(async (attachment) => {
        try {
          const result = await loadChatAttachment(projectRoot, attachment.relativePath || "");
          return [attachment.fileId, result.dataUrl] as const;
        } catch {
          return null;
        }
      }),
    ).then((entries) => {
      if (!active) return;
      setImageUrls(Object.fromEntries(entries.filter((entry): entry is readonly [string, string] => Boolean(entry))));
    });
    return () => {
      active = false;
    };
  }, [attachmentSignature, loadChatAttachment, message.id, projectRoot]);

  return (
    <article className={`thread-message message-${message.author === "user" ? "user" : "ganmaoyuan"}`}>
      <div className="message-meta">
        <strong>{message.author === "user" ? "你" : "感冒院"}</strong>
        <span>{workspaceMessageDisplayLabel(message)}</span>
      </div>
      <p>{message.text}</p>
      {message.status === "interrupted" ? (
        <div className="message-interrupted-note" role="status">
          <span>{message.text.trim() ? "以上仅为已保存的部分内容，不是完整回答。" : "AI 回复在完成前中断，未生成可恢复正文。"}</span>
          {readOnlyRecovery ? (
            <span>当前项目正在使用旧备份只读回退；核对并恢复主数据前不能重试。</span>
          ) : (
            <button type="button" className="btn btn-compact" onClick={() => void onRetry(message.id)}>
              用原问题重试
            </button>
          )}
        </div>
      ) : null}
      {operationReceipts.length ? (
        <div className="message-operation-receipts" role="status" aria-label="项目记录保存回执">
          {operationReceipts.map((item) => (
            <div key={item.id} className="message-operation-receipt">
              <strong>{item.title}</strong>
              <p>{item.summary}</p>
            </div>
          ))}
        </div>
      ) : null}
      {message.attachments.length ? (
        <div className="message-attachments" aria-label="消息附件">
          {message.attachments.map((attachment) => {
            const isImage = attachment.attachmentType === "image";
            const imageUrl = imageUrls[attachment.fileId];
            return isImage ? (
              <figure key={attachment.fileId} className="message-attachment-image">
                {imageUrl ? <img src={imageUrl} alt={attachment.fileName} /> : <span>{attachment.fileName}</span>}
                <figcaption title={attachment.fileName}>{attachment.fileName}</figcaption>
              </figure>
            ) : (
              <button key={attachment.fileId} type="button" className="attachment-tag" onClick={() => void openFilePath(attachment.managedPath).catch(() => undefined)}>
                <span>{attachment.fileName}</span>
              </button>
            );
          })}
        </div>
      ) : null}
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
            {item.sourceRecordId && ["decision", "workLedger", "gitSnapshot"].includes(item.contentType) ? (
              <small className="evidence-record-id">记录 ID：{item.sourceRecordId}</small>
            ) : null}
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
    case "workLedger":
      return "工作记录";
    case "gitSnapshot":
      return "Git 状态";
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

function ReferenceFileDialog({
  file,
  onClose,
  openFilePath,
  openFolderPath,
}: {
  file: ManagedFile;
  onClose: () => void;
  openFilePath: (path: string) => Promise<void>;
  openFolderPath: (path: string) => Promise<void>;
}) {
  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [onClose]);

  return (
    <div className="workspace-reference-dialog" role="dialog" aria-modal="true" aria-labelledby="reference-file-dialog-title">
      <button type="button" className="workspace-reference-dialog-backdrop" onClick={onClose} aria-label="关闭资料详情" />
      <section className="workspace-reference-dialog-card">
        <header className="workspace-reference-dialog-head">
          <div>
            <span className="section-label">参考资料详情</span>
            <h2 id="reference-file-dialog-title">{file.fileName}</h2>
          </div>
          <button type="button" className="btn btn-ghost" onClick={onClose}>
            关闭
          </button>
        </header>
        <FileResult file={file} openFilePath={openFilePath} openFolderPath={openFolderPath} />
      </section>
    </div>
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
