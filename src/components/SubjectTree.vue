<script setup lang="ts">
import { computed, ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { ElMessage, ElMessageBox } from 'element-plus'
import { Search, ArrowDown, ArrowRight, Setting, Download, PriceTag, Delete, Tickets } from '@element-plus/icons-vue'
import AppContextMenu from './AppContextMenu.vue'
import { useAppStore } from '../stores/app'
import { errorMessage } from '../services/errors'
import type { Chapter, Subject } from '../types/domain'
import type { ContextMenuItem } from '../types/contextMenu'

const props = withDefaults(defineProps<{
  selectedSubjectId?: string
  selectedChapterId?: string
  title?: string
}>(), {
  title: '学科与章节',
})

const emit = defineEmits<{
  select: [payload: { subjectId?: string; chapterId?: string }]
}>()

const route = useRoute()
const router = useRouter()
const appStore = useAppStore()
const keyword = ref('')
const expanded = ref(new Set(appStore.subjects.map((subject) => subject.id)))
const contextMenuOpen = ref(false)
const contextMenuX = ref(0)
const contextMenuY = ref(0)
const contextMenuTitle = ref('')
const contextMenuItems = ref<ContextMenuItem[]>([])
const contextSubject = ref<Subject>()
const contextChapter = ref<Chapter>()

const visibleSubjects = computed(() => {
  const value = keyword.value.trim().toLocaleLowerCase('zh-CN')
  if (!value) return appStore.subjects
  return appStore.subjects
    .map((subject) => ({
      ...subject,
      chapters: subject.chapters.filter((chapter) => chapter.name.toLocaleLowerCase('zh-CN').includes(value)),
    }))
    .filter((subject) => subject.name.toLocaleLowerCase('zh-CN').includes(value) || subject.chapters.length)
})

function toggle(subjectId: string) {
  const next = new Set(expanded.value)
  if (next.has(subjectId)) next.delete(subjectId)
  else next.add(subjectId)
  expanded.value = next
}

function expandAll() {
  expanded.value = new Set(appStore.subjects.map((subject) => subject.id))
}

function collapseAll() {
  expanded.value = new Set()
}

function normalizeName(value: string) {
  return value.trim().normalize('NFKC').toLocaleLowerCase('zh-CN')
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
  if (chapter.questionCount) return
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
  contextSubject.value = subject
  contextChapter.value = undefined
  contextMenuTitle.value = `${subject.name} · ${subject.questionCount} 道题`
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
      dividerBefore: true,
    },
    { id: 'toggle', label: expanded.value.has(subject.id) ? '收起该学科章节' : '展开该学科章节' },
    { id: 'rename', label: '重命名学科', dividerBefore: true },
    { id: 'manage-order', label: '管理学科与章节顺序' },
    {
      id: 'delete',
      label: '删除学科',
      danger: true,
      dividerBefore: true,
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
  contextSubject.value = subject
  contextChapter.value = chapter
  contextMenuTitle.value = `${chapter.name} · ${chapter.questionCount} 道题`
  contextMenuItems.value = [
    { id: 'new-question', label: '在该章节新增题目' },
    { id: 'word-import', label: '批量导入到该章节' },
    {
      id: 'duplicate-check',
      label: '检查本章节重复题',
      disabled: chapter.questionCount === 0,
      hint: chapter.questionCount === 0 ? '该章节还没有题目' : undefined,
      dividerBefore: true,
    },
    { id: 'view', label: '查看本章节题目' },
    { id: 'rename', label: '重命名章节', dividerBefore: true },
    { id: 'manage-order', label: '管理章节顺序' },
    {
      id: 'delete',
      label: '删除章节',
      danger: true,
      dividerBefore: true,
      disabled: chapter.questionCount > 0,
      hint: chapter.questionCount ? `请先移走其中 ${chapter.questionCount} 道题` : undefined,
    },
  ]
  contextMenuX.value = event.clientX
  contextMenuY.value = event.clientY
  contextMenuOpen.value = true
}

function handleContextMenu(item: ContextMenuItem) {
  const subject = contextSubject.value
  const chapter = contextChapter.value
  if (!subject) return
  const defaultChapter = chapter ?? subject.chapters[0]
  if (item.id === 'new-question' && defaultChapter) {
    void router.push({
      path: '/questions/new',
      query: { subjectId: subject.id, chapterId: defaultChapter.id },
    })
  }
  if (item.id === 'word-import' && defaultChapter) {
    void router.push({
      path: '/word-import',
      query: { subjectId: subject.id, chapterId: defaultChapter.id },
    })
  }
  if (item.id === 'duplicate-check') {
    void router.push({
      path: '/duplicate-check',
      query: {
        subjectId: subject.id,
        ...(chapter ? { chapterId: chapter.id } : {}),
      },
    })
  }
  if (item.id === 'new-chapter') void createChapter(subject)
  if (item.id === 'toggle') toggle(subject.id)
  if (item.id === 'view' && chapter) emit('select', { subjectId: subject.id, chapterId: chapter.id })
  if (item.id === 'rename') {
    if (chapter) void renameChapter(subject, chapter)
    else void renameSubject(subject)
  }
  if (item.id === 'manage-order') void router.push('/taxonomy')
  if (item.id === 'delete') {
    if (chapter) void deleteChapter(chapter)
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
        title="管理学科与章节"
        aria-label="管理学科与章节"
        @click="router.push('/taxonomy')"
      >
        <el-icon><Setting /></el-icon>
      </el-button>
    </div>
    <div class="subject-tree__search">
      <el-input v-model="keyword" size="small" clearable placeholder="搜索学科或章节">
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
            <el-icon><ArrowDown v-if="expanded.has(subject.id)" /><ArrowRight v-else /></el-icon>
          </button>
          <button class="tree-label" @click="emit('select', { subjectId: subject.id })">{{ subject.name }}</button>
          <span class="tree-count">{{ subject.questionCount }}</span>
        </div>
        <div v-if="expanded.has(subject.id)" class="tree-children">
          <button
            v-for="chapter in subject.chapters"
            :key="chapter.id"
            class="tree-row tree-row--chapter"
            :class="{ 'is-active': props.selectedChapterId === chapter.id }"
            @click="emit('select', { subjectId: subject.id, chapterId: chapter.id })"
            @contextmenu.stop="openChapterContextMenu($event, subject, chapter)"
          >
            <span class="tree-label">{{ chapter.name }}</span>
            <span class="tree-count">{{ chapter.questionCount }}</span>
          </button>
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
      :title="contextMenuTitle"
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
  padding-left: 35px;
}

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
