import { flushPromises, mount } from '@vue/test-utils'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import QuestionBankExportPanel from './QuestionBankExportPanel.vue'

const mocks = vi.hoisted(() => ({
  pickDocxSavePath: vi.fn(), pickXlsxSavePath: vi.fn(), exportQuestionBank: vi.fn(),
  openExportedFile: vi.fn(), revealExportedFile: vi.fn(), confirm: vi.fn(), leave: vi.fn(),
}))
vi.mock('../services/backend', () => ({ backend: mocks, isDesktopRuntime: () => true }))
vi.mock('../stores/app', () => ({ useAppStore: () => ({ databaseHealthy: true }) }))
vi.mock('vue-router', () => ({ onBeforeRouteLeave: mocks.leave }))
vi.mock('element-plus', async (importOriginal) => ({
  ...await importOriginal<typeof import('element-plus')>(),
  ElMessage: { success: vi.fn(), error: vi.fn(), warning: vi.fn() },
  ElMessageBox: { confirm: mocks.confirm },
}))

function render() {
  return mount(QuestionBankExportPanel, { global: { stubs: {
    ElButton: { props: ['disabled'], template: '<button :disabled="disabled"><slot /></button>' },
  } } })
}
beforeEach(() => {
  vi.resetAllMocks()
  mocks.confirm.mockResolvedValue(undefined)
})

describe('question bank export in general settings', () => {
  it.each(['docx', 'xlsx'] as const)('exports %s to the selected file and blocks leaving until completion', async (format) => {
    const path = `C:/test/bank.${format}`
    const picker = format === 'docx' ? mocks.pickDocxSavePath : mocks.pickXlsxSavePath
    picker.mockResolvedValue(path)
    let finish!: (value: unknown) => void
    mocks.exportQuestionBank.mockImplementation(() => new Promise((resolve) => { finish = resolve }))
    const wrapper = render()
    await wrapper.findAll('button')[format === 'docx' ? 0 : 1]!.trigger('click')
    await flushPromises()
    expect(picker).toHaveBeenCalledWith(expect.stringMatching(new RegExp(`^TK试题题库_.*\\.${format}$`)))
    expect(mocks.exportQuestionBank).toHaveBeenCalledWith(format, path)
    const canLeave = mocks.leave.mock.calls[0]![0] as () => boolean
    expect(canLeave()).toBe(false)
    finish({ questionCount: 3, outputBytes: 2048, outputPath: path })
    await flushPromises()
    expect(mocks.openExportedFile).toHaveBeenCalledWith(path)
    expect(canLeave()).toBe(true)
    wrapper.unmount()
  })

  it('does not export when the save picker is cancelled', async () => {
    mocks.pickDocxSavePath.mockResolvedValue(null)
    const wrapper = render()
    await wrapper.findAll('button')[0]!.trigger('click')
    await flushPromises()
    expect(mocks.exportQuestionBank).not.toHaveBeenCalled()
    expect(mocks.openExportedFile).not.toHaveBeenCalled()
    wrapper.unmount()
  })
})
