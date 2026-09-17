import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";

export type ManagedFile = {
  id: string;
  fileName: string;
  originalSourcePath: string;
  managedPath: string;
  fileType: string;
  category: string;
  createdSource: string;
  relatedTask: string;
  currentVersion: string;
  historyVersions: string[];
  createdAt: string;
  modifiedAt: string;
  stillExists: boolean;
  needsUserConfirmation: boolean;
  parseStatus: string;
  contentSummary: string;
  mainFieldsOrSections: string[];
  recommendedCategory: string;
  parseFailureReason: string;
  parser: string;
  extractedTextPath: string;
  pageCount: number | null;
  sheetCount: number | null;
  rowCount: number | null;
  columnCount: number | null;
  analysisWarnings: string[];
  contentHash: string;
  sizeBytes: number;
  versionGroupId: string;
  duplicateOfFileId: string | null;
  previousVersionId: string | null;
  sourcePaths: string[];
  managedRelativePath: string;
  locationReason: string;
  importTransactionId: string;
  lastVerifiedAt: string;
};

export type ProjectSummary = {
  id: string;
  name: string;
  rootDir: string;
  manifestPath: string;
  lastOpenedAt: string;
  nextStep: string;
  createdAt: string;
  description: string;
  repositoryPath?: string;
};

export type WorkspaceConfig = {
  schemaVersion: number;
  workspaceRoot: string;
  inboxRoot: string;
  projectsRoot: string;
  generalRoot: string;
  temporaryRoot: string;
  archiveRoot: string;
  systemRoot: string;
  createdAt: string;
  updatedAt: string;
  status: "ready" | "missing" | "unavailable" | string;
  lastVerifiedAt: string;
};

export type WorkspaceScanFile = {
  id: string;
  path: string;
  fileName: string;
  extension: string;
  sizeBytes: number;
  modifiedAt: string;
  hash: string;
  scanBatchId: string;
  status: string;
  notSuitableForAiContext: boolean;
  parseStatus: string;
  contentSummary: string;
  documentType: string;
  documentPurpose: string;
  ownershipType: string;
  recommendedProjectName: string;
  recommendedLocation: string;
  recommendedCategory: string;
  confidenceLevel: string;
  confidenceScore: number;
  needsConfirmation: boolean;
  failureReason: string;
  decisionTraces: DecisionTrace[];
};

export type WorkspaceScanBatch = {
  id: string;
  sourceDirectory: string;
  createdAt: string;
  completedAt: string;
  fileCount: number;
  status: string;
  failureReason: string;
  organizableCount: number;
  needsConfirmationCount: number;
  unknownCount: number;
  files: WorkspaceScanFile[];
};

export type CleanupPlanItem = {
  id: string;
  sourcePath: string;
  fileName: string;
  hash: string;
  currentLocation: string;
  recommendedOwnership: string;
  recommendedProject: string;
  recommendedTargetPath: string;
  documentPurpose: string;
  confidence: DecisionTraceConfidence;
  evidence: DecisionTraceEvidence[];
  decisionTraceId: string;
  requiredAction: string;
  decisionTrace: DecisionTrace;
  decisionTraces: DecisionTrace[];
  reviewStatus: "pending" | "approved" | "rejected" | "modified" | string;
  reviewedAt: string;
  reviewer: string;
  userReason: string;
  finalProject: string;
  finalCategory: string;
  finalTargetPath: string;
};

export type CleanupPlan = {
  id: string;
  scanBatchId: string;
  createdAt: string;
  status: "draft" | "reviewing" | "approved" | "executed" | "cancelled" | string;
  items: CleanupPlanItem[];
};

export type CleanupPlanReviewFilter = {
  recommendedTargetPathPrefix?: string;
  recommendedOwnership?: string;
  documentPurpose?: string;
  finalCategory?: string;
  confidenceLevel?: string;
};

export type CleanupExecutionItem = {
  cleanupPlanItemId: string;
  sourcePath: string;
  targetPath: string;
  hashBefore: string;
  hashAfter: string;
  operationId: string;
  status: string;
  error: string;
  globalFileId: string;
};

export type CleanupExecutionBatch = {
  id: string;
  cleanupPlanId: string;
  startedAt: string;
  completedAt: string;
  status: string;
  items: CleanupExecutionItem[];
};

export type ExecutionRecord = {
  id: string;
  type: "codexRun" | "cleanupExecution" | string;
  sourceId: string;
  projectId: string;
  status: "queued" | "running" | "succeeded" | "failed" | "cancelled" | "needsReview" | string;
  startedAt: string;
  endedAt: string;
  retryCount: number;
  error: string;
  requiresApproval: boolean;
  approvalStatus: string;
  evidenceRefs: EvidenceRef[];
};

export type MaterialInboxItem = {
  id: string;
  fileName: string;
  sourcePath: string;
  receivedAt: string;
  receivedFrom: string;
  sourceHash: string;
  fileType: string;
  sizeBytes: number;
  parseStatus: string;
  contentSummary: string;
  analysisResult: string;
  mainFieldsOrSections: string[];
  recommendedCategory: string;
  recommendedProjectName: string;
  recommendedLocation: string;
  parseFailureReason: string;
  parser: string;
  judgementStatus: string;
  targetProjectId: string;
  targetProjectRoot: string;
  targetProjectName: string;
  suggestedManagedPath: string;
  suggestedCategory: string;
  duplicateOfFileId: string;
  previousVersionFileId: string;
  proposedProjectName: string;
  decisionBasis: string[];
  processingStatus: string;
  resultNote: string;
  managedFileId: string;
  createdAt: string;
  updatedAt: string;
  statusHistory: Array<{
    from: string;
    to: string;
    changedAt: string;
    reason: string;
    actor: string;
    errorCode: string;
    errorMessage: string;
  }>;
  lastTransitionAt: string;
  errorCode: string;
  errorMessage: string;
  failedStage: string;
  projectCandidates: Array<{
    candidateProjectId: string;
    candidateProjectName: string;
    candidateProjectRoot: string;
    score: number;
    confidence: string;
    reasons: string[];
    evidence: string[];
    matchedSignals: string[];
    ruleScore: number;
    aiScore: number | null;
  }>;
  confidenceLevel: string;
  confidenceScore: number;
  confidenceReasons: string[];
  recommendedRelativeLocation: string;
  locationReason: string;
  suggestedFileName: string;
  duplicateKind: string;
  documentFamilyId: string;
  versionId: string;
  versionNumber: number;
  receivedCount: number;
  sourceHistory: Array<{ sourcePath: string; receivedFrom: string; receivedAt: string; hash: string }>;
  routeOperation: {
    id: string;
    projectId: string;
    projectRoot: string;
    sourcePath: string;
    targetPath: string;
    managedFileId: string;
    sourceHash: string;
    targetHashAtRoute: string;
    createdManagedCopy: boolean;
    createdAt: string;
    status: string;
    undoneAt: string;
    failureReason: string;
  } | null;
  autoRouted: boolean;
  canUndo: boolean;
  aiMatchStatus: string;
  aiMatchModel: string;
  ownershipType: string;
  documentType: string;
  businessDomain: string;
  businessPurpose: string;
  businessSummary: string;
  documentPurpose: string;
  documentPurposeConfidence: number;
  documentPurposeEvidence: string[];
  technicalDetail: string;
  analysisVersion: number;
  generalMaterialDomain: string;
  generalMaterialCategory: string;
  materialSemanticType: string;
  projectCandidateScore: number;
  projectCandidateReasons: string[];
  projectCandidateEvidence: string[];
  projectCandidateConfidence: string;
  decisionTraces: DecisionTrace[];
  globalDestination: string;
};

export type InboxRoutingSettings = {
  autoRouteHighConfidence: boolean;
  updatedAt: string;
};

export type WorkspaceMessage = {
  id: string;
  author: string;
  kind: string;
  text: string;
  createdAt: string;
  status: string;
  attachments: Array<{ fileId: string; fileName: string; managedPath: string }>;
  relatedTask: string;
  source: string;
  modelId: string;
  evidenceItems: Array<{
    id: string;
    title: string;
    contentType: string;
    basisKind: string;
    summary: string;
    managedPath: string;
    sourceRecordId: string;
  }>;
};

export type TaskRecord = {
  id: string;
  title: string;
  status: string;
  sourceMessageId: string;
  createdAt: string;
  updatedAt: string;
};

export type DecisionRecord = {
  id: string;
  summary: string;
  sourceMessageId: string;
  createdAt: string;
};

export type ArtifactRecord = {
  id: string;
  title: string;
  artifactType: string;
  source: string;
  sourceMessageId: string;
  fileId: string;
  managedPath: string;
  relatedTask: string;
  createdAt: string;
};

export type CodexPromptRecord = {
  id: string;
  title: string;
  relatedTask: string;
  promptText: string;
  fileId: string;
  managedPath: string;
  contentHash: string;
  createdAt: string;
};

export type CodexReportRecord = {
  id: string;
  sourceKind: string;
  sourceLabel: string;
  reportHash: string;
  evidenceFileId: string;
  evidenceManagedPath: string;
  parseStatus: string;
  completedStatus: string;
  summary: string;
  modifiedContent: string[];
  testResults: string[];
  commit: string;
  unresolvedIssues: string[];
  nextStepSuggestions: string[];
  parseFailureReason: string;
  duplicateOfReportId: string | null;
  applied: boolean;
  confirmedAt: string;
  createdAt: string;
};

export type EvidenceRef = {
  kind: string;
  id: string;
  pathSnapshot: string;
  hashSnapshot: string;
  label: string;
};

export type FileProjection = {
  identity: {
    fileId: string;
    hash: string;
    originalName: string;
    sourcePaths: string[];
    createdAt: string;
  };
  metadata: {
    fileType: string;
    documentType: string;
    documentPurpose: string;
    businessDomain: string;
    category: string;
    lifecycleStatus: string;
    duplicateOf: string;
    versionGroupId: string;
    versionNumber: number;
  };
  location: {
    originalPath: string;
    managedPath: string;
    workspaceRelativePath: string;
    projectId: string;
    ownershipType: string;
    generalCategory: string;
  };
  summary: string;
  sourceModel: string;
  evidenceRefs: EvidenceRef[];
};

export type CodexTaskGitVerification = {
  status: string;
  repositoryPath: string;
  checkedAt: string;
  reason: string;
};

export type CodexTaskAcceptance = {
  status: string;
  reason: string;
  decidedAt: string;
};

export type CodexTask = {
  taskId: string;
  projectId: string;
  title: string;
  taskType: string;
  prompt: string;
  createdAt: string;
  handedOffAt: string;
  resultReceivedAt: string;
  completedAt: string;
  status: string;
  repositoryPath: string;
  expectedResultPath: string;
  resultId: string;
  resultSource: string;
  resultRunId: string;
  summary: string;
  resultText: string;
  changedFiles: string[];
  reportedCommits: string[];
  verifiedCommits: string[];
  gitVerification: CodexTaskGitVerification;
  tests: string[];
  findings: string[];
  recommendations: string[];
  questions: string[];
  checks: string[];
  passed: string[];
  failed: string[];
  artifacts: string[];
  targetFiles: string[];
  remainingIssues: string[];
  manualAcceptance: string[];
  acceptance: CodexTaskAcceptance;
  evidenceRefs: string[];
  updatedAt: string;
};

export type CodexCliCapabilityProbe = {
  available: boolean;
  version: string;
  supportsCd: boolean;
  supportsAddDir: boolean;
  supportsJson: boolean;
  supportsOutputLastMessage: boolean;
  supportsSandbox: boolean;
  supportsStdin: boolean;
  checkedAt: string;
  failureReason: string;
};

export type CodexRun = {
  id: string;
  taskId: string;
  projectId: string;
  repositoryPath: string;
  commandExecutable: string;
  commandArgs: string[];
  capabilityProbe: CodexCliCapabilityProbe;
  workingDirectory: string;
  stdinPromptSummary: string;
  status: string;
  startedAt: string;
  endedAt: string;
  pid: number;
  exitCode: number | null;
  outputLastMessagePath: string;
  stdoutSummary: string;
  stderrSummary: string;
  error: string;
};

export type CodexTaskList = {
  tasks: CodexTask[];
  runs: CodexRun[];
};

export type DailySession = {
  id: string;
  dateKey: string;
  startedAt: string;
  updatedAt: string;
  messageIds: string[];
  taskIds: string[];
  decisionIds: string[];
  artifactIds: string[];
  importedFileIds: string[];
  recoveryPointIds: string[];
};

export type LocationDecision = {
  id: string;
  fileName: string;
  sourcePath: string;
  managedPath: string;
  managedRelativePath: string;
  category: string;
  fileType: string;
  createdSource: string;
  relatedTask: string;
  purpose: string;
  version: string;
  reason: string;
  requiresConfirmation: boolean;
  createdAt: string;
};

export type AtlasAssessment = {
  atlasVersion: string;
  status: string;
  reusableParts: string[];
  uncoveredParts: string[];
  trainingCandidates: string[];
  evidenceReferences: string[];
  reviewRequired: boolean;
  assessedAt: string;
  failureReason: string;
};

export type ProjectAnalysis = {
  status: string;
  projectDefinition: string;
  goals: string[];
  roles: string[];
  materialUsage: string[];
  knownRequirements: string[];
  gaps: string[];
  questions: string[];
  constraints: string[];
  evidence: string[];
  nextSteps: string[];
  updatedAt: string;
  modelId: string;
  failureReason: string;
};

export type PendingReviewItem = {
  id: string;
  key: string;
  kind: string;
  title: string;
  detail: string;
  fileId: string;
  path: string;
  suggestedManagedPath: string;
  suggestedCategory: string;
  status: string;
  detectedAt: string;
  updatedAt: string;
};

export type MonitoringState = {
  status: string;
  lastScannedAt: string;
  lastEventAt: string;
  lastError: string;
  watchStartedAt: string;
  pendingCount: number;
};

export type WorkspaceDraft = {
  projectId: string;
  text: string;
  pendingFilePaths: string[];
  updatedAt: string;
};

export type RecoveryPoint = {
  id: string;
  projectId: string;
  completed: string;
  nextStep: string;
  createdAt: string;
  messageCount: number;
  fileCount: number;
};

export type ProjectManifest = {
  schemaVersion: number;
  project: ProjectSummary;
  files: ManagedFile[];
  messages: WorkspaceMessage[];
  tasks: TaskRecord[];
  decisions: DecisionRecord[];
  artifacts: ArtifactRecord[];
  codexPrompts: CodexPromptRecord[];
  codexReports: CodexReportRecord[];
  dailySessions: DailySession[];
  locationDecisions: LocationDecision[];
  pendingReviews: PendingReviewItem[];
  monitoring: MonitoringState;
  atlas: AtlasAssessment;
  projectAnalysis: ProjectAnalysis;
  audit: Array<{
    id: string;
    action: string;
    target: string;
    outcome: string;
    createdAt: string;
    details: string;
    requiresConfirmation: boolean;
    confirmed: boolean;
  }>;
  draft: WorkspaceDraft;
  recoveryPoints: RecoveryPoint[];
  projectImpactAnalyses: ProjectImpactAnalysis[];
  projectActionCandidates: ProjectActionCandidate[];
  projectStateProposals: ProjectStateProposal[];
  dailyContinueSnapshots: DailyContinueSnapshot[];
  projectStateSummary: ProjectStateSummary;
  projectAttentions: ProjectAttention[];
  workPatternCandidates: WeeklySkillCandidate[];
  projectStateAutoApply: boolean;
  dataHealthRecords?: DataHealthRecord[];
  atlasSkillAssessments?: AtlasSkillAssessment[];
  skillLibrary?: PersonalSkill[];
  knowledgePatternCandidates?: KnowledgePatternCandidate[];
  improvementCandidates?: ImprovementCandidate[];
  v1Readiness?: V1Readiness;
  deepseekAuthorization: {
    grantedAt: string;
    grantedBy: string;
  };
};

export type DataHealthRecord = {
  id: string;
  type: string;
  severity: string;
  source: string;
  description: string;
  suggestedAction: string;
  status: string;
  createdAt: string;
};

export type AtlasSkillAssessment = {
  id: string;
  skillName: string;
  matchedCapability: string;
  reusableEvidence: string[];
  uncoveredPart: string;
  similarity: number;
  confidence: DecisionTraceConfidence;
  reviewRequired: boolean;
  status: string;
  assessedAt: string;
  evidenceReferences: string[];
  decisionTrace: DecisionTrace;
};

export type PersonalSkill = {
  id: string;
  name: string;
  scenario: string;
  evidence: string[];
  frequency: number;
  steps: string[];
  examples: string[];
  confidence: DecisionTraceConfidence;
  status: string;
  createdAt: string;
  updatedAt: string;
  decisionTrace: DecisionTrace;
};

export type KnowledgePatternCandidate = {
  id: string;
  patternName: string;
  relatedProjects: string[];
  evidence: string[];
  commonSteps: string[];
  reusableValue: string;
  confidence: DecisionTraceConfidence;
  reviewRequired: boolean;
  createdAt: string;
  updatedAt: string;
  decisionTrace: DecisionTrace;
};

export type ImprovementCandidate = {
  id: string;
  problem: string;
  evidence: string[];
  impact: string;
  suggestedImprovement: string;
  confidence: DecisionTraceConfidence;
  status: string;
  createdAt: string;
  updatedAt: string;
  decisionTrace: DecisionTrace;
};

export type V1Readiness = {
  generatedAt: string;
  overviewPath: string;
  usageGuidePath: string;
  limitationsPath: string;
  roadmapPath: string;
  capabilityOverview: string[];
  currentLimitations: string[];
  roadmapCandidates: string[];
  securityChecks: string[];
  installerPaths: string[];
};

export type MemoItem = {
  id: string;
  text: string;
  createdAt: string;
  updatedAt: string;
};

export type ProjectCreateResult = {
  project: ProjectSummary;
  files: ManagedFile[];
  messages: WorkspaceMessage[];
  atlas: AtlasAssessment;
};

export type ImportResult = {
  files: ManagedFile[];
  messages: WorkspaceMessage[];
  atlas: AtlasAssessment;
  duplicates: ManagedFile[];
};

export type MaterialInboxRouteResult = {
  item: MaterialInboxItem;
  project: ProjectSummary | null;
  files: ManagedFile[];
};

export type MessageResult = {
  project: ProjectSummary;
  messages: WorkspaceMessage[];
  streamMessageId: string;
};

export type FinishWorkResult = {
  project: ProjectSummary;
  message: WorkspaceMessage;
  recoveryPoint: RecoveryPoint;
};

export type GeneratedFileResult = {
  file: ManagedFile;
  message: WorkspaceMessage;
};

export type CodexPromptResult = {
  prompt: CodexPromptRecord;
  manifest: ProjectManifest;
};

export type CodexReportImportResult = {
  report: CodexReportRecord;
  manifest: ProjectManifest;
  duplicate: boolean;
};

export type CodexResultBridgeScanResult = {
  resultDir: string;
  scannedCount: number;
  importedCount: number;
  reports: CodexReportRecord[];
  manifest: ProjectManifest;
};

export type CodexReportApplyResult = {
  report: CodexReportRecord;
  manifest: ProjectManifest;
};

export type GlobalSearchResult = {
  id: string;
  fileId: string;
  hash: string;
  projectId: string;
  projectRoot: string;
  projectName: string;
  contentType: string;
  title: string;
  snippet: string;
  updatedAt: string;
  managedPath: string;
  workspaceRelativePath: string;
  fileType: string;
  ownershipType: string;
  category: string;
  documentType: string;
  documentPurpose: string;
  businessDomain: string;
  lifecycleStatus: string;
  duplicateOf: string;
  versionGroupId: string;
  versionNumber: number;
  evidenceRefs: EvidenceRef[];
  decisionTraceId: string;
  recentStatus: string;
  confidenceDisplay: string;
  matchedField: string;
  matchSnippet: string;
};

export type DeepSeekSettings = {
  hasApiKey: boolean;
  selectedModelId: string;
  lastTestedAt: string;
};

export type DeepSeekModelInfo = {
  id: string;
  ownedBy: string;
};

export type DeepSeekConnectionResult = {
  settings: DeepSeekSettings;
  models: DeepSeekModelInfo[];
};

export type DeepSeekStreamEvent = {
  projectRoot: string;
  messageId: string;
  status: string;
  delta: string;
  text: string;
  error: string;
  modelId: string;
};

export type LocalBackupResult = {
  backupDir: string;
  globalFiles: string[];
  projectRoots: string[];
  excludedEntries: string[];
  checksumManifestPath: string;
  createdAt: string;
};

export type BackupRestoreResult = {
  backupDir: string;
  restoredGlobalDir: string;
  restoredProjects: ProjectSummary[];
  restoredGlobalFiles: string[];
  warnings: string[];
  restoredAt: string;
};

export type ProjectMigrationResult = {
  project: ProjectSummary;
  oldRootDir: string;
  newRootDir: string;
  retainedOldRoot: boolean;
  copiedEntries: number;
  migratedAt: string;
};

export type SafeExportResult = {
  exportDir: string;
  includedFiles: string[];
  exportedFileCount: number;
  redactedMessageCount: number;
  exportedAt: string;
};

export type PrivacyArtifactsResult = {
  privacyNoticePath: string;
  thirdPartyNoticesPath: string;
  diagnosticLogPath: string;
  generatedAt: string;
};

export type DecisionTraceEvidence = {
  kind: string;
  label: string;
  summary: string;
  sourceId: string;
};

export type DecisionTraceConfidence = {
  score: number;
  level: "high" | "medium" | "low" | string;
  display: string;
};

export type DecisionTrace = {
  id: string;
  type: string;
  subjectId: string;
  projectId: string;
  inputEvidence: DecisionTraceEvidence[];
  aiUnderstanding: string;
  recommendation: string;
  confidence: DecisionTraceConfidence;
  userDecision: "approved" | "rejected" | "modified" | "pending" | string;
  userDecisionNote: string;
  execution: "executed" | "failed" | "cancelled" | "pending" | string;
  executionNote: string;
  createdAt: string;
  updatedAt: string;
};

export type WeeklyReviewSettings = {
  generationWeekday: number;
  lastAutoGeneratedWeekKey: string;
  updatedAt: string;
};

export type WeeklyTaskDistributionItem = {
  label: string;
  count: number;
};

export type WeeklyRepeatedPattern = {
  label: string;
  count: number;
  evidence: string[];
};

export type WeeklyHabitSummary = {
  effectiveHabits: string[];
  inefficientLoops: string[];
  frequentSwitches: string[];
  repeatedRework: string[];
};

export type WeeklySkillCandidate = {
  name: string;
  scenario: string;
  evidence: string[];
  occurrenceCount: number;
  reusableSteps: string[];
  expectedValue: string;
  currentGap: string;
  confidence: number;
  reviewRequired: boolean;
  decisionTrace: DecisionTrace;
};

export type WeeklyProjectProgress = {
  projectId: string;
  projectName: string;
  completedItems: string[];
  blockers: string[];
  unfinishedItems: string[];
  nextSteps: string[];
};

export type WeeklyReportRecord = {
  id: string;
  scope: string;
  projectId: string;
  projectRoot: string;
  projectName: string;
  weekKey: string;
  weekStart: string;
  weekEnd: string;
  version: number;
  generatedAt: string;
  updatedAt: string;
  confirmedAt: string;
  reviewRequired: boolean;
  markdown: string;
  jsonPath: string;
  markdownPath: string;
  pdfPath: string;
  completedItems: string[];
  achievements: string[];
  blockers: string[];
  unfinishedItems: string[];
  facts: string[];
  analysis: string[];
  suggestions: string[];
  pendingConfirmations: string[];
  taskDistribution: WeeklyTaskDistributionItem[];
  repeatedOperations: WeeklyRepeatedPattern[];
  repeatedIssues: WeeklyRepeatedPattern[];
  habitSummary: WeeklyHabitSummary;
  skillCandidates: WeeklySkillCandidate[];
  projectProgress: WeeklyProjectProgress[];
  evidence: string[];
  dataHealthRecords?: DataHealthRecord[];
};

export type WeeklyReviewDashboard = {
  settings: WeeklyReviewSettings;
  currentWeekKey: string;
  weekStart: string;
  weekEnd: string;
  generatedAt: string;
  generatedNow: boolean;
  globalReports: WeeklyReportRecord[];
  projectReports: WeeklyReportRecord[];
  activeProjectRoot: string;
};

export type WeeklyReportExportResult = {
  reportId: string;
  destinationDir: string;
  exportedPaths: string[];
  exportedAt: string;
};

export type ProjectImpactEvidence = {
  basisKind: string;
  fileId: string;
  fileName: string;
  section: string;
  excerpt: string;
  sourceHash: string;
  granularity: string;
};

export type ProjectImpactFinding = {
  id: string;
  findingType: string;
  title: string;
  description: string;
  confidence: number;
  evidence: ProjectImpactEvidence[];
  sourceReference: string;
  suggestedAction: string;
  basisKind: string;
  reviewRequired: boolean;
  status: string;
  decisionTrace: DecisionTrace;
};

export type ProjectImpactAnalysis = {
  id: string;
  sourceFileId: string;
  sourceHash: string;
  projectId: string;
  generatedAt: string;
  analysisVersion: string;
  summary: string;
  relevance: string;
  impactLevel: string;
  findings: ProjectImpactFinding[];
  evidence: ProjectImpactEvidence[];
  confidence: number;
  status: string;
  failureReason: string;
  decisionTrace: DecisionTrace;
};

export type ProjectActionCandidate = {
  id: string;
  candidateType: string;
  title: string;
  description: string;
  suggestedPriority: string;
  suggestedDueDate: string;
  relatedFileId: string;
  evidence: ProjectImpactEvidence[];
  confidence: number;
  reviewRequired: boolean;
  status: string;
  sourceImpactId: string;
  sourceHash: string;
  createdAt: string;
  updatedAt: string;
  appliedRecordId: string;
  decisionTrace: DecisionTrace;
};

export type ProjectStateChange = {
  field: string;
  before: string;
  after: string;
  reason: string;
};

export type ProjectStateProposal = {
  id: string;
  before: ProjectStateChange[];
  proposedChanges: ProjectStateChange[];
  evidence: ProjectImpactEvidence[];
  confidence: number;
  sourceFileIds: string[];
  sourceHashes: string[];
  createdAt: string;
  appliedAt: string;
  status: string;
  previousNextStep: string;
  decisionTrace: DecisionTrace;
};

export type DailyContinueItem = {
  action: string;
  reason: string;
  evidence: string[];
  relatedProject: string;
  relatedFiles: string[];
  priority: string;
  itemType: "fact" | "suggestion" | "inference" | string;
  source: string;
  confidence: DecisionTraceConfidence;
  category: string;
  decisionTrace: DecisionTrace;
};

export type ProjectStateSummary = {
  projectId: string;
  currentPhase: string;
  recentProgress: string[];
  currentRisks: string[];
  blockers: string[];
  nextMilestone: string;
  recentChanges: string[];
  facts: string[];
  inferences: string[];
  suggestions: string[];
  evidence: DecisionTraceEvidence[];
  generatedAt: string;
  decisionTrace: DecisionTrace;
};

export type ProjectAttention = {
  id: string;
  projectId: string;
  attentionType: string;
  title: string;
  reason: string;
  evidence: DecisionTraceEvidence[];
  suggestedAction: string;
  confidence: DecisionTraceConfidence;
  status: string;
  createdAt: string;
  updatedAt: string;
  decisionTrace: DecisionTrace;
};

export type TodayProjectFocus = {
  projectId: string;
  projectName: string;
  projectRoot: string;
  currentStatus: string;
  recentChange: string;
  nextStep: string;
  updatedAt: string;
};

export type TodayBlocker = {
  category: string;
  title: string;
  relatedProject: string;
  evidence: string[];
};

export type TodayPendingItem = {
  kind: string;
  title: string;
  relatedProject: string;
  sourceId: string;
};

export type ContinueWorkPrimaryAction = {
  type: string;
  label: string;
  projectId: string;
  projectRoot: string;
  sourceId: string;
  panel: string;
};

export type ContinueWorkFocus = {
  projectId: string;
  title: string;
  summary: string;
  reason: string;
  priority: string;
  confidence: DecisionTraceConfidence;
  freshnessStatus: "fresh" | "stale" | "unknown" | string;
  evidenceRefs: EvidenceRef[];
  sourceActionIds: string[];
  primaryAction: ContinueWorkPrimaryAction;
  generatedAt: string;
};

export type CodexTaskTodayItem = {
  taskId: string;
  projectId: string;
  projectName: string;
  projectRoot: string;
  title: string;
  status: string;
  gitStatus: string;
  manualAcceptanceCount: number;
  updatedAt: string;
};

export type CodexTaskTodaySummary = {
  activeOrDelivered: number;
  awaitingAcceptance: number;
  abnormal: number;
  latestTasks: CodexTaskTodayItem[];
};

export type TodayWorkspace = {
  generatedAt: string;
  continueWorkFocus?: ContinueWorkFocus | null;
  lastProgress: string[];
  focusProjects: TodayProjectFocus[];
  recommendedActions: DailyContinueItem[];
  blockers: TodayBlocker[];
  pendingItems: TodayPendingItem[];
  workspaceActivity: WorkspaceActivitySummary;
  codexTaskSummary: CodexTaskTodaySummary;
  activityTimeline?: ActivityProjection[];
  pendingActions?: PendingActionProjection[];
  executions?: ExecutionRecord[];
  status: string;
};

export type WorkEvent = {
  id: string;
  projectId: string;
  sourceType: "git" | "codex" | "user" | "workspace" | "system" | string;
  sourceRef: string;
  eventType: string;
  factKind: "fact" | "inference" | "suggestion" | "userConfirmation" | string;
  occurredAt: string;
  summary: string;
  evidenceRefs: string[];
  confidence: DecisionTraceConfidence;
  decisionTraceId: string;
  decisionTrace: DecisionTrace;
  createdAt: string;
};

export type ProjectFactCaptureRequest = {
  captureType: "progress" | "decision" | "blocker" | "nextAction" | "resolveAction";
  content: string;
  reason?: string;
  actionSourceRef?: string;
};

export type ActivityProjection = {
  id: string;
  projectId: string;
  type: string;
  sourceType: string;
  occurredAt: string;
  summary: string;
  evidenceRefs: EvidenceRef[];
  actor: string;
  confidence: DecisionTraceConfidence;
  userVisible: boolean;
};

export type PendingActionProjection = {
  id: string;
  type: string;
  projectId: string;
  title: string;
  reason: string;
  sourceRef: string;
  priority: string;
  status: "pending" | "approved" | "rejected" | "resolved" | "ignored" | string;
  createdAt: string;
  evidenceRefs: EvidenceRef[];
};

export type GitCommitInfo = {
  hash: string;
  shortHash: string;
  subject: string;
  author: string;
  timestamp: string;
};

export type GitChangedFile = {
  status: string;
  path: string;
};

export type GitSnapshot = {
  id: string;
  projectId: string;
  repositoryPath: string;
  branch: string;
  head: string;
  headShort: string;
  recentCommits: GitCommitInfo[];
  isDirty: boolean;
  changedFiles: GitChangedFile[];
  tags: string[];
  status: string;
  failureReason: string;
  capturedAt: string;
};

export type CodexExternalResult = {
  id: string;
  projectId: string;
  sourceHash: string;
  sourceLabel: string;
  resultSource?: string;
  taskId: string;
  taskType: string;
  status: string;
  resultText: string;
  completedContent: string[];
  changedFiles: string[];
  commits: string[];
  tests: string[];
  findings: string[];
  recommendations: string[];
  questions: string[];
  checks: string[];
  passed: string[];
  failed: string[];
  artifacts: string[];
  targetFiles: string[];
  unresolvedItems: string[];
  manualAcceptance: string[];
  externalThreadId: string;
  resultRunId: string;
  rawEvidencePath: string;
  createdAt: string;
};

export type WorkLedgerSnapshot = {
  events: WorkEvent[];
  gitSnapshot: GitSnapshot | null;
  codexResults: CodexExternalResult[];
  codexTasks: CodexTask[];
  activityTimeline?: ActivityProjection[];
  pendingActions?: PendingActionProjection[];
  executions?: ExecutionRecord[];
};

export type ProjectContextFocus = {
  title: string;
  summary: string;
  reason: string;
  evidence: string[];
};

export type ProjectContextActivity = {
  occurredAt: string;
  summary: string;
  evidence: string[];
};

export type ProjectContextPendingAction = {
  title: string;
  reason: string;
  priority: string;
};

export type ProjectContextFile = {
  name: string;
  documentPurpose: string;
  lifecycleStatus: string;
  location: string;
  summary: string;
};

export type ProjectContextCodexResult = {
  title: string;
  taskType: string;
  status: string;
  summary: string;
  manualAcceptance: string[];
};

export type ProjectContextPacket = {
  projectId: string;
  projectName: string;
  generatedAt: string;
  privacyNotice: string;
  focus: ProjectContextFocus | null;
  recentActivity: ProjectContextActivity[];
  pendingActions: ProjectContextPendingAction[];
  files: ProjectContextFile[];
  codexResult: ProjectContextCodexResult | null;
  sparse: boolean;
  markdown: string;
};

export type WorkspaceActivityItem = {
  kind: string;
  title: string;
  summary: string;
  projectName: string;
  category: string;
  managedPath: string;
  decisionTraceId: string;
  occurredAt: string;
};

export type WorkspaceActivitySummary = {
  newManagedFiles: number;
  projectFiles: number;
  generalFiles: number;
  temporaryFiles: number;
  pendingCleanupPlans: number;
  failedItems: number;
  conflicts: number;
  versionedFiles: number;
  recentItems: WorkspaceActivityItem[];
};

export type DailyContinueSnapshot = {
  id: string;
  projectId: string;
  generatedAt: string;
  lastProgress: string[];
  recommendedActions: DailyContinueItem[];
  blockers: string[];
  pendingConfirmations: string[];
  recentChanges: string[];
  status: string;
};

export async function getImpactAnalyses(projectRoot: string) {
  return invoke<ProjectImpactAnalysis[]>("get_impact_analyses", { projectRoot });
}

export async function getActionCandidates(projectRoot: string) {
  return invoke<ProjectActionCandidate[]>("get_action_candidates", { projectRoot });
}

export async function getStateProposals(projectRoot: string) {
  return invoke<ProjectStateProposal[]>("get_state_proposals", { projectRoot });
}

export async function confirmActionCandidate(projectRoot: string, candidateId: string) {
  return invoke<ProjectActionCandidate>("confirm_action_candidate", { projectRoot, candidateId });
}

export async function ignoreActionCandidate(projectRoot: string, candidateId: string) {
  return invoke<ProjectActionCandidate>("ignore_action_candidate", { projectRoot, candidateId });
}

export async function updateActionCandidate(
  projectRoot: string,
  candidateId: string,
  title: string,
  description: string,
  suggestedPriority: string,
  suggestedDueDate: string,
) {
  return invoke<ProjectActionCandidate>("update_action_candidate", {
    projectRoot,
    candidateId,
    title,
    description,
    suggestedPriority,
    suggestedDueDate,
  });
}

export async function applyStateProposal(projectRoot: string, proposalId: string) {
  return invoke<ProjectStateProposal>("apply_state_proposal", { projectRoot, proposalId });
}

export async function undoStateProposal(projectRoot: string, proposalId: string) {
  return invoke<ProjectStateProposal>("undo_state_proposal", { projectRoot, proposalId });
}

export async function getDailyContinue(projectRoot: string) {
  return invoke<DailyContinueSnapshot>("get_daily_continue", { projectRoot });
}

export async function regenerateDailyContinue(projectRoot: string) {
  return invoke<DailyContinueSnapshot>("regenerate_daily_continue", { projectRoot });
}

export async function getTodayWorkspace() {
  return invoke<TodayWorkspace>("get_today_workspace");
}

export async function updateProjectAttention(projectRoot: string, attentionId: string, status: string) {
  return invoke<ProjectAttention>("update_project_attention", { projectRoot, attentionId, status });
}

export async function updateDataHealthStatus(projectRoot: string, recordId: string, status: string) {
  return invoke<ProjectManifest>("update_data_health_status", { projectRoot, recordId, status });
}

export async function updatePendingReviewStatus(projectRoot: string, reviewId: string, status: string) {
  return invoke<ProjectManifest>("update_pending_review_status", { projectRoot, reviewId, status });
}

export async function markTodayDone(projectRoot: string, completed: string, nextStep: string) {
  return invoke<FinishWorkResult>("mark_today_done", { projectRoot, completed, nextStep });
}

export async function setStateAutoApply(projectRoot: string, enabled: boolean) {
  return invoke<ProjectManifest>("set_state_auto_apply", { projectRoot, enabled });
}

export function isTauriRuntime() {
  return "__TAURI_INTERNALS__" in window;
}

export async function chooseProjectRoot() {
  const result = await open({ directory: true, multiple: false, title: "选择项目根目录" });
  return typeof result === "string" ? result : null;
}

export async function chooseDirectory(title: string) {
  const result = await open({ directory: true, multiple: false, title });
  return typeof result === "string" ? result : null;
}

export async function chooseImportFiles() {
  const result = await open({ directory: false, multiple: true, title: "选择项目资料" });
  if (!result) return [];
  return Array.isArray(result) ? result : [result];
}

export async function listProjects() {
  return invoke<ProjectSummary[]>("list_projects");
}

export async function listFileProjections() {
  return invoke<FileProjection[]>("list_file_projections");
}

export async function getWorkspaceConfig() {
  return invoke<WorkspaceConfig>("get_workspace_config");
}

export async function initializeWorkspaceRoot(root?: string | null) {
  return invoke<WorkspaceConfig>("initialize_workspace_root", { root: root ?? null });
}

export async function verifyWorkspaceRoot() {
  return invoke<WorkspaceConfig>("verify_workspace_root");
}

export async function listWorkspaceScanBatches() {
  return invoke<WorkspaceScanBatch[]>("list_workspace_scan_batches");
}

export async function scanWorkspaceDirectory(sourceDirectory: string) {
  return invoke<WorkspaceScanBatch>("scan_workspace_directory", { sourceDirectory });
}

export async function scanDesktopDirectory() {
  return invoke<WorkspaceScanBatch>("scan_desktop_directory");
}

export async function scanDownloadsDirectory() {
  return invoke<WorkspaceScanBatch>("scan_downloads_directory");
}

export async function generateCleanupPlan(scanBatchId: string) {
  return invoke<CleanupPlan>("generate_cleanup_plan", { scanBatchId });
}

export async function listCleanupPlans() {
  return invoke<CleanupPlan[]>("list_cleanup_plans");
}

export async function getCleanupPlan(planId: string) {
  return invoke<CleanupPlan>("get_cleanup_plan", { planId });
}

export async function reviewCleanupPlanItems(
  planId: string,
  itemIds: string[],
  reviewStatus: "approved" | "rejected" | string,
  userReason?: string,
) {
  return invoke<CleanupPlan>("review_cleanup_plan_items", {
    planId,
    itemIds,
    reviewStatus,
    userReason: userReason ?? null,
  });
}

export async function bulkReviewCleanupPlan(
  planId: string,
  filter: CleanupPlanReviewFilter,
  reviewStatus: "approved" | "rejected" | string,
  userReason?: string,
) {
  return invoke<CleanupPlan>("bulk_review_cleanup_plan", {
    planId,
    filter,
    reviewStatus,
    userReason: userReason ?? null,
  });
}

export async function modifyCleanupPlanItem(
  planId: string,
  itemId: string,
  finalTargetPath: string,
  finalProject?: string,
  finalCategory?: string,
  userReason?: string,
) {
  return invoke<CleanupPlan>("modify_cleanup_plan_item", {
    planId,
    itemId,
    finalProject: finalProject ?? null,
    finalCategory: finalCategory ?? null,
    finalTargetPath,
    userReason: userReason ?? null,
  });
}

export async function resetCleanupPlanItems(planId: string, itemIds: string[]) {
  return invoke<CleanupPlan>("reset_cleanup_plan_items", { planId, itemIds });
}

export async function executeCleanupPlan(planId: string) {
  return invoke<CleanupExecutionBatch>("execute_cleanup_plan", { planId });
}

export async function listCleanupExecutionBatches() {
  return invoke<CleanupExecutionBatch[]>("list_cleanup_execution_batches");
}

export async function listExecutionRecords(projectRoot?: string) {
  return invoke<ExecutionRecord[]>("list_execution_records", { projectRoot: projectRoot || null });
}

export async function undoCleanupExecutionBatch(batchId: string) {
  return invoke<CleanupExecutionBatch>("undo_cleanup_execution_batch", { batchId });
}

export async function loadProject(projectRoot: string) {
  return invoke<ProjectManifest>("load_project", { projectRoot });
}

export async function createProject(name: string, rootDir: string, filePaths: string[], description: string) {
  return invoke<ProjectCreateResult>("create_project", { name, rootDir, filePaths, description });
}

export async function importFiles(projectRoot: string, filePaths: string[], relatedTask: string) {
  return invoke<ImportResult>("import_files", { projectRoot, filePaths, relatedTask });
}

export async function sendProjectMessage(projectRoot: string, text: string) {
  return invoke<MessageResult>("send_project_message", { projectRoot, text });
}

export async function stopProjectMessage(projectRoot: string) {
  return invoke<void>("stop_project_message", { projectRoot });
}

export async function saveProjectDraft(projectRoot: string, text: string, pendingFilePaths: string[] = []) {
  return invoke<WorkspaceDraft>("save_project_draft", { projectRoot, text, pendingFilePaths });
}

export async function finishProjectWork(projectRoot: string, completed: string, nextStep: string) {
  return invoke<FinishWorkResult>("finish_project_work", { projectRoot, completed, nextStep });
}

export async function generateCodexPrompt(projectRoot: string) {
  return invoke<CodexPromptResult>("generate_codex_prompt", { projectRoot });
}

export async function listCodexTasks(projectRoot: string) {
  return invoke<CodexTaskList>("list_codex_tasks", { projectRoot });
}

export async function rebindCodexTaskPrompt(projectRoot: string, taskId: string) {
  return invoke<CodexTask>("rebind_codex_task_prompt", { projectRoot, taskId });
}

export async function markCodexTaskHandedOff(projectRoot: string, taskId: string) {
  return invoke<CodexTask>("mark_codex_task_handed_off", { projectRoot, taskId });
}

export async function startCodexTaskRun(projectRoot: string, taskId: string) {
  return invoke<CodexRun>("start_codex_task_run", { projectRoot, taskId });
}

export async function cancelCodexTaskRun(projectRoot: string, taskId: string) {
  return invoke<CodexRun>("cancel_codex_task_run", { projectRoot, taskId });
}

export async function acceptCodexTask(projectRoot: string, taskId: string, expectedResultId: string) {
  return invoke<CodexTask>("accept_codex_task", { projectRoot, taskId, expectedResultId });
}

export async function rejectCodexTask(projectRoot: string, taskId: string, expectedResultId: string, reason?: string) {
  return invoke<CodexTask>("reject_codex_task", { projectRoot, taskId, expectedResultId, reason: reason || null });
}

export async function importCodexReportText(projectRoot: string, reportText: string) {
  return invoke<CodexReportImportResult>("import_codex_report_text", { projectRoot, reportText });
}

export async function importCodexReportFile(projectRoot: string, reportFilePath: string) {
  return invoke<CodexReportImportResult>("import_codex_report_file", { projectRoot, reportFilePath });
}

export async function scanCodexResultBridge(projectRoot: string) {
  return invoke<CodexResultBridgeScanResult>("scan_codex_result_bridge", { projectRoot });
}

export async function applyCodexReport(projectRoot: string, reportId: string) {
  return invoke<CodexReportApplyResult>("apply_codex_report", { projectRoot, reportId });
}

export async function getWorkLedger(projectRoot: string) {
  return invoke<WorkLedgerSnapshot>("get_work_ledger", { projectRoot });
}

export async function getProjectContextPacket(projectRoot: string) {
  return invoke<ProjectContextPacket>("get_project_context_packet", { projectRoot });
}

export async function refreshGitSnapshot(projectRoot: string, repositoryPath?: string) {
  return invoke<GitSnapshot>("refresh_git_snapshot", { projectRoot, repositoryPath: repositoryPath || null });
}

export async function recordUserDecisionEvent(projectRoot: string, decision: string, reason?: string) {
  return invoke<WorkEvent>("record_user_decision_event", { projectRoot, decision, reason: reason || null });
}

export async function captureProjectFact(projectRoot: string, request: ProjectFactCaptureRequest) {
  return invoke<WorkEvent>("capture_project_fact", { projectRoot, request });
}

export async function searchProjects(query: string) {
  return invoke<GlobalSearchResult[]>("search_projects", { query });
}

export async function registerGeneratedFile(
  projectRoot: string,
  fileName: string,
  content: string,
  createdSource: string,
  relatedTask: string,
  purpose: string,
) {
  return invoke<GeneratedFileResult>("register_generated_file", {
    projectRoot,
    fileName,
    content,
    createdSource,
    relatedTask,
    purpose,
  });
}

export async function openManagedFile(path: string) {
  return invoke<void>("open_file", { path });
}

export async function openManagedFolder(path: string) {
  if (typeof window !== "undefined") {
    const record = {
      timestamp: new Date().toISOString(),
      stage: "frontend_before_open_folder",
      frontendEmittedPath: path,
    };
    const previous = window.localStorage.getItem("ganmaoyuan-open-folder-debug");
    const next = previous ? `${previous}\n${JSON.stringify(record)}` : JSON.stringify(record);
    window.localStorage.setItem("ganmaoyuan-open-folder-debug", next);
  }
  return invoke<void>("open_folder", { path });
}

export async function listMemos() {
  return invoke<MemoItem[]>("list_memos");
}

export async function saveMemos(memos: MemoItem[]) {
  return invoke<MemoItem[]>("save_memos", { memos });
}

export async function listMaterialInbox() {
  return invoke<MaterialInboxItem[]>("list_material_inbox");
}

export async function ingestMaterialInboxFiles(filePaths: string[]) {
  return invoke<MaterialInboxItem[]>("ingest_material_inbox_files", { filePaths });
}

export async function listInboxEntries() {
  return invoke<MaterialInboxItem[]>("list_inbox_entries");
}

export async function receiveInboxFiles(filePaths: string[]) {
  return invoke<MaterialInboxItem[]>("receive_inbox_files", { filePaths });
}

export async function confirmMaterialInboxItem(itemId: string, projectRoot: string) {
  return invoke<MaterialInboxRouteResult>("confirm_material_inbox_item", { itemId, projectRoot });
}

export async function confirmInboxEntry(itemId: string, projectRoot: string) {
  return invoke<MaterialInboxRouteResult>("confirm_inbox_entry", { itemId, projectRoot });
}

export async function refineInboxEntryWithAi(itemId: string) {
  return invoke<MaterialInboxItem>("refine_inbox_entry_with_ai", { itemId });
}

export async function updateInboxRouteDecision(itemId: string, projectRoot: string, semanticLocation: string) {
  return invoke<MaterialInboxItem>("update_inbox_route_decision", { itemId, projectRoot, semanticLocation });
}

export async function ignoreInboxEntry(itemId: string) {
  return invoke<MaterialInboxItem>("ignore_inbox_entry", { itemId });
}

export async function retryInboxEntry(itemId: string) {
  return invoke<MaterialInboxItem>("retry_inbox_entry", { itemId });
}

export async function reanalyzeInboxEntry(itemId: string) {
  return invoke<MaterialInboxItem>("reanalyze_inbox_entry", { itemId });
}

export async function undoInboxRoute(itemId: string) {
  return invoke<MaterialInboxItem>("undo_inbox_route", { itemId });
}

export type GlobalManagedFile = {
  id: string;
  fileName: string;
  sourcePath: string;
  managedPath: string;
  managedRelativePath: string;
  ownershipType: string;
  destination: string;
  category: string;
  generalMaterialDomain: string;
  generalMaterialCategory: string;
  materialSemanticType: string;
  fileType: string;
  contentHash: string;
  documentType: string;
  businessDomain: string;
  businessPurpose: string;
  businessSummary: string;
  contentSummary: string;
  inboxItemId: string;
  operationId: string;
  createdAt: string;
  canUndo: boolean;
  undoneAt: string;
};

export async function routeInboxItemToGlobal(itemId: string, destination: string) {
  return invoke<GlobalManagedFile>("route_inbox_item_to_global", { itemId, destination });
}

export async function undoGlobalFile(fileId: string) {
  return invoke<GlobalManagedFile>("undo_global_file", { fileId });
}

export async function listGlobalFiles() {
  return invoke<GlobalManagedFile[]>("list_global_files");
}

export async function loadInboxRoutingSettings() {
  return invoke<InboxRoutingSettings>("load_inbox_routing_settings");
}

export async function saveInboxRoutingSettings(autoRouteHighConfidence: boolean) {
  return invoke<InboxRoutingSettings>("save_inbox_routing_settings", { autoRouteHighConfidence });
}

export async function createProjectFromMaterialInbox(
  itemId: string,
  projectName: string,
  rootDir: string,
  description: string,
) {
  return invoke<MaterialInboxRouteResult>("create_project_from_material_inbox", {
    itemId,
    projectName,
    rootDir,
    description,
  });
}

export async function createProjectFromInbox(
  itemId: string,
  projectName: string,
  rootDir: string,
  description: string,
) {
  return invoke<MaterialInboxRouteResult>("create_project_from_inbox", {
    itemId,
    projectName,
    rootDir,
    description,
  });
}

export async function consumeLaunchInboxEntries() {
  return invoke<MaterialInboxItem[]>("consume_launch_inbox_entries");
}

export async function loadDeepSeekSettings() {
  return invoke<DeepSeekSettings>("load_deepseek_settings");
}

export async function saveDeepSeekApiKey(apiKey: string, selectedModelId: string) {
  return invoke<DeepSeekSettings>("save_deepseek_api_key", { apiKey, selectedModelId });
}

export async function deleteDeepSeekApiKey() {
  return invoke<DeepSeekSettings>("delete_deepseek_api_key");
}

export async function testDeepSeekConnection(selectedModelId: string) {
  return invoke<DeepSeekConnectionResult>("test_deepseek_connection", { selectedModelId });
}

export async function listDeepSeekModels() {
  return invoke<DeepSeekModelInfo[]>("list_deepseek_models");
}

export async function grantProjectDeepSeekAuthorization(projectRoot: string) {
  return invoke<ProjectManifest>("grant_project_deepseek_authorization", { projectRoot });
}

export async function refreshProjectUnderstanding(projectRoot: string) {
  return invoke<ProjectManifest>("refresh_project_understanding", { projectRoot });
}

export async function scanProjectWorkspace(projectRoot: string) {
  return invoke<ProjectManifest>("scan_project_workspace", { projectRoot });
}

export async function createLocalBackup(projectRoots: string[], destinationDir: string) {
  return invoke<LocalBackupResult>("create_local_backup", { projectRoots, destinationDir });
}

export async function restoreLocalBackup(backupDir: string, restoreProjectsBaseDir: string) {
  return invoke<BackupRestoreResult>("restore_local_backup", {
    backupDir,
    restoreProjectsBaseDir,
  });
}

export async function migrateProjectRoot(projectRoot: string, newRootDir: string) {
  return invoke<ProjectMigrationResult>("migrate_project_root", { projectRoot, newRootDir });
}

export async function exportProjectSafe(projectRoot: string, destinationDir: string) {
  return invoke<SafeExportResult>("export_project_safe", { projectRoot, destinationDir });
}

export async function generatePrivacyArtifacts(projectRoot: string) {
  return invoke<PrivacyArtifactsResult>("generate_privacy_artifacts", { projectRoot });
}

export async function loadWeeklyReviewDashboard(projectRoot?: string | null) {
  return invoke<WeeklyReviewDashboard>("load_weekly_review_dashboard", { projectRoot: projectRoot ?? null });
}

export async function loadWeeklyReviewSettings() {
  return invoke<WeeklyReviewSettings>("load_weekly_review_settings");
}

export async function saveWeeklyReviewSettings(generationWeekday: number) {
  return invoke<WeeklyReviewSettings>("save_weekly_review_settings", { generationWeekday });
}

export async function generateWeeklyReviews(
  projectRoot: string | null,
  weekKey: string,
  weekStart: string,
  weekEnd: string,
  force: boolean,
) {
  return invoke<WeeklyReviewDashboard>("generate_weekly_reviews", {
    projectRoot,
    weekKey,
    weekStart,
    weekEnd,
    force,
  });
}

export async function updateWeeklyReportMarkdown(projectRoot: string, reportId: string, markdown: string) {
  return invoke<WeeklyReportRecord>("update_weekly_report_markdown", { projectRoot, reportId, markdown });
}

export async function confirmWeeklyReport(projectRoot: string, reportId: string) {
  return invoke<WeeklyReportRecord>("confirm_weekly_report", { projectRoot, reportId });
}

export async function exportWeeklyReport(
  projectRoot: string,
  reportId: string,
  destinationDir: string,
) {
  return invoke<WeeklyReportExportResult>("export_weekly_report", {
    projectRoot,
    reportId,
    destinationDir,
  });
}

export async function listenDeepSeekStream(
  handler: (event: DeepSeekStreamEvent) => void,
): Promise<UnlistenFn> {
  return listen<DeepSeekStreamEvent>("deepseek-stream", (event) => handler(event.payload));
}
