import { enableAutoUnmount, flushPromises, mount } from '@vue/test-utils'
import { defineComponent, nextTick } from 'vue'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { Question, QuestionTypeDefinition, Subject, Tag } from '../types/domain'
import RandomDrawDialog from './RandomDrawDialog.vue'

const mocks = vi.hoisted(() => ({
  analyzeRandomDraw: vi.fn(),
  drawRandomQuestions: vi.fn(),
  success: vi.fn(),
  error: vi.fn(),
  warning: vi.fn(),
  confirm: vi.fn().mockResolvedValue('confirm'),
}))

vi.mock('../services/backend', () => ({
  backend: {
    analyzeRandomDraw: mocks.analyzeRandomDraw,
    drawRandomQuestions: mocks.drawRandomQuestions,
  },
}))

vi.mock('element-plus', async (importOriginal) => ({
  ...(await importOriginal<typeof import('element-plus')>()),
  ElMessage: { success: mocks.success, error: mocks.error, warning: mocks.warning },
  ElMessageBox: { confirm: mocks.confirm },
}))

const ModelStub = (name: string, tag = 'div') => defineComponent({
  name,
  props: ['modelValue', 'disabled', 'max', 'multiple', 'loading'],
  emits: ['update:modelValue', 'change', 'click'],
  template: `<${tag} :disabled="disabled || loading" @click="$emit('click')"><slot /></${tag}>`,
})

const subjects: Subject[] = [
  {
    id: '018f4b00-0000-7000-8000-000000000001',
    name: '网设',
    sortOrder: 1,
    questionCount: 2,
    chapters: [{
      id: '018f4b00-0000-7000-8000-000000000002',
      subjectId: '018f4b00-0000-7000-8000-000000000001',
      name: '第一章',
      sortOrder: 1,
      questionCount: 2,
      units: [{ id: '018f4b00-0000-7000-8000-000000000006', chapterId: '018f4b00-0000-7000-8000-000000000002', name: '基础单元', sortOrder: 1, questionCount: 1 }],
    }],
  },
  {
    id: '018f4b00-0000-7000-8000-000000000004',
    name: '软考',
    sortOrder: 2,
    questionCount: 1,
    chapters: [{
      id: '018f4b00-0000-7000-8000-000000000005',
      subjectId: '018f4b00-0000-7000-8000-000000000004',
      name: '第一章',
      sortOrder: 1,
      questionCount: 1,
    }],
  },
]
const tags: Tag[] = [{
  id: '018f4b00-0000-7000-8000-000000000003',
  name: '重点',
  questionCount: 2,
  createdAt: 1,
  updatedAt: 1,
}]
const questionTypes: QuestionTypeDefinition[] = [{
  code: 'single_choice',
  name: '单选题',
  behavior: 'single_choice',
  aliases: [],
  defaultOptions: ['A', 'B', 'C', 'D'],
  isBuiltin: true,
  isEnabled: true,
  sortOrder: 1,
  questionCount: 2,
  paperItemCount: 0,
  createdAt: 1,
  updatedAt: 1,
}]

function question(id: string): Question {
  const content = { schemaVersion: 1 as const, html: '<p>题目</p>', plainText: '题目' }
  return {
    id,
    type: 'single_choice',
    stem: content,
    options: [],
    answer: content,
    explanation: { schemaVersion: 1, html: '', plainText: '' },
    subjectId: subjects[0]!.id,
    chapterId: subjects[0]!.chapters[0]!.id,
    subjectName: '网设',
    chapterName: '第一章',
    tags,
    createdAt: 1,
    updatedAt: 1,
    contentVersion: 1,
  }
}

function mountDialog(excludedQuestionIds = ['018f4b00-0000-7000-8000-000000000010'], realDialog = false) {
  return mount(RandomDrawDialog, {
    props: {
      modelValue: true,
      paperId: 'paper-original',
      subjects,
      tags,
      questionTypes,
      excludedQuestionIds,
    },
    global: {
      stubs: {
        ElDialog: realDialog ? false : {
          name: 'ElDialog', props: ['modelValue'], emits: ['update:modelValue'],
          template: '<section><slot /><slot name="footer" /></section>',
        },
        ElFormItem: { template: '<label><slot /></label>' },
        ElSelect: ModelStub('ElSelect'),
        ElOption: true,
        ElRadioGroup: ModelStub('ElRadioGroup'),
        ElRadioButton: { template: '<span><slot /></span>' },
        ElInputNumber: ModelStub('ElInputNumber'),
        ElTag: { template: '<span><slot /></span>' },
        ElAlert: { props: ['title'], template: '<div>{{ title }}</div>' },
        ElButton: ModelStub('ElButton', 'button'),
      },
    },
  })
}

enableAutoUnmount(afterEach)

beforeEach(() => {
  vi.resetAllMocks()
  vi.useFakeTimers()
  mocks.confirm.mockResolvedValue('confirm')
  mocks.analyzeRandomDraw.mockResolvedValue({
    availableTotal: 2,
    availableByType: { single_choice: 2 },
  })
  mocks.drawRandomQuestions.mockResolvedValue([
    question('018f4b00-0000-7000-8000-000000000011'),
    question('018f4b00-0000-7000-8000-000000000012'),
  ])
})

function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (reason: Error) => void
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no })
  return { promise, resolve, reject }
}

function drawButton(wrapper: ReturnType<typeof mountDialog>) {
  return wrapper.findAllComponents({ name: 'ElButton' })
    .find((button) => button.text().includes('随机抽取并加入'))!
}

async function readyToDraw() {
  const wrapper = mountDialog([])
  await vi.advanceTimersByTimeAsync(200)
  await flushPromises()
  wrapper.findComponent({ name: 'ElInputNumber' }).vm.$emit('update:modelValue', 2)
  await nextTick()
  return wrapper
}

describe('random draw request lifetime', () => {
  it.each(['cancel', 'dialog-close', 'external-close'] as const)('ignores a draw after %s', async (close) => {
    const pending = deferred<Question[]>()
    mocks.drawRandomQuestions.mockReturnValueOnce(pending.promise)
    const wrapper = await readyToDraw()
    await drawButton(wrapper).trigger('click')
    if (close === 'cancel') {
      await wrapper.findAll('button').find((button) => button.text() === '取消')!.trigger('click')
    } else if (close === 'dialog-close') {
      wrapper.findComponent({ name: 'ElDialog' }).vm.$emit('update:modelValue', false)
    } else {
      await wrapper.setProps({ modelValue: false })
    }
    // Cancel/close must take effect even before the parent passes updated props down.
    pending.resolve([question('old-1'), question('old-2')])
    await flushPromises()
    expect(wrapper.emitted('add')).toBeUndefined()
    expect(mocks.success).not.toHaveBeenCalled()
    expect(mocks.error).not.toHaveBeenCalled()
    await vi.advanceTimersByTimeAsync(200)
    expect(mocks.analyzeRandomDraw).toHaveBeenCalledTimes(1)
  })

  it('cancels from the real dialog close button before its leave animation completes', async () => {
    const pending = deferred<Question[]>()
    mocks.drawRandomQuestions.mockReturnValueOnce(pending.promise)
    const wrapper = mountDialog([], true)
    await vi.advanceTimersByTimeAsync(200)
    await flushPromises()
    wrapper.findComponent({ name: 'ElInputNumber' }).vm.$emit('update:modelValue', 2)
    await nextTick()
    await drawButton(wrapper).trigger('click')
    expect(mocks.drawRandomQuestions).toHaveBeenCalledTimes(1)
    await wrapper.get('.el-dialog__headerbtn').trigger('click')
    // The parent has not changed modelValue and the transition has not emitted closed.
    expect(wrapper.props('modelValue')).toBe(true)
    expect(wrapper.emitted('update:modelValue')?.[0]).toEqual([false])
    pending.resolve([question('old-1'), question('old-2')])
    await flushPromises()
    expect(wrapper.emitted('add')).toBeUndefined()
    expect(mocks.confirm).not.toHaveBeenCalled()
  })

  it.each(['resolve', 'reject'] as const)('keeps a reopened draw busy when the cancelled request later %ss', async (outcome) => {
    const old = deferred<Question[]>(), current = deferred<Question[]>()
    mocks.drawRandomQuestions.mockReturnValueOnce(old.promise).mockReturnValueOnce(current.promise)
    const wrapper = await readyToDraw()
    await drawButton(wrapper).trigger('click')
    await wrapper.setProps({ modelValue: false })
    await wrapper.setProps({ modelValue: true })
    await vi.advanceTimersByTimeAsync(200)
    await flushPromises()
    await drawButton(wrapper).trigger('click')
    expect(mocks.drawRandomQuestions).toHaveBeenCalledTimes(2)
    if (outcome === 'resolve') old.resolve([question('old-1'), question('old-2')])
    else old.reject(new Error('cancelled request failed'))
    await flushPromises()
    expect(wrapper.emitted('add')).toBeUndefined()
    expect(mocks.error).not.toHaveBeenCalled()
    expect(drawButton(wrapper).props('loading')).toBe(true)
    current.resolve([question('current-1'), question('current-2')])
    await flushPromises()
    expect(wrapper.emitted('add')).toEqual([[
      [expect.objectContaining({ id: 'current-1' }), expect.objectContaining({ id: 'current-2' })],
      'paper-original',
    ]])
  })

  it('ignores a draw after its target paper changes', async () => {
    const pending = deferred<Question[]>()
    mocks.drawRandomQuestions.mockReturnValueOnce(pending.promise)
    const wrapper = await readyToDraw()
    await drawButton(wrapper).trigger('click')
    await wrapper.setProps({ paperId: 'paper-next' })
    pending.resolve([question('old-1'), question('old-2')])
    await flushPromises()
    expect(wrapper.emitted('add')).toBeUndefined()
    expect(mocks.success).not.toHaveBeenCalled()
  })

  it.each(['close', 'switch-paper'] as const)('revalidates the request after a shortage confirmation and %s', async (action) => {
    const confirmation = deferred<string>()
    mocks.confirm.mockReturnValueOnce(confirmation.promise)
    mocks.drawRandomQuestions.mockResolvedValueOnce([question('only-one')])
    const wrapper = await readyToDraw()
    await drawButton(wrapper).trigger('click')
    await flushPromises()
    expect(mocks.confirm).toHaveBeenCalledTimes(1)
    await wrapper.setProps(action === 'close' ? { modelValue: false } : { paperId: 'paper-next' })
    confirmation.resolve('confirm')
    await flushPromises()
    expect(wrapper.emitted('add')).toBeUndefined()
  })

  it('does not show notifications after unmounting an unfinished draw', async () => {
    const pending = deferred<Question[]>()
    mocks.drawRandomQuestions.mockReturnValueOnce(pending.promise)
    const wrapper = await readyToDraw()
    await drawButton(wrapper).trigger('click')
    wrapper.unmount()
    pending.reject(new Error('late error'))
    await flushPromises()
    expect(mocks.error).not.toHaveBeenCalled()
    expect(mocks.success).not.toHaveBeenCalled()
  })

  it.each(['success', 'failure'] as const)('keeps drawing locked after an overlapping analysis %s', async (outcome) => {
    const pending = deferred<Question[]>()
    mocks.drawRandomQuestions.mockReturnValueOnce(pending.promise)
    const wrapper = await readyToDraw()
    if (outcome === 'failure') mocks.analyzeRandomDraw.mockRejectedValueOnce(new Error('analysis failed'))
    const unit = wrapper.findAllComponents({ name: 'ElSelect' })[2]!
    unit.vm.$emit('update:modelValue', [subjects[0]!.chapters[0]!.units![0]!.id])
    unit.vm.$emit('change')
    await nextTick()
    await drawButton(wrapper).trigger('click')
    await vi.advanceTimersByTimeAsync(200)
    await flushPromises()
    expect(mocks.analyzeRandomDraw).toHaveBeenCalledTimes(2)
    expect(drawButton(wrapper).props('loading')).toBe(true)
    expect(unit.props('disabled')).toBe(true)
    expect(wrapper.findComponent({ name: 'ElInputNumber' }).props('disabled')).toBe(true)
    // Exercise the handler guard as well as the rendered button state.
    drawButton(wrapper).vm.$emit('click')
    await flushPromises()
    expect(mocks.drawRandomQuestions).toHaveBeenCalledTimes(1)
    pending.resolve([question('draw-1'), question('draw-2')])
    await flushPromises()
    expect(wrapper.emitted('add')).toHaveLength(1)
    expect(unit.props('disabled')).toBe(false)
    expect(wrapper.findComponent({ name: 'ElInputNumber' }).props('disabled')).toBe(false)
  })

  it('keeps drawing locked until an insufficient-count confirmation completes', async () => {
    const confirmation = deferred<string>()
    mocks.confirm.mockReturnValueOnce(confirmation.promise)
    mocks.drawRandomQuestions.mockResolvedValueOnce([question('only-one')])
    const wrapper = await readyToDraw()
    await drawButton(wrapper).trigger('click')
    await flushPromises()
    expect(drawButton(wrapper).props('loading')).toBe(true)
    drawButton(wrapper).vm.$emit('click')
    await flushPromises()
    expect(mocks.drawRandomQuestions).toHaveBeenCalledTimes(1)
    confirmation.resolve('confirm')
    await flushPromises()
    expect(wrapper.emitted('add')).toEqual([[[expect.objectContaining({ id: 'only-one' })], 'paper-original']])
  })

  it('allows retry after a draw fails or a shortage is declined', async () => {
    mocks.drawRandomQuestions
      .mockRejectedValueOnce(new Error('draw failed'))
      .mockResolvedValueOnce([question('only-one')])
    mocks.confirm.mockRejectedValueOnce('cancel')
    const wrapper = await readyToDraw()
    await drawButton(wrapper).trigger('click')
    await flushPromises()
    expect(mocks.error).toHaveBeenCalledWith('draw failed')
    expect(drawButton(wrapper).props('loading')).toBe(false)
    await drawButton(wrapper).trigger('click')
    await flushPromises()
    expect(wrapper.emitted('add')).toBeUndefined()
    expect(drawButton(wrapper).props('loading')).toBe(false)
    await drawButton(wrapper).trigger('click')
    await flushPromises()
    expect(mocks.drawRandomQuestions).toHaveBeenCalledTimes(3)
    expect(wrapper.emitted('add')).toHaveLength(1)
  })

  it('refreshes availability and releases drawing when no questions remain', async () => {
    mocks.drawRandomQuestions.mockResolvedValueOnce([])
    const wrapper = await readyToDraw()
    await drawButton(wrapper).trigger('click')
    await flushPromises()
    expect(wrapper.emitted('add')).toBeUndefined()
    expect(mocks.warning).toHaveBeenCalledWith('当前候选题库没有可加入的新题。')
    expect(mocks.analyzeRandomDraw).toHaveBeenCalledTimes(2)
    expect(drawButton(wrapper).props('loading')).toBe(false)
  })

  it('cancels a scheduled analysis when closed before the debounce completes', async () => {
    const wrapper = mountDialog([])
    await wrapper.setProps({ modelValue: false })
    await vi.advanceTimersByTimeAsync(200)
    expect(mocks.analyzeRandomDraw).not.toHaveBeenCalled()
  })

  it('does not display an analysis error from a previous open session', async () => {
    const old = deferred<{ availableTotal: number; availableByType: Record<string, number> }>()
    mocks.analyzeRandomDraw.mockReturnValueOnce(old.promise)
    const wrapper = mountDialog([])
    await vi.advanceTimersByTimeAsync(200)
    await wrapper.setProps({ modelValue: false })
    await wrapper.setProps({ modelValue: true })
    old.reject(new Error('old analysis failed'))
    await flushPromises()
    expect(wrapper.text()).not.toContain('old analysis failed')
    await vi.advanceTimersByTimeAsync(200)
    await flushPromises()
    expect(wrapper.text()).toContain('2 道')
  })
})

afterEach(() => {
  vi.useRealTimers()
  vi.clearAllMocks()
})

describe('RandomDrawDialog', () => {
  it('analyzes the open scope and refreshes after selected questions change', async () => {
    const wrapper = mountDialog()
    await vi.advanceTimersByTimeAsync(200)
    await flushPromises()

    expect(mocks.analyzeRandomDraw).toHaveBeenLastCalledWith(expect.objectContaining({
      excludedQuestionIds: ['018f4b00-0000-7000-8000-000000000010'],
      subjectIds: [],
      tagIds: [],
      chapterIds: [],
      usage: 'all',
    }))
    expect(wrapper.text()).toContain('2 道')

    await wrapper.setProps({ excludedQuestionIds: ['018f4b00-0000-7000-8000-000000000011'] })
    await vi.advanceTimersByTimeAsync(200)
    await flushPromises()
    expect(mocks.analyzeRandomDraw).toHaveBeenLastCalledWith(expect.objectContaining({
      excludedQuestionIds: ['018f4b00-0000-7000-8000-000000000011'],
    }))
  })

  it('draws by total count and emits only the backend-selected questions', async () => {
    const wrapper = mountDialog([])
    await vi.advanceTimersByTimeAsync(200)
    await flushPromises()

    const countMode = wrapper.findAllComponents({ name: 'ElRadioGroup' })[1]!
    countMode.vm.$emit('update:modelValue', 'total')
    await nextTick()
    const numberInput = wrapper.findComponent({ name: 'ElInputNumber' })
    numberInput.vm.$emit('update:modelValue', 2)
    await nextTick()

    const drawButton = wrapper.findAll('button').find((button) => button.text().includes('随机抽取并加入'))!
    await drawButton.trigger('click')
    await flushPromises()

    expect(mocks.drawRandomQuestions).toHaveBeenCalledWith(expect.objectContaining({
      countMode: 'total',
      totalCount: 2,
      questionTypeCounts: {},
    }))
    expect(wrapper.emitted('add')?.[0]?.[0]).toHaveLength(2)
  })

  it('keeps a 200-question request instead of silently clamping it to 100', async () => {
    const wrapper = mountDialog([])
    await vi.advanceTimersByTimeAsync(200)
    await flushPromises()

    const numberInput = wrapper.findComponent({ name: 'ElInputNumber' })
    expect(numberInput.props('max')).toBe(1_000)
    numberInput.vm.$emit('update:modelValue', 200)
    await nextTick()

    expect(wrapper.text()).toContain('随机抽取并加入 200 道')
  })

  it('analyzes multiple selected subjects and drops chapters outside the new subject set', async () => {
    const wrapper = mountDialog([])
    await vi.advanceTimersByTimeAsync(200)
    await flushPromises()
    const selects = wrapper.findAllComponents({ name: 'ElSelect' })
    const subjectSelect = selects[0]!
    const chapterSelect = selects[1]!
    const unitSelect = selects[2]!

    subjectSelect.vm.$emit('update:modelValue', [subjects[0]!.id, subjects[1]!.id])
    subjectSelect.vm.$emit('change')
    chapterSelect.vm.$emit('update:modelValue', [subjects[0]!.chapters[0]!.id])
    chapterSelect.vm.$emit('change')
    await nextTick()
    unitSelect.vm.$emit('update:modelValue', [subjects[0]!.chapters[0]!.units![0]!.id])
    unitSelect.vm.$emit('change')
    await nextTick()
    await vi.advanceTimersByTimeAsync(200)
    await flushPromises()

    expect(mocks.analyzeRandomDraw).toHaveBeenLastCalledWith(expect.objectContaining({
      subjectIds: [subjects[0]!.id, subjects[1]!.id],
      chapterIds: [subjects[0]!.chapters[0]!.id],
      unitIds: [subjects[0]!.chapters[0]!.units![0]!.id],
    }))

    subjectSelect.vm.$emit('update:modelValue', [subjects[1]!.id])
    subjectSelect.vm.$emit('change')
    await nextTick()
    await vi.advanceTimersByTimeAsync(200)
    await flushPromises()

    expect(mocks.analyzeRandomDraw).toHaveBeenLastCalledWith(expect.objectContaining({
      subjectIds: [subjects[1]!.id],
      chapterIds: [],
      unitIds: [],
    }))
  })

  it('keeps the selected scope and draw settings when hidden and reopened', async () => {
    const wrapper = mountDialog([])
    await vi.advanceTimersByTimeAsync(200)
    await flushPromises()
    const selects = wrapper.findAllComponents({ name: 'ElSelect' })
    const radioGroups = wrapper.findAllComponents({ name: 'ElRadioGroup' })

    selects[0]!.vm.$emit('update:modelValue', [subjects[0]!.id])
    selects[0]!.vm.$emit('change')
    selects[1]!.vm.$emit('update:modelValue', [subjects[0]!.chapters[0]!.id])
    selects[1]!.vm.$emit('change')
    selects[3]!.vm.$emit('update:modelValue', [tags[0]!.id])
    selects[3]!.vm.$emit('change')
    selects[4]!.vm.$emit('update:modelValue', 'unused_this_week')
    selects[4]!.vm.$emit('change')
    radioGroups[1]!.vm.$emit('update:modelValue', 'total')
    await nextTick()
    wrapper.findComponent({ name: 'ElInputNumber' }).vm.$emit('update:modelValue', 7)
    await nextTick()

    await wrapper.setProps({ modelValue: false })
    await wrapper.setProps({
      modelValue: true,
      excludedQuestionIds: ['018f4b00-0000-7000-8000-000000000012'],
    })
    await vi.advanceTimersByTimeAsync(200)
    await flushPromises()

    expect(mocks.analyzeRandomDraw).toHaveBeenLastCalledWith(expect.objectContaining({
      subjectIds: [subjects[0]!.id],
      chapterIds: [subjects[0]!.chapters[0]!.id],
      tagIds: [tags[0]!.id],
      usage: 'unused_this_week',
      excludedQuestionIds: ['018f4b00-0000-7000-8000-000000000012'],
    }))
    expect(wrapper.text()).toContain('随机抽取并加入 7 道')
  })

  it('resets the retained scope and draw settings on request', async () => {
    const wrapper = mountDialog([])
    await vi.advanceTimersByTimeAsync(200)
    await flushPromises()
    const selects = wrapper.findAllComponents({ name: 'ElSelect' })
    const radioGroups = wrapper.findAllComponents({ name: 'ElRadioGroup' })

    selects[0]!.vm.$emit('update:modelValue', [subjects[0]!.id])
    selects[0]!.vm.$emit('change')
    selects[3]!.vm.$emit('update:modelValue', [tags[0]!.id])
    selects[3]!.vm.$emit('change')
    selects[4]!.vm.$emit('update:modelValue', 'unused_today')
    selects[4]!.vm.$emit('change')
    radioGroups[1]!.vm.$emit('update:modelValue', 'total')
    await nextTick()
    wrapper.findComponent({ name: 'ElInputNumber' }).vm.$emit('update:modelValue', 7)
    await nextTick()

    const resetButton = wrapper.findAll('button').find((button) => button.text() === '重置条件')!
    await resetButton.trigger('click')
    await vi.advanceTimersByTimeAsync(200)
    await flushPromises()

    expect(mocks.analyzeRandomDraw).toHaveBeenLastCalledWith(expect.objectContaining({
      subjectIds: [],
      chapterIds: [],
      tagIds: [],
      tagMatchMode: 'any',
      usage: 'all',
    }))
    expect(wrapper.text()).toContain('随机抽取并加入 0 道')
  })
})
