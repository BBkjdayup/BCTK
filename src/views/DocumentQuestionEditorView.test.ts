import { flushPromises, mount } from '@vue/test-utils'
import { defineComponent, reactive } from 'vue'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import DocumentQuestionEditorView from './DocumentQuestionEditorView.vue'
import { canCloseApplication } from '../services/closeProtection'
import { plainTextRichContent } from '../utils/richContent'

const mocks = vi.hoisted(() => ({
  leave: undefined as undefined | (() => Promise<boolean>),
  save: vi.fn(), confirm: vi.fn(),
}))
const app = reactive({ subjects: [], questionTypes: [], pendingDraftCount: 0 })
vi.mock('../stores/app', () => ({ useAppStore: () => app }))
vi.mock('../stores/questionBank', () => ({ useQuestionBankStore: () => ({ filters: {} }) }))
vi.mock('vue-router', () => ({
  useRouter: () => ({ push: vi.fn() }),
  onBeforeRouteLeave: (guard: () => Promise<boolean>) => { mocks.leave = guard },
}))
vi.mock('../services/backend', () => ({ backend: {
  listDocumentEntryTemplates: vi.fn().mockResolvedValue([]),
  getDocumentQuestionDraft: vi.fn().mockResolvedValue(null),
  saveDocumentQuestionDraft: mocks.save,
} }))
vi.mock('element-plus', async original => ({
  ...await original<typeof import('element-plus')>(),
  ElMessage: { success: vi.fn(), error: vi.fn(), warning: vi.fn() },
  ElMessageBox: { confirm: mocks.confirm },
}))
const Editor = defineComponent({ props: ['modelValue'], emits: ['update:modelValue'], template: '<div />' })
function mountView() {
  return mount(DocumentQuestionEditorView, { global: { stubs: {
    RichTextEditor: Editor, DocumentEntryTemplateDialog: true,
    ElButton: { template: '<button><slot /></button>' }, ElIcon: true,
    ElSelect: true, ElOption: true, ElTooltip: { template: '<div><slot /></div>' },
    ElInput: true, ElInputNumber: true,
  } } })
}
beforeEach(() => {
  vi.resetAllMocks()
  mocks.confirm.mockResolvedValue(undefined)
  mocks.save.mockResolvedValue({ autosavedAt: 1 })
  app.pendingDraftCount = 0
})
afterEach(() => vi.useRealTimers())

it('keeps navigation and native close pending until edits made during autosave are stored', async () => {
  vi.useFakeTimers({ toFake: ['setInterval', 'clearInterval'] })
  let first!: (value: unknown) => void, second!: (value: unknown) => void
  mocks.save.mockImplementationOnce(() => new Promise(resolve => { first = resolve }))
    .mockImplementationOnce(() => new Promise(resolve => { second = resolve }))
  const wrapper = mountView()
  try {
    await flushPromises()
    const editor = wrapper.getComponent(Editor)
    editor.vm.$emit('update:modelValue', plainTextRichContent('A'))
    await vi.advanceTimersByTimeAsync(60_000)
    editor.vm.$emit('update:modelValue', plainTextRichContent('B'))
    let navigated = false, closed = false
    const leaving = mocks.leave!().then(ok => { navigated = ok })
    const closing = canCloseApplication().then(ok => { closed = ok })
    await flushPromises()
    expect(navigated || closed).toBe(false)
    first({ autosavedAt: 1 })
    await flushPromises()
    expect(mocks.save).toHaveBeenCalledTimes(2)
    expect(mocks.save.mock.calls[1]![0].source.plainText).toBe('B')
    expect(navigated || closed).toBe(false)
    second({ autosavedAt: 2 })
    await Promise.all([leaving, closing])
    expect(navigated && closed).toBe(true)
    expect(app.pendingDraftCount).toBe(1)
    await vi.advanceTimersByTimeAsync(60_000)
    expect(mocks.save).toHaveBeenCalledTimes(2)
  } finally { wrapper.unmount() }
})

it('retains unsaved input and blocks leaving after persistence fails', async () => {
  const wrapper = mountView()
  try {
    await flushPromises()
    wrapper.getComponent(Editor).vm.$emit('update:modelValue', plainTextRichContent('未保存'))
    mocks.save.mockRejectedValueOnce(new Error('disk full'))
    expect(await mocks.leave!()).toBe(false)
    const event = new Event('beforeunload', { cancelable: true })
    window.dispatchEvent(event)
    expect(event.defaultPrevented).toBe(true)
    expect(await mocks.leave!()).toBe(true)
    expect(mocks.save.mock.calls[1]![0].source.plainText).toBe('未保存')
  } finally { wrapper.unmount() }
})
