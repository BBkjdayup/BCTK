import { flushPromises, mount } from '@vue/test-utils'
import type { Editor } from '@tiptap/core'
import { afterEach, describe, expect, it, vi } from 'vitest'
import RichTextEditor from './RichTextEditor.vue'
import { backend } from '../services/backend'
import type { ManagedImagePayload, RichContent } from '../types/domain'

const resourceId = 'resize-image-resource'
const nodeId = 'resize-image-node'
const base64 = 'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+aXyoAAAAASUVORK5CYII='
const dataUrl = `data:image/png;base64,${base64}`
const unmount: Array<() => void> = []
const global = {
  stubs: {
    'el-icon': true,
    'el-tooltip': { template: '<span><slot /></span>' },
    'el-dialog': true,
    'el-input-number': true,
    'el-input': true,
    'el-button': true,
  },
}

function payload(id = resourceId, dataBase64 = base64): ManagedImagePayload {
  return { resourceId: id, mimeType: 'image/png', dataBase64, widthPx: 200, heightPx: 100 }
}

function imageHtml(id = resourceId, node = nodeId, width = 200) {
  return `<img data-resource-id="${id}" data-node-id="${node}" width="${width}" height="${width / 2}" alt="坐标图">`
}

async function open(html = `<p>前文${imageHtml()}后文</p>`, model?: RichContent) {
  const wrapper = mount(RichTextEditor, {
    attachTo: document.body,
    props: { modelValue: model ?? { schemaVersion: 1, html, plainText: '前文[图片]后文' } },
    global,
  })
  unmount.push(() => wrapper.unmount())
  await flushPromises()
  const editor = (wrapper.vm as unknown as { $: { setupState: { tiptap: Editor } } }).$.setupState.tiptap
  const image = wrapper.get<HTMLImageElement>('.rich-editor-image-node img').element
  const handle = wrapper.get<HTMLElement>('.rich-editor-image-node__handle').element
  const latest = () => wrapper.emitted('update:modelValue')?.at(-1)?.[0] as RichContent
  return { wrapper, editor, image, handle, latest }
}

function pointer(target: EventTarget, type: string, clientX: number, pointerId = 1) {
  const event = new MouseEvent(type, { clientX, bubbles: true, cancelable: true })
  Object.defineProperty(event, 'pointerId', { value: pointerId })
  Object.defineProperty(event, 'isPrimary', { value: true })
  target.dispatchEvent(event)
}

function flushEditorDom(editor: Editor) {
  // Exercise the same observer pass that previously reset a drag preview and removed src.
  const view = editor.view as unknown as { domObserver: { flush: () => void } }
  view.domObserver.flush()
}

function deferred<T>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>((done) => { resolve = done })
  return { promise, resolve }
}

afterEach(() => {
  unmount.splice(0).forEach((destroy) => destroy())
  vi.restoreAllMocks()
})

describe('RichTextEditor managed image resizing', () => {
  it('keeps the image visible throughout a pointer drag and saves only the final size', async () => {
    const get = vi.spyOn(backend, 'getManagedImage').mockResolvedValue(payload())
    const { wrapper, editor, image, handle, latest } = await open()
    await flushPromises()
    expect(image.getAttribute('src')).toBe(dataUrl)
    pointer(handle, 'pointerdown', 100)
    for (const x of [120, 140, 160]) {
      pointer(window, 'pointermove', x)
      flushEditorDom(editor)
      await flushPromises()
      expect(image.getAttribute('src')).toBe(dataUrl)
      expect(image.getAttribute('width')).toBe(String(x + 100))
      expect(image.getAttribute('height')).toBe(String((x + 100) / 2))
      expect(wrapper.emitted('update:modelValue')).toBeUndefined()
    }
    pointer(window, 'pointerup', 160)
    expect(image.getAttribute('src')).toBe(dataUrl)
    await flushPromises()
    expect(wrapper.emitted('update:modelValue')).toHaveLength(1)
    expect(get).toHaveBeenCalledOnce()
    expect(latest().document).toMatchObject({ content: [{ content: [
      { text: '前文' },
      { type: 'image', attrs: { resourceId, nodeId, src: null, width: 260, height: 130 } },
      { text: '后文' },
    ] }] })
    expect(latest().html).not.toContain('data:image/')
    expect(latest().html).not.toContain('260 px')
  })

  it('preserves the source immediately during keyboard resizing, undo, redo and reopening', async () => {
    const get = vi.spyOn(backend, 'getManagedImage').mockResolvedValue(payload())
    const original = await open()
    await flushPromises()
    original.handle.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowRight', bubbles: true }))
    expect(original.image.getAttribute('src')).toBe(dataUrl)
    expect(original.image.width).toBe(210)
    expect(original.image.height).toBe(105)
    const saved = original.latest()
    original.editor.commands.undo()
    expect(original.image.width).toBe(200)
    expect(original.image.getAttribute('src')).toBe(dataUrl)
    original.editor.commands.redo()
    expect(original.image.width).toBe(210)
    expect(original.image.getAttribute('src')).toBe(dataUrl)
    const reopened = await open('', saved)
    await flushPromises()
    expect(reopened.image.width).toBe(210)
    expect(reopened.image.height).toBe(105)
    expect(reopened.image.getAttribute('src')).toBe(dataUrl)
    expect(get).toHaveBeenCalledTimes(2)
  })

  it.each([0.5, 2])('uses visible dimensions and table-cell limits at page scale %s', async (scale) => {
    vi.spyOn(backend, 'getManagedImage').mockResolvedValue(payload())
    const { wrapper, editor, image, handle } = await open(`<table><tr><td><p>前文${imageHtml(resourceId, nodeId, 600)}后文</p></td></tr></table>`)
    await flushPromises()
    Object.defineProperty(editor.view.dom, 'offsetWidth', { configurable: true, value: 600 })
    Object.defineProperty(editor.view.dom, 'clientWidth', { configurable: true, value: 600 })
    vi.spyOn(editor.view.dom, 'getBoundingClientRect').mockReturnValue({ width: 600 * scale } as DOMRect)
    Object.defineProperty(wrapper.get('td').element, 'clientWidth', { configurable: true, value: 214 })
    wrapper.get<HTMLElement>('td').element.style.padding = '5px 7px'
    Object.defineProperty(wrapper.get('p').element, 'clientWidth', { configurable: true, value: 200 })
    vi.spyOn(image, 'getBoundingClientRect').mockImplementation(() => {
      const width = Math.min(image.width, 200) * scale
      return { width, height: width / 2 } as DOMRect
    })
    pointer(handle, 'pointerdown', 100)
    pointer(window, 'pointermove', 100 - 10 * scale)
    expect(image.width).toBe(190)
    pointer(window, 'pointerup', 100 - 10 * scale)
    expect(image.height).toBe(95)
    pointer(handle, 'pointerdown', 100)
    pointer(window, 'pointermove', 100 + 500 * scale)
    pointer(window, 'pointerup', 100 + 500 * scale)
    expect(image.width).toBe(200)
    expect(image.height).toBe(100)
    expect(wrapper.get('td p').text()).toContain('前文')
    expect(wrapper.get('td p').text()).toContain('后文')
  })

  it.each(['pointercancel', 'blur'])('restores the original layout when a drag is interrupted by %s', async (event) => {
    vi.spyOn(backend, 'getManagedImage').mockResolvedValue(payload())
    const { wrapper, image, handle } = await open()
    await flushPromises()
    pointer(handle, 'pointerdown', 100)
    pointer(window, 'pointermove', 160)
    expect(image.width).toBe(260)
    if (event === 'blur') window.dispatchEvent(new Event('blur'))
    else pointer(window, event, 160)
    expect(image.width).toBe(200)
    expect(image.height).toBe(100)
    expect(image.getAttribute('src')).toBe(dataUrl)
    pointer(window, 'pointerup', 160)
    expect(wrapper.emitted('update:modelValue')).toBeUndefined()
  })

  it('ignores other pointers and does not resize when the handle is clicked without moving', async () => {
    vi.spyOn(backend, 'getManagedImage').mockResolvedValue(payload())
    const { wrapper, image, handle } = await open()
    await flushPromises()
    pointer(handle, 'pointerdown', 100, 1)
    pointer(window, 'pointermove', 200, 2)
    pointer(window, 'pointerup', 200, 2)
    expect(image.width).toBe(200)
    pointer(window, 'pointerup', 100, 1)
    expect(wrapper.emitted('update:modelValue')).toBeUndefined()
  })

  it('shares pending image reads and keeps the other images in their original order and sizes', async () => {
    const pending = deferred<ManagedImagePayload>()
    const get = vi.spyOn(backend, 'getManagedImage').mockReturnValue(pending.promise)
    const { wrapper, handle, latest } = await open(`<p>${imageHtml()}文字${imageHtml(resourceId, 'second-node', 100)}</p>`)
    expect(get).toHaveBeenCalledOnce()
    pending.resolve(payload())
    await flushPromises()
    const images = wrapper.findAll<HTMLImageElement>('.rich-editor-image-node img')
    expect(images.map((image) => image.attributes('src'))).toEqual([dataUrl, dataUrl])
    handle.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowRight', bubbles: true }))
    expect(images[1]?.element.width).toBe(100)
    expect(latest().document).toMatchObject({ content: [{ content: [
      { attrs: { nodeId, width: 210 } },
      { text: '文字' },
      { attrs: { nodeId: 'second-node', width: '100' } },
    ] }] })
    expect(get).toHaveBeenCalledOnce()
  })

  it('ignores a late image response after the model switches to a different resource', async () => {
    const first = deferred<ManagedImagePayload>()
    const second = deferred<ManagedImagePayload>()
    vi.spyOn(backend, 'getManagedImage').mockImplementation((id) => id === resourceId ? first.promise : second.promise)
    const { wrapper } = await open()
    await wrapper.setProps({ modelValue: {
      schemaVersion: 1, html: `<p>${imageHtml('next-resource', 'next-node')}</p>`, plainText: '[图片]',
    } })
    second.resolve(payload('next-resource', 'bmV4dC1pbWFnZQ=='))
    await flushPromises()
    first.resolve(payload())
    await flushPromises()
    expect(wrapper.get('img').attributes('data-resource-id')).toBe('next-resource')
    expect(wrapper.get('img').attributes('src')).toBe('data:image/png;base64,bmV4dC1pbWFnZQ==')
  })

  it('shows a readable error and retries without inserting the error message into the question', async () => {
    const get = vi.spyOn(backend, 'getManagedImage')
      .mockRejectedValueOnce(new Error('disk read failed'))
      .mockResolvedValueOnce(payload())
    const { wrapper, editor, image } = await open()
    await flushPromises()
    expect(wrapper.get('.rich-editor-image-node').attributes('data-resource-state')).toBe('error')
    expect(wrapper.get('[role="status"]').text()).toContain('图片读取失败')
    await wrapper.get('[aria-label="重新加载图片"]').trigger('click')
    await flushPromises()
    expect(image.getAttribute('src')).toBe(dataUrl)
    expect(wrapper.get('[role="status"]').attributes('hidden')).toBeDefined()
    expect(get).toHaveBeenCalledTimes(2)
    flushEditorDom(editor)
    expect(wrapper.emitted('update:modelValue')).toBeUndefined()
    expect(editor.getHTML()).not.toContain('图片读取失败')
    expect(editor.getHTML()).not.toContain('重新加载')
  })
})
