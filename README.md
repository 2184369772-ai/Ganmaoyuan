# Ganmaoyuan 感冒院

## Pick up where you left off.

Ganmaoyuan is a local-first Windows workspace for people who work across project files, decisions, and code. Reopen a project, recover the latest recorded context, organize files safely, and continue with a task you can verify.

**Files, project context, and Codex tasks — connected in one traceable workflow.**

> **Product hero placeholder** — Replace this with an authentic screenshot from the current Windows build using sanitized demo data. No mockup is presented as a product screenshot.

<!-- Future embed: ![Ganmaoyuan desktop workspace](docs/media/ganmaoyuan-hero.png) -->

[Explore the source](https://github.com/2184369772-ai/Ganmaoyuan) · [Architecture](docs/ARCHITECTURE.md) · [Security](SECURITY.md) · [Releases](https://github.com/2184369772-ai/Ganmaoyuan/releases)

## What you can do

### Organize files without losing the originals

Bring files into Inbox or scan a workspace. Review their classification and proposed location, then approve a cleanup plan. Ganmaoyuan creates managed copies; it does not automatically move or delete source files. Search, duplicate/version relationships, audit history, and Undo help keep the process traceable.

> **Feature image placeholder — File organization.** Planned real capture: Inbox or scan results, a reviewed location proposal, and the safe-copy/Undo boundary. Use sanitized filenames and paths.

<!-- Future embed: ![Review a safe file organization plan](docs/media/file-organization.png) -->

### Return to a project with its recorded context

Project state is assembled from recorded work, current actions, user decisions, and Git facts. Continue Work and the Project Context Packet help you see what is known without treating an AI suggestion or an old summary as a confirmed fact.

> **Feature image placeholder — Project continuity.** Planned real capture: a project workspace showing recent recorded activity, current valid actions, and its context packet.

<!-- Future embed: ![Recover project context](docs/media/project-continuity.png) -->

### Hand a scoped task to local Codex CLI

Create a project-bound Codex task and, when configured, run it through the local Codex CLI. Result Bridge imports task-bound output; Git verification and a human acceptance step provide additional evidence. A process exit alone is not treated as task completion.

> **Feature image placeholder — Codex task.** Planned real capture: a genuine task and its actual result/acceptance state. Do not use seeded success records or imply an unverified run completed.

<!-- Future embed: ![Review a Codex task result](docs/media/codex-task.png) -->

## Demo

> **30–45 second demo placeholder** — Record a real desktop walkthrough with a sanitized project: reopen a recent project → review its current context and recent facts → find a reference file → inspect a real Codex task/result state. Show result acceptance only after a genuine end-to-end run has been verified.

<!-- Future embed: ![Ganmaoyuan product demo](docs/media/ganmaoyuan-demo.mp4) -->

## Quickstart

### Requirements

- Windows 10 or 11 and WebView2 Runtime
- Node.js 20+ and Rust 1.77.2+ to build from source
- Codex CLI installed and signed in only if you want to start local Codex tasks

There is no verified Windows installer or downloadable release yet. The Releases link above is an entry point for a future verified package, not a claim that one is currently available.

### Run from source

```bash
npm install
npm run tauri:dev
```

### Validate and build

```bash
npm test
npm run build
cargo test --manifest-path src-tauri/Cargo.toml
npm run tauri build
```

## Current status and limitations

- The source is public and the Windows desktop executable has been built locally; a verified installer and GitHub Release have not been published.
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
