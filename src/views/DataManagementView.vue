<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { onBeforeRouteLeave, useRoute, useRouter } from 'vue-router'
import { ElMessage, ElMessageBox } from 'element-plus'
import { FolderOpened, RefreshLeft, Files, Setting, WarningFilled } from '@element-plus/icons-vue'
import { backend, isDesktopRuntime } from '../services/backend'
import { errorMessage, isCommandErrorPayload } from '../services/errors'
import { useAppStore } from '../stores/app'
import SettingsView from './SettingsView.vue'
import type {
  BackupRecord,
  DataMovePlan,
  DataMoveResult,
  RestorePlan,
  RestoreResult,
} from '../types/domain'

type TaskState = 'idle' | 'backing-up' | 'restoring' | 'restored' | 'moving' | 'moved'
type DataDirectoryComponent = 'root' | 'database' | 'resources' | 'templates' | 'backup' | 'export'

const router = useRouter()
const appStore = useAppStore()
const route = useRoute()
const dataLocationOpen = ref(route.query.panel === 'location')
watch(() => route.query.panel, (panel) => { dataLocationOpen.value = panel === 'location' })
function beforeDataLocationClose(done: () => void) {
  if (dataMovePreparing.value || dataMoveSubmitting.value || locationDialogOpen.value) return
  done()
  if (route.query.panel === 'location') {
    const query = { ...route.query }
    delete query.panel
    void router.replace({ path: route.path, query })
  }
}
const automaticBackupOpen = ref(false)
const automaticBackupForm = ref<InstanceType<typeof SettingsView> | null>(null)
async function beforeAutomaticBackupClose(done: () => void) {
  if (!automaticBackupForm.value || await automaticBackupForm.value.canLeave()) done()
}
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
  { name: 'export', size: '受管', description: '内置导出目录' },
]

const taskTitle = computed(() => ({
  'backing-up': '正在创建备份',
  restoring: '正在恢复数据',
  restored: '数据恢复完成',
  moving: '正在迁移数据目录',
  moved: '数据目录迁移完成',
  idle: '数据管理',
})[taskState.value])

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
    dataLocationOpen.value = false
    taskState.value = 'restored'
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
    else await loadLastDataMoveResult()
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
  dataLocationOpen.value = false
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
    dataLocationOpen.value = false
    taskState.value = 'moved'
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
  if (backupLoading.value || restorePreparing.value || dataMovePreparing.value || taskState.value === 'backing-up') {
    ElMessage.warning('正在处理数据，请等待当前操作完成。')
    return false
  }
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
onMounted(async () => {
  if (!desktopAvailable) return
  void loadBackups()
  await loadLastRestoreResult()
  if (!restoreResult.value) await loadLastDataMoveResult()
})
</script>

<template>
  <div class="settings-pane">
    <section class="page-main data-page">
      <template v-if="taskState === 'idle'">
        <section class="surface backup-panel">
          <div class="panel-toolbar">
            <div class="toolbar-actions">
              <el-button type="primary" :icon="Files" :loading="backupLoading" :disabled="!desktopAvailable" @click="createFullBackup">创建备份</el-button>
              <el-button :icon="RefreshLeft" :loading="restorePreparing" :disabled="!desktopAvailable" @click="chooseBackupForRestore">从文件恢复</el-button>
            </div>
            <div class="toolbar-actions"><el-button :icon="FolderOpened" :disabled="!desktopAvailable" @click="openFolder('backup')">打开备份目录</el-button><el-button :icon="Setting" @click="automaticBackupOpen = true">自动备份</el-button><el-button @click="dataLocationOpen = true">数据位置</el-button></div>
          </div>
          <div class="backup-table" v-loading="backupLoading">
            <el-table :data="backups" row-key="id" height="100%">
              <el-table-column label="备份文件" min-width="250"><template #default="{ row }"><strong class="backup-filename">{{ row.displayFilename }}</strong><span class="table-secondary">{{ row.fileAvailable === null ? '外部目录' : row.fileAvailable ? '文件可用' : '文件缺失' }}</span></template></el-table-column>
              <el-table-column label="类型" width="140"><template #default="{ row }"><el-tag size="small" :type="row.backupKind === 'automatic' ? 'primary' : 'info'">{{ backupKindLabel(row.backupKind) }}</el-tag></template></el-table-column>
              <el-table-column label="备份时间" width="175"><template #default="{ row }">{{ formatBackupDate(row.createdAt) }}</template></el-table-column>
              <el-table-column label="大小" width="105"><template #default="{ row }">{{ formatBackupSize(row.archiveByteSize) }}</template></el-table-column>
              <el-table-column label="操作" width="110" fixed="right"><template #default="{ row }"><el-button link type="primary" @click="viewBackup(row)">查看内容</el-button></template></el-table-column>
              <template #empty><el-empty :image-size="64" :description="desktopAvailable ? '暂无备份记录' : '请在桌面版查看备份记录'" /></template>
            </el-table>
          </div>
        </section>

      </template>

      <template v-else-if="taskState === 'backing-up' || taskState === 'restoring' || taskState === 'moving'">
        <div class="surface progress-stage" role="status" aria-live="polite" aria-atomic="true">
          <span class="progress-icon" aria-hidden="true">{{ taskState === 'restoring' ? '↻' : taskState === 'moving' ? '→' : '▣' }}</span>
          <h2>{{ taskTitle }}</h2>
          <div class="progress-wrap"><el-progress :percentage="taskState === 'restoring' ? 100 : 0" :stroke-width="8" :status="taskState === 'restoring' ? 'success' : undefined" :indeterminate="taskState !== 'restoring'" :duration="2" /></div>
          <p v-if="taskState === 'restoring'">当前数据已保留，软件将自动重启并完成恢复。</p>
          <p v-else-if="taskState === 'moving'">正在复制并校验文件，原目录仍会保留。</p>
          <p v-else>正在打包题库、模板和图片，并校验备份文件。</p>
          <ul v-if="taskState === 'backing-up'">
            <li class="is-active">正在创建备份文件</li>
            <li>完成后检查文件完整性</li>
            <li>校验通过后完成备份</li>
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

    <el-drawer v-model="dataLocationOpen" title="数据位置" size="min(680px, 100vw)" :before-close="beforeDataLocationClose">
        <section class="surface directory-panel">
          <div class="panel-toolbar"><span class="directory-status">{{ desktopAvailable ? (appStore.databaseHealthy ? '数据库正常' : '数据库不可用') : '桌面版可查看数据目录' }}</span><div class="toolbar-actions"><el-button :icon="FolderOpened" :disabled="!desktopAvailable" @click="openFolder('root')">打开目录</el-button><el-button :loading="dataMovePreparing" :disabled="!desktopAvailable || !appStore.databaseHealthy || !appStore.dataRoot" @click="chooseDataMoveTarget">更改位置</el-button></div></div>
          <div class="directory-path">{{ dataRoot }}</div>
          <div v-for="folder in storageFolders" :key="folder.name" class="directory-row"><el-icon><FolderOpened /></el-icon><strong>{{ folder.description }}</strong><span>{{ folder.name }}/</span><el-button link type="primary" :disabled="!desktopAvailable" :aria-label="`打开${folder.description}`" @click="openFolder(folder.name)">打开</el-button></div>
        </section>
    </el-drawer>

    <el-drawer v-model="automaticBackupOpen" title="自动备份" size="520px" destroy-on-close :before-close="beforeAutomaticBackupClose">
      <SettingsView v-if="automaticBackupOpen" ref="automaticBackupForm" page="backup" embedded />
    </el-drawer>

    <el-dialog v-model="restoreDialogOpen" title="确认从备份恢复" width="720px" :close-on-click-modal="false" :before-close="beforeRestoreDialogClose">
      <template v-if="restorePlan">
        <div class="restore-warning"><span>!</span><div><strong>恢复会用备份内容替换当前题库</strong><p>{{ restorePlan.currentDatabaseHealthy ? '软件会先自动创建当前数据的完整备份，再重启切换；切换失败会自动回滚。' : '当前损坏数据会先原样隔离保留，再重启切换；切换失败会自动回滚。' }}</p></div></div>
        <dl class="restore-details">
          <div><dt>备份文件</dt><dd>{{ restorePlan.sourceDisplayFilename }}</dd></div>
          <div><dt>备份时间</dt><dd>{{ formatBackupDate(restorePlan.archiveInspection.createdAt) }}</dd></div>
          <div><dt>归档内容</dt><dd>{{ restorePlan.archiveInspection.payloadFileCount }} 个文件 · {{ formatBackupSize(restorePlan.archiveInspection.payloadByteSize) }}</dd></div>
          <div><dt>校验状态</dt><dd class="text-success">备份完整，校验通过</dd></div>
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
          <div><dt>切换方式</dt><dd>复制与校验完成后重启生效</dd></div>
        </dl>
        <ul class="restore-plan-warnings"><li v-for="warning in dataMovePlan.warnings" :key="warning">{{ warning }}</li></ul>
        <el-checkbox v-model="dataMoveConfirmed">我确认目标目录为空，并理解迁移成功后旧数据目录仍会保留。</el-checkbox>
      </template>
      <template #footer><el-button :disabled="dataMoveSubmitting" @click="closeDataMoveDialog">取消</el-button><el-button type="primary" :loading="dataMoveSubmitting" :disabled="!desktopAvailable || !dataMovePlan || !dataMoveConfirmed" @click="confirmDataMove">开始复制并校验</el-button></template>
    </el-dialog>
  </div>
</template>

<style scoped>
.settings-pane { display: flex; min-width: 0; min-height: 0; flex: 1; }
.sidebar-menu__item .el-icon { font-size: 16px; }
.data-page { position: relative; display: flex; flex-direction: column; gap: 16px; }
.directory-panel { overflow: hidden; flex-shrink: 0; }
.backup-panel { min-height: 360px; display: flex; flex-direction: column; flex: 1; overflow: hidden; }
.panel-toolbar { padding: 16px 20px; display: flex; align-items: center; justify-content: space-between; flex-wrap: wrap; gap: 12px; border-bottom: 1px solid #edf1f5; flex-shrink: 0; }
.toolbar-actions { display: flex; align-items: center; gap: 10px; flex-wrap: wrap; }
.toolbar-actions :deep(.el-button + .el-button) { margin-left: 0; }
.backup-table { min-height: 0; flex: 1; }
.backup-filename { color: #334155; font-size: 12px; font-weight: 500; overflow-wrap: anywhere; }
.table-secondary { display: block; margin-top: 4px; color: #94a3b8; font-size: 11px; }
.panel-footer { padding: 16px 20px; border-top: 1px solid #edf1f5; display: flex; align-items: center; justify-content: space-between; gap: 20px; color: #94a3b8; font-size: 12px; }
.directory-status { color: #64748b; font-size: 12px; }
.directory-path { padding: 20px 24px; border-bottom: 1px solid #edf1f5; background: #fafbfd; color: #475569; font-size: 13px; overflow-wrap: anywhere; }
.directory-row { min-height: 66px; margin: 0 24px; display: grid; grid-template-columns: 22px 1fr 1fr auto; align-items: center; gap: 14px; border-bottom: 1px solid #edf1f5; }
.directory-row:last-of-type { border-bottom: 0; }
.directory-row .el-icon { color: #94a3b8; font-size: 20px; }
.directory-row strong { font-size: 13px; font-weight: 500; color: #334155; }
.directory-row > span { font-size: 12px; color: #94a3b8; }
.progress-stage, .result-stage { min-height: 520px; flex: 1; display: flex; flex-direction: column; align-items: center; justify-content: center; text-align: center; }
.progress-icon { width: 70px; height: 70px; display: grid; place-items: center; border-radius: 11px; background: #eff6ff; color: #2563eb; font-size: 20px; font-weight: 700; }.progress-stage h2, .result-stage h2 { margin: 15px 0 5px; font-size: 19px; }.progress-stage > p, .result-stage > p { margin: 0; color: #64748b; font-size: 11px; }.progress-wrap { width: 520px; margin-top: 20px; }.progress-stage ul { width: 520px; margin: 18px 0 10px; padding: 13px 18px; list-style: none; border: 1px solid var(--border); border-radius: 8px; background: #fbfcfe; color: #64748b; font-size: 10px; line-height: 2.4; text-align: left; }.progress-stage li::before { margin-right: 8px; color: #cbd5e1; content: '•'; }.progress-stage li.is-done::before { color: #16a34a; content: '✓'; }.progress-stage li.is-active::before { color: #2563eb; }.progress-stage > small { color: #94a3b8; }
.success-ring { width: 88px; height: 88px; display: grid; place-items: center; border: 7px solid #bbf7d0; border-radius: 50%; color: #16a34a; font-size: 38px; }.result-stage > .el-button { margin-top: 14px; }
.result-metrics { width: 520px; margin: 22px 0 14px; display: grid; grid-template-columns: repeat(4, 1fr); gap: 8px; }.result-metrics div { padding: 14px; border-radius: 7px; background: #f8fafc; text-align: left; }.result-metrics strong, .result-metrics span { display: block; }.result-metrics strong { font-size: 24px; }.result-metrics span { margin-top: 4px; color: #64748b; font-size: 9px; }.restore-result dl { width: 520px; margin: 0 0 18px; padding: 12px; border: 1px solid var(--border); border-radius: 8px; }.restore-result dl div, .restore-details div { padding: 8px 0; display: flex; justify-content: space-between; border-bottom: 1px solid #edf1f5; font-size: 10px; }.restore-result dt, .restore-details dt { color: #64748b; }.restore-result dd, .restore-details dd { margin: 0; font-weight: 600; }.restore-result.is-rolled-back .success-ring { border-color: #fed7aa; color: #ea580c; }.text-warning { color: #ea580c; }.restore-result-warnings { width: 520px; margin: 0 0 18px; padding: 12px 12px 12px 28px; border-radius: 8px; background: #fffbeb; color: #92400e; font-size: 10px; line-height: 1.7; text-align: left; }
.restore-warning { display: flex; gap: 14px; align-items: center; }.restore-warning > span { width: 46px; height: 46px; display: grid; place-items: center; border-radius: 50%; background: #fff1f2; color: #ef4444; font-size: 22px; font-weight: 700; }.restore-warning strong { font-size: 14px; }.restore-warning p { margin: 4px 0 0; color: #64748b; font-size: 10px; }.restore-details { margin: 15px 0; padding: 12px; border: 1px solid var(--border); border-radius: 8px; }.restore-comparison { margin: 15px 0; }.restore-comparison > strong { font-size: 12px; }.restore-comparison table { width: 100%; margin-top: 8px; border-collapse: collapse; font-size: 10px; }.restore-comparison th, .restore-comparison td { padding: 8px 10px; border: 1px solid #e5e7eb; text-align: center; }.restore-comparison th:first-child, .restore-comparison td:first-child { text-align: left; }.restore-comparison th { background: #f8fafc; color: #475569; }.restore-plan-warnings { margin: 12px 0; padding: 12px 14px 12px 30px; border-radius: 8px; background: #fffbeb; color: #92400e; font-size: 10px; line-height: 1.7; }.dialog-field { margin-top: 13px; }.migration-note { margin-top: 14px; padding: 14px; border: 1px solid var(--border); border-radius: 8px; }.migration-note strong { font-size: 11px; }.migration-note p { margin: 7px 0 0; color: #64748b; font-size: 10px; line-height: 1.6; }


.progress-stage, .result-stage { min-height: 400px; padding: 32px 24px; }
.progress-wrap, .progress-stage ul, .result-metrics, .restore-result dl, .restore-result-warnings { width: min(100%, 560px); }
.progress-stage > p, .result-stage > p, .progress-stage ul, .restore-details div, .restore-warning p, .restore-comparison table, .restore-plan-warnings { font-size: 12px; }
.restore-result dd, .restore-details dd { overflow-wrap: anywhere; min-width: 0; }
.restore-result dt, .restore-details dt { flex-shrink: 0; margin-right: 16px; }
@media (max-width: 1000px) {
  .panel-footer { flex-wrap: wrap; }
}
</style>
