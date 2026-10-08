import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter, useLocation } from "react-router-dom";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import App from "./App";
import { AppProvider, useAppState } from "./AppState";
import { WorkPage } from "../pages/WorkPage";
import type {
  CodexExternalResult,
  GitSnapshot,
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

let workLedgerEvents: WorkEvent[] = [];
let gitSnapshot: GitSnapshot | null = null;
let codexResults: CodexExternalResult[] = [];
let captureProjectFactFailure = false;

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
  gitFacts: null,
  risks: [],
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

describe("project workspace persistence flow", () => {
  afterEach(() => {
    cleanup();
  });

  beforeEach(() => {
    workLedgerEvents = [];
    gitSnapshot = null;
    codexResults = [];
    captureProjectFactFailure = false;
    Object.defineProperty(window, "__TAURI_INTERNALS__", {
      configurable: true,
      value: {},
    });
    window.localStorage.setItem(
      "ganmaoyuan-ui-state-v0.4",
      JSON.stringify({ lastProjectRoot: project.rootDir }),
    );
    vi.mocked(invoke).mockImplementation(async (command, args) => {
      if (command === "list_projects") return [project];
      if (command === "list_memos") return [];
      if (command === "list_inbox_entries") return [];
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
        return { events: workLedgerEvents, gitSnapshot, codexResults, codexTasks: [] };
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
          request: { captureType: string; content: string; reason?: string; actionSourceRef?: string };
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
          evidenceRefs: ["userCapture:test"],
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
    expect((await screen.findAllByText("项目记录")).length).toBeGreaterThan(0);
    expect(invoke).toHaveBeenCalledWith("send_project_message", {
      projectRoot: project.rootDir,
      text: "今天检查事务导入",
    });
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

      fireEvent.click(await screen.findByRole("button", { name: "交给 Codex" }));

      expect(await screen.findByLabelText("任务标题")).toHaveValue("优化设置后台右侧内容区留白");
      expect(screen.getByLabelText("任务类型")).toHaveValue("coding");
      const instructions = (screen.getByLabelText("任务说明") as HTMLTextAreaElement).value;
      expect(instructions).toContain("增加外层内容区 padding");
      expect(instructions).toContain("settings.png");

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

  it("shows the evidence-backed Today Workspace on startup", async () => {
    render(
      <MemoryRouter initialEntries={["/"]}>
        <App />
      </MemoryRouter>,
    );

    expect(await screen.findByText("今天应该做什么")).toBeInTheDocument();
    expect(await screen.findByText(/上次做到：已完成资料整理/)).toBeInTheDocument();
    expect(await screen.findByText("最近进入的项目")).toBeInTheDocument();
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
    expect(screen.queryByText("优先继续")).not.toBeInTheDocument();
    expect(screen.queryByText("Codex 待验收")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "本周复盘" })).not.toBeInTheDocument();
  });

  it("keeps secondary Today details collapsed until requested", async () => {
    render(
      <MemoryRouter initialEntries={["/"]}>
        <App />
      </MemoryRouter>,
    );

    const disclosure = (await screen.findByText("更多动态")).closest("details");
    expect(disclosure).not.toHaveAttribute("open");
    expect(screen.getByText("最近进入的项目")).toBeInTheDocument();

    fireEvent.click(screen.getByText("更多动态"));
    expect(disclosure).toHaveAttribute("open");
  });

  it("opens the project picker from Start Work even when a continue focus exists", async () => {
    render(
      <MemoryRouter initialEntries={["/"]}>
        <App />
      </MemoryRouter>,
    );

    fireEvent.click(await screen.findByRole("button", { name: "开始工作" }));

    expect(await screen.findByText("继续历史任务")).toBeInTheDocument();
    expect(screen.getByRole("combobox")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "继续" })).toBeDisabled();
  });

  it("keeps low-frequency work tools behind a More disclosure", async () => {
    render(
      <MemoryRouter initialEntries={["/work"]}>
        <App />
      </MemoryRouter>,
    );

    await screen.findByText("工作流");
    expect(screen.getByText("更多")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "本周复盘" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "项目影响与行动" })).toBeInTheDocument();
  });

  it("keeps implementation names out of the ordinary work surface", async () => {
    render(
      <MemoryRouter initialEntries={["/work"]}>
        <App />
      </MemoryRouter>,
    );

    await screen.findByText("工作流");

    expect(screen.getAllByRole("button", { name: "启用 AI 回复" }).length).toBeGreaterThan(0);
    expect(screen.queryByText(/DeepSeek/i)).not.toBeInTheDocument();
    expect(screen.queryByText(/Atlas 与监视/i)).not.toBeInTheDocument();
  });

  it("records a user decision and refreshes the work ledger immediately", async () => {
    render(
      <MemoryRouter initialEntries={["/work"]}>
        <App />
      </MemoryRouter>,
    );

    await screen.findByText("工作流");
    fireEvent.click(screen.getAllByRole("button", { name: "交给 Codex" })[0]);

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

    await screen.findByText("工作流");
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

  it("keeps a captured fact draft when persistence fails", async () => {
    captureProjectFactFailure = true;
    render(
      <MemoryRouter initialEntries={["/work"]}>
        <App />
      </MemoryRouter>,
    );

    await screen.findByText("工作流");
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

    await screen.findByText("工作流");
    fireEvent.click(screen.getByRole("button", { name: "项目上下文" }));

    expect(await screen.findByRole("heading", { name: "项目上下文" })).toBeInTheDocument();
    expect(screen.getAllByText("验收 Codex 结果")).toHaveLength(2);
    expect(screen.getByText("验收说明.md")).toBeInTheDocument();
    expect(invoke).toHaveBeenCalledWith("get_current_project_context_packet", { projectRoot: project.rootDir });
  });

  it("uses the current project root as the Git path default before a snapshot exists", async () => {
    render(
      <MemoryRouter initialEntries={["/work"]}>
        <App />
      </MemoryRouter>,
    );

    await screen.findByText("工作流");
    fireEvent.click(screen.getAllByRole("button", { name: "交给 Codex" })[0]);

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

    await screen.findByText("工作流");
    fireEvent.click(screen.getAllByRole("button", { name: "交给 Codex" })[0]);

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

    await screen.findByText("工作流");
    fireEvent.click(screen.getAllByRole("button", { name: "交给 Codex" })[0]);

    expect(await screen.findByText("Codex 回流")).toBeInTheDocument();
    expect(await screen.findByText("commit：abc1234")).toBeInTheDocument();
    expect(await screen.findByText("测试：npm test 通过")).toBeInTheDocument();
    expect(await screen.findByText("待处理：需要桌面点击验收")).toBeInTheDocument();
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
