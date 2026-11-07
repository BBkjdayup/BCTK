<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { ElMessage, ElMessageBox } from 'element-plus'
import { Plus } from '@element-plus/icons-vue'
import Sortable from 'sortablejs'
import type { Chapter, Unit } from '../types/domain'
import { useAppStore } from '../stores/app'
import { askForName, normalizeName } from '../utils/taxonomyNamePrompt'
import { buildTaxonomyOrderItems } from '../utils/taxonomyOrder'
import { errorMessage } from '../services/errors'

const props = defineProps<{ chapter?: Chapter }>()
const appStore = useAppStore()
const list = ref<HTMLElement>()
const busy = ref(false)
const units = computed(() => [...(props.chapter?.units ?? [])].sort((a, b) => a.sortOrder - b.sortOrder))
let sortable: Sortable | null = null

async function rename(unit?: Unit) {
  const chapter = props.chapter
  if (!chapter || busy.value) return
  const name = await askForName(unit ? '编辑单元' : `在“${chapter.name}”中新增单元`, unit?.name ?? '', 100)
  if (!name || (unit && normalizeName(name) === normalizeName(unit.name))) return
  if (chapter.units?.some((item) => item.id !== unit?.id && normalizeName(item.name) === normalizeName(name))) {
    ElMessage.warning('该章节已经存在同名单元')
    return
  }
  busy.value = true
  try {
    if (unit) await appStore.updateUnit(unit.id, name)
    else await appStore.createUnit(chapter.id, name)
    ElMessage.success(unit ? '单元已更新' : '单元已新增')
  } catch (reason) { ElMessage.error(errorMessage(reason, '保存单元失败')) }
  finally { busy.value = false }
}

async function remove(unit: Unit) {
  if (busy.value) return
  if (unit.questionCount) { ElMessage.warning('请先移走单元中的题目'); return }
  try { await ElMessageBox.confirm(`确定删除单元“${unit.name}”吗？`, '删除单元', { type: 'warning' }) }
  catch { return }
  busy.value = true
  try { await appStore.deleteUnit(unit.id); ElMessage.success('单元已删除') }
  catch (reason) { ElMessage.error(errorMessage(reason, '删除单元失败')) }
  finally { busy.value = false }
}

async function saveOrder() {
  const chapter = props.chapter
  const control = sortable
  if (!chapter || !control) return
  const original = units.value.map((unit) => unit.id)
  const items = buildTaxonomyOrderItems(control.toArray(), original)
  if (!items) { control.sort(original); ElMessage.warning('单元列表已变化，请重新排序'); return }
  busy.value = true
  try { await appStore.saveUnitOrder(chapter.id, items); ElMessage.success('单元顺序已保存') }
  catch (reason) {
    if (props.chapter?.id === chapter.id) control.sort(original)
    await appStore.refreshTaxonomy()
    ElMessage.error(errorMessage(reason, '保存排序失败'))
  } finally { busy.value = false }
}

async function initializeSort() {
  await nextTick()
  sortable?.destroy()
  sortable = list.value ? new Sortable(list.value, {
    dataIdAttr: 'data-unit-id', animation: 180, draggable: '.unit-row', handle: '.unit-drag',
    forceFallback: true, fallbackOnBody: true, fallbackTolerance: 4,
    disabled: busy.value,
    onEnd: (event) => { if (event.oldIndex !== event.newIndex) void saveOrder() },
  }) : null
}
watch(() => `${props.chapter?.id}:${units.value.map((unit) => unit.id).join(',')}`, () => void initializeSort(), { flush: 'post' })
watch(busy, (value) => sortable?.option('disabled', value))
onMounted(() => void initializeSort())
onBeforeUnmount(() => { sortable?.destroy(); sortable = null })
</script>

<template>
  <section class="surface unit-panel">
    <header class="unit-heading">
      <div><h2>单元</h2><span :title="chapter?.name">{{ chapter ? `${chapter.name} · ${units.length} 个单元` : '请先选择章节' }}</span></div>
      <el-button :disabled="!chapter || busy" type="primary" :icon="Plus" @click="rename()">新增单元</el-button>
    </header>
    <div v-if="units.length" ref="list" class="unit-list">
      <div v-for="unit in units" :key="unit.id" class="unit-row" :data-unit-id="unit.id">
        <span class="unit-drag" title="拖动排序" aria-label="拖动调整单元顺序">⠿</span>
        <strong :title="unit.name">{{ unit.name }}</strong>
        <span class="unit-count">{{ unit.questionCount }} 题</span>
        <span class="unit-actions"><el-button link type="primary" :disabled="busy" @click="rename(unit)">编辑</el-button><el-button link type="danger" :disabled="busy" @click="remove(unit)">删除</el-button></span>
      </div>
    </div>
    <div v-else class="unit-empty">{{ chapter ? '还没有单元。可按需要新增，题目也可以直接保留在章节下。' : '选择左侧章节后，在这里管理它的单元。' }}</div>
    <p class="unit-note">单元可选；未归入单元的题目可在分类栏“未分单元”中查看。删除前须移走关联题目（含回收站）。</p>
  </section>
</template>

<style scoped>
.unit-panel { min-width: 0; min-height: 0; display: flex; flex-direction: column; overflow: hidden; }
.unit-heading { min-height: 75px; padding: 12px; display: flex; align-items: center; gap: 8px; justify-content: space-between; border-bottom: 1px solid var(--border); }
.unit-heading > div { min-width: 0; }
.unit-heading h2 { margin: 0 0 5px; font-size: 15px; }
.unit-heading span { display: block; color: #64748b; font-size: 11px; overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
.unit-list { overflow: auto; }
.unit-row { display: grid; grid-template-columns: 18px minmax(0, 1fr) 42px 68px; align-items: center; gap: 5px; padding: 13px 10px; border-bottom: 1px solid #edf1f5; font-size: 12px; }
.unit-row strong { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.unit-drag { cursor: grab; color: #94a3b8; }
.unit-count { font-size: 11px; color: #64748b; }
.unit-actions { display: flex; }
.unit-actions .el-button + .el-button { margin-left: 6px; }
.unit-empty { padding: 60px 20px; text-align: center; font-size: 12px; color: #64748b; line-height: 1.8; }
.unit-note { margin: auto 12px 12px; padding-top: 16px; font-size: 11px; line-height: 1.8; color: #64748b; }
</style>
