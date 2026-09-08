import { h } from 'vue'
import { ElLoading, ElMessage, ElMessageBox } from 'element-plus'
import { checkForAppUpdate, installPendingAppUpdate, type AppUpdateInfo } from '../services/appUpdater'
import {
  UPDATE_PROMPT_STORAGE_KEY,
  appUpdatePromptRecord,
  safeUpdateNotes,
  shouldPromptForAppUpdate,
} from './appUpdatePolicy'

export type AppUpdateFlowOutcome = 'current' | 'deferred' | 'installing'

export interface AppUpdateFlowResult {
  outcome: AppUpdateFlowOutcome
  update: AppUpdateInfo | null
}
let updateFlowInFlight: Promise<AppUpdateFlowResult> | null = null

function readLastPrompt(): string | null {
  try {
    return localStorage.getItem(UPDATE_PROMPT_STORAGE_KEY)
  } catch {
    return null
  }
}

function rememberPrompt(version: string) {
  try {
    localStorage.setItem(UPDATE_PROMPT_STORAGE_KEY, appUpdatePromptRecord(version))
  } catch {
    // Update checks must still work when WebView storage is unavailable.
  }
}

function updateMessage(update: AppUpdateInfo) {
  const notes = safeUpdateNotes(update.notes)
  return h('div', { class: 'app-update-prompt' }, [
    h('p', `当前版本 ${update.currentVersion}，发现新版本 ${update.version}。`),
    h('p', '确认后将下载经过数字签名校验的安装包。安装时软件会自动关闭并重新启动，请先保存正在编辑的内容。'),
    notes
      ? h('div', {
          style: 'margin-top: 12px; padding: 10px 12px; white-space: pre-wrap; border-radius: 6px; background: #f6f8fa; color: #475569;',
        }, notes)
      : null,
  ])
}

async function runUpdateFlow(automatic: boolean): Promise<AppUpdateFlowResult> {
  const update = await checkForAppUpdate()
  if (!update) {
    if (!automatic) ElMessage.success('当前已是最新版本')
    return { outcome: 'current', update: null }
  }

  if (automatic && !shouldPromptForAppUpdate(readLastPrompt(), update.version)) {
    return { outcome: 'deferred', update }
  }
  if (automatic) rememberPrompt(update.version)

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

export async function checkAndOfferAppUpdate(options: { automatic?: boolean } = {}) {
  if (updateFlowInFlight) return updateFlowInFlight
  const task = runUpdateFlow(options.automatic === true)
  updateFlowInFlight = task
  try {
    return await task
  } finally {
    if (updateFlowInFlight === task) updateFlowInFlight = null
  }
}
