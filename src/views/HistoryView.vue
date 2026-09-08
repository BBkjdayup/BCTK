<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import { ElMessage, ElMessageBox } from 'element-plus'
import { ArrowLeft, Delete, DocumentChecked, Plus, Search } from '@element-plus/icons-vue'
import PageHeader from '../components/PageHeader.vue'
import PaperExportDialog, { type PaperExportPreparation } from '../components/PaperExportDialog.vue'
import QuestionStemSummary from '../components/QuestionStemSummary.vue'
import { normalizePaperItems, usePaperStore } from '../stores/paper'
import { useAppStore } from '../stores/app'
import { backend } from '../services/backend'
import { errorMessage } from '../services/errors'
import { type Paper, type PaperItem, type PaperSummary } from '../types/domain'
import { questionTypeLabel } from '../utils/questionTypes'

const router = useRouter()
const paperStore = usePaperStore()
const appStore = useAppStore()
const activeSection = ref<'all' | 'saved' | 'draft'>('all')
const keyword = ref('')
const subject = ref('')
const createdRange = ref<'all' | 'month' | 'quarter'>('all')
const currentPage = ref(1)
const pageSize = 50
const selectedPaper = ref<Paper | null>(null)
const selectedPapers = ref<PaperSummary[]>([])
const bulkDeleting = ref(false)
const historyTable = ref<{ clearSelection: () => void } | null>(null)
const exportDialogOpen = ref(false)

const subjects = computed(() => [...new Set(
  paperStore.papers.map((paper) => paper.subjectSummaryText).filter(Boolean),
)])
const filteredPapers = computed(() => {
  const query = keyword.value.trim().normalize('NFKC').toLocaleLowerCase('zh-CN')
  const now = Date.now()
  return paperStore.papers.filter((paper) => {
    if (activeSection.value !== 'all' && paper.status !== activeSection.value) return false
    if (query && !paper.title.normalize('NFKC').toLocaleLowerCase('zh-CN').includes(query)) return false
    if (subject.value && paper.subjectSummaryText !== subject.value) return false
    const timestamp = paper.savedAt ?? paper.updatedAt
    if (createdRange.value === 'month' && timestamp < now - 30 * 86400000) return false
    if (createdRange.value === 'quarter' && timestamp < now - 90 * 86400000) return false
    return true
  })
})
const pagePapers = computed(() => {
  const start = (currentPage.value - 1) * pageSize
  return filteredPapers.value.slice(start, start + pageSize)
})
const groupedQuestions = computed(() => {
  const groups = new Map<string, PaperItem[]>()
  for (const item of selectedPaper.value?.items ?? []) {
    const label = questionTypeLabel(item.snapshot.type, appStore.questionTypes)
    const current = groups.get(label) ?? []
    current.push(item)
    groups.set(label, current)
  }
  return [...groups.entries()]
})

function clearPaperSelection() {
  selectedPapers.value = []
  historyTable.value?.clearSelection()
}

watch([activeSection, keyword, subject, createdRange], () => {
  currentPage.value = 1
  clearPaperSelection()
})
watch(currentPage, clearPaperSelection)
watch(
  () => filteredPapers.value.length,
  (total) => { currentPage.value = Math.min(currentPage.value, Math.max(1, Math.ceil(total / pageSize))) },
)

onMounted(async () => {
  try {
    await paperStore.loadAllPapers()
  } catch (reason) {
    ElMessage.error(errorMessage(reason, '加载试卷列表失败'))
  }
})

function formatDate(value: number) {
  return new Intl.DateTimeFormat('zh-CN', {
    year: 'numeric', month: '2-digit', day: '2-digit',
  }).format(value)
}

function formatDateTime(value: number) {
  return new Intl.DateTimeFormat('zh-CN', {
    year: 'numeric', month: '2-digit', day: '2-digit', hour: '2-digit', minute: '2-digit',
    hour12: false,
  }).format(value)
}

function compositionLabel(value: Paper['compositionMode']) {
  return value === 'automatic' ? '自动组卷' : '手动组卷'
}

async function openPaper(paper: PaperSummary) {
  try {
    selectedPaper.value = await paperStore.readStored(paper.id)
  } catch (reason) {
    ElMessage.error(errorMessage(reason, '读取试卷详情失败'))
  }
}

function startPaper() {
  paperStore.newPaper()
  void router.push('/papers')
}

async function reEdit(paper: PaperSummary | Paper) {
  try {
    if (paper.status === 'draft') {
      await paperStore.loadPaper(paper.id)
      ElMessage.success(`已继续编辑草稿“${paper.title}”`)
    } else {
      await paperStore.copyAndLoad(paper.id, paper.rowVersion)
      ElMessage.success(`已从“${paper.title}”创建可编辑副本`)
    }
    void router.push({ path: '/papers', query: { view: 'edit' } })
  } catch (reason) {
    ElMessage.error(errorMessage(reason, '打开可编辑试卷失败'))
  }
}

async function deletePaper(paper: PaperSummary | Paper) {
  try {
    await ElMessageBox.confirm(
      `当前数据库结构没有试卷回收站，删除后不能恢复。确定永久删除“${paper.title}”吗？题库原题不会被删除。`,
      '永久删除试卷',
      { type: 'warning', confirmButtonText: '永久删除', cancelButtonText: '取消' },
    )
  } catch {
    return
  }
  try {
    await paperStore.deleteStored(paper.id, paper.rowVersion)
    selectedPaper.value = null
    selectedPapers.value = selectedPapers.value.filter((item) => item.id !== paper.id)
    ElMessage.success('试卷及其题目快照已永久删除')
  } catch (reason) {
    ElMessage.error(errorMessage(reason, '删除试卷失败'))
  }
}

function updatePaperSelection(rows: PaperSummary[]) {
  selectedPapers.value = rows
}

async function deleteSelectedPapers() {
  if (!selectedPapers.value.length || bulkDeleting.value) return
  const targets = [...selectedPapers.value]
  try {
    await ElMessageBox.confirm(
      `确定永久删除选中的 ${targets.length} 份试卷吗？题库原题不会被删除，此操作无法恢复。`,
      '批量永久删除试卷',
      { type: 'warning', confirmButtonText: `删除 ${targets.length} 份`, cancelButtonText: '取消' },
    )
  } catch {
    return
  }

  bulkDeleting.value = true
  try {
    await paperStore.deleteStoredBatch(targets)
    clearPaperSelection()
    ElMessage.success(`已永久删除 ${targets.length} 份试卷及其题目快照`)
  } catch (reason) {
    ElMessage.error(errorMessage(reason, '批量删除试卷失败，未删除任何试卷'))
  } finally {
    bulkDeleting.value = false
  }
}

async function prepareSelectedPaperForExport(preparation: PaperExportPreparation): Promise<Paper> {
  if (!selectedPaper.value) throw new Error('请先重新打开要导出的试卷。')
  const paper = selectedPaper.value
  const saved = await backend.savePaper({
    ...paper,
    title: preparation.title,
    exportContentMode: preparation.contentMode,
    status: 'saved',
    items: normalizePaperItems(paper.items),
  })
  selectedPaper.value = saved
  await paperStore.loadAllPapers()
  return saved
}
</script>

<template>
  <div class="app-page">
    <aside class="page-sidebar history-sidebar">
      <div class="sidebar-title">历史试卷</div>
      <nav class="sidebar-menu">
        <button
          class="sidebar-menu__item"
          :class="{ 'is-active': activeSection === 'all' }"
          @click="activeSection = 'all'; selectedPaper = null"
        >
          <span class="nav-icon" />全部试卷
        </button>
        <button
          class="sidebar-menu__item"
          :class="{ 'is-active': activeSection === 'saved' }"
          @click="activeSection = 'saved'; selectedPaper = null"
        >
          <span class="nav-icon" />已保存试卷
        </button>
        <button
          class="sidebar-menu__item"
          :class="{ 'is-active': activeSection === 'draft' }"
          @click="activeSection = 'draft'; selectedPaper = null"
        >
          <span class="nav-icon" />试卷草稿
        </button>
      </nav>
      <div class="sidebar-note">
        历史试卷保存题目快照。题库原题后续修改或删除，不会影响已保存的试卷。
      </div>
    </aside>

    <section v-if="!selectedPaper" class="page-main history-page">
      <div class="surface filter-bar">
        <el-input v-model="keyword" clearable :prefix-icon="Search" placeholder="搜索试卷名称" />
        <el-select v-model="subject" clearable placeholder="全部学科">
          <el-option v-for="item in subjects" :key="item" :label="item" :value="item" />
        </el-select>
        <el-select v-model="createdRange" placeholder="创建时间">
          <el-option label="全部时间" value="all" />
          <el-option label="最近 30 天" value="month" />
          <el-option label="最近 90 天" value="quarter" />
        </el-select>
        <div class="filter-actions">
          <span class="filter-total">共 {{ filteredPapers.length }} 份试卷</span>
          <el-button type="primary" :icon="Plus" @click="startPaper">开始组卷</el-button>
          <el-button
            type="danger"
            plain
            :icon="Delete"
            :disabled="!selectedPapers.length"
            :loading="bulkDeleting"
            @click="deleteSelectedPapers"
          >
            批量删除{{ selectedPapers.length ? `（${selectedPapers.length}）` : '' }}
          </el-button>
        </div>
      </div>

      <div class="surface history-table">
        <el-table
          ref="historyTable"
          v-loading="paperStore.historyLoading || bulkDeleting"
          :data="pagePapers"
          height="100%"
          row-key="id"
          @selection-change="updatePaperSelection"
          @row-dblclick="openPaper"
        >
          <el-table-column type="selection" width="48" />
          <el-table-column label="预览" width="72">
            <template #default>
              <div class="paper-thumb"><i v-for="line in 5" :key="line" /></div>
            </template>
          </el-table-column>
          <el-table-column label="试卷名称" min-width="320">
            <template #default="{ row }">
              <button class="paper-title" @click="openPaper(row)">{{ row.title }}</button>
              <div><el-tag size="small" type="primary" effect="light">题目快照</el-tag></div>
            </template>
          </el-table-column>
          <el-table-column prop="subjectSummaryText" label="所属学科" width="140">
            <template #default="{ row }">{{ row.subjectSummaryText || '尚未选题' }}</template>
          </el-table-column>
          <el-table-column label="题目数" width="96">
            <template #default="{ row }">{{ row.questionCount }} 题</template>
          </el-table-column>
          <el-table-column label="更新时间" width="126">
            <template #default="{ row }">{{ formatDate(row.updatedAt) }}</template>
          </el-table-column>
          <el-table-column label="状态" width="96">
            <template #default="{ row }">
              <el-tag size="small" :type="row.status === 'saved' ? 'success' : 'warning'">
                {{ row.status === 'saved' ? '已保存' : '草稿' }}
              </el-tag>
            </template>
          </el-table-column>
          <el-table-column label="操作" width="205" fixed="right">
            <template #default="{ row }">
              <el-button link type="primary" @click="openPaper(row)">查看</el-button>
              <el-button link type="primary" @click="reEdit(row)">{{ row.status === 'draft' ? '继续编辑' : '再次编辑' }}</el-button>
              <el-button link type="danger" @click="deletePaper(row)">删除</el-button>
            </template>
          </el-table-column>
          <template #empty>
            <div class="history-empty">没有符合当前条件的历史试卷</div>
          </template>
        </el-table>
        <div class="table-footer">
          <span>本页 {{ pagePapers.length }} 条，共 {{ filteredPapers.length }} 条</span>
          <el-pagination v-model:current-page="currentPage" background layout="prev, pager, next" :page-size="pageSize" :total="filteredPapers.length" />
        </div>
      </div>
    </section>

    <section v-else class="page-main detail-page">
      <div class="breadcrumb">历史试卷 / 试卷详情</div>
      <PageHeader :title="selectedPaper.title" :subtitle="`创建于 ${formatDateTime(selectedPaper.createdAt)} · ${selectedPaper.subjectSummaryText || '尚未选题'} · 共 ${selectedPaper.items.length} 题`">
        <el-button type="danger" plain :icon="Delete" @click="deletePaper(selectedPaper)">永久删除</el-button>
        <el-button @click="reEdit(selectedPaper)">{{ selectedPaper.status === 'draft' ? '继续编辑' : '再次编辑' }}</el-button>
        <el-button
          type="primary"
          :icon="DocumentChecked"
          :disabled="!selectedPaper.items.length"
          @click="exportDialogOpen = true"
        >导出 Word</el-button>
      </PageHeader>

      <div class="snapshot-banner">
        <span>✓</span> 当前内容来自历史题目快照，修改或删除题库原题不会影响本试卷。
      </div>

      <div class="detail-grid">
        <article class="surface question-panel">
          <div class="question-panel__top">
            <span>共 {{ selectedPaper.items.length }} 道题目快照</span>
          </div>
          <template v-if="groupedQuestions.length">
            <section v-for="([type, items], groupIndex) in groupedQuestions" :key="type" class="question-group">
              <h3>{{ groupIndex + 1 }}、{{ type }} <el-tag size="small">{{ items.length }} 题</el-tag></h3>
              <div v-for="item in items" :key="item.id" class="snapshot-question">
                <span class="question-number">{{ item.position + 1 }}</span>
                <div>
                  <QuestionStemSummary
                    class="snapshot-question__stem"
                    :content="item.snapshot.stem"
                    :lines="2"
                  />
                  <el-tag size="small" type="primary" effect="light">快照</el-tag>
                  <el-tag size="small" type="info" effect="light">{{ item.snapshot.chapterName }}</el-tag>
                </div>
              </div>
            </section>
          </template>
          <div v-else class="history-empty">这份草稿还没有题目快照</div>
        </article>

        <aside class="surface paper-info">
          <h3>试卷信息</h3>
          <dl>
            <div><dt>所属学科</dt><dd>{{ selectedPaper.subjectSummaryText || '尚未选题' }}</dd></div>
            <div><dt>题目总数</dt><dd>{{ selectedPaper.items.length }}</dd></div>
            <div><dt>组卷方式</dt><dd>{{ compositionLabel(selectedPaper.compositionMode) }}</dd></div>
            <div><dt>当前状态</dt><dd>{{ selectedPaper.status === 'saved' ? '已保存' : '草稿' }}</dd></div>
            <div><dt>创建时间</dt><dd>{{ formatDateTime(selectedPaper.createdAt) }}</dd></div>
            <div><dt>最近保存</dt><dd>{{ selectedPaper.lastSavedAt ? formatDateTime(selectedPaper.lastSavedAt) : '尚未保存' }}</dd></div>
          </dl>
          <el-button class="back-button" :icon="ArrowLeft" @click="selectedPaper = null">返回试卷列表</el-button>
        </aside>
      </div>
    </section>

    <PaperExportDialog
      v-if="selectedPaper"
      v-model="exportDialogOpen"
      :paper="selectedPaper"
      :prepare-paper="prepareSelectedPaperForExport"
    />
  </div>
</template>

<style scoped>
.history-sidebar {
  padding-top: 2px;
}

.nav-icon {
  width: 18px;
  height: 18px;
  border: 1.5px solid #94a3b8;
  border-radius: 5px;
}

.sidebar-menu__item.is-active .nav-icon {
  border-color: #3b82f6;
}

.sidebar-note {
  margin: 22px 14px;
  padding: 13px 12px;
  border: 1px solid var(--border);
  border-radius: 8px;
  background: #fff;
  color: #526176;
  font-size: 11px;
  line-height: 1.75;
}

.history-page,
.detail-page {
  display: flex;
  flex-direction: column;
}

.breadcrumb {
  margin-bottom: 8px;
  color: #94a3b8;
  font-size: 11px;
}

.filter-bar {
  margin-bottom: 12px;
  padding: 10px 12px;
  display: grid;
  grid-template-columns: minmax(260px, 1.2fr) 150px 190px 1fr;
  align-items: center;
  gap: 10px;
}

.filter-actions {
  justify-self: end;
  display: flex;
  align-items: center;
  gap: 12px;
}

.filter-total {
  color: #64748b;
  font-size: 11px;
}

.history-table {
  min-height: 430px;
  flex: 1;
  display: grid;
  grid-template-rows: minmax(360px, 1fr) 50px;
  overflow: hidden;
}

.paper-thumb {
  width: 38px;
  height: 50px;
  padding: 9px 6px;
  display: flex;
  flex-direction: column;
  gap: 4px;
  border: 1px solid #cbd5e1;
  border-radius: 3px;
  background: #fff;
}

.paper-thumb i {
  height: 3px;
  display: block;
  background: #dbe3ed;
}

.paper-title {
  margin: 0 0 7px;
  padding: 0;
  border: 0;
  background: none;
  color: #18233a;
  font-size: 13px;
  font-weight: 700;
  text-align: left;
}

.paper-title:hover {
  color: var(--blue-600);
}

.table-footer {
  padding: 0 14px;
  display: flex;
  align-items: center;
  justify-content: space-between;
  border-top: 1px solid #edf1f5;
  color: #64748b;
  font-size: 11px;
}

.snapshot-banner {
  margin-bottom: 12px;
  padding: 9px 13px;
  border: 1px solid #ddd6fe;
  border-radius: 7px;
  background: #f5f3ff;
  color: #6d28d9;
  font-size: 11px;
}

.snapshot-banner span {
  margin-right: 8px;
}

.detail-grid {
  min-height: 0;
  flex: 1;
  display: grid;
  grid-template-columns: minmax(0, 1fr) 280px;
  gap: 14px;
}

.question-panel {
  min-height: 500px;
  padding: 18px;
  overflow: auto;
}

.question-panel__top {
  margin-bottom: 15px;
  display: flex;
  align-items: center;
  justify-content: space-between;
  color: #64748b;
  font-size: 11px;
}

.question-group h3 {
  margin: 17px 0 10px;
  color: #172033;
  font-size: 13px;
}

.snapshot-question {
  margin-bottom: 8px;
  padding: 11px 12px;
  display: flex;
  gap: 10px;
  border: 1px solid var(--border);
  border-radius: 8px;
  background: #fff;
}

.question-number {
  width: 22px;
  height: 22px;
  flex: 0 0 22px;
  display: grid;
  place-items: center;
  border-radius: 4px;
  background: #f1f5f9;
  color: #64748b;
  font-size: 10px;
}

.snapshot-question__stem {
  margin: 1px 0 7px;
  color: #1f2937;
  font-size: 12px;
}

.snapshot-question .el-tag + .el-tag {
  margin-left: 5px;
}

.paper-info {
  align-self: stretch;
  padding: 18px;
}

.paper-info h3 {
  margin: 0 0 14px;
  font-size: 14px;
}

.paper-info dl {
  margin: 0;
}

.paper-info dl div {
  padding: 11px 0;
  display: flex;
  justify-content: space-between;
  gap: 12px;
  border-bottom: 1px solid #edf1f5;
  font-size: 11px;
}

.paper-info dt {
  color: #64748b;
}

.paper-info dd {
  margin: 0;
  color: #111827;
  font-weight: 600;
  text-align: right;
}

.back-button {
  width: 100%;
  margin-top: 18px;
}

.history-empty {
  padding: 70px 0;
  color: #94a3b8;
  text-align: center;
}

@media (max-width: 1280px) {
  .filter-bar {
    grid-template-columns: minmax(220px, 1fr) 140px 170px;
  }

  .filter-total {
    display: none;
  }

  .detail-grid {
    grid-template-columns: minmax(0, 1fr) 245px;
  }
}
</style>
