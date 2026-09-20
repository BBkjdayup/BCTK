import type { QuestionStemSummary, RichContent } from '../types/domain'

type Entry = {
  id: string
  version: number
  resolve: (stem: RichContent) => void
  reject: (reason: Error) => void
}

// Per-scan cache: nearby visible rows share bounded batches, not full question reads.
export function createQuestionStemLoader(fetch: (ids: string[]) => Promise<QuestionStemSummary[]>) {
  const cache = new Map<string, Promise<RichContent>>()
  const queue: Entry[] = []
  let running = false
  let disposed = false

  async function drain() {
    if (running || disposed) return
    running = true
    try {
      while (queue.length && !disposed) {
        const batch = queue.splice(0, 16)
        try {
          const rows = await fetch([...new Set(batch.map((entry) => entry.id))])
          if (disposed) throw new Error('查重结果已更新')
          const byId = new Map(rows.map((row) => [row.id, row]))
          for (const entry of batch) {
            const row = byId.get(entry.id)
            if (!row || row.contentVersion !== entry.version) {
              cache.delete(`${entry.id}:${entry.version}`)
              entry.reject(new Error('题目已修改或移除，请重新检查'))
            } else entry.resolve(row.stem)
          }
        } catch {
          for (const entry of batch) {
            cache.delete(`${entry.id}:${entry.version}`)
            entry.reject(new Error('题干读取失败，可重试或重新检查'))
          }
        }
      }
    } finally { running = false }
  }

  return {
    load(id: string, version: number): Promise<RichContent> {
      if (disposed) return Promise.reject(new Error('查重结果已更新'))
      const key = `${id}:${version}`
      const existing = cache.get(key)
      if (existing) return existing
      const promise = new Promise<RichContent>((resolve, reject) => {
        queue.push({ id, version, resolve, reject })
      })
      cache.set(key, promise)
      if (cache.size > 256) cache.delete(cache.keys().next().value!)
      queueMicrotask(() => { void drain() })
      return promise
    },
    dispose() {
      disposed = true
      cache.clear()
      for (const entry of queue.splice(0)) entry.reject(new Error('查重结果已更新'))
    },
  }
}
