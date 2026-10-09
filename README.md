# Ganmaoyuan 感冒院

## Pick up where you left off.

[Download the Windows Beta v0.1.1](https://github.com/2184369772-ai/Ganmaoyuan/releases/tag/v0.1.1)

> Personal-project Beta. The installer is **unsigned** and has **not been independently tested for Windows installation or uninstallation**. Please use fictional or disposable test data only; do not use it for important work materials. See the [release notes and installer](https://github.com/2184369772-ai/Ganmaoyuan/releases/tag/v0.1.1).

<p align="center"><img src="docs/images/hero-github.png" alt="Ganmaoyuan promotional hero visual" width="100%"></p>
<p align="center"><sub>Promotional hero visual; the application window is illustrative.</sub></p>

Ganmaoyuan is a local-first Windows workspace for continuing work across project files, decisions, and code. Reopen a project, review its recorded context, and continue with a task grounded in facts you can inspect.

**Files, project context, and Codex tasks in one traceable workflow.**

[Source](https://github.com/2184369772-ai/Ganmaoyuan) · [Architecture](docs/ARCHITECTURE.md) · [Security](SECURITY.md) · [Releases](https://github.com/2184369772-ai/Ganmaoyuan/releases)

## Three connected workflows

### Organize project files safely

Import files or scan a workspace, review classification and proposed locations, then approve a cleanup plan. Ganmaoyuan creates managed copies; it does not automatically move or delete source files. Search, duplicate/version relationships, audit history, and Undo keep file operations traceable.

### Resume from recorded project context

Project context brings together recorded work, currently valid actions, user decisions, and Git facts. Continue Work and the Project Context Packet distinguish known facts from suggestions, so an old summary is not presented as a confirmed current plan.

<p align="center"><img src="docs/images/workflow-project-context.png" alt="AI-generated project continuity concept image" width="100%"></p>
<p align="center"><sub>AI-generated promotional concept image; not a real running screenshot, test evidence, or real project record.</sub></p>

### Run a project-scoped Codex task

Create a project-bound task and, when configured, run it through the local Codex CLI. Result Bridge imports output for that task; Git verification and human acceptance provide additional evidence. A process exit alone does not mean a task is complete.

<p align="center"><img src="docs/images/workflow-codex-acceptance.png" alt="AI-generated Codex task workflow concept image" width="100%"></p>
<p align="center"><sub>AI-generated promotional concept image; not a real running screenshot, test evidence, or real task record.</sub></p>

## Product walkthrough

The [30–45 second demo storyboard](docs/PROMO_DEMO_STORYBOARD.md) describes a real, concise walkthrough. The concept images above are labeled as illustrations, not screenshots or verification evidence.

## Quickstart

### Download and try the Windows Beta

1. Download the [Ganmaoyuan v0.1.1 Windows x64 installer](https://github.com/2184369772-ai/Ganmaoyuan/releases/download/v0.1.1/Ganmaoyuan_0.1.1_x64-setup.exe) from the public [Release page](https://github.com/2184369772-ai/Ganmaoyuan/releases/tag/v0.1.1).
2. The installer is unsigned, so Windows may show an unknown-publisher warning. Continue only if you are comfortable testing an unsigned Beta.
3. Try it with fictional or disposable project data. Do not use important work materials; independent Windows install/uninstall validation has not been completed.

### Build from source (developers)

Use this path if you want to inspect or develop the source rather than install the Beta.

### Requirements

- Windows 10 or 11 and WebView2 Runtime
- Node.js 20+ and Rust 1.77.2+ to build from source
- Codex CLI installed and signed in only if you want to start local Codex tasks

```bash
git clone https://github.com/2184369772-ai/Ganmaoyuan.git
cd Ganmaoyuan
npm ci
npm run tauri:dev
```

To validate and build:

```bash
npm test
npm run build
cargo test --manifest-path src-tauri/Cargo.toml
npm run tauri -- build
```

## Current status and limitations

- The v0.1.1 Windows x64 package is publicly available as a personal-project Beta. It is unsigned and has not passed independent Windows installation/uninstallation or interactive GUI validation. Use test data only, not important work materials.
- Core file and project-continuity paths have automated coverage. A clean, current-build desktop Codex CLI run through result return, Git verification, and human acceptance still needs real end-to-end verification.
- Codex execution depends on the user's local CLI installation and authentication. AI/provider access may require local configuration.
- The app is an evolving personal project. It has no hosted service, account system, or cloud sync.

## Architecture

- **Frontend:** React 18, TypeScript, Vite
- **Desktop and domain services:** Tauri 2, Rust
- **Persistence:** local JSON/JSONL and project sidecar data
- **Integrations:** Windows filesystem, optional local Codex CLI

The project separates a project's material root from its associated code repository. See [Architecture](docs/ARCHITECTURE.md) for the file, continuity, and Codex flows.

## Privacy and safety

- Project data, workspace files, and application state stay on local paths; this repository should not contain them.
- File cleanup creates Ganmaoyuan-managed copies. Original files are not automatically moved, deleted, or overwritten.
- Project records are scoped by `projectId` and project root; cross-project facts must not be mixed.
- API keys, tokens, and credentials must remain in local configuration and must never be committed or exposed in logs.
- Search, backups, and Undo use local managed records; they do not require uploading original file contents.
- Do not publish customer files, personal data, real project exports, local diagnostics, or screenshots containing private paths.

Read the full [Security Policy](SECURITY.md) before testing or reporting an issue.

## License

This public repository currently has no open-source license. All rights are reserved; do not reuse, redistribute, or use the code or design commercially unless a license is added.
