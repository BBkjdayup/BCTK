import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { backend } from '../services/backend'
import type { BootstrapData, PageResult, Question, QuestionDraft } from '../types/domain'
import { useAppStore } from './app'
import { createQuestionFilters, useQuestionBankStore } from './questionBank'
import { basicLicenseOverview } from '../utils/licensing'

const refreshedBootstrap: BootstrapData = {
  initialized: true,
  dataRoot: 'C:\\TeacherData',
  subjects: [{
    id: 'subject-1',
    name: '计算机网络',
    sortOrder: 1,
    questionCount: 1,
    chapters: [{
      id: 'chapter-1',
      subjectId: 'subject-1',
      name: '第一章',
      sortOrder: 1,
      questionCount: 1,
    }],
  }],
      tags: [],
      questionTypes: [],
  pendingDraftCount: 0,
  databaseHealthy: true,
  appVersion: '0.1.0',
  license: basicLicenseOverview('test-device'),
}

describe('question bank filter isolation', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
  })

  afterEach(() => {
    vi.restoreAllMocks()
  })

  it('replaces every shared filter when entering a page', () => {
    const store = useQuestionBankStore()
    store.filters.keyword = '上一页关键字'
    store.filters.subjectId = 'old-subject'
    store.filters.chapterId = 'old-chapter'
    store.filters.type = 'case_analysis'
    store.filters.tagIds = ['old-tag']
    store.filters.usage = 'never'
    store.filters.deleted = false
    store.filters.page = 9
    store.filters.pageSize = 100
    store.selectedIds = ['old-selection']

    const incomingTags = ['recycle-tag']
    store.initializeFilters({ deleted: true, pageSize: 20, tagIds: incomingTags })
    incomingTags.push('later-mutation')

    expect({ ...store.filters, tagIds: [...store.filters.tagIds] }).toEqual(createQuestionFilters({
      deleted: true,
      pageSize: 20,
      tagIds: ['recycle-tag'],
    }))
    expect(store.selectedIds).toEqual([])
    expect(store.questions).toEqual([])
    expect(store.total).toBe(0)
  })

  it('resets all criteria while retaining only the page deleted mode', () => {
    const store = useQuestionBankStore()
    store.initializeFilters({
      keyword: '待清除',
      subjectId: 'subject',
      chapterId: 'chapter',
      type: 'single_choice',
      tagIds: ['tag'],
      usage: 'unused_this_semester',
      deleted: true,
      page: 4,
      pageSize: 100,
    })

    store.resetFilters()

    expect({ ...store.filters, tagIds: [...store.filters.tagIds] }).toEqual(createQuestionFilters({ deleted: true }))
  })

  it('ignores a previous page response that finishes after the new page query', async () => {
    const store = useQuestionBankStore()
    let finishOldRequest!: (page: PageResult<Question>) => void
    const oldRequest = new Promise<PageResult<Question>>((resolve) => {
      finishOldRequest = resolve
    })
    vi.spyOn(backend, 'listQuestions')
      .mockImplementationOnce(() => oldRequest)
      .mockResolvedValueOnce({ items: [], total: 2, page: 1, pageSize: 20 })

    const pendingOldLoad = store.load()
    store.initializeFilters({ deleted: true })
    await store.load()
    finishOldRequest({ items: [], total: 99, page: 1, pageSize: 20 })
    await pendingOldLoad

    expect(store.filters.deleted).toBe(true)
    expect(store.total).toBe(2)
    expect(store.loading).toBe(false)
  })

  it('refreshes the question page and taxonomy snapshot after every question mutation', async () => {
    const store = useQuestionBankStore()
    const appStore = useAppStore()
    const savedQuestion = { id: 'question-1' } as Question
    vi.spyOn(backend, 'saveQuestion').mockResolvedValue(savedQuestion)
    vi.spyOn(backend, 'moveQuestionsToRecycle').mockResolvedValue()
    vi.spyOn(backend, 'batchEditQuestions').mockResolvedValue({ updatedCount: 1, questions: [] })
    vi.spyOn(backend, 'restoreQuestions').mockResolvedValue()
    vi.spyOn(backend, 'permanentlyDeleteQuestions').mockResolvedValue()
    const listQuestions = vi.spyOn(backend, 'listQuestions')
      .mockResolvedValue({ items: [], total: 1, page: 1, pageSize: 20 })
    const initialize = vi.spyOn(backend, 'initialize').mockResolvedValue(refreshedBootstrap)

    await expect(store.save({} as QuestionDraft)).resolves.toBe(savedQuestion)
    await store.moveToRecycle(['question-1'])
    await store.batchEdit({
      questions: [],
      classification: null,
      usageOperation: 'keep',
      tagOperation: { mode: 'keep', tagIds: [] },
    })
    await store.restore(['question-1'])
    await store.permanentlyDelete(['question-1'])

    expect(listQuestions).toHaveBeenCalledTimes(5)
    expect(initialize).toHaveBeenCalledTimes(5)
    expect(appStore.questionCount).toBe(1)
    expect(appStore.subjects[0]?.chapters[0]?.questionCount).toBe(1)
  })

  it('does not turn a successful save into a failure when the count refresh fails', async () => {
    const store = useQuestionBankStore()
    const savedQuestion = { id: 'question-1' } as Question
    vi.spyOn(backend, 'saveQuestion').mockResolvedValue(savedQuestion)
    vi.spyOn(backend, 'listQuestions')
      .mockResolvedValue({ items: [], total: 1, page: 1, pageSize: 20 })
    vi.spyOn(backend, 'initialize').mockRejectedValue(new Error('temporary failure'))

    await expect(store.save({} as QuestionDraft)).resolves.toBe(savedQuestion)
  })

  it('preserves mutation errors and does not refresh after a failed write', async () => {
    const store = useQuestionBankStore()
    const failure = new Error('write failed')
    vi.spyOn(backend, 'saveQuestion').mockRejectedValue(failure)
    const listQuestions = vi.spyOn(backend, 'listQuestions')
    const initialize = vi.spyOn(backend, 'initialize')

    await expect(store.save({} as QuestionDraft)).rejects.toBe(failure)

    expect(listQuestions).not.toHaveBeenCalled()
    expect(initialize).not.toHaveBeenCalled()
  })
})
