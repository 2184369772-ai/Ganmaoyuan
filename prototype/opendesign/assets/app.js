(function () {
  var themeKey = "ganmaoyuan-theme";
  var pageKey = "ganmaoyuan-last-page";
  var defaultTheme = document.documentElement.getAttribute("data-theme") || "dark";
  var theme = localStorage.getItem(themeKey) || defaultTheme;

  var utilityContent = {
    "switch-project": {
      eyebrow: "切换项目",
      title: "最近项目",
      body:
        '<div class="utility-list">' +
        '<button class="utility-item" type="button" data-action="show-toast" data-message="已切换到感冒院"><strong>感冒院</strong><span>当前任务：完善极简工作界面</span></button>' +
        '<button class="utility-item" type="button" data-action="show-toast" data-message="已切换到 WTS 研发人员项目工时报工系统"><strong>WTS 研发人员项目工时报工系统</strong><span>当前任务：处理测试问题与异常文件</span></button>' +
        '<button class="utility-item" type="button" data-action="show-toast" data-message="已切换到 Atlas"><strong>Atlas</strong><span>当前状态：暂停与冻结</span></button>' +
        "</div>",
      actions:
        '<button class="ghost-btn" type="button" data-action="close-utility-drawer">关闭</button>' +
        '<button class="btn btn-primary" type="button" data-action="show-toast" data-message="已打开新任务创建入口">开始新任务</button>',
    },
    inbox: {
      eyebrow: "待整理资料",
      title: "仅处理系统不确定的内容",
      body:
        '<div class="utility-list">' +
        '<article class="utility-item"><strong>需求最终版2.docx</strong><span>推荐项目：WTS · 推荐分类：原始需求 · 发现相似文件：需求最终版.docx</span></article>' +
        '<article class="utility-item"><strong>OpenDesign 资料汇总.pdf</strong><span>项目已识别为感冒院，但分类置信度偏低，建议人工确认。</span></article>' +
        '<article class="utility-item"><strong>旧版数据导出.xls</strong><span>2 个工作表读取失败，建议稍后重试或打开原文件检查。</span></article>' +
        "</div>",
      actions:
        '<button class="ghost-btn" type="button" data-action="close-utility-drawer">稍后处理</button>' +
        '<button class="btn btn-primary" type="button" data-action="show-toast" data-message="已批量接受高置信度结果">批量接受</button>',
    },
    library: {
      eyebrow: "全部资料",
      title: "当前任务相关入口",
      body:
        '<div class="utility-list">' +
        '<article class="utility-item"><strong>感冒院项目定义 v0.1</strong><span>DOCX · 定义产品定位、目标用户和第一版边界。</span></article>' +
        '<article class="utility-item"><strong>感冒院 v0.1 PRD</strong><span>DOCX · 梳理导入、整理、搜索与上下文生成流程。</span></article>' +
        '<article class="utility-item"><strong>用户工作流分析 v0.1</strong><span>PDF · 记录每天打开、继续、收工与恢复路径。</span></article>' +
        '<article class="utility-item"><strong>UI/UX 设计任务书</strong><span>Markdown · 约束本轮只收敛导航，不扩展新功能。</span></article>' +
        "</div>",
      actions:
        '<button class="ghost-btn" type="button" data-action="close-utility-drawer">关闭</button>' +
        '<a class="btn btn-primary" href="project-overview.html#materials">打开资料抽屉</a>',
    },
    memory: {
      eyebrow: "项目记忆",
      title: "最近决策与恢复点",
      body:
        '<div class="utility-list">' +
        '<article class="utility-item"><strong>默认启动页改为极简入口</strong><span>原因：用户打开后应先继续工作，而不是浏览状态。</span></article>' +
        '<article class="utility-item"><strong>正式页面只保留三页</strong><span>启动页、专注工作页、设置页，其余只保留兼容入口。</span></article>' +
        '<article class="utility-item"><strong>低频能力收进抽屉与命令面板</strong><span>待整理资料、搜索、AI 上下文和项目切换不再长期占位。</span></article>' +
        "</div>",
      actions:
        '<button class="ghost-btn" type="button" data-action="close-utility-drawer">关闭</button>' +
        '<button class="btn btn-primary" type="button" data-action="open-quick-record">记录新决定</button>',
    },
    search: {
      eyebrow: "搜索资料",
      title: "最近搜索示例",
      body:
        '<div class="utility-list">' +
        '<article class="utility-item"><strong>OpenDesign 提示词</strong><span>命中 UI/UX 设计任务书、项目记忆和历史工作记录。</span></article>' +
        '<article class="utility-item"><strong>感冒院为什么本地优先</strong><span>命中项目定义 v0.1 与 PRD 摘要。</span></article>' +
        '<article class="utility-item"><strong>审批流程</strong><span>命中 WTS 需求说明、现有流程图和人员权限表。</span></article>' +
        "</div>",
      actions:
        '<button class="ghost-btn" type="button" data-action="close-utility-drawer">关闭</button>' +
        '<button class="btn btn-primary" type="button" data-action="open-command">用 Ctrl + K 继续搜索</button>',
    },
    "ai-context": {
      eyebrow: "AI 上下文",
      title: "当前任务的上下文摘要",
      body:
        '<div class="utility-list">' +
        '<article class="utility-item"><strong>工具</strong><span>OpenDesign · 任务类型：UI 设计</span></article>' +
        '<article class="utility-item"><strong>包含内容</strong><span>产品定位、当前任务、UI 原则、明确不做事项、当前页面问题。</span></article>' +
        '<article class="utility-item"><strong>当前输出</strong><span>一份可复制给 OpenDesign / Codex 的标准上下文。</span></article>' +
        "</div>",
      actions:
        '<button class="ghost-btn" type="button" data-action="close-utility-drawer">关闭</button>' +
        '<button class="btn btn-primary" type="button" data-action="show-toast" data-message="已复制当前 AI 上下文">复制上下文</button>',
    },
  };

  document.documentElement.setAttribute("data-theme", theme);

  function qs(selector, root) {
    return (root || document).querySelector(selector);
  }

  function qsa(selector, root) {
    return Array.prototype.slice.call((root || document).querySelectorAll(selector));
  }

  function showToast(message) {
    var toast = qs("[data-toast]");
    if (!toast) return;

    var text = qs("[data-toast-text]", toast);
    if (text) {
      text.textContent = message;
    } else {
      toast.textContent = message;
    }

    toast.classList.add("show");
    window.clearTimeout(showToast._timer);
    showToast._timer = window.setTimeout(function () {
      toast.classList.remove("show");
    }, 1800);
  }

  function syncThemeLabels() {
    qsa("[data-theme-label]").forEach(function (node) {
      node.textContent = theme === "dark" ? "深色模式" : "浅色模式";
    });
  }

  function toggleTheme() {
    theme = theme === "dark" ? "light" : "dark";
    document.documentElement.setAttribute("data-theme", theme);
    localStorage.setItem(themeKey, theme);
    syncThemeLabels();
    showToast("主题已切换为" + (theme === "dark" ? "深色模式" : "浅色模式"));
  }

  function overlaySelectors() {
    return [
      "[data-side-panel]",
      "[data-materials-drawer]",
      "[data-utility-drawer]",
      "[data-wrap-panel]",
      "[data-command-palette]",
      "[data-quick-record]",
    ];
  }

  function openPanel(selector) {
    var node = qs(selector);
    if (!node) return;
    node.classList.add("open");
    node.setAttribute("aria-hidden", "false");
  }

  function closePanel(selector) {
    var node = qs(selector);
    if (!node) return;
    node.classList.remove("open");
    node.setAttribute("aria-hidden", "true");
  }

  function closeAllOverlays() {
    overlaySelectors().forEach(function (selector) {
      closePanel(selector);
    });
  }

  function ensureQuickRecord() {
    var existing = qs("[data-quick-record]");
    if (existing) return existing;

    var modal = document.createElement("div");
    modal.className = "quick-record";
    modal.setAttribute("data-quick-record", "");
    modal.setAttribute("aria-hidden", "true");
    modal.innerHTML =
      '<div class="record-card">' +
      '<div class="record-head"><h3>快速记录</h3><button class="icon-btn" type="button" data-action="close-quick-record" aria-label="关闭">×</button></div>' +
      '<textarea placeholder="记录想法、决定、问题或下一步……" rows="6"></textarea>' +
      '<div class="record-actions"><button class="ghost-btn" type="button" data-action="close-quick-record">取消</button><button class="btn btn-primary" type="button" data-action="save-record">保存并继续工作</button></div>' +
      "</div>";
    document.body.appendChild(modal);
    return modal;
  }

  function ensureCommandPalette() {
    var existing = qs("[data-command-palette]");
    if (existing) return existing;

    var modal = document.createElement("div");
    modal.className = "command-palette";
    modal.setAttribute("data-command-palette", "");
    modal.setAttribute("aria-hidden", "true");
    modal.innerHTML =
      '<div class="command-card">' +
      '<div class="command-search"><span>⌘</span><input type="text" placeholder="输入命令..." data-command-input /><kbd>Esc</kbd></div>' +
      '<div class="command-list" data-command-list>' +
      '<button class="command-item active" type="button" data-action="open-utility-drawer" data-utility-kind="switch-project"><strong>切换项目</strong><span>查看最近项目并切换</span></button>' +
      '<button class="command-item" type="button" data-action="open-utility-drawer" data-utility-kind="inbox"><strong>待整理资料</strong><span>处理低置信度结果与失败项</span></button>' +
      '<button class="command-item" type="button" data-action="open-utility-drawer" data-utility-kind="library"><strong>全部资料</strong><span>查看当前任务相关资料入口</span></button>' +
      '<button class="command-item" type="button" data-action="open-utility-drawer" data-utility-kind="memory"><strong>项目记忆</strong><span>查看最近决策与恢复点</span></button>' +
      '<button class="command-item" type="button" data-action="open-utility-drawer" data-utility-kind="search"><strong>搜索资料</strong><span>打开最近搜索与命中摘要</span></button>' +
      '<button class="command-item" type="button" data-action="open-quick-record"><strong>快速记录</strong><span>记录想法、决定、待办或问题</span></button>' +
      '<button class="command-item" type="button" data-action="open-utility-drawer" data-utility-kind="ai-context"><strong>AI 上下文</strong><span>生成给 OpenDesign / Codex 的上下文</span></button>' +
      '<button class="command-item" type="button" data-action="open-wrap-panel"><strong>今日收工</strong><span>保存恢复点并生成下一步</span></button>' +
      '<a class="command-item" href="settings.html"><strong>打开设置</strong><span>项目管理、文件索引、AI、外观与安全</span></a>' +
      "</div></div>";
    document.body.appendChild(modal);
    return modal;
  }

  function commandItems() {
    return qsa(".command-item");
  }

  function setActiveCommand(index) {
    var items = commandItems();
    items.forEach(function (item, itemIndex) {
      item.classList.toggle("active", itemIndex === index);
    });
  }

  function getActiveCommandIndex() {
    var items = commandItems();
    var index = items.findIndex(function (item) {
      return item.classList.contains("active");
    });
    return index < 0 ? 0 : index;
  }

  function moveCommand(delta) {
    var items = commandItems();
    if (!items.length) return;
    setActiveCommand((getActiveCommandIndex() + delta + items.length) % items.length);
  }

  function runActiveCommand() {
    var active = qs(".command-item.active");
    if (!active) return;
    if (active.tagName === "A" && active.href) {
      window.location.href = active.href;
      return;
    }
    active.click();
  }

  function openPalette() {
    ensureCommandPalette();
    openPanel("[data-command-palette]");
    setActiveCommand(0);
    var input = qs("[data-command-input]");
    if (input) {
      input.value = "";
      filterCommands("");
      window.setTimeout(function () {
        input.focus();
      }, 20);
    }
  }

  function filterCommands(value) {
    var keyword = (value || "").trim().toLowerCase();
    var items = commandItems();
    var firstVisible = -1;

    items.forEach(function (item, index) {
      var visible = !keyword || item.textContent.toLowerCase().indexOf(keyword) >= 0;
      item.style.display = visible ? "" : "none";
      item.classList.remove("active");
      if (visible && firstVisible < 0) firstVisible = index;
    });

    if (firstVisible >= 0) {
      items[firstVisible].classList.add("active");
    }
  }

  function openQuickRecord() {
    ensureQuickRecord();
    openPanel("[data-quick-record]");
  }

  function goBack(fallback) {
    if (window.history.length > 1) {
      window.history.back();
      return;
    }
    window.location.href = fallback || "workspace-home.html";
  }

  function rememberCurrentPage() {
    var page = document.body.getAttribute("data-page-id");
    if (page) localStorage.setItem(pageKey, page);
  }

  function setUtilityContent(kind) {
    var drawer = qs("[data-utility-drawer]");
    var content = utilityContent[kind];
    if (!drawer || !content) return;

    var eyebrow = qs("[data-utility-eyebrow]", drawer);
    var title = qs("[data-utility-title]", drawer);
    var body = qs("[data-utility-body]", drawer);
    var actions = qs("[data-utility-actions]", drawer);

    if (eyebrow) eyebrow.textContent = content.eyebrow;
    if (title) title.textContent = content.title;
    if (body) body.innerHTML = content.body;
    if (actions) actions.innerHTML = content.actions;
  }

  function openUtilityDrawer(kind) {
    closePanel("[data-side-panel]");
    setUtilityContent(kind);
    openPanel("[data-utility-drawer]");
  }

  function handleHashRoute() {
    if (document.body.getAttribute("data-page-id") !== "focus-work") return;

    var hash = (window.location.hash || "").replace(/^#/, "");
    if (!hash) return;

    window.setTimeout(function () {
      if (hash === "materials") openPanel("[data-materials-drawer]");
      if (hash === "wrap") openPanel("[data-wrap-panel]");
      if (hash === "command") openPalette();
      if (utilityContent[hash]) openUtilityDrawer(hash);
    }, 40);
  }

  function handleAction(target) {
    var action = target.getAttribute("data-action");

    if (action === "toggle-theme") toggleTheme();
    if (action === "go-back") goBack(target.getAttribute("data-fallback"));
    if (action === "open-command") openPalette();
    if (action === "close-command") closePanel("[data-command-palette]");
    if (action === "open-side-panel") openPanel("[data-side-panel]");
    if (action === "close-side-panel") closePanel("[data-side-panel]");
    if (action === "open-materials-drawer") openPanel("[data-materials-drawer]");
    if (action === "close-materials-drawer") closePanel("[data-materials-drawer]");
    if (action === "open-wrap-panel") openPanel("[data-wrap-panel]");
    if (action === "close-wrap-panel") closePanel("[data-wrap-panel]");
    if (action === "open-quick-record") openQuickRecord();
    if (action === "close-quick-record") closePanel("[data-quick-record]");
    if (action === "open-utility-drawer") openUtilityDrawer(target.getAttribute("data-utility-kind"));
    if (action === "close-utility-drawer") closePanel("[data-utility-drawer]");
    if (action === "save-focus-note") showToast("当前进展已记录");
    if (action === "save-record") {
      closePanel("[data-quick-record]");
      showToast("记录已保存到项目记忆");
    }
    if (action === "finish-day") {
      closePanel("[data-wrap-panel]");
      showToast("今日恢复点已保存，明天可直接继续");
    }
    if (action === "show-toast") showToast(target.getAttribute("data-message") || "操作已完成");
  }

  document.addEventListener("click", function (event) {
    var actionTarget = event.target.closest("[data-action]");
    if (actionTarget) {
      if (actionTarget.tagName !== "A") event.preventDefault();
      handleAction(actionTarget);
      return;
    }

    var overlay = event.target.closest(
      "[data-side-panel], [data-materials-drawer], [data-utility-drawer], [data-wrap-panel], [data-command-palette], [data-quick-record]"
    );
    if (!overlay) return;

    if (event.target === overlay) {
      closeAllOverlays();
    }
  });

  document.addEventListener("input", function (event) {
    if (event.target.matches("[data-command-input]")) {
      filterCommands(event.target.value);
    }
  });

  document.addEventListener("keydown", function (event) {
    if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "k") {
      event.preventDefault();
      openPalette();
      return;
    }

    if (qs("[data-command-palette].open")) {
      if (event.key === "ArrowDown") {
        event.preventDefault();
        moveCommand(1);
      }
      if (event.key === "ArrowUp") {
        event.preventDefault();
        moveCommand(-1);
      }
      if (event.key === "Enter") {
        event.preventDefault();
        runActiveCommand();
      }
    }

    if (event.key === "Escape") {
      closeAllOverlays();
    }
  });

  syncThemeLabels();
  rememberCurrentPage();
  ensureQuickRecord();
  ensureCommandPalette();
  handleHashRoute();
})();
