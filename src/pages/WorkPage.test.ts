import { act, renderHook } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { useState } from "react";
import {
  buildCodexTaskSearchParams,
  buildCodexDraftFromDiscussion,
  buildCodexTaskView,
  buildCodexResultCheckFeedback,
  codexDisplayTaskTitle,
  codexHistoryGitSummary,
  codexReportTaskId,
  codexShortCommit,
  buildCodexRunObservability,
  buildWorkCloseoutDraft,
  codexRunsEqual,
  codexActivityStatus,
  formatCodexElapsed,
  formatCodexLastActivity,
  formatCodexTaskDate,
  codexTaskTimeSummary,
  sortCodexTasksByRecency,
  codexTaskResultGroups,
  codexTaskSteps,
  codexUserResultSummary,
  latestWorkspaceItems,
  resolveConversationScrollTop,
  mergeCodexRuns,
  resolveCodexTaskDisplayType,
  resolveCodexPromptText,
  selectCurrentCodexTask,
  terminalRunSignatures,
  terminalRunStateChanged,
  useConversationMessages,
  visibleReferenceFiles,
  workspaceActivityLabel,
  workspaceMessageDisplayLabel,
  shouldShowChatThinkingIndicator,
  sanitizeWorkspacePublicText,
  projectAnalysisStatusLabel,
} from "./WorkPage";
import type { CodexRun, CodexTask, RecoveryPoint, WorkEvent, WorkspaceMessage } from "../features/project/desktopApi";

describe("chat reply thinking indicator", () => {
  const base = {
    requestPending: false,
    streamActive: false,
    stopRequested: false,
    nonConversationMode: false,
  };

  it("stays visible only while the real request is waiting for the first answer text", () => {
    expect(shouldShowChatThinkingIndicator({ ...base, requestPending: true })).toBe(true);
    expect(shouldShowChatThinkingIndicator({ ...base, streamActive: true, assistantStatus: "streaming", assistantText: "" })).toBe(true);
    expect(shouldShowChatThinkingIndicator({ ...base, streamActive: true, assistantStatus: "streaming", assistantText: "第一段正文" })).toBe(false);
    expect(shouldShowChatThinkingIndicator({ ...base, requestPending: true, assistantStatus: "completed", assistantText: "完整回答" })).toBe(false);
  });

  it("clears for terminal, stopped, and non-chat states", () => {
    for (const assistantStatus of ["completed", "error", "failed", "cancelled", "stopped"]) {
      expect(shouldShowChatThinkingIndicator({ ...base, requestPending: true, assistantStatus })).toBe(false);
    }
    expect(shouldShowChatThinkingIndicator({ ...base, requestPending: true, stopRequested: true })).toBe(false);
    expect(shouldShowChatThinkingIndicator({ ...base, requestPending: true, nonConversationMode: true })).toBe(false);
    expect(shouldShowChatThinkingIndicator(base)).toBe(false);
  });
});

describe("evidence-backed work closeout draft", () => {
  function event(overrides: Partial<WorkEvent>): WorkEvent {
    return {
      id: "event-default",
      projectId: "project-a",
      sourceType: "user",
      sourceRef: "source-default",
      eventType: "user.progressRecorded",
      factKind: "userConfirmed",
      occurredAt: "1790000000000",
      summary: "用户记录进展：默认事项",
      evidenceRefs: ["message:source-default"],
      confidence: { score: 100, level: "high", display: "高" },
      decisionTraceId: "",
      decisionTrace: {
        id: "", type: "", subjectId: "", projectId: "", inputEvidence: [],
        aiUnderstanding: "", recommendation: "",
        confidence: { score: 100, level: "high", display: "高" },
        userDecision: "pending", userDecisionNote: "", execution: "pending",
        executionNote: "", createdAt: "", updatedAt: "",
      },
      createdAt: "1790000000000",
      ...overrides,
    };
  }

  const latestRecovery: RecoveryPoint = {
    id: "recovery-2",
    projectId: "project-a",
    completed: "上次已完成的内容",
    nextStep: "旧下一步",
    createdAt: "1790000000000",
    messageCount: 4,
    fileCount: 1,
  };

  it("prefills only current-project confirmed progress and accepted engineering evidence", () => {
    const events = [
      event({ id: "old-progress", summary: "不应重复：上次收工前的工作" }),
      event({
        id: "recovery-marker",
        sourceRef: "recoveryPoint:recovery-2",
        summary: "用户记录完成：上次已完成的内容",
        evidenceRefs: ["recoveryPoint:recovery-2"],
      }),
      event({ id: "progress-now", summary: "用户记录进展：完成导入和正文解析" }),
      event({
        id: "accepted-codex",
        sourceType: "codex",
        eventType: "codex.taskAccepted",
        factKind: "fact",
        summary: "Codex 任务人工验收通过：修复资料检索",
        evidenceRefs: ["codexTask:task-1", "codexReport:result-1"],
      }),
      event({
        id: "git-commit",
        sourceType: "git",
        eventType: "git.headChanged",
        factKind: "fact",
        summary: "Git HEAD 更新到 abc123：fix: verify document search",
        evidenceRefs: ["commit:abc123"],
      }),
      event({
        id: "ai-suggestion",
        sourceType: "system",
        eventType: "user.progressRecorded",
        factKind: "suggestion",
        summary: "AI 建议：完成检索优化",
      }),
      event({
        id: "scan-reminder",
        sourceType: "scanner",
        eventType: "project.directoryScanned",
        factKind: "fact",
        summary: "扫描发现待处理文件",
      }),
      event({
        id: "foreign-progress",
        projectId: "project-b",
        summary: "WTS 的工作不能进入当前项目",
      }),
    ];

    const draft = buildWorkCloseoutDraft(events, "project-a", [latestRecovery], "用户确认的当前下一步");
    expect(draft.completed).toContain("完成导入和正文解析");
    expect(draft.completed).toContain("修复资料检索");
    expect(draft.completed).toContain("abc123");
    expect(draft.completed).not.toContain("上次收工前");
    expect(draft.completed).not.toContain("AI 建议");
    expect(draft.completed).not.toContain("待处理文件");
    expect(draft.completed).not.toContain("WTS");
    expect(draft.nextStep).toBe("用户确认的当前下一步");
    expect(draft.evidenceCount).toBe(3);
  });

  it("does not replay old events if the latest recovery point has no ledger boundary", () => {
    const draft = buildWorkCloseoutDraft(
      [event({ id: "old", summary: "旧的真实进展" })],
      "project-a",
      [latestRecovery],
      "",
    );
    expect(draft.completed).toBe("");
    expect(draft.evidenceCount).toBe(0);
  });

  it("keeps chat and AI suggestions out when no confirmed work evidence exists", () => {
    const draft = buildWorkCloseoutDraft(
      [event({ sourceType: "system", factKind: "inference", eventType: "chat.reply", summary: "AI 讨论结论" })],
      "project-a",
      [],
      "",
    );
    expect(draft.completed).toBe("");
    expect(draft.nextStep).toBe("");
    expect(draft.evidenceCount).toBe(0);
  });
});

function codexTask(overrides: Partial<CodexTask>): CodexTask {
  return {
    taskId: "task-a",
    projectId: "project-a",
    title: "整理项目理解",
    taskType: "analysis",
    prompt: "taskId：task-a",
    createdAt: "2026-08-26T00:00:00Z",
    handedOffAt: "",
    resultReceivedAt: "",
    completedAt: "",
    status: "ready",
    repositoryPath: "",
    expectedResultPath: "task-a-result.json",
    resultId: "",
    resultSource: "",
    resultRunId: "",
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
    acceptance: { status: "pending", decidedAt: "", reason: "" },
    evidenceRefs: [],
    updatedAt: "2026-08-26T00:00:00Z",
    ...overrides,
  };
}

describe("Codex task prompt binding", () => {
  it("shows the current task prompt instead of a stale archived prompt", () => {
    expect(
      resolveCodexPromptText(
        { prompt: "taskId：task-b\nresult: task-b-result.json" },
        [{ promptText: "taskId：task-a\nresult: task-a-result.json" }],
      ),
    ).toContain("taskId：task-b");
  });

  it("falls back to the latest archived prompt only when there is no task", () => {
    expect(resolveCodexPromptText(null, [{ promptText: "taskId：task-a" }])).toBe("taskId：task-a");
  });
});

describe("conversation scroll restoration", () => {
  it("returns to the latest message when the conversation was following the bottom", () => {
    expect(resolveConversationScrollTop({
      scrollHeight: 2400,
      clientHeight: 800,
      previousScrollTop: 0,
      followBottom: true,
    })).toBe(1600);
  });

  it("preserves a deliberate reading position and clamps it after content changes", () => {
    expect(resolveConversationScrollTop({
      scrollHeight: 2400,
      clientHeight: 800,
      previousScrollTop: 720,
      followBottom: false,
    })).toBe(720);
    expect(resolveConversationScrollTop({
      scrollHeight: 900,
      clientHeight: 800,
      previousScrollTop: 720,
      followBottom: false,
    })).toBe(100);
  });

  it("reuses the conversation projection across unrelated renders", () => {
    const messages: WorkspaceMessage[] = [{
      id: "user-message",
      author: "user",
      kind: "requirement",
      text: "保留最新对话位置。",
      createdAt: "2026-10-13T00:00:00Z",
      status: "completed",
      attachments: [],
      relatedTask: "",
      source: "user",
      modelId: "",
      evidenceItems: [],
    }];

    const { result } = renderHook(() => {
      const [, setUnrelatedState] = useState(0);
      return {
        messages: useConversationMessages(messages, "项目说明"),
        rerenderUnrelated: () => setUnrelatedState((value) => value + 1),
      };
    });
    const firstProjection = result.current.messages;

    act(() => result.current.rerenderUnrelated());

    expect(result.current.messages).toBe(firstProjection);
  });
});

describe("Codex run polling stability", () => {
  it("recognizes unchanged run snapshots", () => {
    const run = { id: "run-a", taskId: "task-a", status: "running" } as unknown as CodexRun;
    expect(codexRunsEqual([run], [{ ...run }])).toBe(true);
  });

  it("keeps real activity and lifecycle changes observable", () => {
    const run = { id: "run-a", taskId: "task-a", status: "running", recentActivity: ["starting"] } as unknown as CodexRun;
    expect(codexRunsEqual([run], [{ ...run, recentActivity: ["running tests"] }])).toBe(false);
    expect(codexRunsEqual([run], [{ ...run, status: "exited" }])).toBe(false);
  });

  it("checks the visible raw-output tail without serializing the whole log", () => {
    const run = {
      id: "run-a",
      taskId: "task-a",
      status: "running",
      rawActivity: Array.from({ length: 5000 }, (_, index) => `line ${index}`),
    } as unknown as CodexRun;
    const unchanged = { ...run, rawActivity: [...run.rawActivity!] };
    const updated = { ...run, rawActivity: [...run.rawActivity!.slice(0, -1), "running tests"] };

    expect(codexRunsEqual([run], [unchanged])).toBe(true);
    expect(codexRunsEqual([run], [updated])).toBe(false);
  });
});

describe("Codex execution observability", () => {
  const run = (overrides: Partial<CodexRun> = {}): CodexRun => ({
    id: "run-a",
    taskId: "task-a",
    projectId: "project-a",
    repositoryPath: "D:/repo",
    commandExecutable: "codex.exe",
    commandArgs: [],
    capabilityProbe: {
      available: true,
      version: "",
      supportsCd: true,
      supportsAddDir: true,
      supportsJson: true,
      supportsOutputLastMessage: true,
      supportsSandbox: true,
      supportsStdin: true,
      checkedAt: "",
      failureReason: "",
    },
    workingDirectory: "D:/repo",
    stdinPromptSummary: "",
    status: "running",
    startedAt: "1000000",
    endedAt: "",
    pid: 123,
    processAlive: true,
    exitCode: null,
    outputLastMessagePath: "",
    stdoutSummary: "",
    stderrSummary: "",
    lastActivityAt: "1005000",
    recentActivity: ["stdout：运行测试"],
    error: "",
    ...overrides,
  });

  it("shows elapsed time, live process state, and the latest real activity", () => {
    expect(formatCodexElapsed("1000000", 1000000 + 1000 * 60 * 6 + 1000 * 18)).toBe("6分18秒");
    expect(formatCodexLastActivity("1005000", 1000000 + 1000 * 60 * 6 + 1000 * 18)).toBe("6分13秒前");
    expect(buildCodexRunObservability(run(), 1000000 + 1000 * 60 * 6 + 1000 * 18)).toMatchObject({
      headline: "Codex 正在执行",
      processText: "进程正常",
      latestActivity: "运行测试",
      quiet: true,
    });
  });

  it("maps real event names without claiming task completion or invented progress", () => {
    expect(codexActivityStatus("stdout: turn.started")).toBe("Codex 已开始处理任务");
    expect(codexActivityStatus("stdout：item.started")).toBe("Codex 已开始一项操作");
    expect(codexActivityStatus("stdout：item.completed")).toBe("Codex 已结束一项操作");
    expect(codexActivityStatus("stdout：turn.completed")).toBe("Codex 本轮处理已结束");
    expect(codexActivityStatus("stderr: error: unavailable")).toBe("Codex 报告了执行错误");
    expect(codexActivityStatus("stdout: future.event")).toBe("Codex 收到新的执行活动");
    const raw = ["stdout：item.started", "stdout：item.updated"];
    expect(buildCodexRunObservability(run({ recentActivity: raw })).recentActivity).toEqual(raw);
    const rawLine = 'stdout：{"type":"turn.started"}';
    expect(buildCodexRunObservability(run({ rawActivity: [rawLine] })).rawActivity).toEqual([rawLine]);
    expect(buildCodexRunObservability(run({ recentActivity: [] })).latestActivity).toBe("等待 Codex 输出");
  });

  it("does not claim a live process after the backend reports it is gone", () => {
    expect(buildCodexRunObservability(run({ processAlive: false }), 1000000 + 1000 * 60 * 2)).toMatchObject({
      processText: "进程状态待确认",
      quiet: false,
    });
  });
});

describe("Codex task draft from current discussion", () => {
  it("prefills a coding task from the latest user request, AI plan, and screenshot reference", () => {
    const messages: WorkspaceMessage[] = [
      {
        id: "user-settings",
        author: "user",
        kind: "requirement",
        text: "我看到设置后台 UI 截图，右侧内容区文字太贴边，希望增加 padding / gap / section spacing。不要修改左侧菜单、业务逻辑和整体视觉风格。",
        createdAt: "2026-09-24T00:00:00Z",
        status: "completed",
        attachments: [{
          fileId: "image-1",
          fileName: "settings.png",
          managedPath: "",
          attachmentType: "image",
          contentType: "image/png",
          relativePath: ".ganmaoyuan/chat-attachments/user-settings/image-1-settings.png",
        }],
        relatedTask: "",
        source: "user",
        modelId: "",
        evidenceItems: [],
      },
      {
        id: "assistant-settings",
        author: "ganmaoyuan",
        kind: "assistant",
        text: "Codex 修改方案：增加外层内容区 padding，调整区块 vertical gap / section spacing，增加表格行上下 padding；不修改左侧菜单，不改变整体视觉风格，不改业务逻辑，修改后运行最新构建进行验证。",
        createdAt: "2026-09-24T00:01:00Z",
        status: "completed",
        attachments: [],
        relatedTask: "",
        source: "deepseek",
        modelId: "deepseek-v4-flash",
        evidenceItems: [],
      },
    ];

    const draft = buildCodexDraftFromDiscussion(messages);

    expect(draft?.title).toBe("优化设置后台右侧内容区留白");
    expect(draft?.taskType).toBe("coding");
    expect(draft?.instructions).toContain("增加外层内容区 padding");
    expect(draft?.instructions).toContain("不修改左侧菜单");
    expect(draft?.instructions).toContain("settings.png");
    expect(draft?.instructions).toContain(".ganmaoyuan/chat-attachments/user-settings");
  });

  it("does not invent a Codex task when the latest discussion is not actionable", () => {
    const messages: WorkspaceMessage[] = [{
      id: "user-chat",
      author: "user",
      kind: "requirement",
      text: "好的，谢谢。",
      createdAt: "2026-09-24T00:00:00Z",
      status: "completed",
      attachments: [],
      relatedTask: "",
      source: "user",
      modelId: "",
      evidenceItems: [],
    }];

    expect(buildCodexDraftFromDiscussion(messages)).toBeNull();
  });

  it("derives a specific title from a concrete latest coding request", () => {
    const messages: WorkspaceMessage[] = [{
      id: "user-live-status",
      author: "user",
      kind: "requirement",
      text: "修复 Codex 实时执行进展的中文显示。",
      createdAt: "2026-10-09T00:00:00Z",
      status: "completed",
      attachments: [],
      relatedTask: "",
      source: "user",
      modelId: "",
      evidenceItems: [],
    }];

    expect(buildCodexDraftFromDiscussion(messages)).toMatchObject({
      title: "修复 Codex 实时执行进展的中文显示",
      taskType: "coding",
    });
  });

  it("does not create a draft with a generic fallback title", () => {
    const messages: WorkspaceMessage[] = [
      {
        id: "user-generic-coding",
        author: "user",
        kind: "requirement",
        text: "请帮我优化一下。",
        createdAt: "2026-10-09T00:00:00Z",
        status: "completed",
        attachments: [],
        relatedTask: "",
        source: "user",
        modelId: "",
        evidenceItems: [],
      },
      {
        id: "assistant-generic-coding",
        author: "ganmaoyuan",
        kind: "assistant",
        text: "可以优化一下。",
        createdAt: "2026-10-09T00:01:00Z",
        status: "completed",
        attachments: [],
        relatedTask: "",
        source: "deepseek",
        modelId: "deepseek-v4-flash",
        evidenceItems: [],
      },
    ];

    expect(buildCodexDraftFromDiscussion(messages)).toBeNull();
  });
});

describe("Workspace rendering limits", () => {
  it("keeps the newest conversation messages in the initial work surface", () => {
    expect(latestWorkspaceItems(["1", "2", "3", "4"], 2)).toEqual(["3", "4"]);
  });

  it("keeps a selected reference file visible before the rest of the file list is expanded", () => {
    const visible = visibleReferenceFiles(
      [{ id: "a" }, { id: "b" }, { id: "c" }],
      "c",
      2,
    );

    expect(visible.map((item) => item.id)).toEqual(["c", "a"]);
  });
});

describe("Workspace activity language", () => {
  it("summarizes internal activity with user-facing labels", () => {
    expect(workspaceActivityLabel({ kind: "analysis", status: "completed" })).toBe("项目理解");
    expect(workspaceActivityLabel({ kind: "monitor", status: "completed" })).toBe("资料变化");
    expect(workspaceActivityLabel({ kind: "review", status: "pending" })).toBe("待确认");
    expect(workspaceActivityLabel({ kind: "monitor", status: "failed" })).toBe("需要检查");
  });

  it("keeps internal activity enums and paths out of the public view", () => {
    expect(workspaceMessageDisplayLabel({ kind: "fileLocation", status: "completed" })).toBe("文件已整理");
    expect(workspaceMessageDisplayLabel({ kind: "assistant", status: "interrupted" })).toBe("生成已中断");
    expect(projectAnalysisStatusLabel("success")).toBe("已整理");
    expect(sanitizeWorkspacePublicText("codex-task-1234.md", "task")).toBe("已保存 Codex 任务记录。");
    expect(sanitizeWorkspacePublicText("Result Bridge updated taskId: 1234", "codex")).toBe("Codex 工作记录已更新。");
    expect(sanitizeWorkspacePublicText("D:\\GanMaoYuan\\SelfProject\\internal.json", "monitor")).toBe("相关资料已更新。");
  });
});

describe("Codex task console actions", () => {
  it("refreshes the ledger only when a run first becomes terminal", () => {
    const running = [{ id: "run-a", taskId: "task-a", status: "running", startedAt: "10" }] as never;
    const exited = [{ id: "run-a", taskId: "task-a", status: "exited", startedAt: "10", endedAt: "20", exitCode: 0 }] as never;

    expect(terminalRunStateChanged(terminalRunSignatures(running), running)).toBe(false);
    expect(terminalRunStateChanged(terminalRunSignatures(running), exited)).toBe(true);
    expect(terminalRunStateChanged(terminalRunSignatures(exited), exited)).toBe(false);
  });

  it("immediately exposes a persisted running run while the ledger refresh is pending", () => {
    const runs = mergeCodexRuns(
      [],
      { id: "run-new", taskId: "task-a", status: "running", startedAt: "2026-09-18T10:00:00Z" } as never,
    );
    const view = buildCodexTaskView(codexTask({ status: "ready" }), runs[0] as never);

    expect(runs).toHaveLength(1);
    expect(view.statusLabel).toBe("Codex 执行中");
    expect(view.primaryAction).toBe("stop");
  });

  it.each([
    ["failed", "exited", 0, "执行失败", "retry"],
    ["cancelled", "exited", 0, "已停止", "retry"],
    ["verifying", "exited", 0, "正在核验", "none"],
    ["ready", "exited", 0, "待启动", "start"],
    ["awaitingAcceptance", "running", null, "等待验收", "accept"],
    ["completed", "failed", 1, "已完成", "none"],
    ["needsReview", "running", null, "需要检查", "accept"],
  ] as const)("keeps task outcome %s authoritative over run %s", (status, runStatus, exitCode, label, action) => {
    const view = buildCodexTaskView(
      codexTask({
        status,
        ...(["needsReview", "verifying", "awaitingAcceptance"].includes(status)
          ? { resultId: "result-a", resultReceivedAt: "1" }
          : {}),
        gitVerification: { status: "notApplicable", repositoryPath: "", checkedAt: "", reason: "" },
      }),
      { status: runStatus, exitCode, error: "" },
    );
    expect(view.statusLabel).toBe(label);
    expect(view.primaryAction).toBe(action);
  });

  it("shows start as the only primary action for ready tasks", () => {
    const view = buildCodexTaskView(
      { status: "ready", summary: "", manualAcceptance: [], gitVerification: { status: "", repositoryPath: "", checkedAt: "", reason: "" } },
      null,
    );

    expect(view.primaryAction).toBe("start");
    expect(view.secondaryActions).toContain("copyPrompt");
  });

  it("shows stop as the only action while running", () => {
    const view = buildCodexTaskView(
      { status: "running", summary: "", manualAcceptance: [], gitVerification: { status: "", repositoryPath: "", checkedAt: "", reason: "" } },
      { status: "running", exitCode: null, error: "" },
    );

    expect(view.primaryAction).toBe("stop");
    expect(view.secondaryActions).toEqual([]);
  });

  it("treats exit 0 without a result as waiting for result bridge", () => {
    const view = buildCodexTaskView(
      { status: "handedOff", summary: "", manualAcceptance: [], gitVerification: { status: "", repositoryPath: "", checkedAt: "", reason: "" } },
      { status: "exited", exitCode: 0, error: "" },
    );

    expect(view.statusLabel).toBe("等待结果回流");
    expect(view.description).toContain("不能把任务判定为完成");
    expect(view.primaryAction).toBe("checkResult");
  });

  it("does not ask for action while result is being processed", () => {
    const view = buildCodexTaskView(
      {
        status: "resultReceived",
        summary: "结果已收到",
        resultId: "result-a",
        resultReceivedAt: "1",
        manualAcceptance: [],
        gitVerification: { status: "notApplicable", repositoryPath: "", checkedAt: "", reason: "" },
      },
      { status: "exited", exitCode: 0, error: "" },
    );

    expect(view.statusLabel).toBe("结果已回流");
    expect(view.primaryAction).toBe("none");
  });

  it("shows acceptance actions for needsReview tasks", () => {
    const view = buildCodexTaskView(
      {
        status: "needsReview",
        summary: "taskId 不一致",
        resultId: "result-a",
        resultReceivedAt: "2026-08-26T00:00:00Z",
        manualAcceptance: [],
        gitVerification: { status: "mismatch", repositoryPath: "", checkedAt: "", reason: "" },
      },
      { status: "exited", exitCode: 0, error: "" },
    );

    expect(view.statusLabel).toBe("需要检查");
    expect(view.primaryAction).toBe("accept");
    expect(view.secondaryActions).toEqual(["reject"]);
  });

  it("does not offer acceptance when needsReview means the result never arrived", () => {
    const view = buildCodexTaskView(
      codexTask({
        status: "needsReview",
        remainingIssues: ["应用重启后未收到匹配的 Codex 结果；恢复窗口已结束。"],
      }),
      { status: "unknownAfterRestart", exitCode: null, error: "应用重启后未检测到原 Codex 进程。" },
    );

    expect(view.statusLabel).toBe("结果未回流");
    expect(view.primaryAction).toBe("checkResult");
    expect(view.secondaryActions).toEqual([]);
    expect(view.description).toContain("不会重新执行任务");
  });

  it("reports missing result scans and does not claim unrelated imports belong to this task", () => {
    const task = codexTask({ status: "needsReview", expectedResultPath: "D:/project/results/task-a-result.json" });
    const missing = buildCodexResultCheckFeedback(task, { status: "unknownAfterRestart", error: "" }, { scannedCount: 0, importedCount: 0 });
    expect(missing).toMatchObject({ tone: "warning", title: "未找到结果文件" });
    expect(missing.message).toContain("结果目录中没有可读取的结果文件");

    const unrelated = buildCodexResultCheckFeedback(task, { status: "unknownAfterRestart", error: "" }, { scannedCount: 2, importedCount: 1 });
    expect(unrelated.tone).toBe("warning");
    expect(unrelated.message).toContain("没有一条绑定到");

    const found = buildCodexResultCheckFeedback(
      codexTask({ ...task, resultId: "result-a", resultReceivedAt: "1" }),
      { status: "exited", error: "" },
      { scannedCount: 2, importedCount: 1 },
    );
    expect(found).toMatchObject({ tone: "success", title: "已找到本次任务结果" });
    expect(found.message).toContain("持久化结果");
  });

  it("only offers acceptance actions while awaiting manual acceptance", () => {
    const view = buildCodexTaskView(
      {
        status: "awaitingAcceptance",
        summary: "完成",
        resultId: "result-a",
        resultReceivedAt: "1",
        manualAcceptance: ["打开桌面应用确认"],
        gitVerification: { status: "verified", repositoryPath: "", checkedAt: "", reason: "" },
      },
      { status: "exited", exitCode: 0, error: "" },
    );

    expect(view.primaryAction).toBe("accept");
    expect(view.secondaryActions).toEqual(["reject"]);
  });

  it("sorts same-title tasks by persisted creation time and formats persisted timestamps", () => {
    const now = Date.UTC(2026, 8, 24, 7, 30);
    const tasks = [
      codexTask({ taskId: "old", title: "优化设置后台右侧内容区留白", createdAt: String(now - 30 * 60 * 1000), updatedAt: String(now - 30 * 60 * 1000) }),
      codexTask({ taskId: "new", title: "优化设置后台右侧内容区留白", createdAt: String(now - 5 * 60 * 1000), updatedAt: String(now - 5 * 60 * 1000) }),
      codexTask({ taskId: "middle", title: "优化设置后台右侧内容区留白", createdAt: String(now - 20 * 60 * 1000), updatedAt: String(now - 20 * 60 * 1000) }),
    ];
    const ordered = sortCodexTasksByRecency(tasks, []);

    expect(ordered.map((task) => task.taskId)).toEqual(["new", "middle", "old"]);
    expect(formatCodexTaskDate(String(now - 5 * 60 * 1000), now)).toMatch(/^今天 \d{2}:\d{2}$/);
    expect(
      codexTaskTimeSummary(
        tasks[1],
        { startedAt: String(now - 4 * 60 * 1000), endedAt: String(now - 1 * 60 * 1000) },
        now,
      ),
    ).toContain("用时 3分00秒");
  });

  it("does not show runnable actions for completed tasks", () => {
    const view = buildCodexTaskView(
      { status: "completed", summary: "完成", manualAcceptance: [], gitVerification: { status: "verified", repositoryPath: "", checkedAt: "", reason: "" } },
      { status: "exited", exitCode: 0, error: "" },
    );

    expect(view.primaryAction).toBe("none");
    expect(view.secondaryActions).toEqual([]);
  });

  it("lets cancelled tasks retry without marking them complete", () => {
    const view = buildCodexTaskView(
      { status: "cancelled", summary: "", manualAcceptance: [], gitVerification: { status: "", repositoryPath: "", checkedAt: "", reason: "" } },
      { status: "cancelled", exitCode: null, error: "" },
    );

    expect(view.statusLabel).toBe("已停止");
    expect(view.primaryAction).toBe("retry");
  });
});

describe("Codex task type result groups", () => {
  it("keeps analysis results free of commit and test sections", () => {
    const groups = codexTaskResultGroups(codexTask({ taskType: "analysis", findings: ["项目资料不足"], tests: ["npm test"] }));

    expect(groups.map((group) => group.title)).toEqual(["分析发现", "建议", "待确认问题"]);
    expect(groups.map((group) => group.title)).not.toContain("测试");
    expect(groups.map((group) => group.title)).not.toContain("修改文件");
  });

  it("shows changed files and tests for coding tasks", () => {
    const groups = codexTaskResultGroups(codexTask({ taskType: "coding", changedFiles: ["src/pages/WorkPage.tsx"], tests: ["npm test"] }));

    expect(groups.map((group) => group.title)).toEqual(["修改文件", "测试"]);
  });

  it("shows checks for verification tasks", () => {
    const groups = codexTaskResultGroups(codexTask({ taskType: "verification", checks: ["桌面验收"], passed: ["构建通过"] }));

    expect(groups.map((group) => group.title)).toEqual(["检查项", "通过", "未通过"]);
  });

  it("shows artifacts and target files for file operation tasks", () => {
    const groups = codexTaskResultGroups(codexTask({ taskType: "fileOperation", artifacts: ["报告.md"], targetFiles: ["D:\\\\example\\\\报告.md"] }));

    expect(groups.map((group) => group.title)).toEqual(["产物", "目标文件", "失败项"]);
  });
});

describe("Codex task console polish", () => {
  it("uses a short display title instead of the full prompt", () => {
    const prompt =
      "taskId：168eb63d-778d-4dcd-8571-984cb92a41c6\n请基于当前项目说明，已导入资料和现有记录，先生成为这个“感冒院”项目的项目理解、待确认问题，以及接下来最值得继续做的下一步建议。";
    const title = codexDisplayTaskTitle(codexTask({ title: prompt, prompt }));

    expect(title).toBe("生成项目理解与下一步建议");
    expect(title).not.toContain("168eb63d");
    expect(title.length).toBeLessThanOrEqual(35);
  });

  it("keeps technical bridge language out of the public result summary", () => {
    const summary =
      "taskId: 168eb63d-778d-4dcd-8571-984cb92a41c6 resultPath: D:/demo runnerFallback Result Bridge Codex CLI Git Observer stderr";
    const publicSummary = codexUserResultSummary(codexTask({ title: "修复 Codex 控制台", taskType: "coding", summary }));

    expect(publicSummary).not.toContain("taskId");
    expect(publicSummary).not.toContain("resultPath");
    expect(publicSummary).not.toContain("runnerFallback");
    expect(publicSummary).not.toContain("Result Bridge");
    expect(publicSummary).toContain("还没有形成可确认的代码提交");
  });

  it("keeps waiting states free of bridge implementation wording", () => {
    const view = buildCodexTaskView(
      { status: "awaitingResult", summary: "", manualAcceptance: [], gitVerification: { status: "", repositoryPath: "", checkedAt: "", reason: "" } },
      null,
    );

    expect(`${view.description} ${view.progressTitle}`).not.toContain("Result Bridge");
    expect(`${view.description} ${view.progressTitle}`).not.toContain("taskId");
  });

  it("filters technical noise from analysis result groups", () => {
    const groups = codexTaskResultGroups(
      codexTask({
        taskType: "analysis",
        findings: ["项目资料不足", "runnerFallback created taskId-result.json", "resultPath: D:/demo/result.json"],
        recommendations: ["继续补充真实资料"],
      }),
    );

    expect(groups[0].items).toEqual(["项目资料不足"]);
    expect(groups.flatMap((group) => group.items).join(" ")).not.toContain("runnerFallback");
    expect(groups.flatMap((group) => group.items).join(" ")).not.toContain("resultPath");
  });

  it("does not expose full commits in recent task summaries", () => {
    const summary = codexHistoryGitSummary(
      codexTask({
        taskType: "coding",
        verifiedCommits: ["1234567890abcdef1234567890abcdef12345678"],
        gitVerification: { status: "verified", repositoryPath: "", checkedAt: "", reason: "" },
      }),
    );

    expect(summary).toBe("Git 1234567");
    expect(summary).not.toContain("1234567890abcdef");
  });

  it("hides Git noise for analysis history rows", () => {
    const summary = codexHistoryGitSummary(codexTask({ taskType: "analysis" }));

    expect(summary).toBe("");
  });

  it("shortens commits consistently", () => {
    expect(codexShortCommit("abcdef1234567890abcdef1234567890abcdef12")).toBe("abcdef1");
  });

  it("never uses a UUID or taskId as the recent task display title", () => {
    const taskId = "168eb63d-778d-4dcd-8571-984cb92a41c6";
    const title = codexDisplayTaskTitle(
      codexTask({
        taskId,
        title: taskId,
        prompt: `taskId：${taskId}\nresultPath：D:\\\\demo\\\\${taskId}-result.json`,
        summary: "",
      }),
    );

    expect(title).toBe("未命名 Codex 任务");
    expect(title).not.toBe(taskId);
    expect(title).not.toMatch(/[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}/i);
  });

  it("falls back from unsafe ids to a safe semantic prompt title", () => {
    const taskId = "168eb63d-778d-4dcd-8571-984cb92a41c6";
    const title = codexDisplayTaskTitle(
      codexTask({
        taskId,
        title: taskId,
        prompt: `taskId：${taskId}\n请生成项目理解、待确认问题和下一步建议。`,
      }),
    );

    expect(title).toBe("生成项目理解与下一步建议");
  });

  it("preserves persisted coding type when the prompt contains generic analysis fields", () => {
    const task = codexTask({
      title: "优化设置后台右侧内容区留白",
      taskType: "coding",
      prompt: "taskId：task-a\ntaskType：coding\ncoding 可包含 changedFiles、commits、tests；analysis 可包含 findings、recommendations、questions。",
      changedFiles: [],
      reportedCommits: [],
      verifiedCommits: [],
      tests: ["runnerFallback 不应出现在分析主视图"],
    });
    const reloaded = JSON.parse(JSON.stringify(task)) as CodexTask;

    expect(resolveCodexTaskDisplayType(reloaded)).toBe("coding");
    expect(codexTaskResultGroups(reloaded).map((group) => group.title)).toEqual(["修改文件", "测试"]);
  });
});

describe("Codex task deep link selection", () => {
  it("selects a newly generated ready task when it is explicitly targeted", () => {
    const oldTask = codexTask({ taskId: "task-old", status: "awaitingAcceptance", title: "旧任务" });
    const newTask = codexTask({ taskId: "task-new", status: "ready", title: "新任务", prompt: "taskId：task-new" });
    const selected = selectCurrentCodexTask(
      [oldTask, newTask],
      [{ taskId: "task-old", status: "exited", startedAt: "9" }] as never,
      "task-new",
    );

    expect(selected?.taskId).toBe("task-new");
    expect(selected?.status).toBe("ready");
  });

  it("builds a stable codex task query for generated and selected tasks", () => {
    const params = buildCodexTaskSearchParams(new URLSearchParams("panel=workflow"), "task-new");

    expect(params.get("panel")).toBe("codex");
    expect(params.get("taskId")).toBe("task-new");
  });

  it("opens a needsReview task from a continue-work taskId", () => {
    const oldTask = codexTask({ taskId: "task-old", status: "awaitingAcceptance", title: "旧任务" });
    const targetTask = codexTask({
      taskId: "task-target",
      status: "needsReview",
      resultId: "result-target",
      resultSource: "codex",
      title: "检查结果",
    });
    const selected = selectCurrentCodexTask([oldTask, targetTask], [{ taskId: "task-old", status: "exited", startedAt: "9" }] as never, "task-target");

    expect(selected?.taskId).toBe("task-target");
    expect(selected?.status).toBe("needsReview");
  });

  it("opens an awaitingAcceptance task from a continue-work taskId", () => {
    const readyTask = codexTask({ taskId: "task-ready", status: "ready", title: "新任务" });
    const targetTask = codexTask({
      taskId: "task-accept",
      status: "awaitingAcceptance",
      resultId: "result-accept",
      resultSource: "codex",
      title: "验收任务",
    });
    const selected = selectCurrentCodexTask([readyTask, targetTask], [], "task-accept");

    expect(selected?.taskId).toBe("task-accept");
    expect(selected?.status).toBe("awaitingAcceptance");
  });

  it("does not cross projects when the target task is not loaded", () => {
    const localTask = codexTask({ taskId: "project-a-task", status: "ready", title: "本项目任务" });
    const selected = selectCurrentCodexTask([localTask], [], "project-b-task");

    expect(selected?.taskId).toBe("project-a-task");
  });

  it("safely falls back when a linked task no longer exists", () => {
    const fallbackTask = codexTask({ taskId: "fallback-task", status: "ready", title: "可处理任务" });
    const selected = selectCurrentCodexTask([fallbackTask], [], "missing-task");

    expect(selected?.taskId).toBe("fallback-task");
  });

  it("does not keep jumping to an already completed deep-linked task", () => {
    const completed = codexTask({ taskId: "completed-task", status: "completed", title: "已完成任务" });
    const active = codexTask({
      taskId: "active-task",
      status: "needsReview",
      resultId: "result-active",
      resultSource: "codex",
      title: "当前问题",
    });
    const selected = selectCurrentCodexTask([completed, active], [], "completed-task");

    expect(selected?.taskId).toBe("active-task");
  });

  it("keeps a just-accepted completed task visible while it is pinned", () => {
    const completed = codexTask({ taskId: "completed-task", status: "completed", title: "刚验收任务" });
    const active = codexTask({
      taskId: "active-task",
      status: "needsReview",
      resultId: "result-active",
      resultSource: "codex",
      title: "另一个任务",
    });
    const selected = selectCurrentCodexTask([completed, active], [], "completed-task", "completed-task");

    expect(selected?.taskId).toBe("completed-task");
    expect(selected?.status).toBe("completed");
  });

  it("does not let an exited historical run outrank a newer ready task", () => {
    const oldTask = codexTask({ taskId: "old-task", status: "failed", updatedAt: "2026-10-10T10:00:00Z" });
    const readyTask = codexTask({ taskId: "ready-task", status: "ready", updatedAt: "2026-10-10T11:00:00Z" });
    const selected = selectCurrentCodexTask(
      [oldTask, readyTask],
      [{ taskId: "old-task", status: "exited", processAlive: false, startedAt: "2026-10-10T10:00:00Z" } as CodexRun],
    );

    expect(selected?.taskId).toBe("ready-task");
  });

  it("does not promote a needsReview task without a bound result over a real next task", () => {
    const staleReview = codexTask({ taskId: "stale-review", status: "needsReview", updatedAt: "2026-10-10T12:00:00Z" });
    const readyTask = codexTask({ taskId: "ready-task", status: "ready", updatedAt: "2026-10-10T11:00:00Z" });

    expect(selectCurrentCodexTask([staleReview, readyTask], [])?.taskId).toBe("ready-task");
  });

  it("ignores a stale deep link when its review has no bound result", () => {
    const staleReview = codexTask({ taskId: "stale-review", status: "needsReview", updatedAt: "2026-10-10T12:00:00Z" });
    const readyTask = codexTask({ taskId: "ready-task", status: "ready", updatedAt: "2026-10-10T11:00:00Z" });

    expect(selectCurrentCodexTask([staleReview, readyTask], [], "stale-review")?.taskId).toBe("ready-task");
  });

  it("keeps an explicitly selected historical task inspectable without treating it as default focus", () => {
    const staleReview = codexTask({ taskId: "stale-review", status: "needsReview" });

    expect(selectCurrentCodexTask([staleReview], [], "stale-review", "stale-review")?.taskId).toBe("stale-review");
  });

  it("keeps a resultless restarted task inspectable after valid work is considered", () => {
    const staleRunTask = codexTask({ taskId: "stale-run", status: "awaitingResult" });
    const unknownRun: CodexRun = {
      id: "run-stale",
      taskId: "stale-run",
      projectId: "project-a",
      repositoryPath: "",
      commandExecutable: "codex.exe",
      commandArgs: [],
      capabilityProbe: {} as CodexRun["capabilityProbe"],
      workingDirectory: "",
      stdinPromptSummary: "",
      status: "unknownAfterRestart",
      startedAt: "2026-10-10T10:00:00Z",
      endedAt: "2026-10-10T10:03:00Z",
      pid: 42,
      processAlive: false,
      exitCode: null,
      outputLastMessagePath: "",
      stdoutSummary: "",
      stderrSummary: "",
      lastActivityAt: "",
      recentActivity: [],
      error: "No matching result file.",
    };

    expect(selectCurrentCodexTask([staleRunTask], [unknownRun])?.taskId).toBe("stale-run");
    const readyTask = codexTask({ taskId: "ready-task", status: "ready", updatedAt: "2026-10-10T11:00:00Z" });
    expect(selectCurrentCodexTask([staleRunTask, readyTask], [unknownRun], "stale-run")?.taskId).toBe("ready-task");
  });

  it("keeps a process-backed live Codex task ahead of queued work", () => {
    const liveTask = codexTask({ taskId: "live-task", status: "running", updatedAt: "2026-10-10T10:00:00Z" });
    const readyTask = codexTask({ taskId: "ready-task", status: "ready", updatedAt: "2026-10-10T12:00:00Z" });
    const liveRun: CodexRun = {
      id: "run-live",
      taskId: "live-task",
      projectId: "project-a",
      repositoryPath: "",
      commandExecutable: "codex.exe",
      commandArgs: [],
      capabilityProbe: {} as CodexRun["capabilityProbe"],
      workingDirectory: "",
      stdinPromptSummary: "",
      status: "running",
      startedAt: "2026-10-10T10:00:00Z",
      endedAt: "",
      pid: 42,
      processAlive: true,
      exitCode: null,
      outputLastMessagePath: "",
      stdoutSummary: "",
      stderrSummary: "",
      lastActivityAt: "",
      recentActivity: [],
      error: "",
    };

    expect(selectCurrentCodexTask([readyTask, liveTask], [liveRun])?.taskId).toBe("live-task");
  });
});

describe("Codex task stepper", () => {
  it("marks Git as not applicable for analysis tasks", () => {
    const steps = codexTaskSteps(codexTask({ taskType: "analysis", status: "resultReceived" }), { status: "exited" } as never);
    const gitStep = steps.find((step) => step.key === "git");

    expect(gitStep?.note).toBe("不适用");
  });
});

describe("Codex report task binding", () => {
  it("extracts the task id from bridge result reports", () => {
    expect(
      codexReportTaskId({
        summary: "taskId：task-b\n完成状态：completed",
        sourceLabel: "task-a-result.json",
        evidenceManagedPath: "",
      }),
    ).toBe("task-b");
  });

  it("falls back to the result file name when summary has no task id", () => {
    expect(
      codexReportTaskId({
        summary: "完成摘要：旧报告",
        sourceLabel: "a39322bb-9d48-4c6e-ab39-721bf3fcfa65-result.json",
        evidenceManagedPath: "",
      }),
    ).toBe("a39322bb-9d48-4c6e-ab39-721bf3fcfa65");
  });
});
