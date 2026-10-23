# 感冒院 · 工作台原型笔记

## 源信息
- 参考站点：
  - https://linear.app/
  - https://affine.pro/
  - https://twenty.com/
  - https://appflowy.io/
  - https://plane.so/
  - https://www.raycast.com/
  - https://openwebui.com/
  - https://lobehub.com/
- 当前交付模式：视觉复刻 + 内容爆改
- 当前目标：为“感冒院”设计一套桌面端优先、可继续交给 Codex 开发的高完成度 Web 应用原型

## 复杂度预判
- 复杂度等级：L3
- 推荐模式：内容爆改
- 可高保真部分：桌面应用壳层、导航、列表、详情抽屉、设置中心、命令面板、导入流程
- 近似处理部分：真实文件系统读取、AI 服务联通、跨文件全文索引、真实拖拽与后台任务
- 本次不实现：文件读写、模型调用、系统托盘、Tauri 原生桥接、数据库

## 设计系统
- 方向：`tech-utility`
- 默认主题：深色
- 强调色：低饱和绿色，用于“已完成 / 可继续 / 当前选择”
- 字体策略：系统无衬线 + Mono，保障工程感和 Windows 桌面可读性

## 页面清单
- `index.html`：原型导航总览
- `onboarding.html`：初次启动页
- `workspace-home.html`：工作台首页
- `project-setup.html`：新建项目流程
- `import-sources.html`：文件与文件夹导入页
- `import-analysis.html`：导入扫描与分析页
- `import-review.html`：导入结果确认页
- `project-overview.html`：项目首页
- `project-library.html`：项目资料中心 + 文件详情侧栏
- `version-groups.html`：重复与版本关系页
- `project-search.html`：项目内搜索页
- `global-search.html`：全局搜索页
- `ai-context.html`：AI 项目上下文生成页
- `project-memory.html`：项目记忆页
- `settings.html`：设置页
- `states-gallery.html`：空 / 加载 / 错误 / 路径失效 / AI 未配置 / AI 处理中状态
- `design-system.html`：基础组件与 token 展示

## 可继续开发建议
1. 先把 `assets/app.css` 里的 token 和组件拆成真正的设计系统层。
2. 用真实路由框架承接当前每个 HTML 页面，保持 URL 与信息架构不变。
3. 优先补导入分析、资料中心、搜索与 AI 上下文四条核心链路的数据状态。
4. 再接 Tauri、本地索引、数据库和 AI 服务。

## 验证
- 已完成：多页面原型结构、深浅主题切换、命令面板、关键状态页、统一组件基线
- 未完成：真实后端联动、文件系统权限、数据持久化、全文索引、真实搜索命中高亮
