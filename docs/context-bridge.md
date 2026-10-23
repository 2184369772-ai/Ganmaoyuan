# Ganmaoyuan Context Bridge v1

`Ganmaoyuan Context Bridge` lets an MCP-compatible local agent read the factual,
privacy-bounded context for one explicitly selected Ganmaoyuan project.

It is deliberately read-only. It does not scan the computer, write project
state, run Codex, read original file bodies, or return absolute paths, prompts,
technical logs, or credentials.

## Scope

Start the dedicated console Bridge executable with exactly one project root:

```text
ganmaoyuan-context.exe --context-bridge --project-root <project-root>
```

`ganmaoyuan-context.exe` is built alongside the desktop app at
`src-tauri/target/release/ganmaoyuan-context.exe`. It intentionally has no GUI
window and exists only to provide a reliable local MCP stdio connection.

The root must already contain Ganmaoyuan project data. One bridge process can
read only that one project; a client cannot use the bridge to enumerate other
projects or arbitrary local files.

## Read-only tools

- `get_project_state`
- `get_continue_work_focus`
- `get_recent_activity`
- `get_pending_actions`
- `get_codex_task_result`

All tool results are derived from the existing `ProjectContextPacket`, which
already uses the Activity Timeline, PendingAction and Codex task projections.
They are summaries, not original source documents.

## Codex setup example

Use the installed executable path and a project root that you explicitly want
to share with the local agent. Do not place credentials in the command.

```text
codex mcp add ganmaoyuan-context -- <ganmaoyuan-context.exe path> --context-bridge --project-root <project-root>
```

The configuration is intentionally per selected project. Change or remove the
MCP entry when switching projects rather than granting an agent broad access to
all Ganmaoyuan data.

## Boundary

The bridge is a local stdio process. It does not open a network port and does
not send anything by itself. The MCP client remains responsible for asking the
user before transmitting any returned context to a remote model or service.
