import { beforeEach, describe, expect, it, vi } from 'vitest'

const mocks = vi.hoisted(() => ({
  checkForAppUpdate: vi.fn(),
  downloadPendingAppUpdate: vi.fn(),
  hasDownloadedAppUpdate: vi.fn(),
  installDownloadedAppUpdate: vi.fn(),
  installPendingAppUpdate: vi.fn(),
  canCloseApplication: vi.fn(),
  confirm: vi.fn(),
  loadingService: vi.fn(),
  loadingClose: vi.fn(),
  loadingSetText: vi.fn(),
  success: vi.fn(),
}))

vi.mock('../services/appUpdater', () => ({
  checkForAppUpdate: mocks.checkForAppUpdate,
  downloadPendingAppUpdate: mocks.downloadPendingAppUpdate,
  hasDownloadedAppUpdate: mocks.hasDownloadedAppUpdate,
  installDownloadedAppUpdate: mocks.installDownloadedAppUpdate,
  installPendingAppUpdate: mocks.installPendingAppUpdate,
}))

vi.mock('../services/closeProtection', () => ({
  canCloseApplication: mocks.canCloseApplication,
}))

vi.mock('element-plus', () => ({
  ElLoading: { service: mocks.loadingService },
  ElMessage: { success: mocks.success },
  ElMessageBox: { confirm: mocks.confirm },
}))

const availableUpdate = {
  currentVersion: '0.1.87',
  version: '0.1.88',
  date: '2026-09-21T00:00:00Z',
  notes: '修复更新体验',
}

beforeEach(() => {
  vi.resetModules()
  vi.clearAllMocks()
  mocks.checkForAppUpdate.mockResolvedValue(null)
  mocks.downloadPendingAppUpdate.mockResolvedValue(undefined)
  mocks.hasDownloadedAppUpdate.mockReturnValue(false)
  mocks.installDownloadedAppUpdate.mockResolvedValue(undefined)
  mocks.installPendingAppUpdate.mockResolvedValue(undefined)
  mocks.canCloseApplication.mockResolvedValue(true)
  mocks.confirm.mockResolvedValue(undefined)
  mocks.loadingService.mockReturnValue({ close: mocks.loadingClose, setText: mocks.loadingSetText })
})

describe('app update flow', () => {
  it('stages an available update in the background without showing a prompt', async () => {
    mocks.checkForAppUpdate.mockResolvedValue(availableUpdate)
    const { checkAndStageAppUpdate, installStagedAppUpdateOnClose } = await import('./appUpdateFlow')

    await expect(checkAndStageAppUpdate()).resolves.toEqual({ outcome: 'staged', update: availableUpdate })
    expect(mocks.downloadPendingAppUpdate).toHaveBeenCalledTimes(1)
    expect(mocks.confirm).not.toHaveBeenCalled()
    expect(mocks.canCloseApplication).not.toHaveBeenCalled()

    mocks.hasDownloadedAppUpdate.mockReturnValue(true)
    await expect(installStagedAppUpdateOnClose()).resolves.toBe(true)
    expect(mocks.installDownloadedAppUpdate).toHaveBeenCalledTimes(1)
  })

  it('lets normal closing continue when background download failed', async () => {
    mocks.checkForAppUpdate.mockResolvedValue(availableUpdate)
    mocks.downloadPendingAppUpdate.mockRejectedValueOnce(new Error('network unavailable'))
    const { checkAndStageAppUpdate, installStagedAppUpdateOnClose } = await import('./appUpdateFlow')

    await expect(checkAndStageAppUpdate()).resolves.toEqual({ outcome: 'deferred', update: availableUpdate })
    await expect(installStagedAppUpdateOnClose()).resolves.toBe(false)
    expect(mocks.installDownloadedAppUpdate).not.toHaveBeenCalled()
  })

  it('keeps the Settings update action explicit and respects unsaved-work protection', async () => {
    mocks.checkForAppUpdate.mockResolvedValue(availableUpdate)
    mocks.canCloseApplication.mockResolvedValue(false)
    const { checkAndOfferAppUpdate } = await import('./appUpdateFlow')

    await expect(checkAndOfferAppUpdate()).resolves.toEqual({ outcome: 'deferred', update: availableUpdate })
    expect(mocks.confirm).toHaveBeenCalledTimes(1)
    expect(mocks.canCloseApplication).toHaveBeenCalledTimes(1)
    expect(mocks.installPendingAppUpdate).not.toHaveBeenCalled()
  })

  it('honors a manual install requested while background staging is still running', async () => {
    let completeDownload!: () => void
    mocks.checkForAppUpdate.mockResolvedValue(availableUpdate)
    mocks.downloadPendingAppUpdate.mockReturnValue(new Promise<void>((resolve) => {
      completeDownload = resolve
    }))
    const { checkAndStageAppUpdate, checkAndOfferAppUpdate } = await import('./appUpdateFlow')

    const background = checkAndStageAppUpdate()
    await vi.waitFor(() => expect(mocks.downloadPendingAppUpdate).toHaveBeenCalledTimes(1))
    const manual = checkAndOfferAppUpdate()
    expect(checkAndOfferAppUpdate()).toBe(manual)
    expect(mocks.confirm).not.toHaveBeenCalled()

    completeDownload()
    await expect(background).resolves.toEqual({ outcome: 'staged', update: availableUpdate })
    await expect(manual).resolves.toEqual({ outcome: 'installing', update: availableUpdate })
    expect(mocks.confirm).toHaveBeenCalledTimes(1)
    expect(mocks.installPendingAppUpdate).toHaveBeenCalledTimes(1)
  })
})
