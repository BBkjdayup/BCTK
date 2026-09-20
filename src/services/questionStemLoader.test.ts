import { describe, expect, it, vi } from 'vitest'
import { createQuestionStemLoader } from './questionStemLoader'
import type { QuestionStemSummary } from '../types/domain'

const row = (id: string, contentVersion = 1): QuestionStemSummary => ({
  id, contentVersion, stem: { schemaVersion: 1, html: '<p>题目</p>', plainText: '题目' },
})

describe('question stem loader', () => {
  it('batches and deduplicates visible rows with one active request', async () => {
    const fetch = vi.fn(async (ids: string[]) => ids.map((id) => row(id)))
    const loader = createQuestionStemLoader(fetch)
    const requests = Array.from({ length: 40 }, (_, index) => loader.load(String(index), 1))
    expect(loader.load('0', 1)).toBe(requests[0])
    await Promise.all(requests)
    expect(fetch.mock.calls.map(([ids]) => ids.length)).toEqual([16, 16, 8])
    await loader.load('0', 1)
    expect(fetch).toHaveBeenCalledTimes(3)
  })

  it('rejects stale versions and allows retry after a failed read', async () => {
    const fetch = vi.fn().mockResolvedValueOnce([row('q', 2)]).mockRejectedValueOnce(new Error('offline'))
      .mockResolvedValue([row('q')])
    const loader = createQuestionStemLoader(fetch)
    await expect(loader.load('q', 1)).rejects.toThrow('题目已修改')
    await expect(loader.load('q', 1)).rejects.toThrow('题干读取失败')
    await expect(loader.load('q', 1)).resolves.toMatchObject({ plainText: '题目' })
  })

  it('ignores a result from a disposed scan', async () => {
    let resolve!: (rows: QuestionStemSummary[]) => void
    const loader = createQuestionStemLoader(() => new Promise((done) => { resolve = done }))
    const result = expect(loader.load('q', 1)).rejects.toThrow()
    await Promise.resolve()
    loader.dispose()
    resolve([row('q')])
    await result
    await expect(loader.load('q', 1)).rejects.toThrow('查重结果已更新')
  })
})
