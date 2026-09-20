import { mount, flushPromises, type VueWrapper } from '@vue/test-utils'
import { afterEach, expect, it, vi } from 'vitest'
import SupportContactDialog from './SupportContactDialog.vue'

const request = vi.hoisted(() => vi.fn())
vi.mock('../services/miniProgramCloud', () => ({ miniCloud: { request } }))
let wrapper: VueWrapper | undefined
afterEach(() => { wrapper?.unmount(); wrapper = undefined; vi.unstubAllGlobals(); vi.restoreAllMocks() })
const remote = { display_name: '更新后的客服', wechat_id: 'TK_support_02', service_hours: '工作日 9:00—18:00', qr_code: 'data:image/png;base64,aGVsbG8=', version: 2 }
async function render(account = 'teacher_01') {
  wrapper = mount(SupportContactDialog, { props: { modelValue: true, account, live: true }, global: { stubs: { teleport: true } } })
  await flushPromises()
  return wrapper
}
it('loads maintained contact details before login and does not show a fake account', async () => {
  request.mockResolvedValue(remote)
  const view = await render('')
  expect(request).toHaveBeenCalledWith('GET', '/support-contact')
  expect(view.text()).toContain('更新后的客服')
  expect(view.text()).toContain('请先登录小程序管理账号')
  expect(view.findAll('button').some(b => b.text() === '复制账号')).toBe(false)
})
it('copies the current software account and requests an authorization refresh', async () => {
  request.mockResolvedValue(remote)
  const copy = vi.fn().mockResolvedValue(undefined)
  Object.defineProperty(navigator, 'clipboard', { configurable: true, value: { writeText: copy } })
  const view = await render()
  await view.findAll('button').find(b => b.text() === '复制微信号')!.trigger('click')
  expect(copy).toHaveBeenLastCalledWith('TK_support_02')
  await view.findAll('button').find(b => b.text() === '复制账号')!.trigger('click')
  expect(copy).toHaveBeenLastCalledWith('teacher_01')
  await view.findAll('button').find(b => b.text() === '刷新授权')!.trigger('click')
  expect(view.emitted('refresh')).toHaveLength(1)
})
it('rejects remote image URLs and keeps a usable bundled contact if the service is unavailable', async () => {
  request.mockResolvedValue({ ...remote, qr_code: 'https://untrusted.example/tracker.svg' })
  const view = await render()
  expect(view.text()).toContain('Ai_chatgpt_01')
  expect(view.text()).toContain('暂时无法更新联系方式')
  expect(view.get('img').attributes('src')).not.toContain('untrusted.example')
})
