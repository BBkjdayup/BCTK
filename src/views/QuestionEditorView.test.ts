import { flushPromises, mount } from '@vue/test-utils'
import { defineComponent, reactive } from 'vue'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import QuestionEditorView from './QuestionEditorView.vue'

const mocks = vi.hoisted(() => ({
  leaveGuard: undefined as undefined | (() => Promise<boolean>),
  confirm: vi.fn().mockResolvedValue(undefined),
  getQuestionDraft: vi.fn().mockResolvedValue(null),
  saveQuestionDraft: vi.fn(),
  deleteQuestionDraft: vi.fn().mockResolvedValue(undefined),
}))

const route = reactive({
  params: {} as Record<string, string>,
  query: {} as Record<string, string>,
})

const appStore = reactive({
  subjects: [
    {
      id: 'subject-1',
      name: '学科一',
      sortOrder: 1024,
      questionCount: 0,
      createdAt: 1,
      updatedAt: 1,
      chapters: [{
        id: 'chapter-1',
        name: '章节一',
        sortOrder: 1024,
        questionCount: 0,
        lastAccessedAt: null,
        createdAt: 1,
        updatedAt: 1,
      }],
    },
    {
      id: 'subject-2',
      name: '学科二',
      sortOrder: 2048,
      questionCount: 0,
      createdAt: 1,
      updatedAt: 1,
      chapters: [{
        id: 'chapter-2',
        name: '章节二',
        sortOrder: 1024,
        questionCount: 0,
        lastAccessedAt: null,
        createdAt: 1,
        updatedAt: 1,
      }],
    },
  ],
  tags: [],
  questionTypes: [{
    code: 'single_choice',
    name: '单选题',
    sortOrder: 1024,
    isBuiltin: true,
    isEnabled: true,
    defaultOptions: [],
  }],
  pendingDraftCount: 0,
  license: {
    capabilities: { canBatchImport: true },
  },
})

const bankStore = reactive({
  filters: {
    subjectId: undefined as string | undefined,
    chapterId: undefined as string | undefined,
    page: 1,
  },
  getById: vi.fn(),
  save: vi.fn(),
})

vi.mock('vue-router', () => ({
  useRoute: () => route,
  useRouter: () => ({ push: vi.fn(), replace: vi.fn() }),
  onBeforeRouteLeave: (guard: () => Promise<boolean>) => {
    mocks.leaveGuard = guard
  },
}))

vi.mock('../stores/app', () => ({
  useAppStore: () => appStore,
}))

vi.mock('../stores/questionBank', () => ({
  useQuestionBankStore: () => bankStore,
}))

vi.mock('../services/backend', () => ({
  backend: {
    getQuestionDraft: mocks.getQuestionDraft,
    saveQuestionDraft: mocks.saveQuestionDraft,
    deleteQuestionDraft: mocks.deleteQuestionDraft,
    checkQuestionDuplicate: vi.fn(),
  },
}))

vi.mock('sortablejs', () => ({
  default: class SortableMock {
    destroy() {}
  },
}))

vi.mock('@tauri-apps/api/window', () => ({
  getCurrentWindow: () => ({
    onCloseRequested: vi.fn(),
    destroy: vi.fn(),
  }),
}))

vi.mock('element-plus', async (importOriginal) => ({
  ...(await importOriginal<typeof import('element-plus')>()),
  ElMessage: { success: vi.fn(), error: vi.fn(), warning: vi.fn(), info: vi.fn() },
  ElMessageBox: { confirm: mocks.confirm },
}))

const RichTextEditorStub = defineComponent({
  name: 'RichTextEditor',
  props: ['modelValue'],
  emits: ['update:modelValue'],
  template: '<div class="rich-text-editor-stub" />',
})

function mountView() {
  return mount(QuestionEditorView, {
    global: {
      stubs: {
        RichTextEditor: RichTextEditorStub,
        QuestionStemSummary: true,
        ElButton: { template: '<button><slot /></button>' },
        ElDialog: { template: '<div><slot /><slot name="footer" /></div>' },
        ElFormItem: { template: '<label><slot /></label>' },
        ElIcon: { template: '<i><slot /></i>' },
        ElOption: true,
        ElSelect: { template: '<div><slot /></div>' },
      },
    },
  })
}

beforeEach(() => {
  mocks.leaveGuard = undefined
  mocks.confirm.mockResolvedValue(undefined)
  mocks.getQuestionDraft.mockResolvedValue(null)
  mocks.saveQuestionDraft.mockResolvedValue({ autosavedAt: 1 })
  appStore.pendingDraftCount = 0
  bankStore.filters.subjectId = undefined
  bankStore.filters.chapterId = undefined
  bankStore.filters.page = 1
  route.params = {}
  route.query = {}
})

afterEach(() => {
  vi.useRealTimers()
  vi.clearAllMocks()
})

describe('QuestionEditorView unsaved-change detection', () => {
  it('waits for an in-flight autosave and then persists edits made during that write', async () => {
    vi.useFakeTimers({ toFake: ['setInterval', 'clearInterval'] })
    let finishFirst!: (value: { autosavedAt: number }) => void
    let finishSecond!: (value: { autosavedAt: number }) => void
    mocks.saveQuestionDraft
      .mockImplementationOnce(() => new Promise((resolve) => { finishFirst = resolve }))
      .mockImplementationOnce(() => new Promise((resolve) => { finishSecond = resolve }))

    const wrapper = mountView()
    await flushPromises()
    const stemEditor = wrapper.findAllComponents(RichTextEditorStub)[0]!
    const editStem = async (text: string) => {
      stemEditor.vm.$emit('update:modelValue', {
        schemaVersion: 2,
        editor: 'tiptap',
        editorVersion: '3.31.0',
        document: {
          type: 'doc',
          content: [{ type: 'paragraph', content: [{ type: 'text', text }] }],
        },
        html: `<p>${text}</p>`,
        plainText: text,
      })
      await wrapper.get('.editor-page').trigger('input')
      await flushPromises()
    }

    await editStem('定时保存开始前的内容')
    await vi.advanceTimersByTimeAsync(60_000)
    expect(mocks.saveQuestionDraft).toHaveBeenCalledOnce()

    await editStem('保存过程中继续输入的最新内容')
    let leaveCompleted = false
    const leaving = mocks.leaveGuard!().then((allowed) => {
      leaveCompleted = true
      return allowed
    })
    await flushPromises()
    expect(leaveCompleted).toBe(false)

    finishFirst({ autosavedAt: 1 })
    await flushPromises()
    expect(mocks.saveQuestionDraft).toHaveBeenCalledTimes(2)
    expect(mocks.saveQuestionDraft.mock.calls[1]?.[0].stem.plainText).toBe('保存过程中继续输入的最新内容')
    expect(leaveCompleted).toBe(false)

    finishSecond({ autosavedAt: 2 })
    await expect(leaving).resolves.toBe(true)
    wrapper.unmount()
  })

  it('does not treat an input event from an otherwise unchanged empty editor as unsaved content', async () => {
    const wrapper = mountView()
    await flushPromises()

    await wrapper.get('.editor-page').trigger('input')
    await flushPromises()

    await expect(mocks.leaveGuard?.()).resolves.toBe(true)
    expect(mocks.confirm).not.toHaveBeenCalled()
    expect(mocks.saveQuestionDraft).not.toHaveBeenCalled()

    wrapper.unmount()
  })

  it('still protects a real user edit after ignoring no-op input events', async () => {
    const wrapper = mountView()
    await flushPromises()

    const stemEditor = wrapper.findAllComponents(RichTextEditorStub)[0]!
    stemEditor.vm.$emit('update:modelValue', {
      schemaVersion: 2,
      editor: 'tiptap',
      editorVersion: '3.28.0',
      document: {
        type: 'doc',
        content: [{ type: 'paragraph', content: [{ type: 'text', text: '真实题干' }] }],
      },
      html: '<p>真实题干</p>',
      plainText: '真实题干',
    })
    await wrapper.get('.editor-page').trigger('input')
    await flushPromises()

    await expect(mocks.leaveGuard?.()).resolves.toBe(true)
    expect(mocks.confirm).toHaveBeenCalledOnce()
    expect(mocks.saveQuestionDraft).toHaveBeenCalledOnce()

    wrapper.unmount()
  })

  it('does not protect classification-only changes while all question content is blank', async () => {
    const wrapper = mountView()
    await flushPromises()

    route.query = { subjectId: 'subject-2', chapterId: 'chapter-2' }
    await flushPromises()

    expect(bankStore.filters.subjectId).toBe('subject-2')
    expect(bankStore.filters.chapterId).toBe('chapter-2')
    await expect(mocks.leaveGuard?.()).resolves.toBe(true)
    expect(mocks.confirm).not.toHaveBeenCalled()
    expect(mocks.saveQuestionDraft).not.toHaveBeenCalled()

    wrapper.unmount()
  })

  it('silently removes an old blank draft even when only classification metadata differs', async () => {
    const emptyContent = () => ({
      schemaVersion: 2,
      editor: 'tiptap',
      editorVersion: '3.28.0',
      document: { type: 'doc', content: [{ type: 'paragraph' }] },
      html: '',
      plainText: '',
    })
    mocks.getQuestionDraft.mockResolvedValueOnce({
      payload: {
        type: 'short_answer',
        stem: emptyContent(),
        options: [],
        answer: emptyContent(),
        explanation: emptyContent(),
        subjectId: 'subject-2',
        chapterId: 'chapter-2',
        tagIds: ['tag-1'],
        resourceRefs: [],
      },
      autosavedAt: 1,
      stale: false,
    })

    const wrapper = mountView()
    await flushPromises()

    expect(mocks.deleteQuestionDraft).toHaveBeenCalledWith(null)
    expect(mocks.confirm).not.toHaveBeenCalled()
    await expect(mocks.leaveGuard?.()).resolves.toBe(true)

    wrapper.unmount()
  })
})
