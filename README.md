# Ganmaoyuan 感冒院

## Pick up where you left off.

Ganmaoyuan is a local-first Windows workspace for continuing work across project files, decisions, and code. Reopen a project, review its recorded context, and continue with a task grounded in facts you can inspect.

**Files, project context, and Codex tasks in one traceable workflow.**

[Source](https://github.com/2184369772-ai/Ganmaoyuan) · [Architecture](docs/ARCHITECTURE.md) · [Security](SECURITY.md) · [Releases](https://github.com/2184369772-ai/Ganmaoyuan/releases)

## Three connected workflows

### Organize project files safely

Import files or scan a workspace, review classification and proposed locations, then approve a cleanup plan. Ganmaoyuan creates managed copies; it does not automatically move or delete source files. Search, duplicate/version relationships, audit history, and Undo keep file operations traceable.

### Resume from recorded project context

Project context brings together recorded work, currently valid actions, user decisions, and Git facts. Continue Work and the Project Context Packet distinguish known facts from suggestions, so an old summary is not presented as a confirmed current plan.

### Run a project-scoped Codex task

Create a project-bound task and, when configured, run it through the local Codex CLI. Result Bridge imports output for that task; Git verification and human acceptance provide additional evidence. A process exit alone does not mean a task is complete.

## Product walkthrough

The [30–45 second demo storyboard](docs/PROMO_DEMO_STORYBOARD.md) describes a real, concise walkthrough. No fabricated screenshots or unverified completion records are presented here.

## Quickstart

### Requirements

- Windows 10 or 11 and WebView2 Runtime
- Node.js 20+ and Rust 1.77.2+ to build from source
- Codex CLI installed and signed in only if you want to start local Codex tasks

The Releases page is the source of truth for downloadable builds. A locally built or draft package is not a public release; wait for a verified, published release before installing it as a user.

### Run from source

```bash
npm ci
npm run tauri:dev
```

### Validate and build

```bash
npm test
npm run build
cargo test --manifest-path src-tauri/Cargo.toml
npm run tauri -- build
```

## Current status and limitations

- The source is public. Windows packages are published only after installation and data-safety validation; check the Releases page for current availability.
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
