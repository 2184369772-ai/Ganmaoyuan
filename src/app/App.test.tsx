import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { MemoryRouter, useLocation } from "react-router-dom";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import App from "./App";
import { AppProvider, useAppState } from "./AppState";
import { WorkPage } from "../pages/WorkPage";
import type {
  CodexExternalResult,
  DeepSeekStreamEvent,
  GitSnapshot,
  ManagedFile,
  MaterialInboxItem,
  ProjectContextPacket,
  ProjectManifest,
  ProjectSummary,
  WorkEvent,
  WorkspaceMessage,
} from "../features/project/desktopApi";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => vi.fn()) }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

const project: ProjectSummary = {
  id: "project-1",
  name: "感冒院验收项目",
  rootDir: "D:\\GanMaoYuan\\Acceptance\\ui",
  manifestPath: "D:\\GanMaoYuan\\Acceptance\\ui\\.ganmaoyuan\\project-location-manifest.json",
  lastOpenedAt: "1",
  nextStep: "继续验证消息恢复。",
  createdAt: "1",
  description: "前端交互测试",
};

const initialMessage: WorkspaceMessage = {
  id: "message-1",
  author: "ganmaoyuan",
  kind: "restore",
  text: "上次工作已经恢复。",
  createdAt: "1",
  status: "completed",
  attachments: [],
  relatedTask: "",
  source: "local_rule",
  modelId: "",
  evidenceItems: [],
};

function RoutePathProbe() {
  const location = useLocation();
  return <output data-testid="current-route">{`${location.pathname}${location.search}`}</output>;
}

function DeepSeekSettingsStateProbe() {
  const { deepSeekSettingsLoaded, error } = useAppState();
  return <output data-testid="deepseek-settings-state">{`${deepSeekSettingsLoaded ? "loaded" : "not-loaded"}:${error}`}</output>;
}

function makeManagedFile(id: string, fileName: string): ManagedFile {
  return {
    id,
    fileName,
    originalSourcePath: `D:\\GanMaoYuan\\Acceptance\\${fileName}`,
    managedPath: `D:\\GanMaoYuan\\Acceptance\\managed\\${fileName}`,
    fileType: "md",
    category: "reference",
    createdSource: "userImport",
    relatedTask: "",
    currentVersion: "1",
    historyVersions: [],
    createdAt: "1",
    modifiedAt: "1",
    stillExists: true,
    needsUserConfirmation: false,
    parseStatus: "success",
    contentSummary: "用于验证资料详情弹窗行为。",
    mainFieldsOrSections: [],
    recommendedCategory: "参考资料",
    parseFailureReason: "",
    parser: "markdown",
    extractedTextPath: "",
    pageCount: null,
    sheetCount: null,
    rowCount: null,
    columnCount: null,
    analysisWarnings: [],
    contentHash: `${id}-hash`,
    sizeBytes: 20,
    versionGroupId: id,
    duplicateOfFileId: null,
    previousVersionId: null,
    sourcePaths: [],
    managedRelativePath: `managed/${fileName}`,
    locationReason: "测试夹具",
    importTransactionId: "",
    lastVerifiedAt: "1",
  };
}

function makeReadyCodexTask(taskId = "task-preflight") {
  return {
    taskId,
    projectId: project.id,
    title: "预检测试任务",
    taskType: "coding",
    prompt: `taskId: ${taskId}\n执行隔离预检测试`,
    status: "ready",
    createdAt: "1",
    expectedResultPath: "D:\\Acceptance\\codex-preflight-result.json",
    resultId: "",
    resultRunId: "",
    resultReceivedAt: "",
    completedAt: "",
    summary: "",
    resultText: "",
    changedFiles: [],
    reportedCommits: [],
    verifiedCommits: [],
    gitVerification: { status: "notApplicable", repositoryPath: "", checkedAt: "", reason: "" },
    tests: [],
    findings: [],
    recommendations: [],
    questions: [],
    checks: [],
    passed: [],
    failed: [],
    artifacts: [],
    targetFiles: [],
    remainingIssues: [],
    manualAcceptance: [],
  };
}

let workLedgerEvents: WorkEvent[] = [];
let gitSnapshot: GitSnapshot | null = null;
let codexResults: CodexExternalResult[] = [];
let codexConsoleTasks: unknown[] = [];
let codexConsoleRuns: unknown[] = [];
let captureProjectFactFailure = false;
let registeredExternalFiles: ManagedFile[] = [];
let inboxEntries: MaterialInboxItem[] = [];

const projectContextPacket: ProjectContextPacket = {
  projectId: project.id,
  projectName: project.name,
  projectDescription: "用于验证感冒院上下文交接。",
  currentPhase: "人工验收",
  generatedAt: "2026-09-16T10:00:00Z",
  privacyNotice: "仅包含事实摘要与相对资料位置。",
  focus: {
    title: "验收 Codex 结果",
    summary: "当前结果等待人工验收。",
    reason: "来自待处理事项。",
    evidence: ["Codex 任务"],
  },
  recentActivity: [{ occurredAt: "2026-09-16", summary: "Git HEAD 已更新。", evidence: ["commit:1234567"] }],
  pendingActions: [{ title: "验收 Codex 结果", reason: "存在 1 项人工验收。", priority: "high" }],
  files: [{ name: "验收说明.md", documentPurpose: "reference", lifecycleStatus: "managed", location: "10_Projects/验收说明.md", summary: "验收资料" }],
  codexResult: null,
  decisions: [],
  rationaleContext: { status: "missing", warnings: [], entries: [] },
  gitFacts: null,
  risks: [],
  readWarnings: [],
  nextStep: "验收 Codex 结果",
  freshness: { generatedAt: "2026-09-16T10:00:00Z", factsSyncedAt: "2026-09-16T10:00:00Z", status: "current" },
  sparse: false,
  markdown: "# 项目上下文：感冒院验收项目",
};

const manifest: ProjectManifest = {
  schemaVersion: 2,
  project,
  files: [],
  messages: [initialMessage],
  tasks: [],
  decisions: [],
  artifacts: [],
  codexPrompts: [],
  codexReports: [],
  dailySessions: [],
  locationDecisions: [],
  pendingReviews: [],
  monitoring: {
    status: "ready",
    lastScannedAt: "1",
    lastEventAt: "1",
    lastError: "",
    watchStartedAt: "1",
    pendingCount: 0,
  },
  atlas: {
    atlasVersion: "",
    status: "waiting_for_integration",
    reusableParts: [],
    uncoveredParts: [],
    trainingCandidates: [],
    evidenceReferences: [],
    reviewRequired: true,
    assessedAt: "1",
    failureReason: "",
  },
  projectAnalysis: {
    status: "success",
    projectDefinition: "感冒院验收项目用于验证工作台恢复。",
    goals: ["验证工作台消息恢复"],
    roles: ["项目使用者"],
    materialUsage: [],
    knownRequirements: [],
    gaps: [],
    questions: [],
    constraints: [],
    evidence: [],
    nextSteps: ["继续验证消息恢复。"],
    updatedAt: "1",
    modelId: "deepseek-chat",
    failureReason: "",
  },
  audit: [],
  draft: {
    projectId: project.id,
    text: "尚未发送的草稿",
    pendingFilePaths: [],
    updatedAt: "1",
  },
  recoveryPoints: [],
  deepseekAuthorization: {
    grantedAt: "",
    grantedBy: "",
  },
  projectImpactAnalyses: [],
  projectActionCandidates: [],
  projectStateProposals: [],
  dailyContinueSnapshots: [],
  projectStateSummary: {
    projectId: project.id,
    currentPhase: "",
    recentProgress: [],
    currentRisks: [],
    blockers: [],
    nextMilestone: "",
    recentChanges: [],
    facts: [],
    inferences: [],
    suggestions: [],
    evidence: [],
    generatedAt: "",
    decisionTrace: {
      id: "",
      type: "",
      subjectId: "",
      projectId: project.id,
      inputEvidence: [],
      aiUnderstanding: "",
      recommendation: "",
      confidence: { score: 0, level: "low", display: "低 0%" },
      userDecision: "pending",
      userDecisionNote: "",
      execution: "pending",
      executionNote: "",
      createdAt: "",
      updatedAt: "",
    },
  },
  projectAttentions: [],
  workPatternCandidates: [],
  projectStateAutoApply: false,
};

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((done, fail) => { resolve = done; reject = fail; });
  return { promise, resolve, reject };
}

function ProjectRefreshTestControl() {
  const { refreshProjectUnderstanding } = useAppState();
  return <button type="button" onClick={() => void refreshProjectUnderstanding()}>触发项目理解刷新</button>;
}

describe("project workspace persistence flow", () => {
  afterEach(() => {
    cleanup();
  });

  beforeEach(() => {
    vi.mocked(listen).mockImplementation(async () => () => undefined);
    vi.mocked(open).mockReset();
    workLedgerEvents = [];
    gitSnapshot = null;
    codexResults = [];
    codexConsoleTasks = [];
    codexConsoleRuns = [];
    captureProjectFactFailure = false;
    registeredExternalFiles = [];
    inboxEntries = [];
    Object.defineProperty(window, "__TAURI_INTERNALS__", {
      configurable: true,
      value: {},
    });
    window.localStorage.setItem(
      "ganmaoyuan-ui-state-v0.4",
      JSON.stringify({ lastProjectRoot: project.rootDir }),
    );
    window.localStorage.removeItem("ganmaoyuan-first-use-guide-dismissed-v1");
    vi.mocked(invoke).mockImplementation(async (command, args) => {
      if (command === "list_projects") return [project];
      if (command === "list_memos") return [];
      if (command === "list_inbox_entries") return inboxEntries;
      if (command === "load_inbox_routing_settings") {
        return { autoRouteHighConfidence: false, updatedAt: "" };
      }
      if (command === "load_deepseek_settings") {
        return { hasApiKey: false, selectedModelId: "", lastTestedAt: "" };
      }
      if (command === "list_deepseek_models") return [];
      if (command === "load_weekly_review_settings") {
        return { generationWeekday: 1, lastAutoGeneratedWeekKey: "", updatedAt: "" };
      }
      if (command === "load_weekly_review_dashboard") {
        return {
          settings: { generationWeekday: 1, lastAutoGeneratedWeekKey: "", updatedAt: "" },
          currentWeekKey: "",
          weekStart: "",
          weekEnd: "",
          generatedAt: "",
          generatedNow: false,
          globalReports: [],
          projectReports: [],
          activeProjectRoot: project.rootDir,
        };
      }
      if (command === "consume_launch_inbox_entries") return [];
      if (command === "scan_codex_result_bridge") {
        return {
          resultDir: `${project.rootDir}\\.ganmaoyuan\\codex\\results`,
          scannedCount: 0,
          importedCount: 0,
          reports: [],
          manifest,
        };
      }
      if (command === "get_work_ledger") {
        return { events: workLedgerEvents, gitSnapshot, codexResults, codexTasks: codexConsoleTasks };
      }
      if (command === "list_codex_tasks") return { tasks: codexConsoleTasks, runs: codexConsoleRuns };
      if (command === "check_codex_cli_readiness") {
        return { status: "ready", message: "本机 Codex CLI 已安装、能力兼容且已登录。", version: "codex-cli test", checkedAt: "3" };
      }
      if (command === "get_current_project_context_packet") return projectContextPacket;
      if (command === "record_user_decision_event") {
        const decision = (args as { decision: string; reason?: string | null }).decision;
        const existing = workLedgerEvents.find(
          (event) => event.sourceRef === `decision-${decision}` && event.eventType === "user.decisionRecorded",
        );
        if (existing) return existing;
        const event: WorkEvent = {
          id: `event-${decision}`,
          projectId: project.id,
          sourceType: "user",
          sourceRef: `decision-${decision}`,
          eventType: "user.decisionRecorded",
          factKind: "fact",
          occurredAt: "3",
          summary: `用户确认决定：${decision}`,
          evidenceRefs: ["decision:test"],
          confidence: { score: 100, level: "high", display: "高 100%" },
          decisionTraceId: "trace-1",
          decisionTrace: {
            id: "trace-1",
            type: "decisionCandidate",
            subjectId: "decision-1",
            projectId: project.id,
            inputEvidence: [],
            aiUnderstanding: "用户确认的项目决定。",
            recommendation: decision,
            confidence: { score: 100, level: "high", display: "高 100%" },
            userDecision: "pending",
            userDecisionNote: "",
            execution: "pending",
            executionNote: "",
            createdAt: "3",
            updatedAt: "3",
          },
          createdAt: "3",
        };
        workLedgerEvents = [...workLedgerEvents, event];
        return event;
      }
      if (command === "capture_project_fact") {
        const request = (args as {
          request: { captureType: string; content: string; reason?: string; actionSourceRef?: string; externalAiSourceFileId?: string };
        }).request;
        if (captureProjectFactFailure) {
          throw new Error("工作事实暂时无法写入");
        }
        const event: WorkEvent = {
          id: `capture-${request.captureType}`,
          projectId: project.id,
          sourceType: "user",
          sourceRef: `capture-${request.captureType}-${request.content}`,
          eventType: `user.${request.captureType}Recorded`,
          factKind: "userConfirmed",
          occurredAt: "4",
          summary: `用户记录进展：${request.content}`,
          evidenceRefs: [
            "userCapture:test",
            ...(request.externalAiSourceFileId ? [`externalAiFile:${request.externalAiSourceFileId}`] : []),
            ...(request.captureType === "decision" ? ["decision:decision-external-test"] : []),
          ],
          confidence: { score: 100, level: "high", display: "高 100%" },
          decisionTraceId: "",
          decisionTrace: {
            id: "",
            type: "",
            subjectId: "",
            projectId: project.id,
            inputEvidence: [],
            aiUnderstanding: "",
            recommendation: "",
            confidence: { score: 100, level: "high", display: "高 100%" },
            userDecision: "pending",
            userDecisionNote: "",
            execution: "pending",
            executionNote: "",
            createdAt: "4",
            updatedAt: "4",
          },
          createdAt: "4",
        };
        workLedgerEvents = [...workLedgerEvents, event];
        return event;
      }
      if (command === "register_generated_file") {
        const request = args as {
          fileName: string;
          content: string;
          createdSource: string;
          relatedTask: string;
          purpose: string;
        };
        const digest = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(request.content));
        const contentHash = Array.from(new Uint8Array(digest), (byte) => byte.toString(16).padStart(2, "0")).join("");
        const file = {
          ...makeManagedFile(`external-source-${registeredExternalFiles.length + 1}`, request.fileName),
          createdSource: request.createdSource,
          contentHash,
          originalSourcePath: "",
          managedPath: `${project.rootDir}\\.ganmaoyuan\\managed\\${request.fileName}`,
        };
        registeredExternalFiles = [...registeredExternalFiles, file];
        manifest.files = [...manifest.files, file];
        return { file, message: { ...initialMessage, id: `external-message-${file.id}`, text: request.content } };
      }
      if (command === "get_today_workspace") {
        return {
          generatedAt: "2",
          continueWorkFocus: {
            projectId: project.id,
            title: "检查 Codex 任务结果",
            summary: "结果证据需要人工检查。",
            reason: "该 Codex 任务存在需检查状态，会阻塞继续信任后续结果。",
            priority: "P0",
            confidence: { score: 94, level: "high", display: "高 94%" },
            freshnessStatus: "fresh",
            evidenceRefs: [{ kind: "codexTask", id: "task-1", pathSnapshot: "", hashSnapshot: "", label: "Codex 任务" }],
            sourceActionIds: ["codex-review:task-1"],
            primaryAction: {
              type: "reviewCodexTask",
              label: "查看问题",
              projectId: project.id,
              projectRoot: project.rootDir,
              sourceId: "task-1",
              panel: "codex",
            },
            generatedAt: "2",
          },
          lastProgress: ["已完成资料整理", "继续回归测试"],
          focusProjects: [
            {
              projectId: project.id,
              projectName: project.name,
              projectRoot: project.rootDir,
              currentStatus: "任务推进",
              recentChange: "已完成资料整理",
              nextStep: "继续回归测试",
              updatedAt: "2",
            },
          ],
          recommendedActions: [],
          blockers: [],
          pendingItems: [],
          status: "ready",
        };
      }
      if (command === "load_project") return manifest;
      if (command === "scan_project_workspace") return manifest;
      if (command === "save_project_draft") {
        return {
          ...manifest.draft,
          text: (args as { text: string }).text,
          updatedAt: "2",
        };
      }
      if (command === "send_project_message") {
        return {
          project: { ...project, lastOpenedAt: "2" },
          messages: [
            {
              ...initialMessage,
              id: "user-2",
              author: "user",
              kind: "requirement",
              text: (args as { text: string }).text,
              createdAt: "2",
              status: "completed",
              source: "user",
              modelId: "",
              attachments: [],
              relatedTask: "",
            },
            {
              ...initialMessage,
              id: "reply-2",
              kind: "assistant",
              text: "",
              createdAt: "2",
              status: "streaming",
              source: "deepseek",
              modelId: "deepseek-chat",
            },
          ],
          streamMessageId: "reply-2",
        };
      }
      throw new Error(`unexpected command: ${command}`);
    });
  });

  it("restores a draft and persists a sent message through Tauri", async () => {
    render(
      <MemoryRouter initialEntries={["/work"]}>
        <App />
      </MemoryRouter>,
    );

    await waitFor(() => expect(invoke).toHaveBeenCalledWith("list_projects"));
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("load_project", {
        projectRoot: project.rootDir,
      }),
    );
    expect((await screen.findAllByText("感冒院验收项目")).length).toBeGreaterThan(0);
    const composer = await screen.findByPlaceholderText("告诉感冒院你现在要做什么……");
    await waitFor(() => expect(composer).toHaveValue("尚未发送的草稿"));

    fireEvent.change(composer, { target: { value: "今天检查事务导入" } });
    fireEvent.click(screen.getByRole("button", { name: "发送" }));

    expect(await screen.findByText("今天检查事务导入")).toBeInTheDocument();
    expect(await screen.findByRole("status", { name: "感冒院正在回复" })).toBeInTheDocument();
    expect((await screen.findAllByText("项目记录")).length).toBeGreaterThan(0);
    expect(invoke).toHaveBeenCalledWith("send_project_message", {
      projectRoot: project.rootDir,
      text: "今天检查事务导入",
    });
  });

  it("shows the persisted decision receipt beside the assistant reply", async () => {
    const originalInvoke = vi.mocked(invoke).getMockImplementation()!;
    vi.mocked(invoke).mockImplementation(async (command, args) => {
      const result = await originalInvoke(command, args);
      if (command !== "send_project_message") return result;
      const messageResult = result as { messages: WorkspaceMessage[] };
      return {
        ...messageResult,
        messages: messageResult.messages.map((message) =>
          message.id === "reply-2"
            ? {
                ...message,
                evidenceItems: [
                  {
                    id: "receipt-1",
                    title: "决定已记录",
                    contentType: "operationReceipt",
                    basisKind: "fact",
                    summary: "DecisionRecord ID：decision-1；Work Ledger 事件 ID：event-1。",
                    managedPath: "",
                    sourceRecordId: "decision-1",
                  },
                ],
              }
            : message,
        ),
      };
    });

    render(
      <MemoryRouter initialEntries={["/work"]}>
        <App />
      </MemoryRouter>,
    );

    const composer = await screen.findByPlaceholderText("告诉感冒院你现在要做什么……");
    fireEvent.change(composer, { target: { value: "我决定采用当前方案，请记录下来。" } });
    fireEvent.click(screen.getByRole("button", { name: "发送" }));

    const receipt = await screen.findByRole("status", { name: "项目记录保存回执" });
    expect(receipt).toHaveTextContent("决定已记录");
    expect(receipt).toHaveTextContent("DecisionRecord ID：decision-1");
    expect(receipt).toHaveTextContent("Work Ledger 事件 ID：event-1");
  });

  it("shows reply dots until the first streamed answer text arrives", async () => {
    let streamListener: ((event: { payload: DeepSeekStreamEvent }) => void) | null = null;
    const pendingResult = deferred<unknown>();
    const originalInvoke = vi.mocked(invoke).getMockImplementation()!;
    const captureStreamListener = async (event: string, handler: (event: unknown) => void) => {
      if (event === "deepseek-stream") {
        streamListener = handler as unknown as (event: { payload: DeepSeekStreamEvent }) => void;
      }
      return vi.fn();
    };
    vi.mocked(listen).mockImplementation(captureStreamListener as typeof listen);
    vi.mocked(invoke).mockImplementation((command, args) =>
      command === "send_project_message" ? pendingResult.promise : originalInvoke(command, args),
    );

    render(
      <MemoryRouter initialEntries={["/work"]}>
        <App />
      </MemoryRouter>,
    );

    const composer = await screen.findByPlaceholderText("告诉感冒院你现在要做什么……");
    fireEvent.change(composer, { target: { value: "测试流式回复状态" } });
    fireEvent.click(screen.getByRole("button", { name: "发送" }));

    expect(await screen.findByRole("status", { name: "感冒院正在回复" })).toBeInTheDocument();
    const sentResult = await originalInvoke("send_project_message", {
      projectRoot: project.rootDir,
      text: "测试流式回复状态",
    });
    await act(async () => {
      pendingResult.resolve(sentResult);
      await pendingResult.promise;
    });
    expect(await screen.findByText("测试流式回复状态")).toBeInTheDocument();
    await waitFor(() => expect(streamListener).not.toBeNull());

    act(() => streamListener?.({ payload: {
      projectRoot: project.rootDir,
      messageId: "reply-2",
      status: "streaming",
      delta: "",
      text: "",
      error: "",
      modelId: "deepseek-chat",
    } }));
    expect(screen.getByRole("status", { name: "感冒院正在回复" })).toBeInTheDocument();

    act(() => streamListener?.({ payload: {
      projectRoot: project.rootDir,
      messageId: "reply-2",
      status: "streaming",
      delta: "开始",
      text: "开始回答",
      error: "",
      modelId: "deepseek-chat",
    } }));
    expect(screen.queryByRole("status", { name: "感冒院正在回复" })).not.toBeInTheDocument();
  });

  it("clears reply dots when a send fails or the user stops generation", async () => {
    const originalInvoke = vi.mocked(invoke).getMockImplementation()!;
    const pendingResult = deferred<unknown>();
    vi.mocked(invoke).mockImplementation((command, args) => {
      if (command === "send_project_message") return pendingResult.promise;
      if (command === "stop_project_message") return Promise.resolve(undefined);
      return originalInvoke(command, args);
    });

    const { unmount } = render(
      <MemoryRouter initialEntries={["/work"]}>
        <App />
      </MemoryRouter>,
    );

    const composer = await screen.findByPlaceholderText("告诉感冒院你现在要做什么……");
    fireEvent.change(composer, { target: { value: "测试请求失败清理" } });
    fireEvent.click(screen.getByRole("button", { name: "发送" }));
    expect(await screen.findByRole("status", { name: "感冒院正在回复" })).toBeInTheDocument();
    await act(async () => {
      pendingResult.reject(new Error("网络不可用"));
      await pendingResult.promise.catch(() => undefined);
    });
    await waitFor(() => expect(screen.queryByRole("status", { name: "感冒院正在回复" })).not.toBeInTheDocument());

    unmount();
    vi.mocked(invoke).mockImplementation(async (command, args) => {
      if (command === "stop_project_message") return undefined;
      return originalInvoke(command, args);
    });
    render(
      <MemoryRouter initialEntries={["/work"]}>
        <App />
      </MemoryRouter>,
    );
    const secondComposer = await screen.findByPlaceholderText("告诉感冒院你现在要做什么……");
    fireEvent.change(secondComposer, { target: { value: "测试主动停止" } });
    fireEvent.click(screen.getByRole("button", { name: "发送" }));
    expect(await screen.findByRole("status", { name: "感冒院正在回复" })).toBeInTheDocument();
    const stopButton = await screen.findByRole("button", { name: "停止" });
    expect(secondComposer.closest(".workspace-composer")).toContainElement(stopButton);
    fireEvent.click(stopButton);
    await waitFor(() => expect(screen.queryByRole("status", { name: "感冒院正在回复" })).not.toBeInTheDocument());
  });

  it("loads workspace settings only when the project-management section is opened", async () => {
    const originalInvoke = vi.mocked(invoke).getMockImplementation()!;
    vi.mocked(invoke).mockImplementation(async (command, args) => {
      if (command === "get_workspace_config") {
        return {
          status: "ready",
          workspaceRoot: "D:\\GanMaoYuan_Workspace",
          inboxRoot: "00_Inbox",
          projectsRoot: "10_Projects",
          generalRoot: "20_General",
          temporaryRoot: "30_Temporary",
          archiveRoot: "40_Archive",
          systemRoot: ".ganmaoyuan",
          lastVerifiedAt: "",
        };
      }
      if (command === "list_workspace_scan_batches") return [];
      return originalInvoke(command, args);
    });

    render(
      <MemoryRouter initialEntries={["/settings"]}>
        <App />
      </MemoryRouter>,
    );

    await screen.findByRole("heading", { name: "感冒院管理中心" });
    expect(invoke).not.toHaveBeenCalledWith("get_workspace_config");
    expect(invoke).not.toHaveBeenCalledWith("list_workspace_scan_batches");

    fireEvent.click(screen.getByRole("button", { name: "项目管理" }));
    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("get_workspace_config");
      expect(invoke).toHaveBeenCalledWith("list_workspace_scan_batches");
    });
  });

  it("keeps all settings sections navigable in the shared content layout", async () => {
    const { container } = render(
      <MemoryRouter initialEntries={["/settings"]}>
        <App />
      </MemoryRouter>,
    );

    await screen.findByRole("heading", { name: "感冒院管理中心" });
    const menu = container.querySelector(".settings-sidebar");
    const content = container.querySelector(".settings-content");
    expect(menu).not.toBeNull();
    expect(content).not.toBeNull();

    const items = [...menu!.querySelectorAll<HTMLButtonElement>(".settings-menu-item")];
    expect(items.map((item) => item.textContent?.trim())).toEqual([
      "常规",
      "项目管理",
      "任务管理",
      "资料管理",
      "待整理资料",
      "文件索引",
      "项目记忆",
      "AI 上下文",
      "AI 模型",
      "数据与备份",
      "每周复盘",
      "隐私安全",
      "外观",
      "高级设置",
    ]);

    for (const item of items) {
      fireEvent.click(item);
      await waitFor(() => expect(item).toHaveAttribute("aria-current", "page"));
      expect(content).toContainElement(document.getElementById(item.getAttribute("aria-controls")!));
      expect(content?.querySelector("h2")?.textContent?.trim()).toBe(item.textContent?.trim());
      if (item.textContent?.trim() === "数据与备份") {
        expect(content).toHaveTextContent("每个已登记项目的 .ganmaoyuan 应用数据");
        expect(content).toHaveTextContent("不会遍历或复制项目根目录中的源码仓库、.git、node_modules");
        expect(content).toHaveTextContent("全局 general/temporary 受管资料和操作记录");
        expect(content).toHaveTextContent("备份未加密");
      }
      if (item.textContent?.trim() === "隐私安全") {
        expect(content).toHaveTextContent("本机备份保留项目内容且未加密");
        expect(content).not.toHaveTextContent("备份和诊断日志默认脱敏");
      }
    }
  });

  it("prefills the Codex form from the latest project discussion", async () => {
    const previousMessages = manifest.messages;
    manifest.messages = [
      ...previousMessages,
      {
        ...initialMessage,
        id: "user-ui-discussion",
        author: "user",
        kind: "requirement",
        text: "设置后台右侧内容区文字太贴边，希望增加 padding / gap / section spacing。不要修改左侧菜单、业务逻辑和整体视觉风格。",
        createdAt: "2",
        status: "completed",
        source: "user",
        attachments: [{
          fileId: "image-1",
          fileName: "settings.png",
          managedPath: "",
          attachmentType: "image",
          contentType: "image/png",
          relativePath: ".ganmaoyuan/chat-attachments/user-ui-discussion/image-1-settings.png",
        }],
      },
      {
        ...initialMessage,
        id: "assistant-ui-discussion",
        author: "ganmaoyuan",
        kind: "assistant",
        text: "增加外层内容区 padding，调整区块 vertical gap / section spacing，增加表格行上下 padding；不修改左侧菜单，不改变整体视觉风格，不改业务逻辑，修改后运行最新构建进行验证。",
        createdAt: "3",
        status: "completed",
        source: "deepseek",
        modelId: "deepseek-v4-flash",
      },
    ];
    try {
      const originalInvoke = vi.mocked(invoke).getMockImplementation()!;
      vi.mocked(invoke).mockImplementation(async (command, args) => {
        if (command === "create_codex_task") {
          return {
            taskId: "task-from-discussion",
            projectId: project.id,
            title: "优化设置后台右侧内容区留白",
            taskType: "coding",
            prompt: "taskId：task-from-discussion",
            status: "ready",
          };
        }
        return originalInvoke(command, args);
      });
      render(
        <MemoryRouter initialEntries={["/work"]}>
          <App />
        </MemoryRouter>,
      );

      fireEvent.click(await screen.findByRole("button", { name: "Codex 任务" }));

      expect(screen.queryByLabelText("任务标题")).not.toBeInTheDocument();
      fireEvent.click(screen.getByRole("button", { name: "生成新任务" }));
      expect(await screen.findByLabelText("任务标题")).toHaveValue("优化设置后台右侧内容区留白");
      expect(screen.getByLabelText("任务类型")).toHaveValue("coding");
      const instructions = (screen.getByLabelText("任务说明") as HTMLTextAreaElement).value;
      expect(instructions).toContain("增加外层内容区 padding");
      expect(instructions).toContain("settings.png");
      expect(invoke).not.toHaveBeenCalledWith("create_codex_task", expect.anything());

      fireEvent.click(screen.getByRole("button", { name: "创建任务" }));
      await waitFor(() => {
        expect(invoke).toHaveBeenCalledWith("create_codex_task", {
          projectRoot: project.rootDir,
          request: {
            title: "优化设置后台右侧内容区留白",
            taskType: "coding",
            instructions,
          },
        });
      });
    } finally {
      manifest.messages = previousMessages;
    }
  });

  it("keeps the homepage focused on work entry and real recent projects", async () => {
    render(
      <MemoryRouter initialEntries={["/"]}>
        <App />
      </MemoryRouter>,
    );

    expect(await screen.findByText("工作空间")).toBeInTheDocument();
    expect(await screen.findByText("最近进入的项目")).toBeInTheDocument();
    expect(await screen.findByText("继续回归测试")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /继续项目/ })).toBeInTheDocument();
    expect(screen.queryByText("今天建议")).not.toBeInTheDocument();
    expect(screen.queryByText("优先处理 1 到 3 件真实事项")).not.toBeInTheDocument();
  });

  it("keeps Start Work as the homepage primary action and lists recent projects", async () => {
    render(
      <MemoryRouter initialEntries={["/"]}>
        <App />
      </MemoryRouter>,
    );

    expect((await screen.findAllByText("最近项目")).length).toBeGreaterThan(0);
    expect((await screen.findAllByText("感冒院验收项目")).length).toBeGreaterThan(0);
    expect(screen.getByRole("button", { name: "开始工作" })).toBeInTheDocument();
    expect(screen.queryByText("第一次使用")).not.toBeInTheDocument();
    expect(screen.queryByText("优先继续")).not.toBeInTheDocument();
    expect(screen.queryByText("Codex 待验收")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "本周复盘" })).not.toBeInTheDocument();
  });

  it("shows a dismissible first-use guide only when no project is registered and remembers dismissal", async () => {
    window.localStorage.setItem("ganmaoyuan-ui-state-v0.4", JSON.stringify({}));
    const originalInvoke = vi.mocked(invoke).getMockImplementation()!;
    vi.mocked(invoke).mockImplementation(async (command, args) => {
      if (command === "list_projects") return [];
      return originalInvoke(command, args);
    });

    const first = render(
      <MemoryRouter initialEntries={["/"]}>
        <App />
      </MemoryRouter>,
    );

    expect(await screen.findByRole("heading", { name: "先选一个项目，再开始工作" })).toBeInTheDocument();
    expect(screen.getByText(/继续工作会恢复已登记项目的进度/)).toBeInTheDocument();
    expect(screen.getByText(/资料可添加到当前项目/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "打开已有项目" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "先导入到 Inbox" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "知道了" }));
    expect(window.localStorage.getItem("ganmaoyuan-first-use-guide-dismissed-v1")).toBe("true");
    first.unmount();

    render(
      <MemoryRouter initialEntries={["/"]}>
        <App />
      </MemoryRouter>,
    );
    await screen.findByRole("button", { name: "打开已有项目" });
    expect(screen.queryByRole("heading", { name: "先选一个项目，再开始工作" })).not.toBeInTheDocument();
  });

  it("opens an existing project folder through the canonical project recovery flow", async () => {
    const existingRoot = "D:\\GanMaoYuan\\Acceptance\\automated\\codex-live-coding-c59e4088-9433-4dfc-99e8-65c8dca9e2a1";
    const existingProject = {
      ...project,
      id: "p-test",
      name: "隔离 Codex 验收项目",
      rootDir: existingRoot,
      manifestPath: `${existingRoot}\\.ganmaoyuan\\project-location-manifest.json`,
      nextStep: "验收会话超时后的重新认证和输入恢复提示",
    };
    const existingProjectManifest = {
      ...manifest,
      project: existingProject,
      audit: [
        ...manifest.audit,
        {
          id: "next-step-confirmation",
          action: "workLedger.userNextStep",
          target: existingProject.id,
          outcome: "success",
          createdAt: "2",
          details: existingProject.nextStep,
          requiresConfirmation: false,
          confirmed: true,
        },
      ],
    };
    vi.mocked(open).mockResolvedValue(existingRoot);
    let registeredProjects = [project];
    const originalInvoke = vi.mocked(invoke).getMockImplementation()!;
    vi.mocked(invoke).mockImplementation(async (command, args) => {
      if (command === "list_projects") return registeredProjects;
      if ((args as { projectRoot?: string } | undefined)?.projectRoot === existingRoot) {
        if (command === "load_project") return existingProjectManifest;
        if (command === "get_work_ledger") {
          return {
            events: [],
            gitSnapshot: null,
            codexResults: [],
            codexTasks: [],
            pendingActions: [
              {
                id: "user-next-step",
                type: "userNextAction",
                projectId: existingProject.id,
                title: existingProject.nextStep,
                reason: "用户已明确确认。",
                sourceRef: "next-step-confirmation",
                priority: "medium",
                status: "pending",
                createdAt: "2",
                evidenceRefs: [],
              },
            ],
          };
        }
        if (command === "mark_project_opened") {
          registeredProjects = [
            existingProject,
            ...registeredProjects.filter(
              (item) => item.id !== existingProject.id &&
                item.rootDir.toLocaleLowerCase() !== existingRoot.toLocaleLowerCase(),
            ),
          ];
          return existingProject;
        }
        if (command === "synchronize_project_facts") {
          return {
            manifest: existingProjectManifest,
            workLedger: {
              events: [],
              gitSnapshot: null,
              codexResults: [],
              codexTasks: [],
              pendingActions: [
                {
                  id: "user-next-step",
                  type: "userNextAction",
                  projectId: existingProject.id,
                  title: existingProject.nextStep,
                  reason: "用户已明确确认。",
                  sourceRef: "next-step-confirmation",
                  priority: "medium",
                  status: "pending",
                  createdAt: "2",
                  evidenceRefs: [],
                },
              ],
            },
            todayWorkspace: {
              generatedAt: "2",
              continueWorkFocus: null,
              lastProgress: [],
              focusProjects: [],
              recommendedActions: [],
              blockers: [],
              pendingItems: [],
              status: "ready",
            },
            fileFactsChanged: false,
            importedCodexResults: 0,
          };
        }
      }
      return originalInvoke(command, args);
    });

    render(
      <MemoryRouter initialEntries={["/"]}>
        <App />
      </MemoryRouter>,
    );

    fireEvent.click(await screen.findByRole("button", { name: "打开已有项目" }));

    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("load_project", { projectRoot: existingRoot }),
    );
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("mark_project_opened", { projectRoot: existingRoot }),
    );
    expect(await screen.findByPlaceholderText("告诉感冒院你现在要做什么……")).toBeInTheDocument();
    expect(await screen.findByText("隔离 Codex 验收项目")).toBeInTheDocument();
    expect(await screen.findByText("验收会话超时后的重新认证和输入恢复提示")).toBeInTheDocument();
    await waitFor(() => {
      const savedState = JSON.parse(window.localStorage.getItem("ganmaoyuan-ui-state-v0.4") ?? "{}");
      expect(savedState.lastProjectRoot).toBe(existingRoot);
    });

    cleanup();
    render(
      <MemoryRouter initialEntries={["/"]}>
        <App />
      </MemoryRouter>,
    );
    const restoredProjectEntries = await screen.findAllByRole("button", {
      name: /隔离 Codex 验收项目/,
    });
    fireEvent.click(restoredProjectEntries[0]);
    expect(await screen.findByPlaceholderText("告诉感冒院你现在要做什么……")).toBeInTheDocument();
    expect(invoke).toHaveBeenCalledWith("load_project", { projectRoot: existingRoot });
  });

  it("does not put cross-project recommendations behind a homepage disclosure", async () => {
    render(
      <MemoryRouter initialEntries={["/"]}>
        <App />
      </MemoryRouter>,
    );

    await screen.findByText("最近进入的项目");
    expect(screen.queryByText("更多动态")).not.toBeInTheDocument();
    expect(screen.queryByText("今天建议")).not.toBeInTheDocument();
    expect(screen.getByText("最近进入的项目")).toBeInTheDocument();
  });

  it("opens the project picker from Start Work even when a continue focus exists", async () => {
    render(
      <MemoryRouter initialEntries={["/"]}>
        <App />
      </MemoryRouter>,
    );

    fireEvent.click(await screen.findByRole("button", { name: "开始工作" }));

    expect(await screen.findByText("选择项目继续")).toBeInTheDocument();
    expect(screen.getByRole("combobox")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "继续" })).toBeDisabled();
  });

  it("searches the full registered project list and opens an older project after saving the draft", async () => {
    const registeredProjects = [project, ...Array.from({ length: 6 }, (_, index) => ({
      ...project,
      id: `registered-${index + 2}`,
      name: `已登记项目 ${index + 2}`,
      rootDir: `D:\\GanMaoYuan\\Acceptance\\registered-${index + 2}`,
      manifestPath: `D:\\GanMaoYuan\\Acceptance\\registered-${index + 2}\\.ganmaoyuan\\manifest.json`,
      lastOpenedAt: String(10 - index),
    }))];
    const target = registeredProjects[6];
    const targetManifest: ProjectManifest = {
      ...structuredClone(manifest),
      project: target,
      messages: [{
        ...initialMessage,
        id: "target-only-message",
        author: "user",
        kind: "requirement",
        source: "user",
        text: "目标项目专属消息",
      }],
      draft: { ...manifest.draft, projectId: target.id, text: "目标项目草稿" },
    };
    const originalInvoke = vi.mocked(invoke).getMockImplementation()!;
    vi.mocked(invoke).mockImplementation(async (command, args) => {
      if (command === "list_projects") return registeredProjects;
      if (command === "load_project" && (args as { projectRoot: string }).projectRoot === target.rootDir) return targetManifest;
      if (command === "mark_project_opened" && (args as { projectRoot: string }).projectRoot === target.rootDir) return target;
      if (command === "scan_project_workspace" && (args as { projectRoot: string }).projectRoot === target.rootDir) return targetManifest;
      return originalInvoke(command, args);
    });

    render(
      <MemoryRouter initialEntries={["/work"]}>
        <App />
      </MemoryRouter>,
    );

    fireEvent.click(await screen.findByRole("button", { name: "切换项目" }));
    const search = screen.getByRole("searchbox", { name: "搜索已登记项目" });
    fireEvent.change(search, { target: { value: target.name } });
    const targetOption = await screen.findByRole("button", { name: new RegExp(target.name) });
    expect(targetOption).toBeEnabled();
    fireEvent.click(targetOption);

    expect(await screen.findByText("目标项目专属消息")).toBeInTheDocument();
    expect(invoke).toHaveBeenCalledWith("save_project_draft", expect.objectContaining({
      projectRoot: project.rootDir,
      text: "尚未发送的草稿",
    }));
    expect(invoke).toHaveBeenCalledWith("load_project", { projectRoot: target.rootDir });
    const calls = vi.mocked(invoke).mock.calls;
    const targetLoadIndex = calls.findIndex(([command, args]) =>
      command === "load_project" && (args as { projectRoot: string }).projectRoot === target.rootDir,
    );
    const draftSaveIndex = calls.findIndex(([command, args]) =>
      command === "save_project_draft" && (args as { projectRoot: string }).projectRoot === project.rootDir,
    );
    expect(draftSaveIndex).toBeGreaterThanOrEqual(0);
    expect(draftSaveIndex).toBeLessThan(targetLoadIndex);
  });

  it("cancels project selection without opening another project", async () => {
    const target = { ...project, id: "project-older", name: "较早登记项目", rootDir: "D:\\GanMaoYuan\\Acceptance\\older" };
    const originalInvoke = vi.mocked(invoke).getMockImplementation()!;
    vi.mocked(invoke).mockImplementation(async (command, args) => {
      if (command === "list_projects") return [project, target];
      return originalInvoke(command, args);
    });
    render(
      <MemoryRouter initialEntries={["/work"]}>
        <App />
      </MemoryRouter>,
    );
    fireEvent.click(await screen.findByRole("button", { name: "切换项目" }));
    fireEvent.change(screen.getByRole("searchbox", { name: "搜索已登记项目" }), { target: { value: target.name } });
    expect(await screen.findByRole("button", { name: new RegExp(target.name) })).toBeInTheDocument();
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByRole("dialog", { name: "切换项目" })).not.toBeInTheDocument();
    expect(invoke).not.toHaveBeenCalledWith("load_project", { projectRoot: target.rootDir });
  });

  it("prevents duplicate project selection while the current draft is being saved", async () => {
    const target = { ...project, id: "project-older", name: "较早登记项目", rootDir: "D:\\GanMaoYuan\\Acceptance\\older" };
    const save = deferred<unknown>();
    const originalInvoke = vi.mocked(invoke).getMockImplementation()!;
    let holdNextDraftSave = false;
    vi.mocked(invoke).mockImplementation(async (command, args) => {
      if (command === "list_projects") return [project, target];
      if (command === "save_project_draft" && holdNextDraftSave) {
        holdNextDraftSave = false;
        return save.promise;
      }
      return originalInvoke(command, args);
    });
    render(
      <MemoryRouter initialEntries={["/work"]}>
        <App />
      </MemoryRouter>,
    );
    fireEvent.click(await screen.findByRole("button", { name: "切换项目" }));
    const targetOption = await screen.findByRole("button", { name: new RegExp(target.name) });
    holdNextDraftSave = true;
    fireEvent.click(targetOption);
    expect(await screen.findByText("正在切换…")).toBeInTheDocument();
    fireEvent.click(targetOption);
    expect(vi.mocked(invoke).mock.calls.filter(([command, args]) =>
      command === "save_project_draft" && (args as { projectRoot: string }).projectRoot === project.rootDir,
    )).toHaveLength(1);
    expect(invoke).not.toHaveBeenCalledWith("load_project", { projectRoot: target.rootDir });
    await act(async () => save.resolve({ ...manifest.draft, text: "尚未发送的草稿", updatedAt: "saved" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("load_project", { projectRoot: target.rootDir }));
  });

  it("keeps the current project open when its composer draft cannot be saved", async () => {
    const target = { ...project, id: "project-older", name: "较早登记项目", rootDir: "D:\\GanMaoYuan\\Acceptance\\older" };
    const originalInvoke = vi.mocked(invoke).getMockImplementation()!;
    let failNextDraftSave = false;
    vi.mocked(invoke).mockImplementation(async (command, args) => {
      if (command === "list_projects") return [project, target];
      if (command === "save_project_draft" && failNextDraftSave) {
        failNextDraftSave = false;
        throw new Error("模拟草稿保存失败");
      }
      return originalInvoke(command, args);
    });
    render(
      <MemoryRouter initialEntries={["/work"]}>
        <App />
      </MemoryRouter>,
    );
    fireEvent.click(await screen.findByRole("button", { name: "切换项目" }));
    const targetOption = await screen.findByRole("button", { name: new RegExp(target.name) });
    failNextDraftSave = true;
    fireEvent.click(targetOption);
    expect(await screen.findByRole("alert")).toHaveTextContent("切换项目失败，当前项目未更改：模拟草稿保存失败");
    expect(invoke).not.toHaveBeenCalledWith("load_project", { projectRoot: target.rootDir });
    expect(screen.getAllByText(project.name).length).toBeGreaterThan(0);
  });

  it("keeps project switching unavailable while a Codex run is active", async () => {
    const target = { ...project, id: "project-older", name: "较早登记项目", rootDir: "D:\\GanMaoYuan\\Acceptance\\older" };
    codexConsoleRuns = [{
      id: "run-active",
      taskId: "task-active",
      projectId: project.id,
      status: "running",
      processAlive: true,
      pid: 42,
      startedAt: "now",
      lastActivityAt: "now",
      recentActivity: [],
      rawActivity: [],
    }];
    const originalInvoke = vi.mocked(invoke).getMockImplementation()!;
    vi.mocked(invoke).mockImplementation(async (command, args) => {
      if (command === "list_projects") return [project, target];
      return originalInvoke(command, args);
    });
    render(
      <MemoryRouter initialEntries={["/work"]}>
        <App />
      </MemoryRouter>,
    );
    fireEvent.click(await screen.findByRole("button", { name: "切换项目" }));
    const targetOption = await screen.findByRole("button", { name: new RegExp(target.name) });
    await waitFor(() => expect(screen.getByRole("status")).toHaveTextContent("Codex 任务正在启动或运行"));
    expect(targetOption).toBeDisabled();
    expect(invoke).not.toHaveBeenCalledWith("load_project", { projectRoot: target.rootDir });
  });

  it("protects an edited Codex prompt from being discarded during project switching", async () => {
    const target = { ...project, id: "project-older", name: "较早登记项目", rootDir: "D:\\GanMaoYuan\\Acceptance\\older" };
    const originalInvoke = vi.mocked(invoke).getMockImplementation()!;
    vi.mocked(invoke).mockImplementation(async (command, args) => {
      if (command === "list_projects") return [project, target];
      return originalInvoke(command, args);
    });
    render(
      <MemoryRouter initialEntries={["/work"]}>
        <App />
      </MemoryRouter>,
    );
    fireEvent.click(await screen.findByRole("button", { name: "Codex 任务" }));
    fireEvent.click(await screen.findByText("查看提示词"));
    const promptTextArea = screen.getAllByRole("textbox").find((node) => node.tagName === "TEXTAREA" && node.closest(".codex-technical-block"));
    expect(promptTextArea).toBeDefined();
    fireEvent.change(promptTextArea!, { target: { value: "尚未保存的 Codex 提示词编辑" } });
    fireEvent.click(screen.getByRole("button", { name: "切换项目" }));
    const targetOption = await screen.findByRole("button", { name: new RegExp(target.name) });
    expect(targetOption).toBeDisabled();
    expect(screen.getByText("当前页面有尚未保存的输入，请先保存、提交或清空后再切换。")).toBeInTheDocument();
    expect(invoke).not.toHaveBeenCalledWith("load_project", { projectRoot: target.rootDir });
  });

  it("shows the inbox next action once in a clearly labeled row", async () => {
    const nextAction = "确认项目与位置后即可归位到 感冒院验收项目。";
    inboxEntries = [{
      id: "inbox-1",
      fileName: "acceptance-plan.md",
      documentType: "计划",
      contentSummary: "用于验收资料列表层级。",
      processingStatus: "pendingReview",
      ownershipType: "existingProject",
      recommendedProjectName: project.name,
      recommendedRelativeLocation: "requirements",
      targetProjectRoot: project.rootDir,
      projectCandidates: [],
      statusHistory: [],
      sourceHistory: [],
      decisionBasis: [],
      decisionTraces: [],
    } as unknown as MaterialInboxItem];

    render(
      <MemoryRouter initialEntries={["/?mode=inbox"]}>
        <App />
      </MemoryRouter>,
    );

    const fileNameNodes = await screen.findAllByText("acceptance-plan.md");
    const fileName = fileNameNodes.find((node) => node.closest(".inbox-item"));
    const item = fileName?.closest(".inbox-item");
    expect(item).not.toBeNull();
    expect(item).toHaveTextContent("系统结论");
    expect(item).toHaveTextContent("下一步");
    expect(item?.textContent?.split(nextAction)).toHaveLength(2);
    expect(item?.querySelector(".inbox-hero-main p")).toBeNull();
    expect(item).toContainElement(screen.getByRole("button", { name: "确认归位" }));
  });

  it("keeps low-frequency work tools behind a More disclosure", async () => {
    render(
      <MemoryRouter initialEntries={["/work"]}>
        <App />
      </MemoryRouter>,
    );

    await screen.findByRole("button", { name: "项目对话" });
    const moreTools = screen.getByText("更多工具");
    expect(moreTools.closest("details")).not.toHaveAttribute("open");
    fireEvent.click(moreTools);
    expect(screen.getByRole("button", { name: "本周复盘" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "项目影响与行动" })).toBeInTheDocument();
  });

  it("switches workspace tools without retaining stale panels or hiding chat controls", async () => {
    const { container } = render(
      <MemoryRouter initialEntries={["/work"]}>
        <App />
      </MemoryRouter>,
    );

    await screen.findByRole("button", { name: "项目对话" });
    const shell = container.querySelector(".workspace-shell");
    const openMoreTools = () => {
      const details = screen.getByText("更多工具").closest("details");
      if (details && !details.open) fireEvent.click(details.querySelector("summary")!);
    };

    openMoreTools();
    fireEvent.click(screen.getByRole("button", { name: "全局搜索" }));
    expect(await screen.findByRole("textbox", { name: "全局搜索" })).toBeInTheDocument();
    expect(container.querySelector(".global-search-panel")).toBeInTheDocument();
    expect(shell).toHaveClass("panel-mode");

    openMoreTools();
    fireEvent.click(screen.getByRole("button", { name: "项目上下文" }));
    expect(await screen.findByRole("heading", { name: "项目上下文" })).toBeInTheDocument();
    expect(container.querySelector(".global-search-panel")).not.toBeInTheDocument();

    openMoreTools();
    fireEvent.click(screen.getByRole("button", { name: "记录工作" }));
    expect(await screen.findByPlaceholderText("例如：完成 Workspace 整理闭环的人工验收并记录结果。"))
      .toBeInTheDocument();
    expect(container.querySelector(".project-context-panel:not(.work-capture-panel)"))
      .not.toBeInTheDocument();

    openMoreTools();
    fireEvent.click(screen.getByRole("button", { name: "本周复盘" }));
    expect(container.querySelector(".weekly-review-note")).toBeInTheDocument();
    expect(shell).toHaveClass("report-mode");

    fireEvent.click(screen.getByRole("button", { name: "项目影响与行动" }));
    await waitFor(() => expect(container.querySelector(".project-impact-note")).toBeInTheDocument());
    expect(container.querySelector(".weekly-review-note")).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "项目对话" }));
    expect(shell).not.toHaveClass("panel-mode", "report-mode");
    expect(container.querySelector(".project-impact-note")).not.toBeInTheDocument();
    expect(screen.getByPlaceholderText("告诉感冒院你现在要做什么……")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "发送" })).toBeInTheDocument();
  });

  it("keeps the conversation composer compact and gives Codex its own workspace mode", async () => {
    const { container } = render(
      <MemoryRouter initialEntries={["/work"]}>
        <App />
      </MemoryRouter>,
    );

    await screen.findByRole("button", { name: "项目对话" });
    const composer = container.querySelector(".composer-row");
    const draft = screen.getByPlaceholderText("告诉感冒院你现在要做什么……");
    expect(draft).toHaveAttribute("rows", "2");
    expect(screen.getByRole("button", { name: "发送" }).closest(".composer-row")).toBe(composer);
    expect(screen.queryByRole("button", { name: "停止" })).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Codex 任务" }));
    await waitFor(() => expect(container.querySelector(".codex-console")).toBeInTheDocument());
    expect(container.querySelector(".workspace-shell")).toHaveClass("panel-mode");

    fireEvent.click(screen.getByRole("button", { name: "项目对话" }));
    expect(container.querySelector(".workspace-shell")).not.toHaveClass("panel-mode");
  });

  it("restores the conversation reading position after returning from Codex", async () => {
    const { container } = render(
      <MemoryRouter initialEntries={["/work"]}>
        <App />
      </MemoryRouter>,
    );

    await screen.findByRole("button", { name: "项目对话" });
    const thread = container.querySelector<HTMLElement>(".workspace-thread");
    expect(thread).not.toBeNull();
    Object.defineProperties(thread!, {
      scrollHeight: { configurable: true, value: 2400 },
      clientHeight: { configurable: true, value: 800 },
    });
    thread!.scrollTop = 720;
    fireEvent.scroll(thread!);

    fireEvent.click(screen.getByRole("button", { name: "Codex 任务" }));
    await waitFor(() => expect(container.querySelector(".codex-console")).toBeInTheDocument());
    fireEvent.click(screen.getByRole("button", { name: "项目对话" }));

    await waitFor(() => expect(container.querySelector(".workspace-shell")).not.toHaveClass("panel-mode"));
    expect(thread!.scrollTop).toBe(720);
  });

  it("keeps the composer in its own bottom grid row outside the scrolling work area", async () => {
    const { container } = render(
      <MemoryRouter initialEntries={["/work"]}>
        <App />
      </MemoryRouter>,
    );

    await screen.findByRole("button", { name: "项目对话" });
    const requireElement = (root: ParentNode, selector: string) => {
      const element = root.querySelector<HTMLElement>(selector);
      if (!element) throw new Error(`Expected ${selector} to exist`);
      return element;
    };
    const shell = requireElement(container, ".workspace-shell");
    const mainArea = requireElement(shell, ":scope > .app-layout-main");
    const conversation = requireElement(mainArea, ":scope > .workspace-body");
    const thread = requireElement(conversation, ".workspace-thread");
    const composer = requireElement(mainArea, ":scope > .workspace-composer");

    expect(mainArea).toContainElement(conversation);
    expect(conversation).toContainElement(thread);
    expect(composer).toContainElement(screen.getByPlaceholderText("告诉感冒院你现在要做什么……"));
    expect(composer).toContainElement(screen.getByRole("button", { name: "添加资料" }));
    expect(composer).toContainElement(screen.getByRole("button", { name: "图片" }));
    expect(composer).toContainElement(screen.getByRole("button", { name: "语音" }));
    expect(composer).toContainElement(screen.getByRole("button", { name: "发送" }));
    expect(composer.parentElement).toBe(mainArea);
    expect(thread).not.toContainElement(composer);
  });

  it("keeps the composer controls available while long conversation history is paged", async () => {
    const originalInvoke = vi.mocked(invoke).getMockImplementation()!;
    const longManifest: ProjectManifest = {
      ...manifest,
      messages: Array.from({ length: 75 }, (_, index) => ({
        ...initialMessage,
        id: `history-${index}`,
        author: index % 2 ? "ganmaoyuan" : "user",
        kind: index % 2 ? "assistant" : "requirement",
        text: `历史消息 ${index}：用于验证长对话滚动时输入区仍独立存在。\n`.repeat(8),
        source: index % 2 ? "deepseek" : "user",
      })),
    };
    vi.mocked(invoke).mockImplementation(async (command, args) => {
      if (command === "load_project" || command === "scan_project_workspace") return longManifest;
      return originalInvoke(command, args);
    });

    const { container } = render(
      <MemoryRouter initialEntries={["/work"]}>
        <App />
      </MemoryRouter>,
    );

    await waitFor(() => {
      expect([...container.querySelectorAll(".thread-message")].some((message) => message.textContent?.includes("历史消息 74"))).toBe(true);
    });
    expect(screen.getByRole("button", { name: /加载更早的/ })).toBeInTheDocument();
    const composer = container.querySelector(".workspace-composer");
    expect(composer).toContainElement(screen.getByPlaceholderText("告诉感冒院你现在要做什么……"));
    expect(composer).toContainElement(screen.getByRole("button", { name: "发送" }));
  });

  it("keeps implementation names out of the ordinary work surface", async () => {
    render(
      <MemoryRouter initialEntries={["/work"]}>
        <App />
      </MemoryRouter>,
    );

    await screen.findByRole("button", { name: "项目对话" });

    expect(screen.getAllByRole("button", { name: "启用 AI 回复" }).length).toBeGreaterThan(0);
    expect(screen.queryByText(/DeepSeek/i)).not.toBeInTheDocument();
    expect(screen.queryByText(/Atlas 与监视/i)).not.toBeInTheDocument();
  });

  it("requires renewed consent for a legacy project AI payload scope", async () => {
    const originalAuthorization = manifest.deepseekAuthorization;
    manifest.deepseekAuthorization = {
      grantedAt: "legacy-grant",
      grantedBy: "user",
      scopeVersion: 1,
    };
    try {
      render(
        <MemoryRouter initialEntries={["/work"]}>
          <App />
        </MemoryRouter>,
      );

      expect(await screen.findByText("需要授权或重新确认数据范围")).toBeInTheDocument();
      expect(screen.getAllByRole("button", { name: "启用 AI 回复" }).length).toBeGreaterThan(0);
      expect(screen.getAllByText(/最多 5 轮历史对话/).length).toBeGreaterThan(0);
      expect(screen.getAllByText(/历史对话仅用于理解当前讨论，不作为事实或已确认决定/).length).toBeGreaterThan(0);
      expect(screen.getByText(/当前只读 Atlas 状态摘要/)).toBeInTheDocument();
    } finally {
      manifest.deepseekAuthorization = originalAuthorization;
    }
  });

  it("opens AI model settings from project authorization and returns to the exact work location", async () => {
    render(
      <MemoryRouter initialEntries={["/work?panel=codex&taskId=task-1"]}>
        <>
          <App />
          <RoutePathProbe />
        </>
      </MemoryRouter>,
    );

    fireEvent.click(await screen.findByRole("button", { name: "配置/测试 AI 连接" }));
    expect(await screen.findByText("DeepSeek API Key")).toBeInTheDocument();
    expect(screen.getByTestId("current-route")).toHaveTextContent("/settings?section=ai-models");

    fireEvent.click(screen.getByRole("button", { name: "返回工作" }));
    await waitFor(() => expect(screen.getByTestId("current-route")).toHaveTextContent("/work?panel=codex&taskId=task-1"));
  });

  it("offers AI connection setup when the Key is missing without re-requesting an existing project authorization", async () => {
    const originalAuthorization = manifest.deepseekAuthorization;
    manifest.deepseekAuthorization = {
      grantedAt: "scope-3-grant",
      grantedBy: "user",
      scopeVersion: 3,
    };

    try {
      render(
        <MemoryRouter initialEntries={["/work?panel=codex"]}>
          <>
            <App />
            <RoutePathProbe />
          </>
        </MemoryRouter>,
      );

      expect(await screen.findByText("需要配置 AI 连接")).toBeInTheDocument();
      expect(screen.getAllByRole("button", { name: "配置/测试 AI 连接" }).length).toBeGreaterThan(0);
      expect(screen.queryByRole("button", { name: "启用 AI 回复" })).not.toBeInTheDocument();
      fireEvent.click(screen.getAllByRole("button", { name: "配置/测试 AI 连接" })[0]);
      expect(await screen.findByText("DeepSeek API Key")).toBeInTheDocument();
      expect(screen.getByTestId("current-route")).toHaveTextContent("/settings?section=ai-models");
      expect(invoke).not.toHaveBeenCalledWith("grant_project_deepseek_authorization", expect.anything());
    } finally {
      manifest.deepseekAuthorization = originalAuthorization;
    }
  });

  it("does not treat a failed settings read as a missing DeepSeek Key", async () => {
    const originalAuthorization = manifest.deepseekAuthorization;
    const originalInvoke = vi.mocked(invoke).getMockImplementation()!;
    manifest.deepseekAuthorization = {
      grantedAt: "scope-3-grant",
      grantedBy: "user",
      scopeVersion: 3,
    };
    vi.mocked(invoke).mockImplementation(async (command, args) => {
      if (command === "load_deepseek_settings") throw new Error("settings read failure");
      return originalInvoke(command, args);
    });

    try {
      render(
        <MemoryRouter initialEntries={["/work"]}>
          <AppProvider>
            <>
              <WorkPage />
              <DeepSeekSettingsStateProbe />
            </>
          </AppProvider>
        </MemoryRouter>,
      );

      await waitFor(() => expect(screen.getByTestId("deepseek-settings-state")).toHaveTextContent("not-loaded:Error: settings read failure"));
      expect(screen.queryByText("需要配置 AI 连接")).not.toBeInTheDocument();
      expect(screen.queryByText("未配置 AI 连接")).not.toBeInTheDocument();
    } finally {
      manifest.deepseekAuthorization = originalAuthorization;
    }
  });

  it("shares immediate AI authorization progress and suppresses duplicate requests", async () => {
    const originalAuthorization = manifest.deepseekAuthorization;
    const originalInvoke = vi.mocked(invoke).getMockImplementation()!;
    const authorization = deferred<ProjectManifest>();
    const otherProject = { ...project, id: "project-other", name: "另一个已登记项目", rootDir: "D:\\GanMaoYuan\\Acceptance\\other" };
    manifest.deepseekAuthorization = {
      grantedAt: "scope-2-grant",
      grantedBy: "user",
      scopeVersion: 2,
    };
    vi.mocked(invoke).mockImplementation(async (command, args) => {
      if (command === "list_projects") return [project, otherProject];
      if (command === "grant_project_deepseek_authorization") return authorization.promise;
      if (command === "refresh_project_understanding") {
        return {
          ...manifest,
          deepseekAuthorization: { grantedAt: "scope-3-grant", grantedBy: "user", scopeVersion: 3 },
        };
      }
      return originalInvoke(command, args);
    });

    try {
      render(
        <MemoryRouter initialEntries={["/work"]}>
          <App />
        </MemoryRouter>,
      );
      const grantButtons = await screen.findAllByRole("button", { name: "启用 AI 回复" });
      expect(grantButtons).toHaveLength(2);
      fireEvent.click(grantButtons[0]);

      const pendingButtons = await screen.findAllByRole("button", { name: "正在启用 AI 回复…" });
      expect(pendingButtons).toHaveLength(2);
      pendingButtons.forEach((button) => expect(button).toBeDisabled());
      fireEvent.click(pendingButtons[1]);
      expect(vi.mocked(invoke).mock.calls.filter(([command]) => command === "grant_project_deepseek_authorization")).toHaveLength(1);

      fireEvent.click(screen.getByRole("button", { name: "切换项目" }));
      const otherProjectOption = await screen.findByRole("button", { name: new RegExp(otherProject.name) });
      expect(otherProjectOption).toBeDisabled();
      fireEvent.click(screen.getByRole("button", { name: "取消" }));

      await act(async () => authorization.resolve({
        ...manifest,
        deepseekAuthorization: { grantedAt: "scope-3-grant", grantedBy: "user", scopeVersion: 3 },
      }));
      expect(await screen.findAllByText("AI 回复授权已保存，项目理解已更新。")).toHaveLength(2);
      expect(invoke).toHaveBeenCalledWith("refresh_project_understanding", { projectRoot: project.rootDir });
    } finally {
      manifest.deepseekAuthorization = originalAuthorization;
    }
  });

  it("hides the consent action after version 3 authorization is persisted", async () => {
    const originalAuthorization = manifest.deepseekAuthorization;
    const originalInvoke = vi.mocked(invoke).getMockImplementation()!;
    manifest.deepseekAuthorization = {
      grantedAt: "scope-2-grant",
      grantedBy: "user",
      scopeVersion: 2,
    };
    vi.mocked(invoke).mockImplementation(async (command, args) => {
      if (command === "grant_project_deepseek_authorization" || command === "refresh_project_understanding") {
        return {
          ...manifest,
          deepseekAuthorization: { grantedAt: "scope-3-grant", grantedBy: "user", scopeVersion: 3 },
        };
      }
      return originalInvoke(command, args);
    });

    try {
      render(
        <MemoryRouter initialEntries={["/work"]}>
          <App />
        </MemoryRouter>,
      );

      const grantButtons = await screen.findAllByRole("button", { name: "启用 AI 回复" });
      fireEvent.click(grantButtons[0]);

      await waitFor(() => {
        expect(invoke).toHaveBeenCalledWith("grant_project_deepseek_authorization", {
          projectRoot: project.rootDir,
        });
        expect(screen.queryByRole("button", { name: "启用 AI 回复" })).not.toBeInTheDocument();
      });
    } finally {
      manifest.deepseekAuthorization = originalAuthorization;
    }
  });

  it("shows a visible error when project AI authorization fails", async () => {
    const originalAuthorization = manifest.deepseekAuthorization;
    const originalInvoke = vi.mocked(invoke).getMockImplementation()!;
    manifest.deepseekAuthorization = {
      grantedAt: "scope-2-grant",
      grantedBy: "user",
      scopeVersion: 2,
    };
    vi.mocked(invoke).mockImplementation(async (command, args) => {
      if (command === "grant_project_deepseek_authorization") {
        throw new Error("模拟授权写入失败");
      }
      return originalInvoke(command, args);
    });

    try {
      render(
        <MemoryRouter initialEntries={["/work"]}>
          <App />
        </MemoryRouter>,
      );

      fireEvent.click((await screen.findAllByRole("button", { name: "启用 AI 回复" }))[0]);

      expect(await screen.findAllByText("AI 授权失败：模拟授权写入失败")).toHaveLength(2);
      expect(screen.getAllByRole("button", { name: "启用 AI 回复" }).length).toBeGreaterThan(0);
    } finally {
      manifest.deepseekAuthorization = originalAuthorization;
    }
  });

  it.each([
    ["操作已取消", "AI 授权已取消：操作已取消"],
    ["请求超时", "AI 授权请求超时：请求超时"],
  ])("shows a distinct AI authorization outcome for %s", async (failure, expectedFeedback) => {
    const originalAuthorization = manifest.deepseekAuthorization;
    const originalInvoke = vi.mocked(invoke).getMockImplementation()!;
    manifest.deepseekAuthorization = {
      grantedAt: "scope-2-grant",
      grantedBy: "user",
      scopeVersion: 2,
    };
    vi.mocked(invoke).mockImplementation(async (command, args) => {
      if (command === "grant_project_deepseek_authorization") throw new Error(failure);
      return originalInvoke(command, args);
    });

    try {
      render(
        <MemoryRouter initialEntries={["/work"]}>
          <App />
        </MemoryRouter>,
      );
      fireEvent.click((await screen.findAllByRole("button", { name: "启用 AI 回复" }))[0]);
      expect(await screen.findAllByText(expectedFeedback)).toHaveLength(2);
      expect(screen.getAllByRole("button", { name: "启用 AI 回复" })).toHaveLength(2);
    } finally {
      manifest.deepseekAuthorization = originalAuthorization;
    }
  });

  it("keeps persisted consent when project understanding refresh fails", async () => {
    const originalAuthorization = manifest.deepseekAuthorization;
    const originalInvoke = vi.mocked(invoke).getMockImplementation()!;
    manifest.deepseekAuthorization = {
      grantedAt: "scope-2-grant",
      grantedBy: "user",
      scopeVersion: 2,
    };
    vi.mocked(invoke).mockImplementation(async (command, args) => {
      if (command === "grant_project_deepseek_authorization") {
        return {
          ...manifest,
          deepseekAuthorization: { grantedAt: "scope-3-grant", grantedBy: "user", scopeVersion: 3 },
        };
      }
      if (command === "refresh_project_understanding") {
        throw new Error("请先在设置页保存 DeepSeek API Key 并测试连接。");
      }
      return originalInvoke(command, args);
    });

    try {
      render(
        <MemoryRouter initialEntries={["/work"]}>
          <App />
        </MemoryRouter>,
      );

      fireEvent.click((await screen.findAllByRole("button", { name: "启用 AI 回复" }))[0]);

      expect(await screen.findAllByText(/AI 回复授权已保存，但项目理解更新失败：请先在设置页保存 DeepSeek API Key 并测试连接/)).toHaveLength(2);
      expect(screen.queryByRole("button", { name: "启用 AI 回复" })).not.toBeInTheDocument();
    } finally {
      manifest.deepseekAuthorization = originalAuthorization;
    }
  });

  it("opens reference details only on explicit request and keeps them closed through authorization and refresh", async () => {
    const originalFiles = manifest.files;
    const originalAuthorization = manifest.deepseekAuthorization;
    const originalInvoke = vi.mocked(invoke).getMockImplementation()!;
    const file = makeManagedFile("reference-one", "授权范围说明.md");
    manifest.files = [file];
    manifest.deepseekAuthorization = { grantedAt: "scope-2-grant", grantedBy: "user", scopeVersion: 2 };
    const authorizedManifest = () => ({
      ...structuredClone(manifest),
      deepseekAuthorization: { grantedAt: "scope-3-grant", grantedBy: "user", scopeVersion: 3 },
    });
    vi.mocked(invoke).mockImplementation(async (command, args) => {
      if (command === "grant_project_deepseek_authorization" || command === "refresh_project_understanding") {
        return authorizedManifest();
      }
      return originalInvoke(command, args);
    });

    const view = render(
      <AppProvider>
        <MemoryRouter initialEntries={["/work"]}>
          <WorkPage />
          <ProjectRefreshTestControl />
        </MemoryRouter>
      </AppProvider>,
    );

    try {
      expect(await screen.findByText(file.fileName)).toBeInTheDocument();
      expect(screen.queryByRole("dialog", { name: file.fileName })).not.toBeInTheDocument();

      fireEvent.click((await screen.findAllByRole("button", { name: "启用 AI 回复" }))[0]);
      expect(await screen.findAllByText("AI 回复授权已保存，项目理解已更新。")).toHaveLength(2);
      expect(screen.queryByRole("dialog", { name: file.fileName })).not.toBeInTheDocument();

      fireEvent.click(screen.getByRole("button", { name: "触发项目理解刷新" }));
      await waitFor(() => expect(invoke).toHaveBeenCalledWith("refresh_project_understanding", {
        projectRoot: project.rootDir,
      }));
      expect(screen.queryByRole("dialog", { name: file.fileName })).not.toBeInTheDocument();

      fireEvent.click(screen.getByRole("button", { name: "展开详情" }));
      const dialog = await screen.findByRole("dialog", { name: file.fileName });
      expect(dialog).toBeInTheDocument();
      fireEvent.click(within(dialog).getByRole("button", { name: "关闭" }));
      expect(screen.queryByRole("dialog", { name: file.fileName })).not.toBeInTheDocument();

      fireEvent.click(screen.getByRole("button", { name: "触发项目理解刷新" }));
      await waitFor(() => {
        expect(vi.mocked(invoke).mock.calls.filter(([command]) => command === "refresh_project_understanding").length)
          .toBeGreaterThanOrEqual(2);
      });
      expect(screen.queryByRole("dialog", { name: file.fileName })).not.toBeInTheDocument();
    } finally {
      view.unmount();
      manifest.files = originalFiles;
      manifest.deepseekAuthorization = originalAuthorization;
    }
  });

  it("clears reference selection when switching projects and does not open the target project's file", async () => {
    const originalFiles = manifest.files;
    const sourceFile = makeManagedFile("source-reference", "来源项目资料.md");
    const targetFile = makeManagedFile("target-reference", "目标项目资料.md");
    const target: ProjectSummary = {
      ...project,
      id: "project-reference-target",
      name: "资料目标项目",
      rootDir: "D:\\GanMaoYuan\\Acceptance\\reference-target",
      manifestPath: "D:\\GanMaoYuan\\Acceptance\\reference-target\\.ganmaoyuan\\manifest.json",
    };
    manifest.files = [sourceFile];
    const targetManifest: ProjectManifest = {
      ...structuredClone(manifest),
      project: target,
      files: [targetFile],
      draft: { ...manifest.draft, projectId: target.id, text: "" },
    };
    const originalInvoke = vi.mocked(invoke).getMockImplementation()!;
    vi.mocked(invoke).mockImplementation(async (command, args) => {
      const projectRoot = (args as { projectRoot?: string } | undefined)?.projectRoot;
      if (command === "list_projects") return [project, target];
      if (command === "load_project" && projectRoot === target.rootDir) return targetManifest;
      if (command === "mark_project_opened" && projectRoot === target.rootDir) return target;
      if (command === "scan_project_workspace" && projectRoot === target.rootDir) return targetManifest;
      return originalInvoke(command, args);
    });

    const view = render(
      <MemoryRouter initialEntries={["/work"]}>
        <App />
      </MemoryRouter>,
    );

    try {
      expect(await screen.findByText(sourceFile.fileName)).toBeInTheDocument();
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
      fireEvent.click(screen.getByRole("button", { name: "展开详情" }));
      expect(await screen.findByRole("dialog", { name: sourceFile.fileName })).toBeInTheDocument();

      fireEvent.click(screen.getByRole("button", { name: "切换项目" }));
      fireEvent.click(await screen.findByRole("button", { name: new RegExp(target.name) }));

      expect(await screen.findByText(targetFile.fileName)).toBeInTheDocument();
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
      expect(screen.queryByText(sourceFile.fileName)).not.toBeInTheDocument();
    } finally {
      view.unmount();
      manifest.files = originalFiles;
    }
  });

  it("records a user decision and refreshes the work ledger immediately", async () => {
    render(
      <MemoryRouter initialEntries={["/work"]}>
        <App />
      </MemoryRouter>,
    );

    await screen.findByRole("button", { name: "项目对话" });
    fireEvent.click(screen.getByRole("button", { name: "Codex 任务" }));

    const decisionInput = await screen.findByPlaceholderText("例如：暂不做 ChatGPT MCP");
    fireEvent.change(decisionInput, { target: { value: "暂不做 ChatGPT MCP" } });
    fireEvent.change(screen.getByPlaceholderText("说明这条决定的依据"), {
      target: { value: "先验证 Phase A 价值。" },
    });
    fireEvent.click(screen.getByRole("button", { name: "记录决定" }));

    expect(await screen.findByText("用户确认决定：暂不做 ChatGPT MCP · 高 100%")).toBeInTheDocument();
    await waitFor(() => expect(decisionInput).toHaveValue(""));
    expect(invoke).toHaveBeenCalledWith("record_user_decision_event", {
      projectRoot: project.rootDir,
      decision: "暂不做 ChatGPT MCP",
      reason: "先验证 Phase A 价值。",
    });
  });

  it("records a confirmed project fact without routing it through AI chat", async () => {
    render(
      <MemoryRouter initialEntries={["/work"]}>
        <App />
      </MemoryRouter>,
    );

    await screen.findByRole("button", { name: "项目对话" });
    fireEvent.click(screen.getByText("更多工具"));
    fireEvent.click(screen.getByRole("button", { name: "记录工作" }));

    const content = await screen.findByPlaceholderText("例如：完成 Workspace 整理闭环的人工验收并记录结果。");
    fireEvent.change(content, { target: { value: "完成真实项目事实回流验收" } });
    fireEvent.click(screen.getByRole("button", { name: "记录完成进展" }));

    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("capture_project_fact", {
        projectRoot: project.rootDir,
        request: {
          captureType: "progress",
          content: "完成真实项目事实回流验收",
          reason: "",
        },
      }),
    );
    await waitFor(() => expect(content).toHaveValue(""));
    expect(await screen.findByText("已写入项目工作事实。继续工作时会自动使用这条记录。")).toBeInTheDocument();
    expect(invoke).not.toHaveBeenCalledWith("send_project_message", expect.anything());
  });

  it("archives external AI text and writes only the paragraph the user explicitly confirms", async () => {
    const originalFiles = manifest.files;
    try {
      render(
        <MemoryRouter initialEntries={["/work"]}>
          <App />
        </MemoryRouter>,
      );

      await screen.findByRole("button", { name: "项目对话" });
      fireEvent.click(screen.getByText("更多工具"));
      fireEvent.click(screen.getByRole("button", { name: "外部 AI 回流" }));
      expect(await screen.findByRole("heading", { name: "带回外部讨论结果" })).toBeInTheDocument();

      const sourceText = "建议检查资料归属。\n\n已核实：Word 批次上限为 120。";
      fireEvent.change(screen.getByPlaceholderText("在这里粘贴外部 AI 的回复……"), {
        target: { value: sourceText },
      });
      fireEvent.click(screen.getByRole("button", { name: "已核实：Word 批次上限为 120。" }));
      fireEvent.change(screen.getByLabelText("如何处理这段内容"), { target: { value: "progress" } });
      fireEvent.click(screen.getByRole("button", { name: "确认并记录已核实进展" }));

      expect(await screen.findByText(/真实 Work Ledger 事件 ID：capture-progress/)).toBeInTheDocument();
      expect(registeredExternalFiles).toHaveLength(1);
      expect(invoke).toHaveBeenCalledWith("register_generated_file", expect.objectContaining({
        projectRoot: project.rootDir,
        createdSource: "外部 AI 回流",
        content: expect.stringContaining("未经感冒院验证"),
      }));
      expect(invoke).toHaveBeenCalledWith("capture_project_fact", {
        projectRoot: project.rootDir,
        request: expect.objectContaining({
          captureType: "progress",
          content: "已核实：Word 批次上限为 120。",
          externalAiSourceFileId: registeredExternalFiles[0].id,
        }),
      });
      expect(invoke).not.toHaveBeenCalledWith("send_project_message", expect.anything());
    } finally {
      manifest.files = originalFiles;
    }
  });

  it("keeps an external AI suggestion as a source document without writing a project fact", async () => {
    const originalFiles = manifest.files;
    try {
      render(
        <MemoryRouter initialEntries={["/work"]}>
          <App />
        </MemoryRouter>,
      );

      await screen.findByRole("button", { name: "项目对话" });
      fireEvent.click(screen.getByText("更多工具"));
      fireEvent.click(screen.getByRole("button", { name: "外部 AI 回流" }));
      fireEvent.change(screen.getByPlaceholderText("在这里粘贴外部 AI 的回复……"), {
        target: { value: "建议以后再考虑其他方案。" },
      });
      fireEvent.click(screen.getByRole("button", { name: "仅归档外部 AI 原文" }));

      expect(await screen.findByText(/已归档为外部 AI 建议原文；未写入决定、工作进展或下一步/)).toBeInTheDocument();
      expect(registeredExternalFiles).toHaveLength(1);
      expect(invoke).not.toHaveBeenCalledWith("capture_project_fact", expect.anything());
      expect(workLedgerEvents).toHaveLength(0);
    } finally {
      manifest.files = originalFiles;
    }
  });

  it("keeps a captured fact draft when persistence fails", async () => {
    captureProjectFactFailure = true;
    render(
      <MemoryRouter initialEntries={["/work"]}>
        <App />
      </MemoryRouter>,
    );

    await screen.findByRole("button", { name: "项目对话" });
    fireEvent.click(screen.getByText("更多工具"));
    fireEvent.click(screen.getByRole("button", { name: "记录工作" }));

    const content = await screen.findByPlaceholderText("例如：完成 Workspace 整理闭环的人工验收并记录结果。");
    fireEvent.change(content, { target: { value: "保留这条失败时的输入" } });
    fireEvent.click(screen.getByRole("button", { name: "记录完成进展" }));

    expect(await screen.findByText(/工作事实暂时无法写入/)).toBeInTheDocument();
    expect(content).toHaveValue("保留这条失败时的输入");
  });

  it("opens a bounded factual project context packet without technical paths", async () => {
    render(
      <MemoryRouter initialEntries={["/work"]}>
        <App />
      </MemoryRouter>,
    );

    await screen.findByRole("button", { name: "项目对话" });
    fireEvent.click(screen.getByText("更多工具"));
    fireEvent.click(screen.getByRole("button", { name: "项目上下文" }));

    expect(await screen.findByRole("heading", { name: "项目上下文" })).toBeInTheDocument();
    expect(screen.getAllByText("验收 Codex 结果")).toHaveLength(2);
    expect(screen.getByText("验收说明.md")).toBeInTheDocument();
    expect(invoke).toHaveBeenCalledWith("get_current_project_context_packet", { projectRoot: project.rootDir });
  });

  it("copies a fresh current-project context packet directly from More Tools", async () => {
    const originalClipboard = Object.getOwnPropertyDescriptor(navigator, "clipboard");
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText },
    });

    try {
      render(
        <MemoryRouter initialEntries={["/work"]}>
          <App />
        </MemoryRouter>,
      );

      await screen.findByRole("button", { name: "项目对话" });
      fireEvent.click(screen.getByText("更多工具"));
      fireEvent.click(screen.getByRole("button", { name: "复制项目上下文" }));

      await waitFor(() =>
        expect(invoke).toHaveBeenCalledWith("get_current_project_context_packet", { projectRoot: project.rootDir }),
      );
      await waitFor(() => expect(writeText).toHaveBeenCalledWith(projectContextPacket.markdown));
      expect(screen.getByRole("button", { name: "已复制项目上下文" })).toBeInTheDocument();
      expect(screen.queryByRole("heading", { name: "项目上下文" })).not.toBeInTheDocument();
    } finally {
      if (originalClipboard) {
        Object.defineProperty(navigator, "clipboard", originalClipboard);
      } else {
        Reflect.deleteProperty(navigator, "clipboard");
      }
    }
  });

  it("uses the current project root as the Git path default before a snapshot exists", async () => {
    render(
      <MemoryRouter initialEntries={["/work"]}>
        <App />
      </MemoryRouter>,
    );

    await screen.findByRole("button", { name: "项目对话" });
    fireEvent.click(screen.getByRole("button", { name: "Codex 任务" }));

    expect(await screen.findByDisplayValue(project.rootDir)).toBeInTheDocument();
  });

  it("restores the saved Git repository path from the project work ledger", async () => {
    gitSnapshot = {
      id: "git-1",
      projectId: project.id,
      repositoryPath: "D:\\GanMaoYuan\\Website-Clone",
      branch: "master",
      head: "abcdef",
      headShort: "abcdef",
      recentCommits: [],
      isDirty: false,
      changedFiles: [],
      tags: [],
      status: "ok",
      failureReason: "",
      capturedAt: "3",
    };

    render(
      <MemoryRouter initialEntries={["/work"]}>
        <App />
      </MemoryRouter>,
    );

    await screen.findByRole("button", { name: "项目对话" });
    fireEvent.click(screen.getByRole("button", { name: "Codex 任务" }));

    expect(await screen.findByDisplayValue("D:\\GanMaoYuan\\Website-Clone")).toBeInTheDocument();
  });

  it("surfaces imported Codex result facts in the work ledger panel", async () => {
    codexResults = [
      {
        id: "codex-result-1",
        projectId: project.id,
        sourceHash: "hash-1",
        sourceLabel: "Codex 完成报告",
        taskId: "task-1",
        taskType: "coding",
        status: "completed",
        resultText: "修复 Work Ledger",
        completedContent: ["修复 Work Ledger"],
        changedFiles: ["src/pages/WorkPage.tsx"],
        commits: ["abc1234"],
        tests: ["npm test 通过"],
        findings: [],
        recommendations: [],
        questions: [],
        checks: [],
        passed: [],
        failed: [],
        artifacts: [],
        targetFiles: [],
        unresolvedItems: [],
        manualAcceptance: ["需要桌面点击验收"],
        externalThreadId: "",
        resultRunId: "",
        rawEvidencePath: "D:\\GanMaoYuan\\SelfProject\\.ganmaoyuan\\managed\\report.md",
        createdAt: "4",
      },
    ];

    render(
      <MemoryRouter initialEntries={["/work"]}>
        <App />
      </MemoryRouter>,
    );

    await screen.findByRole("button", { name: "项目对话" });
    fireEvent.click(screen.getByRole("button", { name: "Codex 任务" }));

    expect(await screen.findByText("Codex 回流")).toBeInTheDocument();
    expect(await screen.findByText("commit：abc1234")).toBeInTheDocument();
    expect(await screen.findByText("测试：npm test 通过")).toBeInTheDocument();
    expect(await screen.findByText("待处理：需要桌面点击验收")).toBeInTheDocument();
  });

  it("shows result recheck feedback inside the current Codex console", async () => {
    codexConsoleTasks = [{
      taskId: "task-missing-result",
      projectId: project.id,
      title: "恢复缺失结果任务",
      taskType: "coding",
      prompt: "taskId：task-missing-result",
      createdAt: "1",
      handedOffAt: "2",
      resultReceivedAt: "",
      completedAt: "",
      status: "needsReview",
      expectedResultPath: "D:\\project\\.ganmaoyuan\\codex\\results\\task-missing-result-result.json",
      resultId: "",
      resultRunId: "run-missing-result",
      resultSource: "",
      summary: "",
      resultText: "",
      changedFiles: [],
      reportedCommits: [],
      verifiedCommits: [],
      gitVerification: { status: "unavailable", repositoryPath: "", checkedAt: "", reason: "" },
      tests: [],
      findings: [],
      recommendations: [],
      questions: [],
      checks: [],
      passed: [],
      failed: [],
      artifacts: [],
      targetFiles: [],
      remainingIssues: ["应用重启后没有找到匹配结果。"],
      manualAcceptance: [],
    }];
    codexConsoleRuns = [{
      id: "run-missing-result",
      taskId: "task-missing-result",
      projectId: project.id,
      status: "unknownAfterRestart",
      startedAt: "2",
      endedAt: "3",
      pid: 12345,
      processAlive: false,
      exitCode: null,
      error: "应用重启后未检测到原 Codex 进程。",
      recentActivity: [],
    }];

    render(
      <MemoryRouter initialEntries={["/work"]}>
        <App />
      </MemoryRouter>,
    );

    await screen.findByRole("button", { name: "项目对话" });
    fireEvent.click(screen.getByRole("button", { name: "Codex 任务" }));
    fireEvent.click(await screen.findByRole("button", { name: "重新检查结果" }));

    const feedback = await screen.findByText(/结果目录中没有可读取的结果文件/);
    expect(feedback.closest(".codex-console")).not.toBeNull();
    expect(screen.getByText("结果未回流")).toBeInTheDocument();
    expect(invoke).toHaveBeenCalledWith("scan_codex_result_bridge", { projectRoot: project.rootDir });
  });

  it.each([
    ["notInstalled", "没有检测到本机 Codex CLI"],
    ["notLoggedIn", "本机 Codex CLI 尚未登录"],
    ["incompatible", "当前 Codex CLI 缺少非交互能力"],
  ])("checks Codex before creating a Run when readiness is %s", async (status, message) => {
    codexConsoleTasks = [makeReadyCodexTask()];
    const originalInvoke = vi.mocked(invoke).getMockImplementation()!;
    vi.mocked(invoke).mockImplementation(async (command, args) => {
      if (command === "check_codex_cli_readiness") {
        return { status, message, version: "test", checkedAt: "3" };
      }
      return originalInvoke(command, args);
    });

    render(
      <MemoryRouter initialEntries={["/work"]}>
        <App />
      </MemoryRouter>,
    );

    await screen.findByRole("button", { name: "项目对话" });
    fireEvent.click(screen.getByRole("button", { name: "Codex 任务" }));
    fireEvent.click(await screen.findByRole("button", { name: "启动 Codex" }));

    expect(await screen.findByText(message)).toBeInTheDocument();
    expect(invoke).toHaveBeenCalledWith("check_codex_cli_readiness");
    expect(invoke).not.toHaveBeenCalledWith("start_codex_task_run", expect.anything());
  });

  it("starts the existing Codex run flow after a successful read-only preflight", async () => {
    codexConsoleTasks = [makeReadyCodexTask()];
    const originalInvoke = vi.mocked(invoke).getMockImplementation()!;
    vi.mocked(invoke).mockImplementation(async (command, args) => {
      if (command === "check_codex_cli_readiness") {
        return { status: "ready", message: "就绪", version: "codex-cli test", checkedAt: "3" };
      }
      if (command === "start_codex_task_run") {
        return {
          id: "run-preflight",
          taskId: "task-preflight",
          projectId: project.id,
          status: "starting",
          startedAt: "4",
        };
      }
      return originalInvoke(command, args);
    });

    render(
      <MemoryRouter initialEntries={["/work"]}>
        <App />
      </MemoryRouter>,
    );

    await screen.findByRole("button", { name: "项目对话" });
    fireEvent.click(screen.getByRole("button", { name: "Codex 任务" }));
    fireEvent.click(await screen.findByRole("button", { name: "启动 Codex" }));

    await waitFor(() => expect(invoke).toHaveBeenCalledWith("start_codex_task_run", {
      projectRoot: project.rootDir,
      taskId: "task-preflight",
    }));
    const readinessCall = vi.mocked(invoke).mock.calls.findIndex(([command]) => command === "check_codex_cli_readiness");
    const runCall = vi.mocked(invoke).mock.calls.findIndex(([command]) => command === "start_codex_task_run");
    expect(readinessCall).toBeGreaterThanOrEqual(0);
    expect(runCall).toBeGreaterThan(readinessCall);
  });

  async function projectIsolationHarness(withConsole = false) {
    let state!: ReturnType<typeof useAppState>;
    function Probe() {
      state = useAppState();
      const location = useLocation();
      return <><span>{state.activeProject?.id}</span><output data-testid="route-query">{location.search}</output>{withConsole && <WorkPage />}</>;
    }
    const otherProject = { ...project, id: "project-2", rootDir: `${project.rootDir}-other`, name: "Other project" };
    const otherManifest = { ...manifest, project: otherProject };
    const original = vi.mocked(invoke).getMockImplementation()!;
    vi.mocked(invoke).mockImplementation(async (command, args) => {
      if (command === "list_codex_tasks") return { tasks: [], runs: [] };
      if ((args as { projectRoot?: string } | undefined)?.projectRoot === otherProject.rootDir) {
        if (command === "load_project" || command === "scan_project_workspace") return otherManifest;
        if (command === "get_work_ledger") return { events: [], gitSnapshot: null, codexResults: [], codexTasks: [] };
        if (command === "scan_codex_result_bridge") return { importedCount: 0, manifest: otherManifest };
      }
      return original(command, args);
    });
    render(<MemoryRouter initialEntries={["/work?panel=codex"]}><AppProvider><Probe /></AppProvider></MemoryRouter>);
    await waitFor(() => expect(state.activeProject?.id).toBe(project.id));
    await waitFor(() => expect(state.workLedger).not.toBeNull());
    return { getState: () => state, otherProject };
  }

  it("does not apply a delayed project A ledger after opening project B", async () => {
    const { getState, otherProject } = await projectIsolationHarness();
    const delayed = deferred<unknown>();
    const original = vi.mocked(invoke).getMockImplementation()!;
    vi.mocked(invoke).mockImplementation((command, args) =>
      command === "get_work_ledger" && (args as { projectRoot: string }).projectRoot === project.rootDir
        ? delayed.promise : original(command, args));
    let refresh!: Promise<void>;
    act(() => { refresh = getState().refreshWorkLedger(); });
    await act(async () => { await getState().openProject(otherProject.rootDir); });
    await act(async () => {
      delayed.resolve({ events: [], gitSnapshot: null, codexResults: [], codexTasks: [{ taskId: "old-a", projectId: project.id }] });
      await refresh;
    });
    expect(getState().activeProject?.id).toBe(otherProject.id);
    expect(getState().codexTasks).toEqual([]);
  });

  it("does not apply a delayed project A chat reply after opening project B", async () => {
    const { getState, otherProject } = await projectIsolationHarness();
    const delayed = deferred<unknown>();
    const original = vi.mocked(invoke).getMockImplementation()!;
    vi.mocked(invoke).mockImplementation((command, args) =>
      command === "send_project_message" ? delayed.promise : original(command, args),
    );
    let send!: Promise<void>;
    act(() => { send = getState().sendProjectMessage("仅属于项目 A 的讨论"); });

    await act(async () => { await getState().openProject(otherProject.rootDir); });
    await act(async () => {
      delayed.resolve({
        project: { ...project, lastOpenedAt: "99" },
        messages: [{ ...initialMessage, id: "late-project-a-reply", text: "A 的迟到回复" }],
        streamMessageId: "",
      });
      await send;
    });

    expect(getState().activeProject?.id).toBe(otherProject.id);
    expect(getState().activeManifest?.project.id).toBe(otherProject.id);
    expect(getState().activeManifest?.messages.some((message) => message.id === "late-project-a-reply")).toBe(false);
  });

  it("reloads canonical project facts after a successful chat write receipt", async () => {
    const { getState } = await projectIsolationHarness();
    const decisionText = "我正式决定：测试项目变更前必须留存原因。请保存并返回真实记录 ID。";
    const userMessage: WorkspaceMessage = {
      ...initialMessage,
      id: "saved-user-message",
      author: "user",
      kind: "requirement",
      text: decisionText,
      status: "completed",
      source: "user",
    };
    const assistantMessage: WorkspaceMessage = {
      ...initialMessage,
      id: "saved-assistant-message",
      author: "ganmaoyuan",
      kind: "assistant",
      text: "已按你的明确请求完成项目记录：DecisionRecord ID：decision-real-42。",
      status: "completed",
      evidenceItems: [{
        id: "receipt-real-42",
        title: "决定已记录",
        contentType: "operationReceipt",
        basisKind: "fact",
        summary: "DecisionRecord ID：decision-real-42；Work Ledger 事件 ID：event-real-42。",
        managedPath: "",
        sourceRecordId: "decision-real-42",
      }],
    };
    const persistedManifest: ProjectManifest = {
      ...manifest,
      project: { ...project, nextStep: "明天核对测试记录" },
      decisions: [{
        id: "decision-real-42",
        summary: "测试项目变更前必须留存原因",
        sourceMessageId: userMessage.id,
        createdAt: "3",
      }],
      messages: [...manifest.messages, userMessage, assistantMessage],
    };
    const savedEvent = {
      id: "event-real-42",
      projectId: project.id,
      sourceType: "user",
      sourceRef: "capture-decision",
      eventType: "user.decisionRecorded",
      factKind: "userConfirmed",
      occurredAt: "3",
      summary: "用户确认决定：测试项目变更前必须留存原因",
      evidenceRefs: [`message:${userMessage.id}`, "decision:decision-real-42"],
      confidence: { score: 100, level: "high", display: "高 100%" },
      decisionTraceId: "trace-real-42",
      decisionTrace: {} as WorkEvent["decisionTrace"],
      createdAt: "3",
    } satisfies WorkEvent;
    const original = vi.mocked(invoke).getMockImplementation()!;
    vi.mocked(invoke).mockImplementation(async (command, args) => {
      if (command === "send_project_message") {
        return { project, messages: [userMessage, assistantMessage], streamMessageId: "" };
      }
      if (command === "load_project" && (args as { projectRoot: string }).projectRoot === project.rootDir) {
        return persistedManifest;
      }
      if (command === "get_work_ledger" && (args as { projectRoot: string }).projectRoot === project.rootDir) {
        return { events: [savedEvent], gitSnapshot: null, codexResults: [], codexTasks: [] };
      }
      return original(command, args);
    });

    await act(async () => { await getState().sendProjectMessage(decisionText); });

    expect(getState().activeManifest?.decisions[0]?.id).toBe("decision-real-42");
    expect(getState().activeManifest?.project.nextStep).toBe("明天核对测试记录");
    expect(getState().workLedger?.events[0]?.id).toBe("event-real-42");
    expect(invoke).toHaveBeenCalledWith("load_project", { projectRoot: project.rootDir });
  });

  it("does not let an older Today snapshot overwrite a newer response", async () => {
    const { getState } = await projectIsolationHarness();
    const stale = deferred<unknown>();
    const original = vi.mocked(invoke).getMockImplementation()!;
    const baseline = getState().todayWorkspace;
    expect(baseline).not.toBeNull();
    let todayCalls = 0;
    vi.mocked(invoke).mockImplementation((command, args) => {
      if (command === "get_today_workspace") {
        todayCalls += 1;
        return todayCalls === 1
          ? stale.promise
          : Promise.resolve({ ...baseline, generatedAt: `newer-${todayCalls}` });
      }
      return original(command, args);
    });

    let oldRefresh!: Promise<void>;
    act(() => { oldRefresh = getState().refreshTodayWorkspace(); });
    await act(async () => { await getState().refreshTodayWorkspace(); });
    const latestGeneratedAt = getState().todayWorkspace?.generatedAt;
    expect(latestGeneratedAt).toBe("newer-2");

    await act(async () => {
      stale.resolve({ ...baseline, generatedAt: "stale-response" });
      await oldRefresh;
    });
    expect(getState().todayWorkspace?.generatedAt).toBe(latestGeneratedAt);
  });

  it("opens a project before its work ledger finishes loading", async () => {
    const { getState, otherProject } = await projectIsolationHarness();
    const delayed = deferred<unknown>();
    const original = vi.mocked(invoke).getMockImplementation()!;
    vi.mocked(invoke).mockImplementation((command, args) =>
      command === "get_work_ledger" && (args as { projectRoot: string }).projectRoot === otherProject.rootDir
        ? delayed.promise : original(command, args));

    await act(async () => { await getState().openProject(otherProject.rootDir); });

    expect(getState().activeProject?.id).toBe(otherProject.id);
    expect(getState().workLedger).toBeNull();
    await act(async () => {
      delayed.resolve({ events: [], gitSnapshot: null, codexResults: [], codexTasks: [{ taskId: "task-b", projectId: otherProject.id }] });
    });
    await waitFor(() => expect(getState().codexTasks.map((task) => task.taskId)).toEqual(["task-b"]));
  });

  it("keeps the latest opened project when an earlier load finishes late", async () => {
    const { getState, otherProject } = await projectIsolationHarness();
    const delayed = deferred<ProjectManifest>();
    const original = vi.mocked(invoke).getMockImplementation()!;
    vi.mocked(invoke).mockImplementation((command, args) =>
      command === "load_project" && (args as { projectRoot: string }).projectRoot === project.rootDir
        ? delayed.promise : original(command, args));
    let first!: Promise<void>;
    act(() => { first = getState().openProject(project.rootDir); });
    await act(async () => { await getState().openProject(otherProject.rootDir); });
    await act(async () => { delayed.resolve(manifest); await first; });
    expect(getState().activeProject?.id).toBe(otherProject.id);
    expect(getState().activeManifest?.project.id).toBe(otherProject.id);
  });

  it("does not reopen A when its generated task returns after switching to B", async () => {
    const { getState, otherProject } = await projectIsolationHarness();
    const delayed = deferred<unknown>();
    const original = vi.mocked(invoke).getMockImplementation()!;
    vi.mocked(invoke).mockImplementation((command, args) =>
      command === "generate_codex_prompt" ? delayed.promise : original(command, args));
    let generation!: ReturnType<ReturnType<typeof useAppState>["generateCodexPrompt"]>;
    act(() => { generation = getState().generateCodexPrompt(); });
    await act(async () => { await getState().openProject(otherProject.rootDir); });
    await act(async () => {
      delayed.resolve({ manifest, prompt: { id: "task-a", promptText: "project A only" } });
      await generation;
    });
    expect(getState().activeProject?.id).toBe(otherProject.id);
    expect(getState().activeManifest?.project.id).toBe(otherProject.id);
  });

  it("does not replace a newer ledger with an older response in the same project", async () => {
    const { getState } = await projectIsolationHarness();
    const delayed = deferred<unknown>();
    const original = vi.mocked(invoke).getMockImplementation()!;
    let ledgerCalls = 0;
    vi.mocked(invoke).mockImplementation((command, args) => {
      if (command !== "get_work_ledger") return original(command, args);
      ledgerCalls += 1;
      return ledgerCalls === 1 ? delayed.promise : Promise.resolve({ events: [], gitSnapshot: null, codexResults: [], codexTasks: [{ taskId: "new", projectId: project.id }] });
    });
    let first!: Promise<void>;
    act(() => { first = getState().refreshWorkLedger(); });
    await act(async () => { await getState().refreshWorkLedger(); });
    await act(async () => {
      delayed.resolve({ events: [], gitSnapshot: null, codexResults: [], codexTasks: [] });
      await first;
    });
    expect(getState().codexTasks.map((task) => task.taskId)).toEqual(["new"]);
  });

  it("rejects an old A response even after switching A -> B -> A", async () => {
    const { getState, otherProject } = await projectIsolationHarness();
    const delayed = deferred<unknown>();
    const original = vi.mocked(invoke).getMockImplementation()!;
    let intercepted = false;
    vi.mocked(invoke).mockImplementation((command, args) => {
      if (command === "get_work_ledger" && !intercepted) {
        intercepted = true;
        return delayed.promise;
      }
      return original(command, args);
    });
    let first!: Promise<void>;
    act(() => { first = getState().refreshWorkLedger(); });
    await act(async () => { await getState().openProject(otherProject.rootDir); });
    await act(async () => { await getState().openProject(project.rootDir); });
    await act(async () => {
      delayed.resolve({ events: [], codexTasks: [{ taskId: "stale-a", projectId: project.id }] });
      await first;
    });
    expect(getState().activeProject?.id).toBe(project.id);
    expect(getState().codexTasks).toEqual([]);
  });

  it("opens project files without stale tasks when its ledger cannot be read", async () => {
    const { getState, otherProject } = await projectIsolationHarness();
    const original = vi.mocked(invoke).getMockImplementation()!;
    vi.mocked(invoke).mockImplementation((command, args) =>
      command === "get_work_ledger" && (args as { projectRoot: string }).projectRoot === otherProject.rootDir
        ? Promise.reject(new Error("Ledger read failed")) : original(command, args));
    await act(async () => { await getState().openProject(otherProject.rootDir); });
    expect(getState().activeManifest?.project.id).toBe(otherProject.id);
    expect(getState().workLedger).toBeNull();
    expect(getState().codexTasks).toEqual([]);
    await waitFor(() => expect(getState().error).toContain("Ledger read failed"));
  });

  it.each([false, true])("ignores an unmounted console's explicit task creation response (failure=%s)", async (failure) => {
    const { getState, otherProject } = await projectIsolationHarness(true);
    const delayed = deferred<unknown>();
    const original = vi.mocked(invoke).getMockImplementation()!;
    vi.mocked(invoke).mockImplementation((command, args) =>
      command === "create_codex_task" ? delayed.promise : original(command, args));
    fireEvent.click(screen.getByRole("button", { name: "生成新任务" }));
    fireEvent.change(screen.getByLabelText("任务标题"), { target: { value: "任务 A" } });
    fireEvent.change(screen.getByLabelText("任务说明"), { target: { value: "只做 A。" } });
    fireEvent.click(screen.getByRole("button", { name: "创建任务" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("create_codex_task", {
      projectRoot: project.rootDir,
      request: { title: "任务 A", taskType: "analysis", instructions: "只做 A。" },
    }));
    await act(async () => { await getState().openProject(otherProject.rootDir); });
    await act(async () => {
      if (failure) delayed.reject(new Error("Old A generation failed"));
      else delayed.resolve({ taskId: "task-a", projectId: project.id, title: "任务 A", prompt: "project A only" });
    });
    expect(getState().activeProject?.id).toBe(otherProject.id);
    expect(screen.getByTestId("route-query")).not.toHaveTextContent("task-a");
    expect(getState().error).not.toContain("Old A");
    expect(screen.getByRole("button", { name: "生成新任务" })).toBeEnabled();
  });

  it("does not refresh A's ledger when its run list returns after leaving A", async () => {
    const { getState, otherProject } = await projectIsolationHarness(true);
    const delayed = deferred<unknown>();
    const original = vi.mocked(invoke).getMockImplementation()!;
    vi.mocked(invoke).mockImplementation((command, args) =>
      command === "list_codex_tasks" && (args as { projectRoot: string }).projectRoot === project.rootDir
        ? delayed.promise : original(command, args));
    // Reopen A to mount a fresh console and issue its initial run-list request.
    await act(async () => { await getState().openProject(otherProject.rootDir); });
    await act(async () => { await getState().openProject(project.rootDir); });
    await act(async () => { await getState().openProject(otherProject.rootDir); });
    const count = vi.mocked(invoke).mock.calls.filter(([command, args]) => command === "get_work_ledger" && (args as { projectRoot: string }).projectRoot === project.rootDir).length;
    await act(async () => { delayed.resolve({ tasks: [], runs: [{ taskId: "task-a", status: "running" }] }); });
    expect(vi.mocked(invoke).mock.calls.filter(([command, args]) => command === "get_work_ledger" && (args as { projectRoot: string }).projectRoot === project.rootDir)).toHaveLength(count);
    expect(getState().activeProject?.id).toBe(otherProject.id);
  });

  it("ignores the bridge's second awaited response after switching projects", async () => {
    const { getState, otherProject } = await projectIsolationHarness();
    await act(async () => { await getState().openProject(otherProject.rootDir); });
    const delayed = deferred<unknown>();
    const original = vi.mocked(invoke).getMockImplementation()!;
    let bridgeReceived = false;
    let ledgerWaiting = false;
    vi.mocked(invoke).mockImplementation((command, args) => {
      if ((args as { projectRoot?: string } | undefined)?.projectRoot === project.rootDir) {
        if (command === "scan_codex_result_bridge") {
          bridgeReceived = true;
          return Promise.resolve({ importedCount: 1, manifest });
        }
        if (command === "get_work_ledger" && bridgeReceived) {
          ledgerWaiting = true;
          return delayed.promise;
        }
      }
      return original(command, args);
    });
    await act(async () => { await getState().openProject(project.rootDir); });
    await waitFor(() => expect(ledgerWaiting).toBe(true));
    await act(async () => { await getState().openProject(otherProject.rootDir); });
    await act(async () => { delayed.resolve({ events: [], codexTasks: [{ taskId: "bridge-a", projectId: project.id }] }); });
    expect(getState().activeManifest?.project.id).toBe(otherProject.id);
    expect(getState().codexTasks).toEqual([]);
  });

  it("keeps acceptance attached to A without publishing its result in B", async () => {
    const { getState, otherProject } = await projectIsolationHarness();
    const delayed = deferred<unknown>();
    const original = vi.mocked(invoke).getMockImplementation()!;
    vi.mocked(invoke).mockImplementation((command, args) =>
      command === "accept_codex_task" ? delayed.promise : original(command, args));
    let acceptance!: ReturnType<ReturnType<typeof useAppState>["acceptCodexTask"]>;
    act(() => { acceptance = getState().acceptCodexTask("task-a", "result-a"); });
    await act(async () => { await getState().openProject(otherProject.rootDir); });
    await act(async () => {
      delayed.resolve({ taskId: "task-a", projectId: project.id, status: "completed" });
      await acceptance;
    });
    expect(invoke).toHaveBeenCalledWith("accept_codex_task", {
      projectRoot: project.rootDir, taskId: "task-a", expectedResultId: "result-a",
    });
    expect(getState().activeProject?.id).toBe(otherProject.id);
    expect(getState().codexTasks).toEqual([]);
  });

  it("keeps the previous project usable after a new project load fails", async () => {
    const { getState, otherProject } = await projectIsolationHarness();
    const original = vi.mocked(invoke).getMockImplementation()!;
    vi.mocked(invoke).mockImplementation((command, args) => {
      if (command === "load_project" && (args as { projectRoot: string }).projectRoot === otherProject.rootDir) {
        return Promise.reject(new Error("Project unavailable"));
      }
      if (command === "get_work_ledger") return Promise.resolve({ events: [], codexTasks: [{ taskId: "active-a", projectId: project.id }] });
      return original(command, args);
    });
    await act(async () => { await expect(getState().openProject(otherProject.rootDir)).rejects.toThrow("Project unavailable"); });
    await act(async () => { await getState().refreshWorkLedger(); });
    expect(getState().activeProject?.id).toBe(project.id);
    expect(getState().codexTasks.map((task) => task.taskId)).toEqual(["active-a"]);
  });

  it("does not carry the previous project's tasks into a newly created project", async () => {
    const { getState, otherProject } = await projectIsolationHarness();
    const original = vi.mocked(invoke).getMockImplementation()!;
    vi.mocked(invoke).mockImplementation((command, args) => {
      if (command === "get_work_ledger") return Promise.resolve({ events: [], codexTasks: [{ taskId: "active-a", projectId: project.id }] });
      if (command === "create_project") return Promise.resolve({ project: otherProject });
      return original(command, args);
    });
    await act(async () => { await getState().refreshWorkLedger(); });
    expect(getState().codexTasks).toHaveLength(1);
    await act(async () => { await getState().createProject(otherProject.name, otherProject.rootDir, [], "Isolated project"); });
    expect(getState().activeProject?.id).toBe(otherProject.id);
    expect(getState().workLedger).toBeNull();
    expect(getState().codexTasks).toEqual([]);
  });

  it("does not navigate to A's explicit task while project B is still loading", async () => {
    const { getState, otherProject } = await projectIsolationHarness(true);
    const generated = deferred<unknown>();
    const loading = deferred<ProjectManifest>();
    const original = vi.mocked(invoke).getMockImplementation()!;
    vi.mocked(invoke).mockImplementation((command, args) => {
      if (command === "create_codex_task") return generated.promise;
      if (command === "load_project" && (args as { projectRoot: string }).projectRoot === otherProject.rootDir) return loading.promise;
      return original(command, args);
    });
    fireEvent.click(screen.getByRole("button", { name: "生成新任务" }));
    fireEvent.change(screen.getByLabelText("任务标题"), { target: { value: "任务 A" } });
    fireEvent.change(screen.getByLabelText("任务说明"), { target: { value: "只做 A。" } });
    fireEvent.click(screen.getByRole("button", { name: "创建任务" }));
    let opening!: Promise<void>;
    act(() => { opening = getState().openProject(otherProject.rootDir); });
    await act(async () => { generated.resolve({ taskId: "task-a", projectId: project.id, title: "任务 A", prompt: "project A only" }); });
    const queryDuringLoad = screen.getByTestId("route-query").textContent;
    await act(async () => { loading.resolve({ ...manifest, project: otherProject }); await opening; });
    expect(queryDuringLoad).not.toContain("task-a");
    expect(screen.getByTestId("route-query")).not.toHaveTextContent("task-a");
    expect(getState().activeProject?.id).toBe(otherProject.id);
  });
});
