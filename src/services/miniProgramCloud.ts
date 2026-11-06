import { invoke } from '@tauri-apps/api/core'

function decodeError(reason: unknown): unknown {
  if (typeof reason !== 'string' || !reason.startsWith('TK_MINI_HTTP_ERROR:')) return reason
  try {
    const error = JSON.parse(reason.slice('TK_MINI_HTTP_ERROR:'.length))
    if (error.status === 429 && error.code === 'RATE_LIMITED') {
      const seconds = Number(error.retryAfterSeconds)
      return Object.assign(new Error('操作过于频繁，请稍后重试'), {
        status: 429, code: 'RATE_LIMITED',
        retryAfterSeconds: Number.isFinite(seconds) ? Math.max(1, Math.min(120, Math.ceil(seconds))) : 60,
      })
    }
  } catch { /* Preserve unknown native errors for the usual error display. */ }
  return reason
}

export const miniCloud = {
  login: async (account: string, password: string, register = false) => {
    try { return await invoke<{ id: string; username: string }>('mini_cloud_login', { account, password, register }) }
    catch (reason) { throw decodeError(reason) }
  },
  logout: async () => {
    try { return await invoke<void>('mini_cloud_logout') }
    catch (reason) { throw decodeError(reason) }
  },
  request: async <T>(method: string, path: string, body: unknown = {}) => {
    try { return await invoke<T>('mini_cloud_request', { method, path: '/mini-owner' + path, body }) }
    catch (reason) { throw decodeError(reason) }
  },
}
