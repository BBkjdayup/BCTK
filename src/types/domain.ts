export const QUESTION_TYPES = [
  'single_choice',
  'multiple_choice',
  'fill_blank',
  'true_false',
  'short_answer',
] as const

export type QuestionType = string

export type QuestionTypeBehavior = 'single_choice' | 'multiple_choice' | 'fill_blank' | 'open_response'

export interface QuestionTypeDefinition {
  code: QuestionType
  name: string
  behavior: QuestionTypeBehavior
  aliases: string[]
  defaultOptions: string[]
  isBuiltin: boolean
  isEnabled: boolean
  sortOrder: number
  questionCount: number
  paperItemCount: number
  createdAt: number
  updatedAt: number
}

export interface SaveQuestionTypeRequest {
  code?: QuestionType | null
  name: string
  behavior: QuestionTypeBehavior
  aliases: string[]
  defaultOptions: string[]
  isEnabled: boolean
}

export const QUESTION_TYPE_LABELS: Record<string, string> = {
  single_choice: '单选题',
  multiple_choice: '多选题',
  fill_blank: '填空题',
  true_false: '判断题',
  short_answer: '简答题',
}

export interface RichContent {
  /** 1 = 旧 HTML 内容；2 = Tiptap JSON，同时保留 HTML/纯文本兼容缓存。 */
  schemaVersion: 1 | 2
  editor?: 'tiptap'
  editorVersion?: string
  document?: Record<string, unknown>
  html: string
  plainText: string
  sourceOoxml?: string
}

export interface Subject {
  id: string
  name: string
  sortOrder: number
  questionCount: number
  lastAccessedAt?: number | null
  chapters: Chapter[]
}

export interface Chapter {
  id: string
  subjectId: string
  name: string
  sortOrder: number
  questionCount: number
  lastAccessedAt?: number | null
}

export interface Tag {
  id: string
  name: string
  questionCount: number
  createdAt: number
  updatedAt: number
}

export interface TaxonomyOrderItem {
  id: string
  sortOrder: number
}

export interface QuestionOption {
  id: string
  position: number
  content: RichContent
}

export type QuestionContentSlot = 'stem' | 'option' | 'answer' | 'explanation'

export interface QuestionResourceRef {
  nodeId: string
  resourceId: string
  contentSlot: QuestionContentSlot
  optionId?: string | null
}

export interface Question {
  id: string
  type: QuestionType
  stem: RichContent
  options: QuestionOption[]
  answer: RichContent
  explanation: RichContent
  subjectId: string
  chapterId: string
  subjectName: string
  chapterName: string
  tags: Tag[]
  resourceRefs?: QuestionResourceRef[]
  lastUsedAt?: number | null
  deletedAt?: number | null
  createdAt: number
  updatedAt: number
  contentVersion: number
}

export interface QuestionDraft {
  id?: string
  questionId?: string
  type: QuestionType
  stem: RichContent
  options: QuestionOption[]
  answer: RichContent
  explanation: RichContent
  subjectId: string
  chapterId: string
  tagIds: string[]
  resourceRefs?: QuestionResourceRef[]
  baseContentVersion?: number
}

export interface ImportedQuestionOverwriteItem {
  clientId: string
  draft: QuestionDraft
}

export interface ImportedQuestionOverwriteRequest {
  /** Reuse this UUID when retrying the exact same uncertain batch request. */
  operationId: string
  items: ImportedQuestionOverwriteItem[]
}

export interface ImportedQuestionOverwriteResultItem {
  clientId: string
  questionId: string
  contentVersion: number
}

export interface ImportedQuestionOverwriteResult {
  operationId: string
  updatedCount: number
  items: ImportedQuestionOverwriteResultItem[]
  replayed: boolean
}

export type QuestionDraftKind = 'question_create' | 'question_edit'

export interface QuestionDraftRecord {
  id: string
  draftKey: string
  draftKind: QuestionDraftKind
  targetQuestionId: string | null
  baseContentVersion: number | null
  payloadSchemaVersion: 1
  payload: QuestionDraft
  autosavedAt: number
  createdAt: number
  updatedAt: number
  stale: boolean
}

export interface DocumentQuestionDraftItem {
  id: string
  ordinal: number
  payload: QuestionDraft
}

export interface LegacyDocumentQuestionDraftPayload {
  schemaVersion: 1
  activeItemId: string | null
  items: DocumentQuestionDraftItem[]
}

export type DocumentEntryOptionStyle =
  | 'letter_dot'
  | 'letter_comma'
  | 'letter_parentheses'
  | 'letter_brackets'

export interface DocumentEntryTemplateConfig {
  schemaVersion: 1
  typeMarker: string
  stemMarker: string
  answerMarker: string
  explanationMarker: string
  optionStyle: DocumentEntryOptionStyle
  questionSeparator: string
  compatibleDefaultMarkers: boolean
  recognizeQuestionNumbers?: boolean
  stripRecognizedQuestionNumbers?: boolean
  stemPrompt: string
  optionPrompt: string
  answerPrompt: string
  explanationPrompt: string
}

export interface DocumentEntryTemplate {
  id: string
  name: string
  config: DocumentEntryTemplateConfig
  isBuiltIn: boolean
  isDefault: boolean
  createdAt: number
  updatedAt: number
}

export interface SaveDocumentEntryTemplateRequest {
  id: string | null
  name: string
  config: DocumentEntryTemplateConfig
  setAsDefault: boolean
}

export interface LegacyFreeDocumentQuestionDraftPayload {
  schemaVersion: 2
  templateId: 'labeled_fields_v1'
  source: RichContent
}

export interface FreeDocumentQuestionDraftPayload {
  schemaVersion: 3
  templateId: string
  templateNameSnapshot: string
  templateConfigSnapshot: DocumentEntryTemplateConfig
  source: RichContent
}

export type DocumentQuestionDraftPayload =
  | LegacyDocumentQuestionDraftPayload
  | LegacyFreeDocumentQuestionDraftPayload
  | FreeDocumentQuestionDraftPayload

export interface DocumentQuestionDraftRecord {
  id: string
  payload: DocumentQuestionDraftPayload
  autosavedAt: number
  createdAt: number
  updatedAt: number
}

export type QuestionDuplicateStatus = 'exact' | 'suspected' | 'none'

export interface QuestionDuplicateCandidate {
  id: string
  type: QuestionType
  stemPreview: string
  subjectName: string
  chapterName: string
  similarityPercent: number
  contentVersion: number
  sourceKind?: 'question' | 'import-item'
  sourceOrdinal?: number | null
}

export interface QuestionDuplicateCheck {
  status: QuestionDuplicateStatus
  candidate: QuestionDuplicateCandidate | null
  evaluatedCandidateCount: number
  suspectedThresholdPercent: number
  similarityMethod: 'nfkc_char_bigram_dice_v1'
}

export interface QuestionDuplicateBatchEntry {
  clientId: string
  draft: QuestionDraft
}

export interface QuestionDuplicateBatchRequest {
  items: QuestionDuplicateBatchEntry[]
  mode?: 'full' | 'exact' | 'import'
  scanId?: string
}

export interface QuestionDuplicateBatchItemResult {
  clientId: string
  exactFingerprint: string
  check: QuestionDuplicateCheck
}

export interface QuestionDuplicateBatchResult {
  items: QuestionDuplicateBatchItemResult[]
}

export interface QuestionDuplicateScanRequest {
  subjectId: string
  chapterId?: string | null
}

export interface IgnoreQuestionDuplicateRequest {
  firstQuestionId: string
  firstContentVersion: number
  secondQuestionId: string
  secondContentVersion: number
}

export interface QuestionDuplicateMember {
  id: string
  type: QuestionType
  stemPreview: string
  subjectId: string
  chapterId: string
  subjectName: string
  chapterName: string
  contentVersion: number
  createdAt: number
  updatedAt: number
}

export interface QuestionDuplicateGroup {
  id: string
  duplicateKind: 'exact' | 'suspected'
  similarityPercent: number
  members: QuestionDuplicateMember[]
}

export interface QuestionDuplicateScanResult {
  scannedQuestionCount: number
  comparedQuestionCount: number
  suspectedThresholdPercent: number
  exactGroups: QuestionDuplicateGroup[]
  suspectedGroups: QuestionDuplicateGroup[]
}

export interface WordImportDraftItem {
  id: string
  ordinal: number
  selected: boolean
  payload: QuestionDraft
  status: 'ok' | 'warning' | 'error'
  diagnostic?: string | null
  duplicateKind: QuestionDuplicateStatus
  candidate?: QuestionDuplicateCandidate | null
  action?: 'skip' | 'overwrite' | 'keep' | null
  needsCheck: boolean
  checkError?: string | null
}

export interface WordImportDraftPayload {
  schemaVersion: 1
  sourceFileName: string
  sourceFileSize: number
  parserVersion: string
  importSessionId?: string | null
  sourceItemCount?: number | null
  omittedItemCount?: number
  activeItemId?: string | null
  items: WordImportDraftItem[]
}

export interface WordImportDraftRecord {
  id: string
  payload: WordImportDraftPayload
  autosavedAt: number
  createdAt: number
  updatedAt: number
}

export interface DocxImageOccurrence {
  nodeId: string
  resourceId: string
  paragraphIndex: number
  textCharOffset: number
  originalFilename?: string | null
  mimeType: 'image/png' | 'image/jpeg'
  byteSize: number
  widthPx: number
  heightPx: number
}

export interface DocxFormulaOccurrence {
  nodeId: string
  paragraphIndex: number
  textCharOffset: number
  latex: string
  sourceKind: 'mathtype_mtef5' | 'word_omml'
  productVersion: number
  productSubversion: number
}

export interface DocxTableCell {
  paragraphIndices: number[]
}

export interface DocxTableOccurrence {
  tableIndex: number
  rows: DocxTableCell[][]
}

export interface BeginWordImportResult {
  analysis: DocxAnalysis
  draft: WordImportDraftRecord
  images: DocxImageOccurrence[]
  formulas: DocxFormulaOccurrence[]
  tables: DocxTableOccurrence[]
}

export interface ExcelWorkbookInspection {
  sourcePath: string
  sourceFileName: string
  sourceFileSize: number
  sheetNames: string[]
  defaultSheetName: string
}

export interface ExcelImportRow {
  rowNumber: number
  questionType: string
  subjectName: string
  chapterName: string
  stem: string
  options: string[]
  answer: string
  explanation: string
  tagNames: string[]
}

export interface ExcelImportAnalysis {
  sourcePath: string
  sourceFileName: string
  sourceFileSize: number
  selectedSheetName: string
  sheetNames: string[]
  headerRowNumber: number
  sourceItemCount: number
  omittedItemCount: number
  formulaCellCount: number
  rows: ExcelImportRow[]
  warnings: string[]
}

export interface BeginExcelImportResult {
  analysis: ExcelImportAnalysis
  draft: WordImportDraftRecord
}

export interface ExcelImportTemplateResult {
  outputPath: string
  outputFilename: string
  outputBytes: number
}

export interface ManagedImagePayload {
  resourceId: string
  mimeType: 'image/png' | 'image/jpeg'
  dataBase64: string
  widthPx?: number | null
  heightPx?: number | null
}

export interface StoreManagedImageInput {
  dataBase64: string
  originalFilename?: string | null
}

export interface WpsClipboardImagePayload {
  path: string
  mimeType: 'image/png' | 'image/jpeg'
  dataBase64: string
  widthPx: number
  heightPx: number
}

export type UsageFilter =
  | 'all'
  | 'never'
  | 'unused_this_semester'
  | 'unused_this_month'
  | 'unused_this_week'
  | 'unused_today'
export type QuestionTagMatchMode = 'any' | 'all'
export type RandomDrawUsageFilter = UsageFilter

export interface QuestionFilters {
  keyword: string
  subjectId?: string
  chapterId?: string
  type?: QuestionType
  tagIds: string[]
  /** 多标签筛选规则：any 表示命中任一标签，all 表示同时包含全部标签。 */
  tagMatchMode: QuestionTagMatchMode
  usage: UsageFilter
  deleted: boolean
  page: number
  pageSize: number
}

export interface RandomDrawScope {
  /** 空数组表示全部学科；非空时在所选学科范围内抽题。 */
  subjectIds: string[]
  chapterIds: string[]
  tagIds: string[]
  tagMatchMode: QuestionTagMatchMode
  usage: RandomDrawUsageFilter
  excludedQuestionIds: string[]
}

export interface RandomDrawAnalysis {
  availableTotal: number
  availableByType: Record<QuestionType, number>
}

export interface RandomDrawRequest {
  scope: RandomDrawScope
  countMode: 'total' | 'by_type'
  totalCount: number
  questionTypeCounts: Partial<Record<QuestionType, number>>
}

export interface QuestionBatchTarget {
  id: string
  expectedContentVersion: number
}

export interface QuestionBatchClassification {
  subjectId: string
  chapterId: string
}

export type QuestionBatchTagMode = 'keep' | 'append' | 'replace' | 'remove'
export type QuestionBatchUsageOperation = 'keep' | 'reset_never'

export interface QuestionBatchTagOperation {
  mode: QuestionBatchTagMode
  tagIds: string[]
}

export interface QuestionBatchEditRequest {
  questions: QuestionBatchTarget[]
  classification: QuestionBatchClassification | null
  usageOperation: QuestionBatchUsageOperation
  tagOperation: QuestionBatchTagOperation
}

export interface QuestionBatchVersion {
  id: string
  contentVersion: number
}

export interface QuestionBatchEditResult {
  updatedCount: number
  questions: QuestionBatchVersion[]
}

export interface PageResult<T> {
  items: T[]
  total: number
  page: number
  pageSize: number
}

export interface EffectiveCapabilities {
  canEditSingleQuestion: boolean
  canBatchImport: boolean
  maxQuestionsPerPaper: number | null
  canExportDocuments: boolean
  canPrint: boolean
  canUseCloudSync: boolean
  canUseWebApp: boolean
}

export interface DesktopLicenseStatus {
  state: 'active' | 'grace' | 'basic' | 'invalid' | string
  plan: string
  licenseId: string | null
  customerName: string | null
  issuedAtMs: number | null
  expiresAtMs: number | null
  graceEndsAtMs: number | null
  message: string
}

export interface CloudSubscriptionStatus {
  state: 'notConfigured' | 'active' | 'expired' | string
  syncEnabled: boolean
  webAppEnabled: boolean
  expiresAtMs: number | null
  message: string
}

export interface LicenseOverview {
  deviceId: string
  desktop: DesktopLicenseStatus
  cloud: CloudSubscriptionStatus
  capabilities: EffectiveCapabilities
}

export interface CloudUser {
  id: string
  username: string
  email: string | null
  created_at: string
}

export interface CloudEntitlement {
  plan_code: string
  status: string
  expires_at: string | null
}

export interface CloudAccountStatus {
  configured: boolean
  apiBaseUrl: string
  loggedIn: boolean
  user: CloudUser | null
  entitlement: CloudEntitlement | null
  canSync: boolean
  databaseBound: boolean
  lastSyncAtMs: number | null
  conflictCount: number
  message: string
}

export interface CloudLoginRequest {
  account: string
  password: string
}

export interface CloudRegisterRequest {
  username: string
  email: string | null
  password: string
}

export interface CloudSyncResult {
  pulledCount: number
  uploadedCount: number
  mergedCount: number
  conflictCount: number
  skippedCount: number
  completedAtMs: number
  message: string
}

export interface CloudSyncPreflight {
  localEntityCount: number
  localQuestionCount: number
  cloudEntityCount: number
  cloudQuestionCount: number
  localHasData: boolean
  cloudHasData: boolean
  bothNonEmpty: boolean
  recommendedMode: 'merge'
}

export interface CloudSyncConflict {
  id: string
  entityKind: string
  entityId: string
  detectedAtMs: number
  message: string
}

export interface ResolveCloudSyncConflictRequest {
  id: string
  resolution: 'keep_local' | 'use_cloud'
}

export interface LicenseFileResult {
  path: string
  filename: string
  bytes: number
}

export interface BootstrapData {
  initialized: boolean
  dataRoot: string
  subjects: Subject[]
  tags: Tag[]
  questionTypes: QuestionTypeDefinition[]
  pendingDraftCount: number
  databaseHealthy: boolean
  databaseError?: string | null
  appVersion: string
  license: LicenseOverview
}

export interface DocxDiagnostic {
  severity: 'info' | 'warning' | 'error'
  code: string
  partName?: string | null
  message: string
  suggestedAction?: string | null
}

export type DocxPackageKind = 'document' | 'template' | 'unknown'

export interface TemplateByteSpan {
  start: number
  end: number
}

export interface TemplateAnchor {
  name: string
  kind: 'content_control' | 'paragraph_marker'
  partName: string
  replacementSpan: TemplateByteSpan
  containerSpan: TemplateByteSpan
  paragraphIndex: number | null
}

export type TemplateAnalysisStatus = 'ready' | 'needs_configuration' | 'warning' | 'failed'

export interface WordTemplate {
  id: string
  name: string
  fileName: string
  fileSha256Hex: string
  fileByteSize: number
  analysisStatus: TemplateAnalysisStatus
  analysisSchemaVersion: number
  parserVersion: string
  rowVersion: number
  createdAt: number
  updatedAt: number
  lastVerifiedAt: number | null
  isDefault: boolean
  fileAvailable: boolean
  regionConfigured: boolean
  packageKind: DocxPackageKind
  anchors: TemplateAnchor[]
  diagnostics: DocxDiagnostic[]
}

export interface TemplateImportResult {
  imported: boolean
  template: WordTemplate | null
  diagnostics: DocxDiagnostic[]
}

export type TemplateRegionPlacementMode =
  | 'replace_range'
  | 'replace_paragraph'
  | 'after_paragraph'
  | 'document_end'

export interface TemplateRegionPlacement {
  mode: TemplateRegionPlacementMode
  paragraphIndex?: number
  endParagraphIndex?: number
}

export interface TemplateStyleSamples {
  sectionHeadingParagraphIndex?: number
  questionParagraphIndex?: number
  optionParagraphIndex?: number
  answerParagraphIndex?: number
  explanationParagraphIndex?: number
}

export interface ConfigureTemplateRegionsRequest {
  templateId: string
  baseRowVersion: number
  questions: TemplateRegionPlacement
  styleSamples?: TemplateStyleSamples
  title?: TemplateRegionPlacement | null
}

export interface TemplateConfigurationParagraph {
  index: number
  text: string
}

export interface TemplateConfigurationPreview {
  templateId: string
  templateName: string
  rowVersion: number
  paragraphs: TemplateConfigurationParagraph[]
  hasQuestionsAnchor: boolean
  hasTitleAnchor: boolean
}

export interface TemplatePageSetup {
  widthTwips: number
  heightTwips: number
  marginTopTwips: number
  marginRightTwips: number
  marginBottomTwips: number
  marginLeftTwips: number
  headerTwips: number
  footerTwips: number
  columnCount: number
  columnGapTwips: number
  columnSeparator: boolean
  documentGridType?: string | null
  documentGridLinePitchTwips?: number | null
}

export interface TemplateFontTheme {
  majorLatin: string | null
  majorEastAsia: string | null
  minorLatin: string | null
  minorEastAsia: string | null
}

export interface TemplateStylePrototype {
  sourceParagraphIndex: number
  paragraphProperties: string
  runProperties: string
}

export interface TemplateStyleProfile {
  schemaVersion: number
  sourceRange: {
    startParagraphIndex: number
    endParagraphIndex: number
  } | null
  roles: Record<string, TemplateStylePrototype>
}

export interface TemplateSideSealRun {
  text: string
  underline: boolean
  sizeHalfPoints: number | null
}

export interface TemplateSideSealLine {
  alignment: string | null
  runs: TemplateSideSealRun[]
}

export interface TemplateSideSealLayout {
  horizontalRelativeFrom: string | null
  verticalRelativeFrom: string | null
  horizontalOffsetEmu: number | null
  verticalOffsetEmu: number | null
  pageXEmu: number | null
  pageYEmu: number | null
  widthEmu: number
  heightEmu: number
  textDirection: string | null
  lineXEmu: number | null
  lineWidthEmu: number | null
  lineDash: string | null
  lines: TemplateSideSealLine[]
}

export interface TemplateLayoutBlock {
  kind: 'static' | 'title' | 'questions' | 'sideSeal'
  text: string
  style: TemplateStylePrototype | null
  sideSeal?: TemplateSideSealLayout | null
}

export interface TemplateLayoutPreview {
  templateId: string
  templateName: string
  rowVersion: number
  page: TemplatePageSetup
  blocks: TemplateLayoutBlock[]
  fontTheme: TemplateFontTheme
  pageNumberFormat: string | null
  styleProfile: TemplateStyleProfile | null
}

export interface BackupRecord {
  id: string
  backupKind: 'manual' | 'automatic' | 'pre_restore' | 'pre_migration' | 'data_move'
  status: 'creating' | 'ready' | 'failed' | 'missing' | 'invalid'
  displayFilename: string
  formatVersion: number
  databaseSchemaVersion: number
  sourceDatabaseUuid: string
  sourceAppVersion: string
  archiveSha256Hex: string | null
  manifestSha256Hex: string | null
  archiveByteSize: number | null
  payloadFileCount: number
  payloadByteSize: number
  createdAt: number
  completedAt: number | null
  /** null means the backup was saved outside the software-managed backup folder. */
  fileAvailable: boolean | null
  errorCode: string | null
  errorMessage: string | null
}

export interface BackupInspection {
  formatVersion: number
  createdAt: number
  sourceAppVersion: string
  databaseSchemaVersion: number
  sourceDatabaseUuid: string
  archiveSha256Hex: string
  manifestSha256Hex: string
  archiveByteSize: number
  payloadByteSize: number
  payloadFileCount: number
}

export interface BackupCreateResult {
  outputPath: string
  record: BackupRecord
}

export interface AutomaticBackupRun {
  outcome: 'disabled' | 'not_due' | 'created'
  message: string
  record: BackupRecord | null
  prunedCount: number
  lastBackupAt: number | null
  nextDueAt: number | null
  warnings: string[]
}

export interface RestoreDataSummary {
  databaseUuid: string
  databaseSchemaVersion: number
  questionCount: number
  paperCount: number
  templateCount: number
  resourceCount: number
}

export interface RestorePlan {
  restoreToken: string
  expiresAt: number
  sourceDisplayFilename: string
  archiveInspection: BackupInspection
  currentSummary: RestoreDataSummary | null
  incomingSummary: RestoreDataSummary
  currentDatabaseHealthy: boolean
  warnings: string[]
  automaticPreRestoreBackupRequired: boolean
  requiresRestart: boolean
}

export interface ScheduleRestoreRequest {
  restoreToken: string
  expectedArchiveSha256Hex: string
  confirmedCurrentDataReplacement: boolean
}

export interface RestoreScheduled {
  operationId: string
  preRestoreBackupFilename: string | null
  requiresRestart: boolean
}

export type RestoreOutcome = 'success' | 'rolled_back'

export interface RestoreResult {
  operationId: string
  outcome: RestoreOutcome
  restoredDatabaseUuid: string | null
  preRestoreBackupFilename: string | null
  summary: RestoreDataSummary | null
  durationMs: number
  warnings: string[]
  errorMessage: string | null
}

export interface DataMovePlan {
  moveToken: string
  expiresAt: number
  sourceDataRoot: string
  targetDataRoot: string
  estimatedFileCount: number
  estimatedByteSize: number
  warnings: string[]
  requiresRestart: boolean
}

export interface ScheduleDataMoveRequest {
  moveToken: string
  expectedSourceDataRoot: string
  expectedTargetDataRoot: string
  confirmedKeepOldData: boolean
}

export interface DataMoveScheduled {
  operationId: string
  sourceDataRoot: string
  targetDataRoot: string
  copiedFileCount: number
  copiedByteSize: number
  requiresRestart: boolean
}

export interface DataMoveResult {
  operationId: string
  outcome: 'success' | 'rolled_back'
  activeDataRoot: string
  retainedDataRoot: string
  copiedFileCount: number
  copiedByteSize: number
  durationMs: number
  warnings: string[]
  errorMessage: string | null
}

export interface DocxParagraph {
  index: number
  text: string
  formulaCount: number
}

export interface DocxAnalysis {
  sourcePath: string
  archiveBytes: number
  packageKind: DocxPackageKind
  isValid: boolean
  visibleText: string
  paragraphCount: number
  formulaCount: number
  partCount: number
  totalUncompressedBytes: number
  paragraphs: DocxParagraph[]
  diagnostics: DocxDiagnostic[]
}

/**
 * Requests a paper export from a managed Word template.
 *
 * The backend reads the content mode from the saved paper record. Keeping it
 * out of this request prevents the UI and the database from describing two
 * different exports.
 */
export interface PaperDocxExportRequest {
  paperId: string
  expectedPaperRowVersion: number
  templateId: string
  outputPath: string
}

export interface PaperDocxExportResult {
  exported: boolean
  exportRunId: string | null
  paperId: string
  paperRowVersion: number
  templateId: string
  templateName: string
  contentMode: ExportContentMode
  outputPath: string
  outputFilename: string
  outputBytes: number | null
  rewrittenParts: string[]
  diagnostics: DocxDiagnostic[]
}

export type QuestionBankExportFormat = 'docx' | 'xlsx'

export interface QuestionBankExportResult {
  format: QuestionBankExportFormat
  outputPath: string
  outputFilename: string
  outputBytes: number
  questionCount: number
}

export interface PaperItem {
  id: string
  sourceQuestionId?: string | null
  position: number
  snapshot: Question
}

export interface AutomaticGenerationConfig {
  /** null 表示自动组卷时选择“全部学科”。 */
  subjectId: string | null
  /** 空数组表示所选学科下的全部章节。 */
  chapterIds: string[]
  questionTypeCounts: Record<QuestionType, number>
}

export type ExportContentMode =
  | 'paper_only'
  | 'answers_only'
  | 'paper_and_answers'
  | 'paper_answers_explanations'

export interface CanvasEditorData {
  header?: unknown[]
  main: unknown[]
  footer?: unknown[]
  graffiti?: unknown[]
}

export interface CanvasEditorOptions {
  [key: string]: unknown
}

/** 用户在排版页中明确选择的最终纸张尺寸；省略时继续沿用 Word 模板。 */
export interface PaperPageSetup {
  widthMm: number
  heightMm: number
}

/** canvas-editor 的原生排版快照。再次进入排版页时，题目发生变化会自动重新生成。 */
export interface PaperLayout {
  schemaVersion: 1
  editor: 'canvas-editor'
  editorVersion: string
  sourceSignature: string
  data: CanvasEditorData
  options: CanvasEditorOptions
  pageSetup?: PaperPageSetup | null
}

export interface Paper {
  id: string
  title: string
  compositionMode: 'manual' | 'automatic'
  /** 仅自动组卷使用；旧试卷可能没有保存条件，因此允许为 null。 */
  generationConfig: AutomaticGenerationConfig | null
  status: 'draft' | 'saved'
  items: PaperItem[]
  exportContentMode: ExportContentMode
  subjectSummaryText: string
  preferredTemplateId?: string | null
  layout?: PaperLayout | null
  layoutUpdatedAt?: number | null
  rowVersion: number
  createdAt: number
  updatedAt: number
  savedAt?: number | null
  lastSavedAt?: number | null
}

export interface PaperSummary {
  id: string
  title: string
  compositionMode: 'manual' | 'automatic'
  status: 'draft' | 'saved'
  subjectSummaryText: string
  questionCount: number
  rowVersion: number
  createdAt: number
  updatedAt: number
  savedAt?: number | null
  lastSavedAt?: number | null
}

export interface PaperDeleteRequest {
  id: string
  baseRowVersion: number
}

export interface PaperFilters {
  keyword: string
  status: 'all' | 'draft' | 'saved'
  page: number
  pageSize: number
}

export interface AppSettings {
  defaultExportDirectory: string
  defaultTemplateId?: string | null
  exportFilenamePattern: string
  optionalConfirmations: boolean
  /** 兼容旧版设置格式；当前“本学期未使用”固定按 180 天判断。 */
  recentUnusedDays: number
  recentAddedDays: number
  recentUsedDays: number
  recycleRetentionDays: number
  recyclePolicy: 'manual_only' | 'remind_only'
  automaticBackupEnabled: boolean
  automaticBackupIntervalDays: number
  automaticBackupRetentionCount: number
}
