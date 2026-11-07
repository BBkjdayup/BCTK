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
  paperId: string
  subjects: Subject[]
  tags: Tag[]
  questionTypes: QuestionTypeDefinition[]
  excludedQuestionIds: string[]
}>()

const emit = defineEmits<{
  'update:modelValue': [value: boolean]
  add: [questions: Question[], paperId: string]
}>()

const MAX_RANDOM_DRAW_QUESTIONS = 1_000

const subjectIds = ref<string[]>([])
const chapterIds = ref<string[]>([])
const unitIds = ref<string[]>([])
const tagIds = ref<string[]>([])
const tagMatchMode = ref<QuestionTagMatchMode>('any')
const usage = ref<RandomDrawUsageFilter>('all')
const countMode = ref<CountMode>('by_type')
const totalCount = ref(10)
const counts = ref<Record<QuestionType, number>>({})
const analyzing = ref(false)
const drawing = ref(false)
const loading = computed(() => analyzing.value || drawing.value)
const loaded = ref(false)
const analysisFailure = ref('')
const drawFailure = ref('')
const failure = computed(() => drawFailure.value || analysisFailure.value)
const analysis = ref<RandomDrawAnalysis>({ availableTotal: 0, availableByType: {} })
let analysisSequence = 0
let drawSequence = 0
let active = false
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
const unitOptions = computed(() => chapterOptions.value
  .filter((chapter) => !chapterIds.value.length || chapterIds.value.includes(chapter.id))
  .flatMap((chapter) => (chapter.units ?? []).map((unit) => ({ ...unit, optionLabel: `${chapter.optionLabel} / ${unit.name}` }))))
function changeChapters() {
  const available = new Set(unitOptions.value.map((unit) => unit.id))
  unitIds.value = unitIds.value.filter((id) => available.has(id))
  scheduleAnalysis(true)
}
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
    unitIds: [...unitIds.value],
    tagIds: [...tagIds.value],
    tagMatchMode: tagMatchMode.value,
    usage: usage.value,
    excludedQuestionIds: [...props.excludedQuestionIds],
  }
}

function invalidateAnalysis() {
  analysisSequence += 1
  if (refreshTimer) clearTimeout(refreshTimer)
  refreshTimer = null
  analyzing.value = false
}

function invalidateRequests() {
  invalidateAnalysis()
  drawSequence += 1
  drawing.value = false
  loaded.value = false
  analysisFailure.value = ''
  drawFailure.value = ''
}

function updateVisibility(open: boolean) {
  // Invalidate synchronously; the parent's updated prop arrives on the next render.
  if (!open) {
    active = false
    invalidateRequests()
  }
  emit('update:modelValue', open)
}

function closeDialog(done: () => void) {
  // Element Plus updates modelValue after the leave animation, so cancel first.
  updateVisibility(false)
  done()
}

async function refreshAnalysis(showFailureMessage = false) {
  if (!active) return
  invalidateAnalysis()
  const sequence = analysisSequence
  analyzing.value = true
  analysisFailure.value = ''
  try {
    const result = await backend.analyzeRandomDraw(scope())
    if (!active || sequence !== analysisSequence) return
    analysis.value = result
    loaded.value = true
  } catch (reason) {
    if (!active || sequence !== analysisSequence) return
    loaded.value = false
    analysisFailure.value = errorMessage(reason, '读取随机抽题候选数量失败')
    if (showFailureMessage) ElMessage.error(analysisFailure.value)
  } finally {
    if (sequence === analysisSequence) analyzing.value = false
  }
}

function scheduleAnalysis(showFailureMessage = false) {
  invalidateAnalysis()
  if (!active) return
  loaded.value = false
  analysisFailure.value = ''
  refreshTimer = setTimeout(() => {
    refreshTimer = null
    void refreshAnalysis(showFailureMessage)
  }, 180)
}

function changeSubjects() {
  const availableChapterIds = new Set(chapterOptions.value.map((chapter) => chapter.id))
  chapterIds.value = chapterIds.value.filter((chapterId) => availableChapterIds.has(chapterId))
  changeChapters()
}

function resetConditions() {
  if (drawing.value) return
  subjectIds.value = []
  chapterIds.value = []
  unitIds.value = []
  tagIds.value = []
  tagMatchMode.value = 'any'
  usage.value = 'all'
  countMode.value = 'by_type'
  totalCount.value = 10
  counts.value = Object.fromEntries(
    enabledTypes.value.map((definition) => [definition.code, 0]),
  )
  loaded.value = false
  drawFailure.value = ''
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
  if (!active || !props.modelValue || drawing.value) return
  const requested = requestedTotal.value
  if (requested < 1) {
    ElMessage.warning(countMode.value === 'total'
      ? '请输入需要随机抽取的题量。'
      : '请先为至少一种题型填写需要抽取的题量。')
    return
  }
  if (requested > MAX_RANDOM_DRAW_QUESTIONS) {
    ElMessage.warning(`单次最多随机抽取 ${MAX_RANDOM_DRAW_QUESTIONS} 道题，请减少本次抽取题量。`)
    return
  }

  const sequence = ++drawSequence
  const paperId = props.paperId
  const isCurrent = () => active && props.modelValue
    && sequence === drawSequence && props.paperId === paperId
  drawing.value = true
  drawFailure.value = ''
  try {
    const selected = await backend.drawRandomQuestions(request())
    if (!isCurrent()) return
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
        if (isCurrent()) await refreshAnalysis()
        return
      }
    }
    if (!isCurrent()) return
    emit('add', selected, paperId)
    if (isCurrent()) scheduleAnalysis()
  } catch (reason) {
    if (!isCurrent()) return
    drawFailure.value = errorMessage(reason, '随机抽题失败')
    ElMessage.error(drawFailure.value)
  } finally {
    if (isCurrent()) drawing.value = false
  }
}

watch(enabledTypes, (definitions) => {
  counts.value = Object.fromEntries(
    definitions.map((definition) => [definition.code, counts.value[definition.code] ?? 0]),
  )
}, { immediate: true })

watch(() => [props.modelValue, props.paperId] as const, ([open]) => {
  active = open
  invalidateRequests()
  if (open) scheduleAnalysis()
}, { immediate: true, flush: 'sync' })

watch(
  () => props.excludedQuestionIds.join(','),
  () => {
    if (props.modelValue) scheduleAnalysis()
  },
)

onBeforeUnmount(() => {
  active = false
  invalidateRequests()
})
</script>

<template>
  <el-dialog
    :model-value="modelValue"
    title="从标签候选题库随机抽题"
    width="780px"
    destroy-on-close
    :before-close="closeDialog"
    @update:model-value="updateVisibility"
  >
    <div class="batch-draw-dialog">
      <div class="batch-draw-scope-grid">
        <el-form-item label="所属学科">
          <el-select
            v-model="subjectIds"
            :disabled="drawing"
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
            :disabled="drawing"
            multiple
            collapse-tags
            collapse-tags-tooltip
            clearable
            placeholder="全部章节"
            @change="changeChapters"
          >
            <el-option v-for="chapter in chapterOptions" :key="chapter.id" :label="chapter.optionLabel" :value="chapter.id" />
          </el-select>
        </el-form-item>
        <el-form-item label="单元范围（可选）">
          <el-select v-model="unitIds" :disabled="drawing" aria-label="抽题单元范围" multiple filterable collapse-tags collapse-tags-tooltip clearable placeholder="全部单元（含未分单元）" @change="scheduleAnalysis(true)">
            <el-option v-for="unit in unitOptions" :key="unit.id" :label="unit.optionLabel" :value="unit.id" />
          </el-select>
        </el-form-item>
      </div>
      <div class="batch-draw-filter-grid">
        <el-form-item label="标签范围（可选）">
          <el-select
            v-model="tagIds"
            :disabled="drawing"
            multiple
            filterable
            collapse-tags
            collapse-tags-tooltip
            clearable
            placeholder="不选则统计当前分类内全部题目"
            @change="scheduleAnalysis(true)"
          >
            <el-option v-for="tag in tags" :key="tag.id" :label="tag.name" :value="tag.id" />
          </el-select>
        </el-form-item>
        <el-form-item label="使用状态">
          <el-select v-model="usage" :disabled="drawing" @change="scheduleAnalysis(true)">
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
            :disabled="drawing || tagIds.length < 2"
            @change="scheduleAnalysis(true)"
          >
            <el-radio-button value="any">任一标签</el-radio-button>
            <el-radio-button value="all">全部标签</el-radio-button>
          </el-radio-group>
        </el-form-item>
        <el-form-item label="抽取方式">
          <el-radio-group v-model="countMode" :disabled="drawing">
            <el-radio-button value="by_type">按题型</el-radio-button>
            <el-radio-button value="total">按总题量</el-radio-button>
          </el-radio-group>
        </el-form-item>
      </div>

      <div v-if="countMode === 'total'" class="batch-draw-total-row">
        <span>本次随机抽取</span>
        <el-input-number v-model="totalCount" :disabled="drawing" :min="1" :max="MAX_RANDOM_DRAW_QUESTIONS" />
        <span>道</span>
        <span class="available-total">当前剩余可用 {{ loaded ? analysis.availableTotal : analyzing ? '读取中…' : '—' }} 道</span>
      </div>
      <div v-else class="type-count-table batch-type-table">
        <div class="type-count-table__head"><span>题型</span><span>本次抽取</span><span>剩余可用</span><span>状态</span></div>
        <div v-for="definition in enabledTypes" :key="definition.code" class="type-count-row">
          <strong>{{ definition.name }}</strong>
          <el-input-number
            v-model="counts[definition.code]"
            :disabled="drawing"
            :min="0"
            :max="MAX_RANDOM_DRAW_QUESTIONS"
            size="small"
          />
          <span>{{ loaded ? `${analysis.availableByType[definition.code] ?? 0} 道` : analyzing ? '读取中…' : '—' }}</span>
          <el-tag
            size="small"
            :type="loaded && (analysis.availableByType[definition.code] ?? 0) >= (counts[definition.code] ?? 0) ? 'success' : 'danger'"
            effect="plain"
          >
            {{ analyzing ? '正在统计' : loaded && (analysis.availableByType[definition.code] ?? 0) >= (counts[definition.code] ?? 0) ? '数量充足' : loaded ? '数量不足' : '尚未统计' }}
          </el-tag>
        </div>
      </div>
      <el-alert v-if="failure" :title="failure" type="error" :closable="false" show-icon />
    </div>
    <template #footer>
      <div class="batch-draw-footer">
        <el-button :disabled="drawing" @click="resetConditions">重置条件</el-button>
        <div class="batch-draw-footer__actions">
          <el-button @click="updateVisibility(false)">取消</el-button>
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
