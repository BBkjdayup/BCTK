import type { PageResult } from '../types/domain'

export interface PageReadLimits {
  pageSize: number
  maxPages: number
  maxItems: number
}

export type PageReadOptions = Partial<PageReadLimits>
type FailureCode = 'PAGE_RESPONSE_INVALID' | 'PAGE_DATA_CHANGED' | 'PAGE_LIMIT_EXCEEDED'
type FailureReason = 'parameters' | 'response' | 'limit' | 'total' | 'duplicate' | 'incomplete' | 'pages'
type ErrorFactory = (code: FailureCode, reason: FailureReason, limits: PageReadLimits) => Error

/** Offset pagination must stop if the dataset changes, rather than return a partial selection. */
export async function readAllPages<T extends { id: string }>(
  loadPage: (page: number, pageSize: number) => Promise<PageResult<T>>,
  defaults: PageReadLimits,
  options: PageReadOptions,
  error: ErrorFactory,
): Promise<T[]> {
  const positiveInteger = (value: number | undefined, fallback: number, maximum?: number) => {
    if (value === undefined) return fallback
    if (!Number.isSafeInteger(value) || value < 1 || (maximum !== undefined && value > maximum)) {
      throw error('PAGE_RESPONSE_INVALID', 'parameters', defaults)
    }
    return value
  }
  const limits = {
    pageSize: positiveInteger(options.pageSize, defaults.pageSize, defaults.pageSize),
    maxPages: positiveInteger(options.maxPages, defaults.maxPages),
    maxItems: positiveInteger(options.maxItems, defaults.maxItems),
  }
  const { pageSize, maxPages, maxItems } = limits
  const items: T[] = []
  const seenIds = new Set<string>()
  let expectedTotal: number | null = null
  for (let page = 1; page <= maxPages; page += 1) {
    const result = await loadPage(page, pageSize)
    if (!Number.isSafeInteger(result.total) || result.total < 0
      || result.page !== page || result.pageSize !== pageSize
      || !Array.isArray(result.items) || result.items.length > pageSize) {
      throw error('PAGE_RESPONSE_INVALID', 'response', limits)
    }
    if (expectedTotal === null) {
      expectedTotal = result.total
      if (expectedTotal > maxItems || Math.ceil(expectedTotal / pageSize) > maxPages) {
        throw error('PAGE_LIMIT_EXCEEDED', 'limit', limits)
      }
    } else if (result.total !== expectedTotal) {
      throw error('PAGE_DATA_CHANGED', 'total', limits)
    }
    for (const item of result.items) {
      if (!item.id || seenIds.has(item.id)) throw error('PAGE_DATA_CHANGED', 'duplicate', limits)
      seenIds.add(item.id)
      items.push(item)
    }
    if (items.length === expectedTotal) return items
    if (items.length > expectedTotal || result.items.length < pageSize) {
      throw error('PAGE_DATA_CHANGED', 'incomplete', limits)
    }
  }
  throw error('PAGE_LIMIT_EXCEEDED', 'pages', limits)
}
