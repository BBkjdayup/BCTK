import { invoke } from '@tauri-apps/api/core'

export const miniCloud = {
  login: (account: string, password: string, register = false) => invoke<{ id: string; username: string }>('mini_cloud_login', { account, password, register }),
  logout: () => invoke<void>('mini_cloud_logout'),
  request: <T>(method: string, path: string, body: unknown = {}) => invoke<T>('mini_cloud_request', { method, path: '/mini-owner' + path, body }),
}
