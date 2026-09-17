## Design Read

Ganmaoyuan should stop behaving like a patched dashboard and become a **single-person project work console**. The interface must feel resumable, inspectable, and calm under long sessions.

Bound to `/abs/path/.ulpi/design/DESIGN.md`.

### Current UI problem diagnosis

#### A. Must-fix layout problems

1. Start page still mixes one outer hero shell with inner card habits, so the eye does not know what is the primary action versus supporting context.
2. Work page still treats many accessory modules as equal to the conversation thread, so the interface feels busy before work starts.
3. Inbox still spends too much structure on internal metadata and too little on “what this file is / what happens next”.
4. Weekly Review still inherits dashboard habits instead of reading as a review document.
5. Settings uses section dividers and information blocks that still read like slices of a hidden card model.

#### B. Suggested optimizations

1. Remove box treatment from all rows that are fundamentally list items.
2. Reserve elevated surfaces only for selected, temporary, or risky actions.
3. Keep the accent for focus and actions only; do not use it as a decorative frame.
4. Turn “AI basis / evidence / reason” into expandable substructure, not first-glance dominance.

#### C. Do not change

1. Dark local-tool direction.
2. Project isolation, file management logic, Inbox logic, and AI evidence model.
3. The three-page shell: start, work, settings.

#### D. Can be solved globally by tokens / layout rules

1. Overuse of rounded boxes.
2. Divider weight.
3. Section spacing rhythm.
4. Status text weight and muted-text usage.
5. Hover/active movement consistency.

#### E. Requires page-level restructuring

1. Start page band hierarchy.
2. Work page topbar + thread + right rail relationship.
3. Inbox row composition.
4. Weekly Review narrative ordering.

---

## Flow: Start and Resume Work

### Overview

**Goal:** Let the user know what matters today within 3 seconds and begin work with one clear action.

**User Story:** As a returning project worker, I want to immediately see what to continue so that I can resume without decoding the interface.

**Trigger:** Open app.

### Entry Points

- [x] App launch - primary
- [x] Return from work page after finishing
- [x] Return from settings

### Prerequisites

- [x] Local app starts normally
- [x] Existing project data may or may not exist

### Steps

#### Step 1: Establish orientation

**Screen/Component:** Start page hero band

**User Action:**
- Read current state
- Decide whether to start/continue/import

**System Response:**
- Show one strong primary action
- Show a short truthful resume line
- Show today suggestions only if grounded in real evidence

**Transitions:**
- Start work → continue/new project flow
- Open inbox → Inbox flow
- Open settings → settings flow

#### Step 2: Pick the right branch

**Screen/Component:** Continue / new / inbox flows

**System Response:**
- Present branch-specific form only
- Keep original context visible through title and back action

### Edge Cases

| Scenario | Handling |
|----------|----------|
| No history | Do not fabricate suggestions; promote new project path |
| Bad history data | Suppress low-quality resume items from first-glance view |
| Pending inbox items | Surface count, not the full technical record |

### Accessibility Considerations

- Primary action is first keyboard-reachable CTA
- Start page must remain readable at 1280×800 without dense text stacks

### Acceptance Criteria

- [ ] User can identify the primary action in under 3 seconds
- [ ] Resume text never competes visually with suggestion metadata
- [ ] Suggestions read as rows, not mini cards

---

## Flow: Work Continuation

### Overview

**Goal:** Make the conversation thread feel like the core workspace and push all support material to secondary rails or expandable sections.

**User Story:** As a project worker, I want the work page to feel like one continuous workspace so that I stay focused on advancing the project, not decoding UI compartments.

### Entry Points

- [x] Open project from start page
- [x] Re-enter from settings
- [x] Resume after restart

### Steps

#### Step 1: Re-enter context

**Screen/Component:** Top runway + project restoration line

**System Response:**
- Show project identity, current mode, and a short recovery line
- Do not place multiple competing panels above the thread by default

#### Step 2: Work in thread

**Screen/Component:** Main conversation thread

**System Response:**
- Thread occupies visual priority
- AI/project understanding appears as one anchored contextual surface
- File results and evidence remain attached to the message rhythm

#### Step 3: Use supporting tools

**Screen/Component:** Right reference rail, top tool actions, bottom composer

**System Response:**
- Support surfaces remain subordinate to thread width and contrast
- Composer always feels docked and stable

### Edge Cases

| Scenario | Handling |
|----------|----------|
| No selected file | Right rail shows empty guidance, not a dead box |
| Weekly review open | Review reads as an inserted memo, not a new page fighting the thread |
| Authorization needed | Use one highlighted action band, not a heavy warning card |

### Acceptance Criteria

- [ ] Thread is visually dominant over every secondary surface
- [ ] Right rail feels like a reference rail, not a second page
- [ ] Top actions do not look like a segmented pill bar unless they are true mode toggles

---

## Flow: Inbox Triage

### Overview

**Goal:** Let the user understand a received file and the next safe action at first glance.

**User Story:** As a user importing material, I want to immediately understand what the system inferred and what I need to do next so that Inbox feels like triage, not raw logs.

### Steps

#### Step 1: Receive file

**Screen/Component:** Inbox list row

**System Response:**
- Show file name
- Show system judgment in human terms
- Show ownership / target / confidence as secondary chips
- Show next step as the most important supporting line

#### Step 2: Expand reasons only on demand

**Screen/Component:** Reason / evidence details

**System Response:**
- Keep technical and trace details collapsed
- Preserve user confidence without flooding the row

### Acceptance Criteria

- [ ] First-glance view answers what / why / next
- [ ] Internal field noise is hidden by default
- [ ] Each row reads as triage, not a mini dashboard

---

## Flow: Weekly Review

### Overview

**Goal:** Make review feel like a calm memo about work, not a statistics panel.

### Acceptance Criteria

- [ ] Lead with this week’s conclusion
- [ ] Evidence and skill candidates follow the conclusion
- [ ] Summary metrics are visually secondary

---

## Flow: Settings Administration

### Overview

**Goal:** Make settings feel structurally stable and readable as an admin workspace.

### Acceptance Criteria

- [ ] Sidebar width and content width feel constant across sections
- [ ] Content reads as one sectioned document, not a floating card with internal card fragments
- [ ] Administrative text blocks do not inherit dashboard card styling

---

## Component: StartHeroSurface

### Purpose

The unified entry surface that contains resume, primary CTA, focus projects, today suggestions, and pending attention.

### Variants

- `default`: existing history available
- `empty`: no trustworthy history
- `recovery-warning`: data health suppresses parts of resume

### States

| State | Visual | Behavior |
|-------|--------|----------|
| Default | one matte sheet with internal ruled bands | primary CTA prominent |
| Empty | calmer, more whitespace, no fake suggestion rows | promote new project |
| Warning | subtle warning text only, no giant alert box | fallback to safe resume |

### Responsive Behavior

| Breakpoint | Behavior |
|------------|----------|
| 1280×800 | two-column internal layout may collapse to single column if suggestion width becomes stressed |
| 1440×900 | two-column default |
| 1920×1080 | maintain readable text width, do not stretch suggestion text indefinitely |

### Accessibility

- Primary CTA first in focus order after global controls
- Suggestion rows are keyboard focusable only when actionable

### Acceptance Criteria

- [ ] No internal nested cards
- [ ] No decorative lines touching text
- [ ] Primary CTA is unmistakable

---

## Component: TodaySuggestionRow

### Purpose

A high-signal action row showing one recommended next action with reason and evidence.

### Variants

- `fact`
- `suggestion`
- `pending`

### States

| State | Visual | Behavior |
|-------|--------|----------|
| Default | ruled row with left content priority | explanation gets width priority |
| Hover | subtle surface tint + 1px lift | indicates clickability |
| Expanded | details flow below row | reveals trace/evidence |

### Edge Cases

| Scenario | Handling |
|----------|----------|
| Long title | wraps across lines; status compresses first |
| Long reason | clamp in collapsed state; full text on expand |
| No evidence | omit evidence line entirely |

### Acceptance Criteria

- [ ] Status never crushes the main sentence
- [ ] Row still reads well with long Chinese text
- [ ] Expanded details feel attached, not like a new card appearing underneath

---

## Component: WorkRunway

### Purpose

The stable top frame for project identity, core mode switches, and accessory actions.

### Variants

- `default`
- `tool-panel-open`

### Rules

- Left: project identity
- Center: only true work modes/toggles
- Right: utility actions
- No equal visual weight across every button
- No thick framed pill bar unless there are real mutually exclusive modes

### Acceptance Criteria

- [ ] User can tell which controls affect the workspace mode versus general utilities
- [ ] Bar reads as a runway, not a toolbar stuffed into a rounded capsule

---

## Component: ReferenceRail

### Purpose

Secondary rail for current reference files and one expanded selected file.

### Rules

- File list items are rows, not tiles
- Only the selected file may gain elevated surface treatment
- Rail should never visually compete with the thread

### Acceptance Criteria

- [ ] Reference list is scannable at a glance
- [ ] Expanded file detail is clearly selected, not one more repeated card

---

## Component: InboxDecisionRow

### Purpose

A receiving row that explains one inbound file and the next safe decision.

### Rules

- Order:
  1. file name
  2. human judgment
  3. next step
  4. secondary meta chips
  5. expandable reasoning

### Acceptance Criteria

- [ ] Technical state is never first-glance dominant
- [ ] The user understands the next action before opening details

---

## Component: SettingsSectionDocument

### Purpose

A stable section format for settings content.

### Rules

- Use one section header block
- Use content rows or field groups below
- Avoid boxed sub-panels unless there is destructive, preview, or result significance

### Acceptance Criteria

- [ ] No “card inside admin card” pattern
- [ ] Rows align consistently across sections
- [ ] Content width feels stable when switching sections

---

## Proposed Implementation Priorities

### Priority 1: structural fixes

1. Start page internal hierarchy
2. Today suggestion row layout
3. Work topbar runway
4. Settings section document model

### Priority 2: de-cardification pass

1. File list rows
2. Inbox decision rows
3. Weekly review memo flow
4. Codex / search / wrap inline panels

### Priority 3: interaction polish

1. Route transition flash audit
2. Panel open/close motion softening
3. Button emphasis normalization
4. Selected-state consistency

---

## Design Pre-Flight Result

### Identity lock

- [x] One accent, one radius logic, one type pairing, one motion scale
- [x] Every screen is specified as the same product family
- [x] Spec binds to `DESIGN.md`

### Anti-slop

- [x] No generic centered-dark-hero default in the target solution
- [x] No nested cards in the target solution
- [x] No purple glow / gradient text / fake premium chrome
- [x] Signature is explicit: continuous working sheet

### State & flow coverage

- [x] Start, work, inbox, weekly review, settings covered
- [x] Empty / warning / authorization / bad-history conditions accounted for

### Accessibility

- [x] Keyboard hierarchy and text-priority rules called out
- [x] Motion remains optional and restrained
- [x] Chinese long-line wrapping has explicit priority rules

### Layout craft

- [x] At least three layout families are specified:
  - start page banded sheet
  - work runway + thread + rail
  - admin section document

### Cognitive load

- [x] One primary action per view remains enforced
- [x] Secondary metadata is subordinated, not equal-weight

### Scored self-critique

| Axis | Score (0-4) | Note |
|------|-------------|------|
| distinctiveness | 3 | clear product-specific direction without decorative excess |
| hierarchy & focus | 4 | thread / CTA / next-action priorities are explicit |
| consistency with DESIGN.md | 4 | tightly bound |
| accessibility | 3 | strong textual and focus guidance; implementation still needs verification |
| state/edge coverage | 3 | key edge cases covered |
| copy quality | 3 | direct and operational |
| restraint | 4 | removes decorative box logic |
| motion motivation | 3 | restrained but not over-specified |

No axis is ≤ 2.

Revise-and-justify:

1. Changed the mental model from card collections to continuous surfaces because the current project keeps generating fragmentation whenever a new state is added.
2. Made status compress before explanatory text because long Chinese working sentences are the real primary content.
3. Reframed settings as a section document because admin stability matters more than decorative separation.

---

## Build handoff

- **Target engineering agent:** `react-vite-tailwind-engineer`
- **Design system:** Radix primitives + locked tokens
- **Framework note:** implement exactly this spec within the existing React + Vite + Tauri shell; do not redesign during build, and do not reintroduce nested cards as a convenience shortcut.

### Acceptance criteria for implementation

- [ ] Start page becomes one clear working sheet with internal section rhythm, not stacked internal cards
- [ ] Work page thread becomes visually dominant over all support modules
- [ ] Right reference rail becomes a secondary list rail with one selected-detail surface only
- [ ] Inbox first glance answers what / why / next without opening details
- [ ] Weekly Review reads as a memo, not a dashboard
- [ ] Settings becomes stable section-document layout with no card-in-card regression
- [ ] No visible copy uses decorative dashboard fragments to express hierarchy
- [ ] All new surfaces follow `/abs/path/.ulpi/design/DESIGN.md`
