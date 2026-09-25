import { check, type DownloadEvent, type Update } from '@tauri-apps/plugin-updater'

const UPDATE_CHECK_TIMEOUT_MS = 15_000
const UPDATE_DOWNLOAD_TIMEOUT_MS = 10 * 60 * 1000

export interface AppUpdateInfo {
  currentVersion: string
  version: string
  date: string | null
  notes: string
}
export interface AppUpdateProgress {
  phase: 'downloading' | 'downloaded' | 'installing'
  downloadedBytes: number
  totalBytes: number | null
  percent: number | null
}

let pendingUpdate: Update | null = null
let pendingUpdateDownloaded = false
let pendingDownloadProgress = { downloadedBytes: 0, totalBytes: null as number | null }
let checkInFlight: Promise<AppUpdateInfo | null> | null = null
let downloadInFlight: Promise<void> | null = null
let installInProgress = false

function updateInfo(update: Update): AppUpdateInfo {
  return {
    currentVersion: update.currentVersion,
    version: update.version,
    date: update.date ?? null,
    notes: update.body?.trim() ?? '',
  }
}

async function replacePendingUpdate(next: Update | null) {
  const previous = pendingUpdate
  if (previous === next) return

  pendingUpdate = next
  pendingUpdateDownloaded = false
  pendingDownloadProgress = { downloadedBytes: 0, totalBytes: null }
  if (previous && previous !== next) {
    await previous.close().catch(() => undefined)
  }
}

export async function checkForAppUpdate(): Promise<AppUpdateInfo | null> {
  // A verified package stays associated with its updater resource until a
  // normal close starts installation. A second check must not discard it.
  if (pendingUpdate && (pendingUpdateDownloaded || downloadInFlight || installInProgress)) {
    return updateInfo(pendingUpdate)
  }
  if (checkInFlight) return checkInFlight

  checkInFlight = (async () => {
    const update = await check({ timeout: UPDATE_CHECK_TIMEOUT_MS })
    await replacePendingUpdate(update)
    return update ? updateInfo(update) : null
  })()

  try {
    return await checkInFlight
  } finally {
    checkInFlight = null
  }
}

function progressFromEvent(
  event: DownloadEvent,
  downloadedBytes: number,
  totalBytes: number | null,
): { downloadedBytes: number, totalBytes: number | null, progress: AppUpdateProgress } {
  if (event.event === 'Started') {
    const total = event.data.contentLength ?? null
    return {
      downloadedBytes: 0,
      totalBytes: total,
      progress: { phase: 'downloading', downloadedBytes: 0, totalBytes: total, percent: total ? 0 : null },
    }
  }

  if (event.event === 'Progress') {
    const downloaded = downloadedBytes + event.data.chunkLength
    const percent = totalBytes
      ? Math.min(100, Math.round(downloaded / totalBytes * 100))
      : null
    return {
      downloadedBytes: downloaded,
      totalBytes,
      progress: { phase: 'downloading', downloadedBytes: downloaded, totalBytes, percent },
    }
  }

  return {
    downloadedBytes,
    totalBytes,
    progress: { phase: 'downloaded', downloadedBytes, totalBytes, percent: 100 },
  }
}

export function hasDownloadedAppUpdate(): boolean {
  return pendingUpdate !== null && pendingUpdateDownloaded
}

export async function downloadPendingAppUpdate(
  onProgress?: (progress: AppUpdateProgress) => void,
): Promise<void> {
  if (pendingUpdateDownloaded) {
    onProgress?.({
      phase: 'downloaded',
      downloadedBytes: pendingDownloadProgress.downloadedBytes,
      totalBytes: pendingDownloadProgress.totalBytes,
      percent: 100,
    })
    return
  }
  if (downloadInFlight) return downloadInFlight

  const update = pendingUpdate
  if (!update) throw new Error('没有可下载的更新，请重新检查。')

  const task = (async () => {
    let downloadedBytes = 0
    let totalBytes: number | null = null
    try {
      await update.download((event) => {
        const next = progressFromEvent(event, downloadedBytes, totalBytes)
        downloadedBytes = next.downloadedBytes
        totalBytes = next.totalBytes
        pendingDownloadProgress = { downloadedBytes, totalBytes }
        onProgress?.(next.progress)
      }, { timeout: UPDATE_DOWNLOAD_TIMEOUT_MS })
      if (pendingUpdate !== update) return
      pendingUpdateDownloaded = true
      pendingDownloadProgress = { downloadedBytes, totalBytes }
    } catch (reason) {
      if (pendingUpdate === update) {
        pendingUpdate = null
        pendingUpdateDownloaded = false
        pendingDownloadProgress = { downloadedBytes: 0, totalBytes: null }
        await update.close().catch(() => undefined)
      }
      throw reason
    }
  })()
  downloadInFlight = task
  try {
    await task
  } finally {
    if (downloadInFlight === task) downloadInFlight = null
  }
}

export async function installDownloadedAppUpdate(): Promise<void> {
  if (installInProgress) throw new Error('软件更新正在进行，请勿重复操作。')
  if (downloadInFlight) await downloadInFlight

  const update = pendingUpdate
  if (!update || !pendingUpdateDownloaded) throw new Error('没有已下载的更新，请重新检查。')

  installInProgress = true
  try {
    await update.install({ restartAfterInstall: true })
    pendingUpdate = null
    pendingUpdateDownloaded = false
    pendingDownloadProgress = { downloadedBytes: 0, totalBytes: null }
  } finally {
    installInProgress = false
  }
}

export async function installPendingAppUpdate(
  onProgress?: (progress: AppUpdateProgress) => void,
): Promise<void> {
  await downloadPendingAppUpdate(onProgress)
  onProgress?.({
    phase: 'installing',
    downloadedBytes: pendingDownloadProgress.downloadedBytes,
    totalBytes: pendingDownloadProgress.totalBytes,
    percent: 100,
  })
  await installDownloadedAppUpdate()
}
