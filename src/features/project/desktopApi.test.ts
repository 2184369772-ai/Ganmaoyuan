import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { acceptCodexTask, rejectCodexTask } from "./desktopApi";

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
