<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import { Delete, EditPen, MoreFilled, Plus } from '@element-plus/icons-vue'
import Sortable from 'sortablejs'
import { useAppStore } from '../stores/app'
import { errorMessage } from '../services/errors'
import type { Chapter, Subject } from '../types/domain'
import { buildTaxonomyOrderItems } from '../utils/taxonomyOrder'

const appStore = useAppStore()
const selectedSubjectId = ref<string>()
const subjectListRoot = ref<HTMLElement>()
const chapterListRoot = ref<HTMLElement>()
const reordering = ref(false)
let subjectSortable: Sortable | null = null
let chapterSortable: Sortable | null = null

const orderedSubjects = computed<Subject[]>(() =>
  [...appStore.subjects].sort((left: Subject, right: Subject) => left.sortOrder - right.sortOrder),
)
const selectedSubject = computed<Subject | undefined>(() =>
  appStore.subjects.find((subject: Subject) => subject.id === selectedSubjectId.value),
)
const orderedChapters = computed<Chapter[]>(() =>
  [...(selectedSubject.value?.chapters ?? [])].sort(
    (left: Chapter, right: Chapter) => left.sortOrder - right.sortOrder,
  ),
)

watch(
  () => orderedSubjects.value.map((subject: Subject) => subject.id).join(','),
  () => {
    if (!selectedSubject.value) selectedSubjectId.value = orderedSubjects.value[0]?.id
  },
  { immediate: true },
)

function normalizeName(value: string) {
  return value.trim().normalize('NFKC').toLocaleLowerCase('zh-CN')
}

function subjectNameExists(name: string, exceptId?: string) {
  const key = normalizeName(name)
  return appStore.subjects.some(
    (subject: Subject) => subject.id !== exceptId && normalizeName(subject.name) === key,
  )
}

function chapterNameExists(subject: Subject, name: string, exceptId?: string) {
  const key = normalizeName(name)
  return subject.chapters.some(
    (chapter) => chapter.id !== exceptId && normalizeName(chapter.name) === key,
  )
}

async function askForName(title: string, initialValue = '', maximum = 100): Promise<string | null> {
  try {
    const result = await ElMessageBox.prompt('名称将显示在题库筛选和题目录入页面中。', title, {
      inputValue: initialValue,
      inputPlaceholder: '请输入名称',
      confirmButtonText: '保存',
      cancelButtonText: '取消',
      inputValidator: (value: string) => {
        const name = value.trim().normalize('NFKC').trim()
        if (!name) return '名称不能为空'
        return [...name].length <= maximum || `名称不能超过 ${maximum} 个字符`
      },
    })
    return result.value.trim().normalize('NFKC').trim()
  } catch {
    return null
  }
}

async function addSubject() {
  const name = await askForName('新增学科', '', 50)
  if (!name) return
  if (subjectNameExists(name)) {
    ElMessage.warning('已经存在同名学科，请换一个名称')
    return
  }

  try {
    selectedSubjectId.value = await appStore.createSubject(name)
    ElMessage.success(`已新增学科“${name}”`)
  } catch (reason) {
    ElMessage.error(errorMessage(reason, '新增学科失败'))
  }
}

async function editSubject(subject: Subject) {
  const name = await askForName('编辑学科', subject.name, 50)
  if (!name || normalizeName(name) === normalizeName(subject.name)) return
  if (subjectNameExists(name, subject.id)) {
    ElMessage.warning('已经存在同名学科，请换一个名称')
    return
  }

  try {
    await appStore.updateSubject(subject.id, name)
    ElMessage.success('学科名称已更新')
  } catch (reason) {
    ElMessage.error(errorMessage(reason, '更新学科失败'))
  }
}

async function deleteSubject(subject: Subject) {
  if (subject.chapters.length || subject.questionCount) {
    ElMessageBox.alert(
      `“${subject.name}”下还有 ${subject.chapters.length} 个章节、${subject.questionCount} 道题。请先移动或删除关联内容。`,
      '暂时无法删除',
      { type: 'warning', confirmButtonText: '知道了' },
    )
    return
  }

  try {
    await ElMessageBox.confirm(`确定删除学科“${subject.name}”吗？`, '删除学科', {
      type: 'warning',
      confirmButtonText: '删除',
      cancelButtonText: '取消',
    })
  } catch {
    return
  }

  try {
    await appStore.deleteSubject(subject.id)
    ElMessage.success('学科已删除')
  } catch (reason) {
    ElMessage.error(errorMessage(reason, '删除学科失败'))
  }
}

async function addChapter() {
  const subject = selectedSubject.value
  if (!subject) {
    ElMessage.warning('请先新增或选择一个学科')
    return
  }

  const name = await askForName(`在“${subject.name}”中新增章节`, '', 100)
  if (!name) return
  if (chapterNameExists(subject, name)) {
    ElMessage.warning('这个学科中已经存在同名章节')
    return
  }

  try {
    await appStore.createChapter(subject.id, name)
    ElMessage.success(`已新增章节“${name}”`)
  } catch (reason) {
    ElMessage.error(errorMessage(reason, '新增章节失败'))
  }
}

async function editChapter(chapter: Chapter) {
  const subject = selectedSubject.value
  if (!subject) return
  const name = await askForName('编辑章节', chapter.name, 100)
  if (!name || normalizeName(name) === normalizeName(chapter.name)) return
  if (chapterNameExists(subject, name, chapter.id)) {
    ElMessage.warning('这个学科中已经存在同名章节')
    return
  }

  try {
    await appStore.updateChapter(chapter.id, name)
    ElMessage.success('章节名称已更新')
  } catch (reason) {
    ElMessage.error(errorMessage(reason, '更新章节失败'))
  }
}

async function deleteChapter(chapter: Chapter) {
  const subject = selectedSubject.value
  if (!subject) return
  if (chapter.questionCount) {
    ElMessageBox.alert(
      `“${chapter.name}”中还有 ${chapter.questionCount} 道题，请先将这些题目移动到其他章节。`,
      '暂时无法删除',
      { type: 'warning', confirmButtonText: '知道了' },
    )
    return
  }

  try {
    await ElMessageBox.confirm(`确定删除章节“${chapter.name}”吗？`, '删除章节', {
      type: 'warning',
      confirmButtonText: '删除',
      cancelButtonText: '取消',
    })
  } catch {
    return
  }

  try {
    await appStore.deleteChapter(chapter.id)
    ElMessage.success('章节已删除')
  } catch (reason) {
    ElMessage.error(errorMessage(reason, '删除章节失败'))
  }
}

function destroyTaxonomySortables() {
  subjectSortable?.destroy()
  chapterSortable?.destroy()
  subjectSortable = null
  chapterSortable = null
}

function setSortablesDisabled(disabled: boolean) {
  subjectSortable?.option('disabled', disabled)
  chapterSortable?.option('disabled', disabled)
}

function orderedElementIds(container: HTMLElement, attribute: 'subjectId' | 'chapterId') {
  return [...container.children]
    .map((element) => (element as HTMLElement).dataset[attribute])
    .filter((id): id is string => Boolean(id))
}

async function saveDraggedSubjectOrder(orderedIds: string[]) {
  const originalIds = orderedSubjects.value.map((subject) => subject.id)
  const items = buildTaxonomyOrderItems(orderedIds, originalIds)
  if (!items) {
    subjectSortable?.sort(originalIds)
    ElMessage.warning('学科列表已发生变化，请刷新后重新排序')
    return
  }

  reordering.value = true
  setSortablesDisabled(true)
  try {
    await appStore.saveSubjectOrder(items)
    ElMessage.success('学科顺序已保存')
  } catch (reason) {
    subjectSortable?.sort(originalIds)
    await appStore.refreshTaxonomy()
    ElMessage.error(errorMessage(reason, '保存学科排序失败'))
  } finally {
    reordering.value = false
    setSortablesDisabled(false)
  }
}

async function saveDraggedChapterOrder(orderedIds: string[]) {
  const subject = selectedSubject.value
  if (!subject) return
  const originalIds = orderedChapters.value.map((chapter) => chapter.id)
  const items = buildTaxonomyOrderItems(orderedIds, originalIds)
  if (!items) {
    chapterSortable?.sort(originalIds)
    ElMessage.warning('章节列表已发生变化，请刷新后重新排序')
    return
  }

  reordering.value = true
  setSortablesDisabled(true)
  try {
    await appStore.saveChapterOrder(subject.id, items)
    ElMessage.success('章节顺序已保存')
  } catch (reason) {
    chapterSortable?.sort(originalIds)
    await appStore.refreshTaxonomy()
    ElMessage.error(errorMessage(reason, '保存章节排序失败'))
  } finally {
    reordering.value = false
    setSortablesDisabled(false)
  }
}

async function initializeTaxonomySortables() {
  await nextTick()
  destroyTaxonomySortables()

  if (subjectListRoot.value && orderedSubjects.value.length > 1) {
    const container = subjectListRoot.value
    subjectSortable = new Sortable(container, {
      dataIdAttr: 'data-subject-id',
      animation: 180,
      draggable: '.subject-row',
      handle: '.drag-handle',
      forceFallback: true,
      fallbackOnBody: true,
      fallbackTolerance: 4,
      chosenClass: 'is-drag-chosen',
      ghostClass: 'is-drag-ghost',
      dragClass: 'is-dragging',
      onEnd: (event) => {
        if (event.oldIndex === event.newIndex) return
        void saveDraggedSubjectOrder(orderedElementIds(container, 'subjectId'))
      },
    })
  }

  if (chapterListRoot.value && orderedChapters.value.length > 1) {
    const container = chapterListRoot.value
    chapterSortable = new Sortable(container, {
      dataIdAttr: 'data-chapter-id',
      animation: 180,
      draggable: '.chapter-row',
      handle: '.chapter-drag',
      forceFallback: true,
      fallbackOnBody: true,
      fallbackTolerance: 4,
      chosenClass: 'is-drag-chosen',
      ghostClass: 'is-drag-ghost',
      dragClass: 'is-dragging',
      onEnd: (event) => {
        if (event.oldIndex === event.newIndex) return
        void saveDraggedChapterOrder(orderedElementIds(container, 'chapterId'))
      },
    })
  }
}

onMounted(() => void initializeTaxonomySortables())
onBeforeUnmount(destroyTaxonomySortables)

watch(
  () => [
    orderedSubjects.value.map((subject) => subject.id).join(','),
    selectedSubjectId.value ?? '',
    orderedChapters.value.map((chapter) => chapter.id).join(','),
  ].join('|'),
  () => void initializeTaxonomySortables(),
  { flush: 'post' },
)

function handleSubjectCommand(command: string | number | object, subject: Subject) {
  if (command === 'edit') void editSubject(subject)
  if (command === 'delete') void deleteSubject(subject)
}

function formatRecentVisit(value?: number | null) {
  if (!value) return '从未访问'
  const date = new Date(value)
  const today = new Date()
  const time = new Intl.DateTimeFormat('zh-CN', {
    hour: '2-digit',
    minute: '2-digit',
    hour12: false,
  }).format(date)
  if (date.toDateString() === today.toDateString()) return `今天 ${time}`
  const yesterday = new Date(today)
  yesterday.setDate(today.getDate() - 1)
  if (date.toDateString() === yesterday.toDateString()) return `昨天 ${time}`
  return new Intl.DateTimeFormat('zh-CN', {
    year: 'numeric',
    month: '2-digit',
    day: '2-digit',
  }).format(date)
}
</script>

<template>
  <section class="page-main taxonomy-page">
      <div class="taxonomy-grid">
        <section class="surface subject-panel">
          <header class="panel-heading">
            <div>
              <h2>学科</h2>
              <span>{{ orderedSubjects.length }} 个</span>
            </div>
            <div class="panel-heading__actions">
              <span v-if="reordering" class="sort-status">正在保存排序…</span>
              <el-button circle plain :icon="Plus" aria-label="新增学科" @click="addSubject" />
            </div>
          </header>

          <div v-if="orderedSubjects.length" ref="subjectListRoot" class="subject-list">
            <button
              v-for="subject in orderedSubjects"
              :key="subject.id"
              type="button"
              class="subject-row"
              :data-subject-id="subject.id"
              :disabled="reordering"
              :class="{ 'is-active': subject.id === selectedSubjectId }"
              @click="selectedSubjectId = subject.id"
            >
              <span
                class="drag-handle"
                aria-label="拖动调整学科顺序"
                title="拖动排序"
              >⠿</span>
              <strong>{{ subject.name }}</strong>
              <span class="subject-count">{{ subject.questionCount }} 题</span>
              <el-dropdown trigger="click" @command="handleSubjectCommand($event, subject)">
                <span class="more-button" title="更多操作" @click.stop>
                  <el-icon><MoreFilled /></el-icon>
                </span>
                <template #dropdown>
                  <el-dropdown-menu>
                    <el-dropdown-item command="edit" :icon="EditPen">编辑学科</el-dropdown-item>
                    <el-dropdown-item command="delete" :icon="Delete" divided>删除学科</el-dropdown-item>
                  </el-dropdown-menu>
                </template>
              </el-dropdown>
            </button>
          </div>
          <div v-else class="panel-empty">
            <div class="panel-empty__icon">学</div>
            <strong>还没有学科</strong>
            <span>先建立第一个学科，再为它添加章节。</span>
            <el-button type="primary" size="small" :icon="Plus" @click="addSubject">新增学科</el-button>
          </div>
        </section>

        <section class="surface chapter-panel">
          <template v-if="selectedSubject">
            <header class="chapter-heading">
              <div>
                <h2>{{ selectedSubject.name }}</h2>
                <span>{{ orderedChapters.length }} 个章节 · {{ selectedSubject.questionCount }} 道题</span>
              </div>
              <div class="chapter-heading__actions">
                <el-button @click="editSubject(selectedSubject)">编辑学科</el-button>
                <el-button type="primary" :icon="Plus" @click="addChapter">新增章节</el-button>
              </div>
            </header>

            <div v-if="orderedChapters.length" ref="chapterListRoot" class="chapter-table">
              <div class="chapter-table__head">
                <span>排序</span>
                <span>章节名称</span>
                <span>题目数量</span>
                <span>最近访问</span>
                <span>操作</span>
              </div>
              <div
                v-for="chapter in orderedChapters"
                :key="chapter.id"
                class="chapter-row"
                :data-chapter-id="chapter.id"
              >
                <span
                  class="drag-handle chapter-drag"
                  aria-label="拖动调整章节顺序"
                  title="拖动排序"
                >⠿</span>
                <strong>{{ chapter.name }}</strong>
                <span>{{ chapter.questionCount }} 题</span>
                <span :class="{ muted: !chapter.lastAccessedAt }">{{ formatRecentVisit(chapter.lastAccessedAt) }}</span>
                <span class="row-actions">
                  <el-button link type="primary" @click="editChapter(chapter)">编辑</el-button>
                  <el-button link type="danger" @click="deleteChapter(chapter)">删除</el-button>
                </span>
              </div>
            </div>
            <div v-else class="chapter-empty">
              <div class="chapter-empty__icon">章</div>
              <strong>“{{ selectedSubject.name }}”还没有章节</strong>
              <span>章节用于进一步归类题目，之后也可以拖动调整顺序。</span>
              <el-button type="primary" size="small" :icon="Plus" @click="addChapter">新增章节</el-button>
            </div>

            <div class="warning-banner chapter-warning">
              删除章节前，系统会检查关联题目并说明影响范围；有题目的章节不能直接删除。
            </div>
          </template>
          <div v-else class="chapter-empty chapter-empty--full">
            <div class="chapter-empty__icon">章</div>
            <strong>请先选择一个学科</strong>
            <span>新增学科后，可以在这里继续维护章节。</span>
          </div>
        </section>
      </div>
  </section>
</template>

<style scoped>
.taxonomy-page {
  display: flex;
  flex-direction: column;
}

.sort-status {
  color: #2563eb;
  font-size: 11px;
  white-space: nowrap;
}

.panel-heading__actions {
  display: flex;
  align-items: center;
  gap: 8px;
}

.taxonomy-grid {
  min-height: 430px;
  flex: 1;
  display: grid;
  grid-template-columns: minmax(270px, 330px) minmax(560px, 1fr);
  gap: 14px;
}

.subject-panel,
.chapter-panel {
  min-height: 0;
  overflow: hidden;
}

.subject-panel {
  display: flex;
  flex-direction: column;
}

.panel-heading,
.chapter-heading {
  min-height: 58px;
  padding: 12px 14px;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  border-bottom: 1px solid var(--border);
}

.panel-heading > div,
.chapter-heading > div:first-child {
  min-width: 0;
  display: flex;
  align-items: baseline;
  gap: 8px;
}

.panel-heading h2,
.chapter-heading h2 {
  margin: 0;
  color: #111827;
  font-size: 15px;
}

.panel-heading span,
.chapter-heading span {
  color: #64748b;
  font-size: 11px;
}

.subject-list {
  padding: 8px;
  overflow: auto;
}

.subject-row {
  width: 100%;
  min-height: 46px;
  padding: 0 9px;
  display: grid;
  grid-template-columns: 18px minmax(0, 1fr) auto 26px;
  align-items: center;
  gap: 6px;
  border: 0;
  border-radius: 7px;
  background: transparent;
  color: #334155;
  text-align: left;
}

.subject-row:hover {
  background: #f8fafc;
}

.subject-row.is-active {
  background: #eaf3ff;
  color: #1d4ed8;
}

.subject-row strong {
  overflow: hidden;
  font-size: 12px;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.subject-count {
  color: #94a3b8;
  font-size: 11px;
  white-space: nowrap;
}

.drag-handle {
  color: #94a3b8;
  cursor: grab;
  font-size: 16px;
  line-height: 1;
  touch-action: none;
  user-select: none;
}

.drag-handle:active {
  cursor: grabbing;
}

.more-button {
  width: 26px;
  height: 26px;
  display: grid;
  place-items: center;
  border-radius: 5px;
  color: #94a3b8;
}

.more-button:hover {
  background: white;
  color: #2563eb;
}

.panel-empty,
.chapter-empty {
  min-height: 260px;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 8px;
  padding: 28px;
  color: #475569;
  text-align: center;
}

.panel-empty span,
.chapter-empty span {
  max-width: 320px;
  margin-bottom: 4px;
  color: #94a3b8;
  font-size: 11px;
  line-height: 1.65;
}

.panel-empty__icon,
.chapter-empty__icon {
  width: 42px;
  height: 42px;
  display: grid;
  place-items: center;
  border-radius: 12px;
  background: #eff6ff;
  color: #2563eb;
  font-weight: 700;
}

.chapter-panel {
  display: flex;
  flex-direction: column;
}

.chapter-heading__actions {
  display: flex;
  align-items: center;
  gap: 8px;
}

.chapter-table {
  overflow: auto;
}

.chapter-table__head,
.chapter-row {
  min-width: 680px;
  display: grid;
  grid-template-columns: 52px minmax(220px, 1.6fr) 105px 135px 110px;
  align-items: center;
}

.chapter-table__head {
  min-height: 44px;
  padding: 0 14px;
  border-bottom: 1px solid var(--border);
  background: #f8fafc;
  color: #64748b;
  font-size: 11px;
  font-weight: 600;
}

.chapter-row {
  min-height: 60px;
  padding: 0 14px;
  border-bottom: 1px solid #edf1f5;
  color: #334155;
  font-size: 12px;
}

.chapter-row:hover {
  background: #fbfdff;
}

.subject-row.is-drag-chosen,
.chapter-row.is-drag-chosen {
  background: #eff6ff;
  box-shadow: inset 0 0 0 1px #93c5fd;
}

.subject-row.is-drag-ghost,
.chapter-row.is-drag-ghost {
  opacity: 0.35;
}

.subject-row.is-dragging,
.chapter-row.is-dragging,
.subject-row.sortable-fallback,
.chapter-row.sortable-fallback {
  background: #fff;
  box-shadow: 0 10px 24px rgb(15 23 42 / 18%);
}

.chapter-row strong {
  overflow: hidden;
  color: #1f2937;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.chapter-drag {
  width: fit-content;
}

.row-actions {
  display: flex;
  align-items: center;
  gap: 2px;
}

.chapter-empty {
  flex: 1;
}

.chapter-empty--full {
  min-height: 100%;
}

.chapter-warning {
  margin: 18px 14px;
}

@media (max-width: 1280px) {
  .taxonomy-grid {
    grid-template-columns: 280px minmax(520px, 1fr);
  }
}
</style>
