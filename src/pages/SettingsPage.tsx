import { useEffect, useMemo, useState, type ReactNode } from "react";
import { useNavigate } from "react-router-dom";
import { type SettingsSection, useAppState } from "../app/AppState";
import { AppLayout } from "../components/AppLayout";
import { EmptyState } from "../components/ui";
import {
  chooseDirectory,
  createLocalBackup,
  exportProjectSafe,
  generatePrivacyArtifacts,
  getWorkspaceConfig,
  initializeWorkspaceRoot,
  listWorkspaceScanBatches,
  migrateProjectRoot,
  restoreLocalBackup,
  scanDesktopDirectory,
  scanDownloadsDirectory,
  scanWorkspaceDirectory,
  verifyWorkspaceRoot,
  type BackupRestoreResult,
  type LocalBackupResult,
  type PendingReviewItem,
  type PrivacyArtifactsResult,
  type ProjectMigrationResult,
  type WeeklyReportRecord,
  type WeeklyReviewDashboard,
  type WeeklyReviewSettings,
  type SafeExportResult,
  type WorkspaceConfig,
  type WorkspaceScanBatch,
} from "../features/project/desktopApi";

const menuItems: Array<{ key: SettingsSection; label: string }> = [
  { key: "general", label: "常规" },
  { key: "projects", label: "项目管理" },
  { key: "tasks", label: "任务管理" },
  { key: "materials", label: "资料管理" },
  { key: "pending-materials", label: "待整理资料" },
  { key: "file-index", label: "文件索引" },
  { key: "project-memory", label: "项目记忆" },
  { key: "ai-context", label: "AI 上下文" },
  { key: "ai-models", label: "AI 模型" },
  { key: "data-backup", label: "数据与备份" },
  { key: "weekly-review", label: "每周复盘" },
  { key: "privacy-security", label: "隐私安全" },
  { key: "appearance", label: "外观" },
  { key: "advanced", label: "高级设置" },
];

type SettingsRenderData = {
  theme: "dark" | "light";
  toggleTheme: () => void;
  currentTask: string;
  nextStep: string;
  materials: Array<{ title: string; detail: string }>;
  pendingMaterials: Array<{ title: string; detail: string }>;
  pendingReviews: PendingReviewItem[];
  updatePendingReviewStatus: (reviewId: string, status: "open" | "pending" | "resolved" | "ignored") => Promise<void>;
  projectMemory: string[];
  aiContextItems: string[];
  deepSeekSettings: { hasApiKey: boolean; selectedModelId: string; lastTestedAt: string };
  deepSeekModels: Array<{ id: string; ownedBy: string }>;
  weeklyReviewDashboard: WeeklyReviewDashboard | null;
  weeklyReviewSettings: WeeklyReviewSettings | null;
  weeklyReports: WeeklyReportRecord[];
  autoRouteHighConfidence: boolean;
  saveInboxAutoRouteSetting: (enabled: boolean) => Promise<void>;
  apiKey: string;
  selectedModelId: string;
  setApiKey: (value: string) => void;
  setSelectedModelId: (value: string) => void;
  saveDeepSeekApiKey: () => Promise<void>;
  deleteDeepSeekApiKey: () => Promise<void>;
  testDeepSeekConnection: () => Promise<void>;
  refreshWeeklyReviewDashboard: () => Promise<void>;
  generateWeeklyReviews: (force?: boolean) => Promise<WeeklyReviewDashboard | null>;
  saveWeeklyReviewGenerationWeekday: (weekday: number) => Promise<void>;
  isSavingKey: boolean;
  isTesting: boolean;
  isScanningWorkspace: boolean;
  currentProjectName: string;
  currentProjectRoot: string;
  projectOptions: string[];
  workspaceConfig: WorkspaceConfig | null;
  workspaceScanBatches: WorkspaceScanBatch[];
  initializeWorkspace: () => Promise<void>;
  verifyWorkspace: () => Promise<void>;
  scanDesktop: () => Promise<void>;
  scanDownloads: () => Promise<void>;
  scanCustomDirectory: () => Promise<void>;
  isDesktopReady: boolean;
  backupDestination: string;
  restoreSource: string;
  restoreDestination: string;
  migrationDestination: string;
  exportDestination: string;
  setBackupDestination: (value: string) => void;
  setRestoreSource: (value: string) => void;
  setRestoreDestination: (value: string) => void;
  setMigrationDestination: (value: string) => void;
  setExportDestination: (value: string) => void;
  pickBackupDestination: () => Promise<void>;
  pickRestoreSource: () => Promise<void>;
  pickRestoreDestination: () => Promise<void>;
  pickMigrationDestination: () => Promise<void>;
  pickExportDestination: () => Promise<void>;
  runBackup: () => Promise<void>;
  runRestore: () => Promise<void>;
  runMigration: () => Promise<void>;
  runSafeExport: () => Promise<void>;
  runPrivacyArtifacts: () => Promise<void>;
  isRunningBackupAction: boolean;
  backupResult: LocalBackupResult | null;
  restoreResult: BackupRestoreResult | null;
  migrationResult: ProjectMigrationResult | null;
  exportResult: SafeExportResult | null;
  privacyResult: PrivacyArtifactsResult | null;
  actionMessage: string;
  openFolderPath: (path: string) => Promise<void>;
};

export function SettingsPage() {
  const navigate = useNavigate();
  const {
    theme,
    toggleTheme,
    selectedSettingsSection,
    setSelectedSettingsSection,
    currentTask,
    nextStep,
    materials,
    pendingMaterials,
    pendingReviews,
    projectMemory,
    aiContextItems,
    deepSeekSettings,
    deepSeekModels,
    weeklyReviewDashboard,
    weeklyReviewSettings,
    weeklyReports,
    projects,
    activeProject,
    refreshProjects,
    openProject,
    isDesktopReady,
    openFolderPath,
    saveDeepSeekApiKey,
    deleteDeepSeekApiKey,
    testDeepSeekConnection,
    refreshWeeklyReviewDashboard,
    generateWeeklyReviews,
    saveWeeklyReviewGenerationWeekday,
    inboxRoutingSettings,
    saveInboxAutoRouteSetting,
    updatePendingReviewStatus,
    setError,
  } = useAppState();
  const [apiKey, setApiKey] = useState("");
  const [selectedModelId, setSelectedModelId] = useState(deepSeekSettings.selectedModelId);
  const [isSavingKey, setIsSavingKey] = useState(false);
  const [isTesting, setIsTesting] = useState(false);
  const [backupDestination, setBackupDestination] = useState("");
  const [restoreSource, setRestoreSource] = useState("");
  const [restoreDestination, setRestoreDestination] = useState("");
  const [migrationDestination, setMigrationDestination] = useState("");
  const [exportDestination, setExportDestination] = useState("");
  const [isRunningBackupAction, setIsRunningBackupAction] = useState(false);
  const [backupResult, setBackupResult] = useState<LocalBackupResult | null>(null);
  const [restoreResult, setRestoreResult] = useState<BackupRestoreResult | null>(null);
  const [migrationResult, setMigrationResult] = useState<ProjectMigrationResult | null>(null);
  const [exportResult, setExportResult] = useState<SafeExportResult | null>(null);
  const [privacyResult, setPrivacyResult] = useState<PrivacyArtifactsResult | null>(null);
  const [actionMessage, setActionMessage] = useState("");
  const [workspaceConfig, setWorkspaceConfig] = useState<WorkspaceConfig | null>(null);
  const [workspaceScanBatches, setWorkspaceScanBatches] = useState<WorkspaceScanBatch[]>([]);
  const [isScanningWorkspace, setIsScanningWorkspace] = useState(false);

  useEffect(() => {
    setSelectedModelId(deepSeekSettings.selectedModelId);
  }, [deepSeekSettings.selectedModelId]);

  useEffect(() => {
    if (!isDesktopReady) return;
    void getWorkspaceConfig()
      .then(setWorkspaceConfig)
      .catch((err) => reportActionError("读取个人文件空间配置", err));
    void listWorkspaceScanBatches()
      .then(setWorkspaceScanBatches)
      .catch((err) => reportActionError("读取扫描批次", err));
  }, [isDesktopReady]);

  const currentProjectName = activeProject?.name ?? "未打开项目";
  const currentProjectRoot = activeProject?.rootDir ?? "";
  const projectOptions = useMemo(
    () => projects.map((project) => `${project.name} · ${project.rootDir}`),
    [projects],
  );

  function reportActionError(action: string, err: unknown) {
    const message = `${action}失败：${String(err)}`;
    setActionMessage(message);
    setError(message);
  }

  function returnToWork() {
    navigate(activeProject ? "/work" : "/");
  }

  async function pickDirectory(setter: (value: string) => void) {
    try {
      const directory = await chooseDirectory("选择目录");
      if (directory) setter(directory);
    } catch (err) {
      reportActionError("选择目录", err);
    }
  }

  async function runBackup() {
    if (!isDesktopReady) return;
    setIsRunningBackupAction(true);
    setActionMessage("");
    try {
      const result = await createLocalBackup(activeProject ? [activeProject.rootDir] : [], backupDestination);
      setBackupResult(result);
      setActionMessage("本机备份已生成。");
    } catch (err) {
      reportActionError("创建本机备份", err);
    } finally {
      setIsRunningBackupAction(false);
    }
  }

  async function runRestore() {
    if (!isDesktopReady || !restoreSource.trim()) return;
    setIsRunningBackupAction(true);
    setActionMessage("");
    try {
      const result = await restoreLocalBackup(restoreSource, restoreDestination);
      setRestoreResult(result);
      await refreshProjects();
      setActionMessage("备份恢复完成，项目注册表已刷新。");
    } catch (err) {
      reportActionError("恢复备份", err);
    } finally {
      setIsRunningBackupAction(false);
    }
  }

  async function runMigration() {
    if (!isDesktopReady || !activeProject || !migrationDestination.trim()) return;
    setIsRunningBackupAction(true);
    setActionMessage("");
    try {
      const result = await migrateProjectRoot(activeProject.rootDir, migrationDestination);
      setMigrationResult(result);
      await refreshProjects();
      await openProject(result.project.rootDir);
      setActionMessage("项目根目录迁移完成，旧目录已保留。");
    } catch (err) {
      reportActionError("迁移项目根目录", err);
    } finally {
      setIsRunningBackupAction(false);
    }
  }

  async function runSafeExport() {
    if (!isDesktopReady || !activeProject) return;
    setIsRunningBackupAction(true);
    setActionMessage("");
    try {
      const result = await exportProjectSafe(activeProject.rootDir, exportDestination);
      setExportResult(result);
      setActionMessage("安全导出已完成。");
    } catch (err) {
      reportActionError("安全导出", err);
    } finally {
      setIsRunningBackupAction(false);
    }
  }

  async function runPrivacyArtifacts() {
    if (!isDesktopReady || !activeProject) return;
    setIsRunningBackupAction(true);
    setActionMessage("");
    try {
      const result = await generatePrivacyArtifacts(activeProject.rootDir);
      setPrivacyResult(result);
      setActionMessage("隐私说明、许可清单和诊断日志已生成。");
    } catch (err) {
      reportActionError("生成隐私与诊断材料", err);
    } finally {
      setIsRunningBackupAction(false);
    }
  }

  async function openFolderFromSettings(path: string) {
    try {
      await openFolderPath(path);
      setActionMessage("已打开所在位置。");
    } catch (err) {
      reportActionError("打开所在位置", err);
    }
  }

  async function initializeWorkspace() {
    if (!isDesktopReady) return;
    setActionMessage("");
    try {
      const config = await initializeWorkspaceRoot(null);
      setWorkspaceConfig(config);
      setActionMessage("个人文件空间已初始化。");
    } catch (err) {
      reportActionError("初始化个人文件空间", err);
    }
  }

  async function verifyWorkspace() {
    if (!isDesktopReady) return;
    setActionMessage("");
    try {
      const config = await verifyWorkspaceRoot();
      setWorkspaceConfig(config);
      setActionMessage("个人文件空间状态已刷新。");
    } catch (err) {
      reportActionError("刷新个人文件空间状态", err);
    }
  }

  async function runWorkspaceScan(action: () => Promise<WorkspaceScanBatch>, label: string) {
    if (!isDesktopReady || isScanningWorkspace) return;
    setIsScanningWorkspace(true);
    setActionMessage(`${label}中，仅生成预览，不会复制、移动或整理文件。`);
    try {
      const batch = await action();
      setWorkspaceScanBatches((items) => [batch, ...items.filter((item) => item.id !== batch.id)].slice(0, 30));
      if (batch.status === "failed") {
        setActionMessage(`${label}失败：${batch.failureReason || "未知错误"}`);
      } else {
        setActionMessage(
          `${label}完成：总文件 ${batch.fileCount}，可整理 ${batch.organizableCount}，需要确认 ${batch.needsConfirmationCount}，无法判断 ${batch.unknownCount}。`,
        );
      }
    } catch (err) {
      reportActionError(label, err);
    } finally {
      setIsScanningWorkspace(false);
    }
  }

  async function scanCustomDirectoryFromSettings() {
    if (!isDesktopReady) return;
    try {
      const directory = await chooseDirectory("选择要扫描的目录");
      if (!directory) return;
      await runWorkspaceScan(() => scanWorkspaceDirectory(directory), "扫描指定目录");
    } catch (err) {
      reportActionError("选择扫描目录", err);
    }
  }

  return (
    <AppLayout className="settings-shell">
      <div className="page-shell page-shell-settings">
        <div className="settings-frame">
        <header className="settings-topbar">
          <div>
            <span className="section-label">设置后台</span>
            <h1>感冒院管理中心</h1>
          </div>
          <button type="button" className="btn" onClick={returnToWork}>
            返回工作
          </button>
        </header>

        <section className="settings-layout">
          <aside className="settings-sidebar">
            {menuItems.map((item) => (
              <button
                key={item.key}
                type="button"
                className={`settings-menu-item ${selectedSettingsSection === item.key ? "active" : ""}`}
                onClick={() => setSelectedSettingsSection(item.key)}
              >
                {item.label}
              </button>
            ))}
          </aside>

          <section className="settings-content">
            {renderSection(selectedSettingsSection, {
              theme,
              toggleTheme,
              currentTask,
              nextStep,
              materials,
              pendingMaterials,
              pendingReviews,
              updatePendingReviewStatus: async (reviewId, status) => {
                try {
                  await updatePendingReviewStatus(reviewId, status);
                  setActionMessage(
                    status === "resolved"
                      ? "待检查事项已标记为已处理。"
                      : status === "ignored"
                        ? "待检查事项已忽略。"
                        : "待检查事项已重新打开。",
                  );
                } catch (err) {
                  reportActionError("更新待检查事项", err);
                }
              },
              projectMemory,
              aiContextItems,
              deepSeekSettings,
              deepSeekModels,
              apiKey,
              selectedModelId,
              setApiKey,
              setSelectedModelId,
              saveDeepSeekApiKey: async () => {
                setIsSavingKey(true);
                try {
                  await saveDeepSeekApiKey(apiKey, selectedModelId);
                  setApiKey("");
                  setActionMessage("DeepSeek Key 已保存到 Windows Credential Manager。");
                } catch (err) {
                  reportActionError("保存 DeepSeek Key", err);
                } finally {
                  setIsSavingKey(false);
                }
              },
              deleteDeepSeekApiKey: async () => {
                try {
                  await deleteDeepSeekApiKey();
                  setActionMessage("DeepSeek Key 已删除。");
                } catch (err) {
                  reportActionError("删除 DeepSeek Key", err);
                }
              },
              testDeepSeekConnection: async () => {
                setIsTesting(true);
                try {
                  await testDeepSeekConnection(selectedModelId);
                  setActionMessage("DeepSeek 连接测试成功。");
                } catch (err) {
                  reportActionError("测试 DeepSeek 连接", err);
                } finally {
                  setIsTesting(false);
                }
              },
              weeklyReviewDashboard,
              weeklyReviewSettings,
              weeklyReports,
              autoRouteHighConfidence: inboxRoutingSettings.autoRouteHighConfidence,
              saveInboxAutoRouteSetting: async (enabled) => {
                try {
                  await saveInboxAutoRouteSetting(enabled);
                  setActionMessage(enabled ? "已允许高置信文件自动归位。" : "已关闭高置信文件自动归位。");
                } catch (err) {
                  reportActionError("保存自动归位设置", err);
                }
              },
              refreshWeeklyReviewDashboard: async () => {
                try {
                  await refreshWeeklyReviewDashboard();
                  setActionMessage("每周复盘已刷新。");
                } catch (err) {
                  reportActionError("刷新每周复盘", err);
                }
              },
              generateWeeklyReviews: async (force) => {
                try {
                  const dashboard = await generateWeeklyReviews(force);
                  setActionMessage(force ? "每周复盘已重新生成。" : "每周复盘已生成。");
                  return dashboard;
                } catch (err) {
                  reportActionError("生成每周复盘", err);
                  return null;
                }
              },
              saveWeeklyReviewGenerationWeekday: async (weekday) => {
                try {
                  await saveWeeklyReviewGenerationWeekday(weekday);
                  setActionMessage("每周复盘生成日已保存。");
                } catch (err) {
                  reportActionError("保存每周复盘生成日", err);
                }
              },
              isSavingKey,
              isTesting,
              isScanningWorkspace,
              currentProjectName,
              currentProjectRoot,
              projectOptions,
              workspaceConfig,
              workspaceScanBatches,
              initializeWorkspace,
              verifyWorkspace,
              scanDesktop: async () => runWorkspaceScan(scanDesktopDirectory, "扫描桌面"),
              scanDownloads: async () => runWorkspaceScan(scanDownloadsDirectory, "扫描下载目录"),
              scanCustomDirectory: scanCustomDirectoryFromSettings,
              isDesktopReady,
              backupDestination,
              restoreSource,
              restoreDestination,
              migrationDestination,
              exportDestination,
              setBackupDestination,
              setRestoreSource,
              setRestoreDestination,
              setMigrationDestination,
              setExportDestination,
              pickBackupDestination: async () => pickDirectory(setBackupDestination),
              pickRestoreSource: async () => pickDirectory(setRestoreSource),
              pickRestoreDestination: async () => pickDirectory(setRestoreDestination),
              pickMigrationDestination: async () => pickDirectory(setMigrationDestination),
              pickExportDestination: async () => pickDirectory(setExportDestination),
              runBackup,
              runRestore,
              runMigration,
              runSafeExport,
              runPrivacyArtifacts,
              isRunningBackupAction,
              backupResult,
              restoreResult,
              migrationResult,
              exportResult,
              privacyResult,
              actionMessage,
              openFolderPath: openFolderFromSettings,
            })}
          </section>
        </section>
        </div>
      </div>
    </AppLayout>
  );
}

function renderSection(section: SettingsSection, data: SettingsRenderData) {
  switch (section) {
    case "general":
      return (
        <SectionFrame title="常规" description="集中查看当前桌面应用的运行形态和当前项目状态。">
          <InfoRow label="当前项目" value={data.currentProjectName} />
          <label className="settings-check-row">
            <input
              type="checkbox"
              checked={data.autoRouteHighConfidence}
              onChange={(event) => void data.saveInboxAutoRouteSetting(event.target.checked)}
            />
            <span>允许高置信文件自动归位（默认关闭）</span>
          </label>
          <p className="settings-help">仅当 AI 项目复核、分类和位置均为高置信，且无命名或版本冲突时执行；仍保留来源、审计和撤销。</p>
          <InfoRow label="当前任务" value={data.currentTask} />
          <InfoRow label="下一步" value={data.nextStep} />
          <InfoRow label="正式页面" value="启动页 / 项目工作台 / 设置后台" />
          <InfoRow
            label="桌面能力"
            value={
              data.isDesktopReady
                ? "已运行于 Tauri 桌面壳，可执行真实文件与目录操作。"
                : "当前不是 Tauri 运行环境。"
            }
          />
        </SectionFrame>
      );
    case "projects":
      return (
        <SectionFrame title="项目管理" description="项目切换、根目录和注册状态统一留在后台查看。">
          <div className="settings-subsection">
            <div>
              <span className="section-label">个人文件空间</span>
              <h3>Workspace Root</h3>
              <p className="settings-help">
                v4.1 只建立长期工作文件空间底座；旧项目、旧 Inbox 和 AppData 受管资料不会被迁移或移动。
              </p>
            </div>
            <InfoRow
              label="当前状态"
              value={workspaceStatusLabel(data.workspaceConfig?.status ?? "missing")}
            />
            <InfoRow
              label="根目录"
              value={data.workspaceConfig?.workspaceRoot || "尚未初始化，默认将使用 D:\\GanMaoYuan_Workspace"}
              mono
            />
            <InfoRow label="Inbox" value={data.workspaceConfig?.inboxRoot || "00_Inbox"} mono />
            <InfoRow label="项目目录" value={data.workspaceConfig?.projectsRoot || "10_Projects"} mono />
            <InfoRow label="通用资料" value={data.workspaceConfig?.generalRoot || "20_General"} mono />
            <InfoRow label="临时资料" value={data.workspaceConfig?.temporaryRoot || "30_Temporary"} mono />
            <InfoRow label="归档目录" value={data.workspaceConfig?.archiveRoot || "40_Archive"} mono />
            <InfoRow label="系统元数据" value={data.workspaceConfig?.systemRoot || ".ganmaoyuan"} mono />
            <InfoRow label="最近校验" value={data.workspaceConfig?.lastVerifiedAt || "尚未校验"} />
            <div className="settings-action-row">
              <button type="button" className="btn btn-primary" onClick={() => void data.initializeWorkspace()}>
                初始化工作空间
              </button>
              <button type="button" className="btn" onClick={() => void data.verifyWorkspace()}>
                刷新状态
              </button>
              <button
                type="button"
                className="btn"
                disabled={!data.workspaceConfig?.workspaceRoot}
                onClick={() => void data.openFolderPath(data.workspaceConfig?.workspaceRoot ?? "")}
              >
                打开工作空间
              </button>
            </div>
          </div>
          <div className="settings-subsection">
            <div>
              <span className="section-label">扫描预览</span>
              <h3>Workspace Scanner</h3>
              <p className="settings-help">
                只遍历目录并复用现有文件理解链路生成建议；本阶段不会复制、移动、覆盖或整理任何原文件。
              </p>
            </div>
            <div className="settings-action-row">
              <button
                type="button"
                className="btn btn-primary"
                disabled={!data.isDesktopReady || data.isScanningWorkspace}
                onClick={() => void data.scanDesktop()}
              >
                {data.isScanningWorkspace ? "扫描中..." : "扫描桌面"}
              </button>
              <button
                type="button"
                className="btn"
                disabled={!data.isDesktopReady || data.isScanningWorkspace}
                onClick={() => void data.scanDownloads()}
              >
                扫描下载目录
              </button>
              <button
                type="button"
                className="btn"
                disabled={!data.isDesktopReady || data.isScanningWorkspace}
                onClick={() => void data.scanCustomDirectory()}
              >
                扫描指定目录
              </button>
            </div>
            {data.workspaceScanBatches[0] ? (
              <>
                <ResultBlock
                  lines={[
                    `最近批次：${workspaceScanStatusLabel(data.workspaceScanBatches[0].status)} · ${data.workspaceScanBatches[0].sourceDirectory}`,
                    `总文件：${data.workspaceScanBatches[0].fileCount}`,
                    `可整理：${data.workspaceScanBatches[0].organizableCount}`,
                    `需要确认：${data.workspaceScanBatches[0].needsConfirmationCount}`,
                    `无法判断：${data.workspaceScanBatches[0].unknownCount}`,
                  ]}
                />
                {data.workspaceScanBatches[0].files.slice(0, 8).map((file) => (
                  <ListCard
                    key={file.id}
                    title={file.fileName}
                    detail={[
                      file.documentType || "尚未识别",
                      file.ownershipType ? `归属：${workspaceOwnershipLabel(file.ownershipType)}` : "",
                      file.recommendedLocation ? `建议位置：${file.recommendedLocation}` : "",
                      `置信度：${confidenceDisplay(file.confidenceLevel, file.confidenceScore)}`,
                      file.needsConfirmation ? "需要确认" : "仅预览",
                    ]
                      .filter(Boolean)
                      .join(" · ")}
                  />
                ))}
              </>
            ) : (
              <InfoRow label="最近扫描" value="暂无扫描批次。可以先扫描桌面、下载目录或指定目录生成预览。" />
            )}
          </div>
          <InfoRow label="当前项目" value={data.currentProjectName} />
          <InfoRow label="当前根目录" value={data.currentProjectRoot || "尚未打开项目"} mono />
          <InfoRow label="已注册项目" value={data.projectOptions.length ? data.projectOptions.join("；") : "暂无"} />
        </SectionFrame>
      );
    case "tasks":
      return (
        <SectionFrame title="任务管理" description="工作台维持单线程推进，后台用于回看任务定义和下一步。">
          <InfoRow label="当前任务" value={data.currentTask} />
          <InfoRow label="下一步" value={data.nextStep} />
          <InfoRow label="任务模式" value="连续工作台 + 自动恢复点" />
        </SectionFrame>
      );
    case "materials":
      return (
        <SectionFrame title="资料管理" description="导入资料、补充资料和解析摘要都在项目内持久化。">
          {data.materials.map((item) => (
            <ListCard key={item.title} title={item.title} detail={item.detail} />
          ))}
        </SectionFrame>
      );
    case "pending-materials": {
      const activePendingReviews = data.pendingReviews.filter((item) => item.status !== "resolved" && item.status !== "ignored");
      return (
        <SectionFrame title="待整理资料" description="分类不确定或需要人工确认的内容统一进入待检查事项。">
          {activePendingReviews.length ? (
            activePendingReviews.map((item) => (
              <ListCard key={item.id || item.key} title={item.title} detail={`${item.detail} · 状态：${pendingReviewStatusLabel(item.status)}`}>
                <div className="settings-action-row">
                  <button type="button" className="btn btn-primary" onClick={() => void data.updatePendingReviewStatus(item.id, "resolved")}>
                    已处理
                  </button>
                  <button type="button" className="btn" onClick={() => void data.updatePendingReviewStatus(item.id, "ignored")}>
                    忽略
                  </button>
                </div>
              </ListCard>
            ))
          ) : data.pendingMaterials.length ? (
            data.pendingMaterials.map((item) => <ListCard key={item.title} title={item.title} detail={item.detail} />)
          ) : (
            <EmptyState kind="pending" title="暂无待整理资料；需要人工确认的项目影响、文件变化或解析异常会出现在这里。" />
          )}
        </SectionFrame>
      );
    }
    case "file-index":
      return (
        <SectionFrame title="文件索引" description="当前由项目根目录内 JSON 清单维护文件位置、版本和解析信息。">
          <InfoRow label="文件位置总管" value="已接入真实项目文件位置管理与审计记录。" />
          <InfoRow label="数据来源" value="项目根目录下 .ganmaoyuan 与 project-location-manifest.json" />
        </SectionFrame>
      );
    case "project-memory":
      return (
        <SectionFrame title="项目记忆" description="项目连续性信息不再挤占工作台，只在后台集中查看。">
          {data.projectMemory.map((item) => (
            <ListCard key={item} title={item} detail="当前为项目记忆条目。" />
          ))}
        </SectionFrame>
      );
    case "ai-context":
      return (
        <SectionFrame title="AI 上下文" description="发送给 DeepSeek 的上下文只包含项目说明、摘要、关键历史和当前问题。">
          {data.aiContextItems.map((item) => (
            <ListCard key={item} title={item} detail="当前为 AI 上下文条目。" />
          ))}
        </SectionFrame>
      );
    case "ai-models":
      return (
        <SectionFrame title="AI 模型" description="当前只接入 DeepSeek，API Key 保存在 Windows Credential Manager。">
          <InfoRow label="当前提供方" value="DeepSeek" />
          <InfoRow
            label="Key 状态"
            value={data.deepSeekSettings.hasApiKey ? "已保存到 Windows Credential Manager" : "未保存"}
          />
          <label className="settings-form-field">
            <span>DeepSeek API Key</span>
            <input
              type="password"
              value={data.apiKey}
              onChange={(event) => data.setApiKey(event.target.value)}
              placeholder="输入 DeepSeek API Key"
            />
          </label>
          <label className="settings-form-field">
            <span>模型</span>
            <select value={data.selectedModelId} onChange={(event) => data.setSelectedModelId(event.target.value)}>
              <option value="">先测试连接并拉取模型列表</option>
              {data.deepSeekModels.map((model) => (
                <option key={model.id} value={model.id}>
                  {model.id}
                </option>
              ))}
            </select>
          </label>
          <InfoRow label="最近测试" value={data.deepSeekSettings.lastTestedAt || "尚未测试"} />
          <div className="settings-action-row">
            <button
              type="button"
              className="btn btn-primary"
              disabled={!data.apiKey.trim() || data.isSavingKey}
              onClick={() => void data.saveDeepSeekApiKey()}
            >
              {data.isSavingKey ? "保存中..." : "保存 Key"}
            </button>
            <button
              type="button"
              className="btn"
              disabled={!data.deepSeekSettings.hasApiKey || data.isTesting}
              onClick={() => void data.testDeepSeekConnection()}
            >
              {data.isTesting ? "测试中..." : "测试连接"}
            </button>
            <button
              type="button"
              className="btn"
              disabled={!data.deepSeekSettings.hasApiKey}
              onClick={() => void data.deleteDeepSeekApiKey()}
            >
              删除 Key
            </button>
          </div>
          {data.actionMessage ? <StatusNotice text={data.actionMessage} /> : null}
        </SectionFrame>
      );
    case "data-backup":
      return (
        <SectionFrame title="数据与备份" description="本机备份、恢复、项目迁移和安全导出都通过桌面端真实执行。">
          <InfoRow label="保存方式" value="项目清单、对话和解析结果保存在项目根目录与本机应用数据目录中。" />
          <InfoRow label="当前项目" value={data.currentProjectRoot || "尚未打开项目"} mono />

          <label className="settings-form-field">
            <span>备份输出目录</span>
            <div className="settings-inline-field">
              <input
                value={data.backupDestination}
                onChange={(event) => data.setBackupDestination(event.target.value)}
                placeholder="留空则使用 D:\\GanMaoYuan\\Backups"
              />
              <button type="button" className="btn" onClick={() => void data.pickBackupDestination()}>
                选择目录
              </button>
            </div>
          </label>
          <div className="settings-action-row">
            <button
              type="button"
              className="btn btn-primary"
              disabled={data.isRunningBackupAction || !data.isDesktopReady}
              onClick={() => void data.runBackup()}
            >
              创建本机备份
            </button>
            {data.backupResult ? (
              <button
                type="button"
                className="btn"
                onClick={() => {
                  const backupDir = data.backupResult?.backupDir;
                  if (backupDir) {
                    void data.openFolderPath(backupDir);
                  }
                }}
              >
                打开备份目录
              </button>
            ) : null}
          </div>
          {data.backupResult ? (
            <ResultBlock
              lines={[
                `备份目录：${data.backupResult.backupDir}`,
                `包含项目：${data.backupResult.projectRoots.length}`,
                `校验清单：${data.backupResult.checksumManifestPath}`,
              ]}
            />
          ) : null}

          <div className="settings-divider" />

          <label className="settings-form-field">
            <span>恢复来源目录</span>
            <div className="settings-inline-field">
              <input
                value={data.restoreSource}
                onChange={(event) => data.setRestoreSource(event.target.value)}
                placeholder="选择 backup-bundle 所在目录"
              />
              <button type="button" className="btn" onClick={() => void data.pickRestoreSource()}>
                选择目录
              </button>
            </div>
          </label>
          <label className="settings-form-field">
            <span>恢复项目根目录</span>
            <div className="settings-inline-field">
              <input
                value={data.restoreDestination}
                onChange={(event) => data.setRestoreDestination(event.target.value)}
                placeholder="留空则使用 D:\\GanMaoYuan\\RestoredProjects"
              />
              <button type="button" className="btn" onClick={() => void data.pickRestoreDestination()}>
                选择目录
              </button>
            </div>
          </label>
          <div className="settings-action-row">
            <button
              type="button"
              className="btn"
              disabled={data.isRunningBackupAction || !data.restoreSource.trim() || !data.isDesktopReady}
              onClick={() => void data.runRestore()}
            >
              恢复备份
            </button>
          </div>
          {data.restoreResult ? (
            <ResultBlock
              lines={[
                `恢复时间：${data.restoreResult.restoredAt}`,
                `恢复项目：${data.restoreResult.restoredProjects.length}`,
                `恢复全局目录：${data.restoreResult.restoredGlobalDir}`,
                ...(data.restoreResult.warnings.length ? data.restoreResult.warnings.map((item) => `提示：${item}`) : []),
              ]}
            />
          ) : null}

          <div className="settings-divider" />

          <label className="settings-form-field">
            <span>迁移到新根目录</span>
            <div className="settings-inline-field">
              <input
                value={data.migrationDestination}
                onChange={(event) => data.setMigrationDestination(event.target.value)}
                placeholder="为当前项目选择新的根目录"
              />
              <button type="button" className="btn" onClick={() => void data.pickMigrationDestination()}>
                选择目录
              </button>
            </div>
          </label>
          <div className="settings-action-row">
            <button
              type="button"
              className="btn"
              disabled={
                data.isRunningBackupAction ||
                !data.currentProjectRoot ||
                !data.migrationDestination.trim() ||
                !data.isDesktopReady
              }
              onClick={() => void data.runMigration()}
            >
              迁移当前项目
            </button>
          </div>
          {data.migrationResult ? (
            <ResultBlock
              lines={[
                `旧根目录：${data.migrationResult.oldRootDir}`,
                `新根目录：${data.migrationResult.newRootDir}`,
                `复制条目数：${String(data.migrationResult.copiedEntries)}`,
              ]}
            />
          ) : null}

          <div className="settings-divider" />

          <label className="settings-form-field">
            <span>安全导出目录</span>
            <div className="settings-inline-field">
              <input
                value={data.exportDestination}
                onChange={(event) => data.setExportDestination(event.target.value)}
                placeholder="留空则使用 D:\\GanMaoYuan\\Exports"
              />
              <button type="button" className="btn" onClick={() => void data.pickExportDestination()}>
                选择目录
              </button>
            </div>
          </label>
          <div className="settings-action-row">
            <button
              type="button"
              className="btn"
              disabled={data.isRunningBackupAction || !data.currentProjectRoot || !data.isDesktopReady}
              onClick={() => void data.runSafeExport()}
            >
              安全导出当前项目
            </button>
            {data.exportResult ? (
              <button
                type="button"
                className="btn"
                onClick={() => {
                  const exportDir = data.exportResult?.exportDir;
                  if (exportDir) {
                    void data.openFolderPath(exportDir);
                  }
                }}
              >
                打开导出目录
              </button>
            ) : null}
          </div>
          {data.exportResult ? (
            <ResultBlock
              lines={[
                `导出目录：${data.exportResult.exportDir}`,
                `导出文件数：${String(data.exportResult.exportedFileCount)}`,
                `脱敏对话数：${String(data.exportResult.redactedMessageCount)}`,
              ]}
            />
          ) : null}
          {data.actionMessage ? <StatusNotice text={data.actionMessage} /> : null}
        </SectionFrame>
      );
    case "weekly-review":
      return (
        <SectionFrame title="每周复盘" description="每周自动生成跨项目与项目内周报，供回顾和技能沉淀使用。">
          <InfoRow label="生成日" value={data.weeklyReviewSettings ? weekDayLabel(data.weeklyReviewSettings.generationWeekday) : "未设置"} />
          <InfoRow label="最近生成" value={data.weeklyReviewDashboard?.generatedAt || "暂无"} />
          <InfoRow label="周报数量" value={String(data.weeklyReports.length)} />
          <label className="settings-form-field">
            <span>生成日设置</span>
            <select
              value={data.weeklyReviewSettings?.generationWeekday ?? 1}
              onChange={(event) => void data.saveWeeklyReviewGenerationWeekday(Number(event.target.value))}
            >
              <option value={0}>周日</option>
              <option value={1}>周一</option>
              <option value={2}>周二</option>
              <option value={3}>周三</option>
              <option value={4}>周四</option>
              <option value={5}>周五</option>
              <option value={6}>周六</option>
            </select>
          </label>
          <div className="settings-action-row">
            <button type="button" className="btn btn-primary" onClick={() => void data.generateWeeklyReviews(true)}>
              立即生成
            </button>
            <button type="button" className="btn" onClick={() => void data.refreshWeeklyReviewDashboard()}>
              刷新
            </button>
          </div>
          {data.weeklyReports.length ? (
            <div className="result-block">
              {data.weeklyReports.slice(0, 3).map((report) => (
                <p key={report.id} className="mono-value">
                  {(report.scope === "global" ? "全局周报" : report.projectName) || "周报"} · {report.weekKey} ·{" "}
                  {report.reviewRequired ? "待确认" : "已确认"}
                </p>
              ))}
            </div>
          ) : null}
        </SectionFrame>
      );
    case "privacy-security":
      return (
        <SectionFrame title="隐私安全" description="仅本机项目，不做云同步；导出、备份和诊断日志默认脱敏。">
          <InfoRow label="外部接口" value="仅 DeepSeek 对话接入，原始文件不直接上传。" />
          <InfoRow label="敏感信息" value="API Key 保存在 Windows Credential Manager，不写入源码、JSON、备份或日志。" />
          <InfoRow label="云同步" value="未启用，保持仅本机项目。" />
          <div className="settings-action-row">
            <button
              type="button"
              className="btn"
              disabled={data.isRunningBackupAction || !data.currentProjectRoot || !data.isDesktopReady}
              onClick={() => void data.runPrivacyArtifacts()}
            >
              生成隐私与诊断材料
            </button>
          </div>
          {data.privacyResult ? (
            <>
              <ResultBlock
                lines={[
                  `隐私说明：${data.privacyResult.privacyNoticePath}`,
                  `第三方许可：${data.privacyResult.thirdPartyNoticesPath}`,
                  `脱敏诊断日志：${data.privacyResult.diagnosticLogPath}`,
                ]}
              />
              <div className="settings-action-row">
                <button
                  type="button"
                  className="btn"
                  onClick={() => {
                    const diagnosticLogPath = data.privacyResult?.diagnosticLogPath;
                    if (diagnosticLogPath) {
                      void data.openFolderPath(diagnosticLogPath);
                    }
                  }}
                >
                  打开诊断目录
                </button>
              </div>
            </>
          ) : null}
        </SectionFrame>
      );
    case "appearance":
      return (
        <SectionFrame title="外观" description="延续当前深色与荧光蓝质感，保留基础主题切换。">
          <InfoRow label="当前主题" value={data.theme === "dark" ? "深色模式" : "浅色模式"} />
          <div className="settings-action-row">
            <button type="button" className="btn" onClick={data.toggleTheme}>
              切换主题
            </button>
          </div>
        </SectionFrame>
      );
    case "advanced":
      return (
        <SectionFrame title="高级设置" description="保留发布前核查视图，不扩展新的业务能力。">
          <InfoRow label="桌面能力" value="已接入 Tauri，可选择目录、导入资料、复制文件、打开文件和目录。" />
          <InfoRow label="数据层" value="当前继续使用本地 JSON；未替换为 SQLite 或云端存储。" />
          <InfoRow label="发布范围" value="仅本机桌面应用，不做在线更新、云同步或公开发布仓库。" />
        </SectionFrame>
      );
  }
}

function SectionFrame({
  title,
  description,
  children,
}: {
  title: string;
  description: string;
  children: ReactNode;
}) {
  return (
    <div className="settings-panel">
      <div className="settings-panel-head">
        <div>
          <span className="section-label">{title}</span>
          <h2>{title}</h2>
        </div>
        <p>{description}</p>
      </div>
      <div className="settings-panel-body">{children}</div>
    </div>
  );
}

function InfoRow({ label, value, mono = false }: { label: string; value: string; mono?: boolean }) {
  return (
    <div className="info-row">
      <strong>{label}</strong>
      <span className={mono ? "mono-value" : undefined}>{value}</span>
    </div>
  );
}

function ListCard({ title, detail, children }: { title: string; detail: string; children?: ReactNode }) {
  return (
    <article className="list-card">
      <strong>{title}</strong>
      <span>{detail}</span>
      {children}
    </article>
  );
}

function StatusNotice({ text }: { text: string }) {
  return <div className="status-notice">{text}</div>;
}

function ResultBlock({ lines }: { lines: string[] }) {
  return (
    <div className="result-block">
      {lines.map((line) => (
        <p key={line} className="mono-value">
          {line}
        </p>
      ))}
    </div>
  );
}

function workspaceStatusLabel(status: string) {
  if (status === "ready") return "可用";
  if (status === "unavailable") return "不可访问或不可写";
  return "尚未初始化或目录不完整";
}

function workspaceScanStatusLabel(status: string) {
  if (status === "completed") return "已完成";
  if (status === "failed") return "失败";
  return "扫描中";
}

function workspaceOwnershipLabel(value: string) {
  if (value === "existingProjectMaterial") return "已有项目资料";
  if (value === "newProjectCandidate") return "新项目候选";
  if (value === "generalWorkMaterial") return "通用工作资料";
  if (value === "temporaryOrReference") return "临时/参考资料";
  if (value === "unsupportedOrFailed") return "不适合 AI 上下文";
  return value || "待确认";
}

function confidenceDisplay(level: string, score: number) {
  const levelText = level === "high" ? "高" : level === "medium" ? "中" : level === "low" ? "低" : "待确认";
  return score > 0 ? `${levelText} ${score}%` : levelText;
}

function pendingReviewStatusLabel(status: string) {
  if (status === "resolved") return "已处理";
  if (status === "ignored") return "已忽略";
  if (status === "pending" || status === "open") return "待处理";
  return status || "待处理";
}

function weekDayLabel(day: number) {
  return ["周日", "周一", "周二", "周三", "周四", "周五", "周六"][day] ?? "未设置";
}
