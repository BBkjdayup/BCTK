<script setup lang="ts">
import { ref } from 'vue'
import { onBeforeRouteLeave } from 'vue-router'
import { ElMessage, ElMessageBox } from 'element-plus'
import { backend, isDesktopRuntime } from '../services/backend'
import { errorMessage } from '../services/errors'
import { useAppStore } from '../stores/app'
import type { QuestionBankExportFormat } from '../types/domain'

const appStore = useAppStore()
const desktopAvailable = isDesktopRuntime()
const wordExporting = ref(false)
const excelExporting = ref(false)
onBeforeRouteLeave(() => {
  if (!wordExporting.value && !excelExporting.value) return true
  ElMessage.warning('正在导出题库，请等待导出完成。')
  return false
})

function formatBackupSize(bytes: number | null) {
  if (bytes === null || !Number.isFinite(bytes)) return '未知大小'
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / 1024 / 1024).toFixed(1)} MB`
  return `${(bytes / 1024 / 1024 / 1024).toFixed(2)} GB`
}

function suggestedQuestionBankExportName(format: QuestionBankExportFormat) {
  const now = new Date()
  const date = [now.getFullYear(), now.getMonth() + 1, now.getDate()]
    .map((value, index) => index === 0 ? String(value) : String(value).padStart(2, '0'))
    .join('-')
  return `TK试题题库_${date}.${format}`
}

async function openQuestionBankExport(path: string, format: QuestionBankExportFormat) {
  let action: 'open' | 'reveal'
  try {
    await ElMessageBox.confirm(
      `${format === 'docx' ? 'Word 题库' : 'Excel 数据'}已完成导出。`,
      '导出成功',
      {
        type: 'success',
        confirmButtonText: '打开文件',
        cancelButtonText: '打开所在位置',
        distinguishCancelAndClose: true,
      },
    )
    action = 'open'
  } catch (reason) {
    if (reason !== 'cancel') return
    action = 'reveal'
  }

  try {
    if (action === 'open') {
      await backend.openExportedFile(path)
    } else {
      await backend.revealExportedFile(path)
    }
  } catch (reason) {
    ElMessage.error(errorMessage(
      reason,
      action === 'open' ? '无法打开导出文件' : '无法在文件资源管理器中定位导出文件',
    ))
  }
}

async function exportQuestionBank(format: QuestionBankExportFormat) {
  if (!desktopAvailable) return
  if (!appStore.databaseHealthy || wordExporting.value || excelExporting.value) return

  const loading = format === 'docx' ? wordExporting : excelExporting
  loading.value = true
  try {
    const suggestedName = suggestedQuestionBankExportName(format)
    const outputPath = format === 'docx'
      ? await backend.pickDocxSavePath(suggestedName)
      : await backend.pickXlsxSavePath(suggestedName)
    if (!outputPath) return

    const result = await backend.exportQuestionBank(format, outputPath)
    ElMessage.success(`已导出 ${result.questionCount} 道题，文件大小 ${formatBackupSize(result.outputBytes)}`)
    await openQuestionBankExport(result.outputPath, format)
  } catch (reason) {
    ElMessage.error(errorMessage(reason, format === 'docx' ? 'Word 题库导出失败' : 'Excel 数据导出失败'))
  } finally {
    loading.value = false
  }
}

</script>

<template>
<section class="surface export-panel">
          <article class="export-row">
            <span class="file-mark file-mark--word" aria-hidden="true">W</span>
            <div class="export-copy"><strong>Word 题库</strong><p>按学科和章节排版，包含题目、答案与解析</p></div>
            <span class="file-format">.docx</span>
            <el-button :loading="wordExporting" :disabled="!desktopAvailable || !appStore.databaseHealthy || excelExporting" @click="exportQuestionBank('docx')">导出 Word</el-button>
          </article>
          <article class="export-row">
            <span class="file-mark file-mark--excel" aria-hidden="true">X</span>
            <div class="export-copy"><strong>Excel 数据</strong><p>每题一行，便于筛选、统计和整理</p></div>
            <span class="file-format">.xlsx</span>
            <el-button :loading="excelExporting" :disabled="!desktopAvailable || !appStore.databaseHealthy || wordExporting" @click="exportQuestionBank('xlsx')">导出 Excel</el-button>
          </article>

        </section>
</template>

<style scoped>
.export-panel { overflow: hidden; }
.export-row { display: flex; align-items: center; gap: 20px; min-height: 132px; padding: 26px; }
.export-row + .export-row { border-top: 1px solid #edf1f5; }
.file-mark { width: 48px; height: 54px; flex-shrink: 0; display: grid; place-items: center; border: 1px solid #e2e8f0; border-radius: 8px; background: #f8fafc; color: #64748b; font-size: 20px; font-weight: 600; }
.file-mark--word { color: #2563eb; background: #f3f7fe; border-color: #dbeafe; }
.file-mark--excel { color: #15803d; background: #f2faf5; border-color: #dcfce7; }
.export-copy { flex: 1; min-width: 0; }
.export-copy strong { font-size: 14px; color: #334155; font-weight: 600; }
.export-copy p { margin: 8px 0 0; font-size: 12px; color: #94a3b8; line-height: 1.7; }
.file-format { color: #94a3b8; font-size: 12px; margin-right: 12px; }
.export-row > .el-button { width: 110px; }
@media (max-width: 1000px) {
  .export-row { gap: 14px; padding: 22px 18px; }
  .file-format { display: none; }
}
</style>
