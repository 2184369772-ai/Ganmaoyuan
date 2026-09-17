# Project Context Packet v1

## Intent

Ganmaoyuan is a local-first continuity workspace. This feature produces a
bounded, factual project handoff that a person can paste into an external
conversation or an agent task without re-explaining the project from memory.
It is a projection, never a second project record and never an AI-generated
status report.

Every screen must read as the same product if placed side by side.

## User Flow: Copy Project Context

**Goal:** Prepare the minimum trustworthy context needed to continue a project
outside Ganmaoyuan.

1. The user opens a project work page.
2. The user selects `复制项目上下文` from the workflow tools.
3. Ganmaoyuan derives a packet from current projections and opens a compact
   preview sheet.
4. The user reads the current focus, real activity, pending actions, relevant
   file references, and the latest Codex outcome.
5. The user copies the packet. No project state changes and no packet history
   is persisted.

If no reliable facts exist, the preview says so explicitly and copies only the
project identity plus the absence of evidence. It never invents a next step.

## Information Contract

The packet has five quiet sections, in this order:

1. `当前焦点`: Continue Work Focus, including its factual reason and evidence
   labels.
2. `最近事实`: up to six user-visible Activity Timeline entries.
3. `待你处理`: up to five open PendingAction entries.
4. `相关资料`: up to five FileProjection references with name, document
   purpose, lifecycle, and a workspace-relative or project-relative location.
5. `最近 Codex 结果`: one current or latest task result, limited to the
   user-facing summary and acceptance items.

The packet includes a generation time and a privacy note. It does not include
full file content, source paths, repository paths, task IDs, run IDs, prompts,
result JSON, stdout, stderr, credentials, or raw technical traces.

## UI Composition

- Entry: one ghost workflow action, `复制项目上下文`, adjacent to existing
  workflow tools. It is a secondary action and never competes with the current
  task action.
- Preview: one temporary elevated sheet in the existing top-panel region. It
  is a reading surface, not a nested-card grid.
- Header: title, one short privacy note, `复制到剪贴板`, and `关闭`.
- Body: ruled sections, with title and reading text; no status-card wall.
- Empty state: one plain sentence describing which factual sources are absent.

## Component Mapping

| Need | Existing component or primitive |
| --- | --- |
| Entry and copy action | `Button` ghost / primary |
| Privacy and freshness status | `Badge` |
| Load and copy failure | `Feedback` |
| Preview surface | Existing top-panel section pattern |
| Bounded details | Native `details` only when evidence needs expansion |

No new button, card, badge, token, or layout system is introduced.

## States

| State | User sees | Action |
| --- | --- | --- |
| Loading | 正在整理当前项目事实 | Wait; copy is disabled |
| Ready | Five factual sections, where available | 复制到剪贴板 |
| Sparse | 当前项目可用事实较少 | 复制现有事实 |
| Copy failed | 无法写入剪贴板 | Retry without clearing preview |
| Backend failed | 无法读取项目上下文 | Retry; existing page remains usable |

## Privacy Rules

- The Rust projection is allow-list based. It only emits selected user-facing
  text and relative locations.
- Values matching secret markers are redacted before formatting.
- Full prompts, source paths, managed absolute paths, raw file content, Git
  output, result artifacts, and runner diagnostics are excluded by design.
- A packet is derived in memory and not written to project data.

## Responsive Rules

- `>= 1440`: preview text column is capped at 820px while the sheet can use the
  existing work-frame width.
- `1280`: action row may wrap beneath the title; packet remains a single
  column.
- `1920`: reading column does not expand beyond its cap; surrounding space is
  intentionally quiet.
- No fixed-height containers, nested scrolling, negative margins, or transforms
  are permitted.

## Engineering Handoff

- Add a read-only `get_project_context_packet(projectRoot)` Tauri command.
- Build the payload solely from `ProjectManifest`, `WorkLedgerSnapshot`,
  `FileProjectionAdapter`, `PendingActionAdapter`, and existing Codex task
  records.
- Add the TypeScript API type and command wrapper.
- Add one work-page panel and clipboard handler. Do not change Today, Inbox,
  Search, Codex state, or storage schemas.
- Test project isolation, bounded output, secret/path redaction, sparse facts,
  and stable packet formatting.

## Preflight

- Locked Design.md re-read: yes.
- New tokens or visual language: none.
- Nested cards: zero.
- One primary action in preview: `复制到剪贴板`.
- Technical details and source file contents in normal preview: zero.
- Keyboard: entry, copy, and close are reachable with Tab/Enter; visible focus
  remains from the existing Button system.
