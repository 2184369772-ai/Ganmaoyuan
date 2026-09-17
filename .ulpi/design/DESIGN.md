---
project: Ganmaoyuan
register: product
aesthetic_direction: technical / utilitarian
color_strategy: restrained
design_system: Radix primitives + locked tokens
design_variance: 6
motion_intensity: 4
visual_density: 6
---

## Design Read

Calm, local, continuous. Ganmaoyuan must feel like a trustworthy working console that lets one person resume, inspect, decide, and continue without visual noise or dashboard theater.

## Signature

The product is remembered by one move: a **continuous working sheet**. Instead of stacked cards, each main screen reads like one large matte surface with quiet ruled sections, soft depth, and one luminous accent used only for action or focus. The signature fits the brief because Ganmaoyuan is a long-running workbench, not a showcase or analytics wall.

## Inspiration

- took: Codex-like calm density, softened controls, continuous conversation-first working area, restrained action emphasis.
- rejected: repeated nested cards, hard dividers cutting sections into boxes, bright neon accents competing with content.

Synthesis note: Ganmaoyuan should inherit the emotional calm of a serious developer tool, but its Chinese project-workflow context requires clearer labels, stronger next-step guidance, and more visible evidence structure.

## Identity Lock

Every screen must read as the same product if placed side by side.

### Product posture

- This is a **local-first desktop work console**.
- It is **not** a dashboard, marketing site, file manager clone, or chat toy.
- The interface should always answer: where was I, what matters now, what is the next safe action.

### Surface model

- Default container is **continuous content on one surface**.
- Cards are exception-only. Use them only for:
  - modal or transient confirmation
  - one selected file preview
  - one AI result needing temporary emphasis
  - destructive or permission-sensitive action blocks
- Never nest a card inside a card.
- Never solve hierarchy by adding another border box.

## Color (locked)

| role | OKLCH | hex | use |
|------|-------|-----|-----|
| background | 0.18 0.012 248 | #141A23 | app canvas |
| surface | 0.23 0.014 246 | #1E2630 | main matte working surface |
| surface-elevated | 0.27 0.017 243 | #273240 | selected or temporary emphasis |
| surface-soft | 0.31 0.020 239 | #324050 | hover / subtle action |
| text-primary | 0.965 0.006 232 | #F2F6FB | primary copy |
| text-muted | 0.77 0.016 230 | #B6C2D0 | secondary copy |
| divider | 0.38 0.018 235 | #4E5A6B | hairline dividers |
| accent | 0.84 0.055 225 | #A9D7FF | single active accent |
| accent-strong | 0.90 0.045 223 | #CAE6FF | focused text / selected emphasis |
| success | 0.80 0.133 167 | #63D7A9 | success |
| warning | 0.83 0.148 84 | #F3C85C | warning |
| danger | 0.74 0.180 29 | #EA6C58 | destructive |

Rules:

- One accent only: mist blue. No purple glow, no green-as-brand, no gradient text.
- 60-30-10 distribution:
  - 60 = dark neutral surfaces
  - 30 = softened elevated neutrals
  - 10 = accent and state colors
- Borders are almost never full rectangles. Prefer bottom rules, section rules, or inset focus rings.
- Contrast targets:
  - primary text on background/surface: WCAG AA+
  - muted text only for secondary information, never for main action labels

## Type (locked)

| role | family | use | notes |
|------|--------|-----|-------|
| display | Inter / system sans | page title, major section title | bold, tight tracking, used sparingly |
| body | Inter / system sans | all reading content | 15–16px body, 1.65–1.8 line-height |
| utility | ui-monospace, SFMono-Regular, Consolas, monospace | paths, identifiers, traces | only for technical content |

Rules:

- No decorative serif.
- No tiny dashboard microtype for meaningful content.
- Chinese copy should avoid cramped lines; body paragraphs target relaxed line-height over dense compression.

## Scales (locked)

### Spacing

- 4 / 8 / 12 / 16 / 20 / 24 / 32 / 40
- Section rhythm is driven by 24 / 32, not 12 / 16.
- If a screen feels busy, increase section spacing before adding boxes.

### Radius

- 0 for normal section containers
- 12 for controls
- 18 for secondary soft surfaces
- 24 for selected / focused / composer surfaces

### Motion

- Duration: 120 / 180 / 260ms
- Easing: cubic-bezier(0.22, 0.84, 0.24, 1)
- Motion use:
  - route / panel reveal = fade + 6px rise
  - button / row interaction = 1px lift max
  - panel open/close = opacity + translate only
- No bounce, no elastic easing, no decorative idle motion
- Must respect `prefers-reduced-motion`

## Voice

- register: plain, assured, work-oriented
- action vocabulary:
  - 开始工作
  - 继续工作
  - 导入资料
  - 查看原因
  - 确认归位
  - 保存恢复点
- Never use theatrical copy, buzzwords, or “AI has done magic” phrasing.

## Page Grammar (locked)

### 1. Start / Today Workspace

- One main surface
- Three internal bands:
  - resume + primary action
  - focus projects
  - today suggestions + pending
- Suggestions are rows, not tiles.

### 2. Work page

- One horizontal work frame:
  - top runway bar
  - left main thread
  - right reference rail
  - bottom composer dock
- Thread is the primary artifact. All other modules are accessory.

### 3. Inbox

- One receiving sheet
- Each file is a decision row:
  - what it is
  - why the system thinks so
  - what happens next
- No boxed metadata grid unless user expands details.

### 4. Weekly Review

- Read like a personal review memo, not operations dashboard.
- Lead with conclusions, then evidence, then candidates.

### 5. Settings

- True admin skeleton:
  - stable sidebar
  - stable content width
  - content presented as sections and rows, not floating cards

## Banned patterns

- three equal cards as the default answer
- nested cards
- hard left decorative line touching text
- thick borders used as hierarchy
- KPI strip styling for content that is actually narrative
- hero-center-dark-mesh as default visual answer
- bright neon accent as the main style device
- fake app-window look inside the real app

## Build consequences

- If a container has no independent state or separate task meaning, it should not be a card.
- If two adjacent boxes exist only because spacing felt empty, remove the boxes and use rhythm instead.
- If a row contains title + status + explanation, the explanation always wins width; status compresses first.
- If a settings block feels cut into fragments, unify it into one reading flow before adding controls.
