import { createContext, useContext, useEffect, useLayoutEffect, useMemo, useRef, useState, type ReactNode } from "react";
import {
  applyCodexReport as applyDesktopCodexReport,
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
  listCodexTasks as listDesktopCodexTasks,
  loadProject,
  markCodexTaskHandedOff as markDesktopCodexTaskHandedOff,
  acceptCodexTask as acceptDesktopCodexTask,
  rejectCodexTask as rejectDesktopCodexTask,
  openManagedFile,
  openManagedFolder,
  confirmInboxEntry as confirmDesktopInboxEntry,
  confirmMaterialInboxItem as confirmDesktopMaterialInboxItem,
  consumeLaunchInboxEntries as consumeDesktopLaunchInboxEntries,
  scanProjectWorkspace as scanDesktopProjectWorkspace,
  refreshProjectUnderstanding as refreshDesktopProjectUnderstanding,
  saveDeepSeekApiKey as saveDesktopDeepSeekApiKey,
  saveProjectDraft as saveDesktopProjectDraft,
  saveWeeklyReviewSettings as saveDesktopWeeklyReviewSettings,
  scanCodexResultBridge as scanDesktopCodexResultBridge,
  saveMemos,
  sendProjectMessage as sendDesktopProjectMessage,
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
  regenerateDailyContinue as regenerateDesktopDailyContinue,
  updateProjectAttention as updateDesktopProjectAttention,
  updateDataHealthStatus as updateDesktopDataHealthStatus,
  updatePendingReviewStatus as updateDesktopPendingReviewStatus,
  markTodayDone as markDesktopTodayDone,
  recordUserDecisionEvent as recordDesktopUserDecisionEvent,
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
  deepSeekModels: DeepSeekModelInfo[];
  isStreaming: boolean;
  isDesktopReady: boolean;
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
  sendProjectMessage: (text: string) => Promise<void>;
  stopProjectMessage: () => Promise<void>;
  saveProjectDraft: (text: string, pendingFilePaths?: string[]) => Promise<void>;
  finishProjectWork: (done: string, nextStep: string) => Promise<void>;
  generateCodexPrompt: () => Promise<CodexPromptResult | null>;
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
  refreshProjectImpact: () => Promise<void>;
  refreshDailyContinue: () => Promise<void>;
  regenerateDailyContinue: () => Promise<void>;
  refreshTodayWorkspace: () => Promise<void>;
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
const aiContextItems = ["DeepSeek 对话只发送项目说明、摘要和关键历史，不上传原始文件。", "文件导入、复制、登记和打开由 Tauri 命令负责。"];

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
  const [theme, setTheme] = useState<ThemeMode>(uiState.theme === "light" ? "light" : "dark");
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
  const [projectStateAutoApply, setProjectStateAutoApply] = useState(false);
  const [deepSeekSettings, setDeepSeekSettings] = useState<DeepSeekSettings>({
    hasApiKey: false,
    selectedModelId: "",
    lastTestedAt: "",
  });
  const [deepSeekModels, setDeepSeekModels] = useState<DeepSeekModelInfo[]>([]);
  const [isStreaming, setIsStreaming] = useState(false);
  const [lastInboxReceptionAt, setLastInboxReceptionAt] = useState("");
  const memosRef = useRef<MemoItem[]>([]);
  const memoSaveQueueRef = useRef<Promise<unknown>>(Promise.resolve());
  const [error, setError] = useState("");
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

  useEffect(() => {
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
    void refreshProjects();
    void getDesktopTodayWorkspace().then(setTodayWorkspace).catch((err) => setError(String(err)));
    // The first frame only needs project continuity. Defer settings and secondary
    // panels so their I/O cannot compete with restoring the active project.
    const secondaryLoad = window.setTimeout(() => {
      void listMemos().then(setMemos).catch((err) => setError(String(err)));
      void listInboxEntries().then(setMaterialInbox).catch((err) => setError(String(err)));
      void loadDesktopInboxRoutingSettings().then(setInboxRoutingSettings).catch((err) => setError(String(err)));
      void loadDesktopDeepSeekSettings().then(setDeepSeekSettings).catch((err) => setError(String(err)));
      void listDesktopDeepSeekModels().then(setDeepSeekModels).catch(() => undefined);
      void loadDesktopWeeklyReviewSettings().then(setWeeklyReviewSettings).catch((err) => setError(String(err)));
    }, 250);
    return () => window.clearTimeout(secondaryLoad);
  }, [isDesktopReady]);

  useEffect(() => {
    if (!isDesktopReady) return;
    let disposed = false;
    let busy = false;
    const poll = async () => {
      if (disposed || busy) return;
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
    if (!isDesktopReady) return;
    let disposed = false;
    const loadWeeklyReview = async () => {
      try {
        const dashboard = await loadDesktopWeeklyReviewDashboard(activeProject?.rootDir ?? readUiState().lastProjectRoot ?? null);
        if (disposed) return;
        setWeeklyReviewDashboard(dashboard);
        if (
          dashboard.generatedNow === false &&
          dashboard.currentWeekKey &&
          dashboard.globalReports.every((report) => report.weekKey !== dashboard.currentWeekKey) &&
          dashboard.projectReports.every((report) => report.weekKey !== dashboard.currentWeekKey)
        ) {
          const generated = await generateDesktopWeeklyReviews(
            dashboard.activeProjectRoot || null,
            dashboard.currentWeekKey,
            dashboard.weekStart,
            dashboard.weekEnd,
            false,
          );
          if (!disposed) {
            setWeeklyReviewDashboard(generated);
            setWeeklyReviewSettings(generated.settings);
          }
        }
      } catch (err) {
        if (!disposed) setError(String(err));
      }
    };
    void loadWeeklyReview();
    return () => {
      disposed = true;
    };
  }, [isDesktopReady, activeProject?.rootDir]);

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
        const manifest = await scanDesktopProjectWorkspace(activeProject.rootDir);
        if (!disposed && isCurrentProjectView(view)) {
          setActiveManifest(manifest);
          setActiveProject(manifest.project);
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
    const resumeVisibleScan = () => {
      if (document.visibilityState === "visible") void runScan();
    };
    document.addEventListener("visibilitychange", resumeVisibleScan);
    window.addEventListener("focus", resumeVisibleScan);
    return () => {
      disposed = true;
      if (timer) window.clearInterval(timer);
      document.removeEventListener("visibilitychange", resumeVisibleScan);
      window.removeEventListener("focus", resumeVisibleScan);
    };
  }, [isDesktopReady, activeProject?.rootDir]);

  useEffect(() => {
    if (!isDesktopReady || !activeProject) return;
    let disposed = false;
    let busy = false;
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
    const timer = window.setInterval(() => {
      void runBridgeScan();
    }, CODEX_RESULT_POLL_INTERVAL_MS);
    const resumeVisibleScan = () => {
      if (document.visibilityState === "visible") void runBridgeScan();
    };
    document.addEventListener("visibilitychange", resumeVisibleScan);
    window.addEventListener("focus", resumeVisibleScan);
    return () => {
      disposed = true;
      window.clearInterval(timer);
      document.removeEventListener("visibilitychange", resumeVisibleScan);
      window.removeEventListener("focus", resumeVisibleScan);
    };
  }, [isDesktopReady, activeProject?.rootDir]);

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
                source: "deepseek",
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
    // Invalidate outstanding reads immediately, including A -> B -> A switches.
    const view = { root: projectRoot, generation: projectViewRef.current.generation + 1 };
    projectViewRef.current = view;
    try {
      let manifest = await loadProject(projectRoot);
      if (!isCurrentProjectView(view)) return;
      try {
        manifest = await scanDesktopProjectWorkspace(projectRoot);
      } catch {
        // Keep the project open even if monitoring scan fails.
      }
      if (!isCurrentProjectView(view)) return;
      let ledger: WorkLedgerSnapshot | null = null;
      let nextTasks: CodexTask[] = [];
      let ledgerError = "";
      try {
        ledger = await getDesktopWorkLedger(projectRoot);
        nextTasks = ledger.codexTasks ?? (await listDesktopCodexTasks(projectRoot)).tasks;
      } catch (err) {
        ledgerError = String(err);
      }
      if (!isCurrentProjectView(view)) return;
      ++ledgerRequestRef.current;
      setActiveProject(manifest.project);
      setActiveManifest(manifest);
      setWorkLedger(ledger);
      setCodexTasks(nextTasks);
      setError(ledgerError);
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
      manifest = await scanDesktopProjectWorkspace(result.project.rootDir);
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
      manifest = await scanDesktopProjectWorkspace(activeProject.rootDir);
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

  async function sendProjectMessage(text: string) {
    const trimmed = text.trim();
    if (!trimmed || !activeProject) return;
    const result = await sendDesktopProjectMessage(activeProject.rootDir, trimmed);
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
  }

  async function stopProjectMessage() {
    if (!activeProject) return;
    await stopDesktopProjectMessage(activeProject.rootDir);
  }

  async function saveProjectDraft(text: string, pendingFilePaths: string[] = []) {
    if (!activeProject) return;
    const draft = await saveDesktopProjectDraft(activeProject.rootDir, text, pendingFilePaths);
    setActiveManifest((previous) => (previous ? { ...previous, draft } : previous));
  }

  async function finishProjectWork(done: string, nextStep: string) {
    if (!activeProject) return;
    const result = await finishDesktopProjectWork(activeProject.rootDir, done, nextStep);
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
    setError("");
  }

  async function deleteDeepSeekApiKey() {
    const settings = await deleteDesktopDeepSeekApiKey();
    setDeepSeekSettings(settings);
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
    if (!activeProject) throw new Error("请先打开一个项目。");
    let manifest = await grantDesktopDeepSeekAuthorization(activeProject.rootDir);
    let refreshError = "";
    try {
      manifest = await refreshDesktopProjectUnderstanding(activeProject.rootDir);
    } catch (err) {
      refreshError = String(err);
    }
    setActiveManifest(manifest);
    setActiveProject(manifest.project);
    setError(refreshError);
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

  async function refreshProjectImpact() {
    if (!activeProject) return;
    const root = activeProject.rootDir;
    try {
      const [impacts, candidates, proposals] = await Promise.all([
        getDesktopImpactAnalyses(root),
        getDesktopActionCandidates(root),
        getDesktopStateProposals(root),
      ]);
      setImpactAnalyses(impacts);
      setActionCandidates(candidates);
      setStateProposals(proposals);
      setActiveManifest(await loadProject(root));
    } catch (err) {
      setError(String(err));
    }
  }

  async function refreshDailyContinue() {
    if (!activeProject) return;
    try {
      const snapshot = await getDesktopDailyContinue(activeProject.rootDir);
      setDailyContinue(snapshot);
    } catch (err) {
      setError(String(err));
    }
  }

  async function regenerateDailyContinueAction() {
    if (!activeProject) return;
    try {
      const snapshot = await regenerateDesktopDailyContinue(activeProject.rootDir);
      setDailyContinue(snapshot);
      setError("");
    } catch (err) {
      setError(String(err));
    }
  }

  async function refreshTodayWorkspace() {
    if (!isDesktopReady) return;
    try {
      setTodayWorkspace(await getDesktopTodayWorkspace());
      setError("");
    } catch (err) {
      setError(String(err));
    }
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
    await refreshScopedWorkLedger(view);
    if (!isCurrentProjectView(view)) return event;
    await refreshTodayWorkspace();
    if (isCurrentProjectView(view)) setError("");
    return event;
  }

  async function updateProjectAttentionAction(attentionId: string, status: "confirmed" | "ignored" | "later") {
    if (!activeProject) return;
    try {
      await updateDesktopProjectAttention(activeProject.rootDir, attentionId, status);
      const manifest = await loadProject(activeProject.rootDir);
      setActiveManifest(manifest);
      await refreshTodayWorkspace();
      setError("");
    } catch (err) {
      setError(String(err));
    }
  }

  async function updateDataHealthStatusAction(recordId: string, status: "resolved" | "ignored") {
    if (!activeProject) return;
    try {
      const manifest = await updateDesktopDataHealthStatus(activeProject.rootDir, recordId, status);
      setActiveManifest(manifest);
      setDailyContinue(manifest.dailyContinueSnapshots[manifest.dailyContinueSnapshots.length - 1] ?? null);
      await refreshTodayWorkspace();
      setError("");
    } catch (err) {
      setError(String(err));
    }
  }

  async function updatePendingReviewStatusAction(reviewId: string, status: "open" | "pending" | "resolved" | "ignored") {
    if (!activeProject) return;
    try {
      const manifest = await updateDesktopPendingReviewStatus(activeProject.rootDir, reviewId, status);
      setActiveManifest(manifest);
      setDailyContinue(manifest.dailyContinueSnapshots[manifest.dailyContinueSnapshots.length - 1] ?? null);
      await refreshTodayWorkspace();
      setError("");
    } catch (err) {
      setError(String(err));
    }
  }

  async function confirmActionCandidateAction(candidateId: string) {
    if (!activeProject) return;
    try {
      await confirmDesktopActionCandidate(activeProject.rootDir, candidateId);
      await refreshProjectImpact();
      setError("");
    } catch (err) {
      setError(String(err));
    }
  }

  async function ignoreActionCandidateAction(candidateId: string) {
    if (!activeProject) return;
    try {
      await ignoreDesktopActionCandidate(activeProject.rootDir, candidateId);
      await refreshProjectImpact();
      setError("");
    } catch (err) {
      setError(String(err));
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
    try {
      await updateDesktopActionCandidate(activeProject.rootDir, candidateId, title, description, suggestedPriority, suggestedDueDate);
      await refreshProjectImpact();
      setError("");
    } catch (err) {
      setError(String(err));
    }
  }

  async function applyStateProposalAction(proposalId: string) {
    if (!activeProject) return;
    try {
      await applyDesktopStateProposal(activeProject.rootDir, proposalId);
      await refreshProjectImpact();
      setError("");
    } catch (err) {
      setError(String(err));
    }
  }

  async function undoStateProposalAction(proposalId: string) {
    if (!activeProject) return;
    try {
      await undoDesktopStateProposal(activeProject.rootDir, proposalId);
      await refreshProjectImpact();
      setError("");
    } catch (err) {
      setError(String(err));
    }
  }

  async function markTodayDoneAction(completed: string, nextStep: string) {
    if (!activeProject) return;
    try {
      const result = await markDesktopTodayDone(activeProject.rootDir, completed, nextStep);
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
      await refreshDailyContinue();
      setProjects((previous) =>
        [result.project, ...previous.filter((project) => project.id !== result.project.id)].sort((left, right) =>
          right.lastOpenedAt.localeCompare(left.lastOpenedAt),
        ),
      );
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
      deepSeekModels,
      isStreaming,
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
      stopProjectMessage,
      saveProjectDraft,
      finishProjectWork,
      generateCodexPrompt,
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
      refreshProjectImpact,
      refreshDailyContinue,
      regenerateDailyContinue: regenerateDailyContinueAction,
      refreshTodayWorkspace,
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
      deepSeekModels,
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

export function useAppState() {
  const context = useContext(AppStateContext);
  if (!context) {
    throw new Error("useAppState must be used within AppProvider");
  }
  return context;
}
