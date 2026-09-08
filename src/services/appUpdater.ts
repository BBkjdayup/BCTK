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
  phase: 'downloading' | 'installing'
  downloadedBytes: number
  totalBytes: number | null
  percent: number | null
}

let pendingUpdate: Update | null = null
let checkInFlight: Promise<AppUpdateInfo | null> | null = null
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
  pendingUpdate = next
  if (previous && previous !== next) {
    await previous.close().catch(() => undefined)
  }
}

export async function checkForAppUpdate(): Promise<AppUpdateInfo | null> {
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
    progress: { phase: 'installing', downloadedBytes, totalBytes, percent: 100 },
  }
}

export async function installPendingAppUpdate(
  onProgress?: (progress: AppUpdateProgress) => void,
): Promise<void> {
  if (installInProgress) throw new Error('软件更新正在进行，请勿重复操作。')
  const update = pendingUpdate
  if (!update) throw new Error('没有可安装的更新，请重新检查。')

  installInProgress = true
  let downloadedBytes = 0
  let totalBytes: number | null = null
  try {
    await update.downloadAndInstall((event) => {
      const next = progressFromEvent(event, downloadedBytes, totalBytes)
      downloadedBytes = next.downloadedBytes
      totalBytes = next.totalBytes
      onProgress?.(next.progress)
    }, {
      timeout: UPDATE_DOWNLOAD_TIMEOUT_MS,
      restartAfterInstall: true,
    })
    pendingUpdate = null
  } finally {
    installInProgress = false
  }
}
