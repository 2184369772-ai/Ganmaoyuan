---
project: Ganmaoyuan
register: product
aesthetic_direction: quiet editorial workbench
design_system: native React primitives + locked CSS tokens
design_variance: 8
motion_intensity: 2
visual_density: 5
---

## Design Read

Ganmaoyuan is a local-first continuity layer for one person's ongoing work. It should feel like a quiet desk: a place to reopen a project, recover context, make one decision, and continue. It is not an operations dashboard, chat wrapper, file browser, or IDE.

## Signature

The signature is a **paper trail on a workbench**: warm neutral canvas, ink-like type, one vermilion action color, and unboxed chronological rows. Important work is marked by rhythm and a small active rule, not by glowing containers.

## Product Posture

- The first question is always: what should I continue?
- One view has one primary action.
- Facts are quiet and attributable; suggestions never masquerade as facts.
- Technical details are available, but never the default reading path.
- A selected item may receive elevation; lists remain lists.

## Identity Lock

Every screen uses the same visual grammar:

- warm paper-like canvas
- charcoal ink text
- restrained terracotta action accent
- thin rules and selected underlines instead of boxed sections
- compact utility controls and generous reading rhythm
- no gradients, glass blur, glow, or decorative dashboard chrome

## Color Tokens

| role | value | use |
|---|---|---|
| canvas | `#F3F0EA` | app background |
| canvas-deep | `#EAE5DC` | navigation / quiet bands |
| surface | `#FBFAF7` | active work surface |
| surface-raised | `#FFFFFF` | selected item, dialog, focused result |
| ink | `#202522` | primary text |
| ink-soft | `#5F665F` | secondary text |
| ink-faint | `#8A9089` | metadata and disabled copy |
| rule | `#D8D2C8` | dividers and field rules |
| accent | `#B74E35` | primary actions, current step, links |
| accent-dark | `#8F3827` | hover / pressed accent |
| accent-wash | `#F3DED6` | selected action background |
| success | `#2E7258` | confirmed / completed |
| warning | `#9A6A20` | waiting / caution |
| danger | `#A63C38` | destructive / failed |

Rules:

- No gradient backgrounds or gradient buttons.
- Accent is for action and focus, not decoration.
- Status colors supplement text and icons; they never communicate meaning alone.
- Dark mode is not part of this redesign phase; the light workbench is the product identity.

## Typography

| role | family | size | weight |
|---|---|---:|---:|
| display | `"Source Serif 4", "Noto Serif SC", Georgia, serif` | 30-36px | 600 |
| section | `"Source Sans 3", "Noto Sans SC", sans-serif` | 18-22px | 650 |
| body | `"Source Sans 3", "Noto Sans SC", sans-serif` | 15px | 400 |
| label | `"Source Sans 3", "Noto Sans SC", sans-serif` | 12-13px | 650 |
| technical | `ui-monospace, Consolas, monospace` | 12px | 400 |

Display type is reserved for orientation and major conclusions. Chinese body copy gets 1.65 line height and a readable measure of 52-68 characters.

## Geometry

- spacing: 4 / 8 / 12 / 16 / 24 / 32 / 48
- normal radius: 6px
- control radius: 5px
- dialog radius: 10px
- border: 1px solid `var(--rule)`
- shadow: only dialogs and selected raised surfaces, `0 12px 28px rgba(35, 31, 24, .08)`
- content measure: 720-860px for prose; shell may remain wide

## Motion

- 120ms for hover and focus
- 180ms for route and panel transitions
- no idle animation, bounce, blur transition, or parallax
- respect `prefers-reduced-motion`
- do not animate layout height for long lists

## Page Grammar

### Start / Today

One orientation header, one clear `开始工作` action, then a quiet continuation list. Continue Work is the reason for the action, not a competing dashboard card.

### Project Work

Project identity and mode controls sit in a compact top rail. The central work thread owns the page. Context, recent changes, files, Codex and ledger are secondary rails or sections that open on demand.

### Codex

A mission strip answers task, stage, user action and result. Lifecycle is a thin progress line. Results read as conclusion, key outcomes, and next action. Technical details are an appendix.

### Inbox and Search

Use decision rows and search results, not tiles. Each row puts human meaning before metadata and offers one next action.

### Settings

Use a stable navigation column and a single document-like content column. Avoid cards inside settings cards.

## Component Rules

- `Button`: solid accent for one primary action; quiet text buttons for secondary actions.
- `Card`: exception-only for dialogs, selected preview, destructive confirmation, or one highlighted result.
- `Badge`: status and category metadata only; never an action.
- `Feedback`: inline and sentence-led; no giant alert panels for ordinary errors.
- `TextInput`: paper-white field with a clear bottom/outline focus state.
- list rows: border-bottom rhythm, selected row uses accent rule and soft wash.
- `details` / collapsible: technical evidence, raw paths, IDs, and diagnostics.

## Accessibility and Responsiveness

- visible `:focus-visible` ring in accent-dark with 2px offset
- keyboard order follows reading order; primary action is first actionable control
- all status colors have text labels
- 1280x800: collapse secondary rails; preserve work thread and primary action
- 1440x900: show main work column plus one secondary rail
- 1920x1080: widen shell, never stretch prose beyond its measure
- Windows scaling must preserve minimum 40px controls and avoid horizontal scroll

## Performance Guardrails

- render secondary modules only when opened or when they contain active work
- keep activity/history lists virtualized or capped
- pause polling while hidden; coalesce background refresh updates
- no large blur/filter surfaces
- preserve scroll position on passive refresh; only focus/scroll on explicit navigation

## Forbidden Patterns

- blue/purple glow or neon outlines
- glassmorphism, backdrop blur, gradient hero backgrounds
- three equal dashboard cards as a default layout
- nested cards and card-in-card hierarchy
- oversized marketing headlines
- technical identifiers in ordinary user-facing surfaces
- multiple competing primary buttons
