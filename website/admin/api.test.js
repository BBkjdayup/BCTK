import { describe, expect, it, vi } from 'vitest'
import { createAdminClient } from './api.js'

describe('administrator API client', () => {
  it('uses same-origin cookies and memory-only CSRF protection for mutations', async () => {
    const fetcher = vi.fn().mockResolvedValueOnce({ ok: true, json: async () => ({ csrf_token: 'csrf-test' }) })
      .mockResolvedValue({ ok: true, json: async () => ({ ok: true }) })
    const client = createAdminClient(fetcher)
    await client.request('/session')
    await client.request('/users/owner/grant', 'PUT', { seat_limit: 50 })
    const [url, options] = fetcher.mock.calls[1]
    expect(url).toBe('/admin-api/v1/users/owner/grant')
    expect(options).toMatchObject({ credentials: 'same-origin', cache: 'no-store', redirect: 'error',
      headers: { 'X-TK-Admin': '1', 'X-CSRF-Token': 'csrf-test', 'Content-Type': 'application/json' } })
    expect(options.headers.Authorization).toBeUndefined()
    client.clear()
    await client.request('/session/logout', 'POST')
    expect(fetcher.mock.calls[2][1].headers['X-CSRF-Token']).toBeUndefined()
  })
  it('preserves server conflict and step-up errors', async () => {
    const client = createAdminClient(async () => ({ ok: false, status: 409,
      json: async () => ({ error: { code: 'CONFLICT', message: '数据已变更' } }) }))
    await expect(client.request('/plans')).rejects.toMatchObject({ status: 409, code: 'CONFLICT', message: '数据已变更' })
  })
  it('does not retry writes with an uncertain outcome', async () => {
    const fetcher = vi.fn().mockRejectedValue(new Error('connection lost'))
    const client = createAdminClient(fetcher)
    await expect(client.request('/plans', 'POST', {})).rejects.toMatchObject({ code: 'NETWORK' })
    expect(fetcher).toHaveBeenCalledTimes(1)
  })
})
