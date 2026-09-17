import { describe, expect, it } from "vitest";
import {
  buildCodexTaskSearchParams,
  buildCodexTaskView,
  codexDisplayTaskTitle,
  codexHistoryGitSummary,
  codexReportTaskId,
  codexShortCommit,
  codexTaskResultGroups,
  codexTaskSteps,
  codexUserResultSummary,
  resolveCodexTaskDisplayType,
  resolveCodexPromptText,
  selectCurrentCodexTask,
} from "./WorkPage";
import type { CodexTask } from "../features/project/desktopApi";

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

describe("Codex task console actions", () => {
  it.each([
    ["failed", "exited", 0, "执行失败", "retry"],
    ["cancelled", "exited", 0, "已停止", "retry"],
    ["verifying", "exited", 0, "正在核验", "none"],
    ["ready", "exited", 0, "待启动", "start"],
    ["awaitingAcceptance", "running", null, "等待验收", "accept"],
    ["completed", "failed", 1, "已完成", "none"],
    ["needsReview", "running", null, "需要检查", "review"],
  ] as const)("keeps task outcome %s authoritative over run %s", (status, runStatus, exitCode, label, action) => {
    const view = buildCodexTaskView(
      codexTask({ status, gitVerification: { status: "notApplicable", repositoryPath: "", checkedAt: "", reason: "" } }),
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
        manualAcceptance: [],
        gitVerification: { status: "notApplicable", repositoryPath: "", checkedAt: "", reason: "" },
      },
      { status: "exited", exitCode: 0, error: "" },
    );

    expect(view.statusLabel).toBe("结果已回流");
    expect(view.primaryAction).toBe("none");
  });

  it("shows review as the only main action for needsReview tasks", () => {
    const view = buildCodexTaskView(
      {
        status: "needsReview",
        summary: "taskId 不一致",
        manualAcceptance: [],
        gitVerification: { status: "mismatch", repositoryPath: "", checkedAt: "", reason: "" },
      },
      { status: "exited", exitCode: 0, error: "" },
    );

    expect(view.statusLabel).toBe("需要检查");
    expect(view.primaryAction).toBe("review");
  });

  it("only offers acceptance actions while awaiting manual acceptance", () => {
    const view = buildCodexTaskView(
      {
        status: "awaitingAcceptance",
        summary: "完成",
        manualAcceptance: ["打开桌面应用确认"],
        gitVerification: { status: "verified", repositoryPath: "", checkedAt: "", reason: "" },
      },
      { status: "exited", exitCode: 0, error: "" },
    );

    expect(view.primaryAction).toBe("accept");
    expect(view.secondaryActions).toEqual(["reject"]);
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

  it("treats project understanding history as analysis even when legacy taskType is coding", () => {
    const task = codexTask({
      title: "生成项目理解与下一步建议",
      taskType: "coding",
      prompt: "请生成项目理解、待确认问题和下一步建议。",
      changedFiles: [],
      reportedCommits: [],
      verifiedCommits: [],
      tests: ["runnerFallback 不应出现在分析主视图"],
      findings: ["资料已经形成项目理解"],
      recommendations: ["下一步做人工验收"],
    });

    expect(resolveCodexTaskDisplayType(task)).toBe("analysis");
    expect(codexTaskResultGroups(task).map((group) => group.title)).toEqual(["分析发现", "建议", "待确认问题"]);
    expect(codexHistoryGitSummary(task)).toBe("");
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
    const targetTask = codexTask({ taskId: "task-target", status: "needsReview", title: "检查结果" });
    const selected = selectCurrentCodexTask([oldTask, targetTask], [{ taskId: "task-old", status: "exited", startedAt: "9" }] as never, "task-target");

    expect(selected?.taskId).toBe("task-target");
    expect(selected?.status).toBe("needsReview");
  });

  it("opens an awaitingAcceptance task from a continue-work taskId", () => {
    const readyTask = codexTask({ taskId: "task-ready", status: "ready", title: "新任务" });
    const targetTask = codexTask({ taskId: "task-accept", status: "awaitingAcceptance", title: "验收任务" });
    const selected = selectCurrentCodexTask([readyTask, targetTask], [], "task-accept");

    expect(selected?.taskId).toBe("task-accept");
    expect(selected?.status).toBe("awaitingAcceptance");
  });

  it("does not cross projects when the target task is not loaded", () => {
    const localTask = codexTask({ taskId: "project-a-task", status: "needsReview", title: "本项目任务" });
    const selected = selectCurrentCodexTask([localTask], [], "project-b-task");

    expect(selected?.taskId).toBe("project-a-task");
  });

  it("safely falls back when a linked task no longer exists", () => {
    const fallbackTask = codexTask({ taskId: "fallback-task", status: "needsReview", title: "可处理任务" });
    const selected = selectCurrentCodexTask([fallbackTask], [], "missing-task");

    expect(selected?.taskId).toBe("fallback-task");
  });

  it("does not keep jumping to an already completed deep-linked task", () => {
    const completed = codexTask({ taskId: "completed-task", status: "completed", title: "已完成任务" });
    const active = codexTask({ taskId: "active-task", status: "needsReview", title: "当前问题" });
    const selected = selectCurrentCodexTask([completed, active], [], "completed-task");

    expect(selected?.taskId).toBe("active-task");
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
