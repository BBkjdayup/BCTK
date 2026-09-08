import { flushPromises, mount } from '@vue/test-utils'
import { reactive } from 'vue'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import TaxonomyView from './TaxonomyView.vue'

const mocks = vi.hoisted(() => ({
  instances: [] as Array<{
    element: HTMLElement
    options: Record<string, unknown>
    sort: ReturnType<typeof vi.fn>
    option: ReturnType<typeof vi.fn>
    destroy: ReturnType<typeof vi.fn>
  }>,
  saveSubjectOrder: vi.fn().mockResolvedValue(undefined),
  saveChapterOrder: vi.fn().mockResolvedValue(undefined),
  refreshTaxonomy: vi.fn().mockResolvedValue(undefined),
  success: vi.fn(),
  error: vi.fn(),
  warning: vi.fn(),
}))

const appStore = reactive({
  subjects: [
    {
      id: 'subject-1',
      name: '学科一',
      sortOrder: 1024,
      questionCount: 1,
      createdAt: 1,
      updatedAt: 1,
      chapters: [
        { id: 'chapter-1', name: '章节一', sortOrder: 1024, questionCount: 1, lastAccessedAt: null, createdAt: 1, updatedAt: 1 },
        { id: 'chapter-2', name: '章节二', sortOrder: 2048, questionCount: 0, lastAccessedAt: null, createdAt: 1, updatedAt: 1 },
      ],
    },
    {
      id: 'subject-2',
      name: '学科二',
      sortOrder: 2048,
      questionCount: 0,
      createdAt: 1,
      updatedAt: 1,
      chapters: [],
    },
  ],
  saveSubjectOrder: mocks.saveSubjectOrder,
  saveChapterOrder: mocks.saveChapterOrder,
  refreshTaxonomy: mocks.refreshTaxonomy,
  createSubject: vi.fn(),
  updateSubject: vi.fn(),
  deleteSubject: vi.fn(),
  createChapter: vi.fn(),
  updateChapter: vi.fn(),
  deleteChapter: vi.fn(),
})

vi.mock('sortablejs', () => ({
  default: class SortableMock {
    element: HTMLElement
    options: Record<string, unknown>
    sort = vi.fn()
    option = vi.fn()
    destroy = vi.fn()

    constructor(element: HTMLElement, options: Record<string, unknown>) {
      this.element = element
      this.options = options
      mocks.instances.push(this)
    }
  },
}))

vi.mock('../stores/app', () => ({
  useAppStore: () => appStore,
}))

vi.mock('element-plus', async (importOriginal) => ({
  ...(await importOriginal<typeof import('element-plus')>()),
  ElMessage: {
    success: mocks.success,
    error: mocks.error,
    warning: mocks.warning,
  },
  ElMessageBox: {
    prompt: vi.fn(),
    alert: vi.fn(),
    confirm: vi.fn(),
  },
}))

function mountView() {
  return mount(TaxonomyView, {
    global: {
      stubs: {
        ElButton: { template: '<button><slot /></button>' },
        ElDropdown: { template: '<span><slot /><slot name="dropdown" /></span>' },
        ElDropdownMenu: { template: '<span><slot /></span>' },
        ElDropdownItem: { template: '<span><slot /></span>' },
        ElIcon: { template: '<i><slot /></i>' },
      },
    },
  })
}

function latestSortable(dataIdAttr: string) {
  return [...mocks.instances].reverse().find((instance) => instance.options.dataIdAttr === dataIdAttr)!
}

async function dragFirstSubjectToEnd(wrapper: ReturnType<typeof mountView>) {
  const sortable = latestSortable('data-subject-id')
  const container = wrapper.get('.subject-list').element
  container.append(container.children[0]!)
  const onEnd = sortable.options.onEnd as (event: { oldIndex: number, newIndex: number }) => void
  onEnd({ oldIndex: 0, newIndex: 1 })
  await flushPromises()
  return sortable
}

beforeEach(() => {
  mocks.instances.length = 0
  mocks.saveSubjectOrder.mockResolvedValue(undefined)
  mocks.saveChapterOrder.mockResolvedValue(undefined)
  mocks.refreshTaxonomy.mockResolvedValue(undefined)
})

afterEach(() => {
  vi.clearAllMocks()
})

describe('TaxonomyView SortableJS integration', () => {
  it('uses the rendered data IDs and saves the dragged subject order while disabling sorting', async () => {
    const wrapper = mountView()
    await flushPromises()

    expect(latestSortable('data-subject-id')).toBeDefined()
    expect(latestSortable('data-chapter-id')).toBeDefined()
    const sortable = await dragFirstSubjectToEnd(wrapper)

    expect(mocks.saveSubjectOrder).toHaveBeenCalledWith([
      { id: 'subject-2', sortOrder: 1024 },
      { id: 'subject-1', sortOrder: 2048 },
    ])
    expect(sortable.option).toHaveBeenCalledWith('disabled', true)
    expect(sortable.option).toHaveBeenCalledWith('disabled', false)
  })

  it('restores the original DOM order and refreshes taxonomy when saving fails', async () => {
    mocks.saveSubjectOrder.mockRejectedValueOnce(new Error('save failed'))
    const wrapper = mountView()
    await flushPromises()
    const sortable = await dragFirstSubjectToEnd(wrapper)

    expect(sortable.sort).toHaveBeenCalledWith(['subject-1', 'subject-2'])
    expect(mocks.refreshTaxonomy).toHaveBeenCalled()
    expect(mocks.error).toHaveBeenCalled()
  })
})
