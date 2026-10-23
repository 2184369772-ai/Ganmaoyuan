import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import {
  acceptCodexTask,
  readChatAttachment,
  rejectCodexTask,
  retryInterruptedProjectMessage,
  scanProjectWorkspace,
  sendProjectMessage,
  takeStorageRecoveryNotices,
} from "./desktopApi";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

describe("Codex review result binding", () => {
  beforeEach(() => { vi.mocked(invoke).mockReset(); });

  it("approves only the result displayed by the caller", async () => {
    vi.mocked(invoke).mockResolvedValue({ status: "completed" });
    await acceptCodexTask("isolated-project", "task-b", "result-b");
    expect(invoke).toHaveBeenCalledWith("accept_codex_task", {
      projectRoot: "isolated-project", taskId: "task-b", expectedResultId: "result-b",
    });
  });

  it("binds rejection to the displayed result and reason", async () => {
    await rejectCodexTask("isolated-project", "task-b", "result-b", "Needs evidence");
    expect(invoke).toHaveBeenCalledWith("reject_codex_task", {
      projectRoot: "isolated-project", taskId: "task-b", expectedResultId: "result-b", reason: "Needs evidence",
    });
  });

  it("does not swallow stale-result errors", async () => {
    vi.mocked(invoke).mockRejectedValue(new Error("Result changed"));
    await expect(acceptCodexTask("isolated-project", "task-b", "old-result"))
      .rejects.toThrow("Result changed");
  });
});

describe("Project workspace monitoring", () => {
  beforeEach(() => { vi.mocked(invoke).mockReset(); });

  it("exposes whether a background scan changed project facts", async () => {
    vi.mocked(invoke).mockResolvedValue({ manifest: { project: { id: "project-1" } }, changed: false });

    const result = await scanProjectWorkspace("isolated-project");

    expect(invoke).toHaveBeenCalledWith("scan_project_workspace", { projectRoot: "isolated-project" });
    expect(result.changed).toBe(false);
  });
});

describe("Project chat image attachments", () => {
  beforeEach(() => { vi.mocked(invoke).mockReset(); });

  it("keeps the legacy payload when no image is attached", async () => {
    vi.mocked(invoke).mockResolvedValue({ project: {}, messages: [] });

    await sendProjectMessage("isolated-project", "请继续工作");

    expect(invoke).toHaveBeenCalledWith("send_project_message", {
      projectRoot: "isolated-project",
      text: "请继续工作",
    });
  });

  it("sends current-message image data without importing it as a project file", async () => {
    vi.mocked(invoke).mockResolvedValue({ project: {}, messages: [] });
    const image = {
      fileName: "screen.png",
      contentType: "image/png",
      dataUrl: "data:image/png;base64,encoded",
    };

    await sendProjectMessage("isolated-project", "请看截图", [image]);

    expect(invoke).toHaveBeenCalledWith("send_project_message", {
      projectRoot: "isolated-project",
      text: "请看截图",
      imageAttachments: [image],
    });
  });

  it("loads persisted chat images through a project-scoped relative path", async () => {
    vi.mocked(invoke).mockResolvedValue({ dataUrl: "data:image/png;base64,encoded", contentType: "image/png" });

    await readChatAttachment("isolated-project", ".ganmaoyuan/chat-attachments/message/screen.png");

    expect(invoke).toHaveBeenCalledWith("read_chat_attachment", {
      projectRoot: "isolated-project",
      relativePath: ".ganmaoyuan/chat-attachments/message/screen.png",
    });
  });
});

describe("chat and storage recovery", () => {
  beforeEach(() => { vi.mocked(invoke).mockReset(); });

  it("retries the selected interrupted assistant message within its project", async () => {
    vi.mocked(invoke).mockResolvedValue({ project: {}, messages: [], streamMessageId: "assistant-retry" });
    await retryInterruptedProjectMessage("project-a", "assistant-interrupted");
    expect(invoke).toHaveBeenCalledWith("retry_interrupted_project_message", {
      projectRoot: "project-a",
      interruptedMessageId: "assistant-interrupted",
    });
  });

  it("retrieves structured storage fallback warnings for visible UI feedback", async () => {
    vi.mocked(invoke).mockResolvedValue([{ fileName: "project-location-manifest.json", primaryState: "内容损坏", backupAge: "约 1 天前", message: "读取 .bak" }]);
    await takeStorageRecoveryNotices();
    expect(invoke).toHaveBeenCalledWith("take_storage_recovery_notices");
  });
});
