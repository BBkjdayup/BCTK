import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import MiniProgramManagementView from './MiniProgramManagementView.vue'
import { useMiniProgramCloudStore } from '../stores/miniProgramCloud'

const mocks = vi.hoisted(() => ({ request: vi.fn(), login: vi.fn(), logout: vi.fn(), listPapers: vi.fn(), getPaper: vi.fn(), confirm: vi.fn() }))
vi.mock('../services/miniProgramCloud', () => ({ miniCloud: mocks }))
vi.mock('../services/backend', () => ({ backend: mocks, isDesktopRuntime: () => true }))
vi.mock('../stores/app', () => ({ useAppStore: () => ({ questionTypes: [] }) }))
vi.mock('element-plus', async original => ({
  ...await original<typeof import('element-plus')>(),
  ElMessage: { success: vi.fn(), error: vi.fn(), warning: vi.fn() },
  ElMessageBox: { confirm: mocks.confirm },
}))

const publication = (paperId = 'remote') => ({ paperId, title: '另一台电脑发布的试卷', questionCount: 5, paperRowVersion: 2, published: true, publishedAt: Date.now() })
let active = [publication()]
let history = [{ ...publication('old'), title: '旧的撤回记录', published: false }]
let wrapper: VueWrapper | undefined
const state = () => ({ title: '我的题库', settings: { quota: 10, usedQuota: 0, serviceStarts: '2020-01-01', serviceEnds: '2099-12-31', paused: false }, publications: active, invites: [], members: [] })

beforeEach(() => {
  vi.clearAllMocks()
  vi.stubGlobal('ResizeObserver', class { observe() {} unobserve() {} disconnect() {} })
  setActivePinia(createPinia())
  active = [publication()]; history = [{ ...publication('old'), title: '旧的撤回记录', published: false }]
  mocks.login.mockResolvedValue({ id: 'teacher', username: 'teacher' })
  mocks.confirm.mockResolvedValue('confirm')
  mocks.listPapers.mockResolvedValue({ items: [], total: 0, page: 1, pageSize: 500 })
  mocks.request.mockImplementation(async (method: string, path: string, body?: { page: number; pageSize: number; keyword: string }) => {
    if (method === 'GET') return state()
    if (method === 'DELETE') { history.unshift(...active.map(p => ({ ...p, published: false }))); active = []; return { ok: true } }
    if (path === '/publications/search' && body) {
      const found = history.filter(p => p.title.includes(body.keyword))
      return { items: found.slice((body.page - 1) * body.pageSize, body.page * body.pageSize), total: found.length, page: body.page, pageSize: body.pageSize }
    }
    throw new Error('Unexpected request')
  })
})
afterEach(() => { wrapper?.unmount(); wrapper = undefined; vi.unstubAllGlobals() })

async function render() {
  await useMiniProgramCloudStore().login('teacher', 'synthetic-only')
  wrapper = mount(MiniProgramManagementView, { global: { stubs: { QuestionStemSummary: true } } })
  await flushPromises()
  return wrapper
}
async function showHistory(view: VueWrapper) {
  await view.get('input[type="radio"][value="withdrawn"]').setValue()
  await new Promise(resolve => setTimeout(resolve, 230))
  await flushPromises()
}

it('withdraws a cloud-only row and hides it by default while keeping it searchable in history', async () => {
  const view = await render()
  expect(view.text()).toContain('另一台电脑发布的试卷')
  expect(view.text()).toContain('本机无原稿')
  expect(view.text()).not.toContain('旧的撤回记录')
  expect(mocks.request.mock.calls.some(call => call[1] === '/publications/search')).toBe(false)
  await view.findAll('button').find(button => button.text() === '取消公开')!.trigger('click')
  await flushPromises()
  expect(mocks.request).toHaveBeenCalledWith('DELETE', '/publications/remote')
  expect(mocks.getPaper).not.toHaveBeenCalled()
  expect(view.text()).not.toContain('另一台电脑发布的试卷')
  await showHistory(view)
  expect(view.text()).toContain('另一台电脑发布的试卷')
  expect(view.text()).toContain('旧的撤回记录')
  expect(view.findAll('button').some(button => button.text() === '取消公开')).toBe(false)
})

it('offers both update and withdrawal when the local original is newer', async () => {
  mocks.listPapers.mockResolvedValue({ items: [{ id: 'remote', title: '本机改名', questionCount: 6, rowVersion: 3, updatedAt: Date.now(), status: 'saved' }], total: 1, page: 1, pageSize: 500 })
  const view = await render()
  expect(view.findAll('tr.el-table__row')).toHaveLength(1)
  expect(view.text()).toContain('本机名称：本机改名')
  const buttons = view.findAll('button').map(button => button.text())
  expect(buttons).toContain('更新公开'); expect(buttons).toContain('取消公开')
  await view.findAll('button').find(button => button.text() === '取消公开')!.trigger('click')
  await flushPromises()
  expect(view.text()).toContain('本机改名'); expect(view.text()).toContain('未公开')
})

it('requests a second history page instead of loading all withdrawn records into the table', async () => {
  history = Array.from({ length: 25 }, (_, i) => ({ ...publication(`old-${i}`), title: `历史试卷-${i}`, published: false }))
  const view = await render(); await showHistory(view)
  expect(view.findAll('tr.el-table__row')).toHaveLength(20)
  await view.get('button.btn-next').trigger('click')
  await new Promise(resolve => setTimeout(resolve, 230)); await flushPromises()
  expect(mocks.request).toHaveBeenLastCalledWith('POST', '/publications/search', { published: false, keyword: '', page: 2, pageSize: 20 })
  expect(view.findAll('tr.el-table__row')).toHaveLength(5)
  expect(view.text()).toContain('历史试卷-24')
})

it('still shows remote publications if the local paper database cannot be read', async () => {
  mocks.listPapers.mockRejectedValue(new Error('Local database unavailable'))
  const view = await render()
  expect(view.text()).toContain('另一台电脑发布的试卷')
  expect(view.findAll('button').find(button => button.text() === '取消公开')!.attributes('disabled')).toBeUndefined()
})
