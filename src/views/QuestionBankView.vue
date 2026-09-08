<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import { ElMessage, ElMessageBox, type TableInstance } from 'element-plus'
import { Delete, EditPen, View, RefreshLeft } from '@element-plus/icons-vue'
import AppContextMenu from '../components/AppContextMenu.vue'
import QuestionPreviewDrawer from '../components/QuestionPreviewDrawer.vue'
import QuestionBatchEditDialog from '../components/QuestionBatchEditDialog.vue'
import QuestionStemSummary from '../components/QuestionStemSummary.vue'
import { useAppStore } from '../stores/app'
import { usePaperStore } from '../stores/paper'
import { useQuestionBankStore } from '../stores/questionBank'
import { backend } from '../services/backend'
import { errorMessage } from '../services/errors'
import {
  type Question,
  type QuestionBatchEditRequest,
} from '../types/domain'
import type { ContextMenuItem } from '../types/contextMenu'
import { questionBankContextMenuItems } from '../utils/questionBankContextMenu'
import { effectiveQuestionTypes, questionTypeLabel } from '../utils/questionTypes'
import { questionUsageFilterOptions } from '../utils/questionUsage'

const router = useRouter()
const appStore = useAppStore()
const bankStore = useQuestionBankStore()
const paperStore = usePaperStore()
const table = ref<TableInstance>()
const previewOpen = ref(false)
const batchDialogOpen = ref(false)
const batchBusy = ref(false)
const batchQuestions = ref<Question[]>([])
const contextMenuOpen = ref(false)
const contextMenuX = ref(0)
const contextMenuY = ref(0)
const contextMenuTitle = ref('')
const contextMenuItems = ref<ContextMenuItem[]>([])
const contextQuestions = ref<Question[]>([])
const recycleRetentionDays = ref(30)
let filterTimer: ReturnType<typeof setTimeout> | undefined

bankStore.initializeFilters({ deleted: false, pageSize: 100 })

const typeOptions = computed(() => effectiveQuestionTypes(appStore.questionTypes).map((definition) => ({
  value: definition.code,
  label: definition.name,
})))
const chapterOptions = computed(() => {
  if (!bankStore.filters.subjectId) return appStore.subjects.flatMap((subject) => subject.chapters)
  return appStore.subjects.find((subject) => subject.id === bankStore.filters.subjectId)?.chapters ?? []
})

onMounted(() => {
  void Promise.all([bankStore.load(), appStore.refreshTaxonomy()])
  void backend.getSettings()
    .then((settings) => {
      recycleRetentionDays.value = settings.recycleRetentionDays
    })
    .catch(() => undefined)
  window.addEventListener('keydown', onKeydown)
})

onBeforeUnmount(() => {
  clearTimeout(filterTimer)
  window.removeEventListener('keydown', onKeydown)
})

function scheduleLoad() {
  clearTimeout(filterTimer)
  filterTimer = setTimeout(() => void bankStore.load(), 180)
}

watch(
  () => [
    bankStore.filters.keyword,
    bankStore.filters.subjectId,
    bankStore.filters.chapterId,
    bankStore.filters.type,
    bankStore.filters.usage,
    bankStore.filters.tagIds.join(','),
  ],
  () => {
    bankStore.filters.page = 1
    scheduleLoad()
  },
)

watch(
  () => [bankStore.filters.page, bankStore.filters.pageSize],
  ([, pageSize], [, previousPageSize]) => {
    if (pageSize !== previousPageSize && bankStore.filters.page !== 1) {
      bankStore.filters.page = 1
      return
    }
    scheduleLoad()
  },
)

function onSelectionChange(items: Question[]) {
  bankStore.selectedIds = items.map((item) => item.id)
}

function openPreview(question: Question) {
  bankStore.previewQuestion = question
  previewOpen.value = true
}

function addToPaper(question: Question) {
  addQuestionsToPaper([question])
}

function addQuestionsToPaper(questions: Question[]) {
  const limit = appStore.license.capabilities.maxQuestionsPerPaper
  const added = paperStore.addQuestions(questions, limit)
  if (!added) {
    ElMessage.warning(limit == null ? '所选题目已经在当前试卷中。' : `基础桌面模式每份试卷最多加入 ${limit} 道题。`)
    return
  }
  const omitted = questions.length - added
  ElMessage.success(omitted > 0
    ? `已加入 ${added} 道，达到 ${limit} 道上限。`
    : `已加入当前试卷，共 ${paperStore.current.items.length} 题`)
}

function addSelectedToPaper() {
  const selected = selectedQuestions()
  addQuestionsToPaper(selected)
}

function selectedQuestions() {
  return bankStore.questions.filter((question) => bankStore.selectedIds.includes(question.id))
}

function openBatchEditFor(questions: Question[]) {
  if (!questions.length) {
    ElMessage.warning('请先选择要批量编辑的题目')
    return
  }
  batchQuestions.value = [...questions]
  batchDialogOpen.value = true
}

function openBatchEdit() {
  openBatchEditFor(selectedQuestions())
}

async function applyBatchEdit(request: QuestionBatchEditRequest) {
  if (batchBusy.value) return
  batchBusy.value = true
  try {
    const result = await bankStore.batchEdit(request)
    table.value?.clearSelection()
    batchDialogOpen.value = false
    batchQuestions.value = []
    ElMessage.success(`已原子更新 ${result.updatedCount} 道题`)
  } catch (reason) {
    ElMessage.error(errorMessage(reason, '批量编辑未执行，请刷新后重试'))
  } finally {
    batchBusy.value = false
  }
}

async function removeQuestions(questions: Question[]) {
  const ids = questions.map((question) => question.id)
  if (!ids.length) return
  try {
    await ElMessageBox.confirm(
      `将 ${ids.length} 道题移入回收站？软件会在 ${recycleRetentionDays.value} 天后提醒清理，在永久删除前仍可恢复。`,
      '移入回收站',
      { type: 'warning', confirmButtonText: '移入回收站', cancelButtonText: '取消' },
    )
  } catch {
    return
  }
  try {
    await bankStore.moveToRecycle(ids)
    table.value?.clearSelection()
    ElMessage.success('题目已移入回收站')
  } catch (reason) {
    ElMessage.error(errorMessage(reason, '题目未能移入回收站，请重试。'))
  }
}

async function removeSelected() {
  await removeQuestions(selectedQuestions())
}

async function copyQuestionStem(question: Question) {
  try {
    await navigator.clipboard.writeText(question.stem.plainText)
    ElMessage.success('题干文字已复制')
  } catch {
    ElMessage.info(question.stem.plainText)
  }
}

function openQuestionContextMenu(row: Question, _column: unknown, event: MouseEvent) {
  event.preventDefault()
  const selected = selectedQuestions()
  const useSelection = selected.length > 1 && selected.some((question) => question.id === row.id)
  contextQuestions.value = useSelection ? selected : [row]
  contextMenuTitle.value = useSelection
    ? `对已选 ${selected.length} 道题操作`
    : row.stem.plainText || '当前题目'
  contextMenuItems.value = questionBankContextMenuItems(useSelection ? selected.length : 1)
  contextMenuX.value = event.clientX
  contextMenuY.value = event.clientY
  contextMenuOpen.value = true
}

function handleQuestionContextMenu(item: ContextMenuItem) {
  const questions = [...contextQuestions.value]
  const question = questions[0]
  if (!question) return
  if (item.id === 'preview') openPreview(question)
  if (item.id === 'edit') void router.push(`/questions/${question.id}/edit`)
  if (item.id === 'copy-question') void router.push({ path: '/questions/new', query: { copy: question.id } })
  if (item.id === 'add-to-paper') addQuestionsToPaper(questions)
  if (item.id === 'batch-edit') openBatchEditFor(questions)
  if (item.id === 'copy-stem') void copyQuestionStem(question)
  if (item.id === 'recycle') void removeQuestions(questions)
  if (item.id === 'clear-selection') {
    table.value?.clearSelection()
    bankStore.selectedIds = []
  }
}

function onKeydown(event: KeyboardEvent) {
  if (event.key !== 'Delete' || !bankStore.selectedIds.length) return
  const target = event.target as HTMLElement | null
  if (target?.closest('input, textarea, [contenteditable="true"], [role="textbox"]')) return
  event.preventDefault()
  void removeSelected()
}

function formatDate(value?: number | null) {
  if (!value) return '从未使用'
  return new Intl.DateTimeFormat('zh-CN', { year: 'numeric', month: '2-digit', day: '2-digit' }).format(value)
}
</script>

<template>
  <section class="page-main question-bank">
    <div class="filter-card surface">
      <el-input v-model="bankStore.filters.keyword" clearable placeholder="搜索题干、答案、解析或标签" class="filter-keyword" />
      <el-select v-model="bankStore.filters.type" clearable placeholder="全部题型">
        <el-option v-for="item in typeOptions" :key="item.value" :label="item.label" :value="item.value" />
      </el-select>
      <el-select v-model="bankStore.filters.chapterId" clearable filterable placeholder="全部章节">
        <el-option v-for="chapter in chapterOptions" :key="chapter.id" :label="chapter.name" :value="chapter.id" />
      </el-select>
      <el-select v-model="bankStore.filters.tagIds" multiple collapse-tags clearable placeholder="全部标签">
        <el-option v-for="tag in appStore.tags" :key="tag.id" :label="tag.name" :value="tag.id" />
      </el-select>
      <el-select v-model="bankStore.filters.usage" placeholder="使用状态">
        <el-option
          v-for="option in questionUsageFilterOptions"
          :key="option.value"
          :label="option.label"
          :value="option.value"
        />
      </el-select>
      <el-button :icon="RefreshLeft" @click="bankStore.resetFilters">重置</el-button>
    </div>

    <div v-if="bankStore.hasSelection" class="selection-bar">
      <span>已选择 <strong>{{ bankStore.selectedIds.length }}</strong> 道题</span>
      <div>
        <el-button size="small" @click="addSelectedToPaper">加入当前试卷</el-button>
        <el-button size="small" :icon="EditPen" :disabled="!bankStore.selectedIds.length" @click="openBatchEdit">批量编辑</el-button>
        <el-button size="small" type="danger" plain :icon="Delete" @click="removeSelected">移入回收站</el-button>
      </div>
    </div>

    <div class="table-card surface">
      <el-alert v-if="bankStore.error" :title="bankStore.error" type="error" show-icon :closable="false" />
      <el-table
        ref="table"
        v-loading="bankStore.loading"
        :data="bankStore.questions"
        row-key="id"
        height="100%"
        @selection-change="onSelectionChange"
        @row-dblclick="openPreview"
        @row-contextmenu="openQuestionContextMenu"
      >
          <el-table-column type="selection" width="44" />
          <el-table-column label="题型" width="88">
            <template #default="{ row }">
              <el-tag size="small" effect="plain">{{ questionTypeLabel(row.type, appStore.questionTypes) }}</el-tag>
            </template>
          </el-table-column>
          <el-table-column label="题目内容" min-width="320">
            <template #default="{ row }">
              <button class="stem-button" @click="openPreview(row)">
                <QuestionStemSummary :content="row.stem" :lines="2" />
              </button>
              <div class="row-tags">
                <el-tag v-for="tag in row.tags" :key="tag.id" size="small" type="info" effect="plain">{{ tag.name }}</el-tag>
              </div>
            </template>
          </el-table-column>
          <el-table-column prop="subjectName" label="学科" width="120" show-overflow-tooltip />
          <el-table-column prop="chapterName" label="章节" min-width="150" show-overflow-tooltip />
          <el-table-column label="最近使用" width="118">
            <template #default="{ row }"><span :class="{ muted: !row.lastUsedAt }">{{ formatDate(row.lastUsedAt) }}</span></template>
          </el-table-column>
          <el-table-column label="更新时间" width="112">
            <template #default="{ row }">{{ formatDate(row.updatedAt) }}</template>
          </el-table-column>
          <el-table-column label="操作" width="170" fixed="right">
            <template #default="{ row }">
              <el-button link type="primary" :icon="View" @click="openPreview(row)">预览</el-button>
              <el-button link type="primary" @click="router.push(`/questions/${row.id}/edit`)">编辑</el-button>
              <el-button link type="primary" @click="router.push({ path: '/questions/new', query: { copy: row.id } })">复制</el-button>
            </template>
          </el-table-column>
          <template #empty>
            <div class="empty-table">
              <div>还没有符合条件的题目</div>
              <span>可以调整筛选条件，或新增、导入题目。</span>
              <div><el-button size="small" type="primary" @click="router.push('/questions/new')">新增题目</el-button></div>
            </div>
          </template>
      </el-table>
      <div class="pagination-row">
        <span>共 {{ bankStore.total }} 道题</span>
        <el-pagination
          v-model:current-page="bankStore.filters.page"
          v-model:page-size="bankStore.filters.pageSize"
          background
          layout="prev, pager, next"
          :total="bankStore.total"
        />
      </div>
    </div>

    <QuestionPreviewDrawer
      v-model="previewOpen"
      :question="bankStore.previewQuestion"
      @add-to-paper="addToPaper"
    />
    <QuestionBatchEditDialog
      v-model="batchDialogOpen"
      :questions="batchQuestions"
      :subjects="appStore.subjects"
      :tags="appStore.tags"
      :busy="batchBusy"
      @confirm="applyBatchEdit"
    />
    <AppContextMenu
      v-model="contextMenuOpen"
      :x="contextMenuX"
      :y="contextMenuY"
      :title="contextMenuTitle"
      :items="contextMenuItems"
      @select="handleQuestionContextMenu"
    />
  </section>
</template>

<style scoped>
.question-bank {
  display: flex;
  flex-direction: column;
}

.filter-card {
  margin-bottom: 12px;
  padding: 12px;
  display: grid;
  grid-template-columns: minmax(240px, 1.5fr) repeat(4, minmax(130px, .7fr)) auto;
  gap: 8px;
}

.selection-bar {
  min-height: 44px;
  margin-bottom: 10px;
  padding: 7px 12px;
  display: flex;
  align-items: center;
  justify-content: space-between;
  border: 1px solid #bfdbfe;
  border-radius: 8px;
  background: #eff6ff;
  color: #1e40af;
  font-size: 12px;
}

.selection-bar > div {
  display: flex;
  gap: 7px;
}

.table-card {
  min-height: 0;
  flex: 1;
  display: grid;
  grid-template-rows: minmax(320px, 1fr) 50px;
  overflow: hidden;
}

.stem-button {
  width: 100%;
  max-width: 100%;
  padding: 0;
  overflow: hidden;
  border: 0;
  background: transparent;
  color: #1f2937;
  font-size: 12px;
  text-align: left;
}

.stem-button:hover {
  color: #2563eb;
}

.row-tags {
  min-height: 18px;
  margin-top: 5px;
  display: flex;
  gap: 4px;
}

.pagination-row {
  padding: 0 14px;
  display: flex;
  align-items: center;
  justify-content: space-between;
  border-top: 1px solid #edf1f5;
  color: #64748b;
  font-size: 11px;
}

.empty-table {
  padding: 55px 0;
  color: #475569;
}

.empty-table span {
  margin: 6px 0 14px;
  display: block;
  color: #94a3b8;
  font-size: 11px;
}

@media (max-width: 1280px) {
  .filter-card {
    grid-template-columns: 1.5fr repeat(3, 1fr);
  }
}
</style>
