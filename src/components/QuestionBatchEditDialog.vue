<script setup lang="ts">
import { computed, reactive, watch } from 'vue'
import type {
  Question,
  QuestionBatchEditRequest,
  Subject,
  Tag,
} from '../types/domain'
import {
  buildQuestionBatchEditRequest,
  createQuestionBatchEditForm,
  questionBatchEditSummary,
  questionBatchEditValidation,
} from '../utils/questionBatchEdit'

const props = defineProps<{
  modelValue: boolean
  questions: Question[]
  subjects: Subject[]
  tags: Tag[]
  busy?: boolean
}>()

const emit = defineEmits<{
  'update:modelValue': [value: boolean]
  confirm: [request: QuestionBatchEditRequest]
}>()

const visible = computed({
  get: () => props.modelValue,
  set: (value: boolean) => emit('update:modelValue', value),
})
const form = reactive(createQuestionBatchEditForm())
const selectedSubject = computed(() => props.subjects.find((subject) => subject.id === form.subjectId))
const chapters = computed(() => selectedSubject.value?.chapters ?? [])
const validation = computed(() => questionBatchEditValidation(props.questions, form))
const changeSummary = computed(() => questionBatchEditSummary(form))

watch(
  () => props.modelValue,
  (open) => {
    if (open) Object.assign(form, createQuestionBatchEditForm())
  },
)

watch(
  () => form.subjectId,
  (subjectId, previousSubjectId) => {
    if (subjectId !== previousSubjectId) form.chapterId = ''
  },
)

watch(
  () => form.tagMode,
  (mode) => {
    if (mode === 'keep') form.tagIds = []
  },
)

function submit() {
  if (validation.value || props.busy) return
  emit('confirm', buildQuestionBatchEditRequest(props.questions, form))
}
</script>

<template>
  <el-dialog
    v-model="visible"
    title="批量编辑题目"
    width="680px"
    :close-on-click-modal="!busy"
    :close-on-press-escape="!busy"
    :show-close="!busy"
  >
    <el-alert
      type="warning"
      show-icon
      :closable="false"
      :title="`本次操作将同时修改 ${questions.length} 道题；任意一道题发生冲突时，全部修改都会撤销。`"
    />

    <div class="batch-form">
      <section class="form-section">
        <div class="section-heading">
          <strong>所属分类</strong>
          <span>学科和章节必须成对修改，避免出现章节与学科不匹配。</span>
        </div>
        <el-select v-model="form.classificationMode" class="operation-select">
          <el-option label="保持原学科和章节" value="keep" />
          <el-option label="统一修改学科和章节" value="change" />
        </el-select>
        <div v-if="form.classificationMode === 'change'" class="paired-fields">
          <el-select v-model="form.subjectId" filterable placeholder="选择目标学科">
            <el-option v-for="subject in subjects" :key="subject.id" :label="subject.name" :value="subject.id" />
          </el-select>
          <el-select v-model="form.chapterId" filterable :disabled="!form.subjectId" placeholder="选择目标章节">
            <el-option v-for="chapter in chapters" :key="chapter.id" :label="chapter.name" :value="chapter.id" />
          </el-select>
        </div>
      </section>

      <section class="form-section">
        <div class="section-heading">
          <strong>标签处理方式</strong>
          <span>“追加”保留原标签；“替换”会先清除原标签；“删除”只移除选中的标签。</span>
        </div>
        <el-select v-model="form.tagMode" class="operation-select">
          <el-option label="保持原标签" value="keep" />
          <el-option label="追加标签（保留原标签）" value="append" />
          <el-option label="替换标签（删除全部原标签）" value="replace" />
          <el-option label="删除指定标签" value="remove" />
        </el-select>
        <el-select
          v-if="form.tagMode !== 'keep'"
          v-model="form.tagIds"
          multiple
          filterable
          collapse-tags
          collapse-tags-tooltip
          clearable
          class="tag-select"
          :placeholder="form.tagMode === 'replace' ? '可留空：留空表示清空全部标签' : '请选择标签'"
        >
          <el-option v-for="tag in tags" :key="tag.id" :label="tag.name" :value="tag.id" />
        </el-select>
        <el-alert
          v-if="form.tagMode === 'replace' && !form.tagIds.length"
          type="warning"
          :closable="false"
          show-icon
          title="当前设置会清空这些题目的全部标签。"
        />
      </section>

      <section class="form-section">
        <div class="section-heading">
          <strong>最近使用状态</strong>
          <span>这是系统根据试卷使用记录维护的状态，不是独立的“用途”字段。</span>
        </div>
        <el-select v-model="form.usageOperation" class="operation-select">
          <el-option label="保持最近使用时间" value="keep" />
          <el-option label="重置为“从未使用”" value="reset_never" />
        </el-select>
      </section>

      <div v-if="changeSummary.length" class="change-summary">
        <strong>将执行：</strong>
        <span>{{ changeSummary.join('；') }}</span>
      </div>
    </div>

    <template #footer>
      <span v-if="validation" class="validation-message">{{ validation }}</span>
      <el-button :disabled="busy" @click="visible = false">取消</el-button>
      <el-button type="primary" :loading="busy" :disabled="Boolean(validation)" @click="submit">
        确认修改 {{ questions.length }} 道题
      </el-button>
    </template>
  </el-dialog>
</template>

<style scoped>
.batch-form {
  display: grid;
  gap: 14px;
  margin-top: 16px;
}

.form-section {
  padding: 13px;
  display: grid;
  gap: 10px;
  border: 1px solid #e2e8f0;
  border-radius: 8px;
  background: #f8fafc;
}

.section-heading {
  display: grid;
  gap: 3px;
}

.section-heading strong {
  color: #1e293b;
  font-size: 13px;
}

.section-heading span {
  color: #64748b;
  font-size: 11px;
  line-height: 1.6;
}

.operation-select,
.tag-select {
  width: 100%;
}

.paired-fields {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 10px;
}

.change-summary {
  padding: 10px 12px;
  display: flex;
  gap: 6px;
  border-radius: 6px;
  background: #eff6ff;
  color: #1e40af;
  font-size: 12px;
}

.validation-message {
  margin-right: auto;
  color: #dc2626;
  font-size: 11px;
}

:deep(.el-dialog__footer) {
  display: flex;
  align-items: center;
  gap: 8px;
}
</style>
