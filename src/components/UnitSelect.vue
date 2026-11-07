<script setup lang="ts">
import { computed, watch } from 'vue'
import type { Chapter } from '../types/domain'

const props = defineProps<{ chapter?: Chapter; modelValue?: string | null; disabled?: boolean }>()
const emit = defineEmits<{ 'update:modelValue': [value: string | undefined]; change: [] }>()
const units = computed(() => [...(props.chapter?.units ?? [])].sort((a, b) => a.sortOrder - b.sortOrder))
function update(value?: string) {
  emit('update:modelValue', value || undefined)
  emit('change')
}
watch(() => [props.chapter?.id, ...units.value.map((unit) => unit.id)], () => {
  if (props.modelValue && !units.value.some((unit) => unit.id === props.modelValue)) update()
})
</script>

<template>
  <el-select
    :model-value="modelValue || ''" :disabled="disabled || !chapter" filterable clearable
    aria-label="单元（可选）" placeholder="未分单元（可选）" @update:model-value="update"
  >
    <el-option label="未分单元" value="" />
    <el-option v-for="unit in units" :key="unit.id" :label="unit.name" :value="unit.id" />
  </el-select>
</template>
