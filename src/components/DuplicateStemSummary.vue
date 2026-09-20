<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref, shallowRef } from 'vue'
import type { RichContent } from '../types/domain'
import QuestionStemSummary from './QuestionStemSummary.vue'

const props = defineProps<{ load: () => Promise<RichContent> }>()
const root = ref<HTMLElement | null>(null)
const content = shallowRef<RichContent | null>(null)
const error = ref('')
let observer: IntersectionObserver | undefined
let disposed = false
let loading = false

async function loadContent() {
  if (loading || disposed || content.value) return
  loading = true
  error.value = ''
  try {
    const stem = await props.load()
    if (!disposed) content.value = stem
  } catch (reason) {
    if (!disposed) error.value = reason instanceof Error ? reason.message : '题干读取失败'
  } finally { loading = false }
}

onMounted(() => {
  if (typeof IntersectionObserver === 'undefined') { void loadContent(); return }
  observer = new IntersectionObserver((entries) => {
    if (!entries.some((entry) => entry.isIntersecting)) return
    observer?.disconnect()
    void loadContent()
  }, { rootMargin: '300px' })
  if (root.value) observer.observe(root.value)
})
onBeforeUnmount(() => { disposed = true; observer?.disconnect() })
</script>

<template>
  <div ref="root" class="duplicate-stem-summary">
    <QuestionStemSummary v-if="content" :content="content" :lines="3" />
    <span v-else-if="error" class="duplicate-stem-summary__status">
      {{ error }} <button type="button" @click.stop="loadContent">重试</button>
    </span>
    <span v-else class="duplicate-stem-summary__status">正在载入题干…</span>
  </div>
</template>

<style scoped>
.duplicate-stem-summary { min-height: 2.1em; }
.duplicate-stem-summary__status { color: #64748b; font-weight: 400; font-size: 12px; }
.duplicate-stem-summary__status button { border: 0; background: transparent; color: #2563eb; cursor: pointer; }
</style>
