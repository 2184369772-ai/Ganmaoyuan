# 感冒院 Visual Redesign v1

## 1. Product Direction

感冒院采用“安静的纸面工作台”方向：暖中性底色、墨色文字、朱砂色动作、文档式层级和时间线式列表。它要让用户愿意每天打开，而不是让用户觉得自己进入了后台或调试台。

## 2. Core Journey

1. 打开应用，看到今天应该继续什么。
2. 点击开始工作，选择或恢复项目。
3. 在项目页先看到当前工作线程和下一动作。
4. 需要时查看最近变化、资料或交给 Codex。
5. 结果回流到项目记录，下一次打开仍能恢复。

## 3. Start Page

首屏顺序：项目问候与日期 → `开始工作` → 当前继续事项 → 最近变化 → 低优先级内容。

不使用 hero 卡片或 KPI。当前继续事项是带左侧活动标记的行组；已完成内容降低对比度，待处理内容使用朱砂动作标记。

状态：无项目时直接给出创建/导入项目；有项目但无可靠焦点时显示事实说明，不编造建议；加载中使用短句和骨架行；错误以内联反馈呈现。

## 4. Project Work Page

桌面布局：窄项目栏 / 主工作列 / 可收起参考栏。顶部只放项目名、当前位置和一项主要动作。主工作列最多占可用宽度的 65%，正文保持 720-860px。

参考栏默认显示最近变化和当前资料摘要，不同时展开 Codex、Ledger、Weekly Review。每个模块通过行列表达，展开时替换或插入内容，不叠加卡片。

## 5. Continue Work

Continue Work 是首页和项目页的同一事实投影。它必须显示：焦点标题、事实原因、当前状态、唯一动作、目标对象。点击后直接进入可处理对象，但不阻止项目切换。

## 6. Codex Console

当前任务区采用“任务标题 + 状态句 + 唯一动作”的 mission strip；生命周期为横向细线和六个节点；结果使用三段式：结论、关键结果、待处理。历史是两行以内的轻量列表。技术详情在页面末尾折叠。

analysis、coding、verification、fileOperation 使用同一结构，不以 Card 区分类型；只改变结果行标题与字段。

## 7. Inbox and Search

Inbox 每个文件是一条决策行：文件是什么 / 系统判断 / 下一步。Search 结果首先显示名称、项目和位置，再显示类型、生命周期或 duplicate/version 关系。路径、ID、原始原因在展开详情中。

## 8. Work Ledger

账本读起来像个人工作记录：时间、发生了什么、影响是什么。技术事件默认合并或隐藏；用户决定和人工验收拥有更高文本权重。

## 9. State Coverage

所有主要页面必须覆盖 loading、empty、error、disabled、success、needs-review。状态不依赖颜色，必须有中文标签和下一步说明。

## 10. Responsive Rules

- 1280x800：侧栏折叠，主工作列单列，参考栏转为抽屉。
- 1440x900：主工作列 + 单参考栏，任务控制区左右排列。
- 1920x1080：增加外侧留白与参考栏宽度，正文不拉长，历史列表不变成超长单行。

## 11. Implementation Order

1. token、字体、基础按钮/输入/列表行。
2. AppLayout 与 StartPage。
3. WorkPage 顶部、主工作列和参考栏。
4. Codex、Search、Inbox、Ledger、Settings 的同语言迁移。
5. 删除旧玻璃/发光 token，执行视觉和性能验收。

## 12. QA Checklist

- 首屏 3 秒内知道如何开始工作。
- 每个页面只有一个 primary action。
- 普通界面没有 taskId、runId、PID、resultPath、raw JSON 等技术字段。
- 1280/1440/1920 无横向滚动、双滚动或按钮漂移。
- 主动导航才改变滚动位置，被动刷新不抢滚动。
- loading/empty/error/disabled 都有明确文本。
- `npm test`、`npm run build`、`cargo test` 和桌面构建通过。
