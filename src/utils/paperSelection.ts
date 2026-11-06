import { readAllPages, type PageReadOptions } from './boundedPages'
import type {
  PageResult,
  PaperItem,
  Question,
  QuestionFilters,
} from '../types/domain'
import { clonePlain } from './clonePlain'

const QUESTION_FETCH_PAGE_SIZE = 100
const QUESTION_FETCH_MAX_PAGES = 500
const QUESTION_FETCH_MAX_ITEMS = 50_000

export type QuestionPageLoader = (filters: QuestionFilters) => Promise<PageResult<Question>>

class QuestionPageReadError extends Error {
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

export async function fetchAllMatchingQuestions(
  loadPage: QuestionPageLoader,
  filters: QuestionFilters,
  options: PageReadOptions = {},
): Promise<Question[]> {
  const baseFilters = { ...filters, tagIds: [...filters.tagIds] }
  return readAllPages(
    (page, pageSize) => loadPage({ ...baseFilters, tagIds: [...baseFilters.tagIds], page, pageSize }),
    { pageSize: QUESTION_FETCH_PAGE_SIZE, maxPages: QUESTION_FETCH_MAX_PAGES, maxItems: QUESTION_FETCH_MAX_ITEMS },
    options,
    (code, reason, { maxItems, maxPages }) => {
      const messages = {
        parameters: '分页读取参数无效。',
        response: '题库返回了无效的分页数据，请重试。',
        limit: `符合条件的题目超过安全读取上限（${maxItems} 道），请缩小学科、章节或题型范围。`,
        total: '读取期间题库内容发生变化，请重新操作。',
        duplicate: '读取期间题库顺序发生变化，请重新操作。',
        incomplete: '题库分页数据不完整，请重新操作。',
        pages: `符合条件的题目超过安全页数上限（${maxPages} 页），请缩小筛选范围。`,
      }
      return new QuestionPageReadError(messages[reason], `QUESTION_${code}`)
    },
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
