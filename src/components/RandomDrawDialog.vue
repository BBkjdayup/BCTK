<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import { MagicStick } from '@element-plus/icons-vue'
import { backend } from '../services/backend'
import { errorMessage } from '../services/errors'
import type {
  Question,
  QuestionTagMatchMode,
  QuestionType,
  QuestionTypeDefinition,
  RandomDrawAnalysis,
  RandomDrawRequest,
  RandomDrawScope,
  RandomDrawUsageFilter,
  Subject,
  Tag,
} from '../types/domain'
import { enabledQuestionTypes } from '../utils/questionTypes'
import { questionUsageFilterOptions } from '../utils/questionUsage'

type CountMode = 'total' | 'by_type'

const props = defineProps<{
  modelValue: boolean
  subjects: Subject[]
  tags: Tag[]
  questionTypes: QuestionTypeDefinition[]
  excludedQuestionIds: string[]
  remainingCapacity: number | null
}>()

const emit = defineEmits<{
  'update:modelValue': [value: boolean]
  add: [questions: Question[]]
}>()

const MAX_RANDOM_DRAW_QUESTIONS = 1_000

const subjectIds = ref<string[]>([])
const chapterIds = ref<string[]>([])
const tagIds = ref<string[]>([])
const tagMatchMode = ref<QuestionTagMatchMode>('any')
const usage = ref<RandomDrawUsageFilter>('all')
const countMode = ref<CountMode>('by_type')
const totalCount = ref(10)
const counts = ref<Record<QuestionType, number>>({})
const loading = ref(false)
const loaded = ref(false)
const failure = ref('')
const analysis = ref<RandomDrawAnalysis>({ availableTotal: 0, availableByType: {} })
let requestSequence = 0
let refreshTimer: ReturnType<typeof setTimeout> | null = null

const enabledTypes = computed(() => enabledQuestionTypes(props.questionTypes))
const chapterOptions = computed(() => {
  const selected = new Set(subjectIds.value)
  return props.subjects
    .filter((subject) => !selected.size || selected.has(subject.id))
    .flatMap((subject) => subject.chapters.map((chapter) => ({
      ...chapter,
      optionLabel: `${subject.name} / ${chapter.name}`,
    })))
})
const requestedTotal = computed(() => countMode.value === 'total'
  ? Math.max(0, Math.trunc(Number(totalCount.value) || 0))
  : Object.values(counts.value).reduce(
      (sum, count) => sum + Math.max(0, Math.trunc(Number(count) || 0)),
      0,
    ))

function scope(): RandomDrawScope {
  return {
    subjectIds: [...subjectIds.value],
    chapterIds: [...chapterIds.value],
    tagIds: [...tagIds.value],
    tagMatchMode: tagMatchMode.value,
    usage: usage.value,
    excludedQuestionIds: [...props.excludedQuestionIds],
  }
}

async function refreshAnalysis(showFailureMessage = false) {
  const sequence = ++requestSequence
  loading.value = true
  failure.value = ''
  try {
    const result = await backend.analyzeRandomDraw(scope())
    if (sequence !== requestSequence) return
    analysis.value = result
    loaded.value = true
  } catch (reason) {
    if (sequence !== requestSequence) return
    loaded.value = false
    failure.value = errorMessage(reason, '读取随机抽题候选数量失败')
    if (showFailureMessage) ElMessage.error(failure.value)
  } finally {
    if (sequence === requestSequence) loading.value = false
  }
}

function scheduleAnalysis(showFailureMessage = false) {
  if (refreshTimer) clearTimeout(refreshTimer)
  refreshTimer = setTimeout(() => {
    refreshTimer = null
    void refreshAnalysis(showFailureMessage)
  }, 180)
}

function changeSubjects() {
  const availableChapterIds = new Set(chapterOptions.value.map((chapter) => chapter.id))
  chapterIds.value = chapterIds.value.filter((chapterId) => availableChapterIds.has(chapterId))
  scheduleAnalysis(true)
}

function resetConditions() {
  subjectIds.value = []
  chapterIds.value = []
  tagIds.value = []
  tagMatchMode.value = 'any'
  usage.value = 'all'
  countMode.value = 'by_type'
  totalCount.value = 10
  counts.value = Object.fromEntries(
    enabledTypes.value.map((definition) => [definition.code, 0]),
  )
  loaded.value = false
  failure.value = ''
  analysis.value = { availableTotal: 0, availableByType: {} }
  scheduleAnalysis(true)
}

function request(): RandomDrawRequest {
  return {
    scope: scope(),
    countMode: countMode.value,
    totalCount: countMode.value === 'total' ? requestedTotal.value : 0,
    questionTypeCounts: countMode.value === 'by_type'
      ? Object.fromEntries(enabledTypes.value.map((type) => [type.code, counts.value[type.code] ?? 0]))
      : {},
  }
}

async function drawAndAdd() {
  const requested = requestedTotal.value
  if (requested < 1) {
    ElMessage.warning(countMode.value === 'total'
      ? '请输入需要随机抽取的题量。'
      : '请先为至少一种题型填写需要抽取的题量。')
    return
  }
  if (props.remainingCapacity !== null && requested > props.remainingCapacity) {
    ElMessage.warning(`当前试卷还能加入 ${props.remainingCapacity} 道题，请减少本次抽取题量。`)
    return
  }
  if (requested > MAX_RANDOM_DRAW_QUESTIONS) {
    ElMessage.warning(`单次最多随机抽取 ${MAX_RANDOM_DRAW_QUESTIONS} 道题，请减少本次抽取题量。`)
    return
  }

  loading.value = true
  failure.value = ''
  try {
    const selected = await backend.drawRandomQuestions(request())
    if (!selected.length) {
      ElMessage.warning('当前候选题库没有可加入的新题。')
      await refreshAnalysis()
      return
    }
    if (selected.length < requested) {
      try {
        await ElMessageBox.confirm(
          `计划抽取 ${requested} 道，当前实际可抽取 ${selected.length} 道。是否将这些题目加入右侧？`,
          '候选题数量不足',
          { type: 'warning', confirmButtonText: `加入 ${selected.length} 道`, cancelButtonText: '返回修改' },
        )
      } catch {
        await refreshAnalysis()
        return
      }
    }
    emit('add', selected)
    ElMessage.success(`已随机抽取并加入 ${selected.length} 道题`)
    scheduleAnalysis()
  } catch (reason) {
    failure.value = errorMessage(reason, '随机抽题失败')
    ElMessage.error(failure.value)
  } finally {
    loading.value = false
  }
}

watch(enabledTypes, (definitions) => {
  counts.value = Object.fromEntries(
    definitions.map((definition) => [definition.code, counts.value[definition.code] ?? 0]),
  )
}, { immediate: true })

watch(() => props.modelValue, (open) => {
  if (open) scheduleAnalysis()
}, { immediate: true })

watch(
  () => props.excludedQuestionIds.join(','),
  () => {
    if (props.modelValue) scheduleAnalysis()
  },
)

onBeforeUnmount(() => {
  requestSequence += 1
  if (refreshTimer) clearTimeout(refreshTimer)
})
</script>

<template>
  <el-dialog
    :model-value="modelValue"
    title="从标签候选题库随机抽题"
    width="780px"
    destroy-on-close
    @update:model-value="emit('update:modelValue', $event)"
  >
    <div class="batch-draw-dialog">
      <div class="batch-draw-scope-grid">
        <el-form-item label="所属学科">
          <el-select
            v-model="subjectIds"
            multiple
            filterable
            collapse-tags
            collapse-tags-tooltip
            clearable
            placeholder="全部学科"
            @change="changeSubjects"
          >
            <el-option v-for="subject in subjects" :key="subject.id" :label="subject.name" :value="subject.id" />
          </el-select>
        </el-form-item>
        <el-form-item label="章节范围">
          <el-select
            v-model="chapterIds"
            multiple
            collapse-tags
            collapse-tags-tooltip
            clearable
            placeholder="全部章节"
            @change="scheduleAnalysis(true)"
          >
            <el-option v-for="chapter in chapterOptions" :key="chapter.id" :label="chapter.optionLabel" :value="chapter.id" />
          </el-select>
        </el-form-item>
      </div>
      <div class="batch-draw-filter-grid">
        <el-form-item label="标签范围（可选）">
          <el-select
            v-model="tagIds"
            multiple
            filterable
            collapse-tags
            collapse-tags-tooltip
            clearable
            placeholder="不选则统计当前学科和章节内全部题目"
            @change="scheduleAnalysis(true)"
          >
            <el-option v-for="tag in tags" :key="tag.id" :label="tag.name" :value="tag.id" />
          </el-select>
        </el-form-item>
        <el-form-item label="使用状态">
          <el-select v-model="usage" @change="scheduleAnalysis(true)">
            <el-option
              v-for="option in questionUsageFilterOptions"
              :key="option.value"
              :label="option.label"
              :value="option.value"
            />
          </el-select>
        </el-form-item>
      </div>
      <div class="batch-draw-options">
        <el-form-item label="多标签匹配">
          <el-radio-group
            v-model="tagMatchMode"
            :disabled="tagIds.length < 2"
            @change="scheduleAnalysis(true)"
          >
            <el-radio-button value="any">任一标签</el-radio-button>
            <el-radio-button value="all">全部标签</el-radio-button>
          </el-radio-group>
        </el-form-item>
        <el-form-item label="抽取方式">
          <el-radio-group v-model="countMode">
            <el-radio-button value="by_type">按题型</el-radio-button>
            <el-radio-button value="total">按总题量</el-radio-button>
          </el-radio-group>
        </el-form-item>
      </div>

      <div v-if="countMode === 'total'" class="batch-draw-total-row">
        <span>本次随机抽取</span>
        <el-input-number v-model="totalCount" :min="1" :max="MAX_RANDOM_DRAW_QUESTIONS" />
        <span>道</span>
        <span class="available-total">当前剩余可用 {{ loaded ? analysis.availableTotal : loading ? '读取中…' : '—' }} 道</span>
      </div>
      <div v-else class="type-count-table batch-type-table">
        <div class="type-count-table__head"><span>题型</span><span>本次抽取</span><span>剩余可用</span><span>状态</span></div>
        <div v-for="definition in enabledTypes" :key="definition.code" class="type-count-row">
          <strong>{{ definition.name }}</strong>
          <el-input-number
            v-model="counts[definition.code]"
            :min="0"
            :max="MAX_RANDOM_DRAW_QUESTIONS"
            size="small"
          />
          <span>{{ loaded ? `${analysis.availableByType[definition.code] ?? 0} 道` : loading ? '读取中…' : '—' }}</span>
          <el-tag
            size="small"
            :type="loaded && (analysis.availableByType[definition.code] ?? 0) >= (counts[definition.code] ?? 0) ? 'success' : 'danger'"
            effect="plain"
          >
            {{ loading ? '正在统计' : loaded && (analysis.availableByType[definition.code] ?? 0) >= (counts[definition.code] ?? 0) ? '数量充足' : loaded ? '数量不足' : '尚未统计' }}
          </el-tag>
        </div>
      </div>
      <el-alert v-if="failure" :title="failure" type="error" :closable="false" show-icon />
    </div>
    <template #footer>
      <div class="batch-draw-footer">
        <el-button @click="resetConditions">重置条件</el-button>
        <div class="batch-draw-footer__actions">
          <el-button @click="emit('update:modelValue', false)">取消</el-button>
          <el-button
            type="primary"
            :icon="MagicStick"
            :loading="loading"
            :disabled="requestedTotal < 1"
            @click="drawAndAdd"
          >随机抽取并加入 {{ requestedTotal }} 道</el-button>
        </div>
      </div>
    </template>
  </el-dialog>
</template>

<style scoped>
.batch-draw-dialog {
  display: grid;
  gap: 14px;
}

.batch-draw-scope-grid,
.batch-draw-filter-grid,
.batch-draw-options {
  display: grid;
  grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
  gap: 16px;
}

.batch-draw-filter-grid {
  grid-template-columns: minmax(0, 1.45fr) minmax(240px, 0.75fr);
}

.batch-draw-dialog :deep(.el-form-item) {
  margin-bottom: 0;
}

.batch-draw-dialog :deep(.el-form-item__content),
.batch-draw-dialog :deep(.el-select) {
  min-width: 0;
  width: 100%;
}

.batch-draw-footer {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}

.batch-draw-footer__actions {
  display: flex;
  align-items: center;
  gap: 10px;
}

.batch-draw-total-row {
  display: flex;
  align-items: center;
  gap: 12px;
  min-height: 64px;
  padding: 0 16px;
  border: 1px solid var(--app-border-color, #dcdfe6);
  border-radius: 8px;
}

.available-total {
  margin-left: auto;
  color: var(--el-text-color-secondary);
}

.type-count-table {
  overflow: hidden;
  border: 1px solid var(--app-border-color, #dcdfe6);
  border-radius: 8px;
}

.batch-type-table {
  max-height: 360px;
  overflow-y: auto;
}

.type-count-table__head,
.type-count-row {
  display: grid;
  grid-template-columns: minmax(120px, 1fr) 170px 150px 140px;
  align-items: center;
  gap: 16px;
  min-height: 58px;
  padding: 0 16px;
}

.type-count-table__head {
  position: sticky;
  top: 0;
  z-index: 1;
  color: var(--el-text-color-secondary);
  background: var(--el-fill-color-light);
}

.type-count-row + .type-count-row {
  border-top: 1px solid var(--app-border-color, #dcdfe6);
}

@media (max-width: 760px) {
  .batch-draw-scope-grid,
  .batch-draw-filter-grid,
  .batch-draw-options {
    grid-template-columns: 1fr;
  }

  .type-count-table__head,
  .type-count-row {
    grid-template-columns: minmax(90px, 1fr) 140px 110px 110px;
    gap: 10px;
    padding: 0 10px;
  }
}
</style>
