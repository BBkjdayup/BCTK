<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, toRaw, watch } from 'vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import {
  Brush,
  Document,
  Grid,
  Operation,
  Printer,
  Refresh,
  RefreshLeft,
  RefreshRight,
  Search,
} from '@element-plus/icons-vue'
import Editor, {
  ElementType,
  EditorMode,
  EditorZone,
  ListStyle,
  ListType,
  PaperDirection,
  RowFlex,
  TitleLevel,
  VerticalAlign,
  type IEditorData,
  type IElement,
  type IEditorOption,
  type IEditorResult,
  type IRangeStyle,
} from '@hufe921/canvas-editor'
import { backend } from '../services/backend'
import { errorMessage } from '../services/errors'
import {
  type Paper,
  type PaperLayout,
  type PaperPageSetup,
  type QuestionTypeDefinition,
  type TemplateLayoutPreview,
} from '../types/domain'
import { printCanvasPages } from '../utils/canvasPrint'
import { renderMathInto } from '../utils/mathRendering'
import {
  CANVAS_EDITOR_VERSION,
  DEFAULT_CANVAS_EDITOR_OPTIONS,
  PAPER_FORMULA_RENDER_SCALE_VERSION,
  createPaperCanvasData,
  createPaperCanvasOptions,
  isPaperCanvasFormulaReference,
  normalizeCanvasLatex,
  paperManagedImageResourceIds,
  paperLayoutSourceSignature,
  type PaperCanvasFormulaChange,
  type PaperCanvasFormulaReference,
  type PaperCanvasImageSource,
} from '../utils/paperLayout'

const props = withDefaults(defineProps<{
  paper: Paper
  templatePreview?: TemplateLayoutPreview | null
  questionTypes?: readonly QuestionTypeDefinition[]
  canPrint?: boolean
}>(), {
  canPrint: true,
})
const layout = defineModel<PaperLayout | null | undefined>({ required: true })
const emit = defineEmits<{
  changed: []
  staleChange: [stale: boolean]
  formulaChange: [change: PaperCanvasFormulaChange]
}>()

const container = ref<HTMLDivElement | null>(null)
const scrollContainer = ref<HTMLDivElement | null>(null)
const imageInput = ref<HTMLInputElement | null>(null)
const ready = ref(false)
const initializing = ref(true)
const regenerating = ref(false)
const printing = ref(false)
const operationMessage = ref('正在准备试卷内容…')
const initializationError = ref('')
const initializationDetail = ref('')
const pageCount = ref(1)
const currentPage = ref(1)
const pageScale = ref(100)
const fitMode = ref<'page' | 'width' | null>('page')
const wordCount = ref(0)
const currentZone = ref<EditorZone>(EditorZone.MAIN)
const activeFont = ref('宋体')
const activeSize = ref(16)
const activeColor = ref('#000000')
const activeHighlight = ref('#ffff00')
const activeStyle = ref<Partial<IRangeStyle>>({})
const headingLevel = ref('')
const tableRows = ref(3)
const tableColumns = ref(3)
const searchKeyword = ref('')
const searchExpanded = ref(false)
const searchInput = ref<HTMLInputElement | null>(null)
const openToolMenu = ref<'insert' | 'table' | 'page' | null>(null)
const pageSizePreset = ref('custom')
const pageOrientation = ref<'portrait' | 'landscape'>('portrait')
const pageWidthMm = ref(210)
const pageHeightMm = ref(297)
const pageSetupOverride = ref<PaperPageSetup | null>(null)
const formulaDialogOpen = ref(false)
const formulaInput = ref('')
const formulaPreviewHost = ref<HTMLElement | null>(null)
const editingFormulaReference = ref<PaperCanvasFormulaReference | null>(null)
let editor: Editor | null = null
let applyingExternalValue = false
let metricsSequence = 0
let metricsFailureReported = false
let fitFrame = 0
let workspaceResizeObserver: ResizeObserver | null = null
let imageLoadGeneration = 0
let componentDestroyed = false
const managedImageSources = new Map<string, PaperCanvasImageSource>()

const LARGE_PAPER_QUESTION_COUNT = 100
const LARGE_PAPER_AUTO_SCALE_LIMIT = 0.5

const fontOptions = [
  '宋体',
  '黑体',
  '仿宋',
  '楷体',
  '微软雅黑',
  'Microsoft YaHei',
  'Arial',
  'Times New Roman',
]
const sizeOptions = [
  { label: '初号', value: 56 },
  { label: '小初', value: 48 },
  { label: '一号', value: 35 },
  { label: '小一', value: 32 },
  { label: '二号', value: 29 },
  { label: '小二', value: 24 },
  { label: '三号', value: 21 },
  { label: '小三', value: 20 },
  { label: '四号', value: 19 },
  { label: '小四', value: 16 },
  { label: '五号', value: 14 },
  { label: '小五', value: 12 },
]
const paperSizeOptions = [
  { value: 'a3', label: 'A3（297 × 420 毫米）', widthMm: 297, heightMm: 420 },
  { value: 'a4', label: 'A4（210 × 297 毫米）', widthMm: 210, heightMm: 297 },
  { value: 'a5', label: 'A5（148 × 210 毫米）', widthMm: 148, heightMm: 210 },
  { value: 'b4', label: 'B4（257 × 364 毫米）', widthMm: 257, heightMm: 364 },
  { value: 'b5', label: 'B5（176 × 250 毫米）', widthMm: 176, heightMm: 250 },
]

const currentSignature = computed(() => paperLayoutSourceSignature(props.paper))
const stale = computed(() => Boolean(layout.value && layout.value.sourceSignature !== currentSignature.value))
const zoneLabel = computed(() => ({
  [EditorZone.HEADER]: '页眉',
  [EditorZone.MAIN]: '正文',
  [EditorZone.FOOTER]: '页脚',
})[currentZone.value])

function inspectEditorData(data: IEditorData) {
  let count = 0
  let hasFormula = false
  function visit(value: unknown) {
    if (Array.isArray(value)) {
      value.forEach(visit)
      return
    }
    if (!value || typeof value !== 'object') return
    if (Reflect.get(value, 'type') === ElementType.LATEX) hasFormula = true
    Object.entries(value).forEach(([key, child]) => {
      if (key === 'value' && typeof child === 'string') {
        count += Array.from(child.replace(/\s/g, '')).length
      } else {
        visit(child)
      }
    })
  }
  visit(data)
  return { characterCount: count, hasFormula }
}

function collectFormulaElements(data: IEditorData) {
  const formulas: IElement[] = []
  function visit(value: unknown) {
    if (Array.isArray(value)) {
      value.forEach(visit)
      return
    }
    if (!value || typeof value !== 'object') return
    if (Reflect.get(value, 'type') === ElementType.LATEX) formulas.push(value as IElement)
    Object.values(value).forEach(visit)
  }
  visit(data)
  return formulas
}

function sameFormulaReference(left: unknown, right: PaperCanvasFormulaReference) {
  return isPaperCanvasFormulaReference(left)
    && left.paperItemId === right.paperItemId
    && left.contentSlot === right.contentSlot
    && (left.optionId ?? '') === (right.optionId ?? '')
    && left.ordinal === right.ordinal
}

function normalizeEditorFormulaDimensions(instance: Editor) {
  const snapshot = instance.command.getValue({ extraPickAttrs: ['id'] })
  const defaultSize = Number(instance.command.getOptions().defaultSize ?? 16)
  let changed = false
  for (const formula of collectFormulaElements(snapshot.data)) {
    const extension = formula.extension && typeof formula.extension === 'object'
      ? formula.extension as Record<string, unknown>
      : {}
    if (extension.renderScaleVersion === PAPER_FORMULA_RENDER_SCALE_VERSION || !formula.id) continue
    const width = Number(formula.width)
    const height = Number(formula.height)
    if (!Number.isFinite(width) || !Number.isFinite(height) || width <= 0 || height <= 0) continue
    const size = Number(formula.size ?? defaultSize)
    const scale = 0.82 * (Number.isFinite(size) && size > 0 ? size / 16 : defaultSize / 16)
    formula.width = Math.max(4, Math.round(width * scale * 100) / 100)
    formula.height = Math.max(4, Math.round(height * scale * 100) / 100)
    formula.imgPreviewDisabled = true
    formula.imgToolDisabled = true
    formula.extension = {
      ...extension,
      renderScaleVersion: PAPER_FORMULA_RENDER_SCALE_VERSION,
    }
    changed = true
  }
  // Updating formulas one by one makes canvas-editor recalculate the whole
  // document after every formula. Apply the normalized snapshot in one pass.
  if (changed) instance.command.executeSetValue(snapshot.data, { isSetCursor: false })
  return changed
}

function openFormulaEditor(element: IElement) {
  if (element.type !== ElementType.LATEX) return
  const reference = isPaperCanvasFormulaReference(element.extension)
    ? element.extension
    : null
  if (!reference) {
    ElMessage.info('该公式不属于试题内容，请先点击“重新生成”后再编辑。')
    return
  }
  editingFormulaReference.value = reference
  formulaInput.value = reference.sourceLatex || element.value
  formulaDialogOpen.value = true
}

function replaceFormulaInData(
  data: IEditorData,
  reference: PaperCanvasFormulaReference,
  latex: string,
) {
  const target = collectFormulaElements(data).find((formula) => (
    sameFormulaReference(formula.extension, reference)
  ))
  if (!target) return false
  target.value = normalizeCanvasLatex(latex)
  target.imgPreviewDisabled = true
  target.imgToolDisabled = true
  target.extension = {
    ...reference,
    sourceLatex: latex,
    renderScaleVersion: undefined,
  }
  delete target.width
  delete target.height
  delete target.laTexSVG
  return true
}

function saveFormulaEdit() {
  const latex = formulaInput.value.trim()
  const reference = editingFormulaReference.value
  if (!editor || !reference || !latex) return
  const result = editor.command.getValue()
  if (!replaceFormulaInData(result.data, reference, latex)) {
    ElMessage.error('没有在当前排版中找到该公式，请重新生成排版后重试。')
    return
  }
  emit('formulaChange', { reference, latex })
  const options = editor.command.getOptions()
  formulaDialogOpen.value = false
  editingFormulaReference.value = null
  if (!mountEditor(result.data, options, true)) {
    ElMessage.error('公式已修改，但重新绘制失败，请点击“重新生成”。')
  }
}

function containsLegacyInlineMath(data: IEditorData): boolean {
  let found = false
  function visit(value: unknown) {
    if (found) return
    if (Array.isArray(value)) {
      value.forEach(visit)
      return
    }
    if (!value || typeof value !== 'object') return
    Object.entries(value).forEach(([key, child]) => {
      if (key === 'value' && typeof child === 'string' && /\\\([\s\S]*?\\\)/.test(child)) {
        found = true
      } else {
        visit(child)
      }
    })
  }
  visit(data)
  return found
}

function toLayout(result: IEditorResult, sourceSignature = currentSignature.value): PaperLayout {
  return {
    schemaVersion: 1,
    editor: 'canvas-editor',
    editorVersion: CANVAS_EDITOR_VERSION,
    sourceSignature,
    // canvas-editor's getValue APIs already return detached plain data. Avoid
    // cloning the full document a second time for long papers.
    data: result.data as PaperLayout['data'],
    options: result.options as PaperLayout['options'],
    pageSetup: pageSetupOverride.value
      ? {
          widthMm: pageSetupOverride.value.widthMm,
          heightMm: pageSetupOverride.value.heightMm,
        }
      : null,
  }
}

function millimetresFromPixels(value: number) {
  return value * (25.4 / 96)
}

function syncPageControls(options: IEditorOption) {
  const landscape = options.paperDirection === PaperDirection.HORIZONTAL
  const width = Number(options.width ?? DEFAULT_CANVAS_EDITOR_OPTIONS.width)
  const height = Number(options.height ?? DEFAULT_CANVAS_EDITOR_OPTIONS.height)
  const shortEdge = millimetresFromPixels(Math.min(width, height))
  const longEdge = millimetresFromPixels(Math.max(width, height))
  pageOrientation.value = landscape ? 'landscape' : 'portrait'
  pageWidthMm.value = Number((landscape ? longEdge : shortEdge).toFixed(1))
  pageHeightMm.value = Number((landscape ? shortEdge : longEdge).toFixed(1))
  const preset = paperSizeOptions.find((item) => (
    Math.abs(item.widthMm - shortEdge) <= 1
    && Math.abs(item.heightMm - longEdge) <= 1
  ))
  pageSizePreset.value = preset?.value ?? 'custom'
}

async function refreshMetrics() {
  if (!editor) return
  const activeEditor = editor
  const sequence = ++metricsSequence
  try {
    const count = await activeEditor.command.getWordCount()
    if (sequence !== metricsSequence || editor !== activeEditor) return
    metricsFailureReported = false
    if (count > 0) wordCount.value = count
    const position = activeEditor.command.getCursorPosition()
    if (position) currentPage.value = position.pageNo + 1
  } catch (reason) {
    if (sequence !== metricsSequence || editor !== activeEditor || metricsFailureReported) return
    metricsFailureReported = true
    // Word count runs in canvas-editor's worker. A worker failure should not
    // invalidate an otherwise usable page editor.
    console.warn('canvas-editor 字数统计暂时不可用', reason)
  }
}

async function yieldForPaint() {
  await nextTick()
  if (import.meta.env.MODE === 'test') return
  await new Promise<void>((resolve) => {
    let timeout = 0
    let settled = false
    const finish = () => {
      if (settled) return
      settled = true
      window.clearTimeout(timeout)
      resolve()
    }
    timeout = window.setTimeout(finish, 100)
    window.requestAnimationFrame(() => window.requestAnimationFrame(finish))
  })
}

function resolveAutomaticScale(options: IEditorOption) {
  if (!scrollContainer.value) return Number(options.scale ?? 1)
  const availableWidth = scrollContainer.value.clientWidth - 48
  const availableHeight = scrollContainer.value.clientHeight - 32
  if (availableWidth <= 0 || availableHeight <= 0) return Number(options.scale ?? 1)

  const width = Number(options.width ?? DEFAULT_CANVAS_EDITOR_OPTIONS.width)
  const height = Number(options.height ?? DEFAULT_CANVAS_EDITOR_OPTIONS.height)
  const landscape = options.paperDirection === PaperDirection.HORIZONTAL
  const paperWidth = landscape ? height : width
  const paperHeight = landscape ? width : height
  if (paperWidth <= 0 || paperHeight <= 0) return Number(options.scale ?? 1)

  const rawScale = fitMode.value === 'width'
    ? availableWidth / paperWidth
    : Math.min(availableWidth / paperWidth, availableHeight / paperHeight)
  const upperLimit = props.paper.items.length >= LARGE_PAPER_QUESTION_COUNT
    ? LARGE_PAPER_AUTO_SCALE_LIMIT
    : 2
  return Math.max(0.4, Math.min(upperLimit, Math.floor(rawScale * 20) / 20))
}

function withInitialPreviewScale(options: IEditorOption): IEditorOption {
  return {
    ...options,
    scale: resolveAutomaticScale(options),
  }
}

function captureLayout() {
  if (!editor || applyingExternalValue) return
  layout.value = toLayout(
    editor.command.getValue(),
    layout.value?.sourceSignature ?? currentSignature.value,
  )
  void refreshMetrics()
  emit('changed')
}

function templateLockedOptions(base: IEditorOption, stored?: PaperLayout['options']): IEditorOption {
  const merged = {
    ...base,
    ...(stored ?? {}),
    locale: 'zhCN',
  } as IEditorOption
  if (!props.templatePreview) return merged
  return {
    ...merged,
    width: base.width,
    height: base.height,
    margins: base.margins,
    paperDirection: base.paperDirection,
    column: base.column,
    header: base.header,
    footer: base.footer,
    pageNumber: base.pageNumber,
  }
}

function destroyEditor() {
  metricsSequence += 1
  ready.value = false
  editor?.destroy()
  editor = null
  container.value?.replaceChildren()
}

function canvasInitializationDetail(reason: unknown) {
  const name = reason instanceof Error ? reason.name : ''
  const message = reason instanceof Error ? reason.message : String(reason ?? '')
  const code = name === 'SecurityError'
    ? 'CANVAS_WORKER_BLOCKED'
    : name === 'DataCloneError'
      ? 'CANVAS_DATA_CLONE_FAILED'
      : 'CANVAS_INIT_FAILED'
  const detail = `${name ? `${name}: ` : ''}${message}`.replace(/\s+/g, ' ').trim().slice(0, 240)
  return `${code}${detail ? ` · ${detail}` : ''}`
}

function mountEditor(
  data: IEditorData,
  options: IEditorOption,
  persistInitialLayout: boolean,
) {
  if (!container.value) return false
  destroyEditor()
  const inspection = inspectEditorData(data)
  wordCount.value = inspection.characterCount
  try {
    const createInstance = () => new Editor(container.value!, toRaw(data), toRaw(options))
    let instance = createInstance()
    editor = instance
    if (inspection.hasFormula) {
      try {
        normalizeEditorFormulaDimensions(instance)
      } catch (reason) {
        // Formula-size normalization is only a visual compatibility pass. If
        // an older WebView/canvas state rejects it, rebuild from the original
        // document and keep the editor usable without that optional pass.
        console.warn('canvas-editor 公式尺寸兼容处理失败，已使用原始尺寸', reason)
        destroyEditor()
        if (!container.value) throw reason
        instance = createInstance()
        editor = instance
      }
    }
    instance.listener.contentChange = captureLayout
    instance.listener.pageSizeChange = (count) => { pageCount.value = Math.max(1, count) }
    instance.listener.intersectionPageNoChange = (pageNo) => { currentPage.value = pageNo + 1 }
    instance.listener.pageScaleChange = (scale) => { pageScale.value = Math.round(scale * 100) }
    instance.listener.zoneChange = (zone) => { currentZone.value = zone }
    instance.listener.rangeStyleChange = applyRangeStyle
    instance.eventBus.on('imageDblclick', ({ element }) => openFormulaEditor(element))

    const resolved = instance.command.getOptions()
    pageScale.value = Math.round(resolved.scale * 100)
    activeFont.value = resolved.defaultFont
    activeSize.value = resolved.defaultSize
    syncPageControls(resolved)
    pageCount.value = Math.max(1, container.value.querySelectorAll('canvas[data-index]').length)
    ready.value = true
    initializationError.value = ''
    initializationDetail.value = ''
    metricsFailureReported = false
    void refreshMetrics()

    if (persistInitialLayout) {
      layout.value = toLayout(instance.command.getValue())
      emit('changed')
    }
    return true
  } catch (reason) {
    console.error('canvas-editor 初始化失败', reason)
    initializationDetail.value = canvasInitializationDetail(reason)
    destroyEditor()
    return false
  }
}

async function loadManagedImageSources() {
  const resourceIds = paperManagedImageResourceIds(props.paper)
  const missingIds = resourceIds.filter((resourceId) => !managedImageSources.has(resourceId))
  if (!missingIds.length) return
  const request = ++imageLoadGeneration
  const results = await Promise.allSettled(missingIds.map(async (resourceId) => {
    const payload = await backend.getManagedImage(resourceId)
    return {
      resourceId,
      source: {
        src: `data:${payload.mimeType};base64,${payload.dataBase64}`,
        widthPx: payload.widthPx,
        heightPx: payload.heightPx,
      } satisfies PaperCanvasImageSource,
    }
  }))
  if (request !== imageLoadGeneration) return
  results.forEach((result) => {
    if (result.status === 'fulfilled') {
      managedImageSources.set(result.value.resourceId, result.value.source)
    } else {
      console.warn('Unable to load a managed image for the paper preview.', result.reason)
    }
  })
}

async function createEditor(forceFresh = false) {
  if (!container.value) return false
  const storedLayout = forceFresh ? null : layout.value
  const sourceChanged = Boolean(
    storedLayout && storedLayout.sourceSignature !== currentSignature.value,
  )
  const migrateOldLayout = Boolean(
    storedLayout
      && storedLayout.editorVersion !== CANVAS_EDITOR_VERSION
      && (
        storedLayout.editorVersion !== '0.9.137-zhitiku.9'
        || containsLegacyInlineMath(storedLayout.data as IEditorData)
      ),
  )
  const useStoredLayout = Boolean(storedLayout && !migrateOldLayout && !sourceChanged)
  if (!useStoredLayout) {
    operationMessage.value = sourceChanged
      ? '题目已变化，正在自动重新生成排版…'
      : '正在读取试题图片…'
    await loadManagedImageSources()
    if (componentDestroyed || !container.value) return false
  }
  pageSetupOverride.value = storedLayout?.pageSetup ?? null
  const baseOptions = createPaperCanvasOptions(props.templatePreview, pageSetupOverride.value)
  operationMessage.value = useStoredLayout
    ? '正在恢复已保存的排版…'
    : sourceChanged
      ? '正在按最新题目重新转换内容…'
      : '正在转换试题内容…'
  await yieldForPaint()
  if (componentDestroyed || !container.value) return false
  const data = (
    useStoredLayout
      ? storedLayout?.data
      : createPaperCanvasData(props.paper, props.templatePreview, managedImageSources, props.questionTypes)
  ) as IEditorData
  const options = withInitialPreviewScale(templateLockedOptions(
    baseOptions,
    useStoredLayout ? storedLayout?.options : undefined,
  ))
  operationMessage.value = `正在计算 ${props.paper.items.length} 道题的分页…`
  await yieldForPaint()
  if (componentDestroyed || !container.value) return false
  if (mountEditor(data, options, !useStoredLayout)) return true

  // A persisted snapshot may have been produced by an older canvas state even
  // when its adapter version matches. Keep the page usable by rebuilding from
  // the canonical paper data instead of leaving a blank, disabled editor.
  if (useStoredLayout) {
    operationMessage.value = '旧排版无法恢复，正在重新转换试题内容…'
    await loadManagedImageSources()
    if (componentDestroyed || !container.value) return false
    await yieldForPaint()
    if (componentDestroyed || !container.value) return false
    const rebuiltData = createPaperCanvasData(
      props.paper,
      props.templatePreview,
      managedImageSources,
      props.questionTypes,
    )
    operationMessage.value = `正在重新计算 ${props.paper.items.length} 道题的分页…`
    await yieldForPaint()
    if (componentDestroyed || !container.value) return false
    return mountEditor(
      rebuiltData,
      withInitialPreviewScale(templateLockedOptions(baseOptions)),
      true,
    )
  }
  return false
}

function applyRangeStyle(style: IRangeStyle) {
  activeStyle.value = style
  activeFont.value = style.font || activeFont.value
  activeSize.value = style.size || activeSize.value
  activeColor.value = style.color || '#000000'
  activeHighlight.value = style.highlight || '#ffff00'
  headingLevel.value = style.level ?? ''
  const position = editor?.command.getCursorPosition()
  if (position) currentPage.value = position.pageNo + 1
}

async function regenerate() {
  if (initializing.value || regenerating.value || printing.value) return
  regenerating.value = true
  initializationError.value = ''
  initializationDetail.value = ''
  let data: IEditorData | null = null
  try {
    operationMessage.value = '正在读取试题图片…'
    await yieldForPaint()
    await loadManagedImageSources()
    if (componentDestroyed || !container.value) return
    operationMessage.value = '正在重新转换试题内容…'
    await yieldForPaint()
    if (componentDestroyed || !container.value) return
    data = createPaperCanvasData(props.paper, props.templatePreview, managedImageSources, props.questionTypes)
    operationMessage.value = `正在重新计算 ${props.paper.items.length} 道题的分页…`
    await yieldForPaint()
    if (componentDestroyed || !container.value) return
    if (!editor) {
      const options = withInitialPreviewScale(templateLockedOptions(createPaperCanvasOptions(
        props.templatePreview,
        pageSetupOverride.value,
      )))
      if (!mountEditor(data, options, true)) {
        initializationError.value = '排版画布初始化失败，请返回后重试。'
        ElMessage.error(initializationError.value)
      }
      return
    }
    applyingExternalValue = true
    editor.command.executeSetValue(data, { isSetCursor: false })
    layout.value = toLayout(editor.command.getValue(), currentSignature.value)
    void refreshMetrics()
    emit('changed')
  } catch (reason) {
    console.error('canvas-editor 重新生成失败', reason)
    initializationDetail.value = canvasInitializationDetail(reason)
    if (!data || !mountEditor(
      data,
      withInitialPreviewScale(templateLockedOptions(createPaperCanvasOptions(
        props.templatePreview,
        pageSetupOverride.value,
      ))),
      true,
    )) {
      initializationError.value = '重新生成排版失败，请返回后重试。'
      ElMessage.error(initializationError.value)
    }
  } finally {
    applyingExternalValue = false
    regenerating.value = false
    operationMessage.value = ''
  }
}

function setFont() { editor?.command.executeFont(activeFont.value) }
function setSize() { editor?.command.executeSize(Number(activeSize.value)) }
function setColor() { editor?.command.executeColor(activeColor.value) }
function setHighlight() { editor?.command.executeHighlight(activeHighlight.value) }
function undo() { editor?.command.executeUndo() }
function redo() { editor?.command.executeRedo() }
function painter() { editor?.command.executePainter({ isDblclick: false }) }
function clearFormat() { editor?.command.executeFormat() }
function bold() { editor?.command.executeBold() }
function italic() { editor?.command.executeItalic() }
function underline() { editor?.command.executeUnderline() }
function strikeout() { editor?.command.executeStrikeout() }
function superscript() { editor?.command.executeSuperscript() }
function subscript() { editor?.command.executeSubscript() }
function align(value: RowFlex) { editor?.command.executeRowFlex(value) }
function unorderedList() { editor?.command.executeList(ListType.UL, ListStyle.DISC) }
function orderedList() { editor?.command.executeList(ListType.OL, ListStyle.DECIMAL) }
function pageBreak() { editor?.command.executePageBreak() }
function scaleDown() {
  fitMode.value = null
  editor?.command.executePageScaleMinus()
}
function scaleUp() {
  fitMode.value = null
  editor?.command.executePageScaleAdd()
}
function recoverScale() {
  fitMode.value = null
  editor?.command.executePageScaleRecovery()
}
function setScale() {
  fitMode.value = null
  editor?.command.executePageScale(pageScale.value / 100)
}
function applyFitMode() {
  if (!editor || !scrollContainer.value || !fitMode.value) return
  const options = editor.command.getOptions()
  const resolvedScale = resolveAutomaticScale(options)
  if (Math.abs(Number(options.scale ?? 1) - resolvedScale) < 0.001) {
    pageScale.value = Math.round(resolvedScale * 100)
    return
  }
  editor.command.executePageScale(resolvedScale)
}
function scheduleFitMode() {
  if (fitFrame) window.cancelAnimationFrame(fitFrame)
  fitFrame = window.requestAnimationFrame(() => {
    fitFrame = 0
    applyFitMode()
  })
}
function fitPage() {
  fitMode.value = 'page'
  scheduleFitMode()
}
function fitWidth() {
  fitMode.value = 'width'
  scheduleFitMode()
}
function insertTable() { editor?.command.executeInsertTable(tableRows.value, tableColumns.value) }
function toggleToolMenu(menu: 'insert' | 'table' | 'page') {
  openToolMenu.value = openToolMenu.value === menu ? null : menu
}
function setColumns(count: number) {
  editor?.command.executeSetColumns(count === 1 ? null : {
    count,
    gap: props.templatePreview
      ? Math.round(props.templatePreview.page.columnGapTwips * (96 / 1440))
      : 24,
  })
}

function selectPaperSizePreset() {
  const preset = paperSizeOptions.find((item) => item.value === pageSizePreset.value)
  if (!preset) return
  const landscape = pageOrientation.value === 'landscape'
  pageWidthMm.value = landscape ? preset.heightMm : preset.widthMm
  pageHeightMm.value = landscape ? preset.widthMm : preset.heightMm
}

function changePageOrientation() {
  const shortEdge = Math.min(Number(pageWidthMm.value), Number(pageHeightMm.value))
  const longEdge = Math.max(Number(pageWidthMm.value), Number(pageHeightMm.value))
  const landscape = pageOrientation.value === 'landscape'
  pageWidthMm.value = landscape ? longEdge : shortEdge
  pageHeightMm.value = landscape ? shortEdge : longEdge
}

function applyPaperSize() {
  if (!editor) return
  const widthMm = Number(pageWidthMm.value)
  const heightMm = Number(pageHeightMm.value)
  if (
    !Number.isFinite(widthMm)
    || !Number.isFinite(heightMm)
    || widthMm < 90
    || widthMm > 600
    || heightMm < 90
    || heightMm > 600
  ) {
    ElMessage.warning('纸张宽度和高度必须在 90～600 毫米之间。')
    return
  }
  const shortEdge = Math.min(widthMm, heightMm)
  const longEdge = Math.max(widthMm, heightMm)
  const landscape = pageOrientation.value === 'landscape'
  const actualWidthMm = landscape ? longEdge : shortEdge
  const actualHeightMm = landscape ? shortEdge : longEdge
  const shortPixels = Math.max(340, Math.round(shortEdge * (96 / 25.4)))
  const longPixels = Math.max(shortPixels + 1, Math.round(longEdge * (96 / 25.4)))
  pageSetupOverride.value = {
    widthMm: Number(actualWidthMm.toFixed(2)),
    heightMm: Number(actualHeightMm.toFixed(2)),
  }
  editor.command.executePaperSize(shortPixels, longPixels)
  editor.command.executePaperDirection(
    landscape ? PaperDirection.HORIZONTAL : PaperDirection.VERTICAL,
  )
  syncPageControls(editor.command.getOptions())
  captureLayout()
  scheduleFitMode()
}

function togglePaperDirection() {
  pageOrientation.value = pageOrientation.value === 'landscape' ? 'portrait' : 'landscape'
  changePageOrientation()
  applyPaperSize()
}
function switchZone(zone: EditorZone) {
  editor?.command.executeSetZone(zone)
  editor?.command.executeFocus()
}
function setHeading() {
  editor?.command.executeTitle(headingLevel.value
    ? headingLevel.value as TitleLevel
    : null)
}
function search() {
  editor?.command.executeSearch(searchKeyword.value.trim() || null)
}

function toggleSearch() {
  searchExpanded.value = !searchExpanded.value
  if (searchExpanded.value) {
    nextTick(() => searchInput.value?.focus())
    return
  }
  searchKeyword.value = ''
  editor?.command.executeSearch(null)
}

function closeSearch() {
  if (!searchExpanded.value) return
  searchExpanded.value = false
  searchKeyword.value = ''
  editor?.command.executeSearch(null)
}

async function addHyperlink() {
  if (!editor) return
  try {
    const { value } = await ElMessageBox.prompt(
      '请输入链接地址，例如 https://example.com',
      '插入超链接',
      {
        inputPattern: /^https?:\/\/\S+$/i,
        inputErrorMessage: '请输入以 http:// 或 https:// 开头的地址',
        confirmButtonText: '插入',
        cancelButtonText: '取消',
      },
    )
    const selectedText = editor.command.getRangeText().trim()
    editor.command.executeHyperlink({
      valueList: [{ value: selectedText || value }],
      url: value,
    })
  } catch {
    // The user cancelled the dialog.
  }
}

function chooseImage() {
  imageInput.value?.click()
}

async function insertImage(event: Event) {
  const input = event.target as HTMLInputElement
  const file = input.files?.[0]
  input.value = ''
  if (!file || !editor) return
  if (file.size > 10 * 1024 * 1024) {
    ElMessage.warning('单张图片不能超过 10 MB。')
    return
  }
  const dataUrl = await new Promise<string>((resolve, reject) => {
    const reader = new FileReader()
    reader.onload = () => resolve(String(reader.result ?? ''))
    reader.onerror = () => reject(reader.error)
    reader.readAsDataURL(file)
  }).catch(() => '')
  if (!dataUrl) {
    ElMessage.error('图片读取失败。')
    return
  }
  const imageSize = await new Promise<{ width: number, height: number }>((resolve) => {
    const image = new Image()
    image.onload = () => {
      const ratio = Math.min(1, 480 / Math.max(1, image.naturalWidth))
      resolve({
        width: Math.max(32, Math.round(image.naturalWidth * ratio)),
        height: Math.max(32, Math.round(image.naturalHeight * ratio)),
      })
    }
    image.onerror = () => resolve({ width: 240, height: 160 })
    image.src = dataUrl
  })
  editor.command.executeImage({ value: dataUrl, ...imageSize })
}

async function print() {
  if (!editor || printing.value) return
  if (!props.canPrint) {
    ElMessage.warning('基础桌面模式不支持打印，请导入有效的桌面专业版授权。')
    return
  }
  printing.value = true
  try {
    await backend.checkPrintAuthorization()
    captureLayout()
    const options = editor.command.getOptions()
    const pageImages = await editor.command.getImage({
      pixelRatio: options.printPixelRatio,
      mode: EditorMode.PRINT,
    })
    const landscape = options.paperDirection === PaperDirection.HORIZONTAL
    await printCanvasPages(pageImages, {
      width: landscape ? options.height : options.width,
      height: landscape ? options.width : options.height,
    })
  } catch (reason) {
    ElMessage.error(errorMessage(reason, '打印失败'))
  } finally {
    printing.value = false
  }
}

watch(stale, (value) => emit('staleChange', value), { immediate: true })
watch([formulaInput, formulaDialogOpen], async ([latex, open]) => {
  if (!open) return
  await nextTick()
  if (formulaPreviewHost.value) renderMathInto(formulaPreviewHost.value, latex)
})

onMounted(async () => {
  const startedAt = performance.now()
  try {
    await yieldForPaint()
    if (!await createEditor()) {
      if (!componentDestroyed) initializationError.value = '排版画布初始化失败，请返回后重试。'
      return
    }
    if (typeof ResizeObserver !== 'undefined' && scrollContainer.value) {
      workspaceResizeObserver = new ResizeObserver(scheduleFitMode)
      workspaceResizeObserver.observe(scrollContainer.value)
    }
    fitPage()
  } catch (reason) {
    console.error('排版画布准备失败', reason)
    if (!componentDestroyed) {
      initializationDetail.value = canvasInitializationDetail(reason)
      initializationError.value = '排版画布初始化失败，请返回后重试。'
    }
  } finally {
    initializing.value = false
    operationMessage.value = ''
    if (!componentDestroyed && props.paper.items.length >= LARGE_PAPER_QUESTION_COUNT) {
      console.info('large paper canvas initialized', {
        questionCount: props.paper.items.length,
        pageCount: pageCount.value,
        elapsedMs: Math.round(performance.now() - startedAt),
        scale: pageScale.value,
      })
    }
  }
})
onBeforeUnmount(() => {
  componentDestroyed = true
  imageLoadGeneration += 1
  workspaceResizeObserver?.disconnect()
  workspaceResizeObserver = null
  if (fitFrame) window.cancelAnimationFrame(fitFrame)
  destroyEditor()
})

defineExpose({ regenerate, print })
</script>

<template>
  <div class="paper-canvas-shell" :class="{ 'has-settings': $slots.settings }">
    <section class="paper-canvas-main">
      <div class="paper-canvas-toolbar" aria-label="试卷排版工具栏">
      <div class="toolbar-row">
        <button
          type="button"
          class="toolbar-icon-button"
          title="撤销"
          aria-label="撤销"
          :disabled="!ready || !activeStyle.undo"
          @click="undo"
        ><RefreshLeft aria-hidden="true" /></button>
        <button
          type="button"
          class="toolbar-icon-button"
          title="重做"
          aria-label="重做"
          :disabled="!ready || !activeStyle.redo"
          @click="redo"
        ><RefreshRight aria-hidden="true" /></button>
        <button
          type="button"
          class="toolbar-icon-button"
          title="格式刷"
          aria-label="格式刷"
          :disabled="!ready"
          @click="painter"
        ><Brush aria-hidden="true" /></button>
        <button
          type="button"
          class="toolbar-icon-button"
          title="清除格式"
          aria-label="清除格式"
          :disabled="!ready"
          @click="clearFormat"
        >
          <svg class="toolbar-svg-icon" viewBox="0 0 24 24" aria-hidden="true">
            <path d="m15.8 4.2 4 4a2 2 0 0 1 0 2.8l-8.6 8.6a2 2 0 0 1-2.8 0l-4-4a2 2 0 0 1 0-2.8L13 4.2a2 2 0 0 1 2.8 0Z" />
            <path d="m8.2 9 6.8 6.8M8.8 20H20" />
          </svg>
        </button>
        <span class="toolbar-separator" />

        <select v-model="activeFont" title="字体" :disabled="!ready" @change="setFont">
          <option v-for="font in fontOptions" :key="font" :value="font">{{ font }}</option>
        </select>
        <select v-model="activeSize" title="字号" :disabled="!ready" @change="setSize">
          <option v-for="item in sizeOptions" :key="item.value" :value="item.value">{{ item.label }}</option>
        </select>
        <button type="button" title="增大字号" :disabled="!ready" @click="editor?.command.executeSizeAdd()">A+</button>
        <button type="button" title="减小字号" :disabled="!ready" @click="editor?.command.executeSizeMinus()">A−</button>
        <button type="button" title="加粗" :class="{ active: activeStyle.bold }" :disabled="!ready" @click="bold"><strong>B</strong></button>
        <button type="button" title="斜体" :class="{ active: activeStyle.italic }" :disabled="!ready" @click="italic"><em>I</em></button>
        <button type="button" title="下划线" :class="{ active: activeStyle.underline }" :disabled="!ready" @click="underline"><u>U</u></button>
        <button type="button" title="删除线" :class="{ active: activeStyle.strikeout }" :disabled="!ready" @click="strikeout"><s>S</s></button>
        <button type="button" title="上标" :disabled="!ready" @click="superscript">X²</button>
        <button type="button" title="下标" :disabled="!ready" @click="subscript">X₂</button>
        <label class="color-tool" title="文字颜色">
          <span>A</span>
          <input v-model="activeColor" type="color" :disabled="!ready" @change="setColor">
        </label>
        <label class="color-tool highlight-tool" title="高亮颜色">
          <span>ab</span>
          <input v-model="activeHighlight" type="color" :disabled="!ready" @change="setHighlight">
        </label>

        <span class="toolbar-separator" />
        <select v-model="headingLevel" title="正文与标题级别" :disabled="!ready" @change="setHeading">
          <option value="">正文</option>
          <option :value="TitleLevel.FIRST">标题 1</option>
          <option :value="TitleLevel.SECOND">标题 2</option>
          <option :value="TitleLevel.THIRD">标题 3</option>
        </select>
        <button type="button" class="toolbar-icon-button" title="左对齐" aria-label="左对齐" :class="{ active: activeStyle.rowFlex === RowFlex.LEFT }" :disabled="!ready" @click="align(RowFlex.LEFT)">
          <span class="align-icon align-left" aria-hidden="true"><i /><i /><i /><i /></span>
        </button>
        <button type="button" class="toolbar-icon-button" title="居中" aria-label="居中" :class="{ active: activeStyle.rowFlex === RowFlex.CENTER }" :disabled="!ready" @click="align(RowFlex.CENTER)">
          <span class="align-icon align-center" aria-hidden="true"><i /><i /><i /><i /></span>
        </button>
        <button type="button" class="toolbar-icon-button" title="右对齐" aria-label="右对齐" :class="{ active: activeStyle.rowFlex === RowFlex.RIGHT }" :disabled="!ready" @click="align(RowFlex.RIGHT)">
          <span class="align-icon align-right" aria-hidden="true"><i /><i /><i /><i /></span>
        </button>
        <button type="button" class="toolbar-icon-button" title="两端对齐" aria-label="两端对齐" :class="{ active: activeStyle.rowFlex === RowFlex.JUSTIFY }" :disabled="!ready" @click="align(RowFlex.JUSTIFY)">
          <span class="align-icon align-justify" aria-hidden="true"><i /><i /><i /><i /></span>
        </button>
        <button type="button" class="list-tool" title="项目符号" aria-label="项目符号" :class="{ active: activeStyle.listType === ListType.UL }" :disabled="!ready" @click="unorderedList"><span>•</span><span class="list-tool-label">列表</span></button>
        <button type="button" class="list-tool" title="编号" aria-label="编号" :class="{ active: activeStyle.listType === ListType.OL }" :disabled="!ready" @click="orderedList"><span>1.</span><span class="list-tool-label">列表</span></button>

        <details class="tool-menu" :open="openToolMenu === 'insert'">
          <summary title="插入" aria-label="插入" @click.prevent="toggleToolMenu('insert')"><Operation aria-hidden="true" /><span>插入</span></summary>
          <div class="tool-menu-panel insert-panel">
            <button type="button" @click="chooseImage">图片</button>
            <button type="button" @click="addHyperlink">超链接</button>
            <button type="button" @click="pageBreak">分页符</button>
            <label>表格行数 <input v-model.number="tableRows" type="number" min="1" max="20"></label>
            <label>表格列数 <input v-model.number="tableColumns" type="number" min="1" max="12"></label>
            <button type="button" class="primary-action" @click="insertTable">插入表格</button>
          </div>
        </details>

        <details class="tool-menu" :open="openToolMenu === 'table'">
          <summary title="表格" aria-label="表格" @click.prevent="toggleToolMenu('table')"><Grid aria-hidden="true" /><span>表格</span></summary>
          <div class="tool-menu-panel table-panel">
            <button type="button" @click="editor?.command.executeInsertTableTopRow()">上方插入行</button>
            <button type="button" @click="editor?.command.executeInsertTableBottomRow()">下方插入行</button>
            <button type="button" @click="editor?.command.executeInsertTableLeftCol()">左侧插入列</button>
            <button type="button" @click="editor?.command.executeInsertTableRightCol()">右侧插入列</button>
            <button type="button" @click="editor?.command.executeMergeTableCell()">合并单元格</button>
            <button type="button" @click="editor?.command.executeCancelMergeTableCell()">取消合并</button>
            <button type="button" @click="editor?.command.executeTableTdVerticalAlign(VerticalAlign.TOP)">顶端对齐</button>
            <button type="button" @click="editor?.command.executeTableTdVerticalAlign(VerticalAlign.MIDDLE)">垂直居中</button>
            <button type="button" class="danger-action" @click="editor?.command.executeDeleteTableRow()">删除行</button>
            <button type="button" class="danger-action" @click="editor?.command.executeDeleteTableCol()">删除列</button>
          </div>
        </details>

        <details class="tool-menu" :open="openToolMenu === 'page'">
          <summary title="页面" aria-label="页面" @click.prevent="toggleToolMenu('page')"><Document aria-hidden="true" /><span>页面</span></summary>
          <div class="tool-menu-panel page-panel">
            <label class="page-field">
              纸张大小
              <select v-model="pageSizePreset" aria-label="纸张大小" @change="selectPaperSizePreset">
                <option v-for="item in paperSizeOptions" :key="item.value" :value="item.value">{{ item.label }}</option>
                <option value="custom">自定义</option>
              </select>
            </label>
            <label class="page-field">
              纸张方向
              <select v-model="pageOrientation" aria-label="纸张方向" @change="changePageOrientation">
                <option value="portrait">纵向</option>
                <option value="landscape">横向</option>
              </select>
            </label>
            <div class="page-dimensions">
              <label>宽 <input v-model.number="pageWidthMm" aria-label="纸张宽度" type="number" min="90" max="600" step="0.1"> 毫米</label>
              <label>高 <input v-model.number="pageHeightMm" aria-label="纸张高度" type="number" min="90" max="600" step="0.1"> 毫米</label>
            </div>
            <button type="button" class="primary-action page-apply" @click="applyPaperSize">应用纸张大小</button>
            <button type="button" @click="setColumns(1)">一栏</button>
            <button type="button" @click="setColumns(2)">两栏</button>
            <button type="button" @click="setColumns(3)">三栏</button>
            <button type="button" @click="togglePaperDirection">切换横向 / 纵向</button>
            <button type="button" @click="switchZone(EditorZone.HEADER)">编辑页眉</button>
            <button type="button" @click="switchZone(EditorZone.MAIN)">编辑正文</button>
            <button type="button" @click="switchZone(EditorZone.FOOTER)">编辑页脚</button>
          </div>
        </details>

        <div class="search-tool" :class="{ expanded: searchExpanded }">
          <button
            type="button"
            class="toolbar-icon-button"
            :class="{ active: searchExpanded }"
            :title="searchExpanded ? '收起查找' : '查找'"
            aria-label="查找"
            @click="toggleSearch"
          ><Search aria-hidden="true" /></button>
          <input
            v-if="searchExpanded"
            ref="searchInput"
            v-model="searchKeyword"
            type="search"
            placeholder="查找"
            aria-label="查找内容"
            @keyup.enter="search"
            @keyup.esc="closeSearch"
          >
        </div>
        <span class="toolbar-separator toolbar-action-separator" />
        <button
          type="button"
          class="toolbar-regenerate"
          :title="stale ? '按当前题目重新生成' : '恢复模板排版'"
          :aria-label="stale ? '按当前题目重新生成' : '恢复模板排版'"
          :disabled="printing || initializing || regenerating"
          @click="regenerate"
        ><Refresh aria-hidden="true" /><span>{{ regenerating ? '正在生成…' : '重新生成' }}</span></button>
        <button type="button" class="toolbar-print" :disabled="!ready || printing || !canPrint" :title="canPrint ? '打印' : '桌面专业版可打印'" @click="print">
          <Printer aria-hidden="true" /><span>{{ printing ? '准备打印…' : canPrint ? '打印' : '专业版可打印' }}</span>
        </button>
        <input ref="imageInput" class="hidden-file-input" type="file" accept="image/*" @change="insertImage">
      </div>
    </div>

      <div v-if="stale" class="paper-canvas-warning">
        题目、顺序、标题或显示内容已经变化。旧排版仍被保留；确认后可点击“按当前题目重新生成”。
      </div>

      <div class="paper-canvas-workspace">
        <div ref="scrollContainer" class="paper-canvas-scroll">
          <div ref="container" class="paper-canvas-editor" />
        </div>
        <div
          v-if="initializing || regenerating || initializationError"
          class="paper-canvas-progress"
          :class="{ 'is-error': initializationError }"
        >
          <span v-if="!initializationError" class="paper-canvas-spinner" aria-hidden="true" />
          <strong>{{ initializationError || operationMessage }}</strong>
          <p v-if="initializationError && initializationDetail" class="paper-canvas-error-detail">
            诊断信息：{{ initializationDetail }}
          </p>
          <p v-if="!initializationError && paper.items.length >= LARGE_PAPER_QUESTION_COUNT">
            当前共有 {{ paper.items.length }} 道题，首次分页可能需要一些时间，请勿重复点击。
          </p>
          <button v-if="initializationError" type="button" @click="regenerate">重新尝试</button>
        </div>
      </div>

      <footer class="paper-canvas-status">
        <div>
          <span>可见页码：{{ currentPage }}</span>
          <span>页面：{{ currentPage }} / {{ pageCount }}</span>
          <span>字数：{{ wordCount }}</span>
        </div>
        <strong>{{ zoneLabel }}编辑模式</strong>
        <div class="scale-control">
          <button type="button" title="缩小" @click="scaleDown">−</button>
          <input v-model.number="pageScale" type="range" min="40" max="200" step="5" @change="setScale">
          <button type="button" title="恢复 100%" @click="recoverScale">{{ pageScale }}%</button>
          <button type="button" title="放大" @click="scaleUp">＋</button>
          <button
            type="button"
            class="fit-control"
            :class="{ active: fitMode === 'page' }"
            title="缩放到完整显示一页"
            @click="fitPage"
          >适合页面</button>
          <button
            type="button"
            class="fit-control"
            :class="{ active: fitMode === 'width' }"
            title="缩放到适合编辑区宽度"
            @click="fitWidth"
          >适合宽度</button>
        </div>
      </footer>
    </section>

    <aside v-if="$slots.settings" class="paper-canvas-settings-slot">
      <slot name="settings" />
    </aside>
  </div>

  <el-dialog
    v-model="formulaDialogOpen"
    title="编辑试卷公式"
    width="560px"
    destroy-on-close
    append-to-body
  >
    <div class="paper-formula-dialog">
      <label for="paper-formula-input">LaTeX 公式</label>
      <el-input
        id="paper-formula-input"
        v-model="formulaInput"
        type="textarea"
        :rows="4"
        placeholder="例如：-2\\le m\\le -1"
      />
      <div class="paper-formula-preview">
        <span>预览</span>
        <strong v-if="formulaInput.trim()" ref="formulaPreviewHost" />
        <em v-else>请输入公式</em>
      </div>
      <p>此处修改只写入当前试卷副本，不会改动题库中的原题。</p>
    </div>
    <template #footer>
      <el-button @click="formulaDialogOpen = false">取消</el-button>
      <el-button type="primary" :disabled="!formulaInput.trim()" @click="saveFormulaEdit">保存公式</el-button>
    </template>
  </el-dialog>
</template>

<style scoped>
.paper-formula-dialog {
  display: grid;
  gap: 12px;
}

.paper-formula-dialog > label {
  color: #475569;
  font-size: 13px;
  font-weight: 600;
}

.paper-formula-dialog > p {
  margin: 0;
  color: #64748b;
  font-size: 13px;
}

.paper-formula-preview {
  min-height: 72px;
  padding: 12px 16px;
  display: grid;
  align-items: center;
  grid-template-columns: auto 1fr;
  gap: 16px;
  border: 1px solid #dbeafe;
  border-radius: 8px;
  background: #f8fbff;
}

.paper-formula-preview span,
.paper-formula-preview em {
  color: #64748b;
  font-size: 13px;
  font-style: normal;
}

.paper-formula-preview strong {
  color: #0f172a;
  font-size: 20px;
  font-weight: 500;
}

.paper-canvas-shell {
  min-width: 0;
  min-height: 0;
  height: 100%;
  overflow: hidden;
  display: grid;
  grid-template-columns: minmax(0, 1fr);
  gap: 12px;
  background: transparent;
}

.paper-canvas-main {
  min-width: 0;
  min-height: 0;
  overflow: hidden;
  display: grid;
  grid-template-areas:
    "toolbar"
    "warning"
    "workspace"
    "status";
  grid-template-rows: auto auto minmax(0, 1fr) 34px;
  border: 1px solid #d9e2ee;
  border-radius: 10px;
  background: #eef2f7;
}

.paper-canvas-shell.has-settings {
  grid-template-columns: minmax(0, 1fr) clamp(260px, 19vw, 300px);
}

.paper-canvas-toolbar {
  grid-area: toolbar;
  position: relative;
  z-index: 12;
  border-bottom: 1px solid #d9e2ee;
  background: #fff;
}

.toolbar-row {
  min-height: 40px;
  padding: 4px 7px;
  display: flex;
  align-items: center;
  flex-wrap: nowrap;
  gap: 2px;
  overflow: visible;
}

.toolbar-row button,
.tool-menu summary,
.color-tool {
  box-sizing: border-box;
  min-width: 26px;
  height: 28px;
  padding: 0 6px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border: 1px solid transparent;
  border-radius: 5px;
  background: #fff;
  color: #334155;
  font-size: 12px;
  white-space: nowrap;
  cursor: pointer;
}

.toolbar-icon-button > svg,
.tool-menu summary > svg,
.search-tool button > svg,
.toolbar-regenerate > svg,
.toolbar-print > svg,
.toolbar-svg-icon {
  width: 16px;
  height: 16px;
  flex: 0 0 auto;
}

.toolbar-svg-icon {
  fill: none;
  stroke: currentColor;
  stroke-linecap: round;
  stroke-linejoin: round;
  stroke-width: 1.8;
}

.tool-menu summary,
.toolbar-regenerate,
.toolbar-print {
  gap: 4px;
}

.toolbar-row button:hover:not(:disabled),
.tool-menu summary:hover,
.color-tool:hover,
.toolbar-row button.active {
  border-color: #bfdbfe;
  background: #eff6ff;
  color: #1d4ed8;
}

.toolbar-row button:disabled {
  cursor: not-allowed;
  opacity: .42;
}

.toolbar-row select {
  height: 28px;
  border: 1px solid #dbe3ee;
  border-radius: 5px;
  background: #fff;
  color: #334155;
  font-size: 12px;
}

.toolbar-row select:first-of-type {
  width: 104px;
}

.toolbar-row select:nth-of-type(2) {
  width: 60px;
}

.toolbar-row select:nth-of-type(3) {
  width: 72px;
}

.toolbar-separator {
  flex: 0 0 1px;
  width: 1px;
  height: 20px;
  margin: 0 2px;
  background: #dbe3ee;
}

.color-tool {
  position: relative;
  min-width: 30px;
  font-weight: 700;
}

.color-tool input {
  position: absolute;
  width: 22px;
  height: 6px;
  padding: 0;
  border: 0;
  bottom: 2px;
  opacity: .85;
}

.highlight-tool {
  background: #fff8b5;
}

.tool-menu {
  position: relative;
}

.tool-menu summary {
  list-style: none;
}

.tool-menu summary::-webkit-details-marker {
  display: none;
}

.tool-menu[open] summary {
  border-color: #93c5fd;
  background: #eff6ff;
  color: #1d4ed8;
}

.tool-menu-panel {
  position: absolute;
  z-index: 30;
  top: 32px;
  left: 0;
  width: 240px;
  padding: 8px;
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 6px;
  border: 1px solid #dbe3ee;
  border-radius: 8px;
  background: #fff;
  box-shadow: 0 12px 30px rgb(15 23 42 / 16%);
}

.tool-menu-panel button {
  border-color: #e2e8f0;
  justify-content: flex-start;
}

.tool-menu-panel label {
  grid-column: 1 / -1;
  display: flex;
  align-items: center;
  justify-content: space-between;
  color: #64748b;
  font-size: 12px;
}

.tool-menu-panel input[type="number"] {
  width: 72px;
  height: 28px;
  border: 1px solid #dbe3ee;
  border-radius: 5px;
}

.tool-menu-panel .primary-action {
  grid-column: 1 / -1;
  justify-content: center;
  border-color: #2563eb;
  background: #2563eb;
  color: #fff;
}

.tool-menu-panel .danger-action {
  color: #b91c1c;
}

.page-panel {
  width: 300px;
}

.page-panel .page-field {
  gap: 8px;
}

.page-panel .page-field select {
  width: 190px;
  height: 28px;
  min-width: 0;
  border: 1px solid #dbe3ee;
  border-radius: 5px;
  background: #fff;
  color: #334155;
}

.page-dimensions {
  grid-column: 1 / -1;
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 6px;
}

.tool-menu-panel .page-dimensions label {
  grid-column: auto;
  justify-content: flex-start;
  gap: 4px;
}

.page-dimensions input[type="number"] {
  width: 58px;
  padding: 0 4px;
}

.page-panel .page-apply {
  grid-column: 1 / -1;
}

.search-tool {
  position: relative;
  height: 28px;
  display: flex;
  align-items: center;
}

.search-tool input {
  position: absolute;
  z-index: 40;
  top: 32px;
  right: 0;
  width: 160px;
  height: 28px;
  min-width: 0;
  padding: 0 7px;
  border: 1px solid #dbe3ee;
  border-radius: 5px;
  background: #fff;
  outline: 0;
  font-size: 12px;
  box-shadow: 0 8px 20px rgb(15 23 42 / 14%);
}

.search-tool input:focus {
  border-color: #93c5fd;
  box-shadow: 0 0 0 2px rgb(59 130 246 / 10%);
}

.toolbar-row .toolbar-regenerate {
  margin-left: 2px;
  border-color: #dbe3ee;
}

.toolbar-row .toolbar-print {
  border-color: #2563eb;
  background: #2563eb;
  color: #fff;
}

.toolbar-row .toolbar-print:hover:not(:disabled) {
  background: #1d4ed8;
  color: #fff;
}

.align-icon {
  width: 16px;
  height: 14px;
  display: flex;
  flex-direction: column;
  justify-content: space-between;
}

.align-icon i {
  width: 100%;
  height: 1.5px;
  display: block;
  border-radius: 2px;
  background: currentColor;
}

.align-icon i:nth-child(2) {
  width: 70%;
}

.align-left i:nth-child(2) {
  align-self: flex-start;
}

.align-center i:nth-child(2) {
  align-self: center;
}

.align-right i:nth-child(2) {
  align-self: flex-end;
}

.align-justify i:nth-child(2) {
  width: 100%;
}

.hidden-file-input {
  display: none;
}

.paper-canvas-warning {
  grid-area: warning;
  padding: 9px 14px;
  border-bottom: 1px solid #fde68a;
  background: #fffbeb;
  color: #92400e;
  font-size: 13px;
  line-height: 1.6;
}

.paper-canvas-workspace {
  grid-area: workspace;
  position: relative;
  min-height: 0;
  display: block;
}

.paper-canvas-scroll {
  height: 100%;
  min-height: 0;
  overflow: auto;
}

.paper-canvas-editor {
  width: max-content;
  min-width: max-content;
  min-height: 100%;
  margin: 0 auto;
}

.paper-canvas-progress {
  position: absolute;
  z-index: 20;
  inset: 0;
  padding: 24px;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 10px;
  background: rgb(238 242 247 / 92%);
  color: #334155;
  text-align: center;
  backdrop-filter: blur(2px);
}

.paper-canvas-progress strong {
  font-size: 16px;
}

.paper-canvas-progress p {
  margin: 0;
  color: #64748b;
  font-size: 13px;
}

.paper-canvas-progress .paper-canvas-error-detail {
  max-width: min(720px, 90%);
  color: #b45309;
  overflow-wrap: anywhere;
}

.paper-canvas-progress.is-error {
  background: rgb(255 247 237 / 96%);
  color: #9a3412;
}

.paper-canvas-progress button {
  height: 32px;
  padding: 0 16px;
  border: 1px solid #fdba74;
  border-radius: 6px;
  background: #fff;
  color: #c2410c;
  cursor: pointer;
}

.paper-canvas-spinner {
  width: 28px;
  height: 28px;
  border: 3px solid #bfdbfe;
  border-top-color: #2563eb;
  border-radius: 50%;
  animation: paper-canvas-spin 0.8s linear infinite;
}

@keyframes paper-canvas-spin {
  to { transform: rotate(360deg); }
}

.paper-canvas-status {
  grid-area: status;
  min-width: 0;
  padding: 0 10px;
  display: grid;
  grid-template-columns: 1fr auto 1fr;
  align-items: center;
  border-top: 1px solid #d9e2ee;
  background: #fff;
  color: #475569;
  font-size: 11px;
}

.paper-canvas-status > div:first-child {
  display: flex;
  gap: 14px;
}

.paper-canvas-status > strong {
  color: #64748b;
  font-weight: 500;
}

.scale-control {
  min-width: 0;
  display: flex;
  align-items: center;
  justify-content: flex-end;
  gap: 4px;
}

.scale-control button {
  min-width: 26px;
  height: 24px;
  border: 0;
  border-radius: 4px;
  background: transparent;
  color: #334155;
  cursor: pointer;
}

.scale-control button:hover {
  background: #eff6ff;
  color: #1d4ed8;
}

.scale-control button.active {
  background: #e8f1ff;
  color: #1d4ed8;
}

.scale-control .fit-control {
  min-width: auto;
  padding: 0 7px;
  border: 1px solid #dbe3ee;
  font-size: 10px;
  white-space: nowrap;
}

.scale-control input {
  width: 76px;
}

.paper-canvas-settings-slot {
  min-width: 0;
  min-height: 0;
  overflow: auto;
  background: transparent;
}

.paper-canvas-settings-slot :slotted(*) {
  min-width: 0;
  min-height: 100%;
  box-sizing: border-box;
}

@media (max-width: 1500px) {
  .toolbar-row {
    gap: 1px;
  }

  .toolbar-row button,
  .tool-menu summary,
  .color-tool {
    min-width: 24px;
    padding: 0 4px;
    font-size: 11px;
  }

  .toolbar-row select:first-of-type {
    width: 90px;
  }

  .toolbar-row select:nth-of-type(2) {
    width: 54px;
  }

  .toolbar-row select:nth-of-type(3) {
    width: 64px;
  }

}

@media (max-width: 1350px) {
  .toolbar-row {
    gap: 0;
  }

  .toolbar-row select:first-of-type {
    width: 80px;
  }

  .tool-menu summary span,
  .list-tool-label {
    display: none;
  }

  .tool-menu summary,
  .toolbar-row .list-tool {
    width: 26px;
    min-width: 26px;
    padding: 0 4px;
  }
}

</style>
