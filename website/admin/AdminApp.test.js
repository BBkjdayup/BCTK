import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import AdminApp from './AdminApp.vue'

let wrapper
const dialogDescriptors = Object.fromEntries(['showModal', 'close'].map(key => [key, Object.getOwnPropertyDescriptor(HTMLDialogElement.prototype, key)]))
const owner = { id: '11111111-1111-4111-8111-111111111111', username: '<img src=x onerror=alert(1)>',
  email: null, account_status: 'active', plan_id: 'p50', plan_name: '50 人包', seat_limit: 50,
  starts_on: '2026-09-01', ends_on: '2027-08-31', paused: false, version: 1, status: 'active' }
const session = { username: '测试管理员', csrf_token: 'test-csrf', expires_at: new Date(Date.now() + 7200000).toISOString(),
  write_authorized_until: new Date(Date.now() + 600000).toISOString() }

beforeEach(() => {
  Object.defineProperty(HTMLDialogElement.prototype, 'showModal', { configurable: true, value() { this.open = true } })
  Object.defineProperty(HTMLDialogElement.prototype, 'close', { configurable: true, value() { this.open = false } })
  vi.stubGlobal('fetch', vi.fn(async (url) => ({ ok: true, status: 200,
    json: async () => url.endsWith('/session') ? session : url.endsWith('/plans') ?
      { items: [{ id: 'p50', name: '50 人包', kind: 'fixed', seats: 50, enabled: true, version: 1 }] } :
      { items: [{ ...owner }], page: 1, has_more: false } })))
})
afterEach(() => {
  wrapper?.unmount(); vi.restoreAllMocks(); vi.unstubAllGlobals()
  for (const [key, descriptor] of Object.entries(dialogDescriptors)) {
    if (descriptor) Object.defineProperty(HTMLDialogElement.prototype, key, descriptor)
    else Reflect.deleteProperty(HTMLDialogElement.prototype, key)
  }
})

describe('administrator console', () => {
  it('saves contact changes with a revision and preserves the QR image', async () => {
    const contact = { display_name: 'TK试题题库', wechat_id: 'Ai_chatgpt_01', service_hours: '', qr_code: 'data:image/png;base64,aGVsbG8=', version: 3 }
    fetch.mockImplementation(async url => ({ ok: true, status: 200, json: async () => url.endsWith('/session') ? session : url.endsWith('/support-contact') ? contact : { items: [], has_more: false } }))
    wrapper = mount(AdminApp); await flushPromises()
    await wrapper.findAll('button').find(b => b.text() === '客服信息').trigger('click'); await flushPromises()
    expect(wrapper.find('.pagination').exists()).toBe(false)
    await wrapper.findAll('button').find(b => b.text() === '编辑客服信息').trigger('click'); await flushPromises()
    await wrapper.get('input[name="wechat_id"]').setValue('TK_support_02')
    await wrapper.get('dialog textarea').setValue('更新工作微信')
    await wrapper.get('dialog form').trigger('submit'); await flushPromises()
    expect(wrapper.get('dialog').text()).toContain('Ai_chatgpt_01')
    expect(wrapper.get('dialog').text()).toContain('TK_support_02')
    await wrapper.get('dialog form').trigger('submit'); await flushPromises()
    const save = fetch.mock.calls.find(([, options]) => options.method === 'PUT')
    expect(save[0]).toBe('/admin-api/v1/support-contact')
    expect(JSON.parse(save[1].body)).toMatchObject({ wechat_id: 'TK_support_02', expected_version: 3, qr_code: contact.qr_code })
  })
  it('renders server-controlled text without executing markup', async () => {
    wrapper = mount(AdminApp)
    await flushPromises()
    expect(wrapper.text()).toContain(owner.username)
    expect(wrapper.find('.data-table img').exists()).toBe(false)
    expect(wrapper.find('.data-table').html()).toContain('&lt;img')
  })
  it('reviews seat changes while preserving both authorization dates', async () => {
    wrapper = mount(AdminApp)
    await flushPromises()
    const button = wrapper.findAll('button').find(b => b.text() === '编辑授权')
    await button.trigger('click'); await flushPromises()
    await wrapper.find('dialog input[type=number]').setValue(80)
    await wrapper.find('dialog textarea').setValue('增加人数')
    await wrapper.find('dialog form').trigger('submit'); await flushPromises()
    expect(wrapper.find('dialog').text()).toContain('2027/08/31')
    expect(wrapper.find('dialog').text()).toContain('2026/09/01')
    await wrapper.find('dialog form').trigger('submit'); await flushPromises()
    const save = fetch.mock.calls.find(([, options]) => options.method === 'PUT')
    expect(JSON.parse(save[1].body)).toMatchObject({ seat_limit: 80, starts_on: '2026-09-01', ends_on: '2027-08-31', expected_version: 1 })
  })
  it('removes private table content when the server rejects an expired session', async () => {
    wrapper = mount(AdminApp); await flushPromises()
    fetch.mockResolvedValueOnce({ ok: false, status: 401, json: async () => ({ error: { code: 'UNAUTHORIZED', message: '过期' } }) })
    await wrapper.findAll('button').find(b => b.text() === '套餐设置').trigger('click'); await flushPromises()
    expect(wrapper.text()).toContain('管理员登录')
    expect(wrapper.text()).not.toContain(owner.username)
  })
})
