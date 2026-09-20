import { flushPromises, mount } from '@vue/test-utils'
import { afterEach, describe, expect, it, vi } from 'vitest'
import DuplicateStemSummary from './DuplicateStemSummary.vue'

afterEach(() => vi.unstubAllGlobals())
describe('lazy duplicate stem summary', () => {
  it('reads only after entering the viewport and disconnects on removal', async () => {
    let enter!: IntersectionObserverCallback
    const disconnect = vi.fn()
    vi.stubGlobal('IntersectionObserver', class {
      constructor(callback: IntersectionObserverCallback) { enter = callback }
      observe() {}
      disconnect = disconnect
    })
    const load = vi.fn().mockResolvedValue({ schemaVersion: 1, html: '<p>题干</p>', plainText: '题干' })
    const wrapper = mount(DuplicateStemSummary, { props: { load } })
    expect(load).not.toHaveBeenCalled()
    enter([{ isIntersecting: true } as IntersectionObserverEntry], {} as IntersectionObserver)
    await flushPromises()
    expect(load).toHaveBeenCalledTimes(1)
    expect(wrapper.text()).toContain('题干')
    wrapper.unmount()
    expect(disconnect).toHaveBeenCalled()
  })
})
