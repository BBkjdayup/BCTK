import { flushPromises, mount } from '@vue/test-utils'
import { defineComponent, nextTick } from 'vue'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { AppSettings, Paper, WordTemplate } from '../types/domain'
import PaperExportDialog from './PaperExportDialog.vue'

const mocks = vi.hoisted(() => ({
  listTemplates: vi.fn(),
  getSettings: vi.fn(),
  routerPush: vi.fn(),
  exportPaperDocx: vi.fn(),
}))

vi.mock('../services/backend', () => ({
  backend: {
    listTemplates: mocks.listTemplates,
    getSettings: mocks.getSettings,
    exportPaperDocx: mocks.exportPaperDocx,
  },
  isDesktopRuntime: () => true,
}))

vi.mock('vue-router', () => ({
  useRouter: () => ({ push: mocks.routerPush }),
}))

vi.mock('element-plus', async (importOriginal) => ({
  ...(await importOriginal<typeof import('element-plus')>()),
  ElMessage: { success: vi.fn(), error: vi.fn(), info: vi.fn() },
}))

const SlotStub = (name: string, tag = 'div') => defineComponent({
  name,
  props: ['modelValue', 'label', 'title', 'disabled'],
  emits: ['update:modelValue', 'click', 'close'],
  template: `<${tag} :disabled="disabled"><slot />{{ label }}{{ title }}<slot name="footer" /></${tag}>`,
})

const template: WordTemplate = {
  id: '018f4b00-0000-7000-8000-000000000001',
  name: '期末试卷模板',
  fileName: '期末试卷模板.docx',
  fileSha256Hex: '00'.repeat(32),
  fileByteSize: 1024,
  analysisStatus: 'ready',
  analysisSchemaVersion: 1,
  parserVersion: 'w1-opc-1',
  rowVersion: 1,
  createdAt: 1,
  updatedAt: 1,
  lastVerifiedAt: 1,
  isDefault: true,
  fileAvailable: true,
  regionConfigured: true,
  packageKind: 'document',
  anchors: [{
    name: 'ZT_QUESTIONS',
    kind: 'content_control',
    partName: 'word/document.xml',
    replacementSpan: { start: 1, end: 2 },
    containerSpan: { start: 0, end: 3 },
    paragraphIndex: null,
  }],
  diagnostics: [],
}

const settings: AppSettings = {
  defaultExportDirectory: '',
  defaultTemplateId: template.id,
  exportFilenamePattern: '{title}_{date}',
  optionalConfirmations: true,
  recentUnusedDays: 90,
  recentAddedDays: 30,
  recentUsedDays: 30,
  recycleRetentionDays: 30,
  recyclePolicy: 'remind_only',
  automaticBackupEnabled: true,
  automaticBackupIntervalDays: 7,
  automaticBackupRetentionCount: 5,
}

const paper: Paper = {
  id: '018f4b00-0000-7000-8000-000000000002',
  title: '期末试卷',
  compositionMode: 'manual',
  generationConfig: null,
  status: 'draft',
  items: [],
  exportContentMode: 'paper_only',
  subjectSummaryText: '',
  preferredTemplateId: null,
  rowVersion: 1,
  createdAt: 1,
  updatedAt: 1,
}

function mountDialog(overrides: Record<string, unknown> = {}) {
  return mount(PaperExportDialog, {
    props: {
      modelValue: true,
      paper,
      preparePaper: vi.fn(),
      ...overrides,
    },
    global: {
      directives: { loading: () => undefined },
      stubs: {
        ElDialog: SlotStub('ElDialog', 'section'),
        ElForm: SlotStub('ElForm', 'form'),
        ElFormItem: SlotStub('ElFormItem', 'section'),
        ElInput: SlotStub('ElInput', 'input'),
        ElSelect: SlotStub('ElSelect'),
        ElOption: SlotStub('ElOption'),
        ElRadioGroup: SlotStub('ElRadioGroup'),
        ElRadio: SlotStub('ElRadio'),
        ElAlert: SlotStub('ElAlert'),
        ElButton: SlotStub('ElButton', 'button'),
        ElTag: SlotStub('ElTag', 'span'),
      },
    },
  })
}

beforeEach(() => {
  vi.clearAllMocks()
  mocks.listTemplates.mockResolvedValue([template])
  mocks.getSettings.mockResolvedValue(settings)
})

describe('PaperExportDialog', () => {
  it('passes export-only choices separately from the persisted paper snapshot', async () => {
    const content = { schemaVersion: 1 as const, html: '<p>内容</p>', plainText: '内容' }
    const stored: Paper = { ...paper, status: 'saved', items: [{
      id: 'item-1', sourceQuestionId: 'question-1', position: 0, snapshot: {
        id: 'question-1', type: 'short_answer', stem: content, options: [], answer: content, explanation: content,
        subjectId: 'subject-1', chapterId: 'chapter-1', subjectName: '学科', chapterName: '章节',
        tags: [], contentVersion: 1, createdAt: 1, updatedAt: 1,
      },
    }] }
    const before = JSON.stringify(stored)
    let finish!: (value: Paper) => void
    const preparePaper = vi.fn(() => new Promise<Paper>((resolve) => { finish = resolve }))
    mocks.exportPaperDocx.mockResolvedValue({ exported: false, diagnostics: [] })
    const wrapper = mountDialog({ paper: stored, preparePaper })
    await flushPromises()
    const vm = wrapper.vm as any
    vm.paperTitle = '本次导出标题'
    vm.outputPath = 'C:\\isolated-test\\export.docx'
    await nextTick()
    const exporting = vm.exportPaper()
    await nextTick()
    expect(preparePaper).toHaveBeenCalledOnce()
    // Edits to the form while preparation is pending must not change this export.
    vm.paperTitle = '后续标题'
    vm.outputPath = 'C:\\isolated-test\\later.docx'
    finish(stored)
    await exporting
    expect(mocks.exportPaperDocx).toHaveBeenCalledWith({
      paperId: stored.id, expectedPaperRowVersion: stored.rowVersion,
      templateId: template.id, outputPath: 'C:\\isolated-test\\export.docx',
      title: '本次导出标题', contentMode: 'paper_only',
    })
    expect(JSON.stringify(stored)).toBe(before)
    wrapper.unmount()
  })

  it('loads and selects templates when initially mounted open', async () => {
    const wrapper = mountDialog()
    await flushPromises()

    expect(mocks.listTemplates).toHaveBeenCalledOnce()
    expect(mocks.getSettings).toHaveBeenCalledOnce()
    expect(wrapper.text()).toContain('期末试卷模板')
    expect(wrapper.text()).not.toContain('还没有模板')
    wrapper.unmount()
  })

  it('does not report an empty template library while it is still loading', async () => {
    let finishLoading!: (value: WordTemplate[]) => void
    mocks.listTemplates.mockReturnValueOnce(new Promise((resolve) => { finishLoading = resolve }))

    const wrapper = mountDialog()
    await nextTick()
    expect(wrapper.text()).not.toContain('还没有模板')

    finishLoading([])
    await flushPromises()
    expect(wrapper.text()).toContain('还没有模板')
    wrapper.unmount()
  })
})
