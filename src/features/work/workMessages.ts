export type WorkMessageKind = "restore" | "user" | "assistant" | "attachment" | "result";

export type WorkAttachment = {
  id: string;
  name: string;
};

export type WorkMessage = {
  id: string;
  kind: WorkMessageKind;
  author: "user" | "ganmaoyuan" | "system";
  text: string;
  createdAt: string;
  attachments?: WorkAttachment[];
};

export function createMessage(
  kind: WorkMessageKind,
  author: WorkMessage["author"],
  text: string,
  attachments: WorkAttachment[] = [],
): WorkMessage {
  return {
    id: createId(),
    kind,
    author,
    text,
    attachments,
    createdAt: new Date().toISOString(),
  };
}

export function createInitialWorkMessages(): WorkMessage[] {
  return [
    createMessage(
      "restore",
      "ganmaoyuan",
      "上次已经完成工作界面与后台管理功能的分离。现在需要把工作页改成可以持续讨论、处理资料和产出结果的工作台。今天准备从哪里开始？",
    ),
  ];
}

export function createAttachmentMessages(files: WorkAttachment[]): WorkMessage[] {
  if (!files.length) return [];

  return [
    createMessage(
      "attachment",
      "system",
      files.length === 1 ? `已添加资料：${files[0].name}` : `已添加 ${files.length} 份资料：${files.map((file) => file.name).join("、")}`,
      files,
    ),
  ];
}

export function createMockReply(userText: string, attachments: WorkAttachment[]): WorkMessage {
  const attachmentLine = attachments.length
    ? `我也会把这次新增的 ${attachments.length} 份资料纳入当前上下文。`
    : "如果你有新的 Word、Excel、PDF 或图片资料，可以继续补充进来。";

  return createMessage(
    "assistant",
    "ganmaoyuan",
    `收到。今天可以先围绕“${summarizeText(userText)}”往前推进。${attachmentLine} 建议下一步先明确本轮要交付的结果，再把需要给 Codex 或 OpenDesign 的上下文整理成可执行任务。`,
  );
}

export function createWrapResult(done: string, nextStep: string): WorkMessage {
  return createMessage(
    "result",
    "ganmaoyuan",
    `已建立恢复点。今天完成：${done || "已推进当前工作"}。下一步：${nextStep || "继续从当前任务恢复并推进。"}`,
  );
}

function summarizeText(text: string) {
  const normalized = text.replace(/\s+/g, " ").trim();
  if (normalized.length <= 28) return normalized;
  return `${normalized.slice(0, 28)}...`;
}

function createId() {
  return `${Date.now()}-${Math.random().toString(16).slice(2)}`;
}
