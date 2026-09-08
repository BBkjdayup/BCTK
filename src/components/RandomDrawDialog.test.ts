import { flushPromises, mount } from '@vue/test-utils'
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
  props: ['modelValue', 'disabled', 'max', 'multiple'],
  emits: ['update:modelValue', 'change', 'click'],
  template: `<${tag} :disabled="disabled" @click="$emit('click')"><slot /></${tag}>`,
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

function mountDialog(excludedQuestionIds = ['018f4b00-0000-7000-8000-000000000010']) {
  return mount(RandomDrawDialog, {
    props: {
      modelValue: true,
      subjects,
      tags,
      questionTypes,
      excludedQuestionIds,
      remainingCapacity: null,
    },
    global: {
      stubs: {
        ElDialog: { props: ['modelValue'], template: '<section><slot /><slot name="footer" /></section>' },
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

beforeEach(() => {
  vi.useFakeTimers()
  mocks.analyzeRandomDraw.mockResolvedValue({
    availableTotal: 2,
    availableByType: { single_choice: 2 },
  })
  mocks.drawRandomQuestions.mockResolvedValue([
    question('018f4b00-0000-7000-8000-000000000011'),
    question('018f4b00-0000-7000-8000-000000000012'),
  ])
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

    subjectSelect.vm.$emit('update:modelValue', [subjects[0]!.id, subjects[1]!.id])
    subjectSelect.vm.$emit('change')
    chapterSelect.vm.$emit('update:modelValue', [subjects[0]!.chapters[0]!.id])
    chapterSelect.vm.$emit('change')
    await nextTick()
    await vi.advanceTimersByTimeAsync(200)
    await flushPromises()

    expect(mocks.analyzeRandomDraw).toHaveBeenLastCalledWith(expect.objectContaining({
      subjectIds: [subjects[0]!.id, subjects[1]!.id],
      chapterIds: [subjects[0]!.chapters[0]!.id],
    }))

    subjectSelect.vm.$emit('update:modelValue', [subjects[1]!.id])
    subjectSelect.vm.$emit('change')
    await nextTick()
    await vi.advanceTimersByTimeAsync(200)
    await flushPromises()

    expect(mocks.analyzeRandomDraw).toHaveBeenLastCalledWith(expect.objectContaining({
      subjectIds: [subjects[1]!.id],
      chapterIds: [],
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
    selects[2]!.vm.$emit('update:modelValue', [tags[0]!.id])
    selects[2]!.vm.$emit('change')
    selects[3]!.vm.$emit('update:modelValue', 'unused_this_week')
    selects[3]!.vm.$emit('change')
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
    selects[2]!.vm.$emit('update:modelValue', [tags[0]!.id])
    selects[2]!.vm.$emit('change')
    selects[3]!.vm.$emit('update:modelValue', 'unused_today')
    selects[3]!.vm.$emit('change')
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
