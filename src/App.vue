<script setup lang="ts">
import { onMounted, onUnmounted } from 'vue'
import { ElMessage } from 'element-plus'
import zhCn from 'element-plus/es/locale/lang/zh-cn'
import AppShell from './layouts/AppShell.vue'
import { backend, isDesktopRuntime } from './services/backend'
import { errorMessage } from './services/errors'
import { useAppStore } from './stores/app'
import { usePaperStore } from './stores/paper'
import { checkAndStageAppUpdate } from './utils/appUpdateFlow'

const appStore = useAppStore()
let appUpdateTimer: ReturnType<typeof setTimeout> | null = null

onMounted(async () => {
  await appStore.initialize()
  if (appStore.databaseHealthy) await usePaperStore().initializeRecovery()
  if (!isDesktopRuntime() || !appStore.databaseHealthy) return

  appUpdateTimer = setTimeout(() => {
    void checkAndStageAppUpdate()
  }, 8_000)

  try {
    const result = await backend.runAutomaticBackup()
    if (result.outcome === 'created') {
      const prunedText = result.prunedCount > 0 ? `，并清理 ${result.prunedCount} 份过期自动备份` : ''
      ElMessage.success(`自动备份已完成${prunedText}`)
    }
    if (result.warnings.length > 0) {
      ElMessage.warning(result.warnings.join('；'))
    }
  } catch (reason) {
    ElMessage.warning(errorMessage(reason, '自动备份未完成；软件可以继续使用，请稍后到“数据备份与恢复”检查'))
  }

})

onUnmounted(() => {
  if (appUpdateTimer) clearTimeout(appUpdateTimer)
})
</script>

<template>
  <el-config-provider :locale="zhCn">
    <AppShell />
  </el-config-provider>
</template>
