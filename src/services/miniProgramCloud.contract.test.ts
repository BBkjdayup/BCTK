import { beforeEach, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }))
vi.mock('@tauri-apps/api/core', () => ({ invoke }))
vi.mock('../stores/app', () => ({ useAppStore: () => ({ questionTypes: [] }) }))
import { useMiniProgramCloudStore } from '../stores/miniProgramCloud'
import { miniCloud } from './miniProgramCloud'

const memberId = '00000000-0000-4000-8000-000000000001'
const member = { id: memberId, status: 'active', countedInCycle: true }
const state = (removed: boolean) => ({
  title: '接口测试题库', bankId: 'synthetic-bank', publications: [], invites: [],
  settings: { quota: 1, usedQuota: removed ? 0 : 1, serviceStarts: '', serviceEnds: '', paused: false },
  members: removed ? [] : [member],
})

beforeEach(() => {
  setActivePinia(createPinia())
  vi.resetAllMocks()
  invoke.mockResolvedValueOnce({ id: 'synthetic-owner', username: 'teacher' })
    .mockResolvedValueOnce(state(false))
})

it('preserves the native rate-limit status and retry delay for resumable uploads', async () => {
  invoke.mockReset().mockRejectedValueOnce('TK_MINI_HTTP_ERROR:{"status":429,"code":"RATE_LIMITED","retryAfterSeconds":7}')
  await expect(miniCloud.request('POST', '/assets', {})).rejects.toMatchObject({ status: 429, code: 'RATE_LIMITED', retryAfterSeconds: 7 })
  invoke.mockRejectedValueOnce('TK_MINI_HTTP_ERROR:{"status":429,"code":"RATE_LIMITED","retryAfterSeconds":60}')
  await expect(miniCloud.login('teacher', 'synthetic-only')).rejects.toThrow('操作过于频繁，请稍后重试')
  invoke.mockRejectedValueOnce('MINI_PAGED_MANAGEMENT_UNAVAILABLE: legacy')
  await expect(miniCloud.request('GET', '/management/state')).rejects.toBe('MINI_PAGED_MANAGEMENT_UNAVAILABLE: legacy')
})

it('passes removal through the real store and service to native IPC, then refreshes quota and members', async () => {
  const store = useMiniProgramCloudStore()
  await store.login('teacher', 'synthetic-password')
  invoke.mockResolvedValueOnce({ ok: true }).mockResolvedValueOnce(state(true))
  await expect(store.removeMember(memberId)).resolves.toBe(true)
  expect(invoke.mock.calls.slice(2)).toEqual([
    ['mini_cloud_request', { method: 'DELETE', path: `/mini-owner/members/${memberId}`, body: {} }],
    ['mini_cloud_request', { method: 'GET', path: '/mini-owner/state?activePublications=true', body: {} }],
  ])
  expect(store.members).toEqual([])
  expect(store.usedQuota).toBe(0)
  expect(store.remainingQuota).toBe(1)
  expect(store.ready).toBe(true)
})

it('does not remove a member locally when the native or cloud request fails', async () => {
  const store = useMiniProgramCloudStore()
  await store.login('teacher', 'synthetic-password')
  invoke.mockRejectedValueOnce(new Error('无权移除此成员'))
  await expect(store.removeMember(memberId)).rejects.toThrow('无权移除此成员')
  expect(store.members).toEqual([member])
  expect(store.usedQuota).toBe(1)
  expect(invoke).toHaveBeenCalledTimes(3)
  expect(store.busy).toBe(false)
})

it('marks state unconfirmed if removal succeeds but the following refresh fails', async () => {
  const store = useMiniProgramCloudStore()
  await store.login('teacher', 'synthetic-password')
  invoke.mockResolvedValueOnce({ ok: true }).mockRejectedValueOnce(new Error('offline'))
  await expect(store.removeMember(memberId)).rejects.toThrow('操作已提交，但列表刷新失败')
  expect(store.ready).toBe(false)
  await expect(store.removeMember(memberId)).rejects.toThrow('云端状态未确认')
  expect(invoke).toHaveBeenCalledTimes(4)
})

it('carries paged management requests through the service and refreshes only the affected list', async () => {
  const overview = (removed: boolean) => ({ ...state(removed), members: [], capabilities: { pagedManagement: true, mediaCache: true }, counts: { pending: 0, members: removed ? 0 : 1, invites: 0, countedMembers: removed ? 0 : 1 } })
  const query = { keyword: '', status: 'all', page: 1, pageSize: 20 }
  invoke.mockReset()
  invoke.mockResolvedValueOnce({ id: 'synthetic-owner', username: 'teacher' })
    .mockResolvedValueOnce(overview(false))
    .mockResolvedValueOnce({ items: [member], total: 1, ...query })
  const store = useMiniProgramCloudStore()
  await store.login('teacher', 'synthetic-password')
  await store.loadManagement('members', query)
  expect(invoke.mock.calls.slice(1)).toEqual([
    ['mini_cloud_request', { method: 'GET', path: '/mini-owner/management/state', body: {} }],
    ['mini_cloud_request', { method: 'POST', path: '/mini-owner/members/search', body: query }],
  ])
  invoke.mockClear()
  invoke.mockResolvedValueOnce({ ok: true }).mockResolvedValueOnce(overview(true))
    .mockResolvedValueOnce({ items: [], total: 0, ...query })
  await store.removeMember(memberId)
  expect(invoke.mock.calls).toEqual([
    ['mini_cloud_request', { method: 'DELETE', path: `/mini-owner/members/${memberId}`, body: {} }],
    ['mini_cloud_request', { method: 'GET', path: '/mini-owner/management/state?omitPublications=true', body: {} }],
    ['mini_cloud_request', { method: 'POST', path: '/mini-owner/members/search', body: query }],
  ])
  expect(store.usedQuota).toBe(0)
  expect(store.members).toEqual([])
})
