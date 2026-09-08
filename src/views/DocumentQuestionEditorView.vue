<script setup lang="ts">
import {
  computed,
  nextTick,
  onBeforeUnmount,
  onMounted,
  ref,
} from 'vue'
import { onBeforeRouteLeave, useRouter } from 'vue-router'
import { ElMessage, ElMessageBox } from 'element-plus'
import {
  Back,
  Brush,
  DocumentAdd,
  Grid,
  Operation,
  Picture,
  RefreshLeft,
  RefreshRight,
  Search,
  Setting,
} from '@element-plus/icons-vue'
import DocumentEntryTemplateDialog from '../components/DocumentEntryTemplateDialog.vue'
import RichTextEditor from '../components/RichTextEditor.vue'
import { backend } from '../services/backend'
import { errorMessage } from '../services/errors'
import { useAppStore } from '../stores/app'
import { useQuestionBankStore } from '../stores/questionBank'
import {
  type DocumentEntryTemplate,
  type DocumentQuestionDraftItem,
  type FreeDocumentQuestionDraftPayload,
  type QuestionDraft,
  type RichContent,
  type WordImportDraftItem,
  type WordImportDraftPayload,
} from '../types/domain'
import { clonePlain } from '../utils/clonePlain'
import {
  DOCUMENT_ENTRY_PARSER_VERSION,
  DOCUMENT_ENTRY_SOURCE_FILE_NAME,
  recognizeLabeledQuestionDocument,
  type DocumentTemplateRecognitionResult,
} from '../utils/documentTemplateRecognition'
import {
  buildDocumentEntryTemplateExample,
  convertDocumentEntryMarkers,
  systemDocumentEntryTemplate,
} from '../utils/documentEntryTemplates'
import {
  emptyRichContent,
  plainTextLinesRichContent,
} from '../utils/richContent'
import { resolveTaxonomyDefault } from '../utils/taxonomyDefaults'
import { questionTypeLabel } from '../utils/questionTypes'

type EditorCommand =
  | 'undo'
  | 'redo'
  | 'bold'
  | 'italic'
  | 'underline'
  | 'strike'
  | 'superscript'
  | 'subscript'
  | 'bulletList'
  | 'orderedList'
  | 'clearFormat'
  | 'image'
  | 'table'
  | 'formula'

interface EditorHandle {
  execute(command: EditorCommand): boolean
  focus(): boolean | void
}

const router = useRouter()
const appStore = useAppStore()
const bankStore = useQuestionBankStore()
const source = ref<RichContent>(emptyRichContent())
const ready = ref(false)
const dirty = ref(false)
const savingDraft = ref(false)
const recognizing = ref(false)
const hasPersistedDraft = ref(false)
const lastAutosavedAt = ref<number | null>(null)
const pageScale = ref(75)
const workspace = ref<HTMLElement | null>(null)
const sourceEditor = ref<EditorHandle | null>(null)
const activeFormat = ref<Record<string, boolean>>({})
const searchExpanded = ref(false)
const searchKeyword = ref('')
const searchInput = ref<HTMLInputElement | null>(null)
const lastRecognition = ref<DocumentTemplateRecognitionResult | null>(null)
const entryTemplates = ref<DocumentEntryTemplate[]>([])
const activeEntryTemplate = ref<DocumentEntryTemplate>(systemDocumentEntryTemplate())
const templateDialogOpen = ref(false)
let autosaveTimer: ReturnType<typeof setInterval> | undefined
let lastAutosavedSnapshot: string | null = null

const sourceCharacterCount = computed(() => source.value.plainText.length)
const sourceLineCount = computed(() => {
  const document = source.value.document as {
    type?: unknown
    content?: Array<{ content?: Array<{ type?: unknown; text?: unknown }> }>
  } | undefined
  if (source.value.schemaVersion !== 2 || document?.type !== 'doc' || !document.content?.length) {
    return source.value.plainText ? source.value.plainText.split(/\r?\n/u).length : 1
  }
  return document.content.reduce((total, node) => (
    total + 1 + (node.content ?? []).reduce((breaks, child) => {
      if (child.type === 'hardBreak') return breaks + 1
      if (child.type === 'text' && typeof child.text === 'string') {
        return breaks + (child.text.match(/\r\n|\r|\n/gu)?.length ?? 0)
      }
      return breaks
    }, 0)
  ), 0)
})
const recognitionErrorCount = computed(() => (
  (lastRecognition.value?.globalIssues.filter((issue) => issue.severity === 'error').length ?? 0)
  + (lastRecognition.value?.questions.reduce(
    (sum, question) => sum + question.issues.filter((issue) => issue.severity === 'error').length,
    0,
  ) ?? 0)
))
const recognitionWarningCount = computed(() => (
  (lastRecognition.value?.globalIssues.filter((issue) => issue.severity === 'warning').length ?? 0)
  + (lastRecognition.value?.questions.reduce(
    (sum, question) => sum + question.issues.filter((issue) => issue.severity === 'warning').length,
    0,
  ) ?? 0)
))

const templateExample = computed(() => (
  buildDocumentEntryTemplateExample(activeEntryTemplate.value.config)
))
const templateOptions = computed(() => {
  const options = [...entryTemplates.value]
  if (!options.some((template) => template.id === activeEntryTemplate.value.id)) {
    options.unshift({
      ...clonePlain(activeEntryTemplate.value),
      name: `${activeEntryTemplate.value.name}（草稿快照）`,
    })
  }
  return options
})

function payloadSnapshot(): FreeDocumentQuestionDraftPayload {
  return {
    schemaVersion: 3,
    templateId: activeEntryTemplate.value.id,
    templateNameSnapshot: activeEntryTemplate.value.name,
    templateConfigSnapshot: clonePlain(activeEntryTemplate.value.config),
    source: clonePlain(source.value),
  }
}

function serializePayload() {
  return JSON.stringify(payloadSnapshot())
}

function updatePendingDraftCount(delta: number) {
  appStore.pendingDraftCount = Math.max(0, appStore.pendingDraftCount + delta)
}

function touch() {
  if (!ready.value) return
  dirty.value = true
  lastRecognition.value = null
}

function appendLabeledField(lines: string[], label: string, value: string) {
  const parts = value.split(/\r?\n/u)
  lines.push(`${label}${parts[0] ?? ''}`)
  lines.push(...parts.slice(1))
}

function legacyItemsToFreeDocument(items: readonly DocumentQuestionDraftItem[]) {
  const lines: string[] = []
  for (const [index, item] of items.entries()) {
    if (index) lines.push('')
    lines.push(`题型：${questionTypeLabel(item.payload.type, appStore.questionTypes)}`)
    appendLabeledField(lines, '题目：', item.payload.stem.plainText)
    item.payload.options.forEach((option, optionIndex) => {
      appendLabeledField(
        lines,
        `${String.fromCharCode(65 + optionIndex)}. `,
        option.content.plainText,
      )
    })
    appendLabeledField(lines, '答案：', item.payload.answer.plainText)
    if (item.payload.explanation.plainText) {
      appendLabeledField(lines, '解析：', item.payload.explanation.plainText)
    }
  }
  return plainTextLinesRichContent(lines)
}

async function autosave(showSuccess = false) {
  if (!ready.value || !dirty.value || savingDraft.value) return true
  const snapshot = serializePayload()
  if (snapshot === lastAutosavedSnapshot) {
    dirty.value = false
    if (showSuccess) ElMessage.success('草稿已经是最新状态')
    return true
  }
  savingDraft.value = true
  try {
    const record = await backend.saveDocumentQuestionDraft(payloadSnapshot())
    if (!hasPersistedDraft.value) updatePendingDraftCount(1)
    hasPersistedDraft.value = true
    lastAutosavedAt.value = record.autosavedAt
    lastAutosavedSnapshot = snapshot
    dirty.value = false
    if (showSuccess) ElMessage.success('自由录题文档已保存到 SQLite')
    return true
  } catch (reason) {
    ElMessage.error(errorMessage(reason, '自由录题文档保存失败，请检查本地数据库。'))
    return false
  } finally {
    savingDraft.value = false
  }
}

async function deletePersistedDraft() {
  await backend.deleteDocumentQuestionDraft()
  if (hasPersistedDraft.value) updatePendingDraftCount(-1)
  hasPersistedDraft.value = false
  lastAutosavedAt.value = null
  lastAutosavedSnapshot = null
}

async function loadEntryTemplates() {
  try {
    const templates = await backend.listDocumentEntryTemplates()
    entryTemplates.value = templates.length ? templates : [systemDocumentEntryTemplate()]
  } catch (reason) {
    entryTemplates.value = [systemDocumentEntryTemplate()]
    ElMessage.warning(errorMessage(reason, '读取录题模板失败，本次将使用系统默认模板。'))
  }
  activeEntryTemplate.value = clonePlain(
    entryTemplates.value.find((template) => template.isDefault)
      ?? entryTemplates.value[0]
      ?? systemDocumentEntryTemplate(),
  )
}

function updateTemplateList(template: DocumentEntryTemplate) {
  if (template.isDefault) {
    entryTemplates.value = entryTemplates.value.map((item) => ({
      ...item,
      isDefault: false,
    }))
  }
  const index = entryTemplates.value.findIndex((item) => item.id === template.id)
  if (index >= 0) entryTemplates.value.splice(index, 1, clonePlain(template))
  else entryTemplates.value.push(clonePlain(template))
}

function onTemplateSaved(template: DocumentEntryTemplate) {
  updateTemplateList(template)
}

function onTemplateDefaulted(template: DocumentEntryTemplate) {
  updateTemplateList(template)
}

function onTemplateDeleted(id: string) {
  const deleted = entryTemplates.value.find((template) => template.id === id)
  entryTemplates.value = entryTemplates.value.filter((template) => template.id !== id)
  if (deleted?.isDefault) {
    entryTemplates.value = entryTemplates.value.map((template) => ({
      ...template,
      isDefault: template.isBuiltIn,
    }))
  }
}

function activateTemplate(template: DocumentEntryTemplate) {
  activeEntryTemplate.value = clonePlain(template)
  touch()
}

async function selectEntryTemplate(id: string) {
  const next = entryTemplates.value.find((template) => template.id === id)
  if (!next) {
    ElMessage.warning('这套录题模板已经不存在，请重新选择。')
    return
  }
  if (
    next.id === activeEntryTemplate.value.id
    && JSON.stringify(next.config) === JSON.stringify(activeEntryTemplate.value.config)
  ) return
  if (!source.value.plainText.trim()) {
    activateTemplate(next)
    return
  }

  const converted = convertDocumentEntryMarkers(
    source.value,
    activeEntryTemplate.value.config,
    next.config,
  )
  try {
    await ElMessageBox.confirm(
      converted.replacementCount
        ? `检测到 ${converted.replacementCount} 处当前模板的行首标记。是否把它们转换成“${next.name}”的格式？`
        : `当前文档中没有找到可自动转换的旧标记。你仍可只切换识别规则，原文字不会改变。`,
      '切换录题模板',
      {
        confirmButtonText: converted.replacementCount ? '转换标记并切换' : '切换识别规则',
        cancelButtonText: '只切换识别规则',
        distinguishCancelAndClose: true,
        type: converted.replacementCount ? 'warning' : 'info',
        closeOnClickModal: false,
      },
    )
    if (converted.replacementCount) source.value = converted.source
    activateTemplate(next)
    if (converted.replacementCount) {
      ElMessage.success(`已安全转换 ${converted.replacementCount} 处行首标记。`)
    }
  } catch (action) {
    if (action === 'cancel') {
      activateTemplate(next)
      ElMessage.info('已切换识别规则，当前文档文字保持不变。')
    }
  }
}

async function load() {
  try {
    await loadEntryTemplates()
    const record = await backend.getDocumentQuestionDraft()
    if (!record) {
      ready.value = true
      return
    }
    hasPersistedDraft.value = true
    lastAutosavedAt.value = record.autosavedAt
    if (record.payload.schemaVersion === 1) {
      try {
        await ElMessageBox.confirm(
          `发现旧版固定输入框草稿，共 ${record.payload.items.length} 道题。是否转换成现在的自由文档模板？`,
          '转换旧版文档录题草稿',
          {
            confirmButtonText: '转换并恢复',
            cancelButtonText: '放弃旧草稿',
            type: 'info',
            closeOnClickModal: false,
          },
        )
        source.value = legacyItemsToFreeDocument(record.payload.items)
        dirty.value = true
      } catch {
        await deletePersistedDraft()
        source.value = emptyRichContent()
      }
    } else {
      try {
        await ElMessageBox.confirm(
          '发现一份尚未识别入库的自由录题文档，是否继续编辑？',
          '恢复文档式录题草稿',
          {
            confirmButtonText: '恢复草稿',
            cancelButtonText: '放弃旧草稿',
            type: 'info',
            closeOnClickModal: false,
          },
        )
        source.value = clonePlain(record.payload.source)
        if (record.payload.schemaVersion === 3) {
          activeEntryTemplate.value = {
            id: record.payload.templateId,
            name: record.payload.templateNameSnapshot,
            config: clonePlain(record.payload.templateConfigSnapshot),
            isBuiltIn: record.payload.templateId === 'labeled_fields_v1',
            isDefault: false,
            createdAt: record.createdAt,
            updatedAt: record.updatedAt,
          }
        } else {
          activeEntryTemplate.value = clonePlain(
            entryTemplates.value.find((template) => template.id === 'labeled_fields_v1')
              ?? systemDocumentEntryTemplate(),
          )
        }
        lastAutosavedSnapshot = JSON.stringify(record.payload)
      } catch {
        await deletePersistedDraft()
        source.value = emptyRichContent()
      }
    }
  } catch (reason) {
    ElMessage.warning(errorMessage(reason, '读取自由录题草稿失败，已打开一张空白文档。'))
    source.value = emptyRichContent()
  } finally {
    ready.value = true
  }
}

function runEditor(command: EditorCommand) {
  if (!sourceEditor.value) {
    ElMessage.info('请先点击中间的空白文档')
    return
  }
  sourceEditor.value.execute(command)
  touch()
}

function setScale(value: number) {
  pageScale.value = Math.max(40, Math.min(150, Math.round(value / 5) * 5))
}

function fitWidth() {
  const available = Math.max(320, (workspace.value?.clientWidth ?? 920) - 54)
  setScale((available / 794) * 100)
}

function fitPage() {
  const availableWidth = Math.max(320, (workspace.value?.clientWidth ?? 920) - 54)
  const availableHeight = Math.max(320, (workspace.value?.clientHeight ?? 720) - 44)
  setScale(Math.min(availableWidth / 794, availableHeight / 1123) * 100)
}

function toggleSearch() {
  searchExpanded.value = !searchExpanded.value
  if (searchExpanded.value) nextTick(() => searchInput.value?.focus())
}

function findInDocument() {
  const keyword = searchKeyword.value.trim().toLocaleLowerCase('zh-CN')
  if (!keyword) return
  const content = source.value.plainText.toLocaleLowerCase('zh-CN')
  const count = content.split(keyword).length - 1
  if (!count) {
    ElMessage.info('当前文档中没有找到该内容')
    return
  }
  ElMessage.success(`当前文档中找到 ${count} 处“${searchKeyword.value.trim()}”`)
  sourceEditor.value?.focus()
}

function questionDiagnostic(
  question: DocumentTemplateRecognitionResult['questions'][number],
) {
  return question.issues.length
    ? question.issues.map((issue) => `第 ${issue.line} 行：${issue.message}`).join('；')
    : null
}

function buildReviewItems(
  result: DocumentTemplateRecognitionResult,
): WordImportDraftItem[] {
  const classification = resolveTaxonomyDefault(
    appStore.subjects,
    bankStore.filters.subjectId,
    bankStore.filters.chapterId,
  )
  return result.questions.map((question) => {
    const hasError = question.issues.some((issue) => issue.severity === 'error')
    const hasWarning = question.issues.some((issue) => issue.severity === 'warning')
    const payload: QuestionDraft = {
      type: question.type,
      stem: clonePlain(question.draft.stem),
      options: clonePlain(question.draft.options),
      answer: clonePlain(question.draft.answer),
      explanation: clonePlain(question.draft.explanation),
      subjectId: classification.subject?.id ?? '',
      chapterId: classification.chapter?.id ?? '',
      tagIds: [],
      resourceRefs: [],
    }
    return {
      id: crypto.randomUUID(),
      ordinal: question.ordinal,
      selected: true,
      payload,
      status: hasError ? 'error' : hasWarning ? 'warning' : 'ok',
      diagnostic: questionDiagnostic(question),
      duplicateKind: 'none',
      candidate: null,
      action: null,
      needsCheck: true,
      checkError: null,
    }
  })
}

async function replaceExistingWordImportDraftIfNeeded() {
  const existing = await backend.getWordImportDraft()
  if (!existing) return true
  try {
    await ElMessageBox.confirm(
      `Word 导入中还有“${existing.payload.sourceFileName}”的未完成审查草稿。继续会删除那份草稿，并用当前文档的识别结果替换。`,
      '替换现有导入审查草稿',
      {
        confirmButtonText: '删除旧草稿并继续',
        cancelButtonText: '返回继续编辑',
        type: 'warning',
        closeOnClickModal: false,
      },
    )
  } catch {
    return false
  }
  await backend.deleteWordImportDraft()
  updatePendingDraftCount(-1)
  return true
}

async function beginRecognition() {
  if (!appStore.license.capabilities.canBatchImport) {
    ElMessage.warning('基础桌面模式不支持批量识别与导入；单题录入仍可正常使用。')
    return
  }
  if (!source.value.plainText.trim()) {
    ElMessage.warning('当前纸张还是空白的，请先按照编辑区内的灰色模板提示手工输入题目内容。')
    sourceEditor.value?.focus()
    return
  }
  const result = recognizeLabeledQuestionDocument(
    source.value,
    activeEntryTemplate.value.config,
  )
  lastRecognition.value = result
  if (!result.questions.length) {
    ElMessage.error(result.globalIssues.find((issue) => issue.severity === 'error')?.message
      ?? '没有识别到题目，请检查模板标签。')
    return
  }

  try {
    await ElMessageBox.confirm(
      `本次识别到 ${result.questions.length} 道题，包含 ${recognitionErrorCount.value} 个必须校对的问题和 ${recognitionWarningCount.value} 个提示。是否进入与 Word 导入相同的识别结果审查页？`,
      '文档识别完成',
      {
        confirmButtonText: '进入识别结果审查',
        cancelButtonText: '继续修改原文档',
        type: recognitionErrorCount.value ? 'warning' : 'success',
        closeOnClickModal: false,
      },
    )
  } catch {
    return
  }

  recognizing.value = true
  try {
    if (!await autosave()) return
    if (!await replaceExistingWordImportDraftIfNeeded()) return
    const reviewItems = buildReviewItems(result)
    const payload: WordImportDraftPayload = {
      schemaVersion: 1,
      sourceFileName: DOCUMENT_ENTRY_SOURCE_FILE_NAME,
      sourceFileSize: new Blob([JSON.stringify(source.value)]).size,
      parserVersion: DOCUMENT_ENTRY_PARSER_VERSION,
      importSessionId: null,
      sourceItemCount: reviewItems.length,
      omittedItemCount: 0,
      activeItemId: reviewItems[0]?.id ?? null,
      items: reviewItems,
    }
    await backend.saveWordImportDraft(payload)
    updatePendingDraftCount(1)
    dirty.value = false
    try {
      await deletePersistedDraft()
    } catch (reason) {
      ElMessage.warning(errorMessage(reason, '识别结果已经安全保存，但原始文档草稿暂时没有清理。'))
    }
    await router.push({
      path: '/word-import',
      query: { source: 'document-entry' },
    })
  } catch (reason) {
    ElMessage.error(errorMessage(reason, '识别结果无法转入审查页，原始文档草稿仍然保留。'))
  } finally {
    recognizing.value = false
  }
}

function warnBeforeBrowserClose(event: BeforeUnloadEvent) {
  if (!dirty.value) return
  event.preventDefault()
  event.returnValue = ''
}

function onKeydown(event: KeyboardEvent) {
  if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 's') {
    event.preventDefault()
    void autosave(true)
  }
  if ((event.ctrlKey || event.metaKey) && event.key === 'Enter') {
    event.preventDefault()
    void beginRecognition()
  }
}

onMounted(() => {
  void load()
  autosaveTimer = setInterval(() => { void autosave() }, 60000)
  window.addEventListener('beforeunload', warnBeforeBrowserClose)
  window.addEventListener('keydown', onKeydown)
})

onBeforeUnmount(() => {
  clearInterval(autosaveTimer)
  window.removeEventListener('beforeunload', warnBeforeBrowserClose)
  window.removeEventListener('keydown', onKeydown)
})

onBeforeRouteLeave(async () => {
  if (!dirty.value) return true
  try {
    await ElMessageBox.confirm(
      '当前自由录题文档尚未保存，离开前保存到 SQLite 草稿吗？',
      '未保存的文档录题内容',
      {
        confirmButtonText: '保存草稿并离开',
        cancelButtonText: '继续编辑',
        type: 'warning',
      },
    )
    return await autosave()
  } catch {
    return false
  }
})
</script>

<template>
  <section class="document-entry-page">
    <el-alert
      v-if="!appStore.license.capabilities.canBatchImport"
      title="当前为基础桌面模式：可继续编辑和保存这份本地草稿，但批量识别与写入题库需要桌面专业版。"
      type="info"
      :closable="false"
      show-icon
    />
    <header class="entry-page-header">
      <div class="entry-header-left">
        <el-button :icon="Back" @click="router.push('/questions/new')">表单录入</el-button>
        <span>录题模板</span>
        <el-select
          :model-value="activeEntryTemplate.id"
          class="entry-template-select"
          size="small"
          @change="selectEntryTemplate"
        >
          <el-option
            v-for="template in templateOptions"
            :key="template.id"
            :label="template.name"
            :value="template.id"
          />
        </el-select>
        <el-button
          class="entry-template-settings"
          :icon="Setting"
          circle
          title="设置录题模板"
          aria-label="设置录题模板"
          @click="templateDialogOpen = true"
        />
      </div>
      <strong>文档式批量录入</strong>
      <div class="entry-header-actions">
        <span class="autosave-status">
          <i />
          <template v-if="savingDraft">正在保存草稿…</template>
          <template v-else-if="lastAutosavedAt">
            草稿已于 {{ new Date(lastAutosavedAt).toLocaleTimeString('zh-CN') }} 保存
          </template>
          <template v-else>每 60 秒自动保存到 SQLite</template>
        </span>
        <el-button :loading="savingDraft" @click="autosave(true)">保存草稿</el-button>
        <el-button
          type="primary"
          :icon="DocumentAdd"
          :loading="recognizing"
          :disabled="!appStore.license.capabilities.canBatchImport"
          @click="beginRecognition"
        >
          开始识别
        </el-button>
      </div>
    </header>

    <div class="entry-workspace-shell">
      <section class="entry-editor-main">
        <div class="entry-toolbar" aria-label="文档式录题工具栏">
          <button
            type="button"
            title="撤销"
            :disabled="!activeFormat.undo"
            @click="runEditor('undo')"
          ><RefreshLeft /></button>
          <button
            type="button"
            title="重做"
            :disabled="!activeFormat.redo"
            @click="runEditor('redo')"
          ><RefreshRight /></button>
          <button type="button" title="清除格式" @click="runEditor('clearFormat')"><Brush /></button>
          <span class="toolbar-separator" />
          <button
            type="button"
            title="加粗"
            :class="{ active: activeFormat.bold }"
            @click="runEditor('bold')"
          ><strong>B</strong></button>
          <button
            type="button"
            title="斜体"
            :class="{ active: activeFormat.italic }"
            @click="runEditor('italic')"
          ><em>I</em></button>
          <button
            type="button"
            title="下划线"
            :class="{ active: activeFormat.underline }"
            @click="runEditor('underline')"
          ><u>U</u></button>
          <button
            type="button"
            title="删除线"
            :class="{ active: activeFormat.strike }"
            @click="runEditor('strike')"
          ><s>S</s></button>
          <button
            type="button"
            title="上标"
            :class="{ active: activeFormat.superscript }"
            @click="runEditor('superscript')"
          >X²</button>
          <button
            type="button"
            title="下标"
            :class="{ active: activeFormat.subscript }"
            @click="runEditor('subscript')"
          >X₂</button>
          <span class="toolbar-separator" />
          <button
            type="button"
            :class="{ active: activeFormat.bulletList }"
            @click="runEditor('bulletList')"
          >• 列表</button>
          <button
            type="button"
            :class="{ active: activeFormat.orderedList }"
            @click="runEditor('orderedList')"
          >1. 列表</button>
          <button type="button" title="插入图片" @click="runEditor('image')"><Picture /></button>
          <button type="button" title="插入表格" @click="runEditor('table')"><Grid /></button>
          <button type="button" title="插入公式" @click="runEditor('formula')"><Operation /></button>
          <span class="toolbar-separator" />
          <div class="search-tool">
            <button type="button" title="查找" @click="toggleSearch"><Search /></button>
            <input
              v-if="searchExpanded"
              ref="searchInput"
              v-model="searchKeyword"
              placeholder="查找文档内容"
              @keyup.enter="findInDocument"
            >
          </div>
          <span class="toolbar-grow" />
          <button type="button" class="recognize-toolbar-button" @click="beginRecognition">
            <DocumentAdd />识别
          </button>
        </div>

        <div ref="workspace" class="entry-document-workspace">
          <article
            class="entry-paper"
            :style="{ zoom: pageScale / 100 }"
            aria-label="自由文档录题纸张"
          >
            <div class="source-document-editor">
              <RichTextEditor
                ref="sourceEditor"
                v-model="source"
                toolbar-mode="hidden"
                :min-height="1000"
                :placeholder="templateExample"
                @focus="() => undefined"
                @format-change="(state) => { activeFormat = state }"
                @update:model-value="touch"
              />
            </div>
          </article>
        </div>

        <footer class="entry-statusbar">
          <span>行数：{{ sourceLineCount }}</span>
          <span>字数：{{ sourceCharacterCount }}</span>
          <strong>自由文档编辑模式</strong>
          <div>
            <button type="button" @click="setScale(pageScale - 5)">−</button>
            <input
              :value="pageScale"
              type="range"
              min="40"
              max="150"
              step="5"
              @input="setScale(Number(($event.target as HTMLInputElement).value))"
            >
            <button type="button" class="scale-value" @click="setScale(100)">{{ pageScale }}%</button>
            <button type="button" @click="setScale(pageScale + 5)">＋</button>
            <button type="button" class="fit-button" @click="fitPage">适合页面</button>
            <button type="button" class="fit-button" @click="fitWidth">适合宽度</button>
          </div>
        </footer>
      </section>
    </div>

    <DocumentEntryTemplateDialog
      v-model="templateDialogOpen"
      :templates="entryTemplates"
      :active-template-id="activeEntryTemplate.id"
      @saved="onTemplateSaved"
      @defaulted="onTemplateDefaulted"
      @deleted="onTemplateDeleted"
      @select="selectEntryTemplate"
    />
  </section>
</template>

<style scoped>
.document-entry-page {
  width: 0;
  height: 100%;
  flex: 1 1 0;
  min-width: 0;
  min-height: 0;
  display: grid;
  grid-template-rows: 58px minmax(0, 1fr);
  background: #e9eef5;
  overflow: hidden;
}

.entry-page-header {
  padding: 0 22px;
  display: grid;
  grid-template-columns: 1fr auto 1fr;
  align-items: center;
  border-bottom: 1px solid #dbe3ed;
  background: #fff;
}

.entry-header-left {
  justify-self: start;
  min-width: 0;
  display: flex;
  align-items: center;
  gap: 7px;
}

.entry-header-left > span {
  color: #64748b;
  font-size: 10px;
  white-space: nowrap;
}

.entry-template-select {
  width: 155px;
}

.entry-template-settings {
  flex: 0 0 auto;
}

.entry-page-header > strong {
  justify-self: center;
  color: #172033;
  font-size: 15px;
}

.entry-header-actions {
  justify-self: end;
  display: flex;
  align-items: center;
  gap: 8px;
}

.autosave-status {
  margin-right: 4px;
  display: inline-flex;
  align-items: center;
  gap: 6px;
  color: #64748b;
  font-size: 11px;
  white-space: nowrap;
}

.autosave-status i {
  width: 7px;
  height: 7px;
  border-radius: 50%;
  background: #22c55e;
}

.entry-workspace-shell {
  min-width: 0;
  min-height: 0;
  padding: 14px;
  display: block;
  overflow: hidden;
}

.entry-editor-main {
  height: 100%;
  min-width: 0;
  min-height: 0;
  display: grid;
  grid-template-rows: 40px minmax(0, 1fr) 34px;
  overflow: hidden;
  border: 1px solid #d9e2ee;
  border-radius: 10px;
  background: #eef2f7;
}

.entry-toolbar {
  position: relative;
  z-index: 12;
  padding: 4px 7px;
  display: flex;
  align-items: center;
  flex-wrap: nowrap;
  gap: 2px;
  border-bottom: 1px solid #d9e2ee;
  background: #fff;
}

.entry-toolbar button {
  box-sizing: border-box;
  min-width: 26px;
  height: 28px;
  padding: 0 6px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 4px;
  border: 1px solid transparent;
  border-radius: 5px;
  background: #fff;
  color: #334155;
  font-size: 12px;
  white-space: nowrap;
  cursor: pointer;
}

.entry-toolbar button:hover:not(:disabled),
.entry-toolbar button.active {
  border-color: #bfdbfe;
  background: #eff6ff;
  color: #1d4ed8;
}

.entry-toolbar button:disabled {
  cursor: not-allowed;
  opacity: .38;
}

.entry-toolbar button :deep(svg) {
  width: 16px;
  height: 16px;
}

.toolbar-separator {
  flex: 0 0 1px;
  width: 1px;
  height: 20px;
  margin: 0 2px;
  background: #dbe3ee;
}

.toolbar-grow {
  margin-left: auto;
}

.entry-toolbar .recognize-toolbar-button {
  border-color: #2563eb;
  background: #2563eb;
  color: #fff;
}

.search-tool {
  position: relative;
}

.search-tool input {
  position: absolute;
  z-index: 30;
  top: 32px;
  right: 0;
  width: 190px;
  height: 30px;
  box-sizing: border-box;
  padding: 0 9px;
  border: 1px solid #cbd5e1;
  border-radius: 6px;
  outline: none;
  background: #fff;
}

.entry-document-workspace {
  min-width: 0;
  min-height: 0;
  padding: 18px 26px 32px;
  overflow: auto;
  text-align: center;
  background: #e8edf4;
}

.entry-paper {
  width: min(980px, calc(100% - 48px));
  min-height: 1123px;
  box-sizing: border-box;
  margin: 0 auto;
  padding: 72px 72px 82px;
  text-align: left;
  background: #fff;
  box-shadow: 0 2px 12px rgb(15 23 42 / 13%);
  transform-origin: top center;
}

.source-document-editor :deep(.rich-editor) {
  border: 0;
  border-radius: 0;
  box-shadow: none;
}

.source-document-editor :deep(.rich-editor__content) {
  padding: 0;
}

.source-document-editor :deep(.tiptap) {
  min-height: 960px !important;
  color: #111827;
  font-family: SimSun, "宋体", serif;
  font-size: 16px;
  line-height: 1.8;
}

.source-document-editor :deep(.tiptap p) {
  margin: 0 0 8px;
}

.source-document-editor :deep(.tiptap p.is-editor-empty:first-child::before) {
  color: #9ca3af;
  font-family: "Microsoft YaHei", sans-serif;
  font-size: 14px;
  line-height: 1.85;
  white-space: pre-line;
}

.entry-statusbar {
  padding: 0 10px;
  display: grid;
  grid-template-columns: auto auto 1fr auto;
  align-items: center;
  gap: 16px;
  border-top: 1px solid #d9e2ee;
  background: #fff;
  color: #64748b;
  font-size: 11px;
}

.entry-statusbar > strong {
  justify-self: center;
  color: #64748b;
  font-weight: 500;
}

.entry-statusbar > div {
  display: flex;
  align-items: center;
  gap: 5px;
}

.entry-statusbar button {
  height: 25px;
  padding: 0 6px;
  border: 0;
  border-radius: 4px;
  background: transparent;
  color: #475569;
  cursor: pointer;
}

.entry-statusbar button:hover {
  background: #eef2f7;
}

.entry-statusbar input[type="range"] {
  width: 90px;
  accent-color: #2563eb;
}

.entry-statusbar .scale-value {
  min-width: 42px;
}

.entry-statusbar .fit-button {
  border: 1px solid #dbe3ee;
  background: #fff;
}

@media (max-width: 1180px) {
  .autosave-status {
    display: none;
  }

  .entry-toolbar button {
    padding: 0 4px;
  }
}
</style>
