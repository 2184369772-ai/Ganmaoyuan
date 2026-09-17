import { useEffect, useState } from "react";
import {
  bulkReviewCleanupPlan,
  chooseDirectory,
  executeCleanupPlan,
  generateCleanupPlan,
  listCleanupExecutionBatches,
  listCleanupPlans,
  listWorkspaceScanBatches,
  modifyCleanupPlanItem,
  resetCleanupPlanItems,
  reviewCleanupPlanItems,
  scanDesktopDirectory,
  scanDownloadsDirectory,
  scanWorkspaceDirectory,
  undoCleanupExecutionBatch,
  type CleanupExecutionBatch,
  type CleanupPlan,
  type CleanupPlanItem,
  type CleanupPlanReviewFilter,
  type WorkspaceScanBatch,
} from "../features/project/desktopApi";

type WorkspaceScannerPanelProps = {
  isDesktopReady: boolean;
  onMessage?: (message: string) => void;
};

export function WorkspaceScannerPanel({ isDesktopReady, onMessage }: WorkspaceScannerPanelProps) {
  const [batches, setBatches] = useState<WorkspaceScanBatch[]>([]);
  const [plans, setPlans] = useState<CleanupPlan[]>([]);
  const [executions, setExecutions] = useState<CleanupExecutionBatch[]>([]);
  const [selectedPlanId, setSelectedPlanId] = useState("");
  const [selectedPlanFilter, setSelectedPlanFilter] = useState("all");
  const [expandedTraceItemId, setExpandedTraceItemId] = useState("");
  const [editingItemId, setEditingItemId] = useState("");
  const [executionPreviewPlanId, setExecutionPreviewPlanId] = useState("");
  const [editTargetPath, setEditTargetPath] = useState("");
  const [editProject, setEditProject] = useState("");
  const [editCategory, setEditCategory] = useState("");
  const [editReason, setEditReason] = useState("");
  const [isScanning, setIsScanning] = useState(false);
  const [isGeneratingPlan, setIsGeneratingPlan] = useState(false);
  const [isReviewingPlan, setIsReviewingPlan] = useState(false);
  const [isExecutingPlan, setIsExecutingPlan] = useState(false);

  useEffect(() => {
    if (!isDesktopReady) return;
    void Promise.all([listWorkspaceScanBatches(), listCleanupPlans(), listCleanupExecutionBatches()])
      .then(([nextBatches, nextPlans, nextExecutions]) => {
        setBatches(nextBatches);
        setPlans(nextPlans);
        setExecutions(nextExecutions);
        if (nextPlans[0]) setSelectedPlanId(nextPlans[0].id);
      })
      .catch((err) => onMessage?.(`读取扫描与整理方案失败：${String(err)}`));
  }, [isDesktopReady, onMessage]);

  async function runScan(action: () => Promise<WorkspaceScanBatch>, label: string) {
    if (!isDesktopReady || isScanning) return;
    setIsScanning(true);
    onMessage?.(`${label}中，仅生成预览，不会复制、移动或整理文件。`);
    try {
      const batch = await action();
      setBatches((items) => [batch, ...items.filter((item) => item.id !== batch.id)].slice(0, 30));
      if (batch.status === "failed") {
        onMessage?.(`${label}失败：${batch.failureReason || "未知错误"}`);
      } else {
        onMessage?.(
          `${label}完成：总文件 ${batch.fileCount}，可整理 ${batch.organizableCount}，需要确认 ${batch.needsConfirmationCount}，无法判断 ${batch.unknownCount}。`,
        );
      }
    } catch (err) {
      onMessage?.(`${label}失败：${String(err)}`);
    } finally {
      setIsScanning(false);
    }
  }

  async function scanCustomDirectory() {
    try {
      const directory = await chooseDirectory("选择要扫描的目录");
      if (!directory) return;
      await runScan(() => scanWorkspaceDirectory(directory), "扫描指定目录");
    } catch (err) {
      onMessage?.(`选择扫描目录失败：${String(err)}`);
    }
  }

  async function generatePlan(batch: WorkspaceScanBatch) {
    if (!isDesktopReady || isGeneratingPlan) return;
    setIsGeneratingPlan(true);
    onMessage?.("正在生成整理方案，仅生成预览，不会复制、移动或删除文件。");
    try {
      const plan = await generateCleanupPlan(batch.id);
      setPlans((items) => [plan, ...items.filter((item) => item.id !== plan.id && item.scanBatchId !== plan.scanBatchId)]);
      setSelectedPlanId(plan.id);
      onMessage?.(`整理方案已生成：${plan.items.length} 个文件，状态 ${cleanupPlanStatusLabel(plan.status)}。`);
    } catch (err) {
      onMessage?.(`生成整理方案失败：${String(err)}`);
    } finally {
      setIsGeneratingPlan(false);
    }
  }

  async function applyPlanUpdate(action: () => Promise<CleanupPlan>, successMessage: string) {
    if (!isDesktopReady || isReviewingPlan) return;
    setIsReviewingPlan(true);
    try {
      const plan = await action();
      setPlans((items) => [plan, ...items.filter((item) => item.id !== plan.id)]);
      setSelectedPlanId(plan.id);
      onMessage?.(successMessage);
    } catch (err) {
      onMessage?.(`整理方案审核失败：${String(err)}`);
    } finally {
      setIsReviewingPlan(false);
    }
  }

  function reviewItem(plan: CleanupPlan, item: CleanupPlanItem, reviewStatus: "approved" | "rejected") {
    void applyPlanUpdate(
      () => reviewCleanupPlanItems(plan.id, [item.id], reviewStatus, reviewStatus === "approved" ? "单项确认" : "单项拒绝"),
      reviewStatus === "approved" ? "已确认该整理项。" : "已拒绝该整理项。",
    );
  }

  function resetItem(plan: CleanupPlan, item: CleanupPlanItem) {
    void applyPlanUpdate(() => resetCleanupPlanItems(plan.id, [item.id]), "已退回待确认。");
  }

  function bulkApproveHighConfidence(plan: CleanupPlan) {
    void applyPlanUpdate(
      () => bulkReviewCleanupPlan(plan.id, { confidenceLevel: "high" }, "approved", "批量确认高置信整理项"),
      "已批量确认高置信整理项。",
    );
  }

  function rejectCurrentFilter(plan: CleanupPlan) {
    void applyPlanUpdate(
      () => bulkReviewCleanupPlan(plan.id, cleanupPlanFilterToReviewFilter(selectedPlanFilter), "rejected", "批量拒绝当前筛选项"),
      "已批量拒绝当前筛选项。",
    );
  }

  function resetCurrentFilter(plan: CleanupPlan, items: CleanupPlanItem[]) {
    void applyPlanUpdate(
      () => resetCleanupPlanItems(plan.id, items.map((item) => item.id)),
      "已将当前筛选项退回待确认。",
    );
  }

  function startEditing(item: CleanupPlanItem) {
    setEditingItemId(item.id);
    setEditProject(item.finalProject || item.recommendedProject || "");
    setEditCategory(item.finalCategory || cleanupOwnershipLabel(item.recommendedOwnership));
    setEditTargetPath(item.finalTargetPath || item.recommendedTargetPath || "");
    setEditReason(item.userReason || "");
  }

  function submitModification(plan: CleanupPlan, item: CleanupPlanItem) {
    void applyPlanUpdate(
      () => modifyCleanupPlanItem(plan.id, item.id, editTargetPath, editProject, editCategory, editReason || "用户修改整理建议"),
      "已保存用户修改后的整理建议。",
    );
    setEditingItemId("");
  }

  async function executeReviewedPlan(plan: CleanupPlan) {
    if (!isDesktopReady || isExecutingPlan) return;
    setIsExecutingPlan(true);
    try {
      const batch = await executeCleanupPlan(plan.id);
      const nextPlans = await listCleanupPlans();
      setExecutions((items) => [batch, ...items.filter((item) => item.id !== batch.id)]);
      setPlans(nextPlans);
      setSelectedPlanId(plan.id);
      setExecutionPreviewPlanId("");
      const completed = batch.items.filter((item) => item.status === "completed").length;
      const failed = batch.items.filter((item) => item.status === "failed").length;
      onMessage?.(`整理执行完成：成功 ${completed}，失败 ${failed}。原文件未移动、未删除。`);
    } catch (err) {
      onMessage?.(`执行整理方案失败：${String(err)}`);
    } finally {
      setIsExecutingPlan(false);
    }
  }

  async function undoExecution(batch: CleanupExecutionBatch) {
    if (!isDesktopReady || isExecutingPlan) return;
    setIsExecutingPlan(true);
    try {
      const updated = await undoCleanupExecutionBatch(batch.id);
      setExecutions((items) => [updated, ...items.filter((item) => item.id !== updated.id)]);
      const conflicts = updated.items.filter((item) => item.status === "undoConflict").length;
      onMessage?.(
        conflicts > 0
          ? `撤销遇到 ${conflicts} 个冲突：受管副本可能已被修改，请人工检查。`
          : "已撤销本次整理产生的受管副本，原文件未受影响。",
      );
    } catch (err) {
      onMessage?.(`撤销整理批次失败：${String(err)}`);
    } finally {
      setIsExecutingPlan(false);
    }
  }

  const latest = batches[0];
  const latestPlan = latest ? plans.find((plan) => plan.scanBatchId === latest.id) : undefined;
  const selectedPlan = plans.find((plan) => plan.id === selectedPlanId) || latestPlan || plans[0];
  const filteredPlanItems = selectedPlan
    ? selectedPlan.items.filter((item) => selectedPlanFilter === "all" || item.recommendedOwnership === selectedPlanFilter)
    : [];
  const planSummary = selectedPlan ? summarizeCleanupPlan(selectedPlan.items) : null;
  const selectedPlanExecution = selectedPlan ? executions.find((batch) => batch.cleanupPlanId === selectedPlan.id) : undefined;
  const executionSummary = selectedPlan ? summarizeExecutionPreview(selectedPlan.items) : null;

  return (
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
          disabled={!isDesktopReady || isScanning}
          onClick={() => void runScan(scanDesktopDirectory, "扫描桌面")}
        >
          {isScanning ? "扫描中..." : "扫描桌面"}
        </button>
        <button
          type="button"
          className="btn"
          disabled={!isDesktopReady || isScanning}
          onClick={() => void runScan(scanDownloadsDirectory, "扫描下载目录")}
        >
          扫描下载目录
        </button>
        <button type="button" className="btn" disabled={!isDesktopReady || isScanning} onClick={() => void scanCustomDirectory()}>
          扫描指定目录
        </button>
      </div>
      {latest ? (
        <>
          <div className="result-block">
            <p className="mono-value">
              最近批次：{workspaceScanStatusLabel(latest.status)} · {latest.sourceDirectory}
            </p>
            <p className="mono-value">总文件：{latest.fileCount}</p>
            <p className="mono-value">可整理：{latest.organizableCount}</p>
            <p className="mono-value">需要确认：{latest.needsConfirmationCount}</p>
            <p className="mono-value">无法判断：{latest.unknownCount}</p>
          </div>
          <div className="settings-action-row">
            <button
              type="button"
              className="btn btn-primary"
              disabled={!isDesktopReady || isGeneratingPlan || latest.status !== "completed"}
              onClick={() => void generatePlan(latest)}
            >
              {isGeneratingPlan ? "生成中..." : "生成整理方案"}
            </button>
            <button
              type="button"
              className="btn"
              disabled={!latestPlan}
              onClick={() => {
                if (latestPlan) setSelectedPlanId(latestPlan.id);
              }}
            >
              查看整理方案
            </button>
          </div>
          {latest.files.slice(0, 8).map((file) => (
            <article key={file.id} className="list-card">
              <strong>{file.fileName}</strong>
              <span>
                {[
                  file.documentType || "尚未识别",
                  file.ownershipType ? `归属：${workspaceOwnershipLabel(file.ownershipType)}` : "",
                  file.recommendedLocation ? `建议位置：${file.recommendedLocation}` : "",
                  `置信度：${confidenceDisplay(file.confidenceLevel, file.confidenceScore)}`,
                  file.needsConfirmation ? "需要确认" : "仅预览",
                ]
                  .filter(Boolean)
                  .join(" · ")}
              </span>
            </article>
          ))}
          {selectedPlan ? (
            <section className="result-block">
              <div>
                <span className="section-label">整理方案</span>
                <h4>Cleanup Plan · {cleanupPlanStatusLabel(selectedPlan.status)}</h4>
                <p className="settings-help">
                  这里只展示计划路径和判断依据；不会复制、移动、覆盖、删除原文件，也不会写入项目 manifest。
                </p>
              </div>
              {planSummary ? (
                <p className="mono-value">
                  扫描 {selectedPlan.items.length} 个文件 · 自动建议 {planSummary.autoSuggested} · 需要确认{" "}
                  {planSummary.needsReview} · 无法判断 {planSummary.unsupported}
                </p>
              ) : null}
              <div className="settings-action-row">
                {cleanupPlanFilters.map((filter) => (
                  <button
                    key={filter.value}
                    type="button"
                    className={selectedPlanFilter === filter.value ? "btn btn-primary" : "btn"}
                    onClick={() => setSelectedPlanFilter(filter.value)}
                  >
                    {filter.label}
                  </button>
                ))}
              </div>
              <div className="settings-action-row">
                <button
                  type="button"
                  className="btn btn-primary"
                  disabled={isReviewingPlan}
                  onClick={() => bulkApproveHighConfidence(selectedPlan)}
                >
                  批量确认高置信
                </button>
                <button
                  type="button"
                  className="btn"
                  disabled={isReviewingPlan || filteredPlanItems.length === 0}
                  onClick={() => rejectCurrentFilter(selectedPlan)}
                >
                  批量拒绝当前筛选
                </button>
                <button
                  type="button"
                  className="btn"
                  disabled={isReviewingPlan || filteredPlanItems.length === 0}
                  onClick={() => resetCurrentFilter(selectedPlan, filteredPlanItems)}
                >
                  退回待确认
                </button>
                <span className="settings-help">中低置信建议逐项确认；本阶段不会执行整理。</span>
              </div>
              <div className="settings-subsection">
                <div>
                  <strong>最终执行确认</strong>
                  <span>
                    只复制 reviewStatus=approved 的文件；pending、rejected、未再次确认的 modified 项不会执行。原文件不会移动或删除。
                  </span>
                </div>
                {executionSummary ? (
                  <span>
                    可执行 {executionSummary.total} 个 · 项目资料 {executionSummary.project} · 通用资料 {executionSummary.general} · 临时资料{" "}
                    {executionSummary.temporary}
                  </span>
                ) : null}
                {executionPreviewPlanId === selectedPlan.id ? (
                  <div className="info-row">
                    <strong>请最后确认</strong>
                    <span>本次只执行安全复制；如果复制失败会记录原因，单个失败不会影响其他文件。</span>
                    <div className="settings-action-row">
                      <button
                        type="button"
                        className="btn btn-primary"
                        disabled={isExecutingPlan || !executionSummary?.total || selectedPlan.status !== "reviewing"}
                        onClick={() => void executeReviewedPlan(selectedPlan)}
                      >
                        {isExecutingPlan ? "执行中..." : "确认执行"}
                      </button>
                      <button type="button" className="btn" disabled={isExecutingPlan} onClick={() => setExecutionPreviewPlanId("")}>
                        取消
                      </button>
                    </div>
                  </div>
                ) : (
                  <button
                    type="button"
                    className="btn btn-primary"
                    disabled={isExecutingPlan || !executionSummary?.total || selectedPlan.status !== "reviewing" || Boolean(selectedPlanExecution)}
                    onClick={() => setExecutionPreviewPlanId(selectedPlan.id)}
                  >
                    准备执行已确认项
                  </button>
                )}
                {selectedPlanExecution ? (
                  <div className="info-row">
                    <strong>最近执行：{cleanupExecutionStatusLabel(selectedPlanExecution.status)}</strong>
                    <span>
                      成功 {selectedPlanExecution.items.filter((item) => item.status === "completed").length} · 失败{" "}
                      {selectedPlanExecution.items.filter((item) => item.status === "failed").length} · 撤销冲突{" "}
                      {selectedPlanExecution.items.filter((item) => item.status === "undoConflict").length}
                    </span>
                    <button
                      type="button"
                      className="btn"
                      disabled={isExecutingPlan || selectedPlanExecution.status === "cancelled"}
                      onClick={() => void undoExecution(selectedPlanExecution)}
                    >
                      撤销本次整理
                    </button>
                  </div>
                ) : null}
              </div>
              {filteredPlanItems.slice(0, 12).map((item) => (
                <article key={item.id} className="list-card">
                  <strong>{item.fileName}</strong>
                  <span>
                    {[
                      item.currentLocation ? `当前：${item.currentLocation}` : "",
                      `判断：${cleanupOwnershipLabel(item.recommendedOwnership)}`,
                      item.finalProject || item.recommendedProject ? `项目：${item.finalProject || item.recommendedProject}` : "",
                      `建议：${item.finalTargetPath || item.recommendedTargetPath || "待确认"}`,
                      `置信度：${item.confidence?.display || "待确认"}`,
                      `状态：${cleanupReviewStatusLabel(item.reviewStatus)}`,
                    ]
                      .filter(Boolean)
                      .join(" · ")}
                  </span>
                  {item.evidence.length > 0 ? (
                    <span>原因：{item.evidence.slice(0, 2).map((evidence) => evidence.summary).join("；")}</span>
                  ) : null}
                  {editingItemId === item.id ? (
                    <div className="settings-subsection">
                      <input
                        className="input"
                        value={editProject}
                        onChange={(event) => setEditProject(event.target.value)}
                        placeholder="最终项目，可留空"
                      />
                      <input
                        className="input"
                        value={editCategory}
                        onChange={(event) => setEditCategory(event.target.value)}
                        placeholder="最终分类"
                      />
                      <input
                        className="input"
                        value={editTargetPath}
                        onChange={(event) => setEditTargetPath(event.target.value)}
                        placeholder="Workspace 内目标路径"
                      />
                      <input
                        className="input"
                        value={editReason}
                        onChange={(event) => setEditReason(event.target.value)}
                        placeholder="修改原因，可选"
                      />
                      <div className="settings-action-row">
                        <button type="button" className="btn btn-primary" onClick={() => submitModification(selectedPlan, item)}>
                          保存修改
                        </button>
                        <button type="button" className="btn" onClick={() => setEditingItemId("")}>
                          取消
                        </button>
                      </div>
                    </div>
                  ) : null}
                  <div className="settings-action-row">
                    <button type="button" className="btn" disabled={isReviewingPlan} onClick={() => reviewItem(selectedPlan, item, "approved")}>
                      确认
                    </button>
                    <button type="button" className="btn" disabled={isReviewingPlan} onClick={() => reviewItem(selectedPlan, item, "rejected")}>
                      拒绝
                    </button>
                    <button type="button" className="btn" disabled={isReviewingPlan} onClick={() => startEditing(item)}>
                      修改建议
                    </button>
                    <button type="button" className="btn" disabled={isReviewingPlan} onClick={() => resetItem(selectedPlan, item)}>
                      退回
                    </button>
                    <button
                      type="button"
                      className="btn"
                      onClick={() => setExpandedTraceItemId(expandedTraceItemId === item.id ? "" : item.id)}
                    >
                      查看原因
                    </button>
                  </div>
                  {expandedTraceItemId === item.id ? (
                    <div className="info-row">
                      <strong>Decision Trace</strong>
                      <span>{item.decisionTrace?.aiUnderstanding || "暂无额外理解"}</span>
                      <span>{item.decisionTrace?.recommendation || "暂无额外建议"}</span>
                    </div>
                  ) : null}
                </article>
              ))}
            </section>
          ) : null}
        </>
      ) : (
        <div className="info-row">
          <strong>最近扫描</strong>
          <span>暂无扫描批次。可以先扫描桌面、下载目录或指定目录生成预览。</span>
        </div>
      )}
    </div>
  );
}

const cleanupPlanFilters = [
  { value: "all", label: "全部" },
  { value: "existingProject", label: "项目资料" },
  { value: "generalWorkMaterial", label: "通用资料" },
  { value: "temporaryOrReference", label: "临时资料" },
  { value: "newProjectCandidate", label: "新项目候选" },
  { value: "needsReview", label: "待确认" },
  { value: "unsupportedOrFailed", label: "无法处理" },
];

function summarizeCleanupPlan(items: CleanupPlanItem[]) {
  return {
    autoSuggested: items.filter((item) => item.requiredAction === "readyForReview").length,
    needsReview: items.filter((item) =>
      ["manualReview", "confirmGeneralLocation", "confirmTemporaryOrReference", "createProjectCandidate"].includes(
        item.requiredAction,
      ),
    ).length,
    unsupported: items.filter((item) => item.recommendedOwnership === "unsupportedOrFailed").length,
  };
}

function cleanupPlanFilterToReviewFilter(value: string): CleanupPlanReviewFilter {
  if (value === "all") return {};
  return { recommendedOwnership: value };
}

function summarizeExecutionPreview(items: CleanupPlanItem[]) {
  const executable = items.filter((item) => item.reviewStatus === "approved");
  return {
    total: executable.length,
    project: executable.filter((item) => item.recommendedOwnership === "existingProject").length,
    general: executable.filter((item) => item.recommendedOwnership === "generalWorkMaterial").length,
    temporary: executable.filter((item) => item.recommendedOwnership === "temporaryOrReference").length,
  };
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

function cleanupPlanStatusLabel(status: string) {
  if (status === "draft") return "草稿";
  if (status === "reviewing") return "待审阅";
  if (status === "approved") return "已确认";
  if (status === "executed") return "已执行";
  if (status === "cancelled") return "已取消";
  return status || "草稿";
}

function cleanupOwnershipLabel(value: string) {
  if (value === "existingProject") return "已有项目资料";
  if (value === "generalWorkMaterial") return "通用工作资料";
  if (value === "temporaryOrReference") return "临时/参考资料";
  if (value === "newProjectCandidate") return "新项目候选";
  if (value === "needsReview") return "待确认";
  if (value === "unsupportedOrFailed") return "无法处理";
  return value || "待确认";
}

function cleanupReviewStatusLabel(value: string) {
  if (value === "approved") return "已确认";
  if (value === "rejected") return "已拒绝";
  if (value === "modified") return "已修改";
  return "待确认";
}

function cleanupExecutionStatusLabel(value: string) {
  if (value === "completed") return "已完成";
  if (value === "failed") return "失败";
  if (value === "cancelled") return "已撤销";
  if (value === "undoConflict") return "撤销冲突";
  return "执行中";
}

function confidenceDisplay(level: string, score: number) {
  const levelText = level === "high" ? "高" : level === "medium" ? "中" : level === "low" ? "低" : "待确认";
  return score > 0 ? `${levelText} ${score}%` : levelText;
}
