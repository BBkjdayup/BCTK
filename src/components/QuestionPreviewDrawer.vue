<script setup lang="ts">
import { computed } from 'vue'
import { useRouter } from 'vue-router'
import { type Question } from '../types/domain'
import { useAppStore } from '../stores/app'
import { questionTypeLabel } from '../utils/questionTypes'
import RichContentView from './RichContentView.vue'

const props = withDefaults(defineProps<{
  modelValue: boolean
  question: Question | null
  showAddToPaper?: boolean
}>(), {
  showAddToPaper: true,
})

const emit = defineEmits<{
  'update:modelValue': [value: boolean]
  addToPaper: [question: Question]
}>()

const router = useRouter()
const appStore = useAppStore()
const optionLetters = 'ABCDEFGHIJKLMNOPQRSTUVWXYZ'
const title = computed(() => props.question ? questionTypeLabel(props.question.type, appStore.questionTypes) : '题目预览')
</script>

<template>
  <el-drawer
    :model-value="modelValue"
    :title="title"
    size="420px"
    destroy-on-close
    @update:model-value="emit('update:modelValue', $event)"
  >
    <template v-if="question">
      <div class="preview-meta">
        <el-tag size="small" effect="plain">{{ questionTypeLabel(question.type, appStore.questionTypes) }}</el-tag>
        <el-tag v-for="tag in question.tags" :key="tag.id" size="small" type="info" effect="plain">{{ tag.name }}</el-tag>
      </div>
      <section class="preview-section">
        <h3>题干</h3>
        <RichContentView class="rich-display" :content="question.stem" />
        <div v-if="question.options.length" class="preview-options">
          <div v-for="(option, index) in question.options" :key="option.id" class="preview-option">
            <span>{{ optionLetters[index] }}</span>
            <RichContentView :content="option.content" />
          </div>
        </div>
      </section>
      <section class="preview-section preview-section--answer">
        <h3>答案</h3>
        <RichContentView class="rich-display" :content="question.answer" />
      </section>
      <section class="preview-section preview-section--explanation">
        <h3>解析</h3>
        <RichContentView class="rich-display" :content="question.explanation" />
      </section>
      <div class="preview-path">{{ question.subjectName }} · {{ question.chapterName }}</div>
    </template>
    <template #footer>
      <div class="drawer-footer">
        <el-button @click="emit('update:modelValue', false)">关闭</el-button>
        <el-button v-if="question" @click="router.push(`/questions/${question.id}/edit`)">编辑题目</el-button>
        <el-button v-if="question" @click="router.push({ path: '/questions/new', query: { copy: question.id } })">复制题目</el-button>
        <el-button v-if="question && showAddToPaper" type="primary" @click="emit('addToPaper', question)">加入当前试卷</el-button>
      </div>
    </template>
  </el-drawer>
</template>

<style scoped>
.preview-meta {
  margin-bottom: 16px;
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
}

.preview-section {
  margin-bottom: 14px;
  padding: 15px;
  border: 1px solid #e5eaf2;
  border-radius: 8px;
  background: #fff;
}

.preview-section h3 {
  margin: 0 0 10px;
  font-size: 12px;
}

.preview-section--answer {
  border-left: 3px solid #22c55e;
  background: #f7fff9;
}

.preview-section--explanation {
  border-left: 3px solid #60a5fa;
  background: #f8fbff;
}

.rich-display {
  color: #334155;
  font-size: 13px;
  line-height: 1.75;
}

.rich-display :deep(p) {
  margin: 0;
}

.preview-options {
  margin-top: 12px;
  display: grid;
  gap: 8px;
}

.preview-option {
  padding: 8px 10px;
  display: flex;
  gap: 9px;
  border: 1px solid #edf1f5;
  border-radius: 6px;
}

.preview-option > span {
  width: 22px;
  height: 22px;
  display: grid;
  place-items: center;
  border-radius: 4px;
  background: #f1f5f9;
  color: #475569;
  font-size: 11px;
}

.preview-option :deep(p) {
  margin: 1px 0 0;
}

.preview-path {
  padding: 10px 12px;
  border-radius: 6px;
  background: #f8fafc;
  color: #64748b;
  font-size: 11px;
}

.drawer-footer {
  width: 100%;
  display: flex;
  flex-wrap: wrap;
  justify-content: flex-end;
  gap: 8px;
}

.drawer-footer :deep(.el-button) {
  min-width: calc(50% - 4px);
  margin: 0;
}
</style>
