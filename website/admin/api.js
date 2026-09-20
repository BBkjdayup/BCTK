export class ApiError extends Error {
  constructor(message, code, status) { super(message); this.code = code; this.status = status }
}

// Session cookies are HttpOnly. Only the CSRF value stays in this module's memory.
export function createAdminClient(fetcher = globalThis.fetch.bind(globalThis)) {
  let csrf = ''
  return {
    clear() { csrf = '' },
    async request(path, method = 'GET', data) {
      if (!path.startsWith('/') || path.startsWith('//')) throw new Error('接口路径无效')
      const headers = { Accept: 'application/json', 'X-TK-Admin': '1' }
      if (method !== 'GET') {
        headers['Content-Type'] = 'application/json'
        if (csrf) headers['X-CSRF-Token'] = csrf
      }
      let response
      try {
        response = await fetcher(`/admin-api/v1${path}`, {
          method, headers, credentials: 'same-origin', cache: 'no-store', redirect: 'error',
          signal: AbortSignal.timeout(15000),
          ...(method !== 'GET' ? { body: JSON.stringify(data ?? {}) } : {}),
        })
      } catch {
        throw new ApiError(method === 'GET' ? '连接失败，请稍后重试' : '未能确认保存结果，请先刷新数据核对，避免重复操作', 'NETWORK', 0)
      }
      let body
      try { body = await response.json() } catch { body = null }
      if (!response.ok) throw new ApiError(body?.error?.message || `请求失败（${response.status}）`, body?.error?.code || 'HTTP_ERROR', response.status)
      if (body?.csrf_token) csrf = body.csrf_token
      return body
    },
  }
}
