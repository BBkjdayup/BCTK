import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { DownloadEvent } from '@tauri-apps/plugin-updater'

const mocks = vi.hoisted(() => ({ check: vi.fn() }))

vi.mock('@tauri-apps/plugin-updater', () => ({ check: mocks.check }))

function fakeUpdate() {
  return {
    currentVersion: '0.1.73',
    version: '0.1.74',
    date: '2026-09-05T05:00:00Z',
    body: '修复同步并增加自动更新',
    close: vi.fn().mockResolvedValue(undefined),
    download: vi.fn().mockImplementation(async (onEvent?: (event: DownloadEvent) => void) => {
      onEvent?.({ event: 'Started', data: { contentLength: 100 } })
      onEvent?.({ event: 'Progress', data: { chunkLength: 40 } })
      onEvent?.({ event: 'Progress', data: { chunkLength: 60 } })
      onEvent?.({ event: 'Finished' })
    }),
    install: vi.fn().mockResolvedValue(undefined),
  }
}

describe('appUpdater', () => {
  beforeEach(() => {
    vi.resetModules()
    mocks.check.mockReset()
  })

  it('returns null when the installed version is current', async () => {
    mocks.check.mockResolvedValue(null)
    const { checkForAppUpdate } = await import('./appUpdater')

    await expect(checkForAppUpdate()).resolves.toBeNull()
    expect(mocks.check).toHaveBeenCalledWith({ timeout: 15_000 })
  })

  it('downloads a verified package in the background and installs it later', async () => {
    const update = fakeUpdate()
    mocks.check.mockResolvedValue(update)
    const {
      checkForAppUpdate,
      downloadPendingAppUpdate,
      hasDownloadedAppUpdate,
      installDownloadedAppUpdate,
    } = await import('./appUpdater')

    await expect(checkForAppUpdate()).resolves.toEqual({
      currentVersion: '0.1.73',
      version: '0.1.74',
      date: '2026-09-05T05:00:00Z',
      notes: '修复同步并增加自动更新',
    })
    const progress: Array<{ phase: string, percent: number | null }> = []
    await downloadPendingAppUpdate((event) => progress.push({ phase: event.phase, percent: event.percent }))

    expect(update.download).toHaveBeenCalledWith(expect.any(Function), {
      timeout: 600_000,
    })
    expect(hasDownloadedAppUpdate()).toBe(true)
    await expect(checkForAppUpdate()).resolves.toMatchObject({ version: '0.1.74' })
    expect(mocks.check).toHaveBeenCalledTimes(1)
    await installDownloadedAppUpdate()
    expect(update.install).toHaveBeenCalledWith({ restartAfterInstall: true })
    expect(hasDownloadedAppUpdate()).toBe(false)
    expect(progress).toEqual([
      { phase: 'downloading', percent: 0 },
      { phase: 'downloading', percent: 40 },
      { phase: 'downloading', percent: 100 },
      { phase: 'downloaded', percent: 100 },
    ])
  })

  it('reuses an already downloaded package for a manual immediate install', async () => {
    const update = fakeUpdate()
    mocks.check.mockResolvedValue(update)
    const { checkForAppUpdate, downloadPendingAppUpdate, installPendingAppUpdate } = await import('./appUpdater')

    await checkForAppUpdate()
    await downloadPendingAppUpdate()
    const progress: Array<{ phase: string, percent: number | null }> = []
    await installPendingAppUpdate((event) => progress.push({ phase: event.phase, percent: event.percent }))

    expect(update.download).toHaveBeenCalledTimes(1)
    expect(update.install).toHaveBeenCalledWith({ restartAfterInstall: true })
    expect(progress).toEqual([
      { phase: 'downloaded', percent: 100 },
      { phase: 'installing', percent: 100 },
    ])
  })

  it('clears a failed background download so a later startup can retry', async () => {
    const update = fakeUpdate()
    update.download.mockRejectedValueOnce(new Error('network unavailable'))
    mocks.check.mockResolvedValue(update)
    const { checkForAppUpdate, downloadPendingAppUpdate, hasDownloadedAppUpdate } = await import('./appUpdater')

    await checkForAppUpdate()
    await expect(downloadPendingAppUpdate()).rejects.toThrow('network unavailable')
    expect(hasDownloadedAppUpdate()).toBe(false)
    expect(update.close).toHaveBeenCalledTimes(1)
  })

  it('deduplicates simultaneous update checks', async () => {
    let resolveCheck: (value: null) => void = () => undefined
    mocks.check.mockReturnValue(new Promise<null>((resolve) => { resolveCheck = resolve }))
    const { checkForAppUpdate } = await import('./appUpdater')

    const first = checkForAppUpdate()
    const second = checkForAppUpdate()
    resolveCheck(null)
    await Promise.all([first, second])

    expect(mocks.check).toHaveBeenCalledTimes(1)
  })
})
