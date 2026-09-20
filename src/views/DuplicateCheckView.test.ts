import { enableAutoUnmount, flushPromises, mount } from '@vue/test-utils'
import { reactive, type VNode } from 'vue'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { QuestionDuplicateGroup, QuestionDuplicateScanResult } from '../types/domain'
import DuplicateCheckView from './DuplicateCheckView.vue'

const mocks = vi.hoisted(() => ({
  scanQuestionDuplicates: vi.fn(),
  ignoreQuestionDuplicate: vi.fn().mockResolvedValue(undefined),
  moveQuestionsToRecycle: vi.fn().mockResolvedValue(undefined),
  getQuestion: vi.fn().mockResolvedValue(null),
  getQuestionStemSummaries: vi.fn(),
  refreshTaxonomy: vi.fn().mockResolvedValue(undefined),
  confirm: vi.fn().mockResolvedValue('confirm'),
  success: vi.fn(),
  error: vi.fn(),
  warning: vi.fn(),
  push: vi.fn().mockResolvedValue(undefined),
}))

const route = reactive({
  query: { subjectId: 'subject-1', chapterId: 'chapter-1' } as Record<string, string>,
})
const appStore = {
  subjects: [{
    id: 'subject-1',
    name: '计算机网络',
    chapters: [{ id: 'chapter-1', name: '第一章' }],
  }],
  questionTypes: [],
  refreshTaxonomy: mocks.refreshTaxonomy,
}
const bankStore = reactive({
  filters: {
    subjectId: undefined as string | undefined,
    chapterId: undefined as string | undefined,
    page: 2,
  },
})

vi.mock('vue-router', () => ({
  useRoute: () => route,
  useRouter: () => ({ push: mocks.push }),
}))

vi.mock('../stores/app', () => ({
  useAppStore: () => appStore,
}))

vi.mock('../stores/questionBank', () => ({
  useQuestionBankStore: () => bankStore,
}))

vi.mock('../services/backend', () => ({
  backend: {
    scanQuestionDuplicates: mocks.scanQuestionDuplicates,
    ignoreQuestionDuplicate: mocks.ignoreQuestionDuplicate,
    moveQuestionsToRecycle: mocks.moveQuestionsToRecycle,
    getQuestion: mocks.getQuestion,
    getQuestionStemSummaries: mocks.getQuestionStemSummaries,
  },
}))

vi.mock('element-plus', async (importOriginal) => ({
  ...(await importOriginal<typeof import('element-plus')>()),
  ElMessage: {
    success: mocks.success,
    error: mocks.error,
    warning: mocks.warning,
  },
  ElMessageBox: { confirm: mocks.confirm },
}))

function member(id: string, contentVersion: number, stemPreview: string) {
  return {
    id,
    type: 'short_answer',
    stemPreview,
    subjectId: id === 'question-1' ? 'subject-1' : 'subject-2',
    chapterId: id === 'question-1' ? 'chapter-1' : 'chapter-2',
    subjectName: id === 'question-1' ? '计算机网络' : '教育学',
    chapterName: id === 'question-1' ? '第一章' : '第二章',
    contentVersion,
    createdAt: 1,
    updatedAt: 2,
  }
}

function group(kind: 'exact' | 'suspected'): QuestionDuplicateGroup {
  return {
    id: `${kind}:question-1:question-2`,
    duplicateKind: kind,
    similarityPercent: kind === 'exact' ? 100 : 91,
    members: [member('question-1', 3, '第一题'), member('question-2', 4, '第二题')],
  }
}

function result(kind: 'exact' | 'suspected'): QuestionDuplicateScanResult {
  return {
    scannedQuestionCount: 1,
    comparedQuestionCount: 2,
    suspectedThresholdPercent: 80,
    exactGroups: kind === 'exact' ? [group(kind)] : [],
    suspectedGroups: kind === 'suspected' ? [group(kind)] : [],
  }
}

function mountView() {
  return mount(DuplicateCheckView, {
    global: {
      stubs: {
        PageHeader: { template: '<header><slot /></header>' },
        QuestionPreviewDrawer: true,
        ElButton: {
          emits: ['click'],
          template: '<button @click="$emit(\'click\')"><slot /></button>',
        },
        ElTabs: { template: '<div><slot /></div>' },
        ElTabPane: true,
        ElRadioGroup: { template: '<div><slot /></div>' },
        ElRadio: { template: '<label><slot /></label>' },
        ElTag: { template: '<span><slot /></span>' },
        ElEmpty: { props: ['description'], template: '<div>{{ description }}</div>' },
        ElAlert: { props: ['title'], template: '<div>{{ title }}</div>' },
        ElProgress: true,
      },
    },
  })
}

enableAutoUnmount(afterEach)
beforeEach(() => {
  mocks.getQuestionStemSummaries.mockImplementation(async (ids: string[]) => ids.map((id) => ({
    id, contentVersion: id === 'question-1' ? 3 : 4,
    stem: { schemaVersion: 1, html: '<p>完整题干</p>', plainText: '完整题干' },
  })))
})

afterEach(() => {
  vi.clearAllMocks()
  route.query = { subjectId: 'subject-1', chapterId: 'chapter-1' }
  bankStore.filters.subjectId = undefined
  bankStore.filters.chapterId = undefined
  bankStore.filters.page = 2
})

describe('DuplicateCheckView', () => {
  it.each(['exact', 'suspected'] as const)('renders complete formulas in %s results and the retention dialog', async (kind) => {
    const data = result(kind)
    const groups = kind === 'exact' ? data.exactGroups : data.suspectedGroups
    groups[0]!.members.forEach((item) => { item.stemPreview = String.raw`675^{\circ}…` })
    mocks.scanQuestionDuplicates.mockResolvedValue(data)
    mocks.getQuestionStemSummaries.mockImplementation(async (ids: string[]) => ids.map((id) => ({
      id, contentVersion: id === 'question-1' ? 3 : 4,
      stem: { schemaVersion: 1,
        html: String.raw`<p><span class="math-node" data-latex="675^{\circ}">675^{\circ}</span>用弧度制表示为 <span class="math-node" data-latex="\frac{15\pi}{4}">公式</span></p>`,
        plainText: String.raw`675^{\circ}用弧度制表示为`,
      },
    })))
    const wrapper = mountView()
    await flushPromises()
    expect(wrapper.findAll('.katex')).toHaveLength(4)
    expect(wrapper.findAll('.frac-line')).toHaveLength(2)
    expect(wrapper.find('.katex-html').text()).toContain('∘')
    expect(mocks.getQuestion).not.toHaveBeenCalled()
    const recycle = wrapper.findAll('button').find((button) => button.text().includes('其余移入回收站'))!
    await recycle.trigger('click')
    await flushPromises()
    const node = mocks.confirm.mock.calls[0]![0] as VNode
    const dialog = mount({ render: () => node })
    await flushPromises()
    expect(dialog.findAll('.katex')).toHaveLength(2)
    expect(dialog.text()).toContain('其余 1 道题移入回收站')
  })

  it('does not recycle when the scanned question has changed', async () => {
    mocks.scanQuestionDuplicates.mockResolvedValue(result('exact'))
    mocks.getQuestionStemSummaries.mockResolvedValue([])
    const wrapper = mountView()
    await flushPromises()
    const recycle = wrapper.findAll('button').find((button) => button.text().includes('其余移入回收站'))!
    await recycle.trigger('click')
    await flushPromises()
    expect(mocks.confirm).not.toHaveBeenCalled()
    expect(mocks.moveQuestionsToRecycle).not.toHaveBeenCalled()
    expect(wrapper.text()).toContain('题目已修改或移除')
  })

  it('does not recycle when confirmation is cancelled', async () => {
    mocks.scanQuestionDuplicates.mockResolvedValue(result('exact'))
    mocks.confirm.mockRejectedValueOnce('cancel')
    const wrapper = mountView()
    await flushPromises()
    const recycle = wrapper.findAll('button').find((button) => button.text().includes('其余移入回收站'))!
    await recycle.trigger('click')
    await flushPromises()
    expect(mocks.confirm).toHaveBeenCalled()
    expect(mocks.moveQuestionsToRecycle).not.toHaveBeenCalled()
  })

  it('scans the selected scope and automatically shows the only non-empty tab', async () => {
    mocks.scanQuestionDuplicates.mockResolvedValue(result('suspected'))
    const wrapper = mountView()
    await flushPromises()

    expect(mocks.scanQuestionDuplicates).toHaveBeenCalledWith({
      subjectId: 'subject-1',
      chapterId: 'chapter-1',
    })
    expect(bankStore.filters).toMatchObject({
      subjectId: 'subject-1',
      chapterId: 'chapter-1',
      page: 1,
    })
    expect(wrapper.text()).toContain('疑似重复组 1')
    expect(wrapper.text()).toContain('题库活跃题目')
  })

  it('ignores a suspected pair with the scanned content versions and rescans', async () => {
    mocks.scanQuestionDuplicates.mockResolvedValue(result('suspected'))
    const wrapper = mountView()
    await flushPromises()

    const ignore = wrapper.findAll('button').find((button) => button.text().includes('标记为非重复'))
    await ignore!.trigger('click')
    await flushPromises()

    expect(mocks.ignoreQuestionDuplicate).toHaveBeenCalledWith({
      firstQuestionId: 'question-1',
      firstContentVersion: 3,
      secondQuestionId: 'question-2',
      secondContentVersion: 4,
    })
    expect(mocks.scanQuestionDuplicates).toHaveBeenCalledTimes(2)
  })

  it('keeps the default first item and recycles the other exact duplicate after confirmation', async () => {
    mocks.scanQuestionDuplicates.mockResolvedValue(result('exact'))
    const wrapper = mountView()
    await flushPromises()

    const recycle = wrapper.findAll('button').find((button) => button.text().includes('其余移入回收站'))
    await recycle!.trigger('click')
    await flushPromises()

    expect(mocks.confirm).toHaveBeenCalled()
    expect(mocks.moveQuestionsToRecycle).toHaveBeenCalledWith(['question-2'])
    expect(mocks.refreshTaxonomy).toHaveBeenCalled()
    expect(mocks.scanQuestionDuplicates).toHaveBeenCalledTimes(2)
  })
})
