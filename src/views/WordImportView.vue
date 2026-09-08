<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { onBeforeRouteLeave, useRoute } from 'vue-router'
import { type UnlistenFn } from '@tauri-apps/api/event'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { ElMessage, ElMessageBox } from 'element-plus'
import { UploadFilled, Document, CircleCheck, Warning, Delete } from '@element-plus/icons-vue'
import AppContextMenu from '../components/AppContextMenu.vue'
import RichContentView from '../components/RichContentView.vue'
import RichTextEditor from '../components/RichTextEditor.vue'
import QuestionStemSummary from '../components/QuestionStemSummary.vue'
import { backend, isDesktopRuntime } from '../services/backend'
import { errorMessage } from '../services/errors'
import { useAppStore } from '../stores/app'
import { useQuestionBankStore } from '../stores/questionBank'
import {
  type BeginExcelImportResult,
  type BeginWordImportResult,
  type DocxAnalysis,
  type DocxFormulaOccurrence,
  type DocxImageOccurrence,
  type DocxTableOccurrence,
  type ExcelImportAnalysis,
  type Question,
  type QuestionDraft,
  type QuestionDuplicateCandidate,
  type QuestionDuplicateBatchItemResult,
  type QuestionDuplicateBatchResult,
  type QuestionDuplicateCheck,
  type QuestionResourceRef,
  type QuestionType,
  type QuestionTypeBehavior,
  type RichContent,
  type WordImportDraftItem,
  type WordImportDraftPayload,
  type WordImportDraftRecord,
} from '../types/domain'
import type { ContextMenuItem } from '../types/contextMenu'
import { clonePlain } from '../utils/clonePlain'
import { chunked, withTimeout } from '../utils/boundedBatch'
import {
  applyWordImportBatchClassification,
  canOpenWordImportDuplicateDialog,
  pendingWordImportItems,
  planWordImportOverwrites,
  setWordImportBatchSelection,
  type WordImportDuplicateScanPhase,
  wordImportQuestionSemanticSignature,
  wordImportDuplicateActionTargets,
  wordImportBatchSelectionState,
  wordImportDuplicateScanPercentage,
} from '../utils/wordImportBatch'
import {
  recognizeWordQuestions,
} from '../utils/wordImportRecognition'
import { buildWordAnalysisLines } from '../utils/wordImportAnalysis'
import { buildWordImportRichContent } from '../utils/wordImportRichContent'
import {
  DOCUMENT_ENTRY_PARSER_VERSION,
  DOCUMENT_ENTRY_SOURCE_FILE_NAME,
} from '../utils/documentTemplateRecognition'
import {
  canPersistWordImportReview,
  expectedWordImportReviewParserVersion,
  isDocumentEntryReviewDraft,
} from '../utils/wordImportReviewAvailability'
import { resolveTaxonomyDefault } from '../utils/taxonomyDefaults'
import { activeQuestionResourceRefs } from '../utils/questionResourceRefs'
import { normalizeExcelImportRow } from '../utils/excelImport'
import {
  enabledQuestionTypes,
  isChoiceQuestionType,
  matchesChoiceBehavior,
  questionTypeLabel,
} from '../utils/questionTypes'

type ImportStatus = 'ok' | 'warning' | 'error'
type DuplicateAction = 'skip' | 'overwrite' | 'keep'
type ImportExecutionPhase =
  | 'idle'
  | 'checking'
  | 'resolving'
  | 'overwriting'
  | 'creating'
  | 'refreshing'
  | 'preserving'
  | 'stopping'

class ImportWriteUncertainError extends Error {
  constructor(message: string) {
    super(message)
    this.name = 'ImportWriteUncertainError'
  }
}

interface ImportItem {
  id: string
  ordinal: number
  selected: boolean
  type: QuestionType
  stem: RichContent
  options: RichContent[]
  optionIds: string[]
  answer: RichContent
  explanation: RichContent
  subjectId: string
  chapterId: string
  tagIds: string[]
  resourceRefs: QuestionResourceRef[]
  status: ImportStatus
  diagnostic?: string
  duplicateKind: 'none' | 'exact' | 'suspected'
  duplicateCandidate: QuestionDuplicateCandidate | null
  duplicateAction?: DuplicateAction
  duplicateChecking: boolean
  duplicateNeedsCheck: boolean
  duplicateCheckError?: string
  detectedUnknownTypeName?: string | null
  suggestedBehavior?: QuestionTypeBehavior | null
  pendingTagNames?: string[]
  sourceRowNumber?: number
}

interface UnknownTypeDecision {
  name: string
  count: number
  action: 'create' | 'map'
  behavior: QuestionTypeBehavior
  defaultOptionsText: string
  mapTo: QuestionType
}

interface SelectedImportFile {
  name: string
  size: number
  path?: string
  kind: 'docx' | 'xlsx'
}

interface UnknownTagDecision {
  name: string
  count: number
  create: boolean
}

const appStore = useAppStore()
const bankStore = useQuestionBankStore()
const route = useRoute()
const step = ref(0)
const selectedFile = ref<SelectedImportFile | null>(null)
const excelSheetNames = ref<string[]>([])
const selectedExcelSheet = ref('')
const excelInspectionRunning = ref(false)
const templateCreating = ref(false)
const dragActive = ref(false)
const progress = ref(0)
const parseLogs = ref<string[]>([])
const analysis = ref<DocxAnalysis | null>(null)
const recognitionError = ref('')
const items = ref<ImportItem[]>([])
const activeItemId = ref<string | null>(null)
const saving = ref(false)
const importExecutionPhase = ref<ImportExecutionPhase>('idle')
const importExecutionCompleted = ref(0)
const importExecutionTotal = ref(0)
const importExecutionNotice = ref('')
const importCancelRequested = ref(false)
const duplicateScanRunning = ref(false)
const duplicateScanCompleted = ref(0)
const duplicateScanTotal = ref(0)
const duplicateScanPhase = ref<WordImportDuplicateScanPhase>('idle')
const duplicateScanIncludesImportBatchPass = ref(false)
const duplicateScanNotice = ref('')
const duplicateDialogOpen = ref(false)
const duplicateDialogItemId = ref<string | null>(null)
const duplicateExistingQuestion = ref<Question | null>(null)
const applyActionToSameKind = ref(true)
const result = ref({ success: 0, failed: 0, skipped: 0 })
const sessionTotalCount = ref(0)
const batchSubjectId = ref('')
const batchChapterId = ref('')
const contextMenuOpen = ref(false)
const contextMenuX = ref(0)
const contextMenuY = ref(0)
const contextMenuTitle = ref('')
const contextMenuItems = ref<ContextMenuItem[]>([])
const contextItemId = ref<string | null>(null)
const draftDirty = ref(false)
const draftAutosaving = ref(false)
const lastDraftAutosavedAt = ref<number | null>(null)
const hasPersistedDraft = ref(false)
const draftReady = ref(false)
const recoveryChecking = ref(true)
const draftReadBlocked = ref(false)
const draftReadError = ref('')
const importLimitNotice = ref('')
const sourceRecognizedItemCount = ref(0)
const omittedItemCount = ref(0)
const importSessionId = ref<string | null>(null)
const pendingImageOccurrences = ref<DocxImageOccurrence[]>([])
const pendingFormulaOccurrences = ref<DocxFormulaOccurrence[]>([])
const pendingTableOccurrences = ref<DocxTableOccurrence[]>([])
const CURRENT_WORD_IMPORT_PARSER_VERSION = 'w1-ooxml-images-formulas-tables-7'
const CURRENT_EXCEL_IMPORT_PARSER_VERSION = 'xlsx-text-v2'
const parserVersion = ref(CURRENT_WORD_IMPORT_PARSER_VERSION)
const desktopAvailable = isDesktopRuntime()
const isDocumentEntryReview = computed(() => route.query.source === 'document-entry')
const reviewPersistenceAvailable = computed(() => (
  canPersistWordImportReview(desktopAvailable, isDocumentEntryReview.value)
))
let timer: ReturnType<typeof setInterval> | undefined
let prepareReviewTimer: ReturnType<typeof setTimeout> | undefined
let autosaveTimer: ReturnType<typeof setInterval> | undefined
let unlistenCloseRequested: UnlistenFn | undefined
let initialDuplicateScan: Promise<boolean> | null = null
let reviewItemCheckQueue: Promise<void> = Promise.resolve()
let resolveDuplicateAction: ((action: DuplicateAction | null) => void) | undefined
let activeDuplicateScans = 0
let duplicateDialogRequest = 0
let lastSavedReviewSnapshot: string | null = null
let recognitionRequest = 0
let draftRecoveryRequest = 0
let duplicateScanGeneration = 0
const activeDuplicateScanIds = new Set<string>()
const overwriteOperationIds = new Map<string, string>()
let componentActive = true
let closePromptOpen = false
const failedImportItemIds = new Set<string>()
const unknownTypeDialogOpen = ref(false)
const resolvingUnknownTypes = ref(false)
const unknownTypeDecisions = ref<UnknownTypeDecision[]>([])
const unknownTagDialogOpen = ref(false)
const creatingUnknownTags = ref(false)
const unknownTagDecisions = ref<UnknownTagDecision[]>([])

const DUPLICATE_BATCH_SIZE = 100
const DUPLICATE_BATCH_TIMEOUT_MS = 30_000
const IMPORT_SAVE_BATCH_SIZE = 100
const IMPORT_WRITE_TIMEOUT_MS = 60_000
const IMPORT_REFRESH_TIMEOUT_MS = 15_000
const IMPORT_DRAFT_TIMEOUT_MS = 15_000
const MAX_WORD_IMPORT_ITEMS = 5_000
const duplicateActionLabels: Record<DuplicateAction, string> = {
  skip: '跳过此题',
  overwrite: '覆盖原题',
  keep: '继续保留',
}
const duplicateActionRules = new Map<'exact' | 'suspected', DuplicateAction>()

function currentDuplicateAction(item: ImportItem): DuplicateAction | undefined {
  return item.duplicateAction
}

function applyRememberedDuplicateAction(item: ImportItem) {
  if (item.duplicateKind === 'none' || !item.duplicateCandidate || item.duplicateAction) return
  const action = duplicateActionRules.get(item.duplicateKind)
  if (!action || (action === 'overwrite' && item.duplicateCandidate.sourceKind === 'import-item')) return
  item.duplicateAction = action
}

const isTauriRuntime = () => desktopAvailable

function explainDesktopRequirement() {
  ElMessage.info('浏览器只能演示页面，不能读取或导入电脑中的 Word、Excel 文件。请在 Windows 桌面版中执行。')
}
const escapeHtml = (text: string) => text.replace(/[&<>"']/g, (character) => ({
  '&': '&amp;',
  '<': '&lt;',
  '>': '&gt;',
  '"': '&quot;',
  "'": '&#39;',
})[character] ?? character)
const rich = (text: string): RichContent => ({
  schemaVersion: 1,
  html: text.split(/\r?\n/).map((line) => `<p>${escapeHtml(line) || '<br>'}</p>`).join(''),
  plainText: text,
})
const emptyDuplicateState = () => ({
  duplicateKind: 'none' as const,
  duplicateCandidate: null,
  duplicateChecking: false,
  duplicateNeedsCheck: true,
})
const activeItem = computed(() => items.value.find((item) => item.id === activeItemId.value) ?? null)
const duplicateDialogItem = computed(() => (
  items.value.find((item) => item.id === duplicateDialogItemId.value) ?? null
))
const duplicateCandidateIsImportItem = computed(() => (
  duplicateDialogItem.value?.duplicateCandidate?.sourceKind === 'import-item'
))
const batchSelectionState = computed(() => wordImportBatchSelectionState(items.value))
const selectedCount = computed(() => batchSelectionState.value.selectedCount)
const allItemsSelected = computed({
  get: () => batchSelectionState.value.allSelected,
  set: (selected: boolean) => setWordImportBatchSelection(items.value, selected),
})
const selectionIndeterminate = computed(() => batchSelectionState.value.indeterminate)
const sessionProcessedCount = computed(() => Math.max(0, sessionTotalCount.value - items.value.length))
const warningCount = computed(() => items.value.filter((item) => item.status !== 'ok').length)
const duplicateRetryCount = computed(() => items.value.filter((item) => (
  !itemValidationMessage(item) && (item.duplicateNeedsCheck || Boolean(item.duplicateCheckError))
)).length)
const duplicateScanPercentage = computed(() => wordImportDuplicateScanPercentage({
  phase: duplicateScanPhase.value,
  completed: duplicateScanCompleted.value,
  total: duplicateScanTotal.value,
  includesImportBatchPass: duplicateScanIncludesImportBatchPass.value,
}))
const duplicateScanProgressLabel = computed(() => {
  if (duplicateScanPhase.value === 'database') {
    return `正在与题库查重 ${duplicateScanCompleted.value}/${duplicateScanTotal.value}`
  }
  if (duplicateScanPhase.value === 'import-batch') {
    return `正在检查本批次内部重复 ${duplicateScanCompleted.value}/${duplicateScanTotal.value}`
  }
  return '正在准备重复题检查'
})
const importExecutionPercentage = computed(() => {
  if (importExecutionPhase.value === 'checking') return duplicateScanPercentage.value
  if (!importExecutionTotal.value) return 0
  return Math.min(100, Math.round(
    (importExecutionCompleted.value / importExecutionTotal.value) * 100,
  ))
})
const importExecutionLabel = computed(() => {
  if (importExecutionPhase.value === 'checking') {
    return duplicateScanRunning.value
      ? duplicateScanProgressLabel.value
      : '正在确认重复题检查结果'
  }
  if (importExecutionPhase.value === 'resolving') return '正在整理本批处理方案'
  if (importExecutionPhase.value === 'overwriting') {
    return `正在批量覆盖原题 ${importExecutionCompleted.value}/${importExecutionTotal.value}`
  }
  if (importExecutionPhase.value === 'creating') {
    return `正在批量写入新题 ${importExecutionCompleted.value}/${importExecutionTotal.value}`
  }
  if (importExecutionPhase.value === 'refreshing') return '题目已写入，正在刷新题库统计'
  if (importExecutionPhase.value === 'preserving') return '正在保存尚未处理的审查草稿'
  if (importExecutionPhase.value === 'stopping') return '正在完成当前批次后停止'
  return ''
})
const importCanCancel = computed(() => saving.value && [
  'checking',
  'resolving',
  'overwriting',
  'creating',
].includes(importExecutionPhase.value))
const typeOptions = computed(() => enabledQuestionTypes(appStore.questionTypes))
const chapterOptions = computed(() => {
  if (!activeItem.value) return []
  return appStore.subjects.find((subject) => subject.id === activeItem.value?.subjectId)?.chapters ?? []
})
const batchChapterOptions = computed(() => (
  appStore.subjects.find((subject) => subject.id === batchSubjectId.value)?.chapters ?? []
))

function selectNone() {
  setWordImportBatchSelection(items.value, false)
}

function initializeBatchClassification(item: ImportItem | undefined = items.value[0]) {
  const subject = appStore.subjects.find((entry) => entry.id === item?.subjectId) ?? appStore.subjects[0]
  batchSubjectId.value = subject?.id ?? ''
  batchChapterId.value = subject?.chapters.some((chapter) => chapter.id === item?.chapterId)
    ? item?.chapterId ?? ''
    : subject?.chapters[0]?.id ?? ''
}

function onBatchSubjectChanged() {
  batchChapterId.value = batchChapterOptions.value[0]?.id ?? ''
}

function applyBatchClassification() {
  if (!selectedCount.value) {
    ElMessage.warning('请先勾选需要批量分类的题目。')
    return
  }
  const subject = appStore.subjects.find((entry) => entry.id === batchSubjectId.value)
  const chapter = subject?.chapters.find((entry) => entry.id === batchChapterId.value)
  if (!subject || !chapter) {
    ElMessage.warning('请选择完整且有效的学科和章节。')
    return
  }

  const updated = applyWordImportBatchClassification(items.value, subject.id, chapter.id)
  for (const item of updated) refreshItemStatus(item)
  ElMessage.success(`已将 ${updated.length} 道题设置为“${subject.name} / ${chapter.name}”，可直接导入本批。`)
}

function openItemContextMenu(event: MouseEvent, item: ImportItem) {
  event.preventDefault()
  activateReviewItem(item)
  contextItemId.value = item.id
  contextMenuX.value = event.clientX
  contextMenuY.value = event.clientY
  contextMenuTitle.value = `第 ${item.ordinal} 题 · ${questionTypeLabel(item.type, appStore.questionTypes)}`
  contextMenuItems.value = [
    { id: 'select-only', label: '仅勾选这道题' },
    { id: 'select-same-type', label: '勾选全部同题型题目' },
    { id: 'select-from-here', label: '勾选从此题到末尾' },
    {
      id: 'apply-classification',
      label: `应用当前批量分类到已选 ${selectedCount.value} 题`,
      disabled: selectedCount.value === 0 || !batchSubjectId.value || !batchChapterId.value,
      dividerBefore: true,
    },
    { id: 'copy-stem', label: '复制题干' },
    { id: 'skip', label: '明确跳过并移出', danger: true, dividerBefore: true },
  ]
  contextMenuOpen.value = true
}

async function handleItemContextAction(action: ContextMenuItem) {
  const item = items.value.find((entry) => entry.id === contextItemId.value)
  if (!item) return

  if (action.id === 'select-only') {
    setWordImportBatchSelection(items.value, false)
    item.selected = true
    return
  }
  if (action.id === 'select-same-type') {
    for (const entry of items.value) entry.selected = entry.type === item.type
    return
  }
  if (action.id === 'select-from-here') {
    const startIndex = items.value.findIndex((entry) => entry.id === item.id)
    for (const [index, entry] of items.value.entries()) entry.selected = index >= startIndex
    return
  }
  if (action.id === 'apply-classification') {
    applyBatchClassification()
    return
  }
  if (action.id === 'copy-stem') {
    try {
      await navigator.clipboard.writeText(item.stem.plainText)
      ElMessage.success('题干已复制')
    } catch {
      ElMessage.error('复制失败，请在题干编辑框中手动复制。')
    }
    return
  }
  if (action.id === 'skip') await skipItem(item)
}

function validateFile(name: string, size: number, kind: 'docx' | 'xlsx') {
  const extension = name.split('.').pop()?.toLowerCase()
  if (extension === 'doc') {
    ElMessage.warning('旧版 .doc 暂不支持，请先用 Word 或 WPS 另存为 .docx')
    return false
  }
  if (extension === 'xls' || extension === 'xlsm') {
    ElMessage.warning('旧版 .xls 和带宏的 .xlsm 暂不支持，请先另存为 .xlsx')
    return false
  }
  if (extension !== kind) {
    ElMessage.error(`请选择 .${kind} 文件`)
    return false
  }
  if (size > 100 * 1024 * 1024) {
    ElMessage.error('文件超过 100 MB 安全限制，请拆分后再导入')
    return false
  }
  return true
}

function onDrop(event: DragEvent) {
  dragActive.value = false
  if (!desktopAvailable) {
    explainDesktopRequirement()
    return
  }
  if (recoveryChecking.value || draftReadBlocked.value) return
  const file = event.dataTransfer?.files?.[0]
  const extension = file?.name.split('.').pop()?.toLowerCase()
  const kind = extension === 'xlsx' ? 'xlsx' : 'docx'
  if (file && validateFile(file.name, file.size, kind)) {
    selectedFile.value = null
    ElMessage.warning('为确保读取的是真实本地文件，桌面版暂不接受拖放路径。请通过 Windows 文件窗口重新选择。')
  }
}

async function chooseNativeFile(kind: 'docx' | 'xlsx') {
  if (!desktopAvailable) {
    explainDesktopRequirement()
    return
  }
  if (recoveryChecking.value || draftReadBlocked.value) return
  const request = recognitionRequest
  try {
    const path = kind === 'xlsx' ? await backend.pickXlsxFile() : await backend.pickDocxFile()
    if (!path || !componentActive || request !== recognitionRequest || recoveryChecking.value) return
    const name = path.split(/[\\/]/).pop() ?? path
    if (!validateFile(name, 0, kind)) return
    excelSheetNames.value = []
    selectedExcelSheet.value = ''
    if (kind === 'xlsx') {
      excelInspectionRunning.value = true
      const inspected = await backend.inspectExcelWorkbook(path)
      if (!componentActive || request !== recognitionRequest || recoveryChecking.value) return
      excelSheetNames.value = inspected.sheetNames
      selectedExcelSheet.value = inspected.defaultSheetName
      selectedFile.value = { name: inspected.sourceFileName, size: inspected.sourceFileSize, path, kind }
    } else {
      selectedFile.value = { name, size: 0, path, kind }
    }
  } catch (reason) {
    ElMessage.error(errorMessage(reason, `无法读取${kind === 'xlsx' ? ' Excel' : ' Word'}文件`))
  } finally {
    excelInspectionRunning.value = false
  }
}

async function downloadExcelTemplate() {
  if (!desktopAvailable || templateCreating.value) return
  templateCreating.value = true
  try {
    const date = new Date().toISOString().slice(0, 10)
    const outputPath = await backend.pickXlsxSavePath(`TK试题题库_Excel导入模板_${date}.xlsx`)
    if (!outputPath) return
    const saved = await backend.createExcelImportTemplate(outputPath)
    try {
      await ElMessageBox.confirm(
        `模板“${saved.outputFilename}”已保存。是否打开所在文件夹？`,
        'Excel 导入模板已生成',
        { confirmButtonText: '打开所在位置', cancelButtonText: '关闭', type: 'success' },
      )
      await backend.revealExportedFile(saved.outputPath)
    } catch {
      // 用户选择关闭提示，不影响已生成的模板。
    }
  } catch (reason) {
    ElMessage.error(errorMessage(reason, 'Excel 导入模板生成失败'))
  } finally {
    templateCreating.value = false
  }
}

async function startRecognition() {
  if (!desktopAvailable) {
    explainDesktopRequirement()
    return
  }
  if (!selectedFile.value || recoveryChecking.value || draftReadBlocked.value) return
  if (selectedFile.value.path) {
    if (selectedFile.value.kind === 'xlsx') {
      await startExcelRecognition(selectedFile.value.path)
    } else {
      await startDesktopRecognition(selectedFile.value.path)
    }
    return
  }
  selectedFile.value = null
  ElMessage.error('没有取得可信的本地文件路径，已停止识别。请通过 Windows 文件选择窗口重新选择。')
}

async function startDesktopRecognition(path: string) {
  if (!appStore.license.capabilities.canBatchImport) {
    ElMessage.warning('基础桌面模式不支持 Word 批量导入；请使用单题录入或导入桌面专业版授权。')
    return
  }
  recognitionRequest += 1
  const request = recognitionRequest
  clearInterval(timer)
  clearTimeout(prepareReviewTimer)
  step.value = 1
  progress.value = 6
  recognitionError.value = ''
  analysis.value = null
  pendingImageOccurrences.value = []
  pendingFormulaOccurrences.value = []
  pendingTableOccurrences.value = []
  parseLogs.value = ['正在验证 Open XML 文件结构与安全边界…']
  const progressTimer = setInterval(() => {
    if (!componentActive || request !== recognitionRequest || step.value !== 1) {
      clearInterval(progressTimer)
      return
    }
    progress.value = Math.min(88, progress.value + 3)
  }, 350)
  timer = progressTimer
  try {
    const started: BeginWordImportResult = await backend.beginWordImport(path)
    if (!componentActive || request !== recognitionRequest || step.value !== 1) return
    const result = started.analysis
    analysis.value = result
    importSessionId.value = started.draft.payload.importSessionId ?? null
    parserVersion.value = started.draft.payload.parserVersion
    pendingImageOccurrences.value = started.images
    pendingFormulaOccurrences.value = started.formulas
    pendingTableOccurrences.value = started.tables
    if (!hasPersistedDraft.value) updatePendingDraftCount(1)
    hasPersistedDraft.value = true
    lastDraftAutosavedAt.value = started.draft.autosavedAt
    if (selectedFile.value) selectedFile.value.size = result.archiveBytes
    progress.value = 100
    parseLogs.value.push(`已检查 ${result.partCount} 个文档部件和 ${result.paragraphCount} 个段落`)
    if (result.formulaCount) parseLogs.value.push(`识别到 ${result.formulaCount} 个 Word 公式片段`)
    if (started.images.length) parseLogs.value.push(`已安全提取并登记 ${started.images.length} 个图片位置`)
    if (started.formulas.length) {
      const nativeFormulaCount = started.formulas.filter((formula) => formula.sourceKind === 'word_omml').length
      const mathtypeFormulaCount = started.formulas.length - nativeFormulaCount
      parseLogs.value.push(
        `已将 ${mathtypeFormulaCount} 个 MathType 公式和 ${nativeFormulaCount} 个 Word 原生公式转换为可编辑公式`,
      )
    }
    if (started.tables.length) parseLogs.value.push(`已保留 ${started.tables.length} 个可编辑 Word 表格`)
    for (const diagnostic of result.diagnostics.slice(0, 6)) {
      parseLogs.value.push(`${diagnostic.severity === 'error' ? '错误' : '提示'}：${diagnostic.message}`)
    }
    if (!result.isValid) {
      recognitionError.value = result.diagnostics.find((item) => item.severity === 'error')?.message
        ?? '这个文件未通过 DOCX 安全和结构检查，未读取题目内容。'
      return
    }
    parseLogs.value.push('文档读取完成，正在生成可人工审查的题目草稿')
    prepareReviewTimer = setTimeout(() => prepareReview(result, request), 250)
  } catch (reason) {
    if (!componentActive || request !== recognitionRequest || step.value !== 1) return
    recognitionError.value = errorMessage(reason, 'Word 文档分析或图片资源登记失败')
    parseLogs.value.push(`错误：${recognitionError.value}`)
  } finally {
    clearInterval(progressTimer)
    if (timer === progressTimer) timer = undefined
  }
}

function recognizeAnalysis(result: DocxAnalysis): ImportItem[] {
  sourceRecognizedItemCount.value = 0
  omittedItemCount.value = 0
  const lines = buildWordAnalysisLines(
    result,
    pendingImageOccurrences.value,
    pendingFormulaOccurrences.value,
    pendingTableOccurrences.value,
  )
  if (!lines.length) return []

  const questions = recognizeWordQuestions(lines, appStore.questionTypes)
  sourceRecognizedItemCount.value = questions.length
  omittedItemCount.value = Math.max(0, questions.length - MAX_WORD_IMPORT_ITEMS)
  if (questions.length > MAX_WORD_IMPORT_ITEMS) {
    const omitted = omittedItemCount.value
    importLimitNotice.value = `这个文档识别到 ${questions.length} 个题目；为保证草稿可安全恢复，本次只载入前 ${MAX_WORD_IMPORT_ITEMS} 项，另有 ${omitted} 项未载入。请把原文档拆分后再次导入剩余内容。`
  }
  const { subject, chapter } = resolveTaxonomyDefault(
    appStore.subjects,
    route.query.subjectId,
    route.query.chapterId,
  )

  return questions.slice(0, MAX_WORD_IMPORT_ITEMS).map((question, index) => {
    const resourceRefs: QuestionResourceRef[] = []
    const definition = appStore.questionTypes.find((item) => item.code === question.type)
    const recognizedOptions = question.options.length
      ? question.options
      : (definition?.defaultOptions ?? []).map((value) => [{
          paragraphIndex: -1,
          text: value,
          images: [],
        }])
    const optionIds = recognizedOptions.map(() => crypto.randomUUID())
    const stem = buildWordImportRichContent(
      question.stemLines,
      pendingFormulaOccurrences.value,
      resourceRefs,
      'stem',
    )
    const options = recognizedOptions.map((optionLines, optionIndex) => (
      buildWordImportRichContent(
        optionLines,
        pendingFormulaOccurrences.value,
        resourceRefs,
        'option',
        optionIds[optionIndex],
      )
    ))
    const answer = buildWordImportRichContent(
      question.answerLines,
      pendingFormulaOccurrences.value,
      resourceRefs,
      'answer',
    )
    const explanation = buildWordImportRichContent(
      question.explanationLines,
      pendingFormulaOccurrences.value,
      resourceRefs,
      'explanation',
    )
    const stemText = stem.plainText
    const answerText = answer.plainText

    const hasRequiredContent = Boolean(stemText && answerText && subject && chapter)
    return {
      id: crypto.randomUUID(),
      ordinal: index + 1,
      selected: true,
      type: question.type,
      stem,
      options,
      optionIds,
      answer,
      explanation,
      subjectId: subject?.id ?? '',
      chapterId: chapter?.id ?? '',
      tagIds: [],
      resourceRefs,
      status: hasRequiredContent ? 'ok' : 'error',
      diagnostic: hasRequiredContent
        ? undefined
        : answerText
          ? '自动识别结果缺少题干或分类，请人工补充；这道题暂时不会写入题库。'
          : '题目已识别，但源文档没有提供答案；请人工补充答案后再写入题库。',
      detectedUnknownTypeName: question.unknownTypeName,
      suggestedBehavior: question.suggestedBehavior,
      ...emptyDuplicateState(),
    }
  })
}

function prepareReview(analysisResult: DocxAnalysis, request = recognitionRequest) {
  if (!componentActive || request !== recognitionRequest || step.value !== 1) return
  invalidateDuplicateScans()
  draftReady.value = false
  importLimitNotice.value = ''
  sourceRecognizedItemCount.value = 0
  omittedItemCount.value = 0
  duplicateActionRules.clear()
  const { subject, chapter } = resolveTaxonomyDefault(
    appStore.subjects,
    route.query.subjectId,
    route.query.chapterId,
  )
  items.value = recognizeAnalysis(analysisResult)
  if (!items.value.length) {
    items.value = [{
      id: crypto.randomUUID(), ordinal: 1, selected: true, type: 'short_answer',
      stem: rich(analysisResult.visibleText), options: [], optionIds: [], answer: rich(''), explanation: rich(''),
      subjectId: subject?.id ?? '', chapterId: chapter?.id ?? '', tagIds: [], resourceRefs: [], status: 'error', ...emptyDuplicateState(),
      diagnostic: '文档中没有识别到可直接拆分的题目，请在这里人工整理后再导入。',
    }]
  }
  sessionTotalCount.value = items.value.length
  result.value = { success: 0, failed: 0, skipped: 0 }
  failedImportItemIds.clear()
  initializeBatchClassification()
  activeItemId.value = items.value[0]?.id ?? null
  items.value.forEach(refreshItemStatus)
  step.value = 2
  draftReady.value = true
  draftDirty.value = true
  lastSavedReviewSnapshot = null
  preparePostRecognitionDialogs()
}

function prepareExcelReview(excel: ExcelImportAnalysis, request = recognitionRequest) {
  if (!componentActive || request !== recognitionRequest || step.value !== 1) return
  invalidateDuplicateScans()
  draftReady.value = false
  sourceRecognizedItemCount.value = excel.sourceItemCount
  omittedItemCount.value = excel.omittedItemCount
  importLimitNotice.value = excel.warnings.join(' ')
  duplicateActionRules.clear()
  const { subject, chapter } = resolveTaxonomyDefault(
    appStore.subjects,
    route.query.subjectId,
    route.query.chapterId,
  )
  const defaults = { subjectId: subject?.id ?? '', chapterId: chapter?.id ?? '' }
  items.value = excel.rows.map((row, index) => {
    const normalized = normalizeExcelImportRow(
      row,
      appStore.questionTypes,
      appStore.subjects,
      appStore.tags,
      defaults,
    )
    return {
      id: crypto.randomUUID(),
      ordinal: index + 1,
      sourceRowNumber: row.rowNumber,
      selected: true,
      type: normalized.type,
      stem: rich(normalized.stem),
      options: normalized.options.map(rich),
      optionIds: normalized.options.map(() => crypto.randomUUID()),
      answer: rich(normalized.answer),
      explanation: rich(normalized.explanation),
      subjectId: normalized.subjectId,
      chapterId: normalized.chapterId,
      tagIds: normalized.tagIds,
      pendingTagNames: normalized.unknownTagNames,
      resourceRefs: [],
      status: 'error',
      detectedUnknownTypeName: normalized.detectedUnknownTypeName,
      suggestedBehavior: normalized.suggestedBehavior,
      ...emptyDuplicateState(),
    }
  })
  sessionTotalCount.value = items.value.length
  result.value = { success: 0, failed: 0, skipped: 0 }
  failedImportItemIds.clear()
  initializeBatchClassification()
  activeItemId.value = items.value[0]?.id ?? null
  items.value.forEach(refreshItemStatus)
  step.value = 2
  draftReady.value = true
  draftDirty.value = true
  lastSavedReviewSnapshot = null
  preparePostRecognitionDialogs()
}

function preparePostRecognitionDialogs() {
  const unknownGroups = new Map<string, ImportItem[]>()
  for (const item of items.value) {
    if (!item.detectedUnknownTypeName) continue
    const group = unknownGroups.get(item.detectedUnknownTypeName) ?? []
    group.push(item)
    unknownGroups.set(item.detectedUnknownTypeName, group)
  }
  unknownTypeDecisions.value = [...unknownGroups.entries()].map(([name, group]) => {
    const behavior = group[0]?.suggestedBehavior ?? 'open_response'
    const mapped = typeOptions.value.find((definition) => definition.behavior === behavior)
      ?? typeOptions.value[0]
    return {
      name,
      count: group.length,
      action: 'create',
      behavior,
      defaultOptionsText: /判断|是非|正误/u.test(name) ? '正确\n错误' : '',
      mapTo: mapped?.code ?? 'short_answer',
    }
  })
  const unknownTags = new Map<string, { name: string, count: number }>()
  for (const item of items.value) {
    for (const name of item.pendingTagNames ?? []) {
      const normalized = name.normalize('NFKC').trim().toLocaleLowerCase('zh-CN')
      const current = unknownTags.get(normalized)
      if (current) current.count += 1
      else unknownTags.set(normalized, { name, count: 1 })
    }
  }
  unknownTagDecisions.value = [...unknownTags.values()].map((entry) => ({ ...entry, create: true }))
  if (unknownTypeDecisions.value.length) {
    unknownTypeDialogOpen.value = true
    return
  }
  continuePostRecognitionSetup()
}

function continuePostRecognitionSetup() {
  if (unknownTagDecisions.value.length) {
    unknownTagDialogOpen.value = true
    return
  }
  if (importSessionId.value) void autosaveWordImportDraft(true, true)
  startReviewDuplicateScan()
}

async function startExcelRecognition(path: string) {
  if (!appStore.license.capabilities.canBatchImport) {
    ElMessage.warning('基础桌面模式不支持 Excel 批量导入；请使用单题录入或导入桌面专业版授权。')
    return
  }
  if (!selectedExcelSheet.value) {
    ElMessage.warning('请选择需要读取的 Excel 工作表。')
    return
  }
  recognitionRequest += 1
  const request = recognitionRequest
  clearInterval(timer)
  clearTimeout(prepareReviewTimer)
  step.value = 1
  progress.value = 6
  recognitionError.value = ''
  analysis.value = null
  pendingImageOccurrences.value = []
  pendingFormulaOccurrences.value = []
  pendingTableOccurrences.value = []
  parseLogs.value = ['正在验证 Excel 文件结构、大小与安全边界…']
  const progressTimer = setInterval(() => {
    if (!componentActive || request !== recognitionRequest || step.value !== 1) {
      clearInterval(progressTimer)
      return
    }
    progress.value = Math.min(88, progress.value + 4)
  }, 350)
  timer = progressTimer
  try {
    const started: BeginExcelImportResult = await backend.beginExcelImport(path, selectedExcelSheet.value)
    if (!componentActive || request !== recognitionRequest || step.value !== 1) return
    importSessionId.value = started.draft.payload.importSessionId ?? null
    parserVersion.value = started.draft.payload.parserVersion
    if (!hasPersistedDraft.value) updatePendingDraftCount(1)
    hasPersistedDraft.value = true
    lastDraftAutosavedAt.value = started.draft.autosavedAt
    if (selectedFile.value) selectedFile.value.size = started.analysis.sourceFileSize
    progress.value = 100
    parseLogs.value.push(
      `已读取工作表“${started.analysis.selectedSheetName}”，表头位于第 ${started.analysis.headerRowNumber} 行`,
    )
    parseLogs.value.push(`识别到 ${started.analysis.sourceItemCount} 行题目数据`)
    for (const warning of started.analysis.warnings) parseLogs.value.push(`提示：${warning}`)
    parseLogs.value.push('Excel 读取完成，正在生成可人工审查的题目草稿')
    prepareReviewTimer = setTimeout(() => prepareExcelReview(started.analysis, request), 180)
  } catch (reason) {
    if (!componentActive || request !== recognitionRequest || step.value !== 1) return
    recognitionError.value = errorMessage(reason, 'Excel 题库分析失败')
    parseLogs.value.push(`错误：${recognitionError.value}`)
  } finally {
    clearInterval(progressTimer)
    if (timer === progressTimer) timer = undefined
  }
}

const sourceKind = computed(() => selectedFile.value?.kind ?? 'docx')
const sourceFormatLabel = computed(() => sourceKind.value === 'xlsx' ? 'Excel' : 'Word')

function startReviewDuplicateScan() {
  const eligibleItems = items.value.filter((item) => !itemValidationMessage(item))
  const scan = scanDuplicates(eligibleItems)
  initialDuplicateScan = scan
  void scan.finally(() => {
    if (initialDuplicateScan === scan) initialDuplicateScan = null
  })
}

function itemValidationMessage(item: ImportItem) {
  const needsOptions = isChoiceQuestionType(item.type, appStore.questionTypes)
  if (!item.type || !item.stem.plainText.trim()) return '题型和题干均为必填项。'
  if (!item.answer.plainText.trim()) return '题目已识别，但源文档没有提供答案；请人工补充答案后再写入题库。'
  if (!item.subjectId || !item.chapterId) return '请为题目选择学科和章节。'
  if (needsOptions && (item.options.length < 2 || item.options.some((option) => !option.plainText.trim()))) {
    return '选择题至少需要两个非空选项。'
  }
  return null
}

function refreshItemStatus(item: ImportItem) {
  const validationError = itemValidationMessage(item)
  if (validationError) {
    item.status = 'error'
    item.diagnostic = validationError
    return false
  }
  if (item.duplicateChecking) {
    item.status = 'warning'
    item.diagnostic = '正在与本地题库进行重复内容检查…'
    return false
  }
  if (item.duplicateCheckError) {
    item.status = 'warning'
    item.diagnostic = item.duplicateCheckError
    return false
  }
  if (item.duplicateNeedsCheck) {
    item.status = 'warning'
    item.diagnostic = '内容已准备好，正在等待重复题检查。'
    return false
  }
  if (item.duplicateKind !== 'none' && !item.duplicateAction) {
    item.status = 'warning'
    item.diagnostic = `检测到${item.duplicateKind === 'exact' ? '完全重复' : '疑似重复'}题目，请先选择处理方式。`
    return false
  }
  item.status = 'ok'
  item.diagnostic = undefined
  return true
}

function validateItem(item: ImportItem) {
  return refreshItemStatus(item)
}

function ensureOptionIds(item: ImportItem) {
  while (item.optionIds.length < item.options.length) item.optionIds.push(crypto.randomUUID())
  if (item.optionIds.length > item.options.length) item.optionIds.splice(item.options.length)
}

function activeResourceRefs(item: ImportItem) {
  ensureOptionIds(item)
  return activeQuestionResourceRefs(item.resourceRefs, {
    stem: item.stem,
    options: item.options.map((content, index) => ({
      id: item.optionIds[index]!,
      content,
    })),
    answer: item.answer,
    explanation: item.explanation,
  })
}

async function confirmUnknownTypes() {
  if (unknownTypeDecisions.value.some((decision) => decision.action === 'map' && !decision.mapTo)) {
    ElMessage.warning('请为所有映射项选择一个现有题型')
    return
  }
  for (const decision of unknownTypeDecisions.value) {
    const mappedDefinition = decision.action === 'map'
      ? typeOptions.value.find((definition) => definition.code === decision.mapTo)
      : null
    const behavior = mappedDefinition?.behavior ?? decision.behavior
    const defaults = mappedDefinition?.defaultOptions
      ?? [...new Set(decision.defaultOptionsText.split(/[\n,，、]+/u).map((value) => value.trim()).filter(Boolean))]
    const groupNeedsOptions = items.value.some((item) => (
      item.detectedUnknownTypeName === decision.name
      && (item.options.length < 2 || item.options.some((option) => !option.plainText.trim()))
    ))
    if (matchesChoiceBehavior(behavior) && groupNeedsOptions && defaults.length < 2) {
      ElMessage.warning(`题型“${decision.name}”使用选择结构，请填写至少两个默认选项，供未带选项的题目使用。`)
      return
    }
  }
  resolvingUnknownTypes.value = true
  try {
    for (const decision of unknownTypeDecisions.value) {
      const requestedDefaults = [...new Set(
        decision.defaultOptionsText.split(/[\n,，、]+/u).map((value) => value.trim()).filter(Boolean),
      )]
      const definition = decision.action === 'create'
        ? await appStore.saveQuestionType({
            code: null,
            name: decision.name,
            behavior: decision.behavior,
            aliases: [],
            defaultOptions: matchesChoiceBehavior(decision.behavior) ? requestedDefaults : [],
            isEnabled: true,
          })
        : typeOptions.value.find((candidate) => candidate.code === decision.mapTo)
      if (!definition) throw new Error(`映射目标“${decision.mapTo}”已经不存在，请刷新后重试。`)
      for (const item of items.value) {
        if (item.detectedUnknownTypeName !== decision.name) continue
        item.type = definition.code
        if (matchesChoiceBehavior(definition.behavior)
          && (item.options.length < 2 || item.options.some((option) => !option.plainText.trim()))) {
          item.options = definition.defaultOptions.map((option) => rich(option))
          item.optionIds = definition.defaultOptions.map(() => crypto.randomUUID())
        }
        item.detectedUnknownTypeName = null
        item.suggestedBehavior = null
        refreshItemStatus(item)
      }
    }
    unknownTypeDialogOpen.value = false
    draftDirty.value = true
    continuePostRecognitionSetup()
    ElMessage.success('来源文件中的新题型已经确认')
  } catch (reason) {
    ElMessage.error(errorMessage(reason, '新题型处理失败'))
  } finally {
    resolvingUnknownTypes.value = false
  }
}

async function confirmUnknownTags() {
  creatingUnknownTags.value = true
  try {
    const selectedKeys = new Set(
      unknownTagDecisions.value
        .filter((decision) => decision.create)
        .map((decision) => decision.name.normalize('NFKC').trim().toLocaleLowerCase('zh-CN')),
    )
    const tagIdByKey = new Map(
      appStore.tags.map((tag) => [tag.name.normalize('NFKC').trim().toLocaleLowerCase('zh-CN'), tag.id]),
    )
    for (const decision of unknownTagDecisions.value.filter((entry) => entry.create)) {
      const normalized = decision.name.normalize('NFKC').trim().toLocaleLowerCase('zh-CN')
      if (tagIdByKey.has(normalized)) continue
      const id = await appStore.createTag(decision.name)
      tagIdByKey.set(normalized, id)
    }
    for (const item of items.value) {
      for (const name of item.pendingTagNames ?? []) {
        const normalized = name.normalize('NFKC').trim().toLocaleLowerCase('zh-CN')
        const id = selectedKeys.has(normalized) ? tagIdByKey.get(normalized) : undefined
        if (id && !item.tagIds.includes(id)) item.tagIds.push(id)
      }
      item.pendingTagNames = []
      refreshItemStatus(item)
    }
    const createdCount = selectedKeys.size
    unknownTagDecisions.value = []
    unknownTagDialogOpen.value = false
    draftDirty.value = true
    if (importSessionId.value) void autosaveWordImportDraft(true, true)
    startReviewDuplicateScan()
    ElMessage.success(createdCount ? `已确认并匹配 ${createdCount} 个新标签` : '未创建新标签，继续题目审查')
  } catch (reason) {
    ElMessage.error(errorMessage(reason, 'Excel 标签确认失败'))
  } finally {
    creatingUnknownTags.value = false
  }
}

function itemDraft(item: ImportItem): QuestionDraft {
  ensureOptionIds(item)
  return {
    id: item.id,
    type: item.type,
    stem: clonePlain(item.stem),
    options: isChoiceQuestionType(item.type, appStore.questionTypes)
      ? item.options.map((content, position) => ({ id: item.optionIds[position]!, position, content: clonePlain(content) }))
      : [],
    answer: clonePlain(item.answer),
    explanation: clonePlain(item.explanation),
    subjectId: item.subjectId,
    chapterId: item.chapterId,
    tagIds: [...item.tagIds],
    resourceRefs: clonePlain(activeResourceRefs(item)),
  }
}

function toWordImportDraftItem(item: ImportItem): WordImportDraftItem {
  const candidate = item.duplicateKind === 'none' || !item.duplicateCandidate
    ? null
    : clonePlain(item.duplicateCandidate)
  const duplicateKind = candidate ? item.duplicateKind : 'none'
  return {
    id: item.id,
    ordinal: item.ordinal,
    selected: item.selected,
    payload: itemDraft(item),
    status: item.status,
    diagnostic: item.diagnostic ?? null,
    duplicateKind,
    candidate,
    action: candidate ? item.duplicateAction ?? null : null,
    needsCheck: item.duplicateNeedsCheck || item.duplicateChecking || Boolean(item.duplicateCheckError),
    checkError: item.duplicateCheckError ?? null,
  }
}

function fromWordImportDraftItem(entry: WordImportDraftItem): ImportItem {
  const candidate = entry.duplicateKind === 'none' || !entry.candidate
    ? null
    : clonePlain(entry.candidate)
  return {
    id: entry.id,
    ordinal: entry.ordinal,
    selected: entry.selected,
    type: entry.payload.type,
    stem: clonePlain(entry.payload.stem),
    options: [...entry.payload.options]
      .sort((left, right) => left.position - right.position)
      .map((option) => clonePlain(option.content)),
    optionIds: [...entry.payload.options]
      .sort((left, right) => left.position - right.position)
      .map((option) => option.id),
    answer: clonePlain(entry.payload.answer),
    explanation: clonePlain(entry.payload.explanation),
    subjectId: entry.payload.subjectId,
    chapterId: entry.payload.chapterId,
    tagIds: [...entry.payload.tagIds],
    resourceRefs: clonePlain(entry.payload.resourceRefs ?? []),
    status: entry.status,
    diagnostic: entry.diagnostic ?? undefined,
    duplicateKind: candidate ? entry.duplicateKind : 'none',
    duplicateCandidate: candidate,
    duplicateAction: candidate ? entry.action ?? undefined : undefined,
    duplicateChecking: false,
    // A recovered candidate may have changed or been recycled while the app
    // was closed, so every recovered item is checked again before import.
    duplicateNeedsCheck: true,
    duplicateCheckError: undefined,
  }
}

function buildWordImportDraftPayload(): WordImportDraftPayload {
  if (!selectedFile.value) throw new Error('没有可保存的批量导入来源文件。')
  if (items.value.length > MAX_WORD_IMPORT_ITEMS
    || items.value.some((item) => !Number.isInteger(item.ordinal) || item.ordinal < 1 || item.ordinal > MAX_WORD_IMPORT_ITEMS)) {
    throw new Error(`单次批量导入草稿最多保存 ${MAX_WORD_IMPORT_ITEMS} 道题，请拆分来源文件后重试。`)
  }
  const activeId = items.value.some((item) => item.id === activeItemId.value)
    ? activeItemId.value
    : items.value[0]?.id ?? null
  return {
    schemaVersion: 1,
    sourceFileName: selectedFile.value.name.normalize('NFKC').trim(),
    sourceFileSize: Math.max(0, Math.trunc(selectedFile.value.size)),
    parserVersion: parserVersion.value,
    importSessionId: importSessionId.value,
    sourceItemCount: Math.max(
      sourceRecognizedItemCount.value,
      items.value.length + omittedItemCount.value,
    ),
    omittedItemCount: omittedItemCount.value,
    activeItemId: activeId,
    items: items.value.map(toWordImportDraftItem),
  }
}

function reviewSnapshot() {
  return JSON.stringify({
    sourceFile: selectedFile.value
      ? { name: selectedFile.value.name, size: selectedFile.value.size }
      : null,
    sourceRecognizedItemCount: sourceRecognizedItemCount.value,
    omittedItemCount: omittedItemCount.value,
    activeItemId: activeItemId.value,
    items: items.value.map((item) => ({
      id: item.id,
      ordinal: item.ordinal,
      selected: item.selected,
      type: item.type,
      stem: item.stem,
      options: item.options,
      optionIds: item.optionIds,
      answer: item.answer,
      explanation: item.explanation,
      subjectId: item.subjectId,
      chapterId: item.chapterId,
      tagIds: item.tagIds,
      resourceRefs: item.resourceRefs,
      status: item.status,
      diagnostic: item.diagnostic ?? null,
      duplicateKind: item.duplicateKind,
      duplicateCandidate: item.duplicateCandidate,
      duplicateAction: item.duplicateAction ?? null,
      duplicateChecking: item.duplicateChecking,
      duplicateNeedsCheck: item.duplicateNeedsCheck,
      duplicateCheckError: item.duplicateCheckError ?? null,
    })),
  })
}

function updatePendingDraftCount(delta: number) {
  appStore.pendingDraftCount = Math.max(0, appStore.pendingDraftCount + delta)
}

async function autosaveWordImportDraft(showFailure = true, force = false) {
  if (!draftReady.value || step.value !== 2 || !selectedFile.value) return true
  if (draftAutosaving.value || (saving.value && !force)) return false
  if (!draftDirty.value && !force) return true

  const snapshot = reviewSnapshot()
  if (!force && snapshot === lastSavedReviewSnapshot) {
    draftDirty.value = false
    return true
  }

  draftAutosaving.value = true
  try {
    const saved = await withTimeout(
      backend.saveWordImportDraft(buildWordImportDraftPayload()),
      IMPORT_DRAFT_TIMEOUT_MS,
      '批量导入草稿保存超时，请稍后重试。',
    )
    if (!hasPersistedDraft.value) updatePendingDraftCount(1)
    hasPersistedDraft.value = true
    lastDraftAutosavedAt.value = saved.autosavedAt
    if (snapshot === reviewSnapshot()) {
      lastSavedReviewSnapshot = snapshot
      draftDirty.value = false
    } else {
      draftDirty.value = true
    }
    return true
  } catch (reason) {
    if (showFailure) {
      ElMessage.error(errorMessage(reason, '批量导入草稿保存失败，请检查本地数据库。'))
    }
    return false
  } finally {
    draftAutosaving.value = false
  }
}

async function saveDraftManually() {
  if (!reviewPersistenceAvailable.value) {
    explainDesktopRequirement()
    return
  }
  if (await autosaveWordImportDraft(true, true)) {
    ElMessage.success(!desktopAvailable && isDocumentEntryReview.value
      ? '审查草稿已保存到当前浏览器演示数据。'
      : '批量导入审查草稿已保存到本地数据库。')
  }
}

async function deletePersistedWordImportDraft() {
  await withTimeout(
    backend.deleteWordImportDraft(),
    IMPORT_DRAFT_TIMEOUT_MS,
    '批量导入草稿清理超时，请稍后重试。',
  )
  if (hasPersistedDraft.value) updatePendingDraftCount(-1)
  hasPersistedDraft.value = false
  lastDraftAutosavedAt.value = null
  lastSavedReviewSnapshot = null
  importSessionId.value = null
  pendingImageOccurrences.value = []
  pendingFormulaOccurrences.value = []
  pendingTableOccurrences.value = []
  parserVersion.value = CURRENT_WORD_IMPORT_PARSER_VERSION
}

function restoreWordImportDraft(saved: WordImportDraftRecord) {
  invalidateDuplicateScans()
  draftReady.value = false
  sourceRecognizedItemCount.value = saved.payload.sourceItemCount ?? saved.payload.items.length
  omittedItemCount.value = saved.payload.omittedItemCount ?? 0
  importLimitNotice.value = omittedItemCount.value > 0
    ? `原文档共识别到 ${sourceRecognizedItemCount.value} 个题目；此前受单次 ${MAX_WORD_IMPORT_ITEMS} 项安全上限影响，有 ${omittedItemCount.value} 项未载入。请拆分原文档后再次导入剩余内容。`
    : ''
  selectedFile.value = {
    name: saved.payload.sourceFileName,
    size: saved.payload.sourceFileSize,
    kind: saved.payload.sourceFileName.toLocaleLowerCase('zh-CN').endsWith('.xlsx') ? 'xlsx' : 'docx',
  }
  importSessionId.value = saved.payload.importSessionId ?? null
  parserVersion.value = saved.payload.parserVersion
  pendingImageOccurrences.value = []
  pendingFormulaOccurrences.value = []
  pendingTableOccurrences.value = []
  analysis.value = null
  recognitionError.value = ''
  progress.value = 100
  parseLogs.value = []
  duplicateActionRules.clear()
  items.value = saved.payload.items.map(fromWordImportDraftItem)
  sessionTotalCount.value = Math.max(
    items.value.length,
    (saved.payload.sourceItemCount ?? items.value.length) - (saved.payload.omittedItemCount ?? 0),
  )
  result.value = { success: 0, failed: 0, skipped: 0 }
  failedImportItemIds.clear()
  initializeBatchClassification()
  activeItemId.value = items.value.some((item) => item.id === saved.payload.activeItemId)
    ? saved.payload.activeItemId ?? null
    : items.value[0]?.id ?? null
  items.value.forEach(refreshItemStatus)
  step.value = 2
  draftReady.value = true
  draftDirty.value = true
  lastSavedReviewSnapshot = null

  const eligibleItems = items.value.filter((item) => !itemValidationMessage(item))
  const scan = scanDuplicates(eligibleItems)
  initialDuplicateScan = scan
  void scan.finally(() => {
    if (initialDuplicateScan === scan) initialDuplicateScan = null
  })
}

async function offerWordImportDraftRecovery() {
  draftRecoveryRequest += 1
  const request = draftRecoveryRequest
  recoveryChecking.value = true
  draftReadBlocked.value = false
  draftReadError.value = ''
  try {
    const saved = await backend.getWordImportDraft()
    if (!componentActive || request !== draftRecoveryRequest || !saved) return

    hasPersistedDraft.value = true
    lastDraftAutosavedAt.value = saved.autosavedAt
    const documentEntryDraft = isDocumentEntryReviewDraft(
      saved.payload.sourceFileName,
      DOCUMENT_ENTRY_SOURCE_FILE_NAME,
    )
    const excelDraft = saved.payload.sourceFileName.toLocaleLowerCase('zh-CN').endsWith('.xlsx')
    const expectedParserVersion = excelDraft
      ? CURRENT_EXCEL_IMPORT_PARSER_VERSION
      : expectedWordImportReviewParserVersion(
          documentEntryDraft,
          CURRENT_WORD_IMPORT_PARSER_VERSION,
          DOCUMENT_ENTRY_PARSER_VERSION,
        )
    if (
      isDocumentEntryReview.value
      && documentEntryDraft
      && saved.payload.parserVersion === expectedParserVersion
    ) {
      restoreWordImportDraft(saved)
      return
    }
    if (saved.payload.parserVersion !== expectedParserVersion) {
      try {
        await ElMessageBox.confirm(
          documentEntryDraft
            ? '发现由旧版文档识别规则生成的录入草稿。当前版本已改进题型自动判断；建议返回文档编辑页重新识别。若你已手工校对，也可以继续恢复旧草稿。'
            : `发现“${saved.payload.sourceFileName}”由旧版识别器生成的导入草稿。建议删除旧草稿后重新选择原文件；若你已手工校对，也可以继续恢复。`,
          documentEntryDraft ? '旧版文档录入草稿' : '旧版批量导入草稿',
          {
            type: 'warning',
            confirmButtonText: documentEntryDraft ? '删除旧草稿' : '删除并重新选择',
            cancelButtonText: '仍恢复旧草稿',
            showClose: false,
            closeOnClickModal: false,
            closeOnPressEscape: false,
            closeOnHashChange: false,
          },
        )
      } catch {
        if (componentActive && request === draftRecoveryRequest) restoreWordImportDraft(saved)
        return
      }
      if (!componentActive || request !== draftRecoveryRequest) return
      try {
        await deletePersistedWordImportDraft()
        ElMessage.success(documentEntryDraft
          ? '旧版草稿已删除，请返回文档式批量录入重新识别。'
          : '旧版草稿已删除，请重新选择原文件。')
        return
      } catch (reason) {
        if (!componentActive || request !== draftRecoveryRequest) return
        ElMessage.error(errorMessage(reason, '旧版草稿删除失败。为避免覆盖数据，已恢复这份草稿。'))
        restoreWordImportDraft(saved)
        return
      }
    }
    try {
      await ElMessageBox.confirm(
        documentEntryDraft
          ? `发现文档式批量录入的未完成审查草稿，共 ${saved.payload.items.length} 道待审查题目。是否恢复？`
          : `发现“${saved.payload.sourceFileName}”的未完成导入草稿，共 ${saved.payload.items.length} 道待审查题目。是否恢复？`,
        documentEntryDraft ? '恢复文档式批量录入草稿' : '恢复批量导入草稿',
        {
          type: 'info',
          confirmButtonText: '恢复草稿',
          cancelButtonText: '删除草稿',
          showClose: false,
          closeOnClickModal: false,
          closeOnPressEscape: false,
          closeOnHashChange: false,
        },
      )
    } catch (action) {
      if (!componentActive || request !== draftRecoveryRequest) return
      if (action !== 'cancel') {
        restoreWordImportDraft(saved)
        return
      }
      try {
        await deletePersistedWordImportDraft()
        return
      } catch (reason) {
        if (!componentActive || request !== draftRecoveryRequest) return
        ElMessage.error(errorMessage(reason, '草稿删除失败。为避免覆盖数据，已改为恢复这份草稿。'))
      }
    }
    if (!componentActive || request !== draftRecoveryRequest) return
    restoreWordImportDraft(saved)
  } catch (reason) {
    if (componentActive && request === draftRecoveryRequest) {
      draftReadBlocked.value = true
      draftReadError.value = errorMessage(reason, '无法读取现有批量导入草稿。')
      ElMessage.error('现有批量导入草稿无法读取；为避免覆盖，已暂停新的导入。')
    }
  } finally {
    if (componentActive && request === draftRecoveryRequest) recoveryChecking.value = false
  }
}

async function retryWordImportDraftRecovery() {
  if (recoveryChecking.value) return
  await offerWordImportDraftRecovery()
}

async function discardUnreadableWordImportDraft() {
  if (recoveryChecking.value) return
  try {
    await ElMessageBox.confirm(
      '现有批量导入草稿无法读取。删除后无法恢复；只有确认不再需要旧草稿时才继续。',
      '删除无法读取的草稿',
      {
        type: 'error',
        confirmButtonText: '确认永久删除',
        cancelButtonText: '保留并返回',
        showClose: false,
        closeOnClickModal: false,
        closeOnPressEscape: false,
      },
    )
  } catch {
    return
  }

  recoveryChecking.value = true
  try {
    await backend.deleteWordImportDraft()
    await appStore.initialize()
    await offerWordImportDraftRecovery()
  } catch (reason) {
    draftReadBlocked.value = true
    draftReadError.value = errorMessage(reason, '无法删除旧草稿。')
    ElMessage.error(draftReadError.value)
  } finally {
    if (componentActive) recoveryChecking.value = false
  }
}

function warnBeforeBrowserClose(event: BeforeUnloadEvent) {
  if (!draftDirty.value && !saving.value && !draftAutosaving.value) return
  event.preventDefault()
  event.returnValue = ''
}

async function registerCloseProtection() {
  if (!isTauriRuntime()) {
    window.addEventListener('beforeunload', warnBeforeBrowserClose)
    return
  }

  try {
    const unlisten = await getCurrentWindow().onCloseRequested(async (event) => {
      if (!draftDirty.value && !saving.value && !draftAutosaving.value) return
      event.preventDefault()
      if (saving.value) {
        ElMessage.warning('正在写入题库，请等待本次导入结束后再关闭软件。')
        return
      }
      if (draftAutosaving.value) {
        ElMessage.info('本地草稿正在保存，请等待完成后再关闭软件。')
        return
      }
      if (closePromptOpen) return
      closePromptOpen = true
      try {
        await ElMessageBox.confirm(
          '当前批量导入审查还有未保存的修改。关闭软件前先保存本地草稿吗？',
          '关闭软件',
          {
            type: 'warning',
            confirmButtonText: '保存草稿并关闭',
            cancelButtonText: '继续审查',
            closeOnClickModal: false,
          },
        )
        if (await autosaveWordImportDraft()) await getCurrentWindow().destroy()
      } catch {
        // The teacher chose to keep reviewing, so the close request stays cancelled.
      } finally {
        closePromptOpen = false
      }
    })
    if (componentActive) unlistenCloseRequested = unlisten
    else unlisten()
  } catch {
    if (componentActive) window.addEventListener('beforeunload', warnBeforeBrowserClose)
  }
}

watch([items, activeItemId], () => {
  if (draftReady.value && step.value === 2) draftDirty.value = true
}, { deep: true })

onMounted(() => {
  componentActive = true
  if (!desktopAvailable && !isDocumentEntryReview.value) {
    recoveryChecking.value = false
    return
  }
  void offerWordImportDraftRecovery()
  void registerCloseProtection()
  autosaveTimer = setInterval(() => { void autosaveWordImportDraft() }, 60_000)
})

onBeforeUnmount(() => {
  componentActive = false
  recognitionRequest += 1
  draftRecoveryRequest += 1
  invalidateDuplicateScans()
  clearInterval(timer)
  clearTimeout(prepareReviewTimer)
  clearInterval(autosaveTimer)
  unlistenCloseRequested?.()
  window.removeEventListener('beforeunload', warnBeforeBrowserClose)
  ElMessageBox.close()
  duplicateDialogRequest += 1
  resolveDuplicateAction?.(null)
  resolveDuplicateAction = undefined
})

onBeforeRouteLeave(async () => {
  if (recoveryChecking.value) {
    ElMessage.info('请先完成未完成草稿检查，再离开批量导入页面。')
    return false
  }
  if (saving.value) {
    ElMessage.warning('正在写入题库，请等待本次导入结束后再离开。')
    return false
  }
  if (draftAutosaving.value) {
    ElMessage.info('本地草稿正在保存，请等待完成后再离开。')
    return false
  }
  if (!draftDirty.value) return true
  try {
    await ElMessageBox.confirm(
      '当前批量导入审查还有未保存的修改，确定离开吗？草稿会保留。',
      '未保存内容',
      {
        type: 'warning',
        confirmButtonText: '保留草稿并离开',
        cancelButtonText: '继续审查',
      },
    )
    return await autosaveWordImportDraft()
  } catch {
    return false
  }
})

function duplicateContentSignature(item: ImportItem) {
  return JSON.stringify({
    type: item.type,
    stem: item.stem,
    options: item.options,
    answer: item.answer,
  })
}

function duplicateResultChanged(item: ImportItem, result: QuestionDuplicateCheck) {
  return item.duplicateKind !== result.status
    || item.duplicateCandidate?.id !== result.candidate?.id
    || item.duplicateCandidate?.contentVersion !== result.candidate?.contentVersion
    || item.duplicateCandidate?.similarityPercent !== result.candidate?.similarityPercent
}

function importItemCandidate(previous: ImportItem): QuestionDuplicateCandidate {
  const preview = [...previous.stem.plainText.trim().replace(/\s+/gu, ' ')]
  return {
    id: previous.id,
    type: previous.type,
    stemPreview: preview.length > 160 ? `${preview.slice(0, 160).join('')}…` : preview.join(''),
    subjectName: '本批导入',
    chapterName: `第 ${previous.ordinal} 题`,
    similarityPercent: 100,
    contentVersion: 0,
    sourceKind: 'import-item',
    sourceOrdinal: previous.ordinal,
  }
}

function normalizeBatchCheck(
  batchResult: QuestionDuplicateBatchResult,
  entry: ImportItem,
  seenFingerprints: Map<string, ImportItem>,
) {
  const result = batchResult.items.find((candidate) => candidate.clientId === entry.id)
  if (!result) throw new Error('批量重复检查返回结果不完整。')
  let check = clonePlain(result.check)
  const previous = seenFingerprints.get(result.exactFingerprint)
    ?? (check.candidate?.sourceKind === 'import-item'
      ? items.value.find((candidate) => candidate.id === check.candidate?.id)
      : undefined)
  if (previous && check.status !== 'exact') {
    check = {
      status: 'exact',
      candidate: importItemCandidate(previous),
      evaluatedCandidateCount: 1,
      suspectedThresholdPercent: check.suspectedThresholdPercent,
      similarityMethod: check.similarityMethod,
    }
  } else if (previous && check.candidate?.sourceKind === 'import-item') {
    check.candidate = importItemCandidate(previous)
  }
  if (!seenFingerprints.has(result.exactFingerprint)) {
    seenFingerprints.set(result.exactFingerprint, entry)
  }
  return check
}

function normalizeImportBatchCheck(
  result: QuestionDuplicateBatchItemResult,
  entry: ImportItem,
) {
  if (result.clientId !== entry.id) throw new Error('批次内重复检查返回结果不匹配。')
  const check = clonePlain(result.check)
  if (check.candidate?.sourceKind === 'import-item') {
    const previous = items.value.find((candidate) => candidate.id === check.candidate?.id)
    if (previous) {
      check.candidate = {
        ...importItemCandidate(previous),
        similarityPercent: check.candidate.similarityPercent,
      }
    }
  }
  return check
}

function mergeImportBatchCheck(item: ImportItem, check: QuestionDuplicateCheck) {
  const currentIsDatabaseExact = item.duplicateKind === 'exact'
    && item.duplicateCandidate?.sourceKind !== 'import-item'
  if (currentIsDatabaseExact || check.status === 'none') return

  const shouldReplace = check.status === 'exact'
    ? item.duplicateKind !== 'exact'
    : item.duplicateKind === 'none'
      || (item.duplicateKind === 'suspected'
        && (check.candidate?.similarityPercent ?? 0)
          > (item.duplicateCandidate?.similarityPercent ?? 0))
  if (!shouldReplace) return
  if (duplicateResultChanged(item, check)) item.duplicateAction = undefined
  item.duplicateKind = check.status
  item.duplicateCandidate = check.candidate ? clonePlain(check.candidate) : null
}

function markDuplicateBatchUnverified(entries: ImportItem[], message: string) {
  for (const entry of entries) {
    if (!items.value.includes(entry)) continue
    entry.duplicateAction = undefined
    entry.duplicateNeedsCheck = true
    entry.duplicateCheckError = message
    refreshItemStatus(entry)
  }
}

async function scanDuplicates(
  entries: ImportItem[],
  generation = duplicateScanGeneration,
  mode: 'full' | 'exact' = 'full',
): Promise<boolean> {
  const eligible = entries.filter((entry) => !itemValidationMessage(entry))
  if (generation !== duplicateScanGeneration) return false
  if (!eligible.length) return true
  const previousNeedsCheck = new Map(eligible.map((entry) => [entry.id, entry.duplicateNeedsCheck]))
  const signatures = new Map(eligible.map((entry) => [entry.id, duplicateContentSignature(entry)]))
  const seenFingerprints = new Map<string, ImportItem>()
  const includeImportBatchPass = mode === 'full' && eligible.length > 1
  const scanId = crypto.randomUUID()
  activeDuplicateScanIds.add(scanId)
  activeDuplicateScans += 1
  duplicateScanRunning.value = true
  duplicateScanPhase.value = 'database'
  duplicateScanIncludesImportBatchPass.value = includeImportBatchPass
  duplicateScanCompleted.value = 0
  duplicateScanTotal.value = eligible.length
  duplicateScanNotice.value = ''
  for (const entry of eligible) {
    entry.duplicateChecking = true
    entry.duplicateCheckError = undefined
    refreshItemStatus(entry)
  }
  try {
    for (const batch of chunked(eligible, DUPLICATE_BATCH_SIZE)) {
      if (generation !== duplicateScanGeneration) return false
      try {
        const result = await withTimeout(
          backend.checkQuestionDuplicatesBatch({
            mode,
            scanId,
            items: batch.map((entry) => ({ clientId: entry.id, draft: itemDraft(entry) })),
          }),
          DUPLICATE_BATCH_TIMEOUT_MS,
          `重复检查超过 ${DUPLICATE_BATCH_TIMEOUT_MS / 1_000} 秒，已停止等待。`,
        )
        if (generation !== duplicateScanGeneration) return false
        for (const entry of batch) {
          if (!items.value.includes(entry)) continue
          if (signatures.get(entry.id) !== duplicateContentSignature(entry)) {
            const message = '题目内容在重复检查期间发生变化，本批题目需要重新检查。'
            duplicateScanNotice.value = message
            markDuplicateBatchUnverified(eligible, message)
            return false
          }
          const check = normalizeBatchCheck(result, entry, seenFingerprints)
          const preserveFreshSuspected = mode === 'exact'
            && !previousNeedsCheck.get(entry.id)
            && entry.duplicateKind === 'suspected'
            && check.status === 'none'
          if (!preserveFreshSuspected) {
            if (duplicateResultChanged(entry, check)) entry.duplicateAction = undefined
            entry.duplicateKind = check.status
            entry.duplicateCandidate = check.candidate ? clonePlain(check.candidate) : null
            if (check.status === 'none') entry.duplicateAction = undefined
          }
          entry.duplicateNeedsCheck = false
          entry.duplicateCheckError = undefined
          applyRememberedDuplicateAction(entry)
        }
      } catch (reason) {
        if (generation !== duplicateScanGeneration) return false
        void backend.cancelQuestionDuplicateScan(scanId)
        const message = `重复题检查失败：${errorMessage(reason, '无法读取本地题库')}`
        duplicateScanNotice.value = `${message} 本批题目均未通过完整检查，可点击“重试检查”继续。`
        markDuplicateBatchUnverified(eligible, message)
        return false
      } finally {
        if (generation === duplicateScanGeneration) {
          duplicateScanCompleted.value = Math.min(
            duplicateScanTotal.value,
            duplicateScanCompleted.value + batch.length,
          )
        }
      }
    }

    if (generation !== duplicateScanGeneration) return false
    const importCandidates = eligible.filter((entry) => !(
      entry.duplicateKind === 'exact' && entry.duplicateCandidate?.sourceKind !== 'import-item'
    ))
    if (includeImportBatchPass && importCandidates.length > 1
      && generation === duplicateScanGeneration) {
      duplicateScanPhase.value = 'import-batch'
      duplicateScanCompleted.value = eligible.length - importCandidates.length
      try {
        const result = await withTimeout(
          backend.checkQuestionDuplicatesBatch({
            mode: 'import',
            scanId,
            items: importCandidates.map((entry) => ({ clientId: entry.id, draft: itemDraft(entry) })),
          }),
          DUPLICATE_BATCH_TIMEOUT_MS * 2,
          `批次内重复检查超过 ${(DUPLICATE_BATCH_TIMEOUT_MS * 2) / 1_000} 秒，已停止等待。`,
        )
        if (generation !== duplicateScanGeneration) return false
        const checksById = new Map(result.items.map((check) => [check.clientId, check]))
        for (const entry of importCandidates) {
          if (!items.value.includes(entry)) continue
          if (signatures.get(entry.id) !== duplicateContentSignature(entry)) {
            const message = '题目内容在批次内重复检查期间发生变化，本批题目需要重新检查。'
            duplicateScanNotice.value = message
            markDuplicateBatchUnverified(eligible, message)
            return false
          }
          const check = checksById.get(entry.id)
          if (!check) throw new Error('批次内重复检查返回结果不完整。')
          mergeImportBatchCheck(entry, normalizeImportBatchCheck(check, entry))
          entry.duplicateNeedsCheck = false
          entry.duplicateCheckError = undefined
          applyRememberedDuplicateAction(entry)
        }
        duplicateScanCompleted.value = duplicateScanTotal.value
      } catch (reason) {
        if (generation !== duplicateScanGeneration) return false
        void backend.cancelQuestionDuplicateScan(scanId)
        const message = `批次内重复检查失败：${errorMessage(reason, '无法比较本批题目')}`
        duplicateScanNotice.value = `${message} 本批题目均未通过完整检查，可点击“重试检查”继续。`
        markDuplicateBatchUnverified(eligible, message)
        return false
      }
    } else if (includeImportBatchPass && generation === duplicateScanGeneration) {
      duplicateScanPhase.value = 'import-batch'
      duplicateScanCompleted.value = duplicateScanTotal.value
    }
    return generation === duplicateScanGeneration
  } finally {
    activeDuplicateScanIds.delete(scanId)
    if (generation === duplicateScanGeneration) {
      for (const entry of eligible) {
        if (!items.value.includes(entry)) continue
        entry.duplicateChecking = false
        refreshItemStatus(entry)
      }
      activeDuplicateScans = Math.max(0, activeDuplicateScans - 1)
      duplicateScanRunning.value = activeDuplicateScans > 0
    }
  }
}

async function checkItemDuplicate(item: ImportItem, generation = duplicateScanGeneration) {
  if (generation !== duplicateScanGeneration || !items.value.includes(item)) return false
  const signature = duplicateContentSignature(item)
  await scanDuplicates([item], generation)
  return generation === duplicateScanGeneration
    && items.value.includes(item)
    && signature === duplicateContentSignature(item)
    && !item.duplicateNeedsCheck
    && !item.duplicateCheckError
}

function queueReviewItemCheck(item: ImportItem) {
  const generation = duplicateScanGeneration
  reviewItemCheckQueue = reviewItemCheckQueue
    .catch(() => undefined)
    .then(async () => {
      const runningScan = initialDuplicateScan
      if (runningScan) await runningScan
      if (
        generation !== duplicateScanGeneration
        || !componentActive
        || step.value !== 2
        || !items.value.includes(item)
      ) return
      if (itemValidationMessage(item)) {
        refreshItemStatus(item)
        return
      }
      if (!item.duplicateNeedsCheck && !item.duplicateCheckError) {
        refreshItemStatus(item)
        return
      }
      await checkItemDuplicate(item, generation)
    })
    .catch((reason) => {
      if (generation !== duplicateScanGeneration || !items.value.includes(item)) return
      item.duplicateNeedsCheck = true
      item.duplicateCheckError = `自动检查失败：${errorMessage(reason, '无法读取本地题库')}`
      refreshItemStatus(item)
    })
}

function activateReviewItem(item: ImportItem) {
  const previous = activeItem.value
  if (previous?.id === item.id) return
  activeItemId.value = item.id
  if (!previous || !items.value.includes(previous)) return
  refreshItemStatus(previous)
  if (
    !itemValidationMessage(previous)
    && (previous.duplicateNeedsCheck || Boolean(previous.duplicateCheckError))
  ) {
    queueReviewItemCheck(previous)
  }
}

function cancelDuplicateScan() {
  invalidateDuplicateScans('重复题检查已取消，可点击“重试检查”继续。')
  duplicateScanNotice.value = '重复题检查已取消，尚未检查的题目不会被导入。'
}

function retryDuplicateChecks() {
  const pending = items.value.filter((entry) => (
    !itemValidationMessage(entry) && (entry.duplicateNeedsCheck || entry.duplicateCheckError)
  ))
  const scan = scanDuplicates(pending)
  initialDuplicateScan = scan
  void scan.finally(() => {
    if (initialDuplicateScan === scan) initialDuplicateScan = null
  })
}

function invalidateDuplicateScans(unverifiedMessage = '重复题检查已失效，请重新检查。') {
  if (activeDuplicateScans > 0 || activeDuplicateScanIds.size > 0) {
    markDuplicateBatchUnverified(
      items.value.filter((entry) => !itemValidationMessage(entry)),
      unverifiedMessage,
    )
  }
  for (const scanId of activeDuplicateScanIds) {
    void backend.cancelQuestionDuplicateScan(scanId)
  }
  activeDuplicateScanIds.clear()
  duplicateScanGeneration += 1
  activeDuplicateScans = 0
  duplicateScanRunning.value = false
  duplicateScanCompleted.value = 0
  duplicateScanTotal.value = 0
  duplicateScanPhase.value = 'idle'
  duplicateScanIncludesImportBatchPass.value = false
  duplicateScanNotice.value = ''
  initialDuplicateScan = null
}

function markDuplicateCheckNeeded(item: ImportItem) {
  item.duplicateNeedsCheck = true
  item.duplicateCheckError = undefined
  item.duplicateAction = undefined
  refreshItemStatus(item)
}

function addOption(item: ImportItem) {
  item.options.push(rich(''))
  item.optionIds.push(crypto.randomUUID())
  markDuplicateCheckNeeded(item)
}

function removeOption(item: ImportItem, index: number) {
  const removedOptionId = item.optionIds[index]
  item.options.splice(index, 1)
  item.optionIds.splice(index, 1)
  if (removedOptionId) {
    item.resourceRefs = item.resourceRefs.filter((entry) => entry.optionId !== removedOptionId)
  }
  markDuplicateCheckNeeded(item)
}

function updateOption(item: ImportItem, index: number, content: RichContent) {
  item.options[index] = content
  markDuplicateCheckNeeded(item)
}

function updateDuplicateRichField(item: ImportItem, field: 'stem' | 'answer', content: RichContent) {
  item[field] = content
  markDuplicateCheckNeeded(item)
}

function onTypeChanged(item: ImportItem) {
  if (!isChoiceQuestionType(item.type, appStore.questionTypes)) {
    item.options = []
    item.optionIds = []
    item.resourceRefs = item.resourceRefs.filter((entry) => entry.contentSlot !== 'option')
  } else if (item.type === 'true_false') {
    const defaults = appStore.questionTypes.find((definition) => definition.code === item.type)?.defaultOptions
      ?? ['正确', '错误']
    item.options = defaults.map((value) => rich(value))
    item.optionIds = defaults.map(() => crypto.randomUUID())
  } else if (item.options.length < 2) {
    item.options = [rich(''), rich('')]
    item.optionIds = [crypto.randomUUID(), crypto.randomUUID()]
  }
  markDuplicateCheckNeeded(item)
}

function onSubjectChanged(item: ImportItem) {
  const subject = appStore.subjects.find((entry) => entry.id === item.subjectId)
  item.chapterId = subject?.chapters[0]?.id ?? ''
  validateItem(item)
}

async function openDuplicateDialog(item: ImportItem): Promise<DuplicateAction | null> {
  if (duplicateDialogOpen.value || resolveDuplicateAction) {
    ElMessage.info('请先处理当前打开的重复题。')
    return null
  }
  duplicateDialogRequest += 1
  const request = duplicateDialogRequest
  if (duplicateScanRunning.value) {
    ElMessage.info('整批重复题检查尚未完成，请等待进度结束后再选择处理方式。')
    return null
  }
  if (item.duplicateChecking) {
    ElMessage.info('这道题仍在执行重复检查，请稍候。')
    return null
  }
  if (item.duplicateNeedsCheck || item.duplicateCheckError) {
    const checked = await checkItemDuplicate(item)
    if (request !== duplicateDialogRequest || !checked) return null
  }
  if (!item.duplicateCandidate || item.duplicateKind === 'none') return null
  duplicateDialogItemId.value = item.id
  duplicateExistingQuestion.value = null
  applyActionToSameKind.value = true
  duplicateDialogOpen.value = true
  const decision = new Promise<DuplicateAction | null>((resolve) => {
    resolveDuplicateAction = resolve
  })
  const candidateId = item.duplicateCandidate.id
  if (item.duplicateCandidate.sourceKind === 'import-item') return decision
  void backend.getQuestion(candidateId).then((question) => {
    if (
      request === duplicateDialogRequest
      && duplicateDialogOpen.value
      && duplicateDialogItemId.value === item.id
    ) {
      duplicateExistingQuestion.value = question
    }
  }).catch(() => {
    // The compact candidate returned by duplicate detection is enough to make a decision.
  })
  return decision
}

function commitDuplicateAction(action: DuplicateAction) {
  const current = duplicateDialogItem.value
  if (
    !current
    || current.duplicateChecking
    || current.duplicateNeedsCheck
    || current.duplicateCheckError
    || !current.duplicateCandidate
    || current.duplicateKind === 'none'
  ) {
    ElMessage.warning('重复检查结果已变化，请重新检查后再选择处理方式。')
    return
  }
  if (action === 'overwrite' && current.duplicateCandidate.sourceKind === 'import-item') {
    ElMessage.warning('批次内较早的题目尚未写入题库，不能使用“覆盖原题”；请选择跳过或继续保留。')
    return
  }
  if (applyActionToSameKind.value) duplicateActionRules.set(current.duplicateKind, action)
  const affected = wordImportDuplicateActionTargets(
    items.value,
    current,
    action,
    applyActionToSameKind.value,
  )
  for (const item of affected) {
    item.duplicateAction = action
    refreshItemStatus(item)
  }
  const resolve = resolveDuplicateAction
  resolveDuplicateAction = undefined
  duplicateDialogRequest += 1
  duplicateDialogOpen.value = false
  resolve?.(action)
}

function onDuplicateDialogClosed() {
  duplicateDialogRequest += 1
  if (resolveDuplicateAction) {
    const resolve = resolveDuplicateAction
    resolveDuplicateAction = undefined
    resolve(null)
  }
  duplicateDialogItemId.value = null
  duplicateExistingQuestion.value = null
}

function beginImportExecution(total: number) {
  importExecutionPhase.value = 'checking'
  importExecutionCompleted.value = 0
  importExecutionTotal.value = total
  importExecutionNotice.value = ''
  importCancelRequested.value = false
}

function requestImportCancellation() {
  if (!saving.value || importCancelRequested.value) return
  importCancelRequested.value = true
  importExecutionPhase.value = 'stopping'
  importExecutionNotice.value = '不会中断正在提交的数据库事务；当前批次完成后将保留其余题目。'
  if (duplicateScanRunning.value) cancelDuplicateScan()
}

function overwriteOperationKey(entries: ImportItem[]) {
  return JSON.stringify(entries.map((item) => ({
    clientId: item.id,
    targetQuestionId: item.duplicateCandidate?.id ?? null,
    baseContentVersion: item.duplicateCandidate?.contentVersion ?? null,
    semanticSignature: wordImportQuestionSemanticSignature(itemDraft(item)),
  })))
}

async function runIdempotentImportWrite<T>(start: () => Promise<T>, label: string): Promise<T> {
  const firstTimeoutMessage = `${label}超过 ${IMPORT_WRITE_TIMEOUT_MS / 1_000} 秒，正在确认是否已经写入。`
  try {
    return await withTimeout(start(), IMPORT_WRITE_TIMEOUT_MS, firstTimeoutMessage)
  } catch (reason) {
    if (!(reason instanceof Error) || reason.message !== firstTimeoutMessage) throw reason
    importExecutionNotice.value = `${label}响应较慢，正在使用同一批次标识安全确认结果。`
    try {
      const result = await withTimeout(
        start(),
        IMPORT_WRITE_TIMEOUT_MS,
        `${label}在再次确认后仍未返回。`,
      )
      importExecutionNotice.value = ''
      return result
    } catch (retryReason) {
      throw new ImportWriteUncertainError(
        `${label}的最终写入结果暂时无法确认；后续批次已停止，未确认题目已保留。${errorMessage(retryReason, '')}`,
      )
    }
  }
}

async function confirmImport() {
  if (!reviewPersistenceAvailable.value) {
    explainDesktopRequirement()
    return
  }
  const selected = items.value.filter((item) => item.selected)
  if (!selected.length) {
    ElMessage.warning('请至少选择一道题目')
    return
  }
  const previousResult = { ...result.value }
  let batchSuccess = 0
  let batchSkipped = 0
  let writeOutcomeUncertain = false
  let restartDuplicateScanAfterSave = false
  const completedIds = new Set<string>()
  const publishResult = () => {
    result.value = {
      success: previousResult.success + batchSuccess,
      failed: failedImportItemIds.size,
      skipped: previousResult.skipped + batchSkipped,
    }
  }

  beginImportExecution(selected.length)
  saving.value = true
  try {
    if (initialDuplicateScan && !await initialDuplicateScan) {
      ElMessage.warning(importCancelRequested.value
        ? '已停止导入，本次没有写入任何题目。'
        : '重复题检查未完整完成，本次没有写入任何题目；请重试检查后再导入。')
      return
    }
    const validSelected = selected.filter((item) => !itemValidationMessage(item))

    // Editing a question marks its duplicate result stale. Fresh results from
    // the complete review scan can be reused; optimistic versions still protect
    // overwrite targets if another window changes the database meanwhile.
    const requiresDuplicateRecheck = validSelected.some((item) => (
      item.duplicateChecking || item.duplicateNeedsCheck || Boolean(item.duplicateCheckError)
    ))
    const duplicateScanSucceeded = !requiresDuplicateRecheck || await scanDuplicates(
      validSelected, duplicateScanGeneration, 'full',
    )
    if (!duplicateScanSucceeded) {
      ElMessage.warning(importCancelRequested.value
        ? '已停止导入，本次没有写入任何题目。'
        : '重复题检查未完整完成，本次没有写入任何题目；请重试检查后再导入。')
      return
    }
    if (importCancelRequested.value) {
      ElMessage.info('已停止导入，本次没有写入任何题目。')
      return
    }
    importExecutionPhase.value = 'resolving'
    validSelected.forEach(applyRememberedDuplicateAction)

    for (const item of selected) {
      if (itemValidationMessage(item)) {
        failedImportItemIds.add(item.id)
        refreshItemStatus(item)
        importExecutionCompleted.value += 1
        continue
      }
      if (item.duplicateNeedsCheck || item.duplicateCheckError) {
        failedImportItemIds.add(item.id)
        importExecutionCompleted.value += 1
        continue
      }
      if (item.duplicateKind !== 'none' && !item.duplicateAction) {
        const action = await openDuplicateDialog(item)
        if (!action) {
          ElMessage.info('已取消导入，请先完成重复题处理；尚未写入任何题目。')
          return
        }
      }
      if (importCancelRequested.value) {
        ElMessage.info('已停止导入，本次没有写入任何题目。')
        return
      }
    }

    const requestedOverwriteItems: ImportItem[] = []
    const createItems: ImportItem[] = []
    for (const item of selected) {
      if (itemValidationMessage(item) || item.duplicateNeedsCheck || item.duplicateCheckError) continue
      const resolvedAction = currentDuplicateAction(item)
      if (resolvedAction === 'skip') {
        batchSkipped += 1
        completedIds.add(item.id)
        failedImportItemIds.delete(item.id)
        importExecutionCompleted.value += 1
        continue
      }
      if (resolvedAction === 'overwrite') {
        if (
          !item.duplicateCandidate
          || item.duplicateKind === 'none'
          || item.duplicateCandidate.sourceKind === 'import-item'
        ) {
          failedImportItemIds.add(item.id)
          item.status = 'error'
          item.diagnostic = '原重复题候选已变化或尚未写入题库，请重新选择处理方式。'
          importExecutionCompleted.value += 1
          continue
        }
        requestedOverwriteItems.push(item)
      } else {
        createItems.push(item)
      }
    }

    // A source workbook can repeat one logical row many times. Writing all of
    // them against the same optimistic-lock version guarantees conflicts after
    // the first write, so equivalent rows are coalesced before any mutation.
    const overwritePlan = planWordImportOverwrites(requestedOverwriteItems, itemDraft)
    if (overwritePlan.conflicts.length) {
      const conflictItemIds = new Set<string>()
      for (const conflict of overwritePlan.conflicts) {
        for (const item of conflict.items) {
          conflictItemIds.add(item.id)
          failedImportItemIds.add(item.id)
          item.status = 'error'
          item.diagnostic = '源文件中有多行准备覆盖同一道原题，但内容不同。请只勾选要保留的一行后再导入。'
        }
      }
      importExecutionCompleted.value += conflictItemIds.size
      batchSkipped = 0
      completedIds.clear()
      result.value = { ...previousResult, failed: failedImportItemIds.size }
      ElMessage.error(`发现 ${overwritePlan.conflicts.length} 组“同一原题、不同内容”的覆盖冲突；为避免误覆盖，本次没有写入。`)
      return
    }

    for (const duplicate of overwritePlan.duplicates) {
      batchSkipped += 1
      completedIds.add(duplicate.item.id)
      failedImportItemIds.delete(duplicate.item.id)
      importExecutionCompleted.value += 1
    }
    publishResult()

    for (const chunk of chunked(overwritePlan.representatives, IMPORT_SAVE_BATCH_SIZE)) {
      if (importCancelRequested.value) break
      importExecutionPhase.value = 'overwriting'
      const operationKey = overwriteOperationKey(chunk)
      const operationId = overwriteOperationIds.get(operationKey) ?? crypto.randomUUID()
      overwriteOperationIds.set(operationKey, operationId)
      const request = {
        operationId,
        items: chunk.map((item) => {
          const draft = itemDraft(item)
          draft.questionId = item.duplicateCandidate!.id
          draft.baseContentVersion = item.duplicateCandidate!.contentVersion
          return { clientId: item.id, draft }
        }),
      }
      try {
        const saved = await runIdempotentImportWrite(
          () => backend.saveImportedQuestionOverwrites(request),
          `批量覆盖 ${chunk.length} 道题`,
        )
        const expected = new Map(chunk.map((item) => [item.id, item.duplicateCandidate!.id]))
        const returned = new Map(saved.items.map((item) => [item.clientId, item.questionId]))
        if (saved.operationId !== operationId
          || saved.updatedCount !== chunk.length
          || saved.items.length !== chunk.length
          || returned.size !== expected.size
          || [...expected].some(([clientId, questionId]) => returned.get(clientId) !== questionId)) {
          throw new Error('批量覆盖返回的题目标识与本批请求不一致，请保留草稿并重试。')
        }
        overwriteOperationIds.delete(operationKey)
        for (const item of chunk) {
          batchSuccess += 1
          completedIds.add(item.id)
          failedImportItemIds.delete(item.id)
        }
      } catch (reason) {
        const message = errorMessage(reason, '整批覆盖原题失败，本批事务未部分写入。')
        for (const item of chunk) {
          failedImportItemIds.add(item.id)
          item.status = 'error'
          item.diagnostic = message
        }
        if (reason instanceof ImportWriteUncertainError) {
          writeOutcomeUncertain = true
          importCancelRequested.value = true
          importExecutionPhase.value = 'stopping'
        } else {
          overwriteOperationIds.delete(operationKey)
        }
      } finally {
        importExecutionCompleted.value += chunk.length
        publishResult()
      }
    }

    // New questions and explicitly kept duplicates are committed with their
    // stable import IDs. A retry after an uncertain response is therefore
    // idempotent and cannot create a second copy.
    for (const chunk of chunked(createItems, IMPORT_SAVE_BATCH_SIZE)) {
      if (importCancelRequested.value) break
      importExecutionPhase.value = 'creating'
      try {
        const drafts = chunk.map(itemDraft)
        const savedIds = await runIdempotentImportWrite(
          () => backend.saveImportedQuestions(drafts),
          `批量写入 ${chunk.length} 道新题`,
        )
        const expectedIds = new Set(chunk.map((item) => item.id))
        const returnedIds = new Set(savedIds)
        if (
          savedIds.length !== expectedIds.size
          || returnedIds.size !== expectedIds.size
          || [...returnedIds].some((id) => !expectedIds.has(id))
        ) {
          throw new Error('批量保存返回的题目标识与本批请求不一致，请重试导入。')
        }
        for (const item of chunk) {
          batchSuccess += 1
          completedIds.add(item.id)
          failedImportItemIds.delete(item.id)
        }
      } catch (reason) {
        const message = errorMessage(reason, '整批写入本地题库失败，未自动重试以避免重复保存。')
        for (const item of chunk) {
          failedImportItemIds.add(item.id)
          item.status = 'error'
          item.diagnostic = message
        }
        if (reason instanceof ImportWriteUncertainError) {
          writeOutcomeUncertain = true
          importCancelRequested.value = true
          importExecutionPhase.value = 'stopping'
        }
      } finally {
        importExecutionCompleted.value += chunk.length
        publishResult()
      }
    }

    if (batchSuccess > 0) {
      importExecutionPhase.value = 'refreshing'
      const [bankRefresh, taxonomyRefresh] = await Promise.allSettled([
        withTimeout(
          bankStore.load(),
          IMPORT_REFRESH_TIMEOUT_MS,
          '题库列表刷新超时；题目已经写入，重新进入题库页即可刷新。',
        ),
        withTimeout(
          appStore.refreshTaxonomy(),
          IMPORT_REFRESH_TIMEOUT_MS,
          '学科和章节计数刷新超时；题目已经写入，进入题库页时会再次刷新。',
        ),
      ])
      if (bankRefresh.status === 'rejected') {
        ElMessage.warning(errorMessage(bankRefresh.reason, '题目已经写入，但题库列表暂时无法刷新。'))
      }
      if (taxonomyRefresh.status === 'rejected') {
        ElMessage.warning(errorMessage(taxonomyRefresh.reason, '题目已经写入，但学科和章节计数暂时无法刷新。'))
      } else if (!taxonomyRefresh.value) {
        ElMessage.warning('题目已经写入，但学科和章节计数暂时无法刷新；进入题库页时会再次刷新。')
      }
    }

    publishResult()
    if (batchSuccess > 0 || writeOutcomeUncertain) {
      for (const item of items.value) {
        if (!completedIds.has(item.id) && !itemValidationMessage(item)) markDuplicateCheckNeeded(item)
      }
    }
    importExecutionPhase.value = 'preserving'
    const draftPreserved = await preserveUnfinishedItems(completedIds)
    if (!draftPreserved) {
      ElMessage.warning('已完成本批可写入题目的导入，但剩余题目草稿尚未保存，请留在当前页面后重试。')
      return
    }
    if (items.value.length) {
      restartDuplicateScanAfterSave = batchSuccess > 0 && !writeOutcomeUncertain
      if (importCancelRequested.value) {
        ElMessage.info(`已停止后续批次；本批成功 ${batchSuccess} 道、跳过 ${batchSkipped} 道，剩余 ${items.value.length} 道已保留。`)
      } else if (batchSuccess + batchSkipped === 0) {
        ElMessage.warning(`本批没有题目写入或跳过；剩余 ${items.value.length} 道已保留，请根据错误提示修复后重试。`)
      } else {
        ElMessage.success(`本批成功 ${batchSuccess} 道、跳过 ${batchSkipped} 道；剩余 ${items.value.length} 道继续审查。`)
      }
      return
    }
    step.value = 3
  } catch (reason) {
    ElMessage.error(errorMessage(reason, 'Word 导入过程中发生错误，未完成的题目仍保留在审查页。'))
  } finally {
    saving.value = false
    importExecutionPhase.value = 'idle'
    importCancelRequested.value = false
    if (restartDuplicateScanAfterSave && componentActive && step.value === 2 && items.value.length) {
      startReviewDuplicateScan()
    }
  }
}

async function preserveUnfinishedItems(completedIds: ReadonlySet<string>) {
  const remaining = pendingWordImportItems(items.value, completedIds)
  if (!remaining.length) {
    try {
      await deletePersistedWordImportDraft()
    } catch (reason) {
      draftDirty.value = true
      ElMessage.warning(errorMessage(reason, '题目已处理，但旧审查草稿暂时无法删除。请留在本页并使用“重新选择”重试清理。'))
      return false
    }
    draftReady.value = false
    items.value = []
    activeItemId.value = null
    draftDirty.value = false
    return true
  }

  draftReady.value = false
  items.value = remaining
  activeItemId.value = remaining.some((item) => item.id === activeItemId.value)
    ? activeItemId.value
    : remaining[0]?.id ?? null
  draftReady.value = true
  draftDirty.value = true
  return await autosaveWordImportDraft(true, true)
}

async function skipItem(item: ImportItem) {
  if (saving.value || draftAutosaving.value) return
  try {
    await ElMessageBox.confirm(
      `确定不导入第 ${item.ordinal} 题吗？该题会从本次待处理列表和恢复草稿中移除。`,
      '跳过这道题',
      {
        type: 'warning',
        confirmButtonText: '确认跳过',
        cancelButtonText: '继续保留',
      },
    )
  } catch {
    return
  }

  saving.value = true
  try {
    failedImportItemIds.delete(item.id)
    result.value = {
      ...result.value,
      failed: failedImportItemIds.size,
      skipped: result.value.skipped + 1,
    }
    const draftPreserved = await preserveUnfinishedItems(new Set([item.id]))
    if (!draftPreserved) {
      ElMessage.warning('题目已从当前列表移除，但草稿尚未保存，请留在本页并重试保存。')
      return
    }
    if (!items.value.length) {
      step.value = 3
      return
    }
    ElMessage.success(`已跳过第 ${item.ordinal} 题，剩余 ${items.value.length} 道继续审查。`)
  } finally {
    saving.value = false
  }
}

function resetState() {
  recognitionRequest += 1
  invalidateDuplicateScans()
  clearInterval(timer)
  clearTimeout(prepareReviewTimer)
  timer = undefined
  prepareReviewTimer = undefined
  duplicateDialogRequest += 1
  resolveDuplicateAction?.(null)
  resolveDuplicateAction = undefined
  duplicateDialogOpen.value = false
  duplicateDialogItemId.value = null
  duplicateExistingQuestion.value = null
  duplicateActionRules.clear()
  selectedFile.value = null
  excelSheetNames.value = []
  selectedExcelSheet.value = ''
  excelInspectionRunning.value = false
  progress.value = 0
  parseLogs.value = []
  analysis.value = null
  recognitionError.value = ''
  importLimitNotice.value = ''
  sourceRecognizedItemCount.value = 0
  omittedItemCount.value = 0
  importSessionId.value = null
  pendingImageOccurrences.value = []
  pendingFormulaOccurrences.value = []
  pendingTableOccurrences.value = []
  parserVersion.value = CURRENT_WORD_IMPORT_PARSER_VERSION
  items.value = []
  activeItemId.value = null
  result.value = { success: 0, failed: 0, skipped: 0 }
  sessionTotalCount.value = 0
  batchSubjectId.value = ''
  batchChapterId.value = ''
  failedImportItemIds.clear()
  overwriteOperationIds.clear()
  importExecutionPhase.value = 'idle'
  importExecutionCompleted.value = 0
  importExecutionTotal.value = 0
  importExecutionNotice.value = ''
  importCancelRequested.value = false
  unknownTypeDialogOpen.value = false
  unknownTypeDecisions.value = []
  unknownTagDialogOpen.value = false
  unknownTagDecisions.value = []
  draftReady.value = false
  draftDirty.value = false
  lastSavedReviewSnapshot = null
  lastDraftAutosavedAt.value = null
  step.value = 0
}

async function reset() {
  if (saving.value || draftAutosaving.value) {
    ElMessage.info('正在保存数据，请稍候再重新选择文件。')
    return
  }

  const hasReview = (step.value === 2 && items.value.length > 0) || hasPersistedDraft.value
  if (hasReview) {
    try {
      await ElMessageBox.confirm(
        '重新选择文件会删除当前批量导入审查草稿，确定继续吗？',
        '删除当前草稿',
        {
          type: 'warning',
          confirmButtonText: '删除并重新选择',
          cancelButtonText: '继续审查',
        },
      )
    } catch {
      return
    }
  }

  if (hasPersistedDraft.value) {
    try {
      await deletePersistedWordImportDraft()
    } catch (reason) {
      ElMessage.error(errorMessage(reason, '本地草稿删除失败，已保留当前页面以避免覆盖数据。'))
      return
    }
  }
  resetState()
}
</script>

<template>
  <section class="page-main word-page">
      <el-alert
        v-if="desktopAvailable && !appStore.license.capabilities.canBatchImport"
        class="browser-runtime-note"
        type="info"
        :closable="false"
        show-icon
        title="当前为基础桌面模式：Word、Excel 与文档式批量导入已关闭，单题录入仍可正常使用。"
      />
      <el-alert
        v-if="!desktopAvailable && !isDocumentEntryReview"
        class="browser-runtime-note"
        type="warning"
        :closable="false"
        show-icon
        title="浏览器界面演示不会读取 Word 或 Excel 文件，也不会把识别结果写入真实题库。请安装并打开 Windows 桌面版后执行导入。"
      />

      <div class="import-progress">
        <el-steps :active="step" align-center finish-status="success" class="import-steps">
          <el-step title="选择文件" />
          <el-step title="自动识别" />
          <el-step title="审查分类" />
          <el-step title="导入完成" />
        </el-steps>
        <el-button v-if="step > 0 && step < 3" :disabled="saving || draftAutosaving" @click="reset">重新选择</el-button>
      </div>

      <div v-if="step === 0" class="select-layout">
        <div
          class="upload-zone surface"
          :class="{ 'is-dragging': dragActive, 'is-browser-disabled': !desktopAvailable }"
          @dragover.prevent="desktopAvailable && (dragActive = true)"
          @dragleave.prevent="dragActive = false"
          @drop.prevent="onDrop"
        >
          <div class="upload-icon"><el-icon><UploadFilled /></el-icon></div>
          <template v-if="!desktopAvailable">
            <h2>桌面版才能选择批量导入文件</h2>
            <p>此处保留导入页面供浏览；拖放和文件选择均已关闭，不会模拟识别成功。</p>
            <button type="button" class="file-button" disabled>仅 Windows 桌面版可用</button>
          </template>
          <template v-else-if="!appStore.license.capabilities.canBatchImport">
            <h2>桌面专业版可使用 Word 与 Excel 批量导入</h2>
            <p>当前基础桌面模式仍可使用单题录入；已有导入草稿不会被删除。</p>
            <button type="button" class="file-button" disabled>批量导入未授权</button>
          </template>
          <template v-else-if="recoveryChecking">
            <h2>正在检查未完成草稿…</h2>
            <p>检查完成后即可选择新的 .docx 或 .xlsx 文件。</p>
          </template>
          <template v-else-if="draftReadBlocked">
            <h2>旧草稿暂时无法读取</h2>
            <p>{{ draftReadError }} 为避免覆盖，新的批量导入已暂停。</p>
            <div class="draft-blocked-actions">
              <el-button @click="retryWordImportDraftRecovery">重试读取</el-button>
              <el-button type="danger" plain @click="discardUnreadableWordImportDraft">明确删除旧草稿</el-button>
            </div>
          </template>
          <template v-else-if="!selectedFile">
            <h2>选择批量导入文件</h2>
            <p>支持 Word `.docx` 和 Excel `.xlsx`；最大 100 MB，所有内容只在本机解析</p>
            <div class="file-actions">
              <button type="button" class="file-button" :disabled="recoveryChecking" @click="chooseNativeFile('docx')">选择 Word 文件</button>
              <button type="button" class="file-button file-button--excel" :disabled="recoveryChecking || excelInspectionRunning" @click="chooseNativeFile('xlsx')">
                {{ excelInspectionRunning ? '正在检查…' : '选择 Excel 文件' }}
              </button>
            </div>
            <el-button text type="primary" :loading="templateCreating" @click="downloadExcelTemplate">下载 Excel 导入模板</el-button>
          </template>
          <template v-else>
            <h2>{{ selectedFile.name }}</h2>
            <p>{{ selectedFile.size ? `${(selectedFile.size / 1024).toFixed(1)} KB` : '等待安全检查' }} · .{{ selectedFile.kind }}</p>
            <div v-if="selectedFile.kind === 'xlsx' && excelSheetNames.length > 1" class="sheet-selector">
              <span>导入工作表</span>
              <el-select v-model="selectedExcelSheet" aria-label="选择 Excel 工作表">
                <el-option v-for="sheet in excelSheetNames" :key="sheet" :label="sheet" :value="sheet" />
              </el-select>
            </div>
            <el-button type="primary" @click="startRecognition">开始离线识别</el-button>
          </template>
        </div>
        <div class="import-guide surface">
          <h3>导入说明</h3>
          <ol>
            <li>自动识别题型、题干、选项、答案和解析</li>
            <li>Word 可保留受支持的图片、表格和公式；Excel 首版读取文字单元格</li>
            <li>逐题检查异常、分类和重复内容</li>
            <li>只导入校验通过的题目，失败项保留草稿</li>
          </ol>
          <div class="warning-banner">旧版 .doc/.xls 或 .xlsm 请先使用 Word、Excel 或 WPS 另存为 .docx/.xlsx。</div>
        </div>
      </div>

      <div v-else-if="step === 1" class="recognition-card surface">
        <div class="document-icon"><el-icon><Document /></el-icon></div>
        <h2>{{ selectedFile?.name }}</h2>
        <p>正在本地解析{{ sourceFormatLabel }}文件，不会上传任何内容。</p>
        <el-progress :percentage="progress" :stroke-width="9" />
        <div v-if="recognitionError" class="warning-banner recognition-error">
          <strong>文件未通过检查</strong>
          <span>{{ recognitionError }}</span>
          <el-button size="small" @click="reset">重新选择文件</el-button>
        </div>
        <div class="parse-log">
          <div v-for="line in parseLogs" :key="line"><span class="status-dot" />{{ line }}</div>
        </div>
      </div>

      <div
        v-else-if="step === 2"
        class="review-layout"
        :class="{ 'is-locked': saving }"
        :inert="saving"
        :aria-busy="saving"
      >
        <div class="review-list surface">
          <div class="review-list__header">
            <div class="review-list__title">
              <strong>待处理 {{ items.length }} 道题</strong>
              <span>
                {{ duplicateScanRunning
                  ? duplicateScanProgressLabel
                  : `${warningCount} 项需要处理` }}
              </span>
            </div>
            <div class="review-list__selection">
              <el-button v-if="duplicateScanRunning" text size="small" @click="cancelDuplicateScan">取消检查</el-button>
              <el-button v-else-if="duplicateRetryCount" text size="small" @click="retryDuplicateChecks">
                重试检查
              </el-button>
              <el-checkbox v-model="allItemsSelected" :indeterminate="selectionIndeterminate">全选</el-checkbox>
              <el-button text size="small" :disabled="selectedCount === 0" @click="selectNone">全不选</el-button>
            </div>
          </div>
          <div v-if="duplicateScanRunning || duplicateScanNotice" class="duplicate-scan-progress">
            <el-progress
              v-if="duplicateScanRunning"
              :percentage="duplicateScanPercentage"
              :show-text="false"
              :stroke-width="4"
            />
            <small v-if="duplicateScanNotice">{{ duplicateScanNotice }}</small>
          </div>
          <div class="batch-classification">
            <div class="batch-classification__heading">
              <strong>批量分类</strong>
              <span>已选 {{ selectedCount }} 道</span>
            </div>
            <div class="batch-classification__fields">
              <el-select v-model="batchSubjectId" size="small" placeholder="选择学科" @change="onBatchSubjectChanged">
                <el-option v-for="subject in appStore.subjects" :key="subject.id" :label="subject.name" :value="subject.id" />
              </el-select>
              <el-select v-model="batchChapterId" size="small" placeholder="选择章节">
                <el-option v-for="chapter in batchChapterOptions" :key="chapter.id" :label="chapter.name" :value="chapter.id" />
              </el-select>
            </div>
            <el-button
              class="batch-classification__apply"
              size="small"
              :disabled="selectedCount === 0 || !batchSubjectId || !batchChapterId"
              @click="applyBatchClassification"
            >应用到已选 {{ selectedCount }} 题</el-button>
          </div>
          <div v-if="importLimitNotice" class="warning-banner import-limit-notice">{{ importLimitNotice }}</div>
          <button
            v-for="item in items"
            :key="item.id"
            class="review-item"
            :class="[`is-${item.status}`, { 'is-active': activeItemId === item.id }]"
            @click="activateReviewItem(item)"
            @contextmenu.stop="openItemContextMenu($event, item)"
          >
            <el-checkbox v-model="item.selected" @click.stop />
            <span class="review-item__ordinal">{{ item.sourceRowNumber ? `Excel 第 ${item.sourceRowNumber} 行` : `第 ${item.ordinal} 题` }}</span>
            <div class="review-item__content">
              <strong>{{ questionTypeLabel(item.type, appStore.questionTypes) }}</strong>
              <RichContentView
                v-if="item.stem.plainText.trim()"
                class="review-item__preview"
                :content="item.stem"
              />
              <span v-else class="review-item__empty">未识别到题干</span>
            </div>
            <el-icon v-if="item.status === 'ok'" class="text-success"><CircleCheck /></el-icon>
            <el-icon v-else-if="item.status === 'warning'" class="review-item__warning-icon"><Warning /></el-icon>
            <el-icon v-else class="text-danger"><Warning /></el-icon>
          </button>
        </div>

        <div v-if="activeItem" class="review-editor surface">
          <div v-if="activeItem.diagnostic" :class="activeItem.status === 'error' ? 'warning-banner' : 'info-banner'">
            {{ activeItem.diagnostic }}
          </div>
          <div class="review-meta">
            <el-form-item label="题型" required>
              <el-select v-model="activeItem.type" @change="onTypeChanged(activeItem)">
                <el-option v-for="type in typeOptions" :key="type.code" :label="type.name" :value="type.code" />
              </el-select>
            </el-form-item>
            <el-form-item label="学科" required>
              <el-select v-model="activeItem.subjectId" @change="onSubjectChanged(activeItem)">
                <el-option v-for="subject in appStore.subjects" :key="subject.id" :label="subject.name" :value="subject.id" />
              </el-select>
            </el-form-item>
            <el-form-item label="章节" required>
              <el-select v-model="activeItem.chapterId" @change="validateItem(activeItem)">
                <el-option v-for="chapter in chapterOptions" :key="chapter.id" :label="chapter.name" :value="chapter.id" />
              </el-select>
            </el-form-item>
            <el-form-item label="标签">
              <el-select v-model="activeItem.tagIds" multiple filterable collapse-tags placeholder="可选">
                <el-option v-for="tag in appStore.tags" :key="tag.id" :label="tag.name" :value="tag.id" />
              </el-select>
            </el-form-item>
          </div>
          <div class="review-field">
            <label>题干</label>
            <RichTextEditor
              :model-value="activeItem.stem"
              :min-height="100"
              toolbar-mode="hidden"
              context-tools
              @update:model-value="updateDuplicateRichField(activeItem, 'stem', $event)"
            />
          </div>
          <div v-if="isChoiceQuestionType(activeItem.type, appStore.questionTypes)" class="review-field">
            <div class="review-field__heading">
              <label>选项</label>
              <el-button v-if="activeItem.type !== 'true_false'" size="small" @click="addOption(activeItem)">添加选项</el-button>
            </div>
            <div v-for="(option, optionIndex) in activeItem.options" :key="optionIndex" class="review-option">
              <span>{{ String.fromCharCode(65 + optionIndex) }}</span>
              <RichTextEditor
                :model-value="option"
                :min-height="48"
                toolbar-mode="hidden"
                context-tools
                @update:model-value="updateOption(activeItem, optionIndex, $event)"
              />
              <el-button v-if="activeItem.type !== 'true_false'" text type="danger" :disabled="activeItem.options.length <= 2" @click="removeOption(activeItem, optionIndex)">删除</el-button>
            </div>
          </div>
          <div class="review-field">
            <label>答案</label>
            <RichTextEditor
              :model-value="activeItem.answer"
              :min-height="70"
              toolbar-mode="hidden"
              context-tools
              @update:model-value="updateDuplicateRichField(activeItem, 'answer', $event)"
            />
          </div>
          <div class="review-field">
            <label>解析</label>
            <RichTextEditor
              v-model="activeItem.explanation"
              :min-height="70"
              toolbar-mode="hidden"
              context-tools
            />
          </div>
          <div v-if="activeItem.duplicateKind !== 'none'" class="duplicate-row">
            <span>
              检测到{{ activeItem.duplicateKind === 'exact' ? '完全重复' : '疑似重复' }}题目
              <b v-if="activeItem.duplicateCandidate"> · {{ activeItem.duplicateCandidate.similarityPercent }}%</b>
            </span>
            <el-button
              size="small"
              :disabled="!canOpenWordImportDuplicateDialog(activeItem, duplicateScanRunning)"
              @click="openDuplicateDialog(activeItem)"
            >
              {{ activeItem.duplicateAction ? `已选择：${duplicateActionLabels[activeItem.duplicateAction]}` : '选择处理方式' }}
            </el-button>
          </div>
          <el-button class="remove-item" text type="danger" :icon="Delete" @click="skipItem(activeItem)">明确跳过并移出这道题</el-button>
        </div>
      </div>

      <div v-else class="result-card surface">
        <div class="result-icon"><el-icon><CircleCheck /></el-icon></div>
        <h2>导入任务已完成</h2>
        <p>本次待处理题目已全部写入题库或由你明确跳过。</p>
        <div class="result-metrics">
          <div><strong>{{ result.success }}</strong><span>成功</span></div>
          <div><strong>{{ result.skipped }}</strong><span>主动跳过</span></div>
          <div><strong class="text-danger">{{ result.failed }}</strong><span>失败</span></div>
        </div>
        <div class="result-actions">
          <el-button @click="reset">继续导入</el-button>
          <el-button type="primary" @click="$router.push('/questions')">查看题库</el-button>
        </div>
      </div>

      <footer v-if="step === 2" class="review-footer">
        <div class="review-footer__summary">
          <span v-if="saving">
            本批执行 {{ importExecutionCompleted }} / {{ importExecutionTotal }} · 已完成项会从恢复草稿中移除
          </span>
          <span v-else>已处理 {{ sessionProcessedCount }} / {{ sessionTotalCount }} · 待处理 {{ items.length }} · 本批已选 {{ selectedCount }}</span>
          <div v-if="saving" class="review-footer__execution" aria-live="polite">
            <el-progress :percentage="importExecutionPercentage" :show-text="false" :stroke-width="4" />
            <small>{{ importExecutionLabel }}</small>
            <small v-if="importExecutionNotice">{{ importExecutionNotice }}</small>
          </div>
          <small>本次累计：成功 {{ result.success }}，跳过 {{ result.skipped }}，失败待修复 {{ result.failed }}</small>
          <small v-if="!desktopAvailable && isDocumentEntryReview" class="browser-demo-note">浏览器演示：导入结果仅保存在当前浏览器中，不会写入桌面版题库。</small>
          <small v-if="draftAutosaving">正在保存本地草稿…</small>
          <small v-else-if="lastDraftAutosavedAt">草稿已于 {{ new Date(lastDraftAutosavedAt).toLocaleTimeString('zh-CN') }} 保存</small>
          <small v-else>每 60 秒自动保存到本地数据库</small>
        </div>
        <div>
          <el-button :loading="draftAutosaving" :disabled="saving || !reviewPersistenceAvailable" @click="saveDraftManually">保存草稿</el-button>
          <el-button
            v-if="importCanCancel"
            type="danger"
            plain
            :disabled="importCancelRequested"
            @click="requestImportCancellation"
          >{{ importCancelRequested ? '正在停止' : '停止后续批次' }}</el-button>
          <el-button
            type="primary"
            :loading="saving"
            :disabled="saving || selectedCount === 0 || duplicateScanRunning || draftAutosaving || !reviewPersistenceAvailable || !appStore.license.capabilities.canBatchImport"
            @click="confirmImport"
          >{{ appStore.license.capabilities.canBatchImport ? `导入本批 ${selectedCount} 题` : '专业版可批量导入' }}</el-button>
        </div>
      </footer>
  </section>

  <el-dialog
    v-model="unknownTypeDialogOpen"
    title="发现尚未配置的题型"
    width="820px"
    :show-close="false"
    :close-on-click-modal="false"
    :close-on-press-escape="false"
  >
      <el-alert
        type="warning"
        :closable="false"
        show-icon
        title="来源文件中的题型名称没有对应题型。请确认是创建新题型，还是映射到已有题型。软件不会擅自添加。"
      />
      <div class="unknown-types-table">
        <div v-for="decision in unknownTypeDecisions" :key="decision.name" class="unknown-type-row">
          <div><strong>{{ decision.name }}</strong><small>识别到 {{ decision.count }} 道题</small></div>
          <el-radio-group v-model="decision.action">
            <el-radio-button value="create">添加为新题型</el-radio-button>
            <el-radio-button value="map">映射到已有题型</el-radio-button>
          </el-radio-group>
          <el-select v-if="decision.action === 'create'" v-model="decision.behavior" aria-label="基础答题结构">
            <el-option label="单选结构" value="single_choice" />
            <el-option label="多选结构" value="multiple_choice" />
            <el-option label="填写答案结构" value="fill_blank" />
            <el-option label="开放作答结构" value="open_response" />
          </el-select>
          <el-select v-else v-model="decision.mapTo" filterable aria-label="映射到已有题型">
            <el-option v-for="type in typeOptions" :key="type.code" :label="type.name" :value="type.code" />
          </el-select>
          <el-input
            v-if="decision.action === 'create' && matchesChoiceBehavior(decision.behavior)"
            v-model="decision.defaultOptionsText"
            class="unknown-type-defaults"
            type="textarea"
            :rows="2"
            placeholder="每行一个默认选项；例如：正确、错误"
          />
        </div>
      </div>
      <template #footer>
        <el-button type="primary" :loading="resolvingUnknownTypes" @click="confirmUnknownTypes">
          确认并进入题目审查
        </el-button>
      </template>
  </el-dialog>

  <el-dialog
    v-model="unknownTagDialogOpen"
    title="发现尚未配置的标签"
    width="680px"
    :show-close="false"
    :close-on-click-modal="false"
    :close-on-press-escape="false"
  >
      <el-alert
        type="info"
        :closable="false"
        show-icon
        title="Excel 中的部分标签尚未存在于题库。勾选后将创建并关联；取消勾选则忽略该标签。"
      />
      <div class="unknown-tags-table">
        <el-checkbox
          v-for="decision in unknownTagDecisions"
          :key="decision.name"
          v-model="decision.create"
          border
        >
          {{ decision.name }}（{{ decision.count }} 道题）
        </el-checkbox>
      </div>
      <template #footer>
        <el-button type="primary" :loading="creatingUnknownTags" @click="confirmUnknownTags">
          确认并进入题目审查
        </el-button>
      </template>
  </el-dialog>

  <el-dialog
    v-model="duplicateDialogOpen"
    :title="duplicateDialogItem?.duplicateKind === 'exact' ? '发现完全重复题目' : '发现疑似重复题目'"
    width="760px"
    :close-on-click-modal="false"
    @closed="onDuplicateDialogClosed"
  >
      <template v-if="duplicateDialogItem?.duplicateCandidate">
        <div class="duplicate-warning">
          <el-icon><Warning /></el-icon>
          系统根据题干、选项和答案判断为
          <b>{{ duplicateDialogItem.duplicateKind === 'exact' ? '完全重复' : '疑似重复' }}</b>，请决定如何处理。
        </div>
        <div class="duplicate-compare">
          <article>
            <header><strong>正在保存的题目</strong><el-tag size="small">新题</el-tag></header>
            <QuestionStemSummary
              class="duplicate-compare__stem"
              :content="duplicateDialogItem.stem"
              :lines="3"
            />
            <small>答案：{{ duplicateDialogItem.answer.plainText }}</small>
          </article>
          <article>
            <header>
              <strong>{{ duplicateCandidateIsImportItem ? '本批中较早的题目' : '题库中的相似题' }}</strong>
              <el-tag size="small" type="warning">
                {{ duplicateDialogItem.duplicateKind === 'exact' ? '完全重复' : `相似度 ${duplicateDialogItem.duplicateCandidate.similarityPercent}%` }}
              </el-tag>
            </header>
            <QuestionStemSummary
              class="duplicate-compare__stem"
              :content="duplicateExistingQuestion?.stem"
              :text="duplicateDialogItem.duplicateCandidate.stemPreview"
              :lines="3"
            />
            <small v-if="duplicateExistingQuestion">答案：{{ duplicateExistingQuestion.answer.plainText }}</small>
            <small v-else>
              {{ questionTypeLabel(duplicateDialogItem.duplicateCandidate.type, appStore.questionTypes) }} ·
              {{ duplicateDialogItem.duplicateCandidate.subjectName }} / {{ duplicateDialogItem.duplicateCandidate.chapterName }}
            </small>
          </article>
        </div>
        <el-checkbox v-model="applyActionToSameKind" class="duplicate-apply-all">
          对本次批量导入中同为“{{ duplicateDialogItem.duplicateKind === 'exact' ? '完全重复' : '疑似重复' }}”的题目应用相同处理
        </el-checkbox>
      </template>
      <template #footer>
        <el-button @click="commitDuplicateAction('skip')">跳过此题</el-button>
        <el-button v-if="!duplicateCandidateIsImportItem" @click="commitDuplicateAction('overwrite')">覆盖原题</el-button>
        <el-button type="primary" @click="commitDuplicateAction('keep')">继续保留</el-button>
      </template>
  </el-dialog>

  <AppContextMenu
    v-model="contextMenuOpen"
    :x="contextMenuX"
    :y="contextMenuY"
    :title="contextMenuTitle"
    :items="contextMenuItems"
    @select="handleItemContextAction"
  />
</template>

<style scoped>
.word-page {
  position: relative;
  display: flex;
  flex-direction: column;
}

.browser-runtime-note {
  margin-bottom: 14px;
}

.unknown-types-table { display: grid; gap: 12px; margin-top: 16px; }
.unknown-type-row { display: grid; grid-template-columns: minmax(130px, 1fr) auto 220px; align-items: center; gap: 14px; padding: 14px; border: 1px solid #dbe3ed; border-radius: 8px; }
.unknown-type-row > div:first-child { display: grid; gap: 4px; }
.unknown-type-defaults { grid-column: 3; }
.unknown-type-row small { color: #64748b; }
.unknown-tags-table { margin-top: 16px; display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 10px; }
.unknown-tags-table :deep(.el-checkbox) { width: 100%; margin: 0; }

.import-progress {
  margin: 0 0 18px;
  padding: 14px 20px;
  display: flex;
  align-items: center;
  gap: 16px;
  border: 1px solid #e5eaf2;
  border-radius: 9px;
  background: #fff;
}

.import-steps {
  min-width: 0;
  flex: 1;
}

.select-layout {
  min-height: 0;
  flex: 1;
  display: grid;
  grid-template-columns: minmax(500px, 1fr) 310px;
  gap: 16px;
}

.upload-zone {
  min-height: 440px;
  padding: 50px;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  border: 2px dashed #cbd5e1;
  text-align: center;
}

.upload-zone.is-dragging {
  border-color: #3b82f6;
  background: #f5f9ff;
}

.upload-zone.is-browser-disabled {
  border-color: #f3c969;
  background: #fffbeb;
}

.upload-icon,
.document-icon {
  width: 64px;
  height: 64px;
  display: grid;
  place-items: center;
  border-radius: 18px;
  background: #eff6ff;
  color: #2563eb;
  font-size: 30px;
}

.upload-zone h2,
.recognition-card h2,
.result-card h2 {
  margin: 18px 0 7px;
  font-size: 18px;
}

.upload-zone p,
.recognition-card p,
.result-card p {
  margin: 0 0 18px;
  color: #64748b;
  font-size: 12px;
}

.file-button {
  padding: 9px 15px;
  border: 0;
  border-radius: 6px;
  background: #2563eb;
  color: white;
  cursor: pointer;
  font-size: 12px;
}

.file-actions {
  display: flex;
  flex-wrap: wrap;
  justify-content: center;
  gap: 10px;
}

.file-button--excel {
  background: #15803d;
}

.file-button:disabled {
  cursor: not-allowed;
  opacity: .55;
}

.sheet-selector {
  width: min(420px, 100%);
  margin: 0 0 18px;
  display: grid;
  grid-template-columns: auto minmax(0, 1fr);
  align-items: center;
  gap: 10px;
  color: #475569;
  font-size: 12px;
}

.sheet-selector :deep(.el-select) {
  width: 100%;
}

.file-button input {
  display: none;
}

.draft-blocked-actions {
  display: flex;
  gap: 8px;
}

.import-guide {
  padding: 22px;
}

.import-guide h3 {
  margin: 0 0 16px;
  font-size: 15px;
}

.import-guide ol {
  margin: 0 0 18px;
  padding-left: 20px;
  color: #475569;
  font-size: 11px;
  line-height: 2.2;
}

.recognition-card {
  width: min(760px, 90%);
  margin: 20px auto;
  padding: 56px 70px;
  text-align: center;
}

.recognition-card .document-icon {
  margin: 0 auto;
}

.parse-log {
  margin-top: 24px;
  padding: 15px;
  display: grid;
  gap: 9px;
  border-radius: 8px;
  background: #f8fafc;
  color: #64748b;
  font-size: 11px;
  text-align: left;
}

.recognition-error {
  margin-top: 18px;
  display: grid;
  justify-items: center;
  gap: 8px;
}

.recognition-error span {
  line-height: 1.6;
}

.parse-log div {
  display: flex;
  align-items: center;
  gap: 9px;
}

.review-layout {
  min-height: 0;
  flex: 1;
  padding-bottom: 82px;
  display: grid;
  grid-template-columns: 330px minmax(480px, 1fr);
  gap: 12px;
}

.review-layout.is-locked {
  pointer-events: none;
  opacity: .78;
}

.review-list,
.review-editor {
  min-height: 0;
  overflow: auto;
}

.review-list__header {
  min-height: 58px;
  padding: 0 14px;
  display: flex;
  align-items: center;
  justify-content: space-between;
  border-bottom: 1px solid #edf1f5;
  font-size: 11px;
}

.review-list__title {
  display: grid;
  gap: 3px;
}

.review-list__title > span {
  color: #d97706;
}

.review-list__selection {
  display: flex;
  align-items: center;
  gap: 3px;
}

.review-list__selection :deep(.el-checkbox__label) {
  padding-left: 5px;
  font-size: 11px;
}

.duplicate-scan-progress {
  padding: 0 14px 8px;
  display: grid;
  gap: 5px;
  color: #d97706;
}

.duplicate-scan-progress small {
  line-height: 1.35;
}

.batch-classification {
  margin: 10px 8px 8px;
  padding: 10px;
  display: grid;
  gap: 8px;
  border: 1px solid #dbeafe;
  border-radius: 8px;
  background: #f8fbff;
}

.batch-classification__heading {
  display: flex;
  align-items: center;
  justify-content: space-between;
  color: #334155;
  font-size: 11px;
}

.batch-classification__heading span {
  color: #64748b;
  font-size: 10px;
}

.batch-classification__fields {
  display: grid;
  grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
  gap: 7px;
}

.batch-classification__fields :deep(.el-select) {
  width: 100%;
}

.batch-classification__apply {
  width: 100%;
}

.import-limit-notice {
  margin: 10px;
  font-size: 10px;
  line-height: 1.6;
}

.review-item {
  width: calc(100% - 16px);
  min-height: 72px;
  margin: 6px 8px;
  padding: 10px;
  display: grid;
  grid-template-columns: 22px 54px minmax(0, 1fr) 18px;
  align-items: start;
  gap: 7px;
  border: 1px solid #edf1f5;
  border-radius: 7px;
  background: #fff;
  text-align: left;
}

.review-item:hover,
.review-item.is-active {
  border-color: #93c5fd;
  background: #f8fbff;
}

.review-item.is-warning {
  border-left: 3px solid #f59e0b;
}

.review-item.is-error {
  border-left: 3px solid #ef4444;
}

.review-item__warning-icon {
  color: #d97706;
}

.review-item__ordinal {
  color: #64748b;
  font-size: 10px;
}

.review-item__content {
  min-width: 0;
  display: grid;
  gap: 5px;
}

.review-item__content strong {
  font-size: 11px;
}

.review-item__preview {
  overflow: hidden;
  color: #64748b;
  font-size: 10px;
  line-height: 1.5;
  display: -webkit-box;
  max-height: 3em;
  -webkit-box-orient: vertical;
  -webkit-line-clamp: 2;
}

.review-item__preview :deep(p) {
  display: inline;
  margin: 0;
}

.review-item__preview :deep(.katex) {
  font-size: 1em;
}

.review-item__empty {
  overflow: hidden;
  color: #64748b;
  font-size: 10px;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.review-editor {
  padding: 16px;
}

.review-meta {
  margin-top: 12px;
  display: grid;
  grid-template-columns: repeat(4, minmax(0, 1fr));
  gap: 10px;
}

.review-meta :deep(.el-form-item) {
  margin-bottom: 2px;
}

.review-meta :deep(.el-select) {
  width: 100%;
}

.review-field {
  margin-top: 12px;
}

.review-field label {
  margin-bottom: 7px;
  display: block;
  color: #334155;
  font-size: 11px;
  font-weight: 600;
}

.review-field__heading {
  margin-bottom: 7px;
  display: flex;
  align-items: center;
  justify-content: space-between;
}

.review-field__heading label {
  margin-bottom: 0;
}

.review-option {
  margin-bottom: 7px;
  display: grid;
  grid-template-columns: 24px minmax(0, 1fr) 46px;
  align-items: center;
  gap: 8px;
}

.review-option > span {
  width: 24px;
  height: 24px;
  display: grid;
  place-items: center;
  border-radius: 50%;
  background: #eff6ff;
  color: #2563eb;
  font-size: 10px;
  font-weight: 700;
}

.duplicate-row {
  margin-top: 12px;
  padding: 10px 12px;
  display: flex;
  align-items: center;
  justify-content: space-between;
  border-radius: 7px;
  background: #fff7ed;
  color: #9a3412;
  font-size: 11px;
}

.duplicate-warning {
  padding: 11px 13px;
  display: flex;
  align-items: center;
  gap: 8px;
  border: 1px solid #fed7aa;
  border-radius: 8px;
  background: #fffaf0;
  color: #9a5b13;
  font-size: 12px;
}

.duplicate-compare {
  margin-top: 14px;
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 12px;
}

.duplicate-compare article {
  min-height: 150px;
  padding: 16px;
  border: 1px solid #dfe6ef;
  border-radius: 9px;
  background: #fff;
}

.duplicate-compare header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
}

.duplicate-compare__stem {
  margin: 18px 0;
  color: #172033;
  font-size: 13px;
  font-weight: 650;
  line-height: 1.7;
}

.duplicate-compare small {
  color: #64748b;
  font-size: 11px;
  line-height: 1.7;
}

.duplicate-apply-all {
  margin-top: 14px;
}

.remove-item {
  margin-top: 8px;
}

.review-footer {
  position: absolute;
  right: 0;
  bottom: 0;
  left: 0;
  z-index: 3;
  min-height: 74px;
  padding: 0 28px;
  display: flex;
  align-items: center;
  justify-content: space-between;
  border-top: 1px solid #e5eaf2;
  background: #fff;
  box-shadow: 0 -5px 18px rgb(15 23 42 / 5%);
  color: #64748b;
  font-size: 11px;
}

.review-footer > div:last-child {
  display: flex;
  gap: 8px;
}

.review-footer__summary {
  display: grid;
  gap: 3px;
}

.review-footer__summary small {
  color: #94a3b8;
  font-size: 10px;
}

.review-footer__execution {
  width: min(520px, 48vw);
  display: grid;
  grid-template-columns: minmax(150px, 240px) auto;
  align-items: center;
  gap: 3px 10px;
}

.review-footer__execution .el-progress {
  width: 100%;
}

.review-footer__execution small:last-child:not(:first-of-type) {
  grid-column: 1 / -1;
  color: #b45309;
}

.review-footer__summary .browser-demo-note {
  color: #b45309;
}

.result-card {
  width: min(760px, 90%);
  margin: 20px auto;
  padding: 58px 70px;
  text-align: center;
}

.result-icon {
  width: 76px;
  height: 76px;
  margin: 0 auto;
  display: grid;
  place-items: center;
  border-radius: 50%;
  background: #dcfce7;
  color: #16a34a;
  font-size: 42px;
}

.result-metrics {
  margin: 28px 0;
  display: grid;
  grid-template-columns: repeat(3, 1fr);
  border-radius: 8px;
  background: #f8fafc;
}

.result-metrics div {
  padding: 16px;
  display: grid;
  gap: 4px;
}

.result-metrics strong {
  color: #2563eb;
  font-size: 24px;
}

.result-metrics span {
  color: #64748b;
  font-size: 10px;
}

.result-actions {
  display: flex;
  justify-content: center;
  gap: 8px;
}
</style>
