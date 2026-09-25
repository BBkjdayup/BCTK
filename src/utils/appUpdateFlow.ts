import { h } from 'vue'
import { ElLoading, ElMessage, ElMessageBox } from 'element-plus'
import {
  checkForAppUpdate,
  downloadPendingAppUpdate,
  hasDownloadedAppUpdate,
  installDownloadedAppUpdate,
  installPendingAppUpdate,
  type AppUpdateInfo,
} from '../services/appUpdater'
import { canCloseApplication } from '../services/closeProtection'
import { safeUpdateNotes } from './appUpdatePolicy'

export type AppUpdateFlowOutcome = 'current' | 'deferred' | 'staged' | 'installing'

export interface AppUpdateFlowResult {
  outcome: AppUpdateFlowOutcome
  update: AppUpdateInfo | null
}
let backgroundFlowInFlight: Promise<AppUpdateFlowResult> | null = null
let manualFlowInFlight: Promise<AppUpdateFlowResult> | null = null

function updateMessage(update: AppUpdateInfo) {
  const notes = safeUpdateNotes(update.notes)
  return h('div', { class: 'app-update-prompt' }, [
    h('p', `当前版本 ${update.currentVersion}，发现新版本 ${update.version}。`),
    h('p', '确认后将下载并校验安装包。安装时软件会自动关闭并重新启动，请先保存正在编辑的内容。'),
    notes
      ? h('div', {
          style: 'margin-top: 12px; padding: 10px 12px; white-space: pre-wrap; border-radius: 6px; background: #f6f8fa; color: #475569;',
        }, notes)
      : null,
  ])
}

async function runManualUpdateFlow(): Promise<AppUpdateFlowResult> {
  const update = await checkForAppUpdate()
  if (!update) {
    ElMessage.success('当前已是最新版本')
    return { outcome: 'current', update: null }
  }

  try {
    await ElMessageBox.confirm(updateMessage(update), `发现新版本 ${update.version}`, {
      type: 'info',
      confirmButtonText: '下载并安装',
      cancelButtonText: '稍后提醒',
      closeOnClickModal: false,
    })
  } catch {
    return { outcome: 'deferred', update }
  }

  const loading = ElLoading.service({
    lock: true,
    text: '正在准备下载更新…',
    background: 'rgba(15, 23, 42, 0.35)',
  })
  try {
    if (!(await canCloseApplication())) return { outcome: 'deferred', update }
    await installPendingAppUpdate((progress) => {
      if (progress.phase === 'installing') {
        loading.setText('下载完成，正在校验签名并启动安装…')
      } else if (progress.percent == null) {
        loading.setText('正在下载更新…')
      } else {
        loading.setText(`正在下载更新… ${progress.percent}%`)
      }
    })
    return { outcome: 'installing', update }
  } finally {
    loading.close()
  }
}

async function runBackgroundUpdateFlow(): Promise<AppUpdateFlowResult> {
  let update: AppUpdateInfo | null = null
  try {
    update = await checkForAppUpdate()
    if (!update) return { outcome: 'current', update: null }
    await downloadPendingAppUpdate()
    return { outcome: 'staged', update }
  } catch {
    // A background update must never affect ordinary offline work or closing.
    return { outcome: 'deferred', update }
  }
}

/** Runs after startup without interrupting the user. The verified package is
 * kept ready only for this app session and installed on a later normal exit. */
export function checkAndStageAppUpdate(): Promise<AppUpdateFlowResult> {
  if (manualFlowInFlight) return manualFlowInFlight
  if (backgroundFlowInFlight) return backgroundFlowInFlight
  const task = Promise.resolve().then(runBackgroundUpdateFlow)
  backgroundFlowInFlight = task
  const clear = () => {
    if (backgroundFlowInFlight === task) backgroundFlowInFlight = null
  }
  void task.then(clear, clear)
  return task
}

/** Keeps an explicit Settings action for people who want to install now. */
export function checkAndOfferAppUpdate(): Promise<AppUpdateFlowResult> {
  if (manualFlowInFlight) return manualFlowInFlight
  const background = backgroundFlowInFlight
  const task = Promise.resolve().then(async () => {
    // A manual request must still show its confirmation after staging completes.
    if (background) await background
    return runManualUpdateFlow()
  })
  manualFlowInFlight = task
  const clear = () => {
    if (manualFlowInFlight === task) manualFlowInFlight = null
  }
  void task.then(clear, clear)
  return task
}

/** Called only after every editor has accepted the normal close request. */
export async function installStagedAppUpdateOnClose(): Promise<boolean> {
  if (!hasDownloadedAppUpdate()) return false
  try {
    await installDownloadedAppUpdate()
    return true
  } catch {
    // Preserve the normal close path when the installer cannot be started.
    return false
  }
}
