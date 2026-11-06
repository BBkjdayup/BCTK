import { flushPromises, mount } from '@vue/test-utils'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import FormulaCompatibilityNotice from './FormulaCompatibilityNotice.vue'
const { check } = vi.hoisted(() => ({ check: vi.fn() }))
vi.mock('../services/backend', () => ({ backend: { checkFormulaCompatibility: check } }))
const fields = (latex: string, label = '第 2 题解析') => [{ label, content: { schemaVersion: 1 as const, html: `<table><tr><td><span data-latex="${latex}"></span></td></tr></table>`, plainText: '' } }]
beforeEach(() => { vi.useFakeTimers(); check.mockReset() })
afterEach(() => { vi.useRealTimers() })
it('locates an unsupported formula in its field and reuses the converter result', async () => {
  check.mockResolvedValue(['暂不支持 limits'])
  const view = mount(FormulaCompatibilityNotice, { props: { fields: fields('\\limits') } })
  await vi.advanceTimersByTimeAsync(300); await flushPromises()
  expect(view.text()).toContain('第 2 题解析：暂不支持 limits')
  await view.setProps({ fields: fields('\\limits', '第 3 题题干') }); await vi.advanceTimersByTimeAsync(300)
  expect(check).toHaveBeenCalledOnce(); expect(view.text()).toContain('第 3 题题干')
  view.unmount()
})
it('discards an obsolete check and shows a real check failure', async () => {
  let resolve!: (v: (string | null)[]) => void
  check.mockImplementationOnce(() => new Promise(r => { resolve = r })).mockResolvedValueOnce([null])
  const view = mount(FormulaCompatibilityNotice, { props: { fields: fields('old') } })
  await vi.advanceTimersByTimeAsync(300)
  await view.setProps({ fields: fields('x^2') }); await vi.advanceTimersByTimeAsync(300)
  resolve(['old error']); await flushPromises(); expect(view.find('aside').exists()).toBe(false)
  check.mockRejectedValueOnce(new Error('检查暂不可用'))
  await view.setProps({ fields: fields('x^3') }); await vi.advanceTimersByTimeAsync(300)
  expect(view.text()).toContain('检查暂不可用'); view.unmount()
})
