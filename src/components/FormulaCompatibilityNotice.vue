<script setup lang="ts">
import { onBeforeUnmount, ref, watch } from 'vue'
import { backend } from '../services/backend'
import { errorMessage } from '../services/errors'
import { formulaOccurrences, type FormulaField } from '../utils/formulaCompatibility'

const props = defineProps<{ fields: FormulaField[] }>()
const issues = ref<string[]>([])
const checkError = ref('')
const cache = new Map<string, string | null>()
let sequence = 0
let timer: ReturnType<typeof setTimeout> | undefined
watch(() => props.fields.map(field => [field.label, field.content.html]), () => {
  const request = ++sequence
  clearTimeout(timer)
  issues.value = []; checkError.value = ''
  timer = setTimeout(() => { void check(request) }, 300)
}, { immediate: true })
async function check(request: number) {
  try {
    const occurrences = formulaOccurrences(props.fields)
    const results = new Map(cache)
    const missing = [...new Set(occurrences.map(item => item.latex))].filter(latex => !cache.has(latex))
    for (let offset = 0; offset < missing.length; offset += 16) {
      if (request !== sequence) return
      const batch = missing.slice(offset, offset + 16)
      const result = await backend.checkFormulaCompatibility(batch)
      if (request !== sequence || result === null) return
      if (result.length !== batch.length) throw new Error('公式检查结果不完整，导出时将再次检查。')
      batch.forEach((latex, index) => {
        cache.set(latex, result[index] ?? null)
        results.set(latex, result[index] ?? null)
        if (cache.size > 128) cache.delete(cache.keys().next().value!)
      })
    }
    if (request !== sequence) return
    issues.value = [...new Set(occurrences.flatMap(({ label, latex }) => {
      const message = results.get(latex)
      return message ? [`${label}：${message}（${latex.slice(0, 80)}${latex.length > 80 ? '…' : ''}）`] : []
    }))]
  } catch (reason) {
    if (request === sequence) checkError.value = errorMessage(reason, '公式兼容性检查暂不可用，导出时将再次检查。')
  }
}
onBeforeUnmount(() => { sequence++; clearTimeout(timer) })
</script>

<template>
  <aside v-if="issues.length || checkError" class="formula-compatibility-notice" role="status">
    <strong>Word 公式兼容性提示</strong>
    <p v-if="checkError">{{ checkError }}</p>
    <template v-else>
      <p>以下公式暂不能导出为 Word 可编辑公式，请在导出前调整。原内容仍可保存。</p>
      <ul><li v-for="issue in issues.slice(0, 10)" :key="issue">{{ issue }}</li></ul>
      <p v-if="issues.length > 10">另有 {{ issues.length - 10 }} 处，调整后会继续显示。</p>
    </template>
  </aside>
</template>

<style scoped>
.formula-compatibility-notice { margin: 10px 0; padding: 10px 14px; color: #92400e; background: #fffbeb; border: 1px solid #fcd34d; border-radius: 6px; overflow-wrap: anywhere; }
p { margin: 4px 0; }
ul { margin: 4px 0; padding-left: 20px; }
</style>
