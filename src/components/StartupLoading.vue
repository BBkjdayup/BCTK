<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import { backend } from '../services/backend'
import type { StartupProgress } from '../types/domain'

const progress = ref<StartupProgress | null>(null)
const elapsedSeconds = ref(0)
const mountedAt = Date.now()
let active = true
let pollTimer: ReturnType<typeof setTimeout> | undefined
let clockTimer: ReturnType<typeof setInterval> | undefined
let lastProgressAt = mountedAt

const message = computed(() => {
  if (progress.value?.finished && !progress.value.error) return '正在加载题库列表'
  return progress.value?.message || '正在准备题库'
})
const percentage = computed(() => {
  const current = progress.value
  if (!current || current.finished || current.completed === null || !current.total) return null
  return Math.min(100, Math.round(current.completed / current.total * 100))
})

async function poll() {
  try {
    const value = await backend.startupProgress()
    if (!active) return
    progress.value = value
    lastProgressAt = Date.now()
  } catch {
    // The bootstrap request owns error handling. A missed progress sample
    // must not hide an otherwise working upgrade or stop later samples.
  }
  if (active) pollTimer = setTimeout(() => { void poll() }, 1000)
}

onMounted(() => {
  void poll()
  clockTimer = setInterval(() => {
    const base = progress.value?.elapsedMs ?? lastProgressAt - mountedAt
    elapsedSeconds.value = Math.floor((base + Date.now() - lastProgressAt) / 1000)
  }, 1000)
})

onBeforeUnmount(() => {
  active = false
  clearTimeout(pollTimer)
  clearInterval(clockTimer)
})
</script>

<template>
  <section class="startup-loading" aria-busy="true">
    <span class="startup-loading__spinner" aria-hidden="true" />
    <h1>正在打开 TK试题题库</h1>
    <p role="status" aria-live="polite">{{ message }}</p>
    <template v-if="percentage !== null">
      <progress :value="progress?.completed ?? 0" :max="progress?.total ?? 1" aria-label="旧题库处理进度" />
      <p class="startup-loading__detail">已处理 {{ progress?.completed }} / {{ progress?.total }} 道题（{{ percentage }}%）</p>
    </template>
    <p class="startup-loading__detail">已用时 {{ elapsedSeconds }} 秒</p>
    <p class="startup-loading__hint">首次打开旧题库可能需要较长时间。你可以最小化窗口，完成后会自动进入题库。</p>
    <p v-if="elapsedSeconds >= 30" class="startup-loading__hint">正在保留原题库并完成检查，请等待当前步骤结束。</p>
  </section>
</template>

<style scoped>
.startup-loading { max-width: 560px; margin: 0 auto; text-align: center; color: #334155; }
.startup-loading h1 { margin: 18px 0 12px; font-size: 22px; }
.startup-loading__spinner { display: inline-block; width: 30px; height: 30px; border: 3px solid #dbeafe; border-top-color: #2563eb; border-radius: 50%; animation: startup-spin 1s linear infinite; }
.startup-loading progress { display: block; width: 100%; height: 12px; margin: 20px 0 10px; accent-color: #2563eb; }
.startup-loading__detail { color: #64748b; font-size: 13px; }
.startup-loading__hint { color: #64748b; font-size: 14px; line-height: 1.7; }
@keyframes startup-spin { to { transform: rotate(360deg); } }
@media (prefers-reduced-motion: reduce) { .startup-loading__spinner { animation: none; } }
</style>
