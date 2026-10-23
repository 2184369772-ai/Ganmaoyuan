# 感冒院 Ganmaoyuan

感冒院是一个本地优先的 Windows 桌面工作空间：它把项目资料、文件整理、工作事实和代码代理任务放到同一条可追溯的工作流中，帮助用户在中断后快速恢复项目上下文并继续下一步工作。

> 这是一个持续演进中的个人作品项目。仓库不包含真实项目资料、用户工作区、API Key 或构建产物。

## 它解决什么问题

- 工作文件散落在桌面、下载目录和不同项目目录，难以判断归属与下一步。
- 隔一段时间回到项目后，需要重新回忆进度、决定和待处理事项。
- 将任务交给代码代理后，执行结果、Git 事实和人工验收难以形成可靠闭环。

## 已实现的核心能力

### 个人工作文件空间

- 通过 Inbox 和 Workspace Scanner 接收或扫描文件。
- 复用统一文件理解链，识别用途、归属、项目候选、位置建议与置信度。
- 先生成 Cleanup Plan 并由用户审核，再安全复制到 Workspace；原文件不会被自动移动或删除。
- 支持哈希去重、版本关系、生命周期、Search、Audit 和 Undo。

### 项目连续性

- Today Workspace 基于 Activity、Pending Action、项目状态和真实 Git 事实生成 Continue Work Focus。
- Work Ledger 区分用户可理解的工作事实与底层技术事件。
- Decision Trace 保存证据、建议、人工决定和执行结果，不保存模型内部思考或原始文件全文。

### Codex Task Bridge

- 在项目内生成带独立 `taskId` 的 Codex 任务。
- 支持本机 `codex exec` 执行、结果文件回流、Git 只读核验与人工验收 Gate。
- Result Bridge、Git Verification 和 Work Ledger 共同构成任务完成事实，不把进程退出误判为任务完成。

## 技术栈

- React 18 + TypeScript + Vite
- Tauri 2 + Rust
- JSON / JSONL 本地存储与 Sidecar 数据
- Windows 文件系统与可选的本机 Codex CLI

详见 [架构说明](docs/ARCHITECTURE.md)。

## 快速开始

### 前置条件

- Node.js 20+
- Rust 1.77+
- Windows WebView2 Runtime
- Windows 10/11

Codex CLI 仅在使用“启动本机 Codex 执行”时需要；其它工作空间能力不依赖它。

### 开发模式

```bash
npm install
npm run tauri:dev
```

### 构建

```bash
npm test
npm run build
cargo test --manifest-path src-tauri/Cargo.toml
npm run tauri build
```

## 数据与安全边界

- 应用数据、工作区和项目资料仅保存在本机路径，不应提交到此仓库。
- Cleanup Execution 只创建感冒院管理的副本；不会自动移动、删除或覆盖用户原文件。
- 项目之间按 `projectId` 和项目根目录隔离；跨项目知识只保存必要证据引用。
- API Key、令牌和凭据必须使用本机安全配置，不得写入源码、日志或 Git。
- `D:\Atlas` 仅作为只读参考边界，不由本项目自动写入。

更多安全问题请阅读 [SECURITY.md](SECURITY.md)。

## 许可证

当前仓库作为公开作品展示，不附带开源许可证；除非未来单独添加 `LICENSE`，否则保留全部权利。请不要将代码或设计用于生产环境、再发布或商业用途。

## 仓库约定

- `.env`、`.ganmaoyuan/`、`AppData/`、证书和构建目录均被忽略。
- 不要提交真实客户文件、项目导出、桌面截图、测试账号或本机结果文件。
- 提交前运行与改动范围相应的 Rust / 前端测试和构建检查。

## 当前状态

v1 的项目、文件、Continue Work、Codex 任务和工作事实回流主链已完成。后续重点是稳定性、真实使用反馈与公开发布准备，而不是继续堆叠功能。
