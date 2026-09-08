import type {
  PageResult,
  PaperItem,
  Question,
  QuestionFilters,
} from '../types/domain'
import { clonePlain } from './clonePlain'

export const QUESTION_FETCH_PAGE_SIZE = 100
export const QUESTION_FETCH_MAX_PAGES = 500
export const QUESTION_FETCH_MAX_ITEMS = 50_000

export type QuestionPageLoader = (filters: QuestionFilters) => Promise<PageResult<Question>>

export class QuestionPageReadError extends Error {
  constructor(
    message: string,
    readonly code:
      | 'QUESTION_PAGE_LIMIT_EXCEEDED'
      | 'QUESTION_PAGE_RESPONSE_INVALID'
      | 'QUESTION_PAGE_DATA_CHANGED',
  ) {
    super(message)
    this.name = 'QuestionPageReadError'
  }
}

interface FetchAllQuestionsOptions {
  pageSize?: number
  maxPages?: number
  maxItems?: number
}

function positiveInteger(value: number | undefined, fallback: number, maximum?: number) {
  if (value === undefined) return fallback
  if (!Number.isSafeInteger(value) || value < 1 || (maximum !== undefined && value > maximum)) {
    throw new QuestionPageReadError('分页读取参数无效。', 'QUESTION_PAGE_RESPONSE_INVALID')
  }
  return value
}

/**
 * Reads every page covered by one question filter.
 *
 * The list API is offset based. If the total changes or a question appears on
 * two pages while reading, continuing could silently omit a question. In that
 * case this helper deliberately stops and asks the caller to retry.
 */
export async function fetchAllMatchingQuestions(
  loadPage: QuestionPageLoader,
  filters: QuestionFilters,
  options: FetchAllQuestionsOptions = {},
): Promise<Question[]> {
  const pageSize = positiveInteger(options.pageSize, QUESTION_FETCH_PAGE_SIZE, QUESTION_FETCH_PAGE_SIZE)
  const maxPages = positiveInteger(options.maxPages, QUESTION_FETCH_MAX_PAGES)
  const maxItems = positiveInteger(options.maxItems, QUESTION_FETCH_MAX_ITEMS)
  const baseFilters: QuestionFilters = {
    ...filters,
    tagIds: [...filters.tagIds],
    page: 1,
    pageSize,
  }
  const questions: Question[] = []
  const seenIds = new Set<string>()
  let expectedTotal: number | null = null

  for (let page = 1; page <= maxPages; page += 1) {
    const result = await loadPage({ ...baseFilters, tagIds: [...baseFilters.tagIds], page })
    if (!Number.isSafeInteger(result.total) || result.total < 0
      || result.page !== page
      || result.pageSize !== pageSize
      || !Array.isArray(result.items)
      || result.items.length > pageSize) {
      throw new QuestionPageReadError('题库返回了无效的分页数据，请重试。', 'QUESTION_PAGE_RESPONSE_INVALID')
    }

    if (expectedTotal === null) {
      expectedTotal = result.total
      if (expectedTotal > maxItems || Math.ceil(expectedTotal / pageSize) > maxPages) {
        throw new QuestionPageReadError(
          `符合条件的题目超过安全读取上限（${maxItems} 道），请缩小学科、章节或题型范围。`,
          'QUESTION_PAGE_LIMIT_EXCEEDED',
        )
      }
    } else if (result.total !== expectedTotal) {
      throw new QuestionPageReadError('读取期间题库内容发生变化，请重新操作。', 'QUESTION_PAGE_DATA_CHANGED')
    }

    for (const question of result.items) {
      if (!question.id || seenIds.has(question.id)) {
        throw new QuestionPageReadError('读取期间题库顺序发生变化，请重新操作。', 'QUESTION_PAGE_DATA_CHANGED')
      }
      seenIds.add(question.id)
      questions.push(question)
    }

    if (questions.length === expectedTotal) return questions
    if (questions.length > expectedTotal || result.items.length < pageSize) {
      throw new QuestionPageReadError('题库分页数据不完整，请重新操作。', 'QUESTION_PAGE_DATA_CHANGED')
    }
  }

  throw new QuestionPageReadError(
    `符合条件的题目超过安全页数上限（${maxPages} 页），请缩小筛选范围。`,
    'QUESTION_PAGE_LIMIT_EXCEEDED',
  )
}

export type PaperItemReplacementFailure =
  | 'target_not_found'
  | 'same_question'
  | 'type_mismatch'
  | 'duplicate_question'

export type PaperItemReplacementResult<T extends PaperItem> =
  | { replaced: true; items: T[] }
  | { replaced: false; items: T[]; reason: PaperItemReplacementFailure }

/**
 * Replaces only the source and snapshot. The paper-item ID, position and any
 * future item-level metadata (for example a score field) are preserved.
 */
export function replacePaperItem<T extends PaperItem>(
  items: readonly T[],
  targetItemId: string,
  replacement: Question,
): PaperItemReplacementResult<T> {
  const target = items.find((item) => item.id === targetItemId)
  if (!target) return { replaced: false, items: [...items], reason: 'target_not_found' }

  const targetQuestionId = target.sourceQuestionId ?? target.snapshot.id
  if (targetQuestionId === replacement.id) {
    return { replaced: false, items: [...items], reason: 'same_question' }
  }
  if (target.snapshot.type !== replacement.type) {
    return { replaced: false, items: [...items], reason: 'type_mismatch' }
  }
  if (items.some((item) => item.id !== targetItemId
    && (item.sourceQuestionId ?? item.snapshot.id) === replacement.id)) {
    return { replaced: false, items: [...items], reason: 'duplicate_question' }
  }

  return {
    replaced: true,
    items: items.map((item) => item.id === targetItemId
      ? {
          ...item,
          sourceQuestionId: replacement.id,
          snapshot: clonePlain(replacement),
        }
      : item) as T[],
  }
}
