import type { PageResult, PaperFilters, PaperSummary } from '../types/domain'

export const PAPER_HISTORY_PAGE_SIZE = 100
export const PAPER_HISTORY_MAX_PAGES = 500
export const PAPER_HISTORY_MAX_ITEMS = 50_000

export type PaperSummaryPageLoader = (
  filters: PaperFilters,
) => Promise<PageResult<PaperSummary>>

export class PaperHistoryReadError extends Error {
  constructor(
    message: string,
    readonly code:
      | 'PAPER_HISTORY_PAGE_LIMIT_EXCEEDED'
      | 'PAPER_HISTORY_PAGE_RESPONSE_INVALID'
      | 'PAPER_HISTORY_PAGE_DATA_CHANGED',
  ) {
    super(message)
    this.name = 'PaperHistoryReadError'
  }
}

interface FetchAllPaperSummariesOptions {
  pageSize?: number
  maxPages?: number
  maxItems?: number
}

function positiveInteger(value: number | undefined, fallback: number, maximum?: number) {
  if (value === undefined) return fallback
  if (!Number.isSafeInteger(value) || value < 1 || (maximum !== undefined && value > maximum)) {
    throw new PaperHistoryReadError(
      '历史试卷分页读取参数无效。',
      'PAPER_HISTORY_PAGE_RESPONSE_INVALID',
    )
  }
  return value
}

/**
 * Reads a complete, stable snapshot of paper summaries from the offset-based
 * list API. A changing total, duplicate ID or short intermediate page means
 * that continuing could silently omit a paper, so the caller must retry.
 */
export async function fetchAllPaperSummaries(
  loadPage: PaperSummaryPageLoader,
  filters: PaperFilters,
  options: FetchAllPaperSummariesOptions = {},
): Promise<PaperSummary[]> {
  const pageSize = positiveInteger(options.pageSize, PAPER_HISTORY_PAGE_SIZE, PAPER_HISTORY_PAGE_SIZE)
  const maxPages = positiveInteger(options.maxPages, PAPER_HISTORY_MAX_PAGES)
  const maxItems = positiveInteger(options.maxItems, PAPER_HISTORY_MAX_ITEMS)
  const baseFilters: PaperFilters = { ...filters, page: 1, pageSize }
  const papers: PaperSummary[] = []
  const seenIds = new Set<string>()
  let expectedTotal: number | null = null

  for (let page = 1; page <= maxPages; page += 1) {
    const result = await loadPage({ ...baseFilters, page })
    if (!Number.isSafeInteger(result.total) || result.total < 0
      || result.page !== page
      || result.pageSize !== pageSize
      || !Array.isArray(result.items)
      || result.items.length > pageSize) {
      throw new PaperHistoryReadError(
        '历史试卷返回了无效的分页数据，请重试。',
        'PAPER_HISTORY_PAGE_RESPONSE_INVALID',
      )
    }

    if (expectedTotal === null) {
      expectedTotal = result.total
      if (expectedTotal > maxItems || Math.ceil(expectedTotal / pageSize) > maxPages) {
        throw new PaperHistoryReadError(
          `历史试卷超过安全读取上限（${maxItems} 份或 ${maxPages} 页），请先缩小查询范围。`,
          'PAPER_HISTORY_PAGE_LIMIT_EXCEEDED',
        )
      }
    } else if (result.total !== expectedTotal) {
      throw new PaperHistoryReadError(
        '读取期间历史试卷发生变化，请重新加载。',
        'PAPER_HISTORY_PAGE_DATA_CHANGED',
      )
    }

    for (const paper of result.items) {
      if (!paper.id || seenIds.has(paper.id)) {
        throw new PaperHistoryReadError(
          '读取期间历史试卷顺序发生变化，请重新加载。',
          'PAPER_HISTORY_PAGE_DATA_CHANGED',
        )
      }
      seenIds.add(paper.id)
      papers.push(paper)
    }

    if (papers.length === expectedTotal) return papers
    if (papers.length > expectedTotal || result.items.length < pageSize) {
      throw new PaperHistoryReadError(
        '历史试卷分页数据不完整，请重新加载。',
        'PAPER_HISTORY_PAGE_DATA_CHANGED',
      )
    }
  }

  throw new PaperHistoryReadError(
    `历史试卷超过安全页数上限（${maxPages} 页），请先缩小查询范围。`,
    'PAPER_HISTORY_PAGE_LIMIT_EXCEEDED',
  )
}
