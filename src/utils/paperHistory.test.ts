import { describe, expect, it, vi } from 'vitest'
import type { PageResult, PaperFilters, PaperSummary } from '../types/domain'
import { fetchAllPaperSummaries } from './paperHistory'

function paper(id: string): PaperSummary {
  return {
    id,
    title: id,
    compositionMode: 'manual',
    status: 'saved',
    subjectSummaryText: '语文',
    questionCount: 10,
    rowVersion: 1,
    createdAt: 1,
    updatedAt: 1,
    savedAt: 1,
    lastSavedAt: 1,
  }
}

function filters(): PaperFilters {
  return { keyword: '', status: 'all', page: 9, pageSize: 500 }
}

function pageLoader(allPapers: PaperSummary[]) {
  return vi.fn(async (request: PaperFilters): Promise<PageResult<PaperSummary>> => {
    const start = (request.page - 1) * request.pageSize
    return {
      items: allPapers.slice(start, start + request.pageSize),
      total: allPapers.length,
      page: request.page,
      pageSize: request.pageSize,
    }
  })
}

describe('complete paper history paging', () => {
  it('loads every paper when history contains more than 500 records', async () => {
    const papers = Array.from({ length: 735 }, (_, index) => paper(`paper-${index + 1}`))
    const loadPage = pageLoader(papers)

    const result = await fetchAllPaperSummaries(loadPage, filters())

    expect(result.map((item) => item.id)).toEqual(papers.map((item) => item.id))
    expect(loadPage).toHaveBeenCalledTimes(8)
    expect(loadPage.mock.calls.map(([request]) => request.page)).toEqual([1, 2, 3, 4, 5, 6, 7, 8])
    expect(loadPage.mock.calls.every(([request]) => request.pageSize === 100)).toBe(true)
  })

  it('rejects a total that changes between pages', async () => {
    const loadPage = vi.fn(async (request: PaperFilters): Promise<PageResult<PaperSummary>> => ({
      items: Array.from({ length: request.page === 1 ? 100 : 1 }, (_, index) => (
        paper(`page-${request.page}-${index}`)
      )),
      total: request.page === 1 ? 101 : 102,
      page: request.page,
      pageSize: request.pageSize,
    }))

    await expect(fetchAllPaperSummaries(loadPage, filters())).rejects.toMatchObject({
      code: 'PAPER_HISTORY_PAGE_DATA_CHANGED',
    })
  })

  it('rejects an ID repeated on a later page', async () => {
    const loadPage = vi.fn(async (request: PaperFilters): Promise<PageResult<PaperSummary>> => ({
      items: request.page === 1
        ? Array.from({ length: 100 }, (_, index) => paper(`paper-${index}`))
        : [paper('paper-0')],
      total: 101,
      page: request.page,
      pageSize: request.pageSize,
    }))

    await expect(fetchAllPaperSummaries(loadPage, filters())).rejects.toMatchObject({
      code: 'PAPER_HISTORY_PAGE_DATA_CHANGED',
    })
  })

  it('rejects an empty page before reaching the reported total', async () => {
    const loadPage = vi.fn(async (request: PaperFilters): Promise<PageResult<PaperSummary>> => ({
      items: request.page === 1
        ? Array.from({ length: 100 }, (_, index) => paper(`paper-${index}`))
        : [],
      total: 101,
      page: request.page,
      pageSize: request.pageSize,
    }))

    await expect(fetchAllPaperSummaries(loadPage, filters())).rejects.toMatchObject({
      code: 'PAPER_HISTORY_PAGE_DATA_CHANGED',
    })
  })

  it('stops before paging when the reported total exceeds a safety limit', async () => {
    const loadPage = vi.fn(async (request: PaperFilters): Promise<PageResult<PaperSummary>> => ({
      items: Array.from({ length: 100 }, (_, index) => paper(`paper-${index}`)),
      total: 201,
      page: request.page,
      pageSize: request.pageSize,
    }))

    await expect(fetchAllPaperSummaries(loadPage, filters(), { maxItems: 200 })).rejects.toMatchObject({
      code: 'PAPER_HISTORY_PAGE_LIMIT_EXCEEDED',
    })
    expect(loadPage).toHaveBeenCalledTimes(1)
  })

  it('also enforces the configured maximum page count', async () => {
    const loadPage = vi.fn(async (request: PaperFilters): Promise<PageResult<PaperSummary>> => ({
      items: Array.from({ length: 100 }, (_, index) => paper(`paper-${index}`)),
      total: 201,
      page: request.page,
      pageSize: request.pageSize,
    }))

    await expect(fetchAllPaperSummaries(loadPage, filters(), { maxPages: 2 })).rejects.toMatchObject({
      code: 'PAPER_HISTORY_PAGE_LIMIT_EXCEEDED',
    })
    expect(loadPage).toHaveBeenCalledTimes(1)
  })
})
