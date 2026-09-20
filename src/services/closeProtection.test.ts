import { describe, expect, it, vi } from 'vitest'
import { canCloseApplication, registerCloseGuard } from './closeProtection'

describe('ordered application close protection', () => {
  it('checks every editor and cancels exit when the paper guard refuses', async () => {
    const calls: string[] = []
    const removePaper = registerCloseGuard(() => { calls.push('paper'); return false }, 100)
    const removeQuestion = registerCloseGuard(() => { calls.push('question'); return true })
    try {
      expect(await canCloseApplication()).toBe(false)
      expect(calls).toEqual(['question', 'paper'])
    } finally { removePaper(); removeQuestion() }
  })
  it('deduplicates close requests while a save or prompt is pending', async () => {
    let finish!: (value: boolean) => void
    const guard = vi.fn(() => new Promise<boolean>((resolve) => { finish = resolve }))
    const remove = registerCloseGuard(guard)
    try {
      const first = canCloseApplication()
      const second = canCloseApplication()
      finish(true)
      expect(await first).toBe(true)
      expect(await second).toBe(true)
      expect(guard).toHaveBeenCalledOnce()
    } finally { remove() }
  })
})
