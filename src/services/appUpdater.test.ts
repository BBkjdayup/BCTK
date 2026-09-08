import { beforeEach, describe, expect, it, vi } from 'vitest'

const mocks = vi.hoisted(() => ({ check: vi.fn() }))

vi.mock('@tauri-apps/plugin-updater', () => ({ check: mocks.check }))

function fakeUpdate() {
  return {
    currentVersion: '0.1.73',
    version: '0.1.74',
    date: '2026-09-05T05:00:00Z',
    body: '修复同步并增加自动更新',
    close: vi.fn().mockResolvedValue(undefined),
    downloadAndInstall: vi.fn().mockImplementation(async (onEvent) => {
      onEvent({ event: 'Started', data: { contentLength: 100 } })
      onEvent({ event: 'Progress', data: { chunkLength: 40 } })
      onEvent({ event: 'Progress', data: { chunkLength: 60 } })
      onEvent({ event: 'Finished' })
    }),
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

  it('keeps update details and reports verified install progress', async () => {
    const update = fakeUpdate()
    mocks.check.mockResolvedValue(update)
    const { checkForAppUpdate, installPendingAppUpdate } = await import('./appUpdater')

    await expect(checkForAppUpdate()).resolves.toEqual({
      currentVersion: '0.1.73',
      version: '0.1.74',
      date: '2026-09-05T05:00:00Z',
      notes: '修复同步并增加自动更新',
    })
    const progress: Array<{ phase: string, percent: number | null }> = []
    await installPendingAppUpdate((event) => progress.push({ phase: event.phase, percent: event.percent }))

    expect(update.downloadAndInstall).toHaveBeenCalledWith(expect.any(Function), {
      timeout: 600_000,
      restartAfterInstall: true,
    })
    expect(progress).toEqual([
      { phase: 'downloading', percent: 0 },
      { phase: 'downloading', percent: 40 },
      { phase: 'downloading', percent: 100 },
      { phase: 'installing', percent: 100 },
    ])
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
