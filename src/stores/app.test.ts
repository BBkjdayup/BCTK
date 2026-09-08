import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { backend } from '../services/backend'
import type { BootstrapData } from '../types/domain'
import { useAppStore } from './app'
import { basicLicenseOverview } from '../utils/licensing'

function bootstrap(questionCount: number): BootstrapData {
  return {
    initialized: true,
    dataRoot: 'C:\\TeacherData',
    subjects: [{
      id: 'subject-1',
      name: '计算机网络',
      sortOrder: 1,
      questionCount,
      chapters: [{
        id: 'chapter-1',
        subjectId: 'subject-1',
        name: '第一章',
        sortOrder: 1,
        questionCount,
      }],
    }],
    tags: [],
    questionTypes: [],
    pendingDraftCount: 0,
    databaseHealthy: true,
    appVersion: '0.1.0',
    license: basicLicenseOverview('test-device'),
  }
}

describe('application taxonomy refresh', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
  })

  afterEach(() => {
    vi.restoreAllMocks()
  })

  it('keeps a newer taxonomy snapshot when an older initialization finishes later', async () => {
    let finishOldRequest!: (value: BootstrapData) => void
    let finishNewRequest!: (value: BootstrapData) => void
    vi.spyOn(backend, 'initialize')
      .mockImplementationOnce(() => new Promise((resolve) => { finishOldRequest = resolve }))
      .mockImplementationOnce(() => new Promise((resolve) => { finishNewRequest = resolve }))
    const store = useAppStore()

    const oldRequest = store.initialize()
    const newRequest = store.refreshTaxonomy()
    finishNewRequest(bootstrap(2))
    await newRequest
    finishOldRequest(bootstrap(1))
    await oldRequest

    expect(store.questionCount).toBe(2)
    expect(store.subjects[0]?.chapters[0]?.questionCount).toBe(2)
    expect(store.loading).toBe(false)
  })

  it('preserves the last good snapshot when a background refresh fails', async () => {
    vi.spyOn(backend, 'initialize')
      .mockResolvedValueOnce(bootstrap(3))
      .mockRejectedValueOnce(new Error('temporary failure'))
    const store = useAppStore()
    await store.initialize()

    await expect(store.refreshTaxonomy()).resolves.toBe(false)

    expect(store.questionCount).toBe(3)
    expect(store.databaseHealthy).toBe(true)
    expect(store.error).toBeNull()
  })
})
