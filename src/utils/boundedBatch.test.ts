import { describe, expect, it, vi } from 'vitest'
import { chunked, withTimeout } from './boundedBatch'

describe('bounded batch processing', () => {
  it('splits a 1200 item import into twelve backend-sized requests', () => {
    const source = Array.from({ length: 1_200 }, (_, index) => index)
    const chunks = chunked(source, 100)

    expect(chunks).toHaveLength(12)
    expect(chunks.every((chunk) => chunk.length === 100)).toBe(true)
    expect(chunks.flat()).toEqual(source)
  })

  it('releases a permanently pending request after the configured timeout', async () => {
    vi.useFakeTimers()
    try {
      const pending = withTimeout(new Promise<never>(() => undefined), 30_000, '检查超时')
      const assertion = expect(pending).rejects.toThrow('检查超时')
      await vi.advanceTimersByTimeAsync(30_000)
      await assertion
    } finally {
      vi.useRealTimers()
    }
  })
})
