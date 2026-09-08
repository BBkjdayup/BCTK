import { flushPromises, mount } from '@vue/test-utils'
import { reactive } from 'vue'
import { afterEach, describe, expect, it, vi } from 'vitest'
import QuestionBankWorkspace from './QuestionBankWorkspace.vue'

const replace = vi.fn().mockResolvedValue(undefined)
const push = vi.fn().mockResolvedValue(undefined)
const route = reactive({
  path: '/word-import',
  query: { source: 'toolbar' } as Record<string, string>,
})
const appStore = {
  subjects: [{
    id: 'subject-1',
    name: '网络设备',
    chapters: [{ id: 'chapter-1', name: '第一章' }],
  }],
}
const bankStore = reactive({
  filters: {
    subjectId: undefined as string | undefined,
    chapterId: undefined as string | undefined,
    page: 3,
  },
})

vi.mock('vue-router', () => ({
  useRoute: () => route,
  useRouter: () => ({ replace, push }),
}))

vi.mock('../stores/app', () => ({
  useAppStore: () => appStore,
}))

vi.mock('../stores/questionBank', () => ({
  useQuestionBankStore: () => bankStore,
}))

afterEach(() => {
  route.path = '/word-import'
  route.query = { source: 'toolbar' }
  bankStore.filters.subjectId = undefined
  bankStore.filters.chapterId = undefined
  bankStore.filters.page = 3
  vi.clearAllMocks()
})

describe('QuestionBankWorkspace entry classification', () => {
  it('keeps Word import open while the sidebar changes its default chapter', async () => {
    const wrapper = mount(QuestionBankWorkspace, {
      global: {
        stubs: {
          SubjectTree: {
            emits: ['select'],
            template: '<button class="subject-tree-stub" @click="$emit(\'select\', { subjectId: \'subject-1\', chapterId: \'chapter-1\' })">选择章节</button>',
          },
          RouterView: true,
        },
      },
    })

    await wrapper.get('.subject-tree-stub').trigger('click')
    await flushPromises()

    expect(replace).toHaveBeenCalledWith({
      path: '/word-import',
      query: {
        source: 'toolbar',
        subjectId: 'subject-1',
        chapterId: 'chapter-1',
      },
    })
    expect(bankStore.filters).toMatchObject({
      subjectId: 'subject-1',
      chapterId: 'chapter-1',
      page: 1,
    })
    expect(push).not.toHaveBeenCalled()
  })

  it('keeps duplicate checking open when the sidebar selects another scope', async () => {
    route.path = '/duplicate-check'
    route.query = { subjectId: 'old-subject' }
    const wrapper = mount(QuestionBankWorkspace, {
      global: {
        stubs: {
          SubjectTree: {
            emits: ['select'],
            template: '<button class="subject-tree-stub" @click="$emit(\'select\', { subjectId: \'subject-1\', chapterId: \'chapter-1\' })">选择章节</button>',
          },
          RouterView: true,
        },
      },
    })

    await wrapper.get('.subject-tree-stub').trigger('click')
    await flushPromises()

    expect(replace).toHaveBeenCalledWith({
      path: '/duplicate-check',
      query: { subjectId: 'subject-1', chapterId: 'chapter-1' },
    })
    expect(bankStore.filters).toMatchObject({
      subjectId: 'subject-1',
      chapterId: 'chapter-1',
      page: 1,
    })
    expect(push).not.toHaveBeenCalled()
  })
})
