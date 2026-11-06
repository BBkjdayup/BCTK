import { readAllPages, type PageReadOptions } from './boundedPages'
import type { PageResult, PaperFilters, PaperSummary } from '../types/domain'

export const PAPER_HISTORY_PAGE_SIZE = 100
const PAPER_HISTORY_MAX_PAGES = 500
const PAPER_HISTORY_MAX_ITEMS = 50_000

export type PaperSummaryPageLoader = (
  filters: PaperFilters,
) => Promise<PageResult<PaperSummary>>

class PaperHistoryReadError extends Error {
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

export async function fetchAllPaperSummaries(
  loadPage: PaperSummaryPageLoader,
  filters: PaperFilters,
  options: PageReadOptions = {},
): Promise<PaperSummary[]> {
  const baseFilters = { ...filters }
  return readAllPages(
    (page, pageSize) => loadPage({ ...baseFilters, page, pageSize }),
    { pageSize: PAPER_HISTORY_PAGE_SIZE, maxPages: PAPER_HISTORY_MAX_PAGES, maxItems: PAPER_HISTORY_MAX_ITEMS },
    options,
    (code, reason, { maxItems, maxPages }) => {
      const messages = {
        parameters: '历史试卷分页读取参数无效。',
        response: '历史试卷返回了无效的分页数据，请重试。',
        limit: `历史试卷超过安全读取上限（${maxItems} 份或 ${maxPages} 页），请先缩小查询范围。`,
        total: '读取期间历史试卷发生变化，请重新加载。',
        duplicate: '读取期间历史试卷顺序发生变化，请重新加载。',
        incomplete: '历史试卷分页数据不完整，请重新加载。',
        pages: `历史试卷超过安全页数上限（${maxPages} 页），请先缩小查询范围。`,
      }
      return new PaperHistoryReadError(messages[reason], `PAPER_HISTORY_${code}`)
    },
  )
}
