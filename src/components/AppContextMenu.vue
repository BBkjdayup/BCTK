<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, watch } from 'vue'
import type { ContextMenuItem } from '../types/contextMenu'

const props = defineProps<{
  modelValue: boolean
  x: number
  y: number
  title?: string
  items: ContextMenuItem[]
}>()

const emit = defineEmits<{
  'update:modelValue': [value: boolean]
  select: [item: ContextMenuItem]
}>()

const menu = ref<HTMLElement>()
const left = ref(0)
const top = ref(0)
const positionStyle = computed(() => ({ left: `${left.value}px`, top: `${top.value}px` }))

function close() {
  emit('update:modelValue', false)
}

function select(item: ContextMenuItem) {
  if (item.disabled) return
  close()
  emit('select', item)
}

function positionMenu() {
  left.value = Math.max(8, props.x)
  top.value = Math.max(8, props.y)
  void nextTick(() => {
    const rect = menu.value?.getBoundingClientRect()
    if (!rect) return
    left.value = Math.max(8, Math.min(props.x, window.innerWidth - rect.width - 8))
    top.value = Math.max(8, Math.min(props.y, window.innerHeight - rect.height - 8))
  })
}

function onKeydown(event: KeyboardEvent) {
  if (event.key === 'Escape') close()
}

watch(
  () => props.modelValue,
  (open) => {
    if (!open) {
      window.removeEventListener('keydown', onKeydown)
      return
    }
    positionMenu()
    window.addEventListener('keydown', onKeydown)
  },
  { immediate: true },
)

watch(() => [props.x, props.y, props.items.length], () => {
  if (props.modelValue) positionMenu()
})

onBeforeUnmount(() => window.removeEventListener('keydown', onKeydown))
</script>

<template>
  <Teleport to="body">
    <div v-if="modelValue" class="context-menu-layer" @pointerdown.self="close" @contextmenu.prevent.self="close">
      <div
        ref="menu"
        class="app-context-menu"
        :style="positionStyle"
        role="menu"
        @contextmenu.prevent
      >
        <div v-if="title" class="app-context-menu__title">{{ title }}</div>
        <template v-for="item in items" :key="item.id">
          <div v-if="item.dividerBefore" class="app-context-menu__divider" />
          <button
            type="button"
            role="menuitem"
            class="app-context-menu__item"
            :class="{ 'is-danger': item.danger }"
            :disabled="item.disabled"
            :title="item.disabled ? item.hint : undefined"
            @click="select(item)"
          >
            <span>{{ item.label }}</span>
            <small v-if="item.hint">{{ item.hint }}</small>
          </button>
        </template>
      </div>
    </div>
  </Teleport>
</template>

<style scoped>
.context-menu-layer {
  position: fixed;
  inset: 0;
  z-index: 5000;
}

.app-context-menu {
  position: fixed;
  width: 228px;
  padding: 6px;
  border: 1px solid #dbe3ee;
  border-radius: 9px;
  background: #fff;
  box-shadow: 0 14px 38px rgb(15 23 42 / 18%);
}

.app-context-menu__title {
  padding: 7px 9px 8px;
  overflow: hidden;
  color: #64748b;
  font-size: 10px;
  font-weight: 600;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.app-context-menu__divider {
  height: 1px;
  margin: 5px 3px;
  background: #edf1f5;
}

.app-context-menu__item {
  width: 100%;
  min-height: 32px;
  padding: 7px 9px;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  border: 0;
  border-radius: 6px;
  background: transparent;
  color: #334155;
  font-size: 11px;
  text-align: left;
}

.app-context-menu__item:hover:not(:disabled),
.app-context-menu__item:focus-visible:not(:disabled) {
  outline: none;
  background: #eff6ff;
  color: #1d4ed8;
}

.app-context-menu__item:disabled {
  color: #aab4c3;
  cursor: not-allowed;
}

.app-context-menu__item.is-danger {
  color: #dc2626;
}

.app-context-menu__item.is-danger:hover:not(:disabled) {
  background: #fef2f2;
  color: #b91c1c;
}

.app-context-menu__item small {
  overflow: hidden;
  color: #94a3b8;
  font-size: 9px;
  text-overflow: ellipsis;
  white-space: nowrap;
}
</style>
