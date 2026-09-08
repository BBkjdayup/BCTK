<script setup lang="ts">
import { computed, defineAsyncComponent, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useRoute } from 'vue-router'
import { ElMessage, ElMessageBox } from 'element-plus'
import { MagicStick, View, Delete, Top, Bottom, DocumentChecked, Back, Rank } from '@element-plus/icons-vue'
import Sortable from 'sortablejs'
import SubjectTree from '../components/SubjectTree.vue'
import type { PaperExportPreparation } from '../components/PaperExportDialog.vue'
import AppContextMenu from '../components/AppContextMenu.vue'
import QuestionStemSummary from '../components/QuestionStemSummary.vue'
import { useAppStore } from '../stores/app'
import { configurePaperQuestionTypeOrder, usePaperStore } from '../stores/paper'
import { createQuestionFilters, useQuestionBankStore } from '../stores/questionBank'
import { errorMessage } from '../services/errors'
import { backend } from '../services/backend'
import {
  fetchAllMatchingQuestions,
} from '../utils/paperSelection'
import { applyPaperFormulaChange } from '../utils/paperFormula'
import type { PaperCanvasFormulaChange } from '../utils/paperLayout'
import {
  type Paper,
  type PaperItem,
  type Question,
  type QuestionType,
  type TemplateLayoutPreview,
  type WordTemplate,
} from '../types/domain'
import type { ContextMenuItem } from '../types/contextMenu'
import { effectiveQuestionTypes, enabledQuestionTypes, questionTypeLabel } from '../utils/questionTypes'
import { resolvePaperBuilderEntryMode } from '../utils/paperBuilderNavigation'

type Mode = 'manual' | 'edit' | 'preview'
const PaperExportDialog = defineAsyncComponent(() => import('../components/PaperExportDialog.vue'))
const PaperCanvasEditor = defineAsyncComponent(() => import('../components/PaperCanvasEditor.vue'))
const QuestionPreviewDrawer = defineAsyncComponent(() => import('../components/QuestionPreviewDrawer.vue'))
const RandomDrawDialog = defineAsyncComponent(() => import('../components/RandomDrawDialog.vue'))

const appStore = useAppStore()
const bankStore = useQuestionBankStore()
const paperStore = usePaperStore()
const route = useRoute()
const mode = ref<Mode>('manual')
const exportDialogOpen = ref(false)
const previewTemplates = ref<WordTemplate[]>([])
const selectedPreviewTemplateId = ref('')
const previewTemplate = ref<TemplateLayoutPreview | null>(null)
const previewTemplateLoading = ref(false)
const previewTemplateError = ref('')
const previewEntering = ref(false)
const canvasEditorKey = ref(0)
const autoSubjectId = ref('')
const autoChapterIds = ref<string[]>([])
const replacementDialogOpen = ref(false)
const replacementTarget = ref<PaperItem | null>(null)
const replacementCandidates = ref<Question[]>([])
const replacementQuestionId = ref('')
const replacementLoading = ref(false)
const replacementError = ref('')
const replacementPage = ref(1)
const replacementPageSize = 10
const paperItemsRoot = ref<HTMLElement | null>(null)
const selectedListRoot = ref<HTMLElement | null>(null)
const questionPreviewOpen = ref(false)
const previewQuestion = ref<Question | null>(null)
const questionContextMenuOpen = ref(false)
const questionContextMenuX = ref(0)
const questionContextMenuY = ref(0)
const questionContextMenuTitle = ref('')
const contextQuestion = ref<Question | null>(null)
const questionContextMenuItems: ContextMenuItem[] = [
  { id: 'view-question', label: '查看试题内容' },
]
let paperSortables: Sortable[] = []
let replacementLoadSequence = 0
let previewTemplateLoadSequence = 0

const questionTypeDefinitions = computed(() => effectiveQuestionTypes(appStore.questionTypes))
const enabledTypeDefinitions = computed(() => enabledQuestionTypes(appStore.questionTypes))
const questionTypeCodes = computed(() => questionTypeDefinitions.value.map((definition) => definition.code))

function typeName(type: QuestionType) {
  return questionTypeLabel(type, appStore.questionTypes)
}

const batchDrawDialogOpen = ref(false)
const batchDrawDialogMounted = ref(false)

bankStore.initializeFilters({ deleted: false, pageSize: 50 })
paperStore.normalizeCurrentItems()

const paperQuestionLimit = computed(() => appStore.license.capabilities.maxQuestionsPerPaper)
const paperLimitReached = computed(() => (
  paperQuestionLimit.value != null && paperStore.current.items.length >= paperQuestionLimit.value
))
const paperOverLimit = computed(() => (
  paperQuestionLimit.value != null && paperStore.current.items.length > paperQuestionLimit.value
))
const typeCounts = computed(() => paperStore.current.items.reduce<Record<string, number>>((counts, item) => {
  counts[item.snapshot.type] = (counts[item.snapshot.type] ?? 0) + 1
  return counts
}, {}))
const groupedItems = computed(() => questionTypeCodes.value
  .map((type) => ({ type, items: paperStore.current.items.filter((item) => item.snapshot.type === type) }))
  .filter((group) => group.items.length))
const randomDrawRemainingCapacity = computed(() => paperQuestionLimit.value == null
  ? null
  : Math.max(0, paperQuestionLimit.value - paperStore.current.items.length))
const replacementPageItems = computed(() => {
  const start = (replacementPage.value - 1) * replacementPageSize
  return replacementCandidates.value.slice(start, start + replacementPageSize)
})
const replacementScopeText = computed(() => {
  if (paperStore.current.compositionMode !== 'automatic') {
    return '沿用手动选题页当前的关键字、学科、章节、标签和使用情况筛选，并限定为同一题型。'
  }
  return paperStore.current.generationConfig
    ? '使用这份试卷已保存的自动组卷学科和章节范围，并始终限定为同一题型。'
    : '这是一份旧自动试卷，当时没有保存学科和章节范围；替换时会从全部题库中查找同一题型。'
})
const selectedPreviewTemplate = computed(() => previewTemplates.value.find(
  (template) => template.id === selectedPreviewTemplateId.value,
) ?? null)
const previewPageDescription = computed(() => {
  if (!previewTemplate.value) return '标准 A4 页面'
  const width = previewTemplate.value.page.widthTwips / 1440 * 25.4
  const height = previewTemplate.value.page.heightTwips / 1440 * 25.4
  const columns = previewTemplate.value.page.columnCount > 1
    ? `，${previewTemplate.value.page.columnCount} 栏`
    : '，单栏'
  const pageNumber = previewTemplate.value.pageNumberFormat ? '，保留动态页码' : ''
  return `${width.toFixed(1)} × ${height.toFixed(1)} 毫米${columns}${pageNumber}`
})

function destroyPaperSortables() {
  paperSortables.forEach((sortable) => sortable.destroy())
  paperSortables = []
}

async function initializePaperSortables() {
  await nextTick()
  destroyPaperSortables()

  if (mode.value === 'manual' && selectedListRoot.value) {
    const container = selectedListRoot.value
    paperSortables.push(new Sortable(container, {
      animation: 180,
      draggable: '.selected-row',
      filter: '.selected-row .el-button, .selected-row .el-button *',
      preventOnFilter: false,
      delay: 400,
      delayOnTouchOnly: false,
      touchStartThreshold: 5,
      forceFallback: true,
      fallbackTolerance: 5,
      chosenClass: 'is-drag-chosen',
      ghostClass: 'is-drag-ghost',
      dragClass: 'is-dragging',
      onMove: (event) => {
        const draggedType = (event.dragged as HTMLElement).dataset.questionType
        const relatedType = (event.related as HTMLElement).dataset.questionType
        return Boolean(draggedType && draggedType === relatedType)
      },
      onEnd: (event) => {
        const type = (event.item as HTMLElement).dataset.questionType as QuestionType | undefined
        if (!type) return
        const orderedIds = [...container.querySelectorAll<HTMLElement>(
          `[data-question-type="${type}"][data-paper-item-id]`,
        )]
          .map((element) => element.dataset.paperItemId)
          .filter((id): id is string => Boolean(id))
        paperStore.reorderItemsWithinType(type, orderedIds)
      },
    }))
  }

  if (mode.value !== 'edit' || !paperItemsRoot.value) return

  paperItemsRoot.value.querySelectorAll<HTMLElement>('.paper-sortable-list').forEach((container) => {
    const type = container.dataset.questionType as QuestionType | undefined
    if (!type) return
    paperSortables.push(new Sortable(container, {
      animation: 180,
      draggable: '.paper-item-row',
      filter: '.paper-item-actions, .paper-item-actions *',
      preventOnFilter: false,
      delay: 400,
      delayOnTouchOnly: false,
      touchStartThreshold: 5,
      forceFallback: true,
      fallbackTolerance: 5,
      chosenClass: 'is-drag-chosen',
      ghostClass: 'is-drag-ghost',
      dragClass: 'is-dragging',
      onEnd: () => {
        const orderedIds = [...container.querySelectorAll<HTMLElement>('[data-paper-item-id]')]
          .map((element) => element.dataset.paperItemId)
          .filter((id): id is string => Boolean(id))
        paperStore.reorderItemsWithinType(type, orderedIds)
      },
    }))
  })
}

onMounted(() => {
  void bankStore.load()
  const config = paperStore.current.generationConfig
  if (paperStore.current.compositionMode === 'automatic' && config) {
    autoSubjectId.value = config.subjectId ?? ''
    autoChapterIds.value = [...config.chapterIds]
  }
  mode.value = resolvePaperBuilderEntryMode(
    route.query.view,
    paperStore.current.rowVersion > 0 || paperStore.current.items.length > 0,
  )
  void initializePaperSortables()
})

onBeforeUnmount(destroyPaperSortables)

watch(
  () => [
    mode.value,
    groupedItems.value.map((group) => `${group.type}:${group.items.map((item) => item.id).join(',')}`).join('|'),
  ],
  () => void initializePaperSortables(),
  { flush: 'post' },
)

watch(questionTypeCodes, (codes) => {
  configurePaperQuestionTypeOrder(codes)
  paperStore.normalizeCurrentItems()
}, { immediate: true })

function selectTree(payload: { subjectId?: string; chapterId?: string }) {
  bankStore.filters.subjectId = payload.subjectId
  bankStore.filters.chapterId = payload.chapterId
  bankStore.filters.page = 1
  void bankStore.load()
}

function applyManualFilters() {
  bankStore.filters.page = 1
  void bankStore.load()
}

function loadManualPage(page: number) {
  bankStore.filters.page = page
  void bankStore.load()
}

function changeManualPageSize(pageSize: number) {
  bankStore.filters.pageSize = pageSize
  bankStore.filters.page = 1
  void bankStore.load()
}

function inAutomaticChapterScope(question: Question) {
  return !autoChapterIds.value.length || autoChapterIds.value.includes(question.chapterId)
}

function openBatchDrawDialog() {
  batchDrawDialogMounted.value = true
  batchDrawDialogOpen.value = true
}

function addRandomQuestions(selected: Question[]) {
  paperStore.current.compositionMode = 'manual'
  paperStore.current.generationConfig = null
  const added = paperStore.addQuestions(selected, paperQuestionLimit.value)
  if (!added) {
    ElMessage.warning('本次抽到的题目均已在右侧，未重复加入。')
  }
}

function templateCanPreview(template: WordTemplate) {
  return template.fileAvailable
    && template.regionConfigured
    && template.packageKind === 'document'
    && template.analysisStatus !== 'failed'
}

function templateOptionLabel(template: WordTemplate) {
  if (!template.fileAvailable) return `${template.name}（文件缺失）`
  if (!template.regionConfigured) return `${template.name}（未配置试题区域）`
  if (template.packageKind !== 'document') return `${template.name}（不是 DOCX 文档）`
  if (template.analysisStatus === 'failed') return `${template.name}（分析失败）`
  return template.isDefault ? `${template.name}（默认）` : template.name
}

async function applyPreviewTemplate(templateId: string, resetLayout: boolean) {
  const requestSequence = ++previewTemplateLoadSequence
  previewTemplateLoading.value = true
  previewTemplateError.value = ''
  try {
    const loadedPreview = templateId
      ? await backend.getTemplateLayoutPreview(templateId)
      : null
    if (requestSequence !== previewTemplateLoadSequence) return
    previewTemplate.value = loadedPreview
    selectedPreviewTemplateId.value = templateId
    paperStore.current.preferredTemplateId = templateId || null
    if (resetLayout) {
      paperStore.current.layout = null
      paperStore.current.layoutUpdatedAt = Date.now()
    }
    paperStore.current.updatedAt = Date.now()
    // Entering preview loads the template before the canvas is mounted. Only
    // force a remount when an already-visible canvas changes templates.
    if (mode.value === 'preview') canvasEditorKey.value += 1
  } catch (reason) {
    if (requestSequence !== previewTemplateLoadSequence) return
    previewTemplateError.value = errorMessage(reason, '读取模板排版预览失败')
    throw reason
  } finally {
    if (requestSequence === previewTemplateLoadSequence) previewTemplateLoading.value = false
  }
}

async function loadPreviewTemplates() {
  previewTemplateLoading.value = true
  previewTemplateError.value = ''
  try {
    const [templates, settings] = await Promise.all([
      backend.listTemplates(),
      backend.getSettings(),
    ])
    previewTemplates.value = templates
    const usable = templates.filter(templateCanPreview)
    const currentId = paperStore.current.preferredTemplateId ?? ''
    const target = usable.find((template) => template.id === currentId)
      ?? usable.find((template) => template.id === settings.defaultTemplateId)
      ?? usable.find((template) => template.isDefault)
      ?? usable[0]
    const targetId = target?.id ?? ''
    const changedTemplate = targetId !== currentId
    await applyPreviewTemplate(targetId, changedTemplate)
  } catch (reason) {
    previewTemplateError.value = errorMessage(reason, '读取可用模板失败')
    ElMessage.error(previewTemplateError.value)
  } finally {
    previewTemplateLoading.value = false
  }
}

async function enterPreview() {
  if (previewEntering.value) return
  previewEntering.value = true
  try {
    // Loading first prevents a large paper from being laid out once without a
    // template and then immediately laid out a second time with the template.
    await loadPreviewTemplates()
    mode.value = 'preview'
  } finally {
    previewEntering.value = false
  }
}

async function changePreviewTemplate(templateId: string) {
  const previousId = paperStore.current.preferredTemplateId ?? ''
  if (templateId === previousId) return
  if (paperStore.current.layout) {
    try {
      await ElMessageBox.confirm(
        '切换模板会按新模板重新生成当前画布，已经手工调整的分页和样式会被覆盖。是否继续？',
        '切换排版模板',
        {
          confirmButtonText: '切换并重新排版',
          cancelButtonText: '取消',
          type: 'warning',
        },
      )
    } catch {
      selectedPreviewTemplateId.value = previousId
      return
    }
  }
  try {
    await applyPreviewTemplate(templateId, true)
    ElMessage.success(templateId ? '已按所选模板重新生成预览' : '已恢复标准 A4 排版')
  } catch (reason) {
    selectedPreviewTemplateId.value = previousId
    ElMessage.error(errorMessage(reason, '切换模板失败'))
  }
}

function addQuestion(question: Question) {
  if (paperLimitReached.value) {
    ElMessage.warning(`基础桌面模式每份试卷最多加入 ${paperQuestionLimit.value} 道题。`)
    return
  }
  paperStore.current.compositionMode = 'manual'
  paperStore.current.generationConfig = null
  const added = paperStore.addQuestions([question], paperQuestionLimit.value)
  if (added) ElMessage.success('已加入当前试卷')
}

function openQuestionContextMenu(event: MouseEvent, question: Question) {
  const target = event.target as HTMLElement | null
  if (target?.closest('button, input, textarea, select, a, [contenteditable="true"], [role="button"], [role="textbox"]')) return
  event.preventDefault()
  contextQuestion.value = question
  questionContextMenuTitle.value = question.stem.plainText || '当前题目'
  questionContextMenuX.value = event.clientX
  questionContextMenuY.value = event.clientY
  questionContextMenuOpen.value = true
}

function handleQuestionContextMenu(item: ContextMenuItem) {
  if (item.id !== 'view-question' || !contextQuestion.value) return
  previewQuestion.value = contextQuestion.value
  questionPreviewOpen.value = true
}

async function clearPaper() {
  if (!paperStore.current.items.length) return
  try {
    await ElMessageBox.confirm('清空当前试卷中的全部题目？题库原题不会被删除。', '清空当前试卷', {
      type: 'warning', confirmButtonText: '清空', cancelButtonText: '取消',
    })
  } catch {
    return
  }
  paperStore.clear()
}

function replacementReadFilters(target: PaperItem) {
  if (paperStore.current.compositionMode === 'automatic') {
    return createQuestionFilters({
      subjectId: autoSubjectId.value || undefined,
      chapterId: autoChapterIds.value.length === 1 ? autoChapterIds.value[0] : undefined,
      type: target.snapshot.type,
      deleted: false,
      pageSize: 100,
    })
  }
  return {
    ...bankStore.filters,
    tagIds: [...bankStore.filters.tagIds],
    type: target.snapshot.type,
    deleted: false,
    page: 1,
    pageSize: 100,
  }
}

async function openReplacement(target: PaperItem) {
  const requestSequence = ++replacementLoadSequence
  replacementTarget.value = target
  replacementCandidates.value = []
  replacementQuestionId.value = ''
  replacementPage.value = 1
  replacementError.value = ''
  replacementLoading.value = true
  replacementDialogOpen.value = true
  try {
    let candidates = await fetchAllMatchingQuestions(backend.listQuestions, replacementReadFilters(target))
    if (paperStore.current.compositionMode === 'automatic' && autoChapterIds.value.length) {
      candidates = candidates.filter(inAutomaticChapterScope)
    }
    const selectedIds = new Set(paperStore.current.items.map((item) => (
      item.sourceQuestionId ?? item.snapshot.id
    )))
    candidates = candidates.filter((question) => !selectedIds.has(question.id))
    if (requestSequence !== replacementLoadSequence) return
    replacementCandidates.value = candidates
    if (!candidates.length) ElMessage.info('当前筛选范围内没有可用于替换的新题。')
  } catch (reason) {
    if (requestSequence !== replacementLoadSequence) return
    replacementError.value = errorMessage(reason, '读取替换候选失败')
    ElMessage.error(replacementError.value)
  } finally {
    if (requestSequence === replacementLoadSequence) replacementLoading.value = false
  }
}

function confirmReplacement() {
  const target = replacementTarget.value
  const replacement = replacementCandidates.value.find((question) => question.id === replacementQuestionId.value)
  if (!target || !replacement) {
    ElMessage.warning('请先选择一道替换题目。')
    return
  }
  const result = paperStore.replaceItem(target.id, replacement)
  if (!result.replaced) {
    const messages = {
      target_not_found: '原题已不在当前试卷中，请重新打开替换窗口。',
      same_question: '新题不能与原题相同。',
      type_mismatch: '只能使用同一题型的题目进行替换。',
      duplicate_question: '这道题已经在当前试卷中，不能重复加入。',
    }
    ElMessage.warning(messages[result.reason])
    return
  }
  replacementDialogOpen.value = false
  ElMessage.success('题目已替换，原位置和题目级设置保持不变。')
}

function resetReplacementDialog() {
  replacementLoadSequence += 1
  replacementTarget.value = null
  replacementCandidates.value = []
  replacementQuestionId.value = ''
  replacementError.value = ''
  replacementLoading.value = false
  replacementPage.value = 1
}

async function savePaper(status: 'draft' | 'saved') {
  if (status === 'saved' && !paperStore.current.items.length) {
    ElMessage.warning('当前试卷还没有题目')
    return
  }
  if (paperOverLimit.value) {
    ElMessage.warning(`这份旧试卷有 ${paperStore.current.items.length} 道题；基础桌面模式需先删减到 ${paperQuestionLimit.value} 道以内才能保存。`)
    return
  }
  try {
    await paperStore.save(status)
    ElMessage.success(status === 'saved' ? '试卷与题目快照已保存到历史记录' : '试卷草稿已保存')
  } catch (reason) {
    ElMessage.error(errorMessage(reason, '保存试卷失败'))
  }
}

function onCanvasLayoutChanged() {
  const timestamp = Date.now()
  paperStore.current.layoutUpdatedAt = timestamp
  paperStore.current.updatedAt = timestamp
}

function onCanvasFormulaChanged(change: PaperCanvasFormulaChange) {
  if (!applyPaperFormulaChange(paperStore.current, change)) {
    ElMessage.error('公式未能写入当前试卷，请重新生成排版后重试。')
  }
}

async function preparePaperForExport(preparation: PaperExportPreparation): Promise<Paper> {
  paperStore.current.title = preparation.title
  paperStore.current.exportContentMode = preparation.contentMode
  if (preparation.templateId !== (paperStore.current.preferredTemplateId ?? '')) {
    await applyPreviewTemplate(preparation.templateId, true)
  } else {
    paperStore.current.preferredTemplateId = preparation.templateId
  }
  return paperStore.save('saved')
}
</script>

<template>
  <div v-if="mode === 'manual'" class="app-page">
    <SubjectTree
      title="学科与章节"
      :selected-subject-id="bankStore.filters.subjectId"
      :selected-chapter-id="bankStore.filters.chapterId"
      @select="selectTree"
    />
    <section class="page-main manual-page">
      <div class="manual-layout">
        <div class="candidate-card surface">
          <div class="candidate-filter">
            <el-input v-model="bankStore.filters.keyword" placeholder="搜索题目" clearable @change="applyManualFilters" />
            <el-select v-model="bankStore.filters.type" clearable placeholder="全部题型" @change="applyManualFilters">
              <el-option v-for="definition in enabledTypeDefinitions" :key="definition.code" :label="definition.name" :value="definition.code" />
            </el-select>
          </div>
          <div v-loading="bankStore.loading" class="candidate-list">
            <div
              v-for="question in bankStore.questions"
              :key="question.id"
              class="candidate-row"
              @contextmenu="openQuestionContextMenu($event, question)"
            >
              <el-tag size="small" effect="plain">{{ typeName(question.type) }}</el-tag>
              <div>
                <QuestionStemSummary class="candidate-row__stem" :content="question.stem" :lines="2" />
                <span>{{ question.subjectName }} · {{ question.chapterName }}</span>
              </div>
              <el-button
                size="small"
                :disabled="paperStore.selectedQuestionIds.has(question.id) || paperLimitReached"
                @click="addQuestion(question)"
              >{{ paperStore.selectedQuestionIds.has(question.id) ? '已加入' : paperLimitReached ? '已达上限' : '加入' }}</el-button>
            </div>
            <div v-if="!bankStore.loading && !bankStore.questions.length" class="candidate-empty">
              当前页没有符合条件的题目，可以调整筛选条件。
            </div>
          </div>
          <div class="candidate-pagination">
            <span>共 {{ bankStore.total }} 道，当前第 {{ bankStore.filters.page }} 页</span>
            <el-pagination
              :current-page="bankStore.filters.page"
              :page-size="bankStore.filters.pageSize"
              :page-sizes="[20, 50, 100]"
              :total="bankStore.total"
              background
              layout="sizes, prev, pager, next"
              @current-change="loadManualPage"
              @size-change="changeManualPageSize"
            />
          </div>
        </div>
        <aside class="selected-panel surface">
          <div class="selected-panel__header">
            <strong>已选题目 <span>{{ paperStore.current.items.length }}</span></strong>
            <div class="selected-panel__actions">
              <el-button plain type="primary" size="small" :icon="MagicStick" @click="openBatchDrawDialog">随机抽题</el-button>
              <el-button text type="danger" size="small" @click="clearPaper">清空</el-button>
            </div>
          </div>
          <el-alert
            v-if="paperQuestionLimit != null"
            :title="`基础桌面模式：每份试卷最多 ${paperQuestionLimit} 道题，当前 ${paperStore.current.items.length} 道。`"
            :type="paperOverLimit ? 'warning' : 'info'"
            :closable="false"
            show-icon
          />
          <el-input v-model="paperStore.current.title" placeholder="试卷标题（可稍后修改）" />
          <div ref="selectedListRoot" class="selected-list">
            <div
              v-for="(item, index) in paperStore.current.items"
              :key="item.id"
              class="selected-row"
              :data-paper-item-id="item.id"
              :data-question-type="item.snapshot.type"
              title="长按题目区域约 0.4 秒后拖动排序"
              @contextmenu="openQuestionContextMenu($event, item.snapshot)"
            >
              <span>{{ index + 1 }}</span>
              <QuestionStemSummary :content="item.snapshot.stem" :lines="2" />
              <el-button text circle type="danger" :icon="Delete" @click="paperStore.removeItem(item.id)" />
            </div>
            <div v-if="!paperStore.current.items.length" class="selected-empty">从左侧加入题目后，会在这里显示。</div>
          </div>
          <el-button type="primary" :disabled="!paperStore.current.items.length" @click="mode = 'edit'">进入试卷编辑</el-button>
        </aside>
      </div>
    </section>
  </div>

  <section v-else-if="mode === 'edit'" class="page-main paper-edit-page">
    <div class="paper-edit-toolbar" role="toolbar" aria-label="试卷编辑操作">
      <div class="paper-sort-tip">
        <el-icon><Rank /></el-icon>
        <span>长按题目的编号或内容区域约 0.4 秒并拖动，可调整同一题型内的试题顺序</span>
      </div>
      <el-alert
        v-if="paperOverLimit"
        :title="`当前有 ${paperStore.current.items.length} 道题，请删减到 ${paperQuestionLimit} 道以内后再保存。`"
        type="warning"
        :closable="false"
        show-icon
      />
      <div class="paper-edit-toolbar__actions">
        <el-button :icon="Back" @click="mode = 'manual'">返回选题</el-button>
        <el-button v-if="paperStore.current.status === 'draft'" :loading="paperStore.saving" @click="savePaper('draft')">保存草稿</el-button>
        <el-button :loading="paperStore.saving" @click="savePaper('saved')">保存到历史试卷</el-button>
        <el-button type="primary" :icon="View" :loading="previewEntering" @click="enterPreview">排版与打印</el-button>
      </div>
    </div>
    <div class="paper-edit-layout">
      <div ref="paperItemsRoot" class="paper-items">
        <el-input v-model="paperStore.current.title" size="large" class="paper-title-input" />
        <section v-for="group in groupedItems" :key="group.type" class="paper-group surface">
          <h3>{{ typeName(group.type) }} <span>{{ group.items.length }} 题</span></h3>
          <div class="paper-sortable-list" :data-question-type="group.type">
            <div
              v-for="(item, itemIndex) in group.items"
              :key="item.id"
              class="paper-item-row"
              :data-paper-item-id="item.id"
              @contextmenu="openQuestionContextMenu($event, item.snapshot)"
            >
              <span class="paper-item-index">{{ item.position + 1 }}</span>
              <div class="paper-item-content" title="长按题目区域约 0.4 秒后拖动排序">
                <QuestionStemSummary class="paper-item-content__stem" :content="item.snapshot.stem" :lines="2" />
                <span>{{ item.snapshot.subjectName }} · {{ item.snapshot.chapterName }}</span>
              </div>
              <div class="paper-item-actions">
                <el-button text circle :icon="Top" :disabled="itemIndex === 0" @click="paperStore.moveItemWithinType(item.id, -1)" />
                <el-button text circle :icon="Bottom" :disabled="itemIndex === group.items.length - 1" @click="paperStore.moveItemWithinType(item.id, 1)" />
                <el-button text type="primary" @click="openReplacement(item)">替换</el-button>
                <el-button text type="danger" @click="paperStore.removeItem(item.id)">移除</el-button>
              </div>
            </div>
          </div>
        </section>
      </div>
      <aside class="paper-structure surface">
        <strong>试卷结构 <span>{{ paperStore.current.items.length }} 题</span></strong>
        <div v-for="definition in questionTypeDefinitions" :key="definition.code" class="summary-row">
          <span>{{ definition.name }}</span><strong>{{ typeCounts[definition.code] ?? 0 }} 题</strong>
        </div>
        <el-button type="primary" :icon="View" :loading="previewEntering" @click="enterPreview">排版与打印</el-button>
      </aside>
    </div>
  </section>

  <section v-else class="paper-preview-page">
    <div class="preview-toolbar">
      <el-button :icon="Back" @click="mode = 'edit'">返回修改</el-button>
      <strong>试卷排版与打印</strong>
      <div>
        <el-button v-if="paperStore.current.status === 'draft'" :loading="paperStore.saving" @click="savePaper('draft')">保存草稿</el-button>
        <el-button :loading="paperStore.saving" @click="savePaper('saved')">保存到历史试卷</el-button>
        <el-button
          type="primary"
          :icon="DocumentChecked"
          :disabled="!paperStore.current.items.length || !appStore.license.capabilities.canExportDocuments"
          @click="exportDialogOpen = true"
        >{{ appStore.license.capabilities.canExportDocuments ? '导出 Word' : '专业版可导出' }}</el-button>
      </div>
    </div>
    <div class="preview-workspace">
      <PaperCanvasEditor
        :key="canvasEditorKey"
        v-model="paperStore.current.layout"
        :paper="paperStore.current"
        :template-preview="previewTemplate"
        :question-types="questionTypeDefinitions"
        :can-print="appStore.license.capabilities.canPrint"
        @changed="onCanvasLayoutChanged"
        @formula-change="onCanvasFormulaChanged"
      >
        <template #settings>
          <aside class="preview-settings surface">
            <h3>排版内容</h3>
            <el-form-item label="排版模板">
              <el-select
                v-model="selectedPreviewTemplateId"
                :loading="previewTemplateLoading"
                placeholder="请选择模板"
                @change="changePreviewTemplate"
              >
                <el-option label="不使用模板（标准 A4）" value="" />
                <el-option
                  v-for="template in previewTemplates"
                  :key="template.id"
                  :label="templateOptionLabel(template)"
                  :value="template.id"
                  :disabled="!templateCanPreview(template)"
                />
              </el-select>
            </el-form-item>
            <el-alert
              v-if="previewTemplateError"
              :title="previewTemplateError"
              type="error"
              :closable="false"
              show-icon
            />
            <div v-if="selectedPreviewTemplate" class="template-preview-summary">
              <strong>{{ selectedPreviewTemplate.name }}</strong>
              <span>{{ previewPageDescription }}</span>
              <span>
                保留模板正文 {{ previewTemplate?.blocks.filter((block) => block.kind === 'static').length ?? 0 }}
                段，套用 {{ Object.keys(previewTemplate?.styleProfile?.roles ?? {}).length }} 类题目样式
              </span>
            </div>
            <el-form-item label="试卷标题"><el-input v-model="paperStore.current.title" /></el-form-item>
            <el-form-item label="显示内容">
              <el-select v-model="paperStore.current.exportContentMode">
                <el-option label="仅试卷" value="paper_only" />
                <el-option label="仅答案" value="answers_only" />
                <el-option label="试卷 + 答案" value="paper_and_answers" />
                <el-option label="试卷 + 答案 + 解析" value="paper_answers_explanations" />
              </el-select>
            </el-form-item>
            <div class="info-banner">中间区域会使用所选模板的纸张尺寸、页边距、保留正文和题目样式生成可编辑分页稿。可在上方“页面”中选择 A3、A4、A5、B4、B5 或自定义尺寸；明确调整后的尺寸会随试卷保存，并同步到打印和 Word 导出。</div>
            <div class="info-banner">画布预览与 Word/WPS 使用不同的文字测量和分页引擎；现在会同步纸张、页边距、字号和模板行距，但复杂字体的换行与最终页数仍可能有少量差异。软件不会生成 PDF。</div>
          </aside>
        </template>
      </PaperCanvasEditor>
    </div>
  </section>

  <RandomDrawDialog
    v-if="batchDrawDialogMounted"
    v-model="batchDrawDialogOpen"
    :subjects="appStore.subjects"
    :tags="appStore.tags"
    :question-types="appStore.questionTypes"
    :excluded-question-ids="[...paperStore.selectedQuestionIds]"
    :remaining-capacity="randomDrawRemainingCapacity"
    @add="addRandomQuestions"
  />

  <el-dialog
    v-model="replacementDialogOpen"
    title="替换题目"
    width="720px"
    destroy-on-close
    @closed="resetReplacementDialog"
  >
    <div class="replacement-dialog">
      <div class="replacement-source">
        <span>当前题目</span>
        <QuestionStemSummary
          class="replacement-source__stem"
          :content="replacementTarget?.snapshot.stem"
          :lines="2"
        />
        <small>{{ replacementScopeText }}</small>
      </div>
      <div v-loading="replacementLoading" class="replacement-candidates">
        <el-alert
          v-if="replacementError"
          :title="replacementError"
          type="error"
          :closable="false"
          show-icon
        />
        <el-radio-group v-else-if="replacementPageItems.length" v-model="replacementQuestionId" class="replacement-radio-list">
          <el-radio
            v-for="question in replacementPageItems"
            :key="question.id"
            :value="question.id"
            class="replacement-radio"
          >
            <QuestionStemSummary class="replacement-radio__stem" :content="question.stem" :lines="2" />
            <small>{{ question.subjectName }} · {{ question.chapterName }}</small>
          </el-radio>
        </el-radio-group>
        <el-empty
          v-else-if="!replacementLoading"
          description="当前筛选范围内没有同题型且未被选入试卷的新题"
          :image-size="72"
        />
      </div>
      <div v-if="replacementCandidates.length > replacementPageSize" class="replacement-pagination">
        <span>共 {{ replacementCandidates.length }} 道可替换题目</span>
        <el-pagination
          v-model:current-page="replacementPage"
          :page-size="replacementPageSize"
          :total="replacementCandidates.length"
          background
          layout="prev, pager, next"
        />
      </div>
    </div>
    <template #footer>
      <el-button @click="replacementDialogOpen = false">取消</el-button>
      <el-button
        type="primary"
        :disabled="replacementLoading || !replacementQuestionId"
        @click="confirmReplacement"
      >确认替换</el-button>
    </template>
  </el-dialog>

  <PaperExportDialog
    v-if="exportDialogOpen"
    v-model="exportDialogOpen"
    :paper="paperStore.current"
    :prepare-paper="preparePaperForExport"
  />
  <QuestionPreviewDrawer
    v-if="questionPreviewOpen"
    v-model="questionPreviewOpen"
    :question="previewQuestion"
    :show-add-to-paper="false"
  />
  <AppContextMenu
    v-model="questionContextMenuOpen"
    :x="questionContextMenuX"
    :y="questionContextMenuY"
    :title="questionContextMenuTitle"
    :items="questionContextMenuItems"
    @select="handleQuestionContextMenu"
  />
</template>

<style scoped>
.manual-page {
  display: flex;
  flex-direction: column;
}

.manual-layout,
.paper-edit-layout {
  min-height: 0;
  flex: 1;
  display: grid;
  gap: 14px;
}

.manual-layout {
  grid-template-columns: minmax(520px, 1fr) 330px;
}

.candidate-card,
.selected-panel {
  min-height: 0;
  overflow: hidden;
}

.candidate-card {
  display: flex;
  flex-direction: column;
}

.candidate-filter {
  padding: 12px;
  display: grid;
  grid-template-columns: 1fr 150px;
  gap: 8px;
  border-bottom: 1px solid #edf1f5;
}

.candidate-list {
  min-height: 0;
  flex: 1;
  overflow: auto;
}

.candidate-empty {
  padding: 64px 20px;
  color: #94a3b8;
  font-size: 11px;
  text-align: center;
}

.candidate-pagination {
  min-height: 52px;
  padding: 8px 12px;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  border-top: 1px solid #edf1f5;
  color: #64748b;
  font-size: 10px;
}

.candidate-row {
  min-height: 62px;
  padding: 10px 13px;
  display: grid;
  grid-template-columns: 76px minmax(0, 1fr) 58px;
  align-items: center;
  gap: 10px;
  border-bottom: 1px solid #f0f3f7;
}

.candidate-row > div {
  min-width: 0;
  display: grid;
  gap: 5px;
}

.candidate-row__stem {
  font-size: 12px;
  font-weight: 600;
}

.candidate-row span {
  color: #64748b;
  font-size: 10px;
}

.selected-panel {
  padding: 14px;
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.selected-panel__header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  font-size: 13px;
}

.selected-panel__header strong span {
  color: #2563eb;
}

.selected-panel__actions {
  display: flex;
  align-items: center;
  justify-content: flex-end;
  flex-wrap: wrap;
  gap: 4px;
}

.selected-list {
  min-height: 180px;
  flex: 1;
  overflow: auto;
  display: grid;
  align-content: start;
  gap: 6px;
}

.selected-row {
  padding: 8px;
  display: grid;
  grid-template-columns: 24px minmax(0, 1fr) 28px;
  align-items: center;
  gap: 6px;
  border: 1px solid #edf1f5;
  border-radius: 6px;
  font-size: 10px;
  cursor: grab;
  touch-action: pan-y;
}

.selected-empty {
  padding: 70px 20px;
  color: #94a3b8;
  font-size: 11px;
  text-align: center;
}

.paper-edit-page {
  height: 100%;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

.paper-edit-toolbar {
  min-height: 40px;
  margin-bottom: 12px;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
}

.paper-edit-toolbar__actions {
  flex: 0 0 auto;
  display: flex;
  align-items: center;
  gap: 10px;
}

.selected-row.is-drag-chosen {
  border-color: #bfdbfe;
  background: #eff6ff;
  cursor: grabbing;
}

.selected-row.is-drag-ghost {
  opacity: .35;
}

.selected-row.is-dragging {
  box-shadow: 0 10px 24px rgb(15 23 42 / 14%);
}

.paper-edit-layout {
  grid-template-columns: minmax(620px, 1fr) 290px;
  overflow: hidden;
}

.paper-items {
  min-height: 0;
  overflow-x: hidden;
  overflow-y: scroll;
  padding-right: 6px;
  display: grid;
  align-content: start;
  grid-auto-rows: max-content;
  gap: 12px;
  scrollbar-gutter: stable;
}

.paper-title-input :deep(.el-input__wrapper) {
  padding: 10px 14px;
  font-size: 16px;
  font-weight: 700;
}

.paper-group {
  min-height: max-content;
}

.paper-group h3 {
  margin: 0;
  padding: 12px 14px;
  border-bottom: 1px solid #edf1f5;
  background: #f8fafc;
  font-size: 13px;
}

.paper-group h3 span {
  margin-left: 6px;
  color: #64748b;
  font-size: 10px;
  font-weight: 400;
}

.paper-sortable-list {
  max-height: none;
  overflow: visible;
  display: flex;
  flex-direction: column;
}

.paper-item-row {
  flex: 0 0 auto;
  min-height: 64px;
  padding: 10px 12px;
  display: grid;
  grid-template-columns: 30px minmax(0, 1fr) 190px;
  align-items: center;
  gap: 10px;
  border-bottom: 1px solid #f0f3f7;
  background: #fff;
  transition: background-color .16s, box-shadow .16s, opacity .16s;
  cursor: grab;
  touch-action: pan-y;
}

.paper-item-row.is-drag-chosen {
  background: #eff6ff;
  box-shadow: inset 0 0 0 1px #bfdbfe;
  cursor: grabbing;
}

.paper-item-row.is-drag-ghost {
  opacity: .35;
}

.paper-item-row.is-dragging {
  box-shadow: 0 10px 30px rgb(15 23 42 / 16%);
}

.paper-item-index {
  width: 26px;
  height: 26px;
  display: grid;
  place-items: center;
  border-radius: 6px;
  background: #f1f5f9;
  color: #475569;
  font-size: 10px;
}

.paper-item-content {
  min-width: 0;
  display: grid;
  gap: 5px;
}

.paper-item-content__stem {
  font-size: 11px;
  font-weight: 600;
}

.paper-item-row span {
  color: #64748b;
  font-size: 10px;
}

.paper-item-actions {
  display: flex;
  justify-content: flex-end;
  cursor: default;
}

.paper-sort-tip {
  min-width: 0;
  display: flex;
  align-items: center;
  gap: 7px;
  color: #64748b;
  font-size: 10px;
}

.paper-sort-tip span {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.paper-sort-tip .el-icon {
  color: #2563eb;
}

.paper-structure {
  min-height: 0;
  overflow: auto;
  padding: 16px;
  display: flex;
  flex-direction: column;
  gap: 13px;
}

.paper-structure > strong {
  margin-bottom: 4px;
  display: flex;
  justify-content: space-between;
}

.paper-structure > strong span {
  color: #2563eb;
}

.paper-preview-page {
  height: 100%;
  display: grid;
  grid-template-rows: 58px minmax(0, 1fr);
  background: #e9eef5;
}

.preview-toolbar {
  padding: 0 22px;
  display: flex;
  align-items: center;
  justify-content: space-between;
  border-bottom: 1px solid #dbe3ed;
  background: #fff;
}

.preview-toolbar > div {
  display: flex;
  gap: 8px;
}

.preview-workspace {
  min-height: 0;
  overflow: hidden;
  padding: 14px;
}

.a4-paper {
  width: 210mm;
  min-height: 297mm;
  margin: 0 auto;
  padding: 22mm 20mm;
  background: #fff;
  box-shadow: 0 4px 18px rgb(15 23 42 / 18%);
  color: #111827;
  font-family: SimSun, "宋体", serif;
}

.a4-paper h1 {
  margin: 0 0 8mm;
  text-align: center;
  font-size: 20pt;
}

.paper-meta-line {
  margin-bottom: 8mm;
  padding-bottom: 4mm;
  border-bottom: 1px solid #111;
  font-size: 10.5pt;
  text-align: center;
}

.a4-paper h2 {
  margin: 6mm 0 3mm;
  font-size: 12pt;
}

.a4-question {
  margin-bottom: 4mm;
  font-size: 10.5pt;
  line-height: 1.75;
}

.a4-question :deep(p) {
  display: inline;
  margin: 0;
}

.a4-options {
  margin: 2mm 0 0 6mm;
  display: grid;
  grid-template-columns: 1fr 1fr;
}

.a4-reference-section {
  margin-top: 7mm;
}

.a4-reference-item {
  margin-bottom: 3mm;
  display: grid;
  grid-template-columns: 9mm minmax(0, 1fr);
  gap: 2mm;
  font-size: 10.5pt;
  line-height: 1.7;
}

.a4-reference-item :deep(p) {
  margin: 0;
}

.preview-settings {
  min-height: 100%;
  padding: 16px;
}

.preview-settings h3 {
  margin: 0 0 18px;
}

.preview-settings :deep(.el-select) {
  width: 100%;
}

.template-preview-summary {
  margin: -4px 0 18px;
  padding: 11px 12px;
  display: grid;
  gap: 4px;
  border: 1px solid #bfdbfe;
  border-radius: 7px;
  background: #eff6ff;
}

.template-preview-summary strong {
  color: #1e3a8a;
}

.template-preview-summary span {
  color: #475569;
  font-size: 12px;
  line-height: 1.5;
}

.preview-settings :deep(.el-alert) {
  margin: -4px 0 16px;
}

.replacement-dialog {
  display: grid;
  gap: 14px;
}

.replacement-source {
  padding: 12px 14px;
  display: grid;
  gap: 6px;
  border-radius: 7px;
  background: #f8fafc;
}

.replacement-source > span,
.replacement-source > small,
.replacement-radio small,
.replacement-pagination > span {
  color: #64748b;
  font-size: 10px;
}

.replacement-source__stem {
  color: #1f2937;
  font-size: 12px;
  font-weight: 600;
}

.replacement-candidates {
  min-height: 220px;
}

.replacement-radio-list {
  width: 100%;
  display: grid;
  gap: 7px;
}

.replacement-radio {
  width: 100%;
  height: auto;
  min-height: 48px;
  margin: 0;
  padding: 9px 12px;
  display: flex;
  align-items: flex-start;
  border: 1px solid #e5eaf2;
  border-radius: 7px;
}

.replacement-radio :deep(.el-radio__label) {
  min-width: 0;
  display: grid;
  gap: 5px;
  white-space: normal;
}

.replacement-radio__stem {
  color: #334155;
  font-size: 11px;
  font-weight: 600;
}

.replacement-pagination {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}
</style>
