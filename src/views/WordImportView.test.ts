import { flushPromises, shallowMount } from '@vue/test-utils'
import { nextTick, reactive } from 'vue'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import WordImportView from './WordImportView.vue'
import { canCloseApplication } from '../services/closeProtection'
import { plainTextRichContent } from '../utils/richContent'
import type { RichContent } from '../types/domain'

const mocks = vi.hoisted(() => ({ leave: undefined as undefined | (() => Promise<boolean>), save: vi.fn(), remove: vi.fn(), confirm: vi.fn() }))
const app = reactive({ subjects: [], tags: [], questionTypes: [], pendingDraftCount: 0 })
vi.mock('../stores/app', () => ({ useAppStore: () => app }))
vi.mock('../stores/questionBank', () => ({ useQuestionBankStore: () => ({ filters: {} }) }))
vi.mock('vue-router', () => ({ useRoute: () => ({ query: {} }), onBeforeRouteLeave: (guard: () => Promise<boolean>) => { mocks.leave = guard } }))
vi.mock('../services/backend', () => ({ isDesktopRuntime: () => true, backend: {
  getWordImportDraft: vi.fn().mockResolvedValue(null), saveWordImportDraft: mocks.save, deleteWordImportDraft: mocks.remove,
} }))
vi.mock('element-plus', async original => ({ ...await original<typeof import('element-plus')>(),
  ElMessage: { success: vi.fn(), error: vi.fn(), warning: vi.fn(), info: vi.fn() },
  ElMessageBox: { confirm: mocks.confirm, close: vi.fn() },
}))
async function review() {
  const wrapper = shallowMount(WordImportView)
  await flushPromises()
  // Seed the state after recognition; exercise the real persistence and both guards.
  const state = wrapper.vm as unknown as {
    step: number; selectedFile: unknown; draftReady: boolean; draftDirty: boolean; draftAutosaving: boolean
    items: Array<{ stem: RichContent }>; activeItemId: string
    autosaveWordImportDraft: (show?: boolean, force?: boolean) => Promise<boolean>
    deletePersistedWordImportDraft: () => Promise<void>
  }
  Object.assign(state, { step: 2, selectedFile: { name: 'test.docx', size: 1, kind: 'docx' }, draftReady: true,
    activeItemId: 'one', items: [{ id: 'one', ordinal: 1, selected: true, type: 'essay', stem: plainTextRichContent('A'),
      options: [], optionIds: [], answer: plainTextRichContent(''), explanation: plainTextRichContent(''),
      subjectId: '', chapterId: '', tagIds: [], resourceRefs: [], status: 'ok', duplicateKind: 'none',
      duplicateCandidate: null, duplicateChecking: false, duplicateNeedsCheck: false }] })
  await nextTick()
  return { wrapper, state }
}
beforeEach(() => {
  mocks.save.mockReset().mockResolvedValue({ autosavedAt: 1 })
  mocks.remove.mockReset().mockResolvedValue(undefined)
  mocks.confirm.mockReset().mockResolvedValue(undefined)
  app.pendingDraftCount = 0
})
afterEach(() => vi.useRealTimers())

it('shares pending autosave and holds navigation and native close until the newest edit is persisted', async () => {
  let first!: (value: unknown) => void, second!: (value: unknown) => void
  mocks.save.mockImplementationOnce(() => new Promise(resolve => { first = resolve }))
    .mockImplementationOnce(() => new Promise(resolve => { second = resolve }))
  const { wrapper, state } = await review()
  try {
    const autosave = state.autosaveWordImportDraft()
    let navigated = false, closed = false
    const leaving = mocks.leave!().then(ok => { navigated = ok })
    const closing = canCloseApplication().then(ok => { closed = ok })
    state.items[0]!.stem = plainTextRichContent('B')
    await nextTick()
    first({ autosavedAt: 1 }); await flushPromises()
    expect(mocks.save).toHaveBeenCalledTimes(2)
    expect(mocks.save.mock.calls[1]![0].items[0].payload.stem.plainText).toBe('B')
    expect(navigated || closed).toBe(false)
    second({ autosavedAt: 2 }); await Promise.all([autosave, leaving, closing])
    expect(navigated && closed).toBe(true)
    expect(state.draftDirty).toBe(false)
    expect(app.pendingDraftCount).toBe(1)
  } finally { wrapper.unmount() }
})

it('retains edits and blocks both guards after persistence fails, then allows a successful retry', async () => {
  const { wrapper, state } = await review()
  try {
    mocks.save.mockRejectedValueOnce(new Error('disk full'))
    expect(await mocks.leave!()).toBe(false)
    expect(state.draftDirty).toBe(true)
    mocks.save.mockRejectedValueOnce(new Error('disk full'))
    expect(await canCloseApplication()).toBe(false)
    expect(await mocks.leave!()).toBe(true)
  } finally { wrapper.unmount() }
})

it('a caller timeout cannot start overlapping writes or delete before a late save finishes', async () => {
  const { wrapper, state } = await review()
  vi.useFakeTimers()
  let finish!: (value: unknown) => void
  mocks.save.mockImplementationOnce(() => new Promise(resolve => { finish = resolve }))
  try {
    const leaving = mocks.leave!()
    await vi.advanceTimersByTimeAsync(60_001)
    expect(await leaving).toBe(false)
    expect(state.draftAutosaving).toBe(true)
    state.items[0]!.stem = plainTextRichContent('B')
    const retry = state.autosaveWordImportDraft()
    const deleting = state.deletePersistedWordImportDraft()
    expect(mocks.save).toHaveBeenCalledTimes(1)
    expect(mocks.remove).not.toHaveBeenCalled()
    finish({ autosavedAt: 1 })
    await Promise.all([retry, deleting])
    expect(mocks.save).toHaveBeenCalledTimes(2)
    expect(mocks.save.mock.calls[1]![0].items[0].payload.stem.plainText).toBe('B')
    expect(mocks.remove).toHaveBeenCalledTimes(1)
  } finally { wrapper.unmount() }
})
