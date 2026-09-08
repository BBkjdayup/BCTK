import { mount } from '@vue/test-utils'
import { afterEach, describe, expect, it } from 'vitest'
import AppContextMenu from './AppContextMenu.vue'

afterEach(() => {
  document.body.innerHTML = ''
})

describe('AppContextMenu', () => {
  it('emits the selected action and closes the menu', async () => {
    const wrapper = mount(AppContextMenu, {
      attachTo: document.body,
      props: {
        modelValue: false,
        x: 120,
        y: 80,
        title: '当前题目',
        items: [{ id: 'preview', label: '预览题目' }],
      },
    })

    await wrapper.setProps({ modelValue: true })
    const button = document.body.querySelector<HTMLButtonElement>('.app-context-menu__item')
    expect(button?.textContent).toContain('预览题目')
    button?.click()

    expect(wrapper.emitted('select')?.[0]?.[0]).toMatchObject({ id: 'preview' })
    const closeEvents = wrapper.emitted('update:modelValue') ?? []
    expect(closeEvents[closeEvents.length - 1]).toEqual([false])
    wrapper.unmount()
  })

  it('keeps disabled actions inactive and closes on Escape', async () => {
    const wrapper = mount(AppContextMenu, {
      attachTo: document.body,
      props: {
        modelValue: true,
        x: 20,
        y: 20,
        items: [{ id: 'delete', label: '删除章节', disabled: true }],
      },
    })

    const button = document.body.querySelector<HTMLButtonElement>('.app-context-menu__item')
    expect(button?.disabled).toBe(true)
    button?.click()
    expect(wrapper.emitted('select')).toBeUndefined()

    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }))
    const closeEvents = wrapper.emitted('update:modelValue') ?? []
    expect(closeEvents[closeEvents.length - 1]).toEqual([false])
    wrapper.unmount()
  })
})
