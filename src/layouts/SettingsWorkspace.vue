<script setup lang="ts">
import { useRoute, useRouter } from 'vue-router'
import { Document, Files, Setting, Delete, InfoFilled } from '@element-plus/icons-vue'

const route = useRoute()
const router = useRouter()
const pages = [
  { path: '/settings/general', label: '常规与导出', icon: Setting },
  { path: '/settings/templates', label: '模板管理', icon: Document },
  { path: '/settings/backup', label: '备份与存储', icon: Files },
  { path: '/settings/maintenance', label: '数据维护', icon: Delete },
  { path: '/settings/about', label: '关于软件', icon: InfoFilled },
]
</script>

<template>
  <div class="app-page">
    <aside class="page-sidebar">
      <div class="sidebar-title">系统设置</div>
      <nav class="sidebar-menu" aria-label="设置分类">
        <button v-for="page in pages" :key="page.path" type="button" class="sidebar-menu__item"
          :class="{ 'is-active': route.path === page.path }" :aria-current="route.path === page.path ? 'page' : undefined"
          @click="router.push(page.path)">
          <el-icon><component :is="page.icon" /></el-icon><span>{{ page.label }}</span>
        </button>
      </nav>
    </aside>
    <div class="settings-workspace-content">
      <RouterView :key="route.path" />
    </div>
  </div>
</template>

<style scoped>
.sidebar-menu__item .el-icon { font-size: 16px; }
.settings-workspace-content { min-width: 0; min-height: 0; flex: 1; display: flex; }
</style>
