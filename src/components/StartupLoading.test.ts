import { flushPromises, mount } from '@vue/test-utils'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import StartupLoading from './StartupLoading.vue'
import { backend } from '../services/backend'
import type { StartupProgress } from '../types/domain'

const snapshot: StartupProgress = {
  phase: 'rebuilding', message: '正在更新旧题库的搜索和查重信息',
  completed: 64, total: 1000, elapsedMs: 6000,
  finished: false, error: null, logPath: null,
}

beforeEach(() => vi.useFakeTimers())
afterEach(() => { vi.useRealTimers(); vi.restoreAllMocks() })

it('shows real counts and elapsed time while startup is still pending', async () => {
  vi.spyOn(backend, 'startupProgress').mockResolvedValue(snapshot)
  const wrapper = mount(StartupLoading)
  await flushPromises()
  await vi.advanceTimersByTimeAsync(1000)
  expect(wrapper.text()).toContain('已处理 64 / 1000 道题')
  expect(wrapper.text()).toContain('已用时 7 秒')
  expect(wrapper.get('progress').attributes('max')).toBe('1000')
  wrapper.unmount()
  expect(vi.getTimerCount()).toBe(0)
})

it('keeps polling after a missed sample without showing a false failure', async () => {
  const poll = vi.spyOn(backend, 'startupProgress')
    .mockRejectedValueOnce(new Error('temporary IPC failure')).mockResolvedValue(snapshot)
  const wrapper = mount(StartupLoading)
  await flushPromises()
  expect(wrapper.text()).toContain('正在准备题库')
  await vi.advanceTimersByTimeAsync(1000)
  expect(poll).toHaveBeenCalledTimes(2)
  expect(wrapper.text()).toContain(snapshot.message)
  wrapper.unmount()
})

it('does not restart polling when an in-flight request completes after unmount', async () => {
  let finish!: (value: StartupProgress) => void
  const poll = vi.spyOn(backend, 'startupProgress').mockImplementation(() => new Promise(resolve => { finish = resolve }))
  const wrapper = mount(StartupLoading)
  wrapper.unmount()
  finish(snapshot)
  await flushPromises()
  await vi.advanceTimersByTimeAsync(5000)
  expect(poll).toHaveBeenCalledTimes(1)
  expect(vi.getTimerCount()).toBe(0)
})
