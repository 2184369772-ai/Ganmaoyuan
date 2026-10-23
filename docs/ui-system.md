# Ganmaoyuan UI System v0.1

本规范用于整理感冒院 v1 之后的界面结构。当前阶段只统一架构和约束，不重新设计视觉风格。

## Design Tokens

所有页面优先使用 `src/styles/global.css` 中的语义 token，不在页面组件里写局部布局常量。

- 页面背景：`--bg`
- 主卡片：`--surface`
- 次级卡片：`--surface-2`
- 悬浮/强调面：`--surface-3`
- 主文字：`--fg`
- 次级文字：`--muted`
- 边框：`--border`
- 荧光蓝强调：`--accent`、`--accent-strong`
- 圆角：`--radius-sm`、`--radius-md`、`--radius-lg`
- 阴影：`--shadow-low`、`--shadow-mid`
- 间距：`--space-1` 到 `--space-10`
- 页面外边距：`--page-gutter`、`--page-gutter-start`
- 内容宽度：`--content-sm`、`--content-md`、`--content-lg`、`--content-xl`
- 表单宽度：`--form-width`、`--form-wide-width`
- 按钮高度：`--control-height`、`--control-height-lg`
- 侧栏宽度：`--left-sidebar-width`、`--workspace-sidebar-width`、`--settings-sidebar-width`

## App Layout

所有正式页面复用 `src/components/AppLayout.tsx`。

- `Sidebar`：左侧日常入口或设置后台菜单。
- `Header`：页面标题、项目名和主要导航动作。
- `Main Content`：页面主体，允许页面内部独立滚动。
- `Modal`：弹层后续统一接入同一层级，不在页面里随意绝对定位。

页面可以保留自己的业务组件，但不应重新定义一套页面壳。

## Page Rules

- 首页：中央入口和 Today Workspace 保持极简，左侧栏默认隐藏。
- Today Workspace：展示事实、建议、推测时必须区分来源和可信度。
- Inbox：列表和处理卡片在主内容内滚动，顶部不能被滚动容器裁掉。
- 项目工作台：整页铺满窗口；右侧文件区使用 `--workspace-sidebar-width`；聊天内容保持阅读宽度。
- AI 助手：回答依据使用现有 evidence 组件，不单独做新卡片样式。
- Weekly Review：复用项目工作台内的 panel 样式，不做复杂 Dashboard。
- 设置页：固定为左侧菜单加右侧内容区，低频功能只放这里。

## Responsive Rules

必须覆盖以下桌面尺寸：

- 1280 x 800
- 1440 x 900
- 1920 x 1080

规则：

- 页面外层使用 `width: 100%`、`max-width: none`、`min-height: 100dvh`。
- 内容区使用 `minmax(0, 1fr)` 防止 Grid 子元素撑破。
- 长文本、路径和文件名使用 `overflow-wrap: anywhere`。
- 不用 `overflow: hidden` 遮住布局问题；只在页面根上阻止全局滚动，具体内容区自己滚动。
- 不在页面组件里新增 `100vw` 宽度计算。
- 不使用负 margin 和 transform 作为主布局定位手段。

## Cleanup Rules

后续 UI 美化前先检查：

- 是否已有 token 能表达该尺寸或颜色。
- 是否已有 `.btn`、`.icon-btn`、`.settings-menu-item`、`.file-result`、`.restore-note` 可复用。
- 是否把布局写在页面业务组件里。
- 是否引入了新的固定宽度、临时 padding、负 margin 或绝对定位。

## Component Foundation

统一组件放在 `src/components/ui/`。新 UI 优先使用这些组件；旧页面 class 继续兼容，但不再新增页面私有按钮、卡片、输入框和状态样式。

### Button

组件：`Button`

变体：

- `primary`：主操作，例如确认、发送、创建。
- `secondary`：默认操作，例如返回、打开、刷新。
- `danger`：删除、移除、不可逆或高风险操作。
- `ghost`：低强调入口。

状态：

- `disabled` 使用原生 disabled。
- `loading` 显示统一 spinner，并自动禁用按钮。

### Card

组件：`Card`

变体：

- `default`：普通卡片。
- `info`：信息卡片。
- `status`：状态卡片。
- `ai`：AI 结果卡片。

旧类 `.file-result`、`.restore-note`、`.global-search-result` 继续保留，但样式来源应逐步向 `ui-card` 收敛。

### Input

组件：`TextInput`、`TextArea`

变体：

- `text`：普通文本输入。
- `search`：搜索框。
- `form`：表单输入。
- `file`：文件选择状态。

输入框必须有统一 focus、error、hint 表达。

### Badge / Status

组件：`Badge`

状态：

- `success`
- `warning`
- `error`
- `pending`
- `high`
- `medium`
- `low`
- `neutral`

confidence 统一使用 `confidenceTone(level)` 映射。

### Empty State

组件：`EmptyState`

场景：

- `data`：无数据。
- `search`：无搜索结果。
- `project`：无项目。
- `pending`：无待处理事项。

### Loading / Error

组件：`Feedback`

状态：

- `info`
- `success`
- `warning`
- `error`
- `loading`

页面不再新增单独的错误颜色和空状态样式。
