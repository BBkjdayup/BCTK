<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { ElMessage, ElMessageBox } from 'element-plus'
import PageHeader from '../components/PageHeader.vue'
import QuestionPreviewDrawer from '../components/QuestionPreviewDrawer.vue'
import { backend } from '../services/backend'
import { errorMessage } from '../services/errors'
import { useAppStore } from '../stores/app'
import { useQuestionBankStore } from '../stores/questionBank'
import type { Question, QuestionDuplicateGroup, QuestionDuplicateScanResult } from '../types/domain'
import { reconcileDuplicateCheckTab, type DuplicateCheckTab } from '../utils/duplicateCheckTabs'
import { questionTypeLabel } from '../utils/questionTypes'

const route = useRoute()
const router = useRouter()
const appStore = useAppStore()
const bankStore = useQuestionBankStore()
const loading = ref(false)
const operationGroupId = ref<string | null>(null)
const result = ref<QuestionDuplicateScanResult | null>(null)
const loadError = ref('')
const activeTab = ref<DuplicateCheckTab>('exact')
const keepers = ref<Record<string, string>>({})
const previewOpen = ref(false)
const previewQuestion = ref<Question | null>(null)
let scanSequence = 0

function queryText(value: unknown) {
  return typeof value === 'string' ? value : ''
}

const subjectId = computed(() => queryText(route.query.subjectId))
const chapterId = computed(() => queryText(route.query.chapterId))
const subject = computed(() => appStore.subjects.find((entry) => entry.id === subjectId.value))
const chapter = computed(() => chapterId.value
  ? subject.value?.chapters.find((entry) => entry.id === chapterId.value)
  : undefined)
const scopeIsValid = computed(() => Boolean(subject.value && (!chapterId.value || chapter.value)))
const scopeName = computed(() => chapter.value
  ? `${subject.value?.name ?? ''} / ${chapter.value.name}`
  : subject.value?.name ?? '未选择范围')
const groups = computed(() => activeTab.value === 'exact'
  ? result.value?.exactGroups ?? []
  : result.value?.suspectedGroups ?? [])

function isInScope(member: QuestionDuplicateGroup['members'][number]) {
  if (member.subjectId !== subjectId.value) return false
  return !chapterId.value || member.chapterId === chapterId.value
}

function resetKeepers(scanResult: QuestionDuplicateScanResult) {
  const next: Record<string, string> = {}
  for (const group of [...scanResult.exactGroups, ...scanResult.suspectedGroups]) {
    const current = keepers.value[group.id]
    next[group.id] = group.members.some((member) => member.id === current)
      ? current
      : group.members[0]?.id ?? ''
  }
  keepers.value = next
}

async function scan() {
  const requestSequence = ++scanSequence
  result.value = null
  loadError.value = ''
  if (!scopeIsValid.value) {
    loadError.value = '要检查的学科或章节已经不存在，请在左侧重新选择。'
    return
  }
  bankStore.filters.subjectId = subjectId.value
  bankStore.filters.chapterId = chapterId.value || undefined
  bankStore.filters.page = 1
  loading.value = true
  try {
    const scanResult = await backend.scanQuestionDuplicates({
      subjectId: subjectId.value,
      chapterId: chapterId.value || undefined,
    })
    if (requestSequence !== scanSequence) return
    result.value = scanResult
    resetKeepers(scanResult)
    activeTab.value = reconcileDuplicateCheckTab(activeTab.value, scanResult)
  } catch (reason) {
    if (requestSequence !== scanSequence) return
    loadError.value = errorMessage(reason, '题目查重失败')
  } finally {
    if (requestSequence === scanSequence) loading.value = false
  }
}

function cancelScan() {
  scanSequence += 1
  loading.value = false
  void router.push('/questions')
}

async function openPreview(questionId: string) {
  try {
    const question = await backend.getQuestion(questionId)
    if (!question) {
      ElMessage.warning('这道题已经不存在，请重新查重')
      return
    }
    previewQuestion.value = question
    previewOpen.value = true
  } catch (reason) {
    ElMessage.error(errorMessage(reason, '读取题目失败'))
  }
}

async function recycleDuplicates(group: QuestionDuplicateGroup) {
  const keeperId = keepers.value[group.id]
  const recycleIds = group.members.filter((member) => member.id !== keeperId).map((member) => member.id)
  if (!keeperId || !recycleIds.length) return
  const keeper = group.members.find((member) => member.id === keeperId)
  try {
    await ElMessageBox.confirm(
      `将其余 ${recycleIds.length} 道题移入回收站，保留“${keeper?.stemPreview || '所选题目'}”。可随后在回收站恢复。`,
      '确认处理重复题',
      {
        type: 'warning',
        confirmButtonText: '移入回收站',
        cancelButtonText: '取消',
      },
    )
  } catch {
    return
  }
  operationGroupId.value = group.id
  try {
    await backend.moveQuestionsToRecycle(recycleIds)
    await appStore.refreshTaxonomy()
    ElMessage.success(`已保留 1 道题，其余 ${recycleIds.length} 道已移入回收站`)
    await scan()
  } catch (reason) {
    ElMessage.error(errorMessage(reason, '处理重复题失败'))
  } finally {
    operationGroupId.value = null
  }
}

async function ignoreSuspected(group: QuestionDuplicateGroup) {
  const [first, second] = group.members
  if (!first || !second) return
  operationGroupId.value = group.id
  try {
    await backend.ignoreQuestionDuplicate({
      firstQuestionId: first.id,
      firstContentVersion: first.contentVersion,
      secondQuestionId: second.id,
      secondContentVersion: second.contentVersion,
    })
    ElMessage.success('已标记为非重复；其中任一题修改后会自动重新检查')
    await scan()
  } catch (reason) {
    ElMessage.error(errorMessage(reason, '标记非重复失败'))
  } finally {
    operationGroupId.value = null
  }
}

function formatTime(timestamp: number) {
  return new Date(timestamp).toLocaleString('zh-CN', {
    year: 'numeric',
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
  })
}

watch(
  () => [route.query.subjectId, route.query.chapterId, appStore.subjects.length],
  () => void scan(),
  { immediate: true },
)
</script>

<template>
  <main class="page-main duplicate-check-page">
    <PageHeader
      title="题目查重"
      :subtitle="`检查范围：${scopeName}；范围内题目会与整个题库的未删除题目比较。`"
    >
      <el-button @click="router.push('/questions')">返回题库</el-button>
      <el-button type="primary" :loading="loading" :disabled="!scopeIsValid" @click="scan">重新检查</el-button>
    </PageHeader>

    <section v-if="loading" class="surface scan-state" aria-live="polite">
      <el-progress :percentage="100" :indeterminate="true" :duration="1.5" />
      <h2>正在检查重复题目</h2>
      <p>题目较多时可能需要一些时间。检查期间不会修改任何题目。</p>
      <el-button @click="cancelScan">取消检查</el-button>
    </section>

    <el-alert
      v-else-if="loadError"
      :title="loadError"
      type="error"
      show-icon
      :closable="false"
    />

    <template v-else-if="result">
      <section class="scan-summary" aria-label="查重结果摘要">
        <div class="surface summary-item"><strong>{{ result.scannedQuestionCount }}</strong><span>范围内题目</span></div>
        <div class="surface summary-item"><strong>{{ result.comparedQuestionCount }}</strong><span>题库活跃题目</span></div>
        <div class="surface summary-item summary-item--exact"><strong>{{ result.exactGroups.length }}</strong><span>完全重复组</span></div>
        <div class="surface summary-item summary-item--suspected"><strong>{{ result.suspectedGroups.length }}</strong><span>疑似重复组</span></div>
      </section>

      <section class="surface result-panel">
        <el-tabs v-model="activeTab">
          <el-tab-pane :label="`完全重复（${result.exactGroups.length}）`" name="exact" />
          <el-tab-pane :label="`疑似重复（${result.suspectedGroups.length}）`" name="suspected" />
        </el-tabs>

        <div v-if="!groups.length" class="empty-result">
          <el-empty :description="activeTab === 'exact' ? '没有发现完全重复题目' : '没有发现疑似重复题目'" />
        </div>

        <div v-else class="duplicate-groups">
          <article v-for="(group, groupIndex) in groups" :key="group.id" class="duplicate-group">
            <header class="duplicate-group__header">
              <div>
                <strong>{{ activeTab === 'exact' ? `完全重复组 ${groupIndex + 1}` : `疑似重复组 ${groupIndex + 1}` }}</strong>
                <span v-if="activeTab === 'suspected'">相似度 {{ group.similarityPercent }}%</span>
                <span v-else>内容指纹一致</span>
              </div>
              <small>请选择要保留的题目</small>
            </header>

            <el-radio-group v-model="keepers[group.id]" class="duplicate-members">
              <div
                v-for="member in group.members"
                :key="member.id"
                class="duplicate-member"
                :class="{ 'is-keeper': keepers[group.id] === member.id }"
              >
                <el-radio :value="member.id" size="large">保留此题</el-radio>
                <div class="duplicate-member__content">
                  <div class="duplicate-member__meta">
                    <el-tag v-if="isInScope(member)" size="small" type="primary">检查范围内</el-tag>
                    <el-tag size="small" effect="plain">{{ questionTypeLabel(member.type, appStore.questionTypes) }}</el-tag>
                    <span>{{ member.subjectName }} / {{ member.chapterName }}</span>
                  </div>
                  <p>{{ member.stemPreview || '（无题干文字，请打开预览查看图片或其他内容）' }}</p>
                  <small>创建：{{ formatTime(member.createdAt) }} · 最近修改：{{ formatTime(member.updatedAt) }}</small>
                </div>
                <el-button @click="openPreview(member.id)">查看内容</el-button>
              </div>
            </el-radio-group>

            <footer class="duplicate-group__actions">
              <el-button
                v-if="activeTab === 'suspected'"
                :loading="operationGroupId === group.id"
                @click="ignoreSuspected(group)"
              >
                标记为非重复
              </el-button>
              <el-button
                type="danger"
                plain
                :loading="operationGroupId === group.id"
                @click="recycleDuplicates(group)"
              >
                保留所选题目，其余移入回收站
              </el-button>
            </footer>
          </article>
        </div>
      </section>

      <el-alert
        class="result-note"
        title="“疑似重复”只表示文字相似，不会自动删除；每道范围内题目最多比较 256 个最接近的候选。标记为非重复后，只要其中一道题被修改，这一对题目就会重新参与检查。"
        type="info"
        show-icon
        :closable="false"
      />
    </template>

    <QuestionPreviewDrawer
      v-model="previewOpen"
      :question="previewQuestion"
      :show-add-to-paper="false"
    />
  </main>
</template>

<style scoped>
.duplicate-check-page {
  overflow: auto;
}

.scan-state {
  min-height: 260px;
  padding: 44px;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  text-align: center;
}

.scan-state .el-progress {
  width: min(460px, 80%);
}

.scan-state h2 {
  margin: 28px 0 8px;
  font-size: 18px;
}

.scan-state p {
  margin: 0 0 20px;
  color: #64748b;
  font-size: 13px;
}

.scan-summary {
  margin-bottom: 16px;
  display: grid;
  grid-template-columns: repeat(4, minmax(0, 1fr));
  gap: 12px;
}

.summary-item {
  padding: 16px 18px;
  display: flex;
  align-items: baseline;
  gap: 10px;
}

.summary-item strong {
  color: #2563eb;
  font-size: 24px;
}

.summary-item span {
  color: #64748b;
  font-size: 12px;
}

.summary-item--exact strong { color: #dc2626; }
.summary-item--suspected strong { color: #d97706; }

.result-panel {
  padding: 4px 20px 20px;
}

.duplicate-groups {
  display: grid;
  gap: 16px;
}

.duplicate-group {
  overflow: hidden;
  border: 1px solid #dbe3ef;
  border-radius: 10px;
}

.duplicate-group__header {
  padding: 14px 16px;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
  border-bottom: 1px solid #e8edf4;
  background: #f8fafc;
}

.duplicate-group__header div {
  display: flex;
  align-items: baseline;
  gap: 10px;
}

.duplicate-group__header span,
.duplicate-group__header small {
  color: #64748b;
  font-size: 12px;
}

.duplicate-members {
  width: 100%;
  display: block;
}

.duplicate-member {
  min-height: 92px;
  padding: 14px 16px;
  display: grid;
  grid-template-columns: 100px minmax(0, 1fr) auto;
  align-items: center;
  gap: 14px;
  border-bottom: 1px solid #edf1f6;
  background: #fff;
}

.duplicate-member.is-keeper {
  background: #f5f9ff;
  box-shadow: inset 3px 0 #409eff;
}

.duplicate-member__content {
  min-width: 0;
}

.duplicate-member__meta {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 7px;
  color: #64748b;
  font-size: 11px;
}

.duplicate-member__content p {
  margin: 8px 0;
  overflow: hidden;
  color: #1e293b;
  font-size: 13px;
  font-weight: 600;
  line-height: 1.55;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.duplicate-member__content > small {
  color: #94a3b8;
  font-size: 10px;
}

.duplicate-group__actions {
  padding: 12px 16px;
  display: flex;
  justify-content: flex-end;
  gap: 10px;
  background: #fafcff;
}

.duplicate-group__actions :deep(.el-button + .el-button) {
  margin-left: 0;
}

.empty-result {
  min-height: 260px;
  display: grid;
  place-items: center;
}

.result-note {
  margin-top: 16px;
}

@media (max-width: 1000px) {
  .scan-summary { grid-template-columns: repeat(2, minmax(0, 1fr)); }
  .duplicate-member { grid-template-columns: 90px minmax(0, 1fr); }
  .duplicate-member > .el-button { grid-column: 2; justify-self: start; }
}
</style>
