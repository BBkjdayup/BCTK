<script setup lang="ts">
import { nextTick, onMounted, ref, watch } from 'vue'
import { backend } from '../services/backend'
import type { RichContent } from '../types/domain'
import { renderMathNodes } from '../utils/mathRendering'

const props = defineProps<{ content: RichContent }>()
const root = ref<HTMLElement | null>(null)
const imageCache = new Map<string, string>()
let generation = 0

async function hydrateContent() {
  await nextTick()
  const container = root.value
  if (!container) return
  renderMathNodes(container)
  const request = ++generation
  const images = [...container.querySelectorAll<HTMLImageElement>('img[data-resource-id]')]
  await Promise.all(images.map(async (image) => {
    const resourceId = image.dataset.resourceId?.trim()
    if (!resourceId) return
    try {
      let dataUrl = imageCache.get(resourceId)
      if (!dataUrl) {
        const payload = await backend.getManagedImage(resourceId)
        dataUrl = `data:${payload.mimeType};base64,${payload.dataBase64}`
        imageCache.set(resourceId, dataUrl)
      }
      if (request !== generation || !root.value?.contains(image)) return
      image.src = dataUrl
    } catch {
      if (request !== generation || !root.value?.contains(image)) return
      image.removeAttribute('src')
      image.dataset.resourceLoadError = 'true'
      if (!image.alt) image.alt = '图片资源暂时无法读取'
    }
  }))
}

watch(() => props.content.html, () => { void hydrateContent() })
onMounted(() => { void hydrateContent() })
</script>

<template>
  <div ref="root" class="rich-content-view" v-html="content.html" />
</template>

<style scoped>
.rich-content-view :deep(p) { margin: 0 0 8px; }
.rich-content-view :deep(img) { max-width: 100%; height: auto; }
.rich-content-view :deep(img[data-resource-load-error='true']) {
  min-width: 140px;
  min-height: 44px;
  border: 1px dashed #f59e0b;
  background: #fffbeb;
}
.rich-content-view :deep(.math-node) {
  display: inline-block;
  line-height: 1;
  vertical-align: -0.12em;
}
.rich-content-view :deep(table) { width: 100%; border-collapse: collapse; }
.rich-content-view :deep(td),
.rich-content-view :deep(th) { padding: 5px 7px; border: 1px solid #cbd5e1; }
</style>
