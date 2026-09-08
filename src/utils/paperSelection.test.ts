import { describe, expect, it, vi } from 'vitest'
import type { PageResult, PaperItem, Question, QuestionFilters, QuestionType } from '../types/domain'
import {
  fetchAllMatchingQuestions,
  replacePaperItem,
} from './paperSelection'

function question(id: string, type: QuestionType = 'single_choice'): Question {
  const content = { schemaVersion: 1 as const, html: `<p>${id}</p>`, plainText: id }
  return {
    id,
    type,
    stem: content,
    options: [],
    answer: content,
    explanation: content,
    subjectId: 'subject',
    chapterId: 'chapter',
    subjectName: '学科',
    chapterName: '章节',
    tags: [],
    resourceRefs: [],
    lastUsedAt: null,
    deletedAt: null,
    createdAt: 1,
    updatedAt: 1,
    contentVersion: 1,
  }
}

function filters(): QuestionFilters {
  return {
    keyword: '',
    subjectId: undefined,
    chapterId: undefined,
    type: undefined,
    tagIds: [],
    tagMatchMode: 'all',
    usage: 'all',
    deleted: false,
    page: 8,
    pageSize: 20,
  }
}

function pageLoader(allQuestions: Question[]) {
  return vi.fn(async (request: QuestionFilters): Promise<PageResult<Question>> => {
    const start = (request.page - 1) * request.pageSize
    return {
      items: allQuestions.slice(start, start + request.pageSize),
      total: allQuestions.length,
      page: request.page,
      pageSize: request.pageSize,
    }
  })
}

describe('complete question paging', () => {
  it('reads all matching questions when the result has more than 100 rows', async () => {
    const questions = Array.from({ length: 235 }, (_, index) => question(`q-${index + 1}`))
    const loadPage = pageLoader(questions)

    const result = await fetchAllMatchingQuestions(loadPage, filters())

    expect(result.map((item) => item.id)).toEqual(questions.map((item) => item.id))
    expect(loadPage).toHaveBeenCalledTimes(3)
    expect(loadPage.mock.calls.map(([request]) => request.page)).toEqual([1, 2, 3])
    expect(loadPage.mock.calls.every(([request]) => request.pageSize === 100)).toBe(true)
  })

  it('stops at the reported total even when the last page is exactly full', async () => {
    const questions = Array.from({ length: 200 }, (_, index) => question(`q-${index + 1}`))
    const loadPage = pageLoader(questions)

    const result = await fetchAllMatchingQuestions(loadPage, filters())

    expect(result).toHaveLength(200)
    expect(loadPage).toHaveBeenCalledTimes(2)
  })

  it('does not accept a changing total as a complete result', async () => {
    const loadPage = vi.fn(async (request: QuestionFilters): Promise<PageResult<Question>> => ({
      items: Array.from({ length: request.page === 1 ? 100 : 1 }, (_, index) => (
        question(`page-${request.page}-${index}`)
      )),
      total: request.page === 1 ? 101 : 102,
      page: request.page,
      pageSize: request.pageSize,
    }))

    await expect(fetchAllMatchingQuestions(loadPage, filters())).rejects.toMatchObject({
      code: 'QUESTION_PAGE_DATA_CHANGED',
    })
  })
})

describe('paper replacement', () => {
  it('keeps the item position and score while rejecting a duplicate replacement', () => {
    type ScoredItem = PaperItem & { score: number }
    const items: ScoredItem[] = [
      {
        id: 'paper-item-1', sourceQuestionId: 'q-1', position: 0,
        snapshot: question('q-1'), score: 6,
      },
      {
        id: 'paper-item-2', sourceQuestionId: 'q-2', position: 1,
        snapshot: question('q-2'), score: 9,
      },
    ]

    const duplicate = replacePaperItem(items, 'paper-item-1', question('q-2'))
    expect(duplicate).toMatchObject({ replaced: false, reason: 'duplicate_question' })
    expect(duplicate.items).toEqual(items)

    const replaced = replacePaperItem(items, 'paper-item-1', question('q-3'))
    expect(replaced.replaced).toBe(true)
    expect(replaced.items.map((item) => item.sourceQuestionId)).toEqual(['q-3', 'q-2'])
    expect(replaced.items.map((item) => item.position)).toEqual([0, 1])
    expect(replaced.items.map((item) => item.score)).toEqual([6, 9])
    expect(replaced.items[0].id).toBe('paper-item-1')
  })
})
