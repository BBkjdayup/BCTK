<script setup lang="ts">
import { computed, nextTick, onMounted, ref, watch } from 'vue'
import type { RichContent } from '../types/domain'
import { renderMathNodes } from '../utils/mathRendering'
import {
  compactQuestionStemHtml,
  plainTextQuestionSummaryHtml,
} from '../utils/questionStemSummary'

const props = withDefaults(defineProps<{
  content?: RichContent | null
  text?: string
  lines?: number
}>(), {
  content: null,
  text: '',
  lines: 2,
})

const root = ref<HTMLElement | null>(null)
const fallbackText = computed(() => props.text || props.content?.plainText || '')
const sourceHtml = computed(() => (
  props.content?.html.trim()
    ? props.content.html
    : plainTextQuestionSummaryHtml(fallbackText.value)
))
const summaryHtml = computed(() => compactQuestionStemHtml(sourceHtml.value))
const lineStyle = computed<Record<string, string>>(() => ({
  '--question-stem-summary-lines': String(Math.max(1, Math.min(4, Math.trunc(props.lines) || 1))),
}))

async function renderFormulas() {
  await nextTick()
  if (root.value) renderMathNodes(root.value)
}

watch(summaryHtml, () => { void renderFormulas() })
onMounted(() => { void renderFormulas() })
</script>

<template>
  <span
    ref="root"
    class="question-stem-summary"
    :style="lineStyle"
    :aria-label="fallbackText"
    v-html="summaryHtml"
  />
</template>

<style scoped>
.question-stem-summary {
  min-width: 0;
  max-width: 100%;
  display: -webkit-box;
  overflow: hidden;
  line-height: 1.55;
  overflow-wrap: anywhere;
  -webkit-box-orient: vertical;
  -webkit-line-clamp: var(--question-stem-summary-lines);
}

.question-stem-summary :deep(.question-stem-summary__segment) {
  display: inline;
}

.question-stem-summary :deep(.math-node) {
  display: inline-block;
  max-width: 100%;
  line-height: 1;
  vertical-align: -0.12em;
}

.question-stem-summary :deep(.katex) {
  font-size: 1em;
}

.question-stem-summary :deep(.question-stem-summary__asset) {
  margin: 0 3px;
  padding: 1px 5px;
  display: inline-flex;
  align-items: center;
  border: 1px solid #cbd5e1;
  border-radius: 4px;
  background: #f8fafc;
  color: #64748b;
  font-size: 0.82em;
  line-height: 1.35;
  vertical-align: 0.08em;
  white-space: nowrap;
}
</style>
