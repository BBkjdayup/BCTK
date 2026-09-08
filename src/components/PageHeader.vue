<script setup lang="ts">
defineProps<{
  title: string
  subtitle?: string
}>()
</script>

<template>
  <div class="page-header">
    <div>
      <h1>{{ title }}</h1>
      <p v-if="subtitle">{{ subtitle }}</p>
    </div>
    <div class="page-header__actions">
      <slot />
    </div>
  </div>
</template>

<style scoped>
.page-header {
  min-height: 55px;
  margin-bottom: 18px;
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 18px;
}

h1 {
  margin: 0;
  color: #111827;
  font-size: 22px;
  font-weight: 750;
  letter-spacing: -0.02em;
}

p {
  margin: 7px 0 0;
  color: #64748b;
  font-size: 12px;
}

.page-header__actions {
  display: flex;
  align-items: center;
  gap: 10px;
}
</style>
