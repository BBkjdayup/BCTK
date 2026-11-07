import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { defineComponent, nextTick } from 'vue'
import { ElMessage } from 'element-plus'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { useAppStore } from '../stores/app'
import { useQuestionBankStore } from '../stores/questionBank'
import { usePaperStore } from '../stores/paper'
import type { PageResult, QuestionSummary } from '../types/domain'
import PaperBuilderView from './PaperBuilderView.vue'

const mocks = vi.hoisted(() => ({
  initialize: vi.fn(), listQuestionSummaries: vi.fn(), listQuestions: vi.fn(),
  getQuestion: vi.fn(), listTemplates: vi.fn(),
}))
vi.mock('../services/backend', () => ({ backend: mocks, isDesktopRuntime: () => false }))
vi.mock('vue-router', () => ({ useRoute: () => ({ query: { view: 'manual' } }) }))
vi.mock('sortablejs', () => ({ default: class { destroy() {} } }))
vi.mock('element-plus', async (importOriginal) => ({
  ...(await importOriginal<typeof import('element-plus')>()),
  ElMessage: { success: vi.fn(), warning: vi.fn(), error: vi.fn(), info: vi.fn() },
}))

const ModelStub = (name: string) => defineComponent({
  name,
  props: ['modelValue', 'disabled'],
  emits: ['update:modelValue', 'change'],
  template: '<div><slot /></div>',
})
let wrapper: VueWrapper
let preview: typeof import('../services/mockBackend').mockBackend

async function changeFilter(label: string, value: unknown) {
  const control = wrapper.findAllComponents({ name: label === '搜索题目' ? 'ElInput' : 'ElSelect' })
    .find((item) => item.attributes('aria-label') === label)!
  control.vm.$emit('update:modelValue', value)
  control.vm.$emit('change', value)
  await flushPromises()
}

async function clickButton(text: string) {
  const button = wrapper.findAll('button').find((item) => item.text() === text)!
  await button.trigger('click')
  await flushPromises()
}

beforeEach(async () => {
  localStorage.clear()
  vi.resetModules()
  vi.resetAllMocks()
  preview = (await import('../services/mockBackend')).mockBackend
  mocks.initialize.mockImplementation(() => preview.initialize())
  mocks.listQuestionSummaries.mockImplementation((filters) => preview.listQuestions(filters))
  mocks.listQuestions.mockImplementation((filters) => preview.listQuestions(filters))
  mocks.getQuestion.mockImplementation((id) => preview.getQuestion(id))
  mocks.listTemplates.mockResolvedValue([])
  const pinia = createPinia()
  setActivePinia(pinia)
  await useAppStore().initialize()
  wrapper = mount(PaperBuilderView, {
    global: {
      plugins: [pinia],
      directives: { loading: () => undefined },
      stubs: {
        SubjectTree: { name: 'SubjectTree', emits: ['select'], template: '<aside />' },
        AppContextMenu: true, QuestionPreviewDrawer: true, PaperExportDialog: true,
        RandomDrawDialog: {
          name: 'RandomDrawDialog', props: ['modelValue', 'paperId', 'excludedQuestionIds'],
          emits: ['update:modelValue', 'add'], template: '<section />',
        },
        PaperCanvasEditor: true,
        QuestionStemSummary: { props: ['content'], template: '<div>{{ content?.plainText }}</div>' },
        ElInput: ModelStub('ElInput'), ElSelect: ModelStub('ElSelect'), ElOption: true,
        ElButton: { props: ['disabled'], template: '<button :disabled="disabled"><slot /></button>' },
        ElTag: { template: '<span><slot /></span>' },
        ElTooltip: { name: 'ElTooltip', props: ['content'], template: '<span :title="content"><slot /></span>' },
        ElPagination: { name: 'ElPagination', emits: ['current-change', 'size-change'], template: '<nav />' },
        ElDialog: { props: ['modelValue'], template: '<section v-if="modelValue"><slot /><slot name="footer" /></section>' },
        ElRadioGroup: ModelStub('ElRadioGroup'), ElRadio: true, ElEmpty: true, ElAlert: true,
      },
    },
  })
  await flushPromises()
})

afterEach(() => {
  wrapper?.unmount()
  localStorage.clear()
})

describe('manual paper tag filtering', () => {
  it('filters by optional unit, keeps selected paper items and returns to the full chapter', async () => {
    const unitId = await preview.createUnit('chapter-edu-1', '第一单元')
    const question = (await preview.getQuestion('question-1'))!
    await preview.saveQuestion({ ...question, questionId: question.id, baseContentVersion: question.contentVersion, unitId, tagIds: question.tags.map((tag) => tag.id) })
    const tree = wrapper.findComponent({ name: 'SubjectTree' })
    const bank = useQuestionBankStore()
    tree.vm.$emit('select', { subjectId: question.subjectId, chapterId: question.chapterId, unitId })
    await changeFilter('标签筛选', ['tag-foundation'])
    expect(bank.questions.map((item) => item.id)).toEqual([question.id])
    expect(wrapper.get('.candidate-row').text()).toContain('第一单元')
    await wrapper.get('.candidate-row button').trigger('click')
    await flushPromises()
    tree.vm.$emit('select', { subjectId: question.subjectId, chapterId: question.chapterId, unassignedUnit: true })
    await flushPromises()
    expect(bank.filters.unitId).toBeUndefined()
    expect(bank.questions.map((item) => item.id)).toEqual(['question-2'])
    expect(usePaperStore().current.items[0]?.sourceQuestionId).toBe(question.id)
    tree.vm.$emit('select', { subjectId: question.subjectId, chapterId: question.chapterId })
    await flushPromises()
    expect(bank.filters.unassignedUnit).toBe(false)
    expect(bank.questions.map((item) => item.id).sort()).toEqual(['question-1', 'question-2'])
    expect(bank.filters.tagIds).toEqual(['tag-foundation'])
  })

  it('combines tags, keyword, type and chapter, then clears only the tags from an empty result', async () => {
    const bank = useQuestionBankStore()
    wrapper.findComponent({ name: 'SubjectTree' }).vm.$emit('select', {
      subjectId: 'subject-education', chapterId: 'chapter-edu-1',
    })
    await changeFilter('搜索题目', '基本要素')
    await changeFilter('题型筛选', 'multiple_choice')
    await changeFilter('标签筛选', ['tag-foundation'])
    expect(bank.questions.map((q) => q.id)).toEqual(['question-2'])
    expect(wrapper.get('.candidate-row__tags').text()).toContain('基础概念')

    await changeFilter('标签筛选', ['tag-thinking'])
    expect(bank.total).toBe(0)
    await clickButton('清空标签筛选')
    expect(bank.filters).toMatchObject({
      keyword: '基本要素', type: 'multiple_choice', subjectId: 'subject-education',
      chapterId: 'chapter-edu-1', tagIds: [], tagMatchMode: 'all', page: 1,
    })
    expect(bank.questions.map((q) => q.id)).toEqual(['question-2'])
  })

  it('uses intersection by default and resets paging when switching to any tag', async () => {
    const bank = useQuestionBankStore()
    const pagination = wrapper.findComponent({ name: 'ElPagination' })
    pagination.vm.$emit('size-change', 1)
    await flushPromises()
    await changeFilter('标签筛选', ['tag-foundation', 'tag-exam'])
    expect(bank.total).toBe(1)
    expect(bank.questions[0]?.id).toBe('question-1')

    await changeFilter('多标签匹配方式', 'any')
    expect(bank.total).toBe(3)
    const firstId = bank.questions[0]?.id
    pagination.vm.$emit('current-change', 2)
    await flushPromises()
    expect(bank.filters.page).toBe(2)
    expect(bank.questions[0]?.id).not.toBe(firstId)
    expect(bank.questions[0]?.tags.some((tag) => bank.filters.tagIds.includes(tag.id))).toBe(true)
    await changeFilter('多标签匹配方式', 'all')
    expect(bank.filters.page).toBe(1)
    expect(bank.total).toBe(1)
    await changeFilter('标签筛选', [])
    expect(bank.total).toBe(4)
  })

  it('keeps selected questions and filters across editing and preview, and narrows replacements', async () => {
    const source = (await preview.getQuestion('question-1'))!
    const replacement = await preview.saveQuestion({
      type: source.type, subjectId: source.subjectId, chapterId: source.chapterId,
      stem: { schemaVersion: 1, html: '<p>新的替换候选</p>', plainText: '新的替换候选' },
      options: source.options, answer: source.answer, explanation: source.explanation,
      tagIds: ['tag-foundation', 'tag-exam'],
    })
    await changeFilter('标签筛选', ['tag-foundation', 'tag-exam'])
    const sourceRow = wrapper.findAll('.candidate-row').find((row) => row.text().includes(source.stem.plainText))!
    await sourceRow.get('button').trigger('click')
    await flushPromises()
    const paper = usePaperStore()
    const originalItem = { ...paper.current.items[0]! }
    await changeFilter('标签筛选', ['tag-thinking'])
    expect(paper.current.items).toHaveLength(1)
    expect(paper.current.items[0]?.sourceQuestionId).toBe(source.id)
    await changeFilter('标签筛选', ['tag-foundation', 'tag-exam'])
    await changeFilter('多标签匹配方式', 'any')

    await clickButton('进入试卷编辑')
    await clickButton('排版与打印')
    await clickButton('返回修改')
    await clickButton('替换')
    expect(mocks.listQuestions).toHaveBeenLastCalledWith(expect.objectContaining({
      tagIds: ['tag-foundation', 'tag-exam'], tagMatchMode: 'any', type: 'single_choice',
    }))
    const candidates = wrapper.findAllComponents({ name: 'ElRadio' })
    expect(candidates.map((item) => item.attributes('value'))).toEqual([replacement.id])
    const radio = wrapper.findComponent({ name: 'ElRadioGroup' })
    radio.vm.$emit('update:modelValue', replacement.id)
    await nextTick()
    await clickButton('确认替换')
    expect(paper.current.items[0]).toMatchObject({
      id: originalItem.id, position: originalItem.position, sourceQuestionId: replacement.id,
    })
    await clickButton('返回选题')
    expect(useQuestionBankStore().filters).toMatchObject({ tagIds: ['tag-foundation', 'tag-exam'], tagMatchMode: 'any' })
    expect(wrapper.findAll('.selected-row')).toHaveLength(1)
  })

  it('ignores an older tag request and shows a retry on failure instead of stale candidates', async () => {
    const bank = useQuestionBankStore()
    let finish!: (page: PageResult<QuestionSummary>) => void
    mocks.listQuestionSummaries.mockImplementationOnce(() => new Promise((resolve) => { finish = resolve }))
    await changeFilter('标签筛选', ['tag-foundation'])
    await changeFilter('标签筛选', ['tag-thinking'])
    expect(bank.questions.map((q) => q.id)).toEqual(['question-3'])
    finish({ items: [], total: 999, page: 1, pageSize: 50 })
    await flushPromises()
    expect(bank.total).toBe(1)
    expect(bank.questions[0]?.id).toBe('question-3')

    mocks.listQuestionSummaries.mockRejectedValueOnce(new Error('测试读取失败'))
    await changeFilter('标签筛选', ['tag-foundation'])
    expect(wrapper.findAll('.candidate-row')).toHaveLength(0)
    expect(wrapper.get('[role="alert"]').text()).toContain('测试读取失败')
    expect(wrapper.text()).not.toContain('当前页没有符合条件')
    await clickButton('重新加载')
    expect(bank.questions.map((q) => q.id).sort()).toEqual(['question-1', 'question-2'])
    expect(wrapper.find('[role="alert"]').exists()).toBe(false)
  })

  it('collapses extra candidate tags while keeping their full names available', async () => {
    const source = (await preview.getQuestion('question-3'))!
    await preview.saveQuestion({
      type: source.type, subjectId: source.subjectId, chapterId: source.chapterId,
      stem: { schemaVersion: 1, html: '<p>三个标签的候选题</p>', plainText: '三个标签的候选题' },
      options: [], answer: source.answer, explanation: source.explanation,
      tagIds: ['tag-foundation', 'tag-exam', 'tag-thinking'],
    })
    await changeFilter('标签筛选', ['tag-foundation', 'tag-thinking'])
    const row = wrapper.get('.candidate-row')
    expect(row.get('.candidate-row__tags').text()).toBe('基础概念常考题+1')
    expect(row.findComponent({ name: 'ElTooltip' }).props('content')).toBe('思考题')
  })
})

describe('random draw target paper', () => {
  it('binds the dialog to its paper and reports the actual number of additions', async () => {
    const paper = usePaperStore()
    const existing = (await preview.getQuestion('question-1'))!
    const addition = (await preview.getQuestion('question-2'))!
    paper.addQuestions([existing])
    await clickButton('随机抽题')
    const dialog = wrapper.findComponent({ name: 'RandomDrawDialog' })
    expect(dialog.props('paperId')).toBe(paper.current.id)
    expect(dialog.props('excludedQuestionIds')).toEqual([existing.id])
    dialog.vm.$emit('add', [existing, addition], paper.current.id)
    await nextTick()
    expect(paper.current.items).toHaveLength(2)
    expect(new Set(dialog.props('excludedQuestionIds'))).toEqual(new Set([existing.id, addition.id]))
    expect(ElMessage.success).toHaveBeenLastCalledWith('已随机抽取并加入 1 道题')
  })

  it('rejects a result for another paper before updated props reach the dialog', async () => {
    const selected = (await preview.getQuestion('question-1'))!
    const paper = usePaperStore()
    await clickButton('随机抽题')
    const dialog = wrapper.findComponent({ name: 'RandomDrawDialog' })
    const originalId = paper.current.id
    paper.current = { ...paper.current, id: 'next-paper', compositionMode: 'automatic' }
    dialog.vm.$emit('add', [selected], originalId)
    expect(paper.current.items).toHaveLength(0)
    expect(paper.current.compositionMode).toBe('automatic')
    expect(ElMessage.success).not.toHaveBeenCalled()
  })

  it('rejects a result after cancellation before updated props reach the dialog', async () => {
    const selected = (await preview.getQuestion('question-1'))!
    const paper = usePaperStore()
    await clickButton('随机抽题')
    const dialog = wrapper.findComponent({ name: 'RandomDrawDialog' })
    dialog.vm.$emit('update:modelValue', false)
    dialog.vm.$emit('add', [selected], paper.current.id)
    expect(paper.current.items).toHaveLength(0)
    expect(ElMessage.success).not.toHaveBeenCalled()
  })
})
