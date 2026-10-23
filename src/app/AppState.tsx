import { createContext, useContext, useEffect, useLayoutEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  applyCodexReport as applyDesktopCodexReport,
  createCodexTask as createDesktopCodexTask,
  createProject as createDesktopProject,
  captureProjectFact as captureDesktopProjectFact,
  createProjectFromMaterialInbox as createDesktopProjectFromMaterialInbox,
  createProjectFromInbox as createDesktopProjectFromInbox,
  deleteDeepSeekApiKey as deleteDesktopDeepSeekApiKey,
  finishProjectWork as finishDesktopProjectWork,
  confirmWeeklyReport as confirmDesktopWeeklyReport,
  exportWeeklyReport as exportDesktopWeeklyReport,
  generateCodexPrompt as generateDesktopCodexPrompt,
  generateWeeklyReviews as generateDesktopWeeklyReviews,
  importCodexReportFile as importDesktopCodexReportFile,
  importCodexReportText as importDesktopCodexReportText,
  ingestMaterialInboxFiles as ingestDesktopMaterialInboxFiles,
  receiveInboxFiles as receiveDesktopInboxFiles,
  retryInterruptedProjectMessage as retryDesktopInterruptedProjectMessage,
  refineInboxEntryWithAi as refineDesktopInboxEntryWithAi,
  updateInboxRouteDecision as updateDesktopInboxRouteDecision,
  ignoreInboxEntry as ignoreDesktopInboxEntry,
  retryInboxEntry as retryDesktopInboxEntry,
  reanalyzeInboxEntry as reanalyzeDesktopInboxEntry,
  undoInboxRoute as undoDesktopInboxRoute,
  routeInboxItemToGlobal as routeDesktopInboxItemToGlobal,
  loadInboxRoutingSettings as loadDesktopInboxRoutingSettings,
  saveInboxRoutingSettings as saveDesktopInboxRoutingSettings,
  isTauriRuntime,
  grantProjectDeepSeekAuthorization as grantDesktopDeepSeekAuthorization,
  listenDeepSeekStream,
  listMemos,
  listInboxEntries,
  loadDeepSeekSettings as loadDesktopDeepSeekSettings,
  loadWeeklyReviewDashboard as loadDesktopWeeklyReviewDashboard,
  loadWeeklyReviewSettings as loadDesktopWeeklyReviewSettings,
  listProjects,
  listDeepSeekModels as listDesktopDeepSeekModels,
  loadProject,
  markProjectOpened,
  markCodexTaskHandedOff as markDesktopCodexTaskHandedOff,
  acceptCodexTask as acceptDesktopCodexTask,
  rejectCodexTask as rejectDesktopCodexTask,
  openManagedFile,
  openManagedFolder,
  confirmInboxEntry as confirmDesktopInboxEntry,
  confirmMaterialInboxItem as confirmDesktopMaterialInboxItem,
  consumeLaunchInboxEntries as consumeDesktopLaunchInboxEntries,
  scanProjectWorkspace as scanDesktopProjectWorkspace,
  synchronizeProjectFacts as synchronizeDesktopProjectFacts,
  refreshProjectUnderstanding as refreshDesktopProjectUnderstanding,
  saveDeepSeekApiKey as saveDesktopDeepSeekApiKey,
  saveProjectDraft as saveDesktopProjectDraft,
  saveWeeklyReviewSettings as saveDesktopWeeklyReviewSettings,
  scanCodexResultBridge as scanDesktopCodexResultBridge,
  saveMemos,
  sendProjectMessage as sendDesktopProjectMessage,
  takeStorageRecoveryNotices,
  stopProjectMessage as stopDesktopProjectMessage,
  testDeepSeekConnection as testDesktopDeepSeekConnection,
  getImpactAnalyses as getDesktopImpactAnalyses,
  getActionCandidates as getDesktopActionCandidates,
  getStateProposals as getDesktopStateProposals,
  confirmActionCandidate as confirmDesktopActionCandidate,
  ignoreActionCandidate as ignoreDesktopActionCandidate,
  updateActionCandidate as updateDesktopActionCandidate,
  applyStateProposal as applyDesktopStateProposal,
  undoStateProposal as undoDesktopStateProposal,
  getDailyContinue as getDesktopDailyContinue,
  getWorkLedger as getDesktopWorkLedger,
  getTodayWorkspace as getDesktopTodayWorkspace,
  refreshTodayWorkspace as refreshDesktopTodayWorkspace,
  updateContinueProjectPreference as updateDesktopContinueProjectPreference,
  regenerateDailyContinue as regenerateDesktopDailyContinue,
  updateProjectAttention as updateDesktopProjectAttention,
  updateDataHealthStatus as updateDesktopDataHealthStatus,
  updatePendingReviewStatus as updateDesktopPendingReviewStatus,
  markTodayDone as markDesktopTodayDone,
  recordUserDecisionEvent as recordDesktopUserDecisionEvent,
  registerGeneratedFile as registerDesktopGeneratedFile,
  refreshGitSnapshot as refreshDesktopGitSnapshot,
  setStateAutoApply as setDesktopStateAutoApply,
  type AtlasAssessment,
  type ArtifactRecord,
  type CodexPromptRecord,
  type CodexPromptResult,
  type CodexReportApplyResult,
  type CodexReportImportResult,
  type CodexReportRecord,
  type CodexTask,
  type CodexTaskCreateRequest,
  type ChatImageAttachmentInput,
  type DailySession,
  type DeepSeekModelInfo,
  type DeepSeekSettings,
  type DecisionRecord,
  type LocationDecision,
  type InboxRoutingSettings,
  type ManagedFile,
  type MaterialInboxItem,
  type MaterialInboxRouteResult,
  type MonitoringState,
  type MemoItem,
  type PendingReviewItem,
  type ProjectManifest,
  type ProjectAnalysis,
  type ProjectFactCaptureRequest,
  type ProjectSummary,
  type TaskRecord,
  type WeeklyReviewDashboard,
  type WeeklyReviewSettings,
  type WeeklyReportRecord,
  type WorkspaceMessage,
  type ProjectImpactAnalysis,
  type ProjectActionCandidate,
  type ProjectStateProposal,
  type DailyContinueSnapshot,
  type GitSnapshot,
  type TodayWorkspace,
  type WorkEvent,
  type WorkLedgerSnapshot,
} from "../features/project/desktopApi";

type ThemeMode = "dark" | "light";
type ProjectAiAuthorizationStatus = "idle" | "saving" | "refreshing" | "success" | "failed" | "cancelled" | "timeout";

export type SettingsSection =
  | "general"
  | "projects"
  | "tasks"
  | "materials"
  | "pending-materials"
  | "file-index"
  | "project-memory"
  | "ai-context"
  | "ai-models"
  | "data-backup"
  | "weekly-review"
  | "privacy-security"
  | "appearance"
  | "advanced";

type AppStateValue = {
  theme: ThemeMode;
  toggleTheme: () => void;
  currentTask: string;
  nextStep: string;
  worklog: string;
  setWorklog: (value: string) => void;
  hasResumeSession: boolean;
  lastCompletedAt: string | null;
  materialCount: number;
  addMaterials: (count: number) => void;
  saveProgress: () => void;
  markWorkStarted: () => void;
  completeWork: () => void;
  selectedSettingsSection: SettingsSection;
  setSelectedSettingsSection: (section: SettingsSection) => void;
  materials: Array<{ title: string; detail: string }>;
  pendingMaterials: Array<{ title: string; detail: string }>;
  projectMemory: string[];
  aiContextItems: string[];
  deepSeekSettings: DeepSeekSettings;
  deepSeekSettingsLoaded: boolean;
  deepSeekModels: DeepSeekModelInfo[];
  projectAiAuthorizationStatus: ProjectAiAuthorizationStatus;
  projectAiAuthorizationFeedback: string;
  isStreaming: boolean;
  isDesktopReady: boolean;
  isStartupHydrated: boolean;
  projects: ProjectSummary[];
  activeProject: ProjectSummary | null;
  activeManifest: ProjectManifest | null;
  workspaceMessages: WorkspaceMessage[];
  tasks: TaskRecord[];
  decisions: DecisionRecord[];
  artifacts: ArtifactRecord[];
  codexPrompts: CodexPromptRecord[];
  codexReports: CodexReportRecord[];
  codexTasks: CodexTask[];
  dailySessions: DailySession[];
  locationDecisions: LocationDecision[];
  pendingReviews: PendingReviewItem[];
  monitoring: MonitoringState | null;
  atlas: AtlasAssessment | null;
  projectAnalysis: ProjectAnalysis | null;
  managedFiles: ManagedFile[];
  memos: MemoItem[];
  materialInbox: MaterialInboxItem[];
  inboxRoutingSettings: InboxRoutingSettings;
  weeklyReviewDashboard: WeeklyReviewDashboard | null;
  weeklyReviewSettings: WeeklyReviewSettings | null;
  weeklyReports: WeeklyReportRecord[];
  error: string;
  setError: (value: string) => void;
  refreshProjects: () => Promise<void>;
  openProject: (projectRoot: string) => Promise<void>;
  createProject: (name: string, rootDir: string, filePaths: string[], description: string) => Promise<void>;
  importFilesToActiveProject: (filePaths: string[], relatedTask: string) => Promise<void>;
  sendProjectMessage: (text: string, imageAttachments?: ChatImageAttachmentInput[]) => Promise<void>;
  retryProjectMessage: (interruptedMessageId: string) => Promise<void>;
  stopProjectMessage: () => Promise<void>;
  saveProjectDraft: (text: string, pendingFilePaths?: string[]) => Promise<void>;
  finishProjectWork: (done: string, nextStep: string) => Promise<void>;
  generateCodexPrompt: () => Promise<CodexPromptResult | null>;
  createCodexTask: (request: CodexTaskCreateRequest) => Promise<CodexTask | null>;
  markCodexTaskHandedOff: (taskId: string) => Promise<CodexTask>;
  acceptCodexTask: (taskId: string, resultId: string) => Promise<CodexTask>;
  rejectCodexTask: (taskId: string, resultId: string, reason?: string) => Promise<CodexTask>;
  importCodexReportText: (text: string) => Promise<CodexReportImportResult>;
  importCodexReportFile: (path: string) => Promise<CodexReportImportResult>;
  applyCodexReport: (reportId: string) => Promise<CodexReportApplyResult>;
  saveDeepSeekApiKey: (apiKey: string, selectedModelId: string) => Promise<void>;
  deleteDeepSeekApiKey: () => Promise<void>;
  testDeepSeekConnection: (selectedModelId: string) => Promise<void>;
  grantProjectDeepSeekAuthorization: () => Promise<void>;
  refreshProjectUnderstanding: () => Promise<void>;
  openFilePath: (path: string) => Promise<void>;
  openFolderPath: (path: string) => Promise<void>;
  addMemo: (text: string) => Promise<void>;
  updateMemo: (id: string, text: string) => Promise<void>;
  deleteMemo: (id: string) => Promise<void>;
  ingestMaterialInboxFiles: (filePaths: string[]) => Promise<void>;
  receiveInboxFiles: (filePaths: string[]) => Promise<void>;
  confirmMaterialInboxItem: (itemId: string, projectRoot: string) => Promise<MaterialInboxRouteResult>;
  confirmInboxEntry: (itemId: string, projectRoot: string) => Promise<MaterialInboxRouteResult>;
  updateInboxRouteDecision: (itemId: string, projectRoot: string, semanticLocation: string) => Promise<void>;
  ignoreInboxEntry: (itemId: string) => Promise<void>;
  retryInboxEntry: (itemId: string) => Promise<void>;
  reanalyzeInboxEntry: (itemId: string) => Promise<void>;
  undoInboxRoute: (itemId: string) => Promise<void>;
  routeInboxItemToGlobal: (itemId: string, destination: string) => Promise<void>;
  saveInboxAutoRouteSetting: (enabled: boolean) => Promise<void>;
  createProjectFromMaterialInbox: (
    itemId: string,
    projectName: string,
    rootDir: string,
    description: string,
  ) => Promise<MaterialInboxRouteResult>;
  createProjectFromInbox: (
    itemId: string,
    projectName: string,
    rootDir: string,
    description: string,
  ) => Promise<MaterialInboxRouteResult>;
  refreshWeeklyReviewDashboard: () => Promise<void>;
  generateWeeklyReviews: (force?: boolean) => Promise<WeeklyReviewDashboard | null>;
  saveWeeklyReviewGenerationWeekday: (weekday: number) => Promise<void>;
  confirmWeeklyReport: (reportId: string) => Promise<WeeklyReportRecord | null>;
  exportWeeklyReport: (reportId: string, destinationDir: string) => Promise<void>;
  impactAnalyses: ProjectImpactAnalysis[];
  actionCandidates: ProjectActionCandidate[];
  stateProposals: ProjectStateProposal[];
  dailyContinue: DailyContinueSnapshot | null;
  todayWorkspace: TodayWorkspace | null;
  workLedger: WorkLedgerSnapshot | null;
  projectStateAutoApply: boolean;
  refreshWorkLedger: () => Promise<void>;
  refreshGitSnapshot: (repositoryPath?: string) => Promise<GitSnapshot>;
  recordUserDecisionEvent: (decision: string, reason?: string) => Promise<WorkEvent>;
  captureProjectFact: (request: ProjectFactCaptureRequest) => Promise<WorkEvent>;
  registerProjectGeneratedFile: (
    fileName: string,
    content: string,
    createdSource: string,
    relatedTask: string,
    purpose: string,
  ) => Promise<ManagedFile>;
  refreshProjectImpact: () => Promise<void>;
  refreshDailyContinue: () => Promise<void>;
  regenerateDailyContinue: () => Promise<void>;
  refreshTodayWorkspace: (options?: { refreshFacts?: boolean }) => Promise<void>;
  updateContinueProjectPreference: (
    projectId: string,
    options: { pinned?: boolean; snoozedUntil?: string },
  ) => Promise<void>;
  updateProjectAttention: (attentionId: string, status: "confirmed" | "ignored" | "later") => Promise<void>;
  updateDataHealthStatus: (recordId: string, status: "resolved" | "ignored") => Promise<void>;
  updatePendingReviewStatus: (reviewId: string, status: "open" | "pending" | "resolved" | "ignored") => Promise<void>;
  confirmActionCandidate: (candidateId: string) => Promise<void>;
  ignoreActionCandidate: (candidateId: string) => Promise<void>;
  updateActionCandidate: (
    candidateId: string,
    title: string,
    description: string,
    suggestedPriority: string,
    suggestedDueDate: string,
  ) => Promise<void>;
  applyStateProposal: (proposalId: string) => Promise<void>;
  undoStateProposal: (proposalId: string) => Promise<void>;
  markTodayDone: (completed: string, nextStep: string) => Promise<void>;
  setStateAutoApply: (enabled: boolean) => Promise<void>;
  lastInboxReceptionAt: string;
};

const STORAGE_KEY = "ganmaoyuan-ui-state-v0.4";
// Opening or returning to a project already performs an authoritative scan. Keep
// background monitoring deliberately quiet so hashing project files never competes
// with normal desktop work.
const PROJECT_MONITOR_INTERVAL_MS = 60_000;
const CODEX_RESULT_POLL_INTERVAL_MS = 12_000;

const demoMaterials = [
  { title: "感冒院项目定义 v0.1", detail: "定义产品定位、目标用户和第一版边界。" },
  { title: "感冒院用户工作流分析 v0.1", detail: "梳理打开、继续、整理资料、收工与恢复路径。" },
  { title: "感冒院 v0.1 PRD", detail: "说明导入、整理、搜索与上下文生成流程。" },
  { title: "感冒院 UI/UX 设计任务书", detail: "约束本轮只完成前端工程壳，不继续扩展功能。" },
];

const demoPendingMaterials = [
  { title: "OpenDesign 资料汇总.pdf", detail: "分类置信度偏低，建议人工确认。" },
  { title: "旧版数据导出.xls", detail: "2 个工作表读取失败，建议打开原文件检查后再导入。" },
];

const projectMemory = ["正式页面只保留启动页、工作页、设置页。", "感冒院 v0.1 的核心能力是项目文件位置掌控。"];
const aiContextItems = [
  "项目理解授权会立即发送项目名称/说明/下一步、最多 24 份资料的名称/类别/摘要/章节、最多 18 条近期消息（每条最多 1,000 字）及只读 Atlas 状态摘要；不发送资料正文、完整聊天历史或本地路径。",
  "聊天发送本轮问题（最多 8,000 字）、当前项目按问题筛选的可信记录，以及最多 5 轮历史对话（最近 3 轮，另加最多 2 轮相关历史；合计不超过 4,800 字，每条历史文本最多 700 字）。只有询问项目背景/目标时才附项目说明，只有询问下一步/当前工作时才附已确认下一步及相关状态。Work Ledger 最多 5 条、任务最多 4 条、成果和待确认事项各最多 3 条。命中时最多 3 份资料的摘要及正文片段（每份正文最多 1,800 字）。最多 4 张本轮图片（单张不超过 8 MB、总计不超过 20 MB）仅发送给支持视觉的模型；不附带历史图片或完整聊天历史。历史对话仅用于理解当前讨论，不作为已确认决定或事实。",
  "Inbox 匹配发送待处理资料摘要，以及最多 3 个已分别授权候选项目的限量说明、项目理解、资料/任务/决定摘要和匹配理由；不发送原文件或本地路径。API Key 仅作请求授权头。",
  "文件导入、复制、登记和打开由 Tauri 命令负责。",
];

const AppStateContext = createContext<AppStateValue | null>(null);

function readUiState() {
  try {
    const raw = window.localStorage.getItem(STORAGE_KEY);
    return raw
      ? (JSON.parse(raw) as { theme?: ThemeMode; selectedSettingsSection?: SettingsSection; lastProjectRoot?: string })
      : {};
  } catch {
    return {};
  }
}

export function AppProvider({ children }: { children: ReactNode }) {
  const uiState = readUiState();
  const [theme, setTheme] = useState<ThemeMode>(uiState.theme === "dark" ? "dark" : "light");
  const [selectedSettingsSection, setSelectedSettingsSection] = useState<SettingsSection>(uiState.selectedSettingsSection ?? "general");
  const [projects, setProjects] = useState<ProjectSummary[]>([]);
  const [activeProject, setActiveProject] = useState<ProjectSummary | null>(null);
  const [activeManifest, setActiveManifest] = useState<ProjectManifest | null>(null);
  const [memos, setMemos] = useState<MemoItem[]>([]);
  const [materialInbox, setMaterialInbox] = useState<MaterialInboxItem[]>([]);
  const [inboxRoutingSettings, setInboxRoutingSettings] = useState<InboxRoutingSettings>({
    autoRouteHighConfidence: false,
    updatedAt: "",
  });
  const [weeklyReviewDashboard, setWeeklyReviewDashboard] = useState<WeeklyReviewDashboard | null>(null);
  const [weeklyReviewSettings, setWeeklyReviewSettings] = useState<WeeklyReviewSettings | null>(null);
  const [impactAnalyses, setImpactAnalyses] = useState<ProjectImpactAnalysis[]>([]);
  const [actionCandidates, setActionCandidates] = useState<ProjectActionCandidate[]>([]);
  const [stateProposals, setStateProposals] = useState<ProjectStateProposal[]>([]);
  const [dailyContinue, setDailyContinue] = useState<DailyContinueSnapshot | null>(null);
  const [todayWorkspace, setTodayWorkspace] = useState<TodayWorkspace | null>(null);
  const [workLedger, setWorkLedger] = useState<WorkLedgerSnapshot | null>(null);
  const [codexTasks, setCodexTasks] = useState<CodexTask[]>([]);
  const projectViewRef = useRef({ root: "", generation: 0 });
  const displayedRootRef = useRef("");
  const ledgerRequestRef = useRef(0);
  const todayWorkspaceRequestRef = useRef(0);
  const dailyContinueRequestRef = useRef(0);
  const [projectStateAutoApply, setProjectStateAutoApply] = useState(false);
  const [deepSeekSettings, setDeepSeekSettings] = useState<DeepSeekSettings>({
    hasApiKey: false,
    selectedModelId: "",
    lastTestedAt: "",
  });
  const [deepSeekSettingsLoaded, setDeepSeekSettingsLoaded] = useState(false);
  const [deepSeekModels, setDeepSeekModels] = useState<DeepSeekModelInfo[]>([]);
  const [projectAiAuthorizationStatus, setProjectAiAuthorizationStatus] = useState<ProjectAiAuthorizationStatus>("idle");
  const [projectAiAuthorizationFeedback, setProjectAiAuthorizationFeedback] = useState("");
  const projectAiAuthorizationInFlightRef = useRef(false);
  const [isStreaming, setIsStreaming] = useState(false);
  const [lastInboxReceptionAt, setLastInboxReceptionAt] = useState("");
  const memosRef = useRef<MemoItem[]>([]);
  const memoSaveQueueRef = useRef<Promise<unknown>>(Promise.resolve());
  const [error, setError] = useState("");
  const [isStartupHydrated, setIsStartupHydrated] = useState(!isTauriRuntime());
  const isDesktopReady = isTauriRuntime();

  useLayoutEffect(() => {
    const root = activeProject?.rootDir ?? "";
    displayedRootRef.current = root;
    if (projectViewRef.current.root !== root) {
      projectViewRef.current = { root, generation: projectViewRef.current.generation + 1 };
      setWorkLedger(null);
      setCodexTasks([]);
    }
  }, [activeProject?.rootDir]);

  useEffect(() => () => {
    projectViewRef.current = { root: "", generation: projectViewRef.current.generation + 1 };
  }, []);

  function captureProjectView() {
    return { root: activeProject?.rootDir ?? "", generation: projectViewRef.current.generation };
  }

  function isCurrentProjectView(view: { root: string; generation: number }) {
    return view.root === projectViewRef.current.root && view.generation === projectViewRef.current.generation;
  }

  async function refreshScopedWorkLedger(view: { root: string; generation: number }) {
    if (!isCurrentProjectView(view)) return;
    const request = ++ledgerRequestRef.current;
    try {
      const ledger = await getDesktopWorkLedger(view.root);
      if (!isCurrentProjectView(view) || request !== ledgerRequestRef.current) return;
      setWorkLedger(ledger);
      setCodexTasks(ledger.codexTasks ?? []);
    } catch (err) {
      if (isCurrentProjectView(view) && request === ledgerRequestRef.current) throw err;
    }
  }

  async function synchronizeProjectFacts(view: { root: string; generation: number }) {
    if (!isCurrentProjectView(view) || !view.root) return;
    const todayRequest = ++todayWorkspaceRequestRef.current;
    const result = await synchronizeDesktopProjectFacts(view.root).catch(() => null);
    if (!isCurrentProjectView(view)) return;
    if (!result) {
      const scan = await scanDesktopProjectWorkspace(view.root);
      if (!isCurrentProjectView(view)) return;
      if (scan.changed) {
        setActiveManifest(scan.manifest);
        setActiveProject(scan.manifest.project);
        setProjects((previous) =>
          [scan.manifest.project, ...previous.filter((project) => project.id !== scan.manifest.project.id)].sort(
            (left, right) => right.lastOpenedAt.localeCompare(left.lastOpenedAt),
          ),
        );
      }
      const ledger = await getDesktopWorkLedger(view.root);
      if (!isCurrentProjectView(view)) return;
      setWorkLedger(ledger);
      setCodexTasks(ledger.codexTasks ?? []);
      await refreshTodayWorkspace();
      return;
    }
    setActiveManifest(result.manifest);
    setActiveProject(result.manifest.project);
    setProjects((previous) =>
      [result.manifest.project, ...previous.filter((project) => project.id !== result.manifest.project.id)].sort(
        (left, right) => right.lastOpenedAt.localeCompare(left.lastOpenedAt),
      ),
    );
    setWorkLedger(result.workLedger);
    setCodexTasks(result.workLedger.codexTasks ?? []);
    if (todayRequest === todayWorkspaceRequestRef.current) {
      setTodayWorkspace(result.todayWorkspace);
    }
  }

  useLayoutEffect(() => {
    document.documentElement.dataset.theme = theme;
  }, [theme]);

  useEffect(() => {
    const previous = readUiState();
    window.localStorage.setItem(
      STORAGE_KEY,
      JSON.stringify({
        theme,
        selectedSettingsSection,
        lastProjectRoot: activeProject?.rootDir ?? previous.lastProjectRoot,
      }),
    );
  }, [theme, selectedSettingsSection, activeProject]);

  useEffect(() => {
    memosRef.current = memos;
  }, [memos]);

  useEffect(() => {
    if (!isDesktopReady) return;
    let disposed = false;
    const hydrateStartup = async () => {
      try {
        await refreshProjects();
        await refreshTodayWorkspace();
      } catch (err) {
        if (!disposed) setError(String(err));
      } finally {
        if (!disposed) setIsStartupHydrated(true);
      }
    };
    void hydrateStartup();
    // The first frame only needs project continuity. Defer settings and secondary
    // panels so their I/O cannot compete with restoring the active project.
    const secondaryLoad = window.setTimeout(() => {
      void listMemos().then(setMemos).catch((err) => setError(String(err)));
      void listInboxEntries().then(setMaterialInbox).catch((err) => setError(String(err)));
      void loadDesktopInboxRoutingSettings().then(setInboxRoutingSettings).catch((err) => setError(String(err)));
      void loadDesktopDeepSeekSettings()
        .then((settings) => {
          setDeepSeekSettings(settings);
          setDeepSeekSettingsLoaded(true);
        })
        .catch((err) => setError(String(err)));
      void listDesktopDeepSeekModels().then(setDeepSeekModels).catch(() => undefined);
      void loadDesktopWeeklyReviewSettings().then(setWeeklyReviewSettings).catch((err) => setError(String(err)));
    }, 250);
    return () => {
      disposed = true;
      window.clearTimeout(secondaryLoad);
    };
  }, [isDesktopReady]);

  useEffect(() => {
    if (!isDesktopReady) return;
    let disposed = false;
    let busy = false;
    const poll = async () => {
      if (disposed || busy || document.visibilityState === "hidden") return;
      busy = true;
      try {
        const items = await consumeDesktopLaunchInboxEntries();
        if (!disposed && items.length) {
          await refineInboxItems(items);
          await refreshMaterialInbox();
          setLastInboxReceptionAt(new Date().toISOString());
        }
      } catch (err) {
        if (!disposed) setError(String(err));
      } finally {
        busy = false;
      }
    };
    void poll();
    const timer = window.setInterval(() => {
      void poll();
    }, 3000);
    return () => {
      disposed = true;
      window.clearInterval(timer);
    };
  }, [isDesktopReady]);

  useEffect(() => {
    if (!isDesktopReady || !activeProject) return;
    let disposed = false;
    let busy = false;
    let timer: number | undefined;
    const runScan = async () => {
      if (disposed || busy || document.visibilityState === "hidden") return;
      const view = captureProjectView();
      if (!isCurrentProjectView(view)) return;
      busy = true;
      try {
        const scan = await scanDesktopProjectWorkspace(activeProject.rootDir);
        if (!disposed && scan.changed && isCurrentProjectView(view)) {
          await synchronizeProjectFacts(view);
        }
      } catch {
        // Monitoring failures should not block normal work.
      } finally {
        busy = false;
      }
    };
    timer = window.setInterval(() => {
      void runScan();
    }, PROJECT_MONITOR_INTERVAL_MS);
    return () => {
      disposed = true;
      if (timer) window.clearInterval(timer);
    };
  }, [isDesktopReady, activeProject?.rootDir]);

  useEffect(() => {
    if (!isDesktopReady || !activeProject) return;
    let disposed = false;
    let busy = false;
    const shouldPoll = codexTasks.some((task) =>
      ["handedOff", "running", "awaitingResult", "resultReceived", "verifying"].includes(task.status),
    );
    const runBridgeScan = async () => {
      if (disposed || busy || document.visibilityState === "hidden") return;
      const view = captureProjectView();
      if (!isCurrentProjectView(view)) return;
      busy = true;
      try {
        const result = await scanDesktopCodexResultBridge(activeProject.rootDir);
        if (!disposed && isCurrentProjectView(view) && result.importedCount > 0) {
          setActiveManifest(result.manifest);
          setActiveProject(result.manifest.project);
          await refreshScopedWorkLedger(view);
          if (!disposed && isCurrentProjectView(view)) await refreshTodayWorkspace();
        }
      } catch (err) {
        if (!disposed && isCurrentProjectView(view)) setError(String(err));
      } finally {
        busy = false;
      }
    };
    void runBridgeScan();
    const timer = shouldPoll
      ? window.setInterval(() => {
          void runBridgeScan();
        }, CODEX_RESULT_POLL_INTERVAL_MS)
      : undefined;
    return () => {
      disposed = true;
      if (timer) window.clearInterval(timer);
    };
  }, [isDesktopReady, activeProject?.rootDir, codexTasks]);

  useEffect(() => {
    if (!isDesktopReady) return;
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void listen("codex-run-close-blocked", () => {
      if (!disposed) {
        setError("Codex 任务仍在执行，已暂缓关闭感冒院。请等待结果回流或先停止任务，再关闭应用。");
      }
    }).then((stopListening) => {
      if (disposed) stopListening();
      else unlisten = stopListening;
    }).catch((err) => {
      if (!disposed) setError(String(err));
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [isDesktopReady]);

  useEffect(() => {
    if (!isDesktopReady) return;
    let disposed = false;
    void listenDeepSeekStream((event) => {
      if (disposed) return;
      setActiveManifest((previous) => {
        if (!previous || previous.project.rootDir !== event.projectRoot) return previous;
        const nextMessages = previous.messages.map((message) =>
          message.id === event.messageId
            ? {
                ...message,
                text: event.text || message.text,
                status: event.status,
                source: event.modelId === "system" ? "system" : "deepseek",
                modelId: event.modelId || message.modelId,
              }
            : message,
        );
        return { ...previous, messages: nextMessages };
      });
      if (event.status === "streaming") {
        setIsStreaming(true);
      } else if (event.status === "completed") {
        setIsStreaming(false);
        void refreshDesktopProjectUnderstanding(event.projectRoot)
          .then((manifest) => {
            if (!disposed) {
              setActiveProject(manifest.project);
              setActiveManifest(manifest);
            }
          })
          .catch((err) => {
            if (!disposed) setError(String(err));
          });
      } else if (event.status === "error" || event.status === "cancelled") {
        setIsStreaming(false);
        setError(event.error || (event.status === "cancelled" ? "已停止生成。" : "DeepSeek 回复失败。"));
      }
    }).then((unlisten) => {
      if (disposed) {
        void unlisten();
      }
    });
    return () => {
      disposed = true;
    };
  }, [isDesktopReady]);

  async function refreshProjects() {
    if (!isDesktopReady) return;
    const generation = projectViewRef.current.generation;
    try {
      const nextProjects = await listProjects();
      setProjects(nextProjects);
      try {
        const notices = await takeStorageRecoveryNotices();
        if (notices.length) setError(notices.map((notice) => notice.message).join("\n"));
      } catch {
        // Older desktop builds may not expose recovery notices yet.
      }
      const lastRoot = readUiState().lastProjectRoot;
      const projectToLoad = nextProjects.find((item) => item.rootDir === lastRoot);
      if (projectToLoad && !displayedRootRef.current && generation === projectViewRef.current.generation) {
        await openProject(projectToLoad.rootDir);
      }
    } catch (err) {
      setError(String(err));
    }
  }

  async function refreshMaterialInbox() {
    if (!isDesktopReady) return;
    try {
      setMaterialInbox(await listInboxEntries());
    } catch (err) {
      setError(String(err));
    }
  }

  async function refineInboxItems(items: MaterialInboxItem[]) {
    for (const item of items) {
      if (item.aiMatchStatus === "notRequired" || item.processingStatus === "routed") continue;
      try {
        await refineDesktopInboxEntryWithAi(item.id);
      } catch (err) {
        // The Inbox record stores the AI failure and remains retryable; other files continue.
        setError(String(err));
      }
    }
  }

  async function openProject(projectRoot: string) {
    if (projectAiAuthorizationInFlightRef.current && projectRoot !== displayedRootRef.current) {
      throw new Error("正在保存或更新本项目的 AI 授权，请完成后再切换项目。");
    }
    const isChangingProject = projectRoot !== displayedRootRef.current;
    // Invalidate outstanding reads immediately, including A -> B -> A switches.
    const view = { root: projectRoot, generation: projectViewRef.current.generation + 1 };
    projectViewRef.current = view;
    try {
      const manifest = await loadProject(projectRoot);
      if (!isCurrentProjectView(view)) return;
      let openedProject = manifest.project;
      if (isTauriRuntime()) {
        try {
          openedProject = await markProjectOpened(projectRoot);
        } catch (err) {
          // Keep browser/test doubles and older desktop binaries readable; the
          // current binary registers this command and persists the timestamp.
          if (!String(err).includes("unexpected command")) throw err;
        }
      }
      if (!isCurrentProjectView(view)) return;
      setActiveProject(openedProject);
      setProjects((current) => [openedProject, ...current.filter((item) => item.id !== openedProject.id)]);
      setActiveManifest(manifest);
      setIsStreaming(!manifest.recoveredFromBackup && manifest.messages.some((message) => message.status === "streaming"));
      if (isChangingProject) {
        setProjectAiAuthorizationStatus("idle");
        setProjectAiAuthorizationFeedback("");
      }
      setWorkLedger(null);
      setCodexTasks([]);
      const recoveryMessages = [manifest.recoveryNotice ?? ""];
      if (isTauriRuntime()) {
        try {
          recoveryMessages.push(...(await takeStorageRecoveryNotices()).map((notice) => notice.message));
        } catch {
          // The loaded manifest still carries its project-specific recovery notice.
        }
      }
      setError([...new Set(recoveryMessages.filter(Boolean))].join("\n"));

      // Project switches keep the existing responsive behavior; startup is
      // gated separately until the initial registry and Today snapshot load.
      if (!manifest.recoveredFromBackup) {
        void synchronizeProjectFacts(view).catch((err) => {
          if (isCurrentProjectView(view)) setError(String(err));
        });
      }
    } catch (err) {
      if (!isCurrentProjectView(view)) return;
      projectViewRef.current = { root: displayedRootRef.current, generation: view.generation + 1 };
      throw err;
    }
  }

  async function createProject(name: string, rootDir: string, filePaths: string[], description: string) {
    const result = await createDesktopProject(name, rootDir, [], description);
    if (filePaths.length) {
      const inboxItems = await receiveDesktopInboxFiles(filePaths);
      for (const item of inboxItems) {
        if (item.processingStatus === "failed") continue;
        const prepared = await updateDesktopInboxRouteDecision(
          item.id,
          result.project.rootDir,
          semanticLocationForInboxItem(item),
        );
        await confirmDesktopInboxEntry(prepared.id, result.project.rootDir);
      }
    }
    let manifest = await loadProject(result.project.rootDir);
    try {
      manifest = await refreshDesktopProjectUnderstanding(result.project.rootDir);
    } catch {
      // Project creation must still succeed before DeepSeek authorization exists.
    }
    try {
      manifest = (await scanDesktopProjectWorkspace(result.project.rootDir)).manifest;
    } catch {
      // Monitoring remains best-effort.
    }
    setProjects(await listProjects());
    setActiveProject(manifest.project);
    setActiveManifest(manifest);
    await refreshMaterialInbox();
    setError("");
  }

  async function importFilesToActiveProject(filePaths: string[], _relatedTask: string) {
    if (!activeProject) throw new Error("请先选择项目。");
    const inboxItems = await receiveDesktopInboxFiles(filePaths);
    for (const item of inboxItems) {
      if (item.processingStatus === "failed") continue;
      const prepared = await updateDesktopInboxRouteDecision(
        item.id,
        activeProject.rootDir,
        semanticLocationForInboxItem(item),
      );
      await confirmDesktopInboxEntry(prepared.id, activeProject.rootDir);
    }
    let manifest = await loadProject(activeProject.rootDir);
    try {
      manifest = await refreshDesktopProjectUnderstanding(activeProject.rootDir);
    } catch {
      // Import remains a file-location operation; analysis can be refreshed after authorization.
    }
    try {
      manifest = (await scanDesktopProjectWorkspace(activeProject.rootDir)).manifest;
    } catch {
      // Monitoring remains best-effort.
    }
    setActiveManifest(manifest);
    setActiveProject(manifest.project);
    await refreshMaterialInbox();
    setError("");
  }

  async function ingestMaterialInboxFiles(filePaths: string[]) {
    if (!isDesktopReady || !filePaths.length) return;
    const items = await ingestDesktopMaterialInboxFiles(filePaths);
    await refineInboxItems(items);
    await refreshMaterialInbox();
    setError("");
  }

  async function receiveInboxFiles(filePaths: string[]) {
    if (!isDesktopReady || !filePaths.length) return;
    const items = await receiveDesktopInboxFiles(filePaths);
    await refineInboxItems(items);
    await refreshMaterialInbox();
    setError("");
  }

  async function confirmMaterialInboxItem(itemId: string, projectRoot: string) {
    const result = await confirmDesktopMaterialInboxItem(itemId, projectRoot);
    await refreshMaterialInbox();
    await refreshProjects();
    if (activeProject?.rootDir === projectRoot) {
      await openProject(projectRoot);
    }
    setError("");
    return result;
  }

  async function confirmInboxEntry(itemId: string, projectRoot: string) {
    const result = await confirmDesktopInboxEntry(itemId, projectRoot);
    await refreshMaterialInbox();
    await refreshProjects();
    if (activeProject?.rootDir === projectRoot) {
      await openProject(projectRoot);
    }
    setError("");
    return result;
  }

  async function updateInboxRouteDecision(itemId: string, projectRoot: string, semanticLocation: string) {
    await updateDesktopInboxRouteDecision(itemId, projectRoot, semanticLocation);
    await refreshMaterialInbox();
    setError("");
  }

  async function ignoreInboxEntry(itemId: string) {
    await ignoreDesktopInboxEntry(itemId);
    await refreshMaterialInbox();
    setError("");
  }

  async function retryInboxEntry(itemId: string) {
    const item = await retryDesktopInboxEntry(itemId);
    await refineInboxItems([item]);
    await refreshMaterialInbox();
  }

  async function reanalyzeInboxEntry(itemId: string) {
    await reanalyzeDesktopInboxEntry(itemId);
    await refreshMaterialInbox();
    setError("");
  }

  async function undoInboxRoute(itemId: string) {
    await undoDesktopInboxRoute(itemId);
    await refreshMaterialInbox();
    await refreshProjects();
    setError("");
  }

  async function routeInboxItemToGlobal(itemId: string, destination: string) {
    await routeDesktopInboxItemToGlobal(itemId, destination);
    await refreshMaterialInbox();
    setError("");
  }

  async function saveInboxAutoRouteSetting(enabled: boolean) {
    setInboxRoutingSettings(await saveDesktopInboxRoutingSettings(enabled));
    setError("");
  }

  async function createProjectFromMaterialInbox(itemId: string, projectName: string, rootDir: string, description: string) {
    const result = await createDesktopProjectFromMaterialInbox(itemId, projectName, rootDir, description);
    await refreshMaterialInbox();
    await refreshProjects();
    if (result.project?.rootDir) {
      await openProject(result.project.rootDir);
    }
    setError("");
    return result;
  }

  async function createProjectFromInbox(itemId: string, projectName: string, rootDir: string, description: string) {
    const result = await createDesktopProjectFromInbox(itemId, projectName, rootDir, description);
    await refreshMaterialInbox();
    await refreshProjects();
    if (result.project?.rootDir) {
      await openProject(result.project.rootDir);
    }
    setError("");
    return result;
  }

  async function sendProjectMessage(text: string, imageAttachments: ChatImageAttachmentInput[] = []) {
    const trimmed = text.trim();
    if (!trimmed || !activeProject) return;
    const view = captureProjectView();
    const result = await sendDesktopProjectMessage(view.root, trimmed, imageAttachments);
    if (!isCurrentProjectView(view)) return;
    const hasCanonicalReceipt = result.messages.some((message) =>
      message.evidenceItems.some((item) => item.contentType === "operationReceipt" && item.basisKind === "fact"),
    );
    setIsStreaming(Boolean(result.streamMessageId));
    setActiveProject(result.project);
    setActiveManifest((previous) =>
      previous
        ? {
            ...previous,
            project: result.project,
            messages: [...previous.messages, ...result.messages],
            draft: { ...previous.draft, text: "", updatedAt: new Date().toISOString() },
          }
        : previous,
    );
    setError("");

    if (hasCanonicalReceipt) {
      try {
        const [manifest, ledger] = await Promise.all([
          loadProject(view.root),
          getDesktopWorkLedger(view.root),
        ]);
        if (!isCurrentProjectView(view)) return;
        setActiveManifest(manifest);
        setActiveProject(manifest.project);
        setWorkLedger(ledger);
        setCodexTasks(ledger.codexTasks ?? []);
        setProjects((previous) =>
          [manifest.project, ...previous.filter((project) => project.id !== manifest.project.id)].sort(
            (left, right) => right.lastOpenedAt.localeCompare(left.lastOpenedAt),
          ),
        );
      } catch (err) {
        if (isCurrentProjectView(view)) {
          setError(`项目记录已有可核验回执，但刷新界面失败：${String(err)}`);
        }
      }
    }
  }

  async function retryProjectMessage(interruptedMessageId: string) {
    if (!activeProject) return;
    const view = captureProjectView();
    try {
      const result = await retryDesktopInterruptedProjectMessage(view.root, interruptedMessageId);
      if (!isCurrentProjectView(view)) return;
      setIsStreaming(true);
      setActiveProject(result.project);
      setActiveManifest((previous) =>
        previous
          ? { ...previous, project: result.project, messages: [...previous.messages, ...result.messages] }
          : previous,
      );
      setError("");
    } catch (err) {
      if (isCurrentProjectView(view)) setError(String(err));
    }
  }

  async function stopProjectMessage() {
    if (!activeProject) return;
    const view = captureProjectView();
    await stopDesktopProjectMessage(view.root);
  }

  async function saveProjectDraft(text: string, pendingFilePaths: string[] = []) {
    if (!activeProject) return;
    const view = captureProjectView();
    const draft = await saveDesktopProjectDraft(view.root, text, pendingFilePaths);
    if (!isCurrentProjectView(view)) return;
    setActiveManifest((previous) => (previous ? { ...previous, draft } : previous));
  }

  async function finishProjectWork(done: string, nextStep: string) {
    if (!activeProject) return;
    const view = captureProjectView();
    const result = await finishDesktopProjectWork(activeProject.rootDir, done, nextStep);
    if (!isCurrentProjectView(view)) return;
    setActiveProject(result.project);
    setActiveManifest((previous) =>
      previous
        ? {
            ...previous,
            project: result.project,
            messages: [...previous.messages, result.message],
            recoveryPoints: [...previous.recoveryPoints, result.recoveryPoint],
            draft: { ...previous.draft, text: "", pendingFilePaths: [] },
          }
        : previous,
    );
    setProjects((previous) =>
      [result.project, ...previous.filter((project) => project.id !== result.project.id)].sort((left, right) =>
        right.lastOpenedAt.localeCompare(left.lastOpenedAt),
      ),
    );
    await refreshScopedWorkLedger(view);
    if (!isCurrentProjectView(view)) return;
    await refreshDailyContinue();
    if (!isCurrentProjectView(view)) return;
    await refreshTodayWorkspace();
    if (!isCurrentProjectView(view)) return;
    setError("");
  }

  async function generateCodexPrompt() {
    if (!activeProject) throw new Error("请先打开一个项目。");
    const view = captureProjectView();
    const result = await generateDesktopCodexPrompt(activeProject.rootDir);
    // The task is saved in its project, but an obsolete view must not navigate to it.
    if (!isCurrentProjectView(view)) return null;
    setActiveManifest(result.manifest);
    setActiveProject(result.manifest.project);
    setProjects((previous) =>
      [result.manifest.project, ...previous.filter((project) => project.id !== result.manifest.project.id)].sort((left, right) =>
        right.lastOpenedAt.localeCompare(left.lastOpenedAt),
      ),
    );
    await refreshScopedWorkLedger(view);
    if (!isCurrentProjectView(view)) return null;
    setError("");
    return result;
  }

  async function createCodexTask(request: CodexTaskCreateRequest) {
    if (!activeProject) throw new Error("请先打开一个项目。");
    const view = captureProjectView();
    const task = await createDesktopCodexTask(activeProject.rootDir, request);
    await refreshScopedWorkLedger(view);
    if (!isCurrentProjectView(view)) return null;
    setError("");
    return task;
  }

  async function markCodexTaskHandedOff(taskId: string) {
    if (!activeProject) throw new Error("请先打开一个项目。");
    const view = captureProjectView();
    const task = await markDesktopCodexTaskHandedOff(activeProject.rootDir, taskId);
    await refreshScopedWorkLedger(view);
    if (!isCurrentProjectView(view)) return task;
    await refreshTodayWorkspace();
    if (isCurrentProjectView(view)) setError("");
    return task;
  }

  async function acceptCodexTask(taskId: string, resultId: string) {
    if (!activeProject) throw new Error("请先打开一个项目。");
    const view = captureProjectView();
    const task = await acceptDesktopCodexTask(activeProject.rootDir, taskId, resultId);
    await refreshScopedWorkLedger(view);
    if (!isCurrentProjectView(view)) return task;
    await refreshTodayWorkspace();
    if (isCurrentProjectView(view)) setError("");
    return task;
  }

  async function rejectCodexTask(taskId: string, resultId: string, reason?: string) {
    if (!activeProject) throw new Error("请先打开一个项目。");
    const view = captureProjectView();
    const task = await rejectDesktopCodexTask(activeProject.rootDir, taskId, resultId, reason);
    await refreshScopedWorkLedger(view);
    if (!isCurrentProjectView(view)) return task;
    await refreshTodayWorkspace();
    if (isCurrentProjectView(view)) setError("");
    return task;
  }

  async function importCodexReportText(text: string) {
    if (!activeProject) throw new Error("请先打开一个项目。");
    const view = captureProjectView();
    const result = await importDesktopCodexReportText(activeProject.rootDir, text);
    if (!isCurrentProjectView(view)) return result;
    setActiveManifest(result.manifest);
    setActiveProject(result.manifest.project);
    await refreshScopedWorkLedger(view);
    if (!isCurrentProjectView(view)) return result;
    await refreshTodayWorkspace();
    if (isCurrentProjectView(view)) setError("");
    return result;
  }

  async function importCodexReportFile(path: string) {
    if (!activeProject) throw new Error("请先打开一个项目。");
    const view = captureProjectView();
    const result = await importDesktopCodexReportFile(activeProject.rootDir, path);
    if (!isCurrentProjectView(view)) return result;
    setActiveManifest(result.manifest);
    setActiveProject(result.manifest.project);
    await refreshScopedWorkLedger(view);
    if (!isCurrentProjectView(view)) return result;
    await refreshTodayWorkspace();
    if (isCurrentProjectView(view)) setError("");
    return result;
  }

  async function applyCodexReport(reportId: string) {
    if (!activeProject) throw new Error("请先打开一个项目。");
    const view = captureProjectView();
    const result = await applyDesktopCodexReport(activeProject.rootDir, reportId);
    if (!isCurrentProjectView(view)) return result;
    setActiveManifest(result.manifest);
    setActiveProject(result.manifest.project);
    await refreshScopedWorkLedger(view);
    if (!isCurrentProjectView(view)) return result;
    await refreshTodayWorkspace();
    setProjects((previous) =>
      [result.manifest.project, ...previous.filter((project) => project.id !== result.manifest.project.id)].sort((left, right) =>
        right.lastOpenedAt.localeCompare(left.lastOpenedAt),
      ),
    );
    if (isCurrentProjectView(view)) setError("");
    return result;
  }

  async function saveDeepSeekApiKey(apiKey: string, selectedModelId: string) {
    const settings = await saveDesktopDeepSeekApiKey(apiKey, selectedModelId);
    setDeepSeekSettings(settings);
    setDeepSeekSettingsLoaded(true);
    setError("");
  }

  async function deleteDeepSeekApiKey() {
    const settings = await deleteDesktopDeepSeekApiKey();
    setDeepSeekSettings(settings);
    setDeepSeekSettingsLoaded(true);
    setDeepSeekModels([]);
    setError("");
  }

  async function testDeepSeekConnection(selectedModelId: string) {
    const result = await testDesktopDeepSeekConnection(selectedModelId);
    setDeepSeekSettings(result.settings);
    setDeepSeekModels(result.models);
    setError("");
  }

  async function grantProjectDeepSeekAuthorization() {
    if (projectAiAuthorizationInFlightRef.current) return;
    if (!activeProject) {
      setProjectAiAuthorizationStatus("failed");
      setProjectAiAuthorizationFeedback("请先打开一个项目。");
      return;
    }
    projectAiAuthorizationInFlightRef.current = true;
    const view = captureProjectView();
    setProjectAiAuthorizationStatus("saving");
    setProjectAiAuthorizationFeedback("正在保存本项目的 AI 授权…");
    setError("");
    try {
      const grantedManifest = await grantDesktopDeepSeekAuthorization(view.root);
      if (!isCurrentProjectView(view)) return;
      setActiveManifest(grantedManifest);
      setActiveProject(grantedManifest.project);
      setProjectAiAuthorizationStatus("refreshing");
      setProjectAiAuthorizationFeedback("授权已保存，正在更新项目理解…");

      try {
        const refreshedManifest = await refreshDesktopProjectUnderstanding(view.root);
        if (!isCurrentProjectView(view)) return;
        setActiveManifest(refreshedManifest);
        setActiveProject(refreshedManifest.project);
        setProjectAiAuthorizationStatus("success");
        setProjectAiAuthorizationFeedback("AI 回复授权已保存，项目理解已更新。");
      } catch (err) {
        if (isCurrentProjectView(view)) {
          const message = err instanceof Error ? err.message : String(err);
          const status = projectAiAuthorizationFailureStatus(message);
          const reason = status === "cancelled" ? "已取消" : status === "timeout" ? "超时" : "失败";
          setProjectAiAuthorizationStatus(status);
          setProjectAiAuthorizationFeedback(`AI 回复授权已保存，但项目理解更新${reason}：${message}`);
        }
      }
    } catch (err) {
      if (isCurrentProjectView(view)) {
        const message = err instanceof Error ? err.message : String(err);
        const status = projectAiAuthorizationFailureStatus(message);
        const reason = status === "cancelled" ? "已取消" : status === "timeout" ? "请求超时" : "失败";
        setProjectAiAuthorizationStatus(status);
        setProjectAiAuthorizationFeedback(`AI 授权${reason}：${message}`);
      }
    } finally {
      projectAiAuthorizationInFlightRef.current = false;
    }
  }

  async function refreshProjectUnderstanding() {
    if (!activeProject) throw new Error("请先打开一个项目。");
    const manifest = await refreshDesktopProjectUnderstanding(activeProject.rootDir);
    setActiveManifest(manifest);
    setActiveProject(manifest.project);
    setError("");
  }

  async function refreshWeeklyReviewDashboard() {
    const dashboard = await loadDesktopWeeklyReviewDashboard(activeProject?.rootDir ?? readUiState().lastProjectRoot ?? null);
    setWeeklyReviewDashboard(dashboard);
    const settings = await loadDesktopWeeklyReviewSettings();
    setWeeklyReviewSettings(settings);
  }

  async function generateWeeklyReviews(force = false) {
    const dashboard = weeklyReviewDashboard ?? (await loadDesktopWeeklyReviewDashboard(activeProject?.rootDir ?? null));
    const generated = await generateDesktopWeeklyReviews(
      dashboard.activeProjectRoot || null,
      dashboard.currentWeekKey,
      dashboard.weekStart,
      dashboard.weekEnd,
      force,
    );
    setWeeklyReviewDashboard(generated);
    setWeeklyReviewSettings(generated.settings);
    return generated;
  }

  async function saveWeeklyReviewGenerationWeekday(weekday: number) {
    const settings = await saveDesktopWeeklyReviewSettings(weekday);
    setWeeklyReviewSettings(settings);
    if (weeklyReviewDashboard) {
      setWeeklyReviewDashboard({ ...weeklyReviewDashboard, settings });
    }
  }

  async function confirmWeeklyReport(reportId: string) {
    const dashboard = weeklyReviewDashboard ?? (await loadDesktopWeeklyReviewDashboard(activeProject?.rootDir ?? null));
    const target = dashboard.projectReports.find((item) => item.id === reportId) ?? dashboard.globalReports.find((item) => item.id === reportId);
    if (!target) return null;
    const report = await confirmDesktopWeeklyReport(target.projectRoot, reportId);
    await refreshWeeklyReviewDashboard();
    return report;
  }

  async function exportWeeklyReport(reportId: string, destinationDir: string) {
    const dashboard = weeklyReviewDashboard ?? (await loadDesktopWeeklyReviewDashboard(activeProject?.rootDir ?? null));
    const target = dashboard.projectReports.find((item) => item.id === reportId) ?? dashboard.globalReports.find((item) => item.id === reportId);
    if (!target) return;
    await exportDesktopWeeklyReport(target.projectRoot, reportId, destinationDir);
  }

  async function refreshProjectImpact(view = captureProjectView()) {
    if (!view.root || !isCurrentProjectView(view)) return;
    try {
      const [impacts, candidates, proposals] = await Promise.all([
        getDesktopImpactAnalyses(view.root),
        getDesktopActionCandidates(view.root),
        getDesktopStateProposals(view.root),
      ]);
      if (!isCurrentProjectView(view)) return;
      const manifest = await loadProject(view.root);
      if (!isCurrentProjectView(view)) return;
      setImpactAnalyses(impacts);
      setActionCandidates(candidates);
      setStateProposals(proposals);
      setActiveManifest(manifest);
      setActiveProject(manifest.project);
    } catch (err) {
      if (isCurrentProjectView(view)) setError(String(err));
    }
  }

  async function refreshDailyContinue() {
    const view = captureProjectView();
    if (!view.root || !isCurrentProjectView(view)) return;
    const request = ++dailyContinueRequestRef.current;
    try {
      const snapshot = await getDesktopDailyContinue(view.root);
      if (!isCurrentProjectView(view) || request !== dailyContinueRequestRef.current) return;
      setDailyContinue(snapshot);
    } catch (err) {
      if (isCurrentProjectView(view) && request === dailyContinueRequestRef.current) setError(String(err));
    }
  }

  async function regenerateDailyContinueAction() {
    const view = captureProjectView();
    if (!view.root || !isCurrentProjectView(view)) return;
    const request = ++dailyContinueRequestRef.current;
    try {
      const snapshot = await regenerateDesktopDailyContinue(view.root);
      if (!isCurrentProjectView(view) || request !== dailyContinueRequestRef.current) return;
      setDailyContinue(snapshot);
      setError("");
    } catch (err) {
      if (isCurrentProjectView(view) && request === dailyContinueRequestRef.current) setError(String(err));
    }
  }

  async function refreshTodayWorkspace(options?: { refreshFacts?: boolean }) {
    if (!isDesktopReady) return;
    const request = ++todayWorkspaceRequestRef.current;
    try {
      const workspace = await (options?.refreshFacts
        ? refreshDesktopTodayWorkspace()
        : getDesktopTodayWorkspace());
      if (request !== todayWorkspaceRequestRef.current) return;
      setTodayWorkspace(workspace);
      setError("");
    } catch (err) {
      if (request === todayWorkspaceRequestRef.current) setError(String(err));
    }
  }

  async function updateContinueProjectPreference(
    projectId: string,
    options: { pinned?: boolean; snoozedUntil?: string },
  ) {
    const request = ++todayWorkspaceRequestRef.current;
    const workspace = await updateDesktopContinueProjectPreference(projectId, options);
    if (request === todayWorkspaceRequestRef.current) setTodayWorkspace(workspace);
  }

  async function refreshWorkLedger() {
    if (!activeProject) return;
    await refreshScopedWorkLedger(captureProjectView());
  }

  async function refreshGitSnapshot(repositoryPath?: string) {
    if (!activeProject) throw new Error("请先打开一个项目。");
    const view = captureProjectView();
    const snapshot = await refreshDesktopGitSnapshot(activeProject.rootDir, repositoryPath);
    await refreshScopedWorkLedger(view);
    if (!isCurrentProjectView(view)) return snapshot;
    await refreshTodayWorkspace();
    if (isCurrentProjectView(view)) setError("");
    return snapshot;
  }

  async function recordUserDecisionEvent(decision: string, reason?: string) {
    if (!activeProject) throw new Error("请先打开一个项目。");
    const view = captureProjectView();
    const projectRoot = activeProject.rootDir;
    const event = await recordDesktopUserDecisionEvent(projectRoot, decision, reason);
    if (!isCurrentProjectView(view)) return event;
    setWorkLedger((previous) => {
      if (!previous) {
        return { events: [event], gitSnapshot: null, codexResults: [], codexTasks: [] };
      }
      const exists = previous.events.some(
        (item) =>
          item.sourceType === event.sourceType &&
          item.sourceRef === event.sourceRef &&
          item.eventType === event.eventType,
      );
      return exists ? previous : { ...previous, events: [...previous.events, event] };
    });
    const manifest = await loadProject(projectRoot);
    if (!isCurrentProjectView(view)) return event;
    setActiveManifest(manifest);
    setActiveProject(manifest.project);
    await refreshScopedWorkLedger(view);
    if (!isCurrentProjectView(view)) return event;
    await refreshTodayWorkspace();
    if (isCurrentProjectView(view)) setError("");
    return event;
  }

  async function captureProjectFact(request: ProjectFactCaptureRequest) {
    if (!activeProject) throw new Error("请先打开一个项目。");
    const view = captureProjectView();
    const event = await captureDesktopProjectFact(activeProject.rootDir, request);
    if (!isCurrentProjectView(view)) return event;
    const manifest = await loadProject(view.root);
    if (!isCurrentProjectView(view)) return event;
    setActiveManifest(manifest);
    setActiveProject(manifest.project);
    setProjects((previous) =>
      [manifest.project, ...previous.filter((project) => project.id !== manifest.project.id)].sort(
        (left, right) => right.lastOpenedAt.localeCompare(left.lastOpenedAt),
      ),
    );
    await refreshScopedWorkLedger(view);
    if (!isCurrentProjectView(view)) return event;
    await refreshTodayWorkspace();
    if (isCurrentProjectView(view)) setError("");
    return event;
  }

  async function registerProjectGeneratedFile(
    fileName: string,
    content: string,
    createdSource: string,
    relatedTask: string,
    purpose: string,
  ) {
    if (!activeProject) throw new Error("请先打开一个项目。");
    const view = captureProjectView();
    const result = await registerDesktopGeneratedFile(
      view.root,
      fileName,
      content,
      createdSource,
      relatedTask,
      purpose,
    );
    if (!isCurrentProjectView(view)) return result.file;
    const manifest = await loadProject(view.root);
    if (!isCurrentProjectView(view)) return result.file;
    setActiveManifest(manifest);
    setActiveProject(manifest.project);
    setProjects((previous) =>
      [manifest.project, ...previous.filter((project) => project.id !== manifest.project.id)].sort(
        (left, right) => right.lastOpenedAt.localeCompare(left.lastOpenedAt),
      ),
    );
    await refreshScopedWorkLedger(view);
    if (!isCurrentProjectView(view)) return result.file;
    await refreshTodayWorkspace();
    if (isCurrentProjectView(view)) setError("");
    return result.file;
  }

  async function updateProjectAttentionAction(attentionId: string, status: "confirmed" | "ignored" | "later") {
    if (!activeProject) return;
    const view = captureProjectView();
    try {
      await updateDesktopProjectAttention(view.root, attentionId, status);
      if (!isCurrentProjectView(view)) return;
      const manifest = await loadProject(view.root);
      if (!isCurrentProjectView(view)) return;
      setActiveManifest(manifest);
      setActiveProject(manifest.project);
      await refreshTodayWorkspace();
      if (!isCurrentProjectView(view)) return;
      setError("");
    } catch (err) {
      if (isCurrentProjectView(view)) setError(String(err));
    }
  }

  async function updateDataHealthStatusAction(recordId: string, status: "resolved" | "ignored") {
    if (!activeProject) return;
    const view = captureProjectView();
    ++dailyContinueRequestRef.current;
    try {
      const manifest = await updateDesktopDataHealthStatus(view.root, recordId, status);
      if (!isCurrentProjectView(view)) return;
      setActiveManifest(manifest);
      setActiveProject(manifest.project);
      await refreshDailyContinue();
      if (!isCurrentProjectView(view)) return;
      await refreshTodayWorkspace();
      if (!isCurrentProjectView(view)) return;
      setError("");
    } catch (err) {
      if (isCurrentProjectView(view)) setError(String(err));
    }
  }

  async function updatePendingReviewStatusAction(reviewId: string, status: "open" | "pending" | "resolved" | "ignored") {
    if (!activeProject) return;
    const view = captureProjectView();
    ++dailyContinueRequestRef.current;
    try {
      const manifest = await updateDesktopPendingReviewStatus(view.root, reviewId, status);
      if (!isCurrentProjectView(view)) return;
      setActiveManifest(manifest);
      setActiveProject(manifest.project);
      await refreshDailyContinue();
      if (!isCurrentProjectView(view)) return;
      await refreshTodayWorkspace();
      if (!isCurrentProjectView(view)) return;
      setError("");
    } catch (err) {
      if (isCurrentProjectView(view)) setError(String(err));
    }
  }

  async function confirmActionCandidateAction(candidateId: string) {
    if (!activeProject) return;
    const view = captureProjectView();
    try {
      await confirmDesktopActionCandidate(view.root, candidateId);
      if (!isCurrentProjectView(view)) return;
      await refreshProjectImpact(view);
      if (isCurrentProjectView(view)) setError("");
    } catch (err) {
      if (isCurrentProjectView(view)) setError(String(err));
    }
  }

  async function ignoreActionCandidateAction(candidateId: string) {
    if (!activeProject) return;
    const view = captureProjectView();
    try {
      await ignoreDesktopActionCandidate(view.root, candidateId);
      if (!isCurrentProjectView(view)) return;
      await refreshProjectImpact(view);
      if (isCurrentProjectView(view)) setError("");
    } catch (err) {
      if (isCurrentProjectView(view)) setError(String(err));
    }
  }

  async function updateActionCandidateAction(
    candidateId: string,
    title: string,
    description: string,
    suggestedPriority: string,
    suggestedDueDate: string,
  ) {
    if (!activeProject) return;
    const view = captureProjectView();
    try {
      await updateDesktopActionCandidate(view.root, candidateId, title, description, suggestedPriority, suggestedDueDate);
      if (!isCurrentProjectView(view)) return;
      await refreshProjectImpact(view);
      if (isCurrentProjectView(view)) setError("");
    } catch (err) {
      if (isCurrentProjectView(view)) setError(String(err));
    }
  }

  async function applyStateProposalAction(proposalId: string) {
    if (!activeProject) return;
    const view = captureProjectView();
    try {
      await applyDesktopStateProposal(view.root, proposalId);
      if (!isCurrentProjectView(view)) return;
      await refreshProjectImpact(view);
      if (isCurrentProjectView(view)) setError("");
    } catch (err) {
      if (isCurrentProjectView(view)) setError(String(err));
    }
  }

  async function undoStateProposalAction(proposalId: string) {
    if (!activeProject) return;
    const view = captureProjectView();
    try {
      await undoDesktopStateProposal(view.root, proposalId);
      if (!isCurrentProjectView(view)) return;
      await refreshProjectImpact(view);
      if (isCurrentProjectView(view)) setError("");
    } catch (err) {
      if (isCurrentProjectView(view)) setError(String(err));
    }
  }

  async function markTodayDoneAction(completed: string, nextStep: string) {
    if (!activeProject) return;
    try {
      const view = captureProjectView();
      const result = await markDesktopTodayDone(view.root, completed, nextStep);
      if (!isCurrentProjectView(view)) return;
      const dailyContinueRequest = ++dailyContinueRequestRef.current;
      setActiveProject(result.project);
      setActiveManifest((previous) =>
        previous
          ? {
              ...previous,
              project: result.project,
              messages: result.message.id ? [...previous.messages, result.message] : previous.messages,
              recoveryPoints: [...previous.recoveryPoints, result.recoveryPoint],
              draft: { ...previous.draft, text: "", pendingFilePaths: [] },
            }
          : previous,
      );
      setProjects((previous) =>
        [result.project, ...previous.filter((project) => project.id !== result.project.id)].sort((left, right) =>
          right.lastOpenedAt.localeCompare(left.lastOpenedAt),
        ),
      );
      await refreshScopedWorkLedger(view);
      if (!isCurrentProjectView(view)) return;
      const dailyContinue = await getDesktopDailyContinue(view.root);
      if (!isCurrentProjectView(view)) return;
      if (dailyContinueRequest !== dailyContinueRequestRef.current) return;
      setDailyContinue(dailyContinue);
      await refreshTodayWorkspace();
      if (!isCurrentProjectView(view)) return;
      setError("");
    } catch (err) {
      setError(String(err));
    }
  }

  async function setStateAutoApplyAction(enabled: boolean) {
    if (!activeProject) return;
    try {
      const manifest = await setDesktopStateAutoApply(activeProject.rootDir, enabled);
      setProjectStateAutoApply(manifest.projectStateAutoApply);
      setActiveManifest(manifest);
      setActiveProject(manifest.project);
      setError("");
    } catch (err) {
      setError(String(err));
    }
  }

  async function persistMemos(nextMemos: MemoItem[]) {
    memosRef.current = nextMemos;
    setMemos(nextMemos);
    if (isDesktopReady) {
      const save = memoSaveQueueRef.current.then(() => saveMemos(nextMemos));
      memoSaveQueueRef.current = save.catch(() => undefined);
      await save;
    }
  }

  const value = useMemo<AppStateValue>(
    () => ({
      theme,
      toggleTheme: () => setTheme((prev) => (prev === "dark" ? "light" : "dark")),
      currentTask: activeProject?.name ?? "感冒院项目工作台",
      nextStep: activeProject?.nextStep ?? "选择历史项目或新建项目。",
      worklog: activeManifest?.messages.map((item) => item.text).join("\n\n") ?? "",
      setWorklog: () => undefined,
      hasResumeSession: projects.length > 0,
      lastCompletedAt: activeProject?.lastOpenedAt ?? null,
      materialCount: activeManifest?.files.length ?? 0,
      addMaterials: () => undefined,
      saveProgress: () => undefined,
      markWorkStarted: () => undefined,
      completeWork: () => undefined,
      selectedSettingsSection,
      setSelectedSettingsSection,
      materials: demoMaterials,
      pendingMaterials: demoPendingMaterials,
      projectMemory,
      aiContextItems,
      deepSeekSettings,
      deepSeekSettingsLoaded,
      deepSeekModels,
      projectAiAuthorizationStatus,
      projectAiAuthorizationFeedback,
      isStreaming,
      isStartupHydrated,
      isDesktopReady,
      projects,
      activeProject,
      activeManifest,
      workspaceMessages: activeManifest?.messages ?? [],
      tasks: activeManifest?.tasks ?? [],
      decisions: activeManifest?.decisions ?? [],
      artifacts: activeManifest?.artifacts ?? [],
      codexPrompts: activeManifest?.codexPrompts ?? [],
      codexReports: activeManifest?.codexReports ?? [],
      codexTasks,
      dailySessions: activeManifest?.dailySessions ?? [],
      locationDecisions: activeManifest?.locationDecisions ?? [],
      pendingReviews: activeManifest?.pendingReviews ?? [],
      monitoring: activeManifest?.monitoring ?? null,
      atlas: activeManifest?.atlas ?? null,
      projectAnalysis: activeManifest?.projectAnalysis ?? null,
      managedFiles: activeManifest?.files ?? [],
      memos,
      materialInbox,
      inboxRoutingSettings,
      weeklyReviewDashboard,
      weeklyReviewSettings,
      weeklyReports: [
        ...(weeklyReviewDashboard?.globalReports ?? []),
        ...(weeklyReviewDashboard?.projectReports ?? []),
      ],
      lastInboxReceptionAt,
      error,
      setError,
      refreshProjects,
      openProject,
      createProject,
      importFilesToActiveProject,
      sendProjectMessage,
      retryProjectMessage,
      stopProjectMessage,
      saveProjectDraft,
      finishProjectWork,
      generateCodexPrompt,
      createCodexTask,
      markCodexTaskHandedOff,
      acceptCodexTask,
      rejectCodexTask,
      importCodexReportText,
      importCodexReportFile,
      applyCodexReport,
      saveDeepSeekApiKey,
      deleteDeepSeekApiKey,
      testDeepSeekConnection,
      grantProjectDeepSeekAuthorization,
      refreshProjectUnderstanding,
      openFilePath: async (path) => {
        try {
          await openManagedFile(path);
          setError("");
        } catch (err) {
          setError(String(err));
          throw err;
        }
      },
      openFolderPath: async (path) => {
        try {
          await openManagedFolder(path);
          setError("");
        } catch (err) {
          setError(String(err));
          throw err;
        }
      },
      addMemo: async (text) => {
        const now = new Date().toISOString();
        await persistMemos([{ id: crypto.randomUUID(), text, createdAt: now, updatedAt: now }, ...memosRef.current]);
      },
      updateMemo: async (id, text) => {
        const now = new Date().toISOString();
        await persistMemos(memosRef.current.map((item) => (item.id === id ? { ...item, text, updatedAt: now } : item)));
      },
      deleteMemo: async (id) => {
        await persistMemos(memosRef.current.filter((item) => item.id !== id));
      },
      ingestMaterialInboxFiles,
      receiveInboxFiles,
      confirmMaterialInboxItem,
      confirmInboxEntry,
      updateInboxRouteDecision,
      ignoreInboxEntry,
      retryInboxEntry,
      reanalyzeInboxEntry,
      undoInboxRoute,
      routeInboxItemToGlobal,
      saveInboxAutoRouteSetting,
      createProjectFromMaterialInbox,
      createProjectFromInbox,
      refreshWeeklyReviewDashboard,
      generateWeeklyReviews,
      saveWeeklyReviewGenerationWeekday,
      confirmWeeklyReport,
      exportWeeklyReport,
      impactAnalyses,
      actionCandidates,
      stateProposals,
      dailyContinue,
      todayWorkspace,
      workLedger,
      projectStateAutoApply,
      refreshWorkLedger,
      refreshGitSnapshot,
      recordUserDecisionEvent,
      captureProjectFact,
      registerProjectGeneratedFile,
      refreshProjectImpact,
      refreshDailyContinue,
      regenerateDailyContinue: regenerateDailyContinueAction,
      refreshTodayWorkspace,
      updateContinueProjectPreference,
      updateProjectAttention: updateProjectAttentionAction,
      updateDataHealthStatus: updateDataHealthStatusAction,
      updatePendingReviewStatus: updatePendingReviewStatusAction,
      confirmActionCandidate: confirmActionCandidateAction,
      ignoreActionCandidate: ignoreActionCandidateAction,
      updateActionCandidate: updateActionCandidateAction,
      applyStateProposal: applyStateProposalAction,
      undoStateProposal: undoStateProposalAction,
      markTodayDone: markTodayDoneAction,
      setStateAutoApply: setStateAutoApplyAction,
    }),
    [
      theme,
      selectedSettingsSection,
      projects,
      activeProject,
      activeManifest,
      codexTasks,
      memos,
      materialInbox,
      inboxRoutingSettings,
      weeklyReviewDashboard,
      weeklyReviewSettings,
      lastInboxReceptionAt,
      error,
      impactAnalyses,
      actionCandidates,
      stateProposals,
      dailyContinue,
      todayWorkspace,
      workLedger,
      projectStateAutoApply,
      refreshProjectImpact,
      refreshDailyContinue,
      regenerateDailyContinueAction,
      isDesktopReady,
      deepSeekSettings,
      deepSeekSettingsLoaded,
      deepSeekModels,
      projectAiAuthorizationStatus,
      projectAiAuthorizationFeedback,
      isStreaming,
    ],
  );

  return <AppStateContext.Provider value={value}>{children}</AppStateContext.Provider>;
}

function semanticLocationForInboxItem(item: MaterialInboxItem) {
  if (item.recommendedRelativeLocation) return item.recommendedRelativeLocation;
  const categoryMap: Record<string, string> = {
    需求文档: "requirements",
    项目资料: "requirements",
    会议记录: "meetings",
    方案文档: "design",
    方案演示: "design",
    图片资料: "design",
    测试资料: "test",
    数据表格: "data",
    交付文件: "delivery",
    产出文件: "reports",
    提示词: "development",
  };
  return categoryMap[item.recommendedCategory] || "other";
}

function projectAiAuthorizationFailureStatus(message: string): "failed" | "cancelled" | "timeout" {
  const normalized = message.toLocaleLowerCase();
  if (/\b(cancelled|canceled|abort(?:ed)?)\b/.test(normalized) || /取消/.test(message)) {
    return "cancelled";
  }
  if (/\b(timeout|timed out)\b/.test(normalized) || /超时/.test(message)) {
    return "timeout";
  }
  return "failed";
}

export function useAppState() {
  const context = useContext(AppStateContext);
  if (!context) {
    throw new Error("useAppState must be used within AppProvider");
  }
  return context;
}
