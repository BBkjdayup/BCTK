<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import { Delete, RefreshLeft } from '@element-plus/icons-vue'
import QuestionStemSummary from '../components/QuestionStemSummary.vue'
import { useQuestionBankStore } from '../stores/questionBank'
import { useAppStore } from '../stores/app'
import { type Question } from '../types/domain'
import { backend } from '../services/backend'
import { errorMessage } from '../services/errors'
import { questionTypeLabel } from '../utils/questionTypes'

const store = useQuestionBankStore()
const appStore = useAppStore()
const retentionDays = ref(30)
const recyclePolicy = ref<'manual_only' | 'remind_only'>('remind_only')
store.initializeFilters({ deleted: true, pageSize: 100 })

onMounted(() => {
  void store.load()
  void backend.getSettings()
    .then((settings) => {
      retentionDays.value = settings.recycleRetentionDays
      recyclePolicy.value = settings.recyclePolicy
    })
    .catch(() => undefined)
})

function onSelectionChange(items: Question[]) {
  store.selectedIds = items.map((item) => item.id)
}

async function restore(ids: string[]) {
  try {
    await store.restore(ids)
    ElMessage.success(`已恢复 ${ids.length} 道题`)
  } catch (reason) {
    ElMessage.error(errorMessage(reason, '恢复题目失败，请重试。'))
  }
}

async function permanentlyDelete(ids: string[]) {
  try {
    await ElMessageBox.confirm(
      `永久删除选中的 ${ids.length} 道题？此操作无法撤销，软件会同时检查相关图片是否仍被历史试卷引用。`,
      '永久删除确认',
      { type: 'error', confirmButtonText: '永久删除', cancelButtonText: '取消', confirmButtonClass: 'el-button--danger' },
    )
  } catch {
    return
  }
  try {
    await store.permanentlyDelete(ids)
    ElMessage.success('题目已永久删除')
  } catch (reason) {
    ElMessage.error(errorMessage(reason, '题目未能永久删除，请重试。'))
  }
}

function daysRemaining(deletedAt?: number | null) {
  if (!deletedAt) return retentionDays.value
  return Math.max(0, retentionDays.value - Math.floor((Date.now() - deletedAt) / 86400000))
}
</script>

<template>
  <section class="page-main recycle-page">
      <div class="recycle-note">
        <span>已删除题目按设置提醒保留 {{ retentionDays }} 天。{{ recyclePolicy === 'manual_only' ? '只允许你手动清理' : '到期后提醒并由你决定' }}，软件不会自动永久删除。</span>
        <div v-if="store.selectedIds.length" class="recycle-note__actions">
          <el-button
            :icon="RefreshLeft"
            @click="restore([...store.selectedIds])"
          >批量恢复</el-button>
          <el-button
            type="danger"
            plain
            :icon="Delete"
            @click="permanentlyDelete([...store.selectedIds])"
          >永久删除</el-button>
        </div>
      </div>

      <div class="surface recycle-table">
        <el-table v-loading="store.loading" :data="store.questions" row-key="id" height="100%" @selection-change="onSelectionChange">
          <el-table-column type="selection" width="48" />
          <el-table-column label="题型" width="100">
            <template #default="{ row }">{{ questionTypeLabel(row.type, appStore.questionTypes) }}</template>
          </el-table-column>
          <el-table-column label="题目内容" min-width="360">
            <template #default="{ row }">
              <QuestionStemSummary :content="row.stem" :lines="2" />
            </template>
          </el-table-column>
          <el-table-column prop="subjectName" label="学科" width="130" />
          <el-table-column prop="chapterName" label="章节" min-width="170" show-overflow-tooltip />
          <el-table-column label="剩余保留" width="110">
            <template #default="{ row }"><el-tag size="small" type="warning" effect="plain">{{ daysRemaining(row.deletedAt) }} 天</el-tag></template>
          </el-table-column>
          <el-table-column label="操作" width="160" fixed="right">
            <template #default="{ row }">
              <el-button link type="primary" @click="restore([row.id])">恢复</el-button>
              <el-button link type="danger" @click="permanentlyDelete([row.id])">永久删除</el-button>
            </template>
          </el-table-column>
          <template #empty><div class="empty-state">回收站是空的</div></template>
        </el-table>
        <div class="pagination-row">
          <span>共 {{ store.total }} 道题</span>
          <el-pagination
            v-model:current-page="store.filters.page"
            background
            layout="prev, pager, next"
            :page-size="store.filters.pageSize"
            :total="store.total"
            @current-change="store.load"
          />
        </div>
      </div>
  </section>
</template>

<style scoped>
.recycle-note {
  margin-bottom: 12px;
  padding: 12px;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
  border: 1px solid #dbeafe;
  border-radius: 7px;
  background: #f8fbff;
  color: #64748b;
  font-size: 10px;
  line-height: 1.7;
}

.recycle-note__actions {
  display: flex;
  align-items: center;
  flex-shrink: 0;
  gap: 8px;
}

.recycle-note__actions :deep(.el-button + .el-button) {
  margin-left: 0;
}

.recycle-page {
  display: flex;
  flex-direction: column;
}

.recycle-table {
  min-height: 380px;
  flex: 1;
  display: grid;
  grid-template-rows: minmax(320px, 1fr) 50px;
  overflow: hidden;
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
</style>
