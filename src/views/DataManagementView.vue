<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { onBeforeRouteLeave, useRoute, useRouter } from 'vue-router'
import { ElMessage, ElMessageBox } from 'element-plus'
import { Upload, WarningFilled } from '@element-plus/icons-vue'
import PageHeader from '../components/PageHeader.vue'
import { backend, isDesktopRuntime } from '../services/backend'
import { errorMessage, isCommandErrorPayload } from '../services/errors'
import { useAppStore } from '../stores/app'
import type {
  BackupRecord,
  DataMovePlan,
  DataMoveResult,
  QuestionBankExportFormat,
  RestorePlan,
  RestoreResult,
} from '../types/domain'

type DataSection = 'export' | 'backup' | 'resources' | 'directory'
type TaskState = 'idle' | 'backing-up' | 'restoring' | 'restored' | 'moving' | 'moved'
type DataDirectoryComponent = 'root' | 'database' | 'resources' | 'templates' | 'backup' | 'export'

const route = useRoute()
const router = useRouter()
const appStore = useAppStore()
const allowedSections: DataSection[] = ['export', 'backup', 'resources', 'directory']
const querySection = typeof route.query.section === 'string' ? route.query.section : ''
const activeSection = ref<DataSection>(allowedSections.includes(querySection as DataSection) ? querySection as DataSection : 'backup')
const taskState = ref<TaskState>('idle')
const restoreDialogOpen = ref(false)
const selectedBackupPath = ref('')
const restorePlan = ref<RestorePlan | null>(null)
const restoreResult = ref<RestoreResult | null>(null)
const restoreConfirmed = ref(false)
const restorePreparing = ref(false)
const restoreSubmitting = ref(false)
const restoreScheduled = ref(false)
const backupLoading = ref(false)
const wordExporting = ref(false)
const excelExporting = ref(false)
const locationDialogOpen = ref(false)
const desktopAvailable = isDesktopRuntime()
const dataRoot = computed(() => (
  desktopAvailable ? (appStore.dataRoot || '正在读取…') : '浏览器演示无法读取电脑中的真实数据目录'
))
const dataMovePlan = ref<DataMovePlan | null>(null)
const dataMoveResult = ref<DataMoveResult | null>(null)
const dataMoveConfirmed = ref(false)
const dataMovePreparing = ref(false)
const dataMoveSubmitting = ref(false)
const dataMoveScheduled = ref(false)

const backups = ref<BackupRecord[]>([])

function requireDesktopFileAccess(action: string) {
  if (desktopAvailable) return true
  ElMessage.info(`${action}只能在 Windows 桌面版执行；浏览器演示不会读写电脑中的文件。`)
  return false
}

const storageFolders: Array<{ name: Exclude<DataDirectoryComponent, 'root'>, size: string, description: string }> = [
  { name: 'database', size: '受管', description: 'SQLite 数据库' },
  { name: 'resources', size: '受管', description: '题目资源' },
  { name: 'templates', size: '受管', description: 'Word 模板' },
  { name: 'backup', size: '受管', description: '备份文件' },
  { name: 'export', size: '受管', description: '默认导出' },
]

const taskTitle = computed(() => ({
  'backing-up': '正在创建备份',
  restoring: '正在恢复数据',
  restored: '数据恢复完成',
  moving: '正在迁移数据目录',
  moved: '数据目录迁移完成',
  idle: '数据管理',
})[taskState.value])

watch(
  () => route.query.section,
  (value) => {
    if (typeof value === 'string' && allowedSections.includes(value as DataSection)) {
      activeSection.value = value as DataSection
      taskState.value = 'idle'
    }
  },
)

function selectSection(section: DataSection) {
  activeSection.value = section
  taskState.value = 'idle'
}

function formatBackupDate(timestamp: number) {
  return new Intl.DateTimeFormat('zh-CN', {
    year: 'numeric', month: '2-digit', day: '2-digit', hour: '2-digit', minute: '2-digit',
    hour12: false,
  }).format(timestamp)
}

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
  if (!requireDesktopFileAccess(format === 'docx' ? '导出 Word 题库' : '导出 Excel 数据')) return
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

function backupKindLabel(kind: BackupRecord['backupKind']) {
  return ({
    manual: '手工备份',
    automatic: '自动备份',
    pre_restore: '恢复前备份',
    pre_migration: '升级前备份',
    data_move: '迁移备份',
  })[kind]
}

async function loadBackups(showError = true) {
  if (!desktopAvailable) {
    backups.value = []
    backupLoading.value = false
    return
  }
  backupLoading.value = true
  try {
    backups.value = await backend.listBackups()
  } catch (reason) {
    if (showError) ElMessage.error(errorMessage(reason, '加载备份记录失败'))
  } finally {
    backupLoading.value = false
  }
}

async function createFullBackup() {
  if (!requireDesktopFileAccess('创建 .tqb 备份')) return
  let outputPath: string | null
  try {
    outputPath = await backend.pickBackupSavePath()
  } catch (reason) {
    ElMessage.error(errorMessage(reason, '无法打开备份保存窗口'))
    return
  }
  if (!outputPath) return

  taskState.value = 'backing-up'
  try {
    await backend.createBackup(outputPath)
    await loadBackups(false)
    ElMessage.success('完整备份已创建，并通过数据库、清单和逐文件哈希校验')
  } catch (reason) {
    ElMessage.error(errorMessage(reason, '创建完整备份失败'))
  } finally {
    taskState.value = 'idle'
  }
}

async function chooseBackupForRestore() {
  if (!requireDesktopFileAccess('选择并恢复 .tqb 备份')) return
  if (restorePreparing.value || restoreSubmitting.value) return
  try {
    const path = await backend.pickBackupFile()
    if (!path) return
    restorePreparing.value = true
    const plan = await backend.prepareRestore(path)
    selectedBackupPath.value = path
    restorePlan.value = plan
    restoreConfirmed.value = false
    restoreScheduled.value = false
    restoreDialogOpen.value = true
  } catch (reason) {
    ElMessage.error(errorMessage(reason, '备份文件未通过恢复预检'))
  } finally {
    restorePreparing.value = false
  }
}

async function cancelPreparedRestore() {
  const token = restorePlan.value?.restoreToken
  if (!token || restoreScheduled.value) return
  try {
    await backend.cancelRestore(token)
  } catch (reason) {
    ElMessage.error(errorMessage(reason, '无法清理本次恢复预检，请稍后重试'))
    throw reason
  }
  restorePlan.value = null
  selectedBackupPath.value = ''
  restoreConfirmed.value = false
}

async function closeRestoreDialog() {
  if (restoreSubmitting.value) return
  try {
    await cancelPreparedRestore()
    restoreDialogOpen.value = false
  } catch {
    // 清理失败时保留对话框，避免遗留一个用户看不见的恢复计划。
  }
}

function beforeRestoreDialogClose(done: () => void) {
  if (restoreSubmitting.value) return
  void cancelPreparedRestore().then(done).catch(() => undefined)
}

async function confirmRestore() {
  if (!requireDesktopFileAccess('恢复 .tqb 备份')) return
  const plan = restorePlan.value
  if (!plan || !restoreConfirmed.value || restoreSubmitting.value) return
  restoreSubmitting.value = true
  try {
    const scheduled = await backend.scheduleRestore({
      restoreToken: plan.restoreToken,
      expectedArchiveSha256Hex: plan.archiveInspection.archiveSha256Hex,
      confirmedCurrentDataReplacement: true,
    })
    restoreScheduled.value = true
    restoreDialogOpen.value = false
    taskState.value = 'restoring'
    ElMessage.success(scheduled.preRestoreBackupFilename
      ? `恢复前备份已创建：${scheduled.preRestoreBackupFilename}`
      : '救援恢复已安排；当前损坏数据会在切换前原样隔离保留')
    if (!scheduled.requiresRestart) {
      await loadLastRestoreResult()
    }
  } catch (reason) {
    ElMessage.error(errorMessage(reason, '无法安排恢复，当前数据没有被替换'))
  } finally {
    restoreSubmitting.value = false
  }
}

async function loadLastRestoreResult() {
  if (!desktopAvailable) return
  try {
    const result = await backend.getLastRestoreResult()
    if (!result) return
    restoreResult.value = result
    taskState.value = 'restored'
    activeSection.value = 'backup'
  } catch (reason) {
    ElMessage.error(errorMessage(reason, '无法读取上一次恢复结果'))
  }
}

async function acknowledgeRestoreResult(goToQuestions = false) {
  const result = restoreResult.value
  if (!result) return
  try {
    await backend.acknowledgeRestoreResult(result.operationId)
    restoreResult.value = null
    taskState.value = 'idle'
    if (goToQuestions) await router.push('/questions')
  } catch (reason) {
    ElMessage.error(errorMessage(reason, '无法确认恢复结果'))
  }
}

function formatDuration(durationMs: number) {
  if (durationMs < 1_000) return `${durationMs} 毫秒`
  return `${(durationMs / 1_000).toFixed(1)} 秒`
}

function viewBackup(backup: BackupRecord) {
  void ElMessageBox.alert(
    `归档文件：${backup.payloadFileCount} 个\n有效载荷：${formatBackupSize(backup.payloadByteSize)}\n数据库版本：${backup.databaseSchemaVersion}\n文件：${backup.displayFilename}\n状态：${backup.status === 'ready' ? '创建时已完整校验' : backup.status}`,
    '备份内容',
    { confirmButtonText: '知道了' },
  )
}

async function chooseDataMoveTarget() {
  if (!requireDesktopFileAccess('选择并迁移数据目录')) return
  if (dataMovePreparing.value || dataMoveSubmitting.value) return
  dataMovePreparing.value = true
  try {
    const target = await backend.pickDataDirectory()
    if (!target) return
    const plan = await backend.prepareDataMove(target)
    dataMovePlan.value = plan
    dataMoveConfirmed.value = false
    dataMoveScheduled.value = false
    locationDialogOpen.value = true
  } catch (reason) {
    ElMessage.error(errorMessage(reason, '目标数据目录未通过安全预检'))
  } finally {
    dataMovePreparing.value = false
  }
}

async function cancelPreparedDataMove() {
  const token = dataMovePlan.value?.moveToken
  if (!token || dataMoveScheduled.value) return
  await backend.cancelDataMove(token)
  dataMovePlan.value = null
  dataMoveConfirmed.value = false
}

async function closeDataMoveDialog() {
  if (dataMoveSubmitting.value) return
  try {
    await cancelPreparedDataMove()
    locationDialogOpen.value = false
  } catch (reason) {
    ElMessage.error(errorMessage(reason, '无法清理数据迁移计划，请稍后重试'))
  }
}

function beforeDataMoveDialogClose(done: () => void) {
  if (dataMoveSubmitting.value) return
  void cancelPreparedDataMove().then(done).catch((reason) => {
    ElMessage.error(errorMessage(reason, '无法清理数据迁移计划'))
  })
}

async function confirmDataMove() {
  if (!requireDesktopFileAccess('迁移数据目录')) return
  const plan = dataMovePlan.value
  if (!plan || !dataMoveConfirmed.value || dataMoveSubmitting.value) return
  dataMoveSubmitting.value = true
  taskState.value = 'moving'
  try {
    const scheduled = await backend.scheduleDataMove({
      moveToken: plan.moveToken,
      expectedSourceDataRoot: plan.sourceDataRoot,
      expectedTargetDataRoot: plan.targetDataRoot,
      confirmedKeepOldData: true,
    })
    dataMoveScheduled.value = true
    dataMovePlan.value = null
    locationDialogOpen.value = false
    ElMessage.success(`已复制并校验 ${scheduled.copiedFileCount} 个文件，软件即将重启`)
    if (!scheduled.requiresRestart) await loadLastDataMoveResult()
  } catch (reason) {
    taskState.value = 'idle'
    if (isCommandErrorPayload(reason) && [
      'DATA_MOVE_PLAN_EXPIRED',
      'DATA_MOVE_PLAN_INVALID',
      'DATA_MOVE_PLAN_NOT_FOUND',
      'DATA_MOVE_CONFIRMATION_MISMATCH',
    ].includes(reason.code)) {
      dataMovePlan.value = null
      dataMoveConfirmed.value = false
      locationDialogOpen.value = false
    }
    ElMessage.error(errorMessage(reason, '数据目录迁移失败，软件仍使用原目录'))
  } finally {
    dataMoveSubmitting.value = false
  }
}

async function loadLastDataMoveResult() {
  if (!desktopAvailable) return
  try {
    const result = await backend.getLastDataMoveResult()
    if (!result) return
    dataMoveResult.value = result
    taskState.value = 'moved'
    activeSection.value = 'directory'
    await appStore.initialize()
  } catch (reason) {
    ElMessage.error(errorMessage(reason, '无法读取上一次数据迁移结果'))
  }
}

async function acknowledgeDataMoveResult() {
  const result = dataMoveResult.value
  if (!result) return
  try {
    await backend.acknowledgeDataMoveResult(result.operationId)
    dataMoveResult.value = null
    taskState.value = 'idle'
  } catch (reason) {
    ElMessage.error(errorMessage(reason, '无法确认数据迁移结果'))
  }
}

async function openFolder(component: DataDirectoryComponent) {
  if (!requireDesktopFileAccess('打开数据目录')) return
  try {
    await backend.openDataDirectory(component)
  } catch (reason) {
    ElMessage.error(errorMessage(reason, '无法打开数据目录'))
  }
}

onBeforeRouteLeave(async () => {
  if (dataMoveSubmitting.value || restoreSubmitting.value) {
    ElMessage.warning('正在提交需要重启的安全操作，请等待软件自动重启。')
    return false
  }
  if (dataMovePlan.value && !dataMoveScheduled.value) {
    try {
      await backend.cancelDataMove(dataMovePlan.value.moveToken)
      dataMovePlan.value = null
    } catch (reason) {
      ElMessage.error(errorMessage(reason, '数据迁移计划尚未清理，暂时不能离开页面'))
      return false
    }
  }
  if (!restorePlan.value || restoreScheduled.value) return true
  try {
    await backend.cancelRestore(restorePlan.value.restoreToken)
    restorePlan.value = null
    return true
  } catch (reason) {
    ElMessage.error(errorMessage(reason, '恢复预检临时文件尚未清理，暂时不能离开页面'))
    return false
  }
})
onMounted(() => {
  if (!desktopAvailable) return
  void loadBackups()
  void loadLastRestoreResult()
  void loadLastDataMoveResult()
})
</script>

<template>
  <div class="app-page">
    <aside class="page-sidebar data-sidebar">
      <div class="sidebar-title">数据管理</div>
      <nav class="sidebar-menu">
        <button type="button" class="sidebar-menu__item" :class="{ 'is-active': activeSection === 'export' }" :aria-pressed="activeSection === 'export'" :aria-current="activeSection === 'export' ? 'page' : undefined" @click="selectSection('export')">
          <span class="nav-icon" aria-hidden="true" />题库导出
        </button>
        <button type="button" class="sidebar-menu__item" :class="{ 'is-active': activeSection === 'backup' }" :aria-pressed="activeSection === 'backup'" :aria-current="activeSection === 'backup' ? 'page' : undefined" @click="selectSection('backup')">
          <span class="nav-icon" aria-hidden="true" />数据备份与恢复
        </button>
        <button type="button" class="sidebar-menu__item" :class="{ 'is-active': activeSection === 'resources' }" :aria-pressed="activeSection === 'resources'" :aria-current="activeSection === 'resources' ? 'page' : undefined" @click="selectSection('resources')">
          <span class="nav-icon" aria-hidden="true" />资源清理
        </button>
        <button type="button" class="sidebar-menu__item" :class="{ 'is-active': activeSection === 'directory' }" :aria-pressed="activeSection === 'directory'" :aria-current="activeSection === 'directory' ? 'page' : undefined" @click="selectSection('directory')">
          <span class="nav-icon" aria-hidden="true" />数据目录
        </button>
      </nav>
      <div class="sidebar-note">{{ desktopAvailable ? '所有备份、恢复和资源检查均在本地完成，不需要网络。' : '浏览器只展示数据管理界面，不会读取、备份、恢复或迁移电脑中的真实数据。' }}</div>
    </aside>

    <section class="page-main data-page">
      <el-alert
        v-if="!desktopAvailable"
        class="browser-runtime-note"
        type="warning"
        :closable="false"
        show-icon
        title="浏览器界面演示不会创建或恢复 .tqb 备份，也不会选择、打开或迁移数据目录。以下入口均已关闭。"
      />

      <template v-if="taskState === 'idle'">
        <template v-if="activeSection === 'export'">
          <div class="surface export-card">
            <h2>可用的导出方式</h2>
            <el-alert
              v-if="desktopAvailable && !appStore.license.capabilities.canExportDocuments"
              title="当前为基础桌面模式：题库 Word/Excel 导出已关闭，软件专用备份仍可正常使用。"
              type="info"
              :closable="false"
              show-icon
            />
            <div class="export-option-grid" role="list">
              <article class="export-option" role="listitem" aria-labelledby="word-export-title">
                <span class="export-option__mark" aria-hidden="true">W</span>
                <div>
                  <header><h3 id="word-export-title">Word 题库</h3><el-tag :type="desktopAvailable ? 'success' : 'info'" effect="plain">{{ desktopAvailable ? '桌面版已接入' : '浏览器不可用' }}</el-tag></header>
                  <p id="word-export-status">按学科、章节和题型整理全部有效题目，包含选项、答案、解析、标签、受管图片及支持的表格和可编辑公式，不设置分值。</p>
                  <el-button type="primary" :loading="wordExporting" :disabled="!desktopAvailable || !appStore.databaseHealthy || excelExporting || !appStore.license.capabilities.canExportDocuments" aria-describedby="word-export-status" @click="exportQuestionBank('docx')">{{ !desktopAvailable ? '桌面版才能导出' : appStore.license.capabilities.canExportDocuments ? '导出 Word 题库' : '专业版可导出' }}</el-button>
                </div>
              </article>
              <article class="export-option" role="listitem" aria-labelledby="excel-export-title">
                <span class="export-option__mark" aria-hidden="true">X</span>
                <div>
                  <header><h3 id="excel-export-title">Excel 数据</h3><el-tag :type="desktopAvailable ? 'success' : 'info'" effect="plain">{{ desktopAvailable ? '桌面版已接入' : '浏览器不可用' }}</el-tag></header>
                  <p id="excel-export-status">每道题占一行，导出题型、分类、题干、选项、答案、解析、标签和时间信息，不设置分值。</p>
                  <el-button type="primary" :loading="excelExporting" :disabled="!desktopAvailable || !appStore.databaseHealthy || wordExporting || !appStore.license.capabilities.canExportDocuments" aria-describedby="excel-export-status" @click="exportQuestionBank('xlsx')">{{ !desktopAvailable ? '桌面版才能导出' : appStore.license.capabilities.canExportDocuments ? '导出 Excel 数据' : '专业版可导出' }}</el-button>
                </div>
              </article>
              <article class="export-option" role="listitem" aria-labelledby="backup-export-title">
                <span class="export-option__mark" aria-hidden="true">B</span>
                <div>
                  <header><h3 id="backup-export-title">软件专用备份</h3><el-tag :type="desktopAvailable ? 'success' : 'info'" effect="plain">{{ desktopAvailable ? '桌面版已接入' : '浏览器不可用' }}</el-tag></header>
                  <p>{{ desktopAvailable ? '创建真实的 .tqb 完整备份，包含 SQLite 数据库、Word 模板和受管资源，并在完成前执行完整校验。' : '浏览器不会创建模拟备份；请在 Windows 桌面版中选择保存位置并生成真实 .tqb 文件。' }}</p>
                  <el-button type="primary" :icon="Upload" :disabled="!desktopAvailable || !appStore.databaseHealthy" @click="createFullBackup">{{ desktopAvailable ? '创建专用备份' : '桌面版才能创建' }}</el-button>
                </div>
              </article>
            </div>
          </div>
        </template>

        <template v-else-if="activeSection === 'backup'">
          <div class="backup-action-grid">
            <article class="surface backup-action-card">
              <span class="action-icon action-icon--blue" aria-hidden="true">▣</span>
              <div><h2>创建完整备份</h2><p>包含题库、分类、标签、历史试卷、模板、设置和图片资源。</p><el-button type="primary" :loading="backupLoading" :disabled="!desktopAvailable" @click="createFullBackup">{{ desktopAvailable ? '创建备份' : '桌面版才能创建' }}</el-button></div>
            </article>
            <article class="surface backup-action-card">
              <span class="action-icon action-icon--green" aria-hidden="true">↻</span>
              <div><h2>从备份恢复</h2><p>先完整校验备份，再创建恢复前备份；重启切换失败时会自动回滚。</p><el-button :loading="restorePreparing" :disabled="!desktopAvailable" @click="chooseBackupForRestore">{{ desktopAvailable ? '选择并检查备份' : '桌面版才能恢复' }}</el-button></div>
            </article>
          </div>

          <article v-loading="backupLoading" class="surface recent-backups">
            <header><h2>{{ desktopAvailable ? '最近的备份' : '真实备份记录' }}</h2><el-button :disabled="!desktopAvailable" @click="openFolder('backup')">{{ desktopAvailable ? '打开备份目录' : '桌面版才能打开' }}</el-button></header>
            <div v-for="(backup, index) in backups" :key="backup.id" class="backup-row">
              <span class="backup-file">B</span>
              <div class="backup-name"><strong>{{ backup.displayFilename }}</strong><small>{{ backup.fileAvailable === null ? '保存在用户选择的外部目录' : backup.fileAvailable ? '软件备份目录 · 文件可用' : '软件备份目录 · 文件缺失' }}</small></div>
              <div class="backup-tags">
                <el-tag size="small" :type="backup.backupKind === 'automatic' ? 'primary' : 'info'">{{ backupKindLabel(backup.backupKind) }}</el-tag>
                <el-tag v-if="index === 0" size="small" type="success">最新</el-tag>
              </div>
              <span>{{ formatBackupDate(backup.createdAt) }}</span><span>{{ formatBackupSize(backup.archiveByteSize) }}</span>
              <el-button link type="primary" @click="viewBackup(backup)">查看内容</el-button>
              <el-button link type="primary" :loading="restorePreparing" :disabled="!desktopAvailable" @click="chooseBackupForRestore">选择恢复文件</el-button>
            </div>
            <el-empty v-if="!backupLoading && !backups.length" :image-size="70" :description="desktopAvailable ? '还没有真实备份记录' : '浏览器不会读取或生成电脑中的备份记录'" />
          </article>
        </template>

        <template v-else-if="activeSection === 'resources'">
          <div class="surface cleanup-card">
            <el-alert type="warning" :closable="false" title="为避免误删正在被题目、草稿或历史试卷使用的图片，当前版本不会永久删除资源文件。" />
            <div class="migration-note"><strong>为什么暂时不能清理？</strong><p>新 Word 导入图片的引用已经覆盖导入草稿、编辑草稿、正式题目和历史试卷快照；但编辑器手工插入的图片、公式和旧版数据还没有全部迁移。完成这部分迁移前，软件无法可靠判断文件是否真正“无引用”，因此不会永久删除。</p></div>
            <div class="migration-note"><strong>后续安全流程</strong><p>统一图片/公式入库 → 为所有题目、草稿和试卷建立引用 → 扫描并再次核对 → 用户明确选择 → 永久删除。索引未完整前，删除按钮保持关闭。</p></div>
          </div>
        </template>

        <template v-else>
          <article class="surface storage-card">
            <header>
              <div><h2>当前数据目录</h2><p>{{ dataRoot }}</p></div>
              <div class="storage-actions">
                <el-tag :type="desktopAvailable ? (appStore.databaseHealthy ? 'success' : 'danger') : 'info'">{{ desktopAvailable ? (appStore.databaseHealthy ? '运行正常' : '数据库不可用') : '浏览器演示' }}</el-tag>
                <el-button :loading="dataMovePreparing" :disabled="!desktopAvailable || !appStore.databaseHealthy || !appStore.dataRoot" @click="chooseDataMoveTarget">{{ desktopAvailable ? '迁移数据位置' : '桌面版才能迁移' }}</el-button>
              </div>
            </header>
            <div class="warning-banner"><el-icon aria-hidden="true"><WarningFilled /></el-icon> {{ desktopAvailable ? '迁移预检会实时统计需要复制的真实文件数量和总大小。' : '浏览器不会执行迁移预检，也不会读取电脑中的文件数量和大小。' }}</div>
            <div class="folder-grid">
              <button v-for="folder in storageFolders" :key="folder.name" type="button" :disabled="!desktopAvailable" :aria-label="`打开${folder.description}目录`" @click="openFolder(folder.name)"><strong>{{ folder.name }}</strong><b>{{ desktopAvailable ? folder.size : '不可读取' }}</b><small>{{ folder.description }}</small></button>
            </div>
          </article>

          <article class="surface resource-overview">
            <header><div><h2>图片与公式资源</h2><p>新 Word 图片已建立完整引用；旧数据、手工图片和公式仍待迁移，当前不会自动清理文件。</p></div><el-button @click="selectSection('resources')">查看安全说明</el-button></header>
          </article>
        </template>
      </template>

      <template v-else-if="taskState === 'backing-up' || taskState === 'restoring' || taskState === 'moving'">
        <PageHeader :title="taskTitle" :subtitle="taskState === 'restoring' ? '恢复已安全排队，软件将自动重启并切换数据。' : taskState === 'moving' ? '正在复制并逐文件校验；完成前当前数据位置不会改变。' : '请保持软件运行，处理过程中不会修改题库内容。'" />
        <div class="surface progress-stage" role="status" aria-live="polite" aria-atomic="true">
          <span class="progress-icon" aria-hidden="true">{{ taskState === 'restoring' ? '↻' : taskState === 'moving' ? '→' : '▣' }}</span>
          <h2>{{ taskState === 'backing-up' ? '正在创建并校验完整备份' : taskState === 'moving' ? '正在复制数据库、模板和资源' : '正在准备重启' }}</h2>
          <div class="progress-wrap"><el-progress :percentage="taskState === 'restoring' ? 100 : 0" :stroke-width="8" :status="taskState === 'restoring' ? 'success' : undefined" :indeterminate="taskState !== 'restoring'" :duration="2" /></div>
          <p v-if="taskState === 'restoring'">恢复前备份和候选数据复检已完成，接下来由启动流程完成安全切换。</p>
          <p v-else-if="taskState === 'moving'">数据库使用一致性快照，其他文件会在复制前后核对 SHA-256 摘要。</p>
          <p v-else>软件正在创建一致的数据库快照、打包受管文件，并重新打开归档进行校验。</p>
          <ul v-if="taskState === 'backing-up'">
            <li class="is-active">正在创建真实的 .tqb 备份文件</li>
            <li>写入完成后将校验归档清单和逐文件 SHA-256</li>
            <li>只有后端确认完成后才会显示成功提示</li>
          </ul>
          <ul v-else-if="taskState === 'restoring'">
            <li class="is-done">备份归档、清单和逐文件哈希已校验</li>
            <li class="is-done">恢复候选数据库和受管资源已复检</li>
            <li class="is-done">当前数据已完成安全保护（完整备份或原样隔离）</li>
            <li class="is-active">等待软件自动重启并切换</li>
          </ul>
          <ul v-else>
            <li class="is-done">当前题库继续在原目录运行</li>
            <li class="is-active">正在复制到空的目标目录</li>
            <li>复制完成后复检数据库、资源和完整目录摘要</li>
            <li>全部通过后才会安排重启切换</li>
          </ul>
          <small>{{ taskState === 'backing-up' ? '正在写入您刚才选择的 .tqb 文件；完成前不会覆盖已有文件' : '请保持软件运行；异常中断不会改变当前数据位置' }}</small>
        </div>
      </template>

      <template v-else-if="taskState === 'moved' && dataMoveResult">
        <PageHeader :title="dataMoveResult.outcome === 'success' ? '数据目录迁移完成' : '数据目录迁移已回滚'" :subtitle="dataMoveResult.outcome === 'success' ? '新目录已通过启动复检并成为当前数据位置。' : '目标目录未启用，软件继续使用原数据目录。'" />
        <div class="surface result-stage restore-result" :class="{ 'is-rolled-back': dataMoveResult.outcome === 'rolled_back' }" role="status" aria-live="polite" aria-atomic="true">
          <span class="success-ring" aria-hidden="true">{{ dataMoveResult.outcome === 'success' ? '✓' : '!' }}</span>
          <h2>{{ dataMoveResult.outcome === 'success' ? '迁移成功' : '迁移失败，已继续使用原目录' }}</h2>
          <p>{{ dataMoveResult.errorMessage || '旧目录仍完整保留，不会自动删除。' }}</p>
          <div class="result-metrics"><div><strong>{{ dataMoveResult.copiedFileCount }}</strong><span>已校验文件</span></div><div><strong>{{ formatBackupSize(dataMoveResult.copiedByteSize) }}</strong><span>复制数据量</span></div></div>
          <dl><div><dt>当前活动目录</dt><dd>{{ dataMoveResult.activeDataRoot }}</dd></div><div><dt>保留的目录</dt><dd>{{ dataMoveResult.retainedDataRoot }}</dd></div><div><dt>处理耗时</dt><dd>{{ formatDuration(dataMoveResult.durationMs) }}</dd></div></dl>
          <ul v-if="dataMoveResult.warnings.length" class="restore-result-warnings"><li v-for="warning in dataMoveResult.warnings" :key="warning">{{ warning }}</li></ul>
          <el-button type="primary" @click="acknowledgeDataMoveResult">我知道了</el-button>
        </div>
      </template>

      <template v-else-if="restoreResult">
        <PageHeader :title="restoreResult.outcome === 'success' ? '数据恢复完成' : '恢复已自动回滚'" :subtitle="restoreResult.outcome === 'success' ? '恢复后的数据库、模板和资源已通过复检。' : '恢复切换未成功，但原有数据已恢复到原位置。'" />
        <div class="surface result-stage restore-result" :class="{ 'is-rolled-back': restoreResult.outcome === 'rolled_back' }" role="status" aria-live="polite" aria-atomic="true">
          <span class="success-ring" aria-hidden="true">{{ restoreResult.outcome === 'success' ? '✓' : '!' }}</span>
          <h2>{{ restoreResult.outcome === 'success' ? '恢复成功' : '恢复失败，原数据已自动找回' }}</h2>
          <p>{{ restoreResult.outcome === 'success' ? (restoreResult.preRestoreBackupFilename ? '恢复前已创建当前数据备份，如需撤销可再次从该备份恢复。' : '救援恢复已完成，切换前的损坏数据目录仍原样保留。') : restoreResult.errorMessage }}</p>
          <div v-if="restoreResult.summary" class="result-metrics">
            <div><strong>{{ restoreResult.summary.questionCount }}</strong><span>题目</span></div>
            <div><strong>{{ restoreResult.summary.paperCount }}</strong><span>历史试卷</span></div>
            <div><strong>{{ restoreResult.summary.templateCount }}</strong><span>Word 模板</span></div>
            <div><strong>{{ restoreResult.summary.resourceCount }}</strong><span>受管资源</span></div>
          </div>
          <dl>
            <div><dt>恢复耗时</dt><dd>{{ formatDuration(restoreResult.durationMs) }}</dd></div>
            <div><dt>恢复前备份</dt><dd>{{ restoreResult.preRestoreBackupFilename || '未提供' }}</dd></div>
            <div><dt>数据状态</dt><dd :class="restoreResult.outcome === 'success' ? 'text-success' : 'text-warning'">{{ restoreResult.outcome === 'success' ? '全部复检通过' : '已回滚到原数据' }}</dd></div>
          </dl>
          <ul v-if="restoreResult.warnings.length" class="restore-result-warnings"><li v-for="warning in restoreResult.warnings" :key="warning">{{ warning }}</li></ul>
          <div><el-button @click="acknowledgeRestoreResult(false)">我知道了</el-button><el-button type="primary" @click="acknowledgeRestoreResult(true)">返回题库</el-button></div>
        </div>
      </template>
    </section>

    <el-dialog v-model="restoreDialogOpen" title="确认从备份恢复" width="720px" :close-on-click-modal="false" :before-close="beforeRestoreDialogClose">
      <template v-if="restorePlan">
        <div class="restore-warning"><span>!</span><div><strong>恢复会用备份内容替换当前题库</strong><p>{{ restorePlan.currentDatabaseHealthy ? '软件会先自动创建当前数据的完整备份，再重启切换；切换失败会自动回滚。' : '当前损坏数据会先原样隔离保留，再重启切换；切换失败会自动回滚。' }}</p></div></div>
        <dl class="restore-details">
          <div><dt>备份文件</dt><dd>{{ restorePlan.sourceDisplayFilename }}</dd></div>
          <div><dt>备份时间</dt><dd>{{ formatBackupDate(restorePlan.archiveInspection.createdAt) }}</dd></div>
          <div><dt>归档内容</dt><dd>{{ restorePlan.archiveInspection.payloadFileCount }} 个文件 · {{ formatBackupSize(restorePlan.archiveInspection.payloadByteSize) }}</dd></div>
          <div><dt>校验状态</dt><dd class="text-success">归档、数据库、模板和资源全部通过</dd></div>
        </dl>
        <div class="restore-comparison">
          <strong>恢复前后数据对比</strong>
          <table><thead><tr><th>项目</th><th>当前题库</th><th>备份内容</th></tr></thead><tbody>
            <tr><td>题目</td><td>{{ restorePlan.currentSummary?.questionCount ?? '无法读取' }}</td><td>{{ restorePlan.incomingSummary.questionCount }}</td></tr>
            <tr><td>历史试卷</td><td>{{ restorePlan.currentSummary?.paperCount ?? '无法读取' }}</td><td>{{ restorePlan.incomingSummary.paperCount }}</td></tr>
            <tr><td>Word 模板</td><td>{{ restorePlan.currentSummary?.templateCount ?? '无法读取' }}</td><td>{{ restorePlan.incomingSummary.templateCount }}</td></tr>
            <tr><td>受管资源</td><td>{{ restorePlan.currentSummary?.resourceCount ?? '无法读取' }}</td><td>{{ restorePlan.incomingSummary.resourceCount }}</td></tr>
          </tbody></table>
        </div>
        <ul class="restore-plan-warnings"><li v-for="warning in restorePlan.warnings" :key="warning">{{ warning }}</li></ul>
        <el-alert v-if="!restorePlan.currentDatabaseHealthy" type="warning" :closable="false" title="这是救援恢复：当前数据库无法读取，不能生成标准 .tqb；软件会把现有数据目录原样隔离保留后再切换。" />
        <el-checkbox v-model="restoreConfirmed">{{ restorePlan.currentDatabaseHealthy ? '我已理解：当前题库会先被安全备份，然后由所选备份内容替换。' : '我已理解：当前损坏数据会原样隔离保留，然后由所选备份内容替换。' }}</el-checkbox>
      </template>
      <template #footer><el-button :disabled="restoreSubmitting" @click="closeRestoreDialog">取消</el-button><el-button type="danger" :loading="restoreSubmitting" :disabled="!desktopAvailable || !restorePlan || !restoreConfirmed" @click="confirmRestore">确认恢复并重启</el-button></template>
    </el-dialog>

    <el-dialog v-model="locationDialogOpen" title="确认迁移数据目录" width="680px" :close-on-click-modal="false" :before-close="beforeDataMoveDialogClose">
      <template v-if="dataMovePlan">
        <div class="warning-banner"><el-icon aria-hidden="true"><WarningFilled /></el-icon> 迁移会复制全部持久化数据并自动重启；当前目录不会被移动或删除。</div>
        <dl class="restore-details">
          <div><dt>当前目录</dt><dd>{{ dataMovePlan.sourceDataRoot }}</dd></div>
          <div><dt>目标空目录</dt><dd>{{ dataMovePlan.targetDataRoot }}</dd></div>
          <div><dt>预计复制</dt><dd>{{ dataMovePlan.estimatedFileCount }} 个文件 · {{ formatBackupSize(dataMovePlan.estimatedByteSize) }}</dd></div>
          <div><dt>切换方式</dt><dd>复制并复检成功后，重启切换配置指针</dd></div>
        </dl>
        <ul class="restore-plan-warnings"><li v-for="warning in dataMovePlan.warnings" :key="warning">{{ warning }}</li></ul>
        <el-checkbox v-model="dataMoveConfirmed">我确认目标目录为空，并理解迁移成功后旧数据目录仍会保留。</el-checkbox>
      </template>
      <template #footer><el-button :disabled="dataMoveSubmitting" @click="closeDataMoveDialog">取消</el-button><el-button type="primary" :loading="dataMoveSubmitting" :disabled="!desktopAvailable || !dataMovePlan || !dataMoveConfirmed" @click="confirmDataMove">开始复制并校验</el-button></template>
    </el-dialog>
  </div>
</template>

<style scoped>
.data-sidebar { padding-top: 2px; }
.nav-icon { width: 18px; height: 18px; border: 1.5px solid #94a3b8; border-radius: 5px; }
.sidebar-menu__item.is-active .nav-icon { border-color: #3b82f6; }
.sidebar-note { margin: 22px 14px; padding: 13px 12px; border: 1px solid var(--border); border-radius: 8px; background: #fff; color: #526176; font-size: 11px; line-height: 1.75; }
.data-page { position: relative; display: flex; flex-direction: column; }
.browser-runtime-note { margin-bottom: 14px; }
.export-card { padding: 20px 22px; }
.export-card h2, .recent-backups h2, .storage-card h2, .resource-overview h2 { margin: 0; color: #111827; font-size: 14px; }
.export-option-grid { margin-top: 14px; display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 12px; }
.export-option { min-height: 190px; padding: 16px; display: grid; grid-template-columns: 46px minmax(0, 1fr); gap: 12px; border: 1px solid var(--border); border-radius: 9px; background: #fff; }
.export-option__mark { width: 44px; height: 44px; display: grid; place-items: center; border-radius: 9px; background: #eff6ff; color: #2563eb; font-size: 18px; font-weight: 700; }
.export-option > div { min-width: 0; display: flex; flex-direction: column; align-items: flex-start; }
.export-option header { width: 100%; display: flex; align-items: center; justify-content: space-between; gap: 8px; }
.export-option h3 { margin: 0; color: #111827; font-size: 13px; }
.export-option p { min-height: 58px; margin: 10px 0 14px; color: #64748b; font-size: 10px; line-height: 1.7; }
.backup-action-grid { display: grid; grid-template-columns: 1fr 1fr; gap: 14px; }
.backup-action-card { min-height: 180px; padding: 20px; display: flex; gap: 18px; }
.action-icon { width: 50px; height: 50px; flex: 0 0 50px; display: grid; place-items: center; border-radius: 10px; font-size: 18px; }
.action-icon--blue { background: #eff6ff; color: #2563eb; }.action-icon--green { background: #f0fdf4; color: #16a34a; }
.backup-action-card h2 { margin: 0; font-size: 15px; }.backup-action-card p { min-height: 38px; margin: 7px 0 12px; color: #64748b; font-size: 10px; line-height: 1.7; }
.recent-backups { margin-top: 14px; padding: 16px; }.recent-backups header, .storage-card header, .resource-overview header { margin-bottom: 12px; display: flex; align-items: flex-start; justify-content: space-between; }
.backup-row { min-height: 58px; display: grid; grid-template-columns: 34px minmax(260px, 1fr) auto 130px 70px auto auto; align-items: center; gap: 10px; border-bottom: 1px solid #edf1f5; color: #475569; font-size: 10px; }
.backup-file { width: 32px; height: 32px; display: grid; place-items: center; border-radius: 7px; background: #eff6ff; color: #2563eb; font-weight: 700; }
.backup-name strong, .backup-name small { display: block; }.backup-name strong { color: #172033; font-size: 11px; }.backup-name small { margin-top: 3px; color: #94a3b8; }
.backup-tags { display: flex; align-items: center; gap: 5px; }
.selection-summary { margin-bottom: 12px; padding: 12px 14px; display: flex; justify-content: space-between; color: #64748b; font-size: 11px; }
.cleanup-card { padding: 20px; }.cleanup-card .warning-banner { display: flex; align-items: center; gap: 8px; }
.cleanup-grid { margin-top: 14px; display: grid; grid-template-columns: repeat(3, 1fr); gap: 10px; }
.resource-card { position: relative; padding: 9px; display: flex; flex-direction: column; border: 1px solid var(--border); border-radius: 9px; background: #fff; }
.resource-card > .el-checkbox { position: absolute; z-index: 2; top: 12px; right: 12px; }
.resource-preview { height: 130px; display: grid; place-items: end start; padding: 8px; border: 1px solid #fca5a5; border-radius: 7px; background: #fff1f2; color: #334155; font-size: 10px; }
.resource-card strong { margin-top: 8px; color: #111827; font-size: 11px; }.resource-card small { margin-top: 5px; color: #94a3b8; font-size: 10px; }
.scan-row { margin-top: 14px; padding: 12px; display: flex; justify-content: space-between; border: 1px solid var(--border); border-radius: 8px; color: #64748b; font-size: 10px; }.scan-row strong { color: #111827; }
.storage-card, .resource-overview { padding: 20px; }.storage-card header p, .resource-overview header p { margin: 5px 0 0; color: #64748b; font-size: 10px; }
.storage-actions { display: flex; align-items: center; gap: 10px; }
.storage-scale { margin-top: 7px; display: flex; justify-content: space-between; color: #64748b; font-size: 9px; }
.folder-grid { margin-top: 16px; display: grid; grid-template-columns: repeat(5, 1fr); gap: 12px; }.folder-grid button { min-height: 90px; padding: 13px; display: flex; flex-direction: column; align-items: flex-start; border: 1px solid var(--border); border-radius: 9px; background: #fbfcfe; }.folder-grid button:hover { border-color: #93c5fd; background: #eff6ff; }.folder-grid strong { font-size: 11px; }.folder-grid b { margin-top: 9px; font-size: 20px; }.folder-grid small { margin-top: 4px; color: #94a3b8; font-size: 9px; }
.folder-grid button:disabled { cursor: not-allowed; opacity: .62; }.folder-grid button:disabled:hover { border-color: var(--border); background: #fbfcfe; }
.resource-overview { margin-top: 14px; }.resource-strip { display: grid; grid-template-columns: repeat(5, 1fr); gap: 10px; }.resource-strip button { height: 92px; padding: 8px; display: flex; align-items: flex-end; border: 1px solid #dbe3ed; border-radius: 7px; color: #334155; font-size: 9px; }.resource-strip button.is-blue { background: #eaf2ff; }.resource-strip button.is-green { background: #ecfdf3; }.resource-strip button.is-red { border-color: #fca5a5; background: #fff1f2; }
.progress-stage, .result-stage { min-height: 520px; flex: 1; display: flex; flex-direction: column; align-items: center; justify-content: center; text-align: center; }
.progress-icon { width: 70px; height: 70px; display: grid; place-items: center; border-radius: 11px; background: #eff6ff; color: #2563eb; font-size: 20px; font-weight: 700; }.progress-stage h2, .result-stage h2 { margin: 15px 0 5px; font-size: 19px; }.progress-stage > p, .result-stage > p { margin: 0; color: #64748b; font-size: 11px; }.progress-wrap { width: 520px; margin-top: 20px; }.progress-stage ul { width: 520px; margin: 18px 0 10px; padding: 13px 18px; list-style: none; border: 1px solid var(--border); border-radius: 8px; background: #fbfcfe; color: #64748b; font-size: 10px; line-height: 2.4; text-align: left; }.progress-stage li::before { margin-right: 8px; color: #cbd5e1; content: '•'; }.progress-stage li.is-done::before { color: #16a34a; content: '✓'; }.progress-stage li.is-active::before { color: #2563eb; }.progress-stage > small { color: #94a3b8; }
.success-ring { width: 88px; height: 88px; display: grid; place-items: center; border: 7px solid #bbf7d0; border-radius: 50%; color: #16a34a; font-size: 38px; }.result-stage > .el-button { margin-top: 14px; }
.result-metrics { width: 520px; margin: 22px 0 14px; display: grid; grid-template-columns: repeat(4, 1fr); gap: 8px; }.result-metrics div { padding: 14px; border-radius: 7px; background: #f8fafc; text-align: left; }.result-metrics strong, .result-metrics span { display: block; }.result-metrics strong { font-size: 24px; }.result-metrics span { margin-top: 4px; color: #64748b; font-size: 9px; }.restore-result dl { width: 520px; margin: 0 0 18px; padding: 12px; border: 1px solid var(--border); border-radius: 8px; }.restore-result dl div, .restore-details div { padding: 8px 0; display: flex; justify-content: space-between; border-bottom: 1px solid #edf1f5; font-size: 10px; }.restore-result dt, .restore-details dt { color: #64748b; }.restore-result dd, .restore-details dd { margin: 0; font-weight: 600; }.restore-result.is-rolled-back .success-ring { border-color: #fed7aa; color: #ea580c; }.text-warning { color: #ea580c; }.restore-result-warnings { width: 520px; margin: 0 0 18px; padding: 12px 12px 12px 28px; border-radius: 8px; background: #fffbeb; color: #92400e; font-size: 10px; line-height: 1.7; text-align: left; }
.restore-warning { display: flex; gap: 14px; align-items: center; }.restore-warning > span { width: 46px; height: 46px; display: grid; place-items: center; border-radius: 50%; background: #fff1f2; color: #ef4444; font-size: 22px; font-weight: 700; }.restore-warning strong { font-size: 14px; }.restore-warning p { margin: 4px 0 0; color: #64748b; font-size: 10px; }.restore-details { margin: 15px 0; padding: 12px; border: 1px solid var(--border); border-radius: 8px; }.restore-comparison { margin: 15px 0; }.restore-comparison > strong { font-size: 12px; }.restore-comparison table { width: 100%; margin-top: 8px; border-collapse: collapse; font-size: 10px; }.restore-comparison th, .restore-comparison td { padding: 8px 10px; border: 1px solid #e5e7eb; text-align: center; }.restore-comparison th:first-child, .restore-comparison td:first-child { text-align: left; }.restore-comparison th { background: #f8fafc; color: #475569; }.restore-plan-warnings { margin: 12px 0; padding: 12px 14px 12px 30px; border-radius: 8px; background: #fffbeb; color: #92400e; font-size: 10px; line-height: 1.7; }.dialog-field { margin-top: 13px; }.migration-note { margin-top: 14px; padding: 14px; border: 1px solid var(--border); border-radius: 8px; }.migration-note strong { font-size: 11px; }.migration-note p { margin: 7px 0 0; color: #64748b; font-size: 10px; line-height: 1.6; }

@media (max-width: 1280px) {
  .backup-row { grid-template-columns: 34px minmax(220px, 1fr) auto 110px 60px auto auto; }
  .folder-grid { gap: 8px; }.folder-grid button { padding: 10px; }
  .cleanup-grid { grid-template-columns: repeat(3, 1fr); }
}
</style>
