# 感冒院架构概览

## 核心原则

感冒院不把 AI 建议直接写成事实。所有可见工作状态都应能回到本地记录、文件证据、用户决定或 Git / Codex 执行结果。

## 五个稳定核心

1. **Workspace / File**：Inbox、Scanner、Location Service、Cleanup Plan 和受管副本组成安全文件流。FileProjection 是文件事实的统一读取投影。
2. **Activity / Pending Action**：Work Ledger 保存事实，Activity Timeline 面向用户展示，Pending Action 只保留需要人工处理的事项。
3. **Execution**：Cleanup Execution 与 Codex Run 共享执行、失败、取消和恢复的通用语义，但保持各自领域规则。
4. **Decision / Evidence**：Decision Trace 关联输入证据、建议、用户选择和执行结果；只保存必要摘要与引用。
5. **Project**：项目根目录、关联代码仓库和项目状态可以分离，避免把代码目录误当作资料目录。

## 文件工作流

```text
Inbox / Workspace Scanner
  -> 文件理解与归属判断
  -> Location Preview
  -> Cleanup Plan 审核
  -> 安全复制到 Workspace
  -> Audit / Undo / Search / FileProjection
```

原文件始终是来源证据。执行整理时仅创建感冒院管理的副本，并在撤销时只处理该副本。

## 连续工作流

```text
Git facts + Workspace activity + Codex result + user decision
  -> Work Ledger / Activity Timeline
  -> Pending Action + Project State
  -> Continue Work Focus
  -> Today Workspace
```

Today 是事实投影视图，不是第二套项目管理数据源。

## Codex 任务流

```text
CodexTask
  -> CodexRun (optional local codex exec)
  -> result.json / result.md
  -> Result Bridge
  -> Git Verification (when applicable)
  -> Manual Acceptance (when required)
  -> completed
```

进程 `exit 0` 仅代表命令已结束，不代表任务完成。分析任务的 Git 核验可为“不适用”，代码任务则需要根据报告和仓库事实独立核验。

## 本地数据边界

- 应用配置与兼容数据位于本机应用数据目录。
- Workspace Root 承载未来受管工作空间；旧项目可继续在原目录读取。
- `.ganmaoyuan` 保存项目 Sidecar 元数据，不应进入公开 Git。
- 搜索、备份和 Undo 只读取本地受管记录；不要求上传原始文件正文。
