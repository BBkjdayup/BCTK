import { flushPromises, mount } from '@vue/test-utils'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { ElementType } from '@hufe921/canvas-editor'
import type { Paper, PaperLayout, Question } from '../types/domain'
import { CANVAS_EDITOR_VERSION, paperLayoutSourceSignature } from '../utils/paperLayout'
import PaperCanvasEditor from './PaperCanvasEditor.vue'

const canvasMock = vi.hoisted(() => ({
  failuresRemaining: 0,
  constructorCalls: 0,
  pageScales: [] as number[],
  paperSizes: [] as Array<[number, number]>,
  paperDirections: [] as string[],
  initialScales: [] as number[],
  setValueCalls: 0,
  setValueFailuresRemaining: 0,
  wordCountFailuresRemaining: 0,
  destroyCalls: 0,
  updateElementCalls: 0,
}))

vi.mock('@hufe921/canvas-editor', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@hufe921/canvas-editor')>()
  class MockEditor {
    listener: Record<string, unknown> = {}
    eventBus = { on: vi.fn(), off: vi.fn() }
    command: Record<string, (...args: never[]) => unknown>
    private data: unknown
    private options: Record<string, unknown>

    constructor(_container: HTMLDivElement, data: unknown, options: Record<string, unknown>) {
      canvasMock.constructorCalls += 1
      if (canvasMock.failuresRemaining > 0) {
        canvasMock.failuresRemaining -= 1
        throw new Error('invalid persisted canvas snapshot')
      }
      this.data = data
      this.options = { scale: 1, defaultFont: '宋体', defaultSize: 16, ...options }
      canvasMock.initialScales.push(Number(this.options.scale))
      this.command = {
        getOptions: () => this.options,
        getValue: () => ({ data: this.data, options: this.options }),
        getWordCount: async () => {
          if (canvasMock.wordCountFailuresRemaining > 0) {
            canvasMock.wordCountFailuresRemaining -= 1
            throw new Error('worker unavailable')
          }
          return 12
        },
        getCursorPosition: () => null,
        executeSetValue: (next: unknown) => {
          canvasMock.setValueCalls += 1
          if (canvasMock.setValueFailuresRemaining > 0) {
            canvasMock.setValueFailuresRemaining -= 1
            throw new Error('formula normalization unavailable')
          }
          this.data = next
        },
        getElementById: () => [],
        executeUpdateElementById: () => { canvasMock.updateElementCalls += 1 },
        executePageScale: (scale: number) => { canvasMock.pageScales.push(scale) },
        executePaperSize: (width: number, height: number) => {
          canvasMock.paperSizes.push([width, height])
          this.options.width = width
          this.options.height = height
        },
        executePaperDirection: (direction: string) => {
          canvasMock.paperDirections.push(direction)
          this.options.paperDirection = direction
        },
      }
    }

    destroy() { canvasMock.destroyCalls += 1 }
  }
  return { ...actual, default: MockEditor }
})

function testPaper(): Paper {
  const rich = (plainText: string) => ({ schemaVersion: 1 as const, html: `<p>${plainText}</p>`, plainText })
  const question: Question = {
    id: 'question-1',
    type: 'single_choice',
    stem: rich('第一题'),
    options: [],
    answer: rich('A'),
    explanation: rich(''),
    subjectId: 'subject-1',
    chapterId: 'chapter-1',
    subjectName: '计算机',
    chapterName: '网络',
    tags: [],
    contentVersion: 1,
    createdAt: 1,
    updatedAt: 1,
  }
  return {
    id: 'paper-1',
    title: '测试试卷',
    compositionMode: 'manual',
    generationConfig: null,
    status: 'draft',
    items: [{
      id: 'paper-item-1',
      sourceQuestionId: question.id,
      position: 0,
      snapshot: question,
    }],
    exportContentMode: 'paper_only',
    subjectSummaryText: '计算机',
    rowVersion: 1,
    createdAt: 1,
    updatedAt: 1,
  }
}

function staleLayout(): PaperLayout {
  return {
    schemaVersion: 1,
    editor: 'canvas-editor',
    editorVersion: CANVAS_EDITOR_VERSION,
    sourceSignature: 'outdated-signature',
    data: { main: [{ value: '旧排版' }] },
    options: {},
  }
}

afterEach(() => {
  canvasMock.failuresRemaining = 0
  canvasMock.constructorCalls = 0
  canvasMock.pageScales = []
  canvasMock.paperSizes = []
  canvasMock.paperDirections = []
  canvasMock.initialScales = []
  canvasMock.setValueCalls = 0
  canvasMock.setValueFailuresRemaining = 0
  canvasMock.wordCountFailuresRemaining = 0
  canvasMock.destroyCalls = 0
  canvasMock.updateElementCalls = 0
  vi.restoreAllMocks()
})

describe('PaperCanvasEditor', () => {
  it('removes the outline so the full workspace is available to the paper', async () => {
    const wrapper = mount(PaperCanvasEditor, {
      props: { paper: testPaper(), modelValue: null },
    })
    await flushPromises()

    expect(wrapper.find('.paper-outline').exists()).toBe(false)
    expect(wrapper.find('.paper-canvas-scroll').exists()).toBe(true)
    wrapper.unmount()
  })

  it('separates the editor and settings into two frames while keeping one toolbar row', async () => {
    const wrapper = mount(PaperCanvasEditor, {
      props: { paper: testPaper(), modelValue: null },
      slots: {
        settings: '<section class="test-settings">排版内容</section>',
      },
    })
    await flushPromises()

    expect(wrapper.get('.paper-canvas-shell').classes()).toContain('has-settings')
    expect(wrapper.get('.paper-canvas-main').find('.paper-canvas-toolbar').exists()).toBe(true)
    expect(wrapper.get('.paper-canvas-main').element.nextElementSibling).toBe(
      wrapper.get('.paper-canvas-settings-slot').element,
    )
    expect(wrapper.findAll('.toolbar-row')).toHaveLength(1)
    expect(wrapper.get('.paper-canvas-settings-slot').text()).toContain('排版内容')
    expect(wrapper.findAll('.fit-control')).toHaveLength(2)
    expect(wrapper.get('button[aria-label="撤销"]').find('svg').exists()).toBe(true)
    expect(wrapper.get('button[aria-label="重做"]').find('svg').exists()).toBe(true)
    expect(wrapper.get('button[aria-label="格式刷"]').find('svg').exists()).toBe(true)
    expect(wrapper.get('button[aria-label="格式刷"]').text()).toBe('')
    expect(wrapper.get('button[aria-label="左对齐"]').find('.align-icon').exists()).toBe(true)
    expect(wrapper.get('.toolbar-print').find('svg').exists()).toBe(true)
    wrapper.unmount()
  })

  it('shows the search input only after clicking the search icon', async () => {
    const wrapper = mount(PaperCanvasEditor, {
      props: { paper: testPaper(), modelValue: null },
    })
    await flushPromises()

    const searchButton = wrapper.get('button[aria-label="查找"]')
    expect(searchButton.find('svg').exists()).toBe(true)
    expect(wrapper.find('input[aria-label="查找内容"]').exists()).toBe(false)

    await searchButton.trigger('click')

    expect(wrapper.find('input[aria-label="查找内容"]').exists()).toBe(true)
    expect(searchButton.classes()).toContain('active')
    wrapper.unmount()
  })

  it('applies a selected paper size and saves it with the canvas layout', async () => {
    const wrapper = mount(PaperCanvasEditor, {
      props: { paper: testPaper(), modelValue: null },
    })
    await flushPromises()

    await wrapper.get('summary[aria-label="页面"]').trigger('click')
    await wrapper.get('select[aria-label="纸张大小"]').setValue('a3')
    await wrapper.get('select[aria-label="纸张方向"]').setValue('landscape')
    await wrapper.get('.page-apply').trigger('click')

    expect(canvasMock.paperSizes[canvasMock.paperSizes.length - 1]).toEqual([1123, 1587])
    expect(canvasMock.paperDirections[canvasMock.paperDirections.length - 1]).toBe('horizontal')
    const updates = wrapper.emitted<PaperLayout[]>('update:modelValue') ?? []
    expect(updates[updates.length - 1]?.[0].pageSetup).toEqual({
      widthMm: 420,
      heightMm: 297,
    })
    wrapper.unmount()
  })

  it('can scale the paper to the available workspace width', async () => {
    const wrapper = mount(PaperCanvasEditor, {
      props: { paper: testPaper(), modelValue: null },
    })
    await flushPromises()
    const scroll = wrapper.get('.paper-canvas-scroll').element
    Object.defineProperty(scroll, 'clientWidth', { configurable: true, value: 1000 })
    Object.defineProperty(scroll, 'clientHeight', { configurable: true, value: 700 })

    await wrapper.get('.fit-control:last-child').trigger('click')
    await new Promise((resolve) => window.requestAnimationFrame(resolve))

    expect(canvasMock.pageScales[canvasMock.pageScales.length - 1]).toBe(1.15)
    wrapper.unmount()
  })

  it('automatically regenerates layouts saved by an older adapter version', async () => {
    const oldLayout: PaperLayout = {
      ...staleLayout(),
      editorVersion: '0.9.137-zhitiku.9',
      data: { main: [{ value: '\\(x^2\\)' }] },
    }
    const wrapper = mount(PaperCanvasEditor, {
      props: { paper: testPaper(), modelValue: oldLayout },
    })
    await flushPromises()

    expect(canvasMock.constructorCalls).toBe(1)
    const updates = wrapper.emitted<PaperLayout[]>('update:modelValue') ?? []
    const latest = updates[updates.length - 1]?.[0]
    expect(latest?.editorVersion).toBe(CANVAS_EDITOR_VERSION)
    expect(latest?.data.main).toEqual(expect.arrayContaining([
      expect.objectContaining({ value: expect.stringContaining('测试试卷') }),
    ]))
    expect(latest?.data.main).not.toEqual(expect.arrayContaining([
      expect.objectContaining({ value: '\\(x^2\\)' }),
    ]))
    wrapper.unmount()
  })

  it('rebuilds v0.1.40 layouts that flattened tables and images into text', async () => {
    const oldLayout: PaperLayout = {
      ...staleLayout(),
      editorVersion: '0.9.137-zhitiku.10',
      data: { main: [{ value: 'x\t\ny\t\n[图片]' }] },
    }
    const wrapper = mount(PaperCanvasEditor, {
      props: { paper: testPaper(), modelValue: oldLayout },
    })
    await flushPromises()

    const updates = wrapper.emitted<PaperLayout[]>('update:modelValue') ?? []
    const latest = updates[updates.length - 1]?.[0]
    expect(latest?.editorVersion).toBe(CANVAS_EDITOR_VERSION)
    expect(latest?.data.main).not.toEqual(expect.arrayContaining([
      expect.objectContaining({ value: expect.stringContaining('[图片]') }),
    ]))
    wrapper.unmount()
  })

  it('preserves manually edited old layouts that do not contain legacy formula text', async () => {
    const source = testPaper()
    const oldLayout: PaperLayout = {
      ...staleLayout(),
      editorVersion: '0.9.137-zhitiku.9',
      sourceSignature: paperLayoutSourceSignature(source),
      data: { main: [{ value: '用户手工调整的排版' }] },
    }
    const wrapper = mount(PaperCanvasEditor, {
      props: { paper: source, modelValue: oldLayout },
    })
    await flushPromises()

    expect(canvasMock.constructorCalls).toBe(1)
    expect(wrapper.emitted('update:modelValue')).toBeUndefined()
    wrapper.unmount()
  })

  it('automatically regenerates a current layout after the paper basket changes', async () => {
    const source = testPaper()
    const oldLayout: PaperLayout = {
      ...staleLayout(),
      pageSetup: { widthMm: 297, heightMm: 420 },
    }
    const wrapper = mount(PaperCanvasEditor, {
      props: { paper: source, modelValue: oldLayout },
    })
    await flushPromises()

    expect(canvasMock.constructorCalls).toBe(1)
    const updates = wrapper.emitted<PaperLayout[]>('update:modelValue') ?? []
    const latest = updates[updates.length - 1]?.[0]
    expect(latest?.sourceSignature).toBe(paperLayoutSourceSignature(source))
    expect(latest?.sourceSignature).not.toBe('outdated-signature')
    expect(latest?.pageSetup).toEqual({ widthMm: 297, heightMm: 420 })
    expect(latest?.data.main).toEqual(expect.arrayContaining([
      expect.objectContaining({ value: expect.stringContaining('测试试卷') }),
    ]))
    expect(wrapper.find('.paper-canvas-warning').exists()).toBe(false)
    wrapper.unmount()
  })

  it('allows regeneration after a persisted snapshot leaves the editor blank', async () => {
    vi.spyOn(console, 'error').mockImplementation(() => {})
    canvasMock.failuresRemaining = 2
    const source = testPaper()
    const storedLayout: PaperLayout = {
      ...staleLayout(),
      sourceSignature: paperLayoutSourceSignature(source),
    }
    const wrapper = mount(PaperCanvasEditor, {
      props: { paper: source, modelValue: storedLayout },
    })
    await flushPromises()

    const regenerate = wrapper.get('.toolbar-regenerate')
    expect((regenerate.element as HTMLButtonElement).disabled).toBe(false)
    expect(regenerate.attributes('aria-label')).toBe('恢复模板排版')
    expect(regenerate.text()).toContain('重新生成')

    await regenerate.trigger('click')
    await flushPromises()

    expect(canvasMock.constructorCalls).toBe(3)
    const updates = wrapper.emitted<PaperLayout[]>('update:modelValue') ?? []
    const latest = updates[updates.length - 1]?.[0]
    expect(latest?.sourceSignature).not.toBe('outdated-signature')
    expect(latest?.data.main).toEqual(expect.arrayContaining([
      expect.objectContaining({ value: expect.stringContaining('测试试卷') }),
    ]))
    wrapper.unmount()
  })

  it('caps the automatic preview scale before mounting a large paper', async () => {
    const width = vi.spyOn(HTMLElement.prototype, 'clientWidth', 'get').mockReturnValue(1600)
    const height = vi.spyOn(HTMLElement.prototype, 'clientHeight', 'get').mockReturnValue(1200)
    const source = testPaper()
    const item = source.items[0]!
    source.items = Array.from({ length: 200 }, (_, index) => ({
      ...item,
      id: `paper-item-${index + 1}`,
      position: index,
    }))

    const wrapper = mount(PaperCanvasEditor, {
      props: { paper: source, modelValue: null },
    })
    await flushPromises()

    expect(canvasMock.constructorCalls).toBe(1)
    expect(canvasMock.initialScales[0]).toBe(0.5)
    wrapper.unmount()
    width.mockRestore()
    height.mockRestore()
  })

  it('normalizes all formula dimensions with one canvas value update', async () => {
    const source = testPaper()
    const formulaLayout: PaperLayout = {
      schemaVersion: 1,
      editor: 'canvas-editor',
      editorVersion: CANVAS_EDITOR_VERSION,
      sourceSignature: paperLayoutSourceSignature(source),
      data: {
        main: Array.from({ length: 40 }, (_, index) => ({
          id: `formula-${index}`,
          type: ElementType.LATEX,
          value: 'x^2',
          width: 40,
          height: 20,
          extension: {
            kind: 'paper-formula',
            paperItemId: source.items[0]!.id,
            contentSlot: 'stem',
            ordinal: index,
            sourceLatex: 'x^2',
          },
        })),
      },
      options: {},
    }
    const wrapper = mount(PaperCanvasEditor, {
      props: { paper: source, modelValue: formulaLayout },
    })
    await flushPromises()

    expect(canvasMock.setValueCalls).toBe(1)
    expect(canvasMock.updateElementCalls).toBe(0)
    wrapper.unmount()
  })

  it('keeps the canvas usable when optional formula normalization fails', async () => {
    vi.spyOn(console, 'warn').mockImplementation(() => {})
    canvasMock.setValueFailuresRemaining = 1
    const source = testPaper()
    const formulaLayout: PaperLayout = {
      ...staleLayout(),
      sourceSignature: paperLayoutSourceSignature(source),
      data: {
        main: [{
          id: 'formula-1',
          type: ElementType.LATEX,
          value: 'x^2',
          width: 40,
          height: 20,
          extension: {
            kind: 'paper-formula',
            paperItemId: source.items[0]!.id,
            contentSlot: 'stem',
            ordinal: 0,
            sourceLatex: 'x^2',
          },
        }],
      },
    }
    const wrapper = mount(PaperCanvasEditor, {
      props: { paper: source, modelValue: formulaLayout },
    })
    await flushPromises()

    expect(canvasMock.constructorCalls).toBe(2)
    expect(canvasMock.destroyCalls).toBeGreaterThanOrEqual(1)
    expect(wrapper.find('.paper-canvas-progress.is-error').exists()).toBe(false)
    wrapper.unmount()
  })

  it('does not fail canvas initialization when the word-count worker is unavailable', async () => {
    vi.spyOn(console, 'warn').mockImplementation(() => {})
    canvasMock.wordCountFailuresRemaining = 1
    const wrapper = mount(PaperCanvasEditor, {
      props: { paper: testPaper(), modelValue: null },
    })
    await flushPromises()

    expect(canvasMock.constructorCalls).toBe(1)
    expect(wrapper.find('.paper-canvas-progress.is-error').exists()).toBe(false)
    expect(wrapper.text()).toContain('字数：')
    wrapper.unmount()
  })
})
