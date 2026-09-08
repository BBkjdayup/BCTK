import { flushPromises, mount } from '@vue/test-utils'
import { describe, expect, it, vi } from 'vitest'
import RichTextEditor from './RichTextEditor.vue'
import type { RichContent } from '../types/domain'
import { backend } from '../services/backend'

function content(html: string, plainText: string): RichContent {
  return { schemaVersion: 1, html, plainText }
}

describe('RichTextEditor', () => {
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

  it('renders safe plain text when recovered content has no HTML cache', async () => {
    const wrapper = mount(RichTextEditor, {
      props: { modelValue: content('', '<选项甲>& 选项乙') },
      global,
    })

    await flushPromises()
    const editor = wrapper.get('[role="textbox"]')
    expect(editor.text()).toBe('<选项甲>& 选项乙')
    expect(editor.html()).toContain('&lt;选项甲&gt;&amp; 选项乙')
  })

  it('updates the editor when recovered rich HTML arrives after mounting', async () => {
    const wrapper = mount(RichTextEditor, {
      props: { modelValue: content('', '') },
      global,
    })

    await flushPromises()
    await wrapper.setProps({ modelValue: content('<p><strong>答案 A</strong></p>', '答案 A') })
    await flushPromises()
    expect(wrapper.get('[role="textbox"]').html()).toContain('<strong>答案 A</strong>')
  })

  it('restores a locally edited answer after switching to another empty model and back', async () => {
    const empty = content('', '')
    const wrapper = mount(RichTextEditor, {
      props: { modelValue: empty },
      global,
    })

    await flushPromises()
    const internal = (wrapper.vm as unknown as {
      $: { setupState: { tiptap: { commands: { setContent: (value: string) => boolean } } } }
    }).$.setupState.tiptap
    internal.commands.setContent('<p>手动答案 A</p>')
    await flushPromises()
    const updates = wrapper.emitted('update:modelValue') ?? []
    const edited = updates[updates.length - 1]?.[0] as RichContent
    expect(edited.plainText).toBe('手动答案 A')

    await wrapper.setProps({ modelValue: edited })
    await flushPromises()
    await wrapper.setProps({ modelValue: content('', '') })
    await flushPromises()
    expect(wrapper.get('[role="textbox"]').text()).toBe('')

    await wrapper.setProps({ modelValue: edited })
    await flushPromises()
    expect(wrapper.get('[role="textbox"]').text()).toBe('手动答案 A')
  })

  it('updates the empty-editor placeholder after its template changes', async () => {
    const wrapper = mount(RichTextEditor, {
      props: {
        modelValue: content('', ''),
        placeholder: '题目：旧提示',
      },
      global,
    })

    await flushPromises()
    expect(wrapper.get('.is-editor-empty').attributes('data-placeholder')).toBe('题目：旧提示')
    await wrapper.setProps({ placeholder: '【题目】新提示' })
    await flushPromises()
    expect(wrapper.get('.is-editor-empty').attributes('data-placeholder')).toBe('【题目】新提示')
  })

  it('uses the complete toolbar by default', () => {
    const wrapper = mount(RichTextEditor, {
      props: { modelValue: content('', '') },
      global,
    })

    expect(wrapper.find('[aria-label="加粗"]').exists()).toBe(true)
    expect(wrapper.find('[aria-label="斜体"]').exists()).toBe(true)
    expect(wrapper.find('[aria-label="项目符号列表"]').exists()).toBe(true)
    expect(wrapper.find('[aria-label="插入图片"]').exists()).toBe(true)
    expect(wrapper.find('[aria-label="插入表格"]').exists()).toBe(true)
    expect(wrapper.find('[aria-label="插入公式"]').exists()).toBe(true)
  })

  it('uses a compact toolbar for choice options', () => {
    const wrapper = mount(RichTextEditor, {
      props: { modelValue: content('', ''), toolbarMode: 'compact' },
      global,
    })

    expect(wrapper.find('[aria-label="加粗"]').exists()).toBe(true)
    expect(wrapper.find('[aria-label="插入图片"]').exists()).toBe(true)
    expect(wrapper.find('[aria-label="插入公式"]').exists()).toBe(true)
    expect(wrapper.find('[aria-label="斜体"]').exists()).toBe(false)
    expect(wrapper.find('[aria-label="项目符号列表"]').exists()).toBe(false)
    expect(wrapper.find('[aria-label="插入表格"]').exists()).toBe(false)
  })

  it('can hide the toolbar while keeping the editable area', async () => {
    const wrapper = mount(RichTextEditor, {
      props: { modelValue: content('', ''), toolbarMode: 'hidden' },
      global,
    })

    await flushPromises()
    expect(wrapper.find('.rich-editor__toolbar').exists()).toBe(false)
    expect(wrapper.find('[role="textbox"]').exists()).toBe(true)
    expect(wrapper.find('input[type="file"]').exists()).toBe(true)
  })

  it('keeps the native context menu and opens editing tools with Ctrl + right-click', async () => {
    const wrapper = mount(RichTextEditor, {
      props: {
        modelValue: content('<p>需要编辑的文字</p>', '需要编辑的文字'),
        toolbarMode: 'hidden',
        contextTools: true,
      },
      global,
    })

    await flushPromises()
    expect(wrapper.find('.rich-editor__toolbar').exists()).toBe(false)
    expect(wrapper.findAll('[aria-label="更多编辑工具"]')).toHaveLength(1)

    const editor = wrapper.get('[role="textbox"]')
    const nativeMenuEvent = new MouseEvent('contextmenu', { bubbles: true, cancelable: true })
    editor.element.dispatchEvent(nativeMenuEvent)
    expect(nativeMenuEvent.defaultPrevented).toBe(false)

    const editingToolsEvent = new MouseEvent('contextmenu', {
      bubbles: true,
      cancelable: true,
      ctrlKey: true,
      clientX: 120,
      clientY: 80,
    })
    editor.element.dispatchEvent(editingToolsEvent)
    await flushPromises()

    expect(editingToolsEvent.defaultPrevented).toBe(true)
    expect(document.body.textContent).not.toContain('普通右键可使用语音输入')
    expect(document.body.textContent).not.toContain('斜体')
    expect(document.body.textContent).not.toContain('Ctrl+I')
    expect(document.body.textContent).toContain('插入图片')
    expect(document.body.textContent).toContain('插入表格')
    expect(document.body.textContent).toContain('插入公式')

    const fileInput = wrapper.get<HTMLInputElement>('input[type="file"]')
    const clickSpy = vi.spyOn(fileInput.element, 'click')
    const imageMenuItem = [...document.body.querySelectorAll<HTMLElement>('[role="menuitem"]')]
      .find((item) => item.textContent?.includes('插入图片'))
    imageMenuItem?.click()
    expect(clickSpy).toHaveBeenCalledOnce()

    wrapper.unmount()
  })

  it('shows imported editable formulas as typeset mathematics instead of raw LaTeX', async () => {
    const wrapper = mount(RichTextEditor, {
      props: {
        modelValue: {
          schemaVersion: 2,
          editor: 'tiptap',
          document: {
            type: 'doc',
            content: [{
              type: 'paragraph',
              content: [
                { type: 'text', text: '675' },
                { type: 'mathNode', attrs: { latex: '^{\\circ}' } },
                { type: 'text', text: '用弧度制表示为 ' },
                { type: 'mathNode', attrs: { latex: '\\frac{11}{4}\\pi' } },
              ],
            }],
          },
          html: '',
          plainText: '675°用弧度制表示为 11/4π',
        } satisfies RichContent,
      },
      global,
    })

    await flushPromises()
    const formulas = wrapper.findAll('.math-node')
    expect(formulas).toHaveLength(2)
    expect(formulas[0]?.find('.katex-html').text()).toContain('∘')
    expect(formulas[1]?.find('.katex-html .frac-line').exists()).toBe(true)
    expect(formulas[1]?.attributes('data-latex')).toBe('\\frac{11}{4}\\pi')
  })

  it('lets the user select an image and resize it from inside the editor', async () => {
    const wrapper = mount(RichTextEditor, {
      props: {
        modelValue: content('<p><img src="data:image/png;base64,AA==" width="200" alt="坐标图"></p>', '[图片]'),
      },
      global,
    })

    await flushPromises()
    await wrapper.get('.rich-editor-image-node img').trigger('click')
    await flushPromises()
    expect(wrapper.get('.rich-editor-image-node').classes()).toContain('is-selected')
    const handle = wrapper.get('[aria-label="拖动调整图片大小"]')
    await handle.trigger('keydown', { key: 'ArrowRight' })
    await flushPromises()

    const updates = wrapper.emitted('update:modelValue') ?? []
    const latest = updates[updates.length - 1]?.[0] as RichContent
    const document = latest.document as {
      content?: Array<{ content?: Array<{ type?: string; attrs?: { width?: number } }> }>
    }
    expect(document.content?.[0]?.content?.[0]).toMatchObject({
      type: 'image',
      attrs: { width: 210 },
    })
  })

  it('preserves managed image identifiers after resizing', async () => {
    const resourceId = '019fa7fc-2c03-7111-a8b6-4166afe943bc'
    const nodeId = '019fa7fc-2c03-7111-a8b6-4166afe943bd'
    const wrapper = mount(RichTextEditor, {
      props: {
        modelValue: content(
          `<p><img src="data:image/png;base64,AA==" data-resource-id="${resourceId}" data-node-id="${nodeId}" width="200"></p>`,
          '[image]',
        ),
      },
      global,
    })

    await flushPromises()
    await wrapper.get('.rich-editor-image-node img').trigger('click')
    await wrapper.get('.rich-editor-image-node__handle').trigger('keydown', { key: 'ArrowRight' })
    await flushPromises()

    const updates = wrapper.emitted('update:modelValue') ?? []
    const latest = updates[updates.length - 1]?.[0] as RichContent
    const document = latest.document as {
      content?: Array<{ content?: Array<{ type?: string; attrs?: Record<string, unknown> }> }>
    }
    expect(document.content?.[0]?.content?.[0]).toMatchObject({
      type: 'image',
      attrs: { width: 210, resourceId, nodeId },
    })
    expect(latest.html).toContain(`data-resource-id="${resourceId}"`)
    expect(latest.html).toContain(`data-node-id="${nodeId}"`)
  })

  it('stores a newly selected image and persists only its managed reference', async () => {
    const resourceId = '019fa7fc-2c03-7111-a8b6-4166afe943bc'
    const store = vi.spyOn(backend, 'storeManagedImages').mockResolvedValue([{
      resourceId,
      mimeType: 'image/png',
      dataBase64: 'cG5nLWJ5dGVz',
      widthPx: 32,
      heightPx: 16,
    }])
    const wrapper = mount(RichTextEditor, {
      props: { modelValue: content('', '') },
      global,
    })
    const file = new File(['png-bytes'], 'diagram.png', { type: 'image/png' })
    const input = wrapper.get<HTMLInputElement>('input[type="file"]')
    Object.defineProperty(input.element, 'files', { configurable: true, value: [file] })
    await input.trigger('change')
    await vi.waitFor(() => {
      expect(store).toHaveBeenCalledWith([{
        dataBase64: expect.any(String),
        originalFilename: 'diagram.png',
      }])
    })
    await flushPromises()
    const updates = wrapper.emitted('update:modelValue') ?? []
    const latest = updates[updates.length - 1]?.[0] as RichContent
    expect(latest.html).toContain(`data-resource-id="${resourceId}"`)
    expect(latest.html).toContain('data-node-id=')
    expect(latest.html).not.toContain('data:image/')
    expect(latest.document).toMatchObject({
      content: [{ content: [{ type: 'image', attrs: { resourceId, src: null } }] }],
    })

    wrapper.unmount()
    store.mockRestore()
  })
})
