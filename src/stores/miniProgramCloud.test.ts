import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { flushPromises } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
const mocks = vi.hoisted(() => ({ login: vi.fn(), logout: vi.fn(), request: vi.fn(), getPaper: vi.fn() }))
vi.mock('../services/miniProgramCloud', () => ({ miniCloud: mocks }))
vi.mock('../services/backend', () => ({ backend: { getPaper: mocks.getPaper } }))
vi.mock('./app', () => ({ useAppStore: () => ({ questionTypes: [] }) }))
import { useMiniProgramCloudStore } from './miniProgramCloud'
afterEach(() => vi.useRealTimers())

for (const cancel of ['button', 'logout'] as const) it(`stops a waiting publication after ${cancel} without submitting a partial paper`, async () => {
  await import('../utils/miniProgramMedia')
  const store = useMiniProgramCloudStore(); await store.login('teacher', 'synthetic-only')
  vi.useFakeTimers()
  const field = { schemaVersion: 1, plainText: '', html: '<img src="data:image/png;base64,QQ==">' }
  mocks.getPaper.mockResolvedValue({ rowVersion: 1, status: 'saved', title: '图片试卷', items: [{ id: 'q', position: 0,
    snapshot: { type: 'short_answer', stem: field, answer: field, explanation: field, options: [] } }] })
  mocks.request.mockRejectedValueOnce(Object.assign(new Error('limited'), { status: 429, code: 'RATE_LIMITED', retryAfterSeconds: 60 }))
  const publishing = expect(store.publishPaper('paper', 1)).rejects.toThrow(/取消|切换/)
  await flushPromises()
  expect(store.publicationProgress?.phase).toBe('waiting')
  if (cancel === 'button') store.cancelPublication(); else await store.logout()
  await publishing
  await vi.advanceTimersByTimeAsync(60_000)
  expect(mocks.request.mock.calls.filter(([method]) => method === 'PUT')).toHaveLength(0)
  expect(store.publicationProgress).toBeNull()
  expect(store.busy).toBe(false)
})

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
  expect(mocks.request).toHaveBeenCalledWith('GET', '/management/state')
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

const pagedState = () => ({ ...cloudState(), members: [], capabilities: { pagedManagement: true, mediaCache: true }, counts: { members: 200, invites: 505, pending: 99, countedMembers: 1 } })
it('loads paged lists on demand and refreshes only members and quota after approval', async () => {
  mocks.request.mockImplementation(async (method: string, path: string, body: unknown) => {
    if (method === 'GET') return pagedState()
    if (path.endsWith('/search')) return { items: [], total: 200, ...(body as object) }
    return { ok: true }
  })
  const store = useMiniProgramCloudStore(); await store.login('teacher', 'synthetic-only')
  expect(store.pendingCount).toBe(99); expect(store.members).toEqual([])
  const query = { keyword: '学员', status: 'pending', page: 2, pageSize: 20 }
  await store.loadManagement('members', query)
  mocks.request.mockClear()
  await store.approveMember('member-id')
  expect(mocks.request.mock.calls).toEqual([
    ['PUT', '/members/member-id', { status: 'active' }],
    ['GET', '/management/state?omitPublications=true'],
    ['POST', '/members/search', query],
  ])
  expect(store.totals.members).toBe(200)
})
it('discards pages from superseded filters and switched accounts', async () => {
  mocks.request.mockResolvedValue(pagedState())
  const store = useMiniProgramCloudStore(); await store.login('teacher', 'synthetic-only')
  let resolve!: (v: unknown) => void
  mocks.request.mockImplementationOnce(() => new Promise(r => { resolve = r }))
  const query = { keyword: '', status: 'all', page: 1, pageSize: 20 }
  const pending = store.loadManagement('members', query)
  store.invalidateManagement('members')
  resolve({ items: [{ id: 'old' }], total: 100, ...query }); await pending
  expect(store.members).toEqual([])
  mocks.request.mockImplementationOnce(() => new Promise(r => { resolve = r }))
  const older = store.loadManagement('invites', query)
  await store.logout(); await store.login('teacher', 'synthetic-only')
  resolve({ items: [{ id: 'other-account' }], total: 1, ...query }); await older
  expect(store.invites).toEqual([])
})
it('falls back only for the explicit old-server response, not a network or authorization failure', async () => {
  mocks.request.mockRejectedValueOnce('MINI_PAGED_MANAGEMENT_UNAVAILABLE: old server').mockResolvedValueOnce(cloudState())
  const store = useMiniProgramCloudStore(); await store.login('teacher', 'synthetic-only')
  expect(store.managementMode).toBe('legacy')
  expect(mocks.request.mock.calls.map(c => c[1])).toEqual(['/management/state', '/state?activePublications=true'])
  await store.logout(); mocks.request.mockClear(); mocks.request.mockRejectedValueOnce(new Error('offline'))
  await expect(store.login('teacher', 'synthetic-only')).rejects.toThrow('offline')
  expect(mocks.request).toHaveBeenCalledTimes(1); expect(store.ready).toBe(false)
})
