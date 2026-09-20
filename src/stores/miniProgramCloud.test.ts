import { beforeEach, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
const mocks = vi.hoisted(() => ({ login: vi.fn(), logout: vi.fn(), request: vi.fn(), getPaper: vi.fn() }))
vi.mock('../services/miniProgramCloud', () => ({ miniCloud: mocks }))
vi.mock('../services/backend', () => ({ backend: { getPaper: mocks.getPaper } }))
vi.mock('./app', () => ({ useAppStore: () => ({ questionTypes: [] }) }))
import { useMiniProgramCloudStore } from './miniProgramCloud'

const cloudState = () => ({ title: '老师的题库', settings: { quota: 10, usedQuota: 7, serviceStarts: '2026-09-01', serviceEnds: '2026-09-30', paused: false }, publications: [], invites: [], members: [{ id: 'one', status: 'stopped', countedInCycle: true }] })
beforeEach(() => { setActivePinia(createPinia()); vi.resetAllMocks(); mocks.login.mockResolvedValue({ id: 'owner', username: 'teacher' }); mocks.request.mockResolvedValue(cloudState()) })
it('uses server quota instead of local demo state and never writes credentials to localStorage', async () => {
  const write = vi.spyOn(Storage.prototype, 'setItem')
  const store = useMiniProgramCloudStore(); expect(store.settings.quota).toBe(0)
  await store.login('teacher', 'synthetic-only')
  expect(store.usedQuota).toBe(7); expect(store.remainingQuota).toBe(3); expect(store.countedMembers).toHaveLength(1)
  expect(write).not.toHaveBeenCalled(); write.mockRestore()
  expect(() => store.addMember({ name: 'fake', account: 'fake', inviteId: 'fake' })).toThrow('微信小程序')
})
it('does not mark a failed publication or member approval successful locally', async () => {
  const store = useMiniProgramCloudStore(); await store.login('teacher', 'synthetic-only')
  mocks.request.mockRejectedValue(new Error('quota reached'))
  await expect(store.approveMember('one')).rejects.toThrow('quota reached')
  expect(store.members[0]!.status).toBe('stopped'); expect(store.usedQuota).toBe(7)
  mocks.getPaper.mockResolvedValue({ rowVersion: 3 })
  await expect(store.publishPaper('paper', 2)).rejects.toThrow('试卷已变更')
  expect(store.publications).toHaveLength(0)
})
it('clears local owner state even when logout cannot reach the server', async () => {
  const store = useMiniProgramCloudStore(); await store.login('teacher', 'synthetic-only')
  mocks.logout.mockRejectedValue(new Error('offline')); await expect(store.logout()).rejects.toThrow('offline')
  expect(store.user).toBeNull(); expect(store.members).toEqual([]); expect(store.settings.quota).toBe(0)
})

it('withdraws a cloud-only paper without reading or uploading a local original', async () => {
  const publication = { paperId: 'remote-id', title: '另一台电脑发布', paperRowVersion: 4, questionCount: 5, publishedAt: 10, published: true }
  mocks.request.mockResolvedValueOnce({ ...cloudState(), publications: [publication] })
  const store = useMiniProgramCloudStore(); await store.login('teacher', 'synthetic-only')
  expect(mocks.request).toHaveBeenCalledWith('GET', '/state?activePublications=true')
  expect(store.publications[0]?.title).toBe('另一台电脑发布')
  await store.withdrawPaper('remote-id')
  expect(mocks.request).toHaveBeenCalledWith('DELETE', '/publications/remote-id')
  expect(mocks.getPaper).not.toHaveBeenCalled()
  expect(store.publications).toEqual([])
})

it('loads withdrawn history only on request and keeps it separate from current publications', async () => {
  const store = useMiniProgramCloudStore(); await store.login('teacher', 'synthetic-only')
  expect(mocks.request).toHaveBeenCalledTimes(1)
  mocks.request.mockResolvedValueOnce({ items: [{ paperId: 'old-id', published: false }], total: 21, page: 2, pageSize: 20 })
  const page = await store.searchWithdrawn('旧试卷', 2, 20)
  expect(mocks.request).toHaveBeenLastCalledWith('POST', '/publications/search', { published: false, keyword: '旧试卷', page: 2, pageSize: 20 })
  expect(page.total).toBe(21); expect(store.publications).toEqual([])
})

it('marks stale state unconfirmed and blocks publication changes after refresh failure', async () => {
  const store = useMiniProgramCloudStore(); await store.login('teacher', 'synthetic-only')
  mocks.request.mockRejectedValueOnce(new Error('offline'))
  await expect(store.refresh()).rejects.toThrow('offline')
  expect(store.ready).toBe(false)
  await expect(store.withdrawPaper('id')).rejects.toThrow('云端状态未确认')
  expect(mocks.request).toHaveBeenCalledTimes(2)
})

it('discards previous-account refresh and history responses after logout and login', async () => {
  const store = useMiniProgramCloudStore(); await store.login('teacher', 'synthetic-only')
  let resolveState!: (value: unknown) => void
  let resolveHistory!: (value: unknown) => void
  mocks.request.mockImplementationOnce(() => new Promise(resolve => { resolveState = resolve }))
    .mockImplementationOnce(() => new Promise(resolve => { resolveHistory = resolve }))
  const refresh = store.refresh()
  const history = store.searchWithdrawn('', 1, 20)
  await store.logout()
  mocks.login.mockResolvedValueOnce({ id: 'different-owner', username: 'different' })
  await store.login('different', 'synthetic-only')
  resolveState({ ...cloudState(), title: '旧账号题库' })
  resolveHistory({ items: [{ paperId: 'private-old-id' }], total: 1 })
  await refresh
  await expect(history).rejects.toThrow('账号已切换')
  expect(store.title).toBe('老师的题库'); expect(store.publications).toEqual([])
})
