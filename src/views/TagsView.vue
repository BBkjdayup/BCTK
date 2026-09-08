<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import { Delete, EditPen, Plus, Search } from '@element-plus/icons-vue'
import { useAppStore } from '../stores/app'
import { errorMessage } from '../services/errors'
import type { Tag } from '../types/domain'

const appStore = useAppStore()
const keyword = ref('')
const currentPage = ref(1)
const pageSize = 8
const selectedTagId = ref<string>()
const palette = ['#2563eb', '#f59e0b', '#0f9f9a', '#7c3aed', '#ec4899', '#16a34a']

const filteredTags = computed<Tag[]>(() => {
  const query = keyword.value.trim().normalize('NFKC').toLocaleLowerCase('zh-CN')
  return [...appStore.tags]
    .filter((tag: Tag) => !query || tag.name.normalize('NFKC').toLocaleLowerCase('zh-CN').includes(query))
    .sort((left: Tag, right: Tag) => right.questionCount - left.questionCount || left.name.localeCompare(right.name, 'zh-CN'))
})

const pageTags = computed(() => {
  const start = (currentPage.value - 1) * pageSize
  return filteredTags.value.slice(start, start + pageSize)
})

watch(keyword, () => { currentPage.value = 1 })
watch(
  () => filteredTags.value.length,
  (total) => {
    const lastPage = Math.max(1, Math.ceil(total / pageSize))
    if (currentPage.value > lastPage) currentPage.value = lastPage
  },
)

function normalizeName(value: string) {
  return value.trim().normalize('NFKC').toLocaleLowerCase('zh-CN')
}

function tagNameExists(name: string, exceptId?: string) {
  const key = normalizeName(name)
  return appStore.tags.some((tag: Tag) => tag.id !== exceptId && normalizeName(tag.name) === key)
}

async function askForTagName(title: string, initialValue = ''): Promise<string | null> {
  try {
    const result = await ElMessageBox.prompt('标签可用于题目筛选和组卷时快速定位。', title, {
      inputValue: initialValue,
      inputPlaceholder: '请输入标签名称',
      confirmButtonText: '保存',
      cancelButtonText: '取消',
      inputValidator: (value: string) => {
        const name = value.trim().normalize('NFKC').trim()
        if (!name) return '标签名称不能为空'
        return [...name].length <= 50 || '标签名称不能超过 50 个字符'
      },
    })
    return result.value.trim().normalize('NFKC').trim()
  } catch {
    return null
  }
}

async function addTag() {
  const name = await askForTagName('新增标签')
  if (!name) return
  if (tagNameExists(name)) {
    ElMessage.warning('已经存在同名标签')
    return
  }

  try {
    selectedTagId.value = await appStore.createTag(name)
    currentPage.value = 1
    ElMessage.success(`已新增标签“${name}”`)
  } catch (reason) {
    ElMessage.error(errorMessage(reason, '新增标签失败'))
  }
}

async function editTag(tag: Tag) {
  const name = await askForTagName('编辑标签', tag.name)
  if (!name || normalizeName(name) === normalizeName(tag.name)) return
  if (tagNameExists(name, tag.id)) {
    ElMessage.warning('已经存在同名标签')
    return
  }

  try {
    await appStore.updateTag(tag.id, name)
    ElMessage.success('标签名称已更新')
  } catch (reason) {
    ElMessage.error(errorMessage(reason, '更新标签失败'))
  }
}

async function deleteTag(tag: Tag) {
  const impact = tag.questionCount
    ? `这个标签当前关联 ${tag.questionCount} 道题。删除后只会解除标签关系，不会删除题目。`
    : '这个标签还没有关联题目。'
  try {
    await ElMessageBox.confirm(`${impact} 确定删除“${tag.name}”吗？`, '删除标签', {
      type: 'warning',
      confirmButtonText: '删除标签',
      cancelButtonText: '取消',
    })
  } catch {
    return
  }

  try {
    await appStore.deleteTag(tag.id)
    if (selectedTagId.value === tag.id) selectedTagId.value = undefined
    ElMessage.success('标签已删除，题目内容未受影响')
  } catch (reason) {
    ElMessage.error(errorMessage(reason, '删除标签失败'))
  }
}

function tagColor(tag: Tag) {
  let hash = 0
  for (const character of tag.id) hash = (hash * 31 + character.charCodeAt(0)) >>> 0
  return palette[hash % palette.length] ?? palette[0] ?? '#2563eb'
}

function formatDate(value: number) {
  return new Intl.DateTimeFormat('zh-CN', {
    year: 'numeric',
    month: '2-digit',
    day: '2-digit',
  }).format(value)
}

function rowClassName({ row }: { row: Tag }) {
  return row.id === selectedTagId.value ? 'is-selected-tag' : ''
}
</script>

<template>
  <section class="page-main tags-page">
      <div class="surface search-card">
        <el-input
          v-model="keyword"
          clearable
          :prefix-icon="Search"
          placeholder="搜索标签名称"
          class="tag-search"
        />
        <div class="search-card__actions">
          <span>共 {{ appStore.tags.length }} 个标签</span>
          <el-button type="primary" :icon="Plus" @click="addTag">新增标签</el-button>
        </div>
      </div>

      <div v-if="appStore.tags.length" class="surface tag-cloud" aria-label="标签概览">
        <button
          v-for="tag in filteredTags"
          :key="tag.id"
          type="button"
          class="tag-chip"
          :class="{ 'is-active': selectedTagId === tag.id }"
          @click="selectedTagId = tag.id"
        >
          <span class="tag-dot" :style="{ backgroundColor: tagColor(tag) }" />
          <strong>{{ tag.name }}</strong>
          <span>{{ tag.questionCount }}</span>
        </button>
        <span v-if="!filteredTags.length" class="tag-cloud__empty">没有匹配的标签</span>
      </div>

      <div class="surface tag-table-card">
        <el-table
          :data="pageTags"
          row-key="id"
          :row-class-name="rowClassName"
          @row-click="selectedTagId = $event.id"
        >
          <el-table-column label="标签名称" min-width="300">
            <template #default="{ row }">
              <div class="tag-name-cell">
                <span class="tag-dot" :style="{ backgroundColor: tagColor(row) }" />
                <strong>{{ row.name }}</strong>
              </div>
            </template>
          </el-table-column>
          <el-table-column label="关联题目" width="150">
            <template #default="{ row }"><span class="count-cell">{{ row.questionCount }} 道题</span></template>
          </el-table-column>
          <el-table-column label="创建时间" width="165">
            <template #default="{ row }">{{ formatDate(row.createdAt) }}</template>
          </el-table-column>
          <el-table-column label="操作" width="150" fixed="right">
            <template #default="{ row }">
              <el-button link type="primary" :icon="EditPen" @click.stop="editTag(row)">编辑</el-button>
              <el-button link type="danger" :icon="Delete" @click.stop="deleteTag(row)">删除</el-button>
            </template>
          </el-table-column>
          <template #empty>
            <div class="empty-tags">
              <strong>{{ appStore.tags.length ? '没有匹配的标签' : '还没有标签' }}</strong>
              <span>{{ appStore.tags.length ? '请尝试其他关键词。' : '新增标签后，可以在录题和筛选时使用。' }}</span>
              <el-button v-if="!appStore.tags.length" size="small" type="primary" :icon="Plus" @click="addTag">新增标签</el-button>
            </div>
          </template>
        </el-table>
        <footer class="tag-pagination">
          <span>显示 {{ pageTags.length ? (currentPage - 1) * pageSize + 1 : 0 }}–{{ (currentPage - 1) * pageSize + pageTags.length }}，共 {{ filteredTags.length }} 个标签</span>
          <el-pagination
            v-model:current-page="currentPage"
            background
            layout="prev, pager, next"
            :page-size="pageSize"
            :total="filteredTags.length"
          />
        </footer>
      </div>
  </section>
</template>

<style scoped>
.tags-page {
  display: flex;
  flex-direction: column;
}

.search-card {
  min-height: 58px;
  margin-bottom: 12px;
  padding: 9px 12px;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 18px;
}

.search-card__actions {
  display: flex;
  align-items: center;
  gap: 12px;
}

.search-card__actions > span {
  color: #64748b;
  font-size: 11px;
  white-space: nowrap;
}

.tag-search {
  width: 280px;
}

.tag-cloud {
  min-height: 90px;
  margin-bottom: 12px;
  padding: 14px;
  display: flex;
  align-content: flex-start;
  align-items: center;
  flex-wrap: wrap;
  gap: 8px;
}

.tag-chip {
  height: 34px;
  padding: 0 11px;
  display: inline-flex;
  align-items: center;
  gap: 7px;
  border: 1px solid #dbe3ee;
  border-radius: 7px;
  background: white;
  color: #334155;
  font-size: 11px;
}

.tag-chip:hover,
.tag-chip.is-active {
  border-color: #93c5fd;
  background: #eff6ff;
  color: #1d4ed8;
}

.tag-chip strong {
  font-weight: 600;
}

.tag-chip > span:last-child {
  color: #2563eb;
}

.tag-dot {
  width: 7px;
  height: 7px;
  flex: 0 0 7px;
  border-radius: 50%;
}

.tag-cloud__empty {
  margin: auto;
  color: #94a3b8;
  font-size: 12px;
}

.tag-table-card {
  min-height: 0;
  flex: 1;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

.tag-table-card :deep(.el-table) {
  flex: 1;
}

.tag-table-card :deep(.el-table__header-wrapper th) {
  background: #f8fafc;
  color: #64748b;
  font-size: 11px;
}

.tag-table-card :deep(.el-table__row) {
  height: 60px;
  font-size: 12px;
}

.tag-table-card :deep(.el-table__row.is-selected-tag td) {
  background: #f0f7ff !important;
}

.tag-name-cell {
  display: flex;
  align-items: center;
  gap: 9px;
}

.tag-name-cell strong {
  color: #1f2937;
}

.count-cell {
  color: #1d4ed8;
}

.tag-pagination {
  min-height: 52px;
  padding: 0 14px;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
  border-top: 1px solid var(--border);
  color: #64748b;
  font-size: 11px;
}

.empty-tags {
  min-height: 260px;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 9px;
  color: #475569;
}

.empty-tags span {
  color: #94a3b8;
  font-size: 11px;
}
</style>
