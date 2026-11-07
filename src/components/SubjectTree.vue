<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { ElMessage, ElMessageBox } from 'element-plus'
import { Search, ArrowDown, ArrowRight, Setting, Download, PriceTag, Delete, Tickets } from '@element-plus/icons-vue'
import AppContextMenu from './AppContextMenu.vue'
import { useAppStore } from '../stores/app'
import { errorMessage } from '../services/errors'
import { askForName, normalizeName } from '../utils/taxonomyNamePrompt'
import type { Chapter, Subject, Unit } from '../types/domain'
import type { ContextMenuItem } from '../types/contextMenu'

const props = withDefaults(defineProps<{
  selectedSubjectId?: string
  selectedChapterId?: string
  selectedUnitId?: string | null
  unassignedUnit?: boolean
  title?: string
}>(), {
  title: '题目分类',
})

const emit = defineEmits<{
  select: [payload: { subjectId?: string; chapterId?: string; unitId?: string; unassignedUnit?: boolean }]
}>()

const route = useRoute()
const router = useRouter()
const appStore = useAppStore()
const keyword = ref('')
const expanded = ref(new Set(appStore.subjects.map((subject) => subject.id)))
const contextMenuOpen = ref(false)
const contextMenuX = ref(0)
const contextMenuY = ref(0)
const contextMenuItems = ref<ContextMenuItem[]>([])
const contextSubject = ref<Subject>()
const contextChapter = ref<Chapter>()
const contextUnit = ref<Unit>()

watch(() => [props.selectedSubjectId, props.selectedChapterId, props.selectedUnitId, props.unassignedUnit], () => {
  if (props.selectedUnitId || props.unassignedUnit) {
    expanded.value = new Set([...expanded.value, ...[props.selectedSubjectId, props.selectedChapterId].filter((id): id is string => Boolean(id))])
  }
}, { immediate: true })

const visibleSubjects = computed(() => {
  const value = keyword.value.trim().toLocaleLowerCase('zh-CN')
  if (!value) return appStore.subjects
  return appStore.subjects.map((subject) => {
    if (subject.name.toLocaleLowerCase('zh-CN').includes(value)) return subject
    return { ...subject, chapters: subject.chapters.flatMap((chapter) => {
      if (chapter.name.toLocaleLowerCase('zh-CN').includes(value)) return [chapter]
      const units = chapter.units?.filter((unit) => unit.name.toLocaleLowerCase('zh-CN').includes(value)) ?? []
      return units.length ? [{ ...chapter, units }] : []
    }) }
  }).filter((subject) => subject.chapters.length || subject.name.toLocaleLowerCase('zh-CN').includes(value))
})

function isExpanded(id: string) { return Boolean(keyword.value.trim()) || expanded.value.has(id) }
function unassignedCount(chapter: Chapter) {
  const original = appStore.subjects.flatMap((subject) => subject.chapters).find((item) => item.id === chapter.id) ?? chapter
  return Math.max(0, original.questionCount - (original.units ?? []).reduce((sum, unit) => sum + unit.questionCount, 0))
}
async function createUnit(chapter: Chapter) {
  const name = await askForName(`在“${chapter.name}”中新增单元`)
  if (!name) return
  try {
    await appStore.createUnit(chapter.id, name)
    expanded.value = new Set([...expanded.value, chapter.subjectId, chapter.id])
    ElMessage.success('单元已新增')
  } catch (reason) { ElMessage.error(errorMessage(reason, '新增单元失败')) }
}
async function renameUnit(unit: Unit) {
  const name = await askForName('重命名单元', unit.name)
  if (!name || normalizeName(name) === normalizeName(unit.name)) return
  try { await appStore.updateUnit(unit.id, name); ElMessage.success('单元名称已更新') }
  catch (reason) { ElMessage.error(errorMessage(reason, '更新单元失败')) }
}
async function deleteUnit(unit: Unit) {
  if (unit.questionCount) return
  try { await ElMessageBox.confirm(`确定删除单元“${unit.name}”吗？`, '删除单元', { type: 'warning' }) }
  catch { return }
  try { await appStore.deleteUnit(unit.id); ElMessage.success('单元已删除') }
  catch (reason) { ElMessage.error(errorMessage(reason, '删除单元失败')) }
}
function openUnitContextMenu(event: MouseEvent, subject: Subject, chapter: Chapter, unit: Unit) {
  openChapterContextMenu(event, subject, chapter)
  contextUnit.value = unit
  contextMenuItems.value = [
    { id: 'new-question', label: '在该单元新增题目' },
    { id: 'word-import', label: '批量导入到该单元' },
    { id: 'duplicate-check', label: '检查本单元重复题', disabled: !unit.questionCount },
    { id: 'view', label: '查看本单元题目' },
    { id: 'rename', label: '重命名单元' },
    { id: 'manage-order', label: '管理单元顺序' },
    { id: 'delete', label: '删除单元', danger: true, disabled: Boolean(unit.questionCount), hint: unit.questionCount ? '请先移走题目' : undefined },
  ]
}

function toggle(subjectId: string) {
  const next = new Set(expanded.value)
  if (next.has(subjectId)) next.delete(subjectId)
  else next.add(subjectId)
  expanded.value = next
}

function expandAll() {
  expanded.value = new Set(appStore.subjects.flatMap((subject) => [subject.id, ...subject.chapters.map((chapter) => chapter.id)]))
}

function collapseAll() {
  expanded.value = new Set()
}

async function createChapter(subject: Subject) {
  const name = await askForName(`在“${subject.name}”中新增章节`)
  if (!name) return
  if (subject.chapters.some((chapter) => normalizeName(chapter.name) === normalizeName(name))) {
    ElMessage.warning('这个学科中已经存在同名章节')
    return
  }
  try {
    await appStore.createChapter(subject.id, name)
    expanded.value = new Set([...expanded.value, subject.id])
    ElMessage.success(`已新增章节“${name}”`)
  } catch (reason) {
    ElMessage.error(errorMessage(reason, '新增章节失败'))
  }
}

async function renameSubject(subject: Subject) {
  const name = await askForName('重命名学科', subject.name, 50)
  if (!name || normalizeName(name) === normalizeName(subject.name)) return
  if (appStore.subjects.some((entry) => entry.id !== subject.id && normalizeName(entry.name) === normalizeName(name))) {
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

async function renameChapter(subject: Subject, chapter: Chapter) {
  const name = await askForName('重命名章节', chapter.name)
  if (!name || normalizeName(name) === normalizeName(chapter.name)) return
  if (subject.chapters.some((entry) => entry.id !== chapter.id && normalizeName(entry.name) === normalizeName(name))) {
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

async function deleteSubject(subject: Subject) {
  if (subject.chapters.length || subject.questionCount) return
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

async function deleteChapter(chapter: Chapter) {
  if (chapter.questionCount || chapter.units?.length) return
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

function openSubjectContextMenu(event: MouseEvent, subject: Subject) {
  event.preventDefault()
  contextUnit.value = undefined
  contextSubject.value = subject
  contextChapter.value = undefined
  const hasNoChapter = subject.chapters.length === 0
  contextMenuItems.value = [
    {
      id: 'new-question',
      label: '在该学科新增题目',
      disabled: hasNoChapter,
      hint: hasNoChapter ? '请先新建章节' : undefined,
    },
    { id: 'new-chapter', label: '新建章节' },
    {
      id: 'word-import',
      label: '批量导入到该学科',
      disabled: hasNoChapter,
      hint: hasNoChapter ? '请先新建章节' : undefined,
    },
    {
      id: 'duplicate-check',
      label: '检查本学科重复题',
      disabled: subject.questionCount === 0,
      hint: subject.questionCount === 0 ? '该学科还没有题目' : undefined,
    },
    { id: 'rename', label: '重命名学科' },
    { id: 'manage-order', label: '管理学科、章节与单元顺序' },
    {
      id: 'delete',
      label: '删除学科',
      danger: true,
      disabled: Boolean(subject.chapters.length || subject.questionCount),
      hint: subject.chapters.length || subject.questionCount ? '请先移走题目并删除章节' : undefined,
    },
  ]
  contextMenuX.value = event.clientX
  contextMenuY.value = event.clientY
  contextMenuOpen.value = true
}

function openChapterContextMenu(event: MouseEvent, subject: Subject, chapter: Chapter) {
  event.preventDefault()
  contextUnit.value = undefined
  contextSubject.value = subject
  contextChapter.value = chapter
  contextMenuItems.value = [
    { id: 'new-question', label: '在该章节新增题目' },
    { id: 'word-import', label: '批量导入到该章节' },
    {
      id: 'duplicate-check',
      label: '检查本章节重复题',
      disabled: chapter.questionCount === 0,
      hint: chapter.questionCount === 0 ? '该章节还没有题目' : undefined,
    },
    { id: 'view', label: '查看本章节题目' },
    { id: 'new-unit', label: '新建单元' },
    { id: 'rename', label: '重命名章节' },
    { id: 'manage-order', label: '管理章节顺序' },
    {
      id: 'delete',
      label: '删除章节',
      danger: true,
      disabled: chapter.questionCount > 0 || Boolean(chapter.units?.length),
      hint: chapter.questionCount ? `请先移走其中 ${chapter.questionCount} 道题` : chapter.units?.length ? '请先删除空单元' : undefined,
    },
  ]
  contextMenuX.value = event.clientX
  contextMenuY.value = event.clientY
  contextMenuOpen.value = true
}

function handleContextMenu(item: ContextMenuItem) {
  const subject = contextSubject.value
  const chapter = contextChapter.value
  const unit = contextUnit.value
  if (!subject) return
  const defaultChapter = chapter ?? subject.chapters[0]
  if (item.id === 'new-question' && defaultChapter) {
    void router.push({
      path: '/questions/new',
      query: { subjectId: subject.id, chapterId: defaultChapter.id, ...(unit ? { unitId: unit.id } : {}) },
    })
  }
  if (item.id === 'word-import' && defaultChapter) {
    void router.push({
      path: '/word-import',
      query: { subjectId: subject.id, chapterId: defaultChapter.id, ...(unit ? { unitId: unit.id } : {}) },
    })
  }
  if (item.id === 'duplicate-check') {
    void router.push({
      path: '/duplicate-check',
      query: {
        subjectId: subject.id,
        ...(chapter ? { chapterId: chapter.id } : {}),
        ...(unit ? { unitId: unit.id } : {}),
      },
    })
  }
  if (item.id === 'new-unit' && chapter) void createUnit(chapter)
  if (item.id === 'new-chapter') void createChapter(subject)
  if (item.id === 'view' && chapter) emit('select', { subjectId: subject.id, chapterId: chapter.id, unitId: unit?.id })
  if (item.id === 'rename') {
    if (unit) void renameUnit(unit)
    else if (chapter) void renameChapter(subject, chapter)
    else void renameSubject(subject)
  }
  if (item.id === 'manage-order') void router.push('/taxonomy')
  if (item.id === 'delete') {
    if (unit) void deleteUnit(unit)
    else if (chapter) void deleteChapter(chapter)
    else void deleteSubject(subject)
  }
}
</script>

<template>
  <aside class="subject-tree page-sidebar">
    <div class="subject-tree__heading">
      <strong>{{ title }}</strong>
      <el-button
        class="taxonomy-manage-button"
        :class="{ 'is-active': route.path === '/taxonomy' }"
        text
        circle
        size="small"
        title="管理学科、章节与单元"
        aria-label="管理学科、章节与单元"
        @click="router.push('/taxonomy')"
      >
        <el-icon><Setting /></el-icon>
      </el-button>
    </div>
    <div class="subject-tree__search">
      <el-input v-model="keyword" size="small" clearable placeholder="搜索学科、章节或单元">
        <template #prefix><el-icon><Search /></el-icon></template>
      </el-input>
    </div>
    <div class="subject-tree__tools">
      <button @click="expandAll">展开全部</button>
      <span>·</span>
      <button @click="collapseAll">收起全部</button>
    </div>

    <div class="subject-tree__tree">
      <button
        class="tree-row tree-row--root"
        :class="{ 'is-active': !props.selectedSubjectId && !props.selectedChapterId }"
        @click="emit('select', {})"
      >
        <span>全部题目</span>
        <span>{{ appStore.questionCount }}</span>
      </button>

      <div v-for="subject in visibleSubjects" :key="subject.id" class="tree-group">
        <div
          class="tree-row tree-row--subject"
          :class="{ 'is-active': props.selectedSubjectId === subject.id && !props.selectedChapterId }"
          @contextmenu.stop="openSubjectContextMenu($event, subject)"
        >
          <button class="tree-toggle" :aria-label="expanded.has(subject.id) ? '收起' : '展开'" @click="toggle(subject.id)">
            <el-icon><ArrowDown v-if="isExpanded(subject.id)" /><ArrowRight v-else /></el-icon>
          </button>
          <button class="tree-label" @click="emit('select', { subjectId: subject.id })">{{ subject.name }}</button>
          <span class="tree-count">{{ subject.questionCount }}</span>
        </div>
        <div v-if="isExpanded(subject.id)" class="tree-children">
          <div v-for="chapter in subject.chapters" :key="chapter.id">
            <div class="tree-row tree-row--chapter" :class="{ 'is-active': props.selectedChapterId === chapter.id && !props.selectedUnitId && !props.unassignedUnit }" @contextmenu.stop="openChapterContextMenu($event, subject, chapter)">
              <button class="tree-toggle" :aria-label="isExpanded(chapter.id) ? '收起章节' : '展开章节'" @click="toggle(chapter.id)">
                <el-icon><ArrowDown v-if="isExpanded(chapter.id)" /><ArrowRight v-else /></el-icon>
              </button>
              <button class="tree-label" :title="chapter.name" @click="emit('select', { subjectId: subject.id, chapterId: chapter.id })">{{ chapter.name }}</button>
              <span class="tree-count">{{ chapter.questionCount }}</span>
            </div>
            <template v-if="isExpanded(chapter.id)">
              <button v-for="unit in chapter.units ?? []" :key="unit.id" class="tree-row tree-row--unit" :class="{ 'is-active': props.selectedUnitId === unit.id }" :title="unit.name" @click="emit('select', { subjectId: subject.id, chapterId: chapter.id, unitId: unit.id })" @contextmenu.stop="openUnitContextMenu($event, subject, chapter, unit)">
                <span class="tree-label">{{ unit.name }}</span><span class="tree-count">{{ unit.questionCount }}</span>
              </button>
              <button v-if="unassignedCount(chapter) && !keyword.trim()" class="tree-row tree-row--unit" :class="{ 'is-active': props.selectedChapterId === chapter.id && props.unassignedUnit }" @click="emit('select', { subjectId: subject.id, chapterId: chapter.id, unassignedUnit: true })">
                <span class="tree-label">未分单元</span><span class="tree-count">{{ unassignedCount(chapter) }}</span>
              </button>
            </template>
          </div>
        </div>
      </div>
    </div>

    <nav class="management-shortcuts" aria-label="题库管理工具">
      <button type="button" @click="router.push('/data?section=export')">
        <span class="management-shortcuts__icon"><el-icon><Download /></el-icon></span>
        <span>题库导出</span>
      </button>
      <button
        type="button"
        :class="{ 'is-active': route.path === '/tags' }"
        :aria-current="route.path === '/tags' ? 'page' : undefined"
        @click="router.push('/tags')"
      >
        <span class="management-shortcuts__icon"><el-icon><PriceTag /></el-icon></span>
        <span>标签管理</span>
      </button>
      <button
        type="button"
        :class="{ 'is-active': route.path === '/question-types' }"
        :aria-current="route.path === '/question-types' ? 'page' : undefined"
        @click="router.push('/question-types')"
      >
        <span class="management-shortcuts__icon"><el-icon><Tickets /></el-icon></span>
        <span>题型管理</span>
      </button>
      <button
        type="button"
        :class="{ 'is-active': route.path === '/recycle-bin' }"
        :aria-current="route.path === '/recycle-bin' ? 'page' : undefined"
        @click="router.push('/recycle-bin')"
      >
        <span class="management-shortcuts__icon"><el-icon><Delete /></el-icon></span>
        <span>回收站</span>
      </button>
    </nav>
    <AppContextMenu
      v-model="contextMenuOpen"
      :x="contextMenuX"
      :y="contextMenuY"
      :items="contextMenuItems"
      @select="handleContextMenu"
    />
  </aside>
</template>

<style scoped>
.subject-tree {
  padding-bottom: 0;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

.subject-tree__heading {
  flex: 0 0 54px;
  height: 54px;
  padding: 0 14px 0 18px;
  display: flex;
  align-items: center;
  justify-content: space-between;
  font-size: 13px;
}

.subject-tree__search {
  flex: 0 0 auto;
  padding: 0 14px 8px;
}

.taxonomy-manage-button {
  color: #64748b;
}

.taxonomy-manage-button:hover,
.taxonomy-manage-button.is-active {
  background: #eff6ff;
  color: #2563eb;
}

.subject-tree__tools {
  flex: 0 0 auto;
  padding: 2px 18px 9px;
  display: flex;
  gap: 6px;
  color: #94a3b8;
  font-size: 10px;
}

.subject-tree__tree {
  min-height: 0;
  flex: 1;
  overflow: auto;
  padding-bottom: 8px;
}

.subject-tree__tools button {
  padding: 0;
  border: 0;
  background: transparent;
  color: #64748b;
  font-size: inherit;
}

.tree-row {
  width: calc(100% - 16px);
  min-height: 36px;
  margin: 1px 8px;
  padding: 0 10px;
  display: flex;
  align-items: center;
  gap: 6px;
  border: 0;
  border-radius: 7px;
  background: transparent;
  color: #475569;
  font-size: 11px;
  text-align: left;
}

.tree-row:hover,
.tree-row.is-active {
  background: #eff6ff;
  color: #1d4ed8;
}

.tree-row.is-active {
  font-weight: 600;
}

.tree-row--root {
  justify-content: space-between;
}

.tree-row--subject {
  padding-left: 4px;
}

.tree-row--chapter {
  padding-left: 20px;
}

.tree-row--unit { padding-left: 58px; }

.tree-toggle {
  width: 26px;
  height: 30px;
  padding: 0;
  display: grid;
  place-items: center;
  border: 0;
  background: transparent;
  color: inherit;
}

.tree-label {
  min-width: 0;
  flex: 1;
  overflow: hidden;
  border: 0;
  background: transparent;
  color: inherit;
  font-size: inherit;
  text-align: left;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.tree-count {
  color: #94a3b8;
  font-size: 10px;
}

.management-shortcuts {
  margin: 10px 12px 12px;
  padding: 8px;
  flex: 0 0 auto;
  display: grid;
  grid-template-columns: repeat(4, minmax(0, 1fr));
  gap: 4px;
  border: 1px solid #e2e8f0;
  border-radius: 8px;
  background: #fff;
}

.management-shortcuts button {
  min-width: 0;
  min-height: 62px;
  padding: 7px 2px;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 6px;
  border: 0;
  border-radius: 7px;
  background: transparent;
  color: #475569;
  font-size: 10px;
}

.management-shortcuts button:hover,
.management-shortcuts button.is-active {
  background: #eff6ff;
  color: #1d4ed8;
}

.management-shortcuts__icon {
  width: 28px;
  height: 28px;
  display: grid;
  place-items: center;
  border-radius: 8px;
  background: #f1f5f9;
  color: #64748b;
  font-size: 16px;
}

.management-shortcuts button:hover .management-shortcuts__icon,
.management-shortcuts button.is-active .management-shortcuts__icon {
  background: #dbeafe;
  color: #2563eb;
}
</style>
