import { useState } from "react";
import { DecisionTraceDetails, formatTraceConfidence } from "../../components/DecisionTraceDetails";
import type {
  ProjectImpactAnalysis,
  ProjectActionCandidate,
  ProjectStateProposal,
  DailyContinueSnapshot,
  ProjectImpactFinding,
  ProjectImpactEvidence,
  ProjectAttention,
  ProjectStateSummary,
  WeeklySkillCandidate,
  DataHealthRecord,
  AtlasSkillAssessment,
  PersonalSkill,
  KnowledgePatternCandidate,
  ImprovementCandidate,
  V1Readiness,
} from "./desktopApi";

type ProjectImpactPanelProps = {
  impactAnalyses: ProjectImpactAnalysis[];
  actionCandidates: ProjectActionCandidate[];
  stateProposals: ProjectStateProposal[];
  dailyContinue: DailyContinueSnapshot | null;
  projectStateSummary: ProjectStateSummary | null;
  projectAttentions: ProjectAttention[];
  workPatternCandidates: WeeklySkillCandidate[];
  dataHealthRecords: DataHealthRecord[];
  atlasSkillAssessments: AtlasSkillAssessment[];
  skillLibrary: PersonalSkill[];
  knowledgePatternCandidates: KnowledgePatternCandidate[];
  improvementCandidates: ImprovementCandidate[];
  v1Readiness: V1Readiness | null;
  projectStateAutoApply: boolean;
  onConfirmCandidate: (candidateId: string) => void;
  onIgnoreCandidate: (candidateId: string) => void;
  onUpdateCandidate: (
    candidateId: string,
    title: string,
    description: string,
    suggestedPriority: string,
    suggestedDueDate: string,
  ) => void;
  onApplyProposal: (proposalId: string) => void;
  onUndoProposal: (proposalId: string) => void;
  onMarkTodayDone: (completed: string, nextStep: string) => void;
  onSetAutoApply: (enabled: boolean) => void;
  onRegenerateDailyContinue: () => void;
  onUpdateAttention: (attentionId: string, status: "confirmed" | "ignored" | "later") => void;
  onUpdateDataHealth: (recordId: string, status: "resolved" | "ignored") => void;
  onReload: () => void;
  onClose: () => void;
};

const FINDING_LABELS: Record<string, string> = {
  information: "信息",
  requirement: "需求",
  requirementChange: "需求变更",
  taskCandidate: "任务候选",
  decisionCandidate: "决策候选",
  risk: "风险",
  blocker: "阻塞",
  milestone: "里程碑",
  outcome: "成果",
  question: "待确认问题",
};

const CANDIDATE_LABELS: Record<string, string> = {
  task: "任务",
  decision: "决策",
  risk: "风险",
  blocker: "阻塞",
  requirementChange: "需求变更",
};

const BASIS_LABELS: Record<string, string> = {
  file: "文件事实",
  inference: "AI 推断",
  suggestion: "建议",
  toConfirm: "待确认",
};

function EvidenceList({ items }: { items: ProjectImpactEvidence[] }) {
  if (!items.length) return null;
  return (
    <div className="evidence-group">
      <strong>证据</strong>
      <div className="answer-evidence">
        {items.map((evidence, index) => (
          <div key={`${evidence.sourceHash}-${index}`} className="evidence-item">
            <div className="evidence-item-head">
              <span>
                {evidence.fileName}
                {evidence.section ? ` · ${evidence.section}` : ""}
              </span>
              <span>{BASIS_LABELS[evidence.basisKind] ?? evidence.basisKind}</span>
            </div>
            <p>{evidence.excerpt || "（无摘录）"}</p>
          </div>
        ))}
      </div>
    </div>
  );
}

function FindingCard({ finding }: { finding: ProjectImpactFinding }) {
  return (
    <div className="file-result">
      <div className="file-result-actions" style={{ justifyContent: "space-between" }}>
        <span className="impact-tag">{FINDING_LABELS[finding.findingType] ?? finding.findingType}</span>
        <span className="inline-placeholder">
          置信度 {formatTraceConfidence(Math.round((finding.confidence ?? 0) * 100))}
          {finding.reviewRequired ? " · 待人工确认" : ""}
        </span>
      </div>
      <strong>{finding.title}</strong>
      <p>{finding.description}</p>
      <EvidenceList items={finding.evidence} />
      <DecisionTraceDetails trace={finding.decisionTrace} />
      {finding.suggestedAction ? (
        <p className="inline-notice">建议动作：{finding.suggestedAction}</p>
      ) : null}
    </div>
  );
}

function CandidateCard({
  candidate,
  onConfirm,
  onIgnore,
  onUpdate,
}: {
  candidate: ProjectActionCandidate;
  onConfirm: () => void;
  onIgnore: () => void;
  onUpdate: (title: string, description: string, priority: string, dueDate: string) => void;
}) {
  const [editing, setEditing] = useState(false);
  const [title, setTitle] = useState(candidate.title);
  const [description, setDescription] = useState(candidate.description);
  const [priority, setPriority] = useState(candidate.suggestedPriority);
  const [dueDate, setDueDate] = useState(candidate.suggestedDueDate);

  const settled = candidate.status === "confirmed" || candidate.status === "ignored";

  return (
    <div className="file-result">
      <div className="file-result-actions" style={{ justifyContent: "space-between" }}>
        <span className="impact-tag">
          {CANDIDATE_LABELS[candidate.candidateType] ?? candidate.candidateType}
        </span>
        <span className="inline-placeholder">
          {settled ? (candidate.status === "confirmed" ? "已确认" : "已忽略") : "待处理"}
          {candidate.reviewRequired ? " · 待确认" : ""}
        </span>
      </div>
      <strong>{candidate.title}</strong>
      <p>{candidate.description}</p>
      <div className="inline-notice">
        优先级：{candidate.suggestedPriority || "未指定"} · 截止：{candidate.suggestedDueDate || "未指定"} ·
        置信度 {formatTraceConfidence(Math.round((candidate.confidence ?? 0) * 100))}
      </div>
      <EvidenceList items={candidate.evidence} />
      <DecisionTraceDetails trace={candidate.decisionTrace} />
      {editing ? (
        <div className="wrap-inline">
          <label className="work-block">
            <span>标题</span>
            <input className="composer-file-button" value={title} onChange={(event) => setTitle(event.target.value)} />
          </label>
          <label className="work-block">
            <span>描述</span>
            <textarea value={description} onChange={(event) => setDescription(event.target.value)} />
          </label>
          <label className="work-block">
            <span>优先级</span>
            <input value={priority} onChange={(event) => setPriority(event.target.value)} />
          </label>
          <label className="work-block">
            <span>截止日期</span>
            <input value={dueDate} onChange={(event) => setDueDate(event.target.value)} />
          </label>
          <div className="file-result-actions">
            <button
              type="button"
              className="btn btn-primary"
              onClick={() => {
                onUpdate(title, description, priority, dueDate);
                setEditing(false);
              }}
            >
              保存修改
            </button>
            <button type="button" className="btn" onClick={() => setEditing(false)}>
              取消
            </button>
          </div>
        </div>
      ) : (
        <div className="file-result-actions">
          <button type="button" className="btn btn-primary" disabled={settled} onClick={onConfirm}>
            确认
          </button>
          <button type="button" className="btn" disabled={settled} onClick={onIgnore}>
            忽略
          </button>
          <button type="button" className="btn" disabled={settled} onClick={() => setEditing(true)}>
            修改
          </button>
        </div>
      )}
    </div>
  );
}

function ProposalCard({
  proposal,
  onApply,
  onUndo,
}: {
  proposal: ProjectStateProposal;
  onApply: () => void;
  onUndo: () => void;
}) {
  const applied = proposal.status === "applied" || proposal.status === "autoApplied";
  return (
    <div className="file-result">
      <div className="file-result-actions" style={{ justifyContent: "space-between" }}>
        <span className="impact-tag">状态提案</span>
        <span className="inline-placeholder">
          {applied ? "已应用" : proposal.status === "undone" ? "已撤销" : "待应用"} · 置信度{" "}
          {formatTraceConfidence(Math.round((proposal.confidence ?? 0) * 100))}
        </span>
      </div>
      <div className="evidence-group">
        <strong>改动</strong>
        <div className="answer-evidence">
          {proposal.proposedChanges.map((change, index) => (
            <div key={`${change.field}-${index}`} className="evidence-item">
              <div className="evidence-item-head">
                <span>{change.field}</span>
                <span>{change.reason || "无说明"}</span>
              </div>
              <p>
                {change.before || "（空）"} → {change.after || "（空）"}
              </p>
            </div>
          ))}
          {proposal.before.map((change, index) => (
            <p key={`before-${index}`} className="inline-notice">
              应用前：{change.field} = {change.before || "（空）"}
            </p>
          ))}
        </div>
      </div>
      <EvidenceList items={proposal.evidence} />
      <DecisionTraceDetails trace={proposal.decisionTrace} />
      <div className="file-result-actions">
        {applied ? (
          <button type="button" className="btn" onClick={onUndo}>
            撤销
          </button>
        ) : (
          <button type="button" className="btn btn-primary" onClick={onApply}>
            应用
          </button>
        )}
      </div>
    </div>
  );
}

export function ProjectImpactPanel({
  impactAnalyses,
  actionCandidates,
  stateProposals,
  dailyContinue,
  projectStateSummary,
  projectAttentions,
  workPatternCandidates,
  dataHealthRecords,
  atlasSkillAssessments,
  skillLibrary,
  knowledgePatternCandidates,
  improvementCandidates,
  v1Readiness,
  projectStateAutoApply,
  onConfirmCandidate,
  onIgnoreCandidate,
  onUpdateCandidate,
  onApplyProposal,
  onUndoProposal,
  onMarkTodayDone,
  onSetAutoApply,
  onRegenerateDailyContinue,
  onUpdateAttention,
  onUpdateDataHealth,
  onReload,
  onClose,
}: ProjectImpactPanelProps) {
  const [completed, setCompleted] = useState("");
  const [nextStep, setNextStep] = useState("");
  const pendingCandidates = actionCandidates.filter(
    (candidate) => candidate.status !== "confirmed" && candidate.status !== "ignored",
  );

  return (
    <section className="restore-note project-impact-note">
      <div className="wrap-inline-head">
        <span className="section-label">项目影响分析</span>
        <strong>项目状态 · 主动提醒 · 影响分析 · 今日继续</strong>
      </div>
      <div className="settings-action-row">
        <label className="settings-check-row" style={{ gap: 8 }}>
          <input
            type="checkbox"
            checked={projectStateAutoApply}
            onChange={(event) => onSetAutoApply(event.target.checked)}
          />
          <span className="inline-placeholder">高置信度状态自动应用</span>
        </label>
        <button type="button" className="btn" onClick={onReload}>
          刷新
        </button>
        <button type="button" className="btn" onClick={onClose}>
          关闭
        </button>
      </div>

      <div className="panel-grid">
        <div className="panel-section">
          <span className="section-label">数据健康（{dataHealthRecords.filter((item) => item.status === "open").length}）</span>
          {dataHealthRecords.filter((item) => item.status === "open").length ? (
            dataHealthRecords.filter((item) => item.status === "open").map((record) => (
              <div className="file-result" key={record.id}>
                <div className="file-result-actions" style={{ justifyContent: "space-between" }}>
                  <strong>{record.description}</strong>
                  <span className="inline-placeholder">{record.type} · {record.severity}</span>
                </div>
                <p>{record.suggestedAction}</p>
                <p className="inline-placeholder">来源：{record.source}</p>
                <div className="file-result-actions">
                  <button type="button" className="btn btn-primary" onClick={() => onUpdateDataHealth(record.id, "resolved")}>已处理</button>
                  <button type="button" className="btn" onClick={() => onUpdateDataHealth(record.id, "ignored")}>忽略标记</button>
                </div>
              </div>
            ))
          ) : <p className="inline-placeholder">当前没有会污染 AI 判断的数据质量问题。</p>}
        </div>

        <div className="panel-section">
          <span className="section-label">v1 收口准备</span>
          {v1Readiness?.capabilityOverview?.length ? (
            <div className="file-result">
              <strong>系统能力总览已生成</strong>
              <ul className="insight-block">
                {v1Readiness.capabilityOverview.slice(0, 5).map((item, index) => (
                  <li key={index}>{item}</li>
                ))}
              </ul>
              <p className="inline-placeholder">使用说明：{v1Readiness.usageGuidePath}</p>
              <p className="inline-placeholder">限制列表：{v1Readiness.limitationsPath}</p>
            </div>
          ) : <p className="inline-placeholder">v1 收口文档尚未生成。</p>}
        </div>

        <div className="panel-section">
          <span className="section-label">Atlas Shadow（{atlasSkillAssessments.length}）</span>
          {atlasSkillAssessments.length ? atlasSkillAssessments.map((item) => (
            <div className="file-result" key={item.id}>
              <div className="file-result-actions" style={{ justifyContent: "space-between" }}>
                <strong>{item.skillName}</strong>
                <span className="inline-placeholder">{item.status} · {item.confidence.display}</span>
              </div>
              <p>匹配能力：{item.matchedCapability || "unmatched"}</p>
              <p className="inline-notice">{item.uncoveredPart}</p>
              <DecisionTraceDetails trace={item.decisionTrace} />
            </div>
          )) : <p className="inline-placeholder">暂无可对照 Atlas 的技能候选。</p>}
        </div>

        <div className="panel-section">
          <span className="section-label">个人技能库（{skillLibrary.length}）</span>
          {skillLibrary.length ? skillLibrary.map((skill) => (
            <div className="file-result" key={skill.id}>
              <strong>{skill.name}</strong>
              <p>{skill.scenario} · {skill.status} · {skill.confidence.display}</p>
              <p className="inline-notice">步骤：{skill.steps.join(" → ")}</p>
              <DecisionTraceDetails trace={skill.decisionTrace} />
            </div>
          )) : <p className="inline-placeholder">重复证据不足，暂未形成个人技能候选。</p>}
        </div>

        <div className="panel-section">
          <span className="section-label">跨项目知识候选（{knowledgePatternCandidates.length}）</span>
          {knowledgePatternCandidates.length ? knowledgePatternCandidates.map((pattern) => (
            <div className="file-result" key={pattern.id}>
              <strong>{pattern.patternName}</strong>
              <p>{pattern.reusableValue} · {pattern.confidence.display}</p>
              <p className="inline-notice">项目：{pattern.relatedProjects.join("、")}</p>
              <DecisionTraceDetails trace={pattern.decisionTrace} />
            </div>
          )) : <p className="inline-placeholder">目前只发现单项目记录，暂不生成跨项目模式。</p>}
        </div>

        <div className="panel-section">
          <span className="section-label">自我优化候选（{improvementCandidates.length}）</span>
          {improvementCandidates.length ? improvementCandidates.map((item) => (
            <div className="file-result" key={item.id}>
              <strong>{item.problem}</strong>
              <p>{item.impact}</p>
              <p className="inline-notice">建议：{item.suggestedImprovement} · {item.confidence.display}</p>
              <DecisionTraceDetails trace={item.decisionTrace} />
            </div>
          )) : <p className="inline-placeholder">暂无足够真实反馈形成优化候选。</p>}
        </div>
        <div className="panel-section">
          <span className="section-label">项目状态</span>
          {projectStateSummary?.facts.length ? (
            <div className="file-result">
              <strong>{projectStateSummary.currentPhase || "状态待形成"}</strong>
              <p>下一里程碑：{projectStateSummary.nextMilestone || "暂无真实记录"}</p>
              <div className="evidence-group">
                <strong>事实</strong>
                {projectStateSummary.facts.map((item, index) => <p key={index}>{item}</p>)}
              </div>
              {projectStateSummary.inferences.length ? (
                <p className="inline-notice">AI 判断：{projectStateSummary.inferences.join("；")}</p>
              ) : null}
              {projectStateSummary.suggestions.length ? (
                <p className="inline-notice">建议：{projectStateSummary.suggestions.join("；")}</p>
              ) : null}
              <DecisionTraceDetails trace={projectStateSummary.decisionTrace} />
            </div>
          ) : (
            <p className="inline-placeholder">现有记录不足，不生成虚假项目状态。</p>
          )}
        </div>

        <div className="panel-section">
          <span className="section-label">主动提醒（{projectAttentions.length}）</span>
          {projectAttentions.length ? projectAttentions.map((attention) => (
            <div className="file-result" key={attention.id}>
              <div className="file-result-actions" style={{ justifyContent: "space-between" }}>
                <strong>{attention.title}</strong>
                <span className="inline-placeholder">{attention.confidence.display} · {attention.status}</span>
              </div>
              <p>{attention.reason}</p>
              <p className="inline-notice">建议动作：{attention.suggestedAction}</p>
              <DecisionTraceDetails trace={attention.decisionTrace} />
              <div className="file-result-actions">
                <button type="button" className="btn btn-primary" onClick={() => onUpdateAttention(attention.id, "confirmed")}>确认</button>
                <button type="button" className="btn" onClick={() => onUpdateAttention(attention.id, "later")}>稍后处理</button>
                <button type="button" className="btn" onClick={() => onUpdateAttention(attention.id, "ignored")}>忽略</button>
              </div>
            </div>
          )) : <p className="inline-placeholder">当前没有基于真实记录触发的提醒。</p>}
        </div>

        <div className="panel-section">
          <span className="section-label">工作模式候选（{workPatternCandidates.length}）</span>
          {workPatternCandidates.length ? workPatternCandidates.map((pattern) => (
            <div className="file-result" key={pattern.name}>
              <strong>{pattern.name}</strong>
              <p>{pattern.scenario} · 出现 {pattern.occurrenceCount} 次</p>
              <p className="inline-notice">可复用步骤：{pattern.reusableSteps.join(" → ")}</p>
              <DecisionTraceDetails trace={pattern.decisionTrace} />
            </div>
          )) : <p className="inline-placeholder">真实重复记录不足，暂不生成技能候选。</p>}
        </div>

        <div className="panel-section">
          <span className="section-label">影响分析（{impactAnalyses.length}）</span>
          {impactAnalyses.length === 0 ? (
            <p className="inline-placeholder">暂无影响分析。导入或归类文件后，路由完成后会自动生成。</p>
          ) : (
            impactAnalyses.map((analysis) => (
              <div className="file-result" key={analysis.id}>
                <div className="file-result-actions" style={{ justifyContent: "space-between" }}>
                  <strong>{analysis.summary || "影响分析"}</strong>
                  <span className="inline-placeholder">
                    {analysis.impactLevel || "未知影响"} · 置信度{" "}
                    {formatTraceConfidence(Math.round((analysis.confidence ?? 0) * 100))} · {analysis.status}
                  </span>
                </div>
                <p>{analysis.relevance}</p>
                {analysis.findings.map((finding) => (
                  <FindingCard key={finding.id} finding={finding} />
                ))}
                <EvidenceList items={analysis.evidence} />
                <DecisionTraceDetails trace={analysis.decisionTrace} />
              </div>
            ))
          )}
        </div>

        <div className="panel-section">
          <span className="section-label">行动候选（待处理 {pendingCandidates.length} / 共 {actionCandidates.length}）</span>
          {actionCandidates.length === 0 ? (
            <p className="inline-placeholder">暂无行动候选。</p>
          ) : (
            actionCandidates.map((candidate) => (
              <CandidateCard
                key={candidate.id}
                candidate={candidate}
                onConfirm={() => onConfirmCandidate(candidate.id)}
                onIgnore={() => onIgnoreCandidate(candidate.id)}
                onUpdate={(title, description, priority, dueDate) =>
                  onUpdateCandidate(candidate.id, title, description, priority, dueDate)
                }
              />
            ))
          )}
        </div>

        <div className="panel-section">
          <span className="section-label">状态提案（{stateProposals.length}）</span>
          {stateProposals.length === 0 ? (
            <p className="inline-placeholder">暂无状态提案。</p>
          ) : (
            stateProposals.map((proposal) => (
              <ProposalCard
                key={proposal.id}
                proposal={proposal}
                onApply={() => onApplyProposal(proposal.id)}
                onUndo={() => onUndoProposal(proposal.id)}
              />
            ))
          )}
        </div>

        <div className="panel-section">
          <span className="section-label">今天继续做</span>
          <div className="file-result-actions" style={{ justifyContent: "flex-end" }}>
            <button type="button" className="btn" onClick={onRegenerateDailyContinue}>
              重新生成
            </button>
          </div>
          {dailyContinue ? (
            <div className="file-result">
              <div className="file-result-actions" style={{ justifyContent: "space-between" }}>
                <strong>最近进度快照</strong>
                <span className="inline-placeholder">{dailyContinue.generatedAt || dailyContinue.status}</span>
              </div>
              {dailyContinue.lastProgress.length ? (
                <div className="evidence-group">
                  <strong>上次进度</strong>
                  <ul className="insight-block">
                    {dailyContinue.lastProgress.map((item, index) => (
                      <li key={index}>{item}</li>
                    ))}
                  </ul>
                </div>
              ) : null}
              {dailyContinue.recommendedActions.length ? (
                <div className="evidence-group">
                  <strong>建议动作</strong>
                  <div className="answer-evidence">
                    {dailyContinue.recommendedActions.map((item, index) => (
                      <div key={index} className="evidence-item">
                        <div className="evidence-item-head">
                          <span>{item.action}</span>
                          <span>{item.priority || "普通"}</span>
                        </div>
                        <p>{item.reason}</p>
                        {item.relatedProject ? (
                          <p className="inline-notice">关联项目：{item.relatedProject}</p>
                        ) : null}
                        <DecisionTraceDetails trace={item.decisionTrace} />
                      </div>
                    ))}
                  </div>
                </div>
              ) : null}
              {dailyContinue.blockers.length ? (
                <p className="inline-notice">阻塞：{dailyContinue.blockers.join("；")}</p>
              ) : null}
              {dailyContinue.pendingConfirmations.length ? (
                <p className="inline-notice">待确认：{dailyContinue.pendingConfirmations.join("；")}</p>
              ) : null}
              {dailyContinue.recentChanges.length ? (
                <p className="inline-notice">近期变化：{dailyContinue.recentChanges.join("；")}</p>
              ) : null}
            </div>
          ) : (
            <p className="inline-placeholder">暂无今日继续快照，点击「刷新」生成。</p>
          )}

          <div className="wrap-inline">
            <label className="work-block">
              <span>今日完成</span>
              <textarea
                className="work-input"
                value={completed}
                placeholder="简述今天完成的工作"
                onChange={(event) => setCompleted(event.target.value)}
              />
            </label>
            <label className="work-block">
              <span>下一步</span>
              <input
                className="composer-file-button"
                value={nextStep}
                placeholder="明天或接下来的动作"
                onChange={(event) => setNextStep(event.target.value)}
              />
            </label>
            <div className="file-result-actions">
              <button
                type="button"
                className="btn btn-primary"
                disabled={!completed.trim()}
                onClick={() => {
                  onMarkTodayDone(completed.trim(), nextStep.trim());
                  setCompleted("");
                  setNextStep("");
                }}
              >
                标记今日完成
              </button>
            </div>
          </div>
        </div>
      </div>
    </section>
  );
}
