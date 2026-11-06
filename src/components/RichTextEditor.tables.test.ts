import { flushPromises, mount } from '@vue/test-utils'
import type { Editor } from '@tiptap/core'
import { TableMap } from '@tiptap/pm/tables'
import { afterEach, describe, expect, it, vi } from 'vitest'
import RichTextEditor from './RichTextEditor.vue'
import { backend } from '../services/backend'
import type { RichContent } from '../types/domain'
import { richContentToCanvasElements } from '../utils/paperLayout'
import { readFileSync } from 'node:fs'
import type { BeginWordImportResult } from '../types/domain'
import { buildWordAnalysisLines } from '../utils/wordImportAnalysis'
import { buildWordImportRichContent } from '../utils/wordImportRichContent'
import { recognizeWordQuestions } from '../utils/wordImportRecognition'
import { fallbackQuestionTypes } from '../utils/questionTypes'

const unmount: Array<() => void> = []
const global = { stubs: {
  'el-icon': true, 'el-tooltip': { template: '<span><slot /></span>' },
  'el-dialog': true, 'el-input-number': true, 'el-input': true, 'el-button': true,
} }
const html = '<table data-table-width="300"><tr data-row-min-height="40"><td colwidth="100"><p><strong>U/V</strong></p></td><td colwidth="200"><p><span class="math-node" data-latex="x^2">x^2</span></p></td></tr><tr data-row-min-height="60"><td colwidth="100"><p>I/A</p></td><td colwidth="200"><p>0.20</p></td></tr></table>'

async function open(value: RichContent = { schemaVersion: 1, html, plainText: 'U/V x^2 I/A 0.20' }, scale = 1) {
  const wrapper = mount(RichTextEditor, { attachTo: document.body, props: { modelValue: value }, global })
  unmount.push(() => wrapper.unmount())
  await flushPromises()
  const editor = (wrapper.vm as unknown as { $: { setupState: { tiptap: Editor } } }).$.setupState.tiptap
  const table = wrapper.get<HTMLTableElement>('table').element
  const handle = wrapper.get<HTMLElement>('.rich-editor-table-node__handle').element
  Object.defineProperty(editor.view.dom, 'offsetWidth', { configurable: true, value: 600 })
  Object.defineProperty(editor.view.dom, 'clientWidth', { configurable: true, value: 600 })
  vi.spyOn(editor.view.dom, 'getBoundingClientRect').mockReturnValue({ width: 600 * scale } as DOMRect)
  vi.spyOn(table, 'getBoundingClientRect').mockImplementation(() => ({
    width: Number.parseFloat(table.style.width) * scale,
    height: Array.from(table.rows).reduce((total, row) => total + (Number.parseFloat(row.style.height) || 32), 0) * scale,
  } as DOMRect))
  Array.from(table.rows).forEach((row) => {
    vi.spyOn(row, 'getBoundingClientRect').mockImplementation(() => ({ height: (Number.parseFloat(row.style.height) || 32) * scale } as DOMRect))
  })
  const latest = () => wrapper.emitted('update:modelValue')?.at(-1)?.[0] as RichContent
  const node = () => editor.state.doc.firstChild!
  const widths = () => Array.from(table.querySelectorAll<HTMLElement>('col'), (col) => Number.parseFloat(col.style.width))
  return { wrapper, editor, table, handle, latest, node, widths }
}

function pointer(target: EventTarget, type: string, x = 100, y = 100, pointerId = 1) {
  const event = new MouseEvent(type, { clientX: x, clientY: y, bubbles: true, cancelable: true })
  Object.defineProperties(event, { pointerId: { value: pointerId }, isPrimary: { value: true } })
  target.dispatchEvent(event)
}

function flushDom(editor: Editor) {
  (editor.view as unknown as { domObserver: { flush: () => void } }).domObserver.flush()
}

afterEach(() => { unmount.splice(0).forEach((destroy) => destroy()); vi.restoreAllMocks() })

describe('editable table dimensions', () => {
  it('previews the whole table without saving, commits once, and retains content through undo, reopen and composition', async () => {
    const { wrapper, editor, table, handle, latest, node, widths } = await open()
    pointer(handle, 'pointerdown')
    for (const step of [20, 40, 60]) {
      pointer(window, 'pointermove', 100 + step, 100 + step)
      flushDom(editor)
      await flushPromises()
      expect(table.style.width).toBe(`${300 + step}px`)
      expect(wrapper.emitted('update:modelValue')).toBeUndefined()
    }
    pointer(window, 'pointerup', 160, 160)
    await flushPromises()
    expect(wrapper.emitted('update:modelValue')).toHaveLength(1)
    expect(node().attrs.width).toBe(360)
    expect(widths()).toEqual([120, 240])
    expect(node().child(0).attrs.minHeight).toBe(64)
    expect(node().child(1).attrs.minHeight).toBe(96)
    expect(node().child(0).toJSON()).toMatchObject({ content: [
      { content: [{ content: [{ text: 'U/V', marks: [{ type: 'bold' }] }] }] },
      { content: [{ content: [{ type: 'mathNode', attrs: { latex: 'x^2' } }] }] },
    ] })
    const saved = latest()
    expect(saved.html).not.toContain('适应页面')
    expect(saved.html).not.toContain('360 ×')
    editor.commands.undo()
    expect(node().attrs.width).toBe(300)
    expect(node().child(0).attrs.minHeight).toBe(40)
    editor.commands.redo()
    expect(node().attrs.width).toBe(360)
    const reopened = await open(saved)
    expect(reopened.widths()).toEqual([120, 240])
    expect(reopened.node().child(1).attrs.minHeight).toBe(96)
    const htmlOnly = await open({ schemaVersion: 1, html: saved.html, plainText: saved.plainText })
    expect(htmlOnly.widths()).toEqual([120, 240])
    expect(htmlOnly.node().child(0).attrs.minHeight).toBe(64)
    const canvasTable = richContentToCanvasElements(saved).find((element) => element.type === 'table')
    expect(canvasTable?.colgroup).toEqual([{ width: 120 }, { width: 240 }])
    expect(canvasTable?.trList?.map((row) => row.minHeight)).toEqual([64, 96])
  })

  it.each([0.5, 2])('resizes columns within the page at scale %s, and keeps total width and font marks', async (scale) => {
    const { wrapper, node, widths, handle } = await open(undefined, scale)
    const column = wrapper.get<HTMLElement>('.rich-editor-table-node__column-handle').element
    pointer(column, 'pointerdown')
    pointer(window, 'pointermove', 100 + 20 * scale)
    pointer(window, 'pointerup', 100 + 20 * scale)
    expect(widths()).toEqual([120, 180])
    expect(node().attrs.width).toBe(300)
    pointer(column, 'pointerdown')
    pointer(window, 'pointermove', -1000)
    pointer(window, 'pointerup', -1000)
    expect(widths()).toEqual([40, 260])
    pointer(handle, 'pointerdown')
    pointer(window, 'pointermove', 3000)
    pointer(window, 'pointerup', 3000)
    expect(node().attrs.width).toBe(600)
    expect(widths().reduce((a, b) => a + b)).toBe(600)
  })

  it('resizes one row and protects its actual content height', async () => {
    const { wrapper, node, table } = await open()
    vi.spyOn(table.rows[0]!.cells[0]!.firstElementChild!, 'getBoundingClientRect').mockReturnValue({ height: 76 } as DOMRect)
    const row = wrapper.get<HTMLElement>('.rich-editor-table-node__row-handle').element
    pointer(row, 'pointerdown')
    pointer(window, 'pointermove', 100, 0)
    pointer(window, 'pointerup', 100, 0)
    expect(node().child(0).attrs.minHeight).toBeGreaterThanOrEqual(78)
    expect(node().child(1).attrs.minHeight).toBe(60)
    row.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true }))
    expect(node().child(0).attrs.minHeight).toBeGreaterThanOrEqual(88)
  })

  it.each(['pointercancel', 'blur'])('restores geometry without saving after %s', async (event) => {
    const { wrapper, editor, table, handle } = await open()
    pointer(handle, 'pointerdown')
    pointer(window, 'pointermove', 160, 150)
    expect(table.style.width).toBe('360px')
    if (event === 'blur') window.dispatchEvent(new Event('blur'))
    else pointer(window, event, 160, 150)
    flushDom(editor)
    await flushPromises()
    expect(table.style.width).toBe('300px')
    expect(table.rows[0]!.style.height).toBe('40px')
    expect(wrapper.emitted('update:modelValue')).toBeUndefined()
  })

  it('ignores other pointers, no-motion clicks and a drag on a read-only editor', async () => {
    const { wrapper, editor, handle, table } = await open()
    pointer(handle, 'pointerdown')
    pointer(window, 'pointermove', 200, 200, 2)
    pointer(window, 'pointerup', 200, 200, 2)
    pointer(window, 'pointerup')
    expect(wrapper.emitted('update:modelValue')).toBeUndefined()
    editor.setEditable(false)
    pointer(handle, 'pointerdown')
    pointer(window, 'pointermove', 200, 200)
    pointer(window, 'pointerup', 200, 200)
    expect(table.style.width).toBe('300px')
  })

  it('fits and resets a table and keeps row/column insertion working', async () => {
    const { wrapper, editor, node, widths } = await open()
    const buttons = wrapper.findAll('.rich-editor-table-node__toolbar button')
    await buttons[0]!.trigger('click')
    expect(node().attrs.width).toBe(600)
    expect(widths()).toEqual([200, 400])
    editor.commands.setTextSelection(4)
    expect(editor.commands.addColumnAfter()).toBe(true)
    expect(TableMap.get(node()).width).toBe(3)
    expect(editor.commands.addRowAfter()).toBe(true)
    expect(node().childCount).toBe(3)
    await buttons[1]!.trigger('click')
    expect(node().attrs.width).toBeNull()
    node().forEach((row) => {
      expect(row.attrs.minHeight).toBeNull()
      row.forEach((cell) => expect(cell.attrs.colwidth).toBeNull())
    })
  })

  it('updates every spanned column without losing merged cells, formulas or managed images', async () => {
    vi.spyOn(backend, 'getManagedImage').mockResolvedValue({ resourceId: 'table-image', mimeType: 'image/png', dataBase64: 'AA==', widthPx: 80, heightPx: 40 })
    const { wrapper, node, latest, handle } = await open({ schemaVersion: 1, plainText: '合并', html:
      '<table data-table-width="300"><tr><td colspan="2" colwidth="100,200"><p>合并<span class="math-node" data-latex="\\sqrt{x}">root</span><img data-resource-id="table-image" width="80" height="40"></p></td></tr><tr><td colwidth="100"><p>A</p></td><td colwidth="200"><p>B</p></td></tr></table>' })
    handle.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowRight', bubbles: true }))
    expect(node().child(0).child(0).attrs).toMatchObject({ colspan: 2, colwidth: [104, 206] })
    expect(latest().html).toContain('data-latex="\\sqrt{x}"')
    expect(latest().html).toContain('data-resource-id="table-image"')
    expect(wrapper.get('img').attributes('src')).toBe('data:image/png;base64,AA==')
    expect(latest().html).not.toContain('data:image/')
  })

  it('keeps vertically merged cells editable when resizing and changing text', async () => {
    const { wrapper, editor, node, handle } = await open({ schemaVersion: 1, plainText: 'A B C', html:
      '<table data-table-width="300"><tr><td rowspan="2" colwidth="100"><p>A</p></td><td colwidth="200"><p>B</p></td></tr><tr><td colwidth="200"><p>C</p></td></tr></table>' })
    handle.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowRight', bubbles: true }))
    expect(node().child(0).child(0).attrs).toMatchObject({ rowspan: 2, colwidth: [104] })
    expect(node().child(1).child(0).attrs.colwidth).toEqual([206])
    editor.commands.setTextSelection(4)
    editor.commands.insertContent('new')
    flushDom(editor)
    await flushPromises()
    expect(wrapper.get('td').text()).toBe('newA')
    expect(node().child(0).child(0).attrs.rowspan).toBe(2)
  })
})

const physicsFixture = process.env.TKUP_TABLE_PHYSICS_PREPARED_JSON
describe.runIf(Boolean(physicsFixture))('supplied Wuxi physics paper', () => {
  it('imports and resizes the exact question 12 table, retaining all eighteen cells', async () => {
    const prepared = JSON.parse(readFileSync(physicsFixture!, 'utf8')) as Pick<BeginWordImportResult, 'analysis' | 'images' | 'formulas' | 'tables'>
    const questions = recognizeWordQuestions(buildWordAnalysisLines(prepared.analysis, prepared.images, prepared.formulas, prepared.tables), fallbackQuestionTypes)
    const question = questions.find((item) => item.sourceNumber === 12)!
    const sourceTable = question.stemLines.flatMap((line) => line.tables ?? [])[0]!
    expect(sourceTable.widthPx).toBe(555)
    expect(sourceTable.columnWidthsPx).toEqual([92, 93, 93, 93, 93, 93])
    const content = buildWordImportRichContent(question.stemLines, prepared.formulas, [], 'stem')
    // No resource hydration is necessary to check table geometry in this source paper.
    vi.spyOn(backend, 'getManagedImage').mockResolvedValue({ resourceId: 'fixture', mimeType: 'image/png', dataBase64: 'AA==', widthPx: 100, heightPx: 100 })
    const { wrapper, editor, handle, latest, table } = await open(content)
    expect(table.rows).toHaveLength(3)
    expect(Array.from(table.rows).map((row) => Array.from(row.cells, (cell) => cell.textContent))).toEqual([
      ['序号', '①', '②', '③', '④', '⑤'],
      ['U/V', '0.8', '1.0', '1.5', '2.0', '2.5'],
      ['I/A', '0.20', '0.22', '0.26', '0.30', '0.32'],
    ])
    pointer(handle, 'pointerdown')
    pointer(window, 'pointermove', 0, 130)
    flushDom(editor)
    expect(wrapper.emitted('update:modelValue')).toBeUndefined()
    pointer(window, 'pointerup', 0, 130)
    expect(table.style.width).toBe('455px')
    const reopened = await open(latest())
    expect(reopened.table.style.width).toBe('455px')
    expect(reopened.table.querySelectorAll('td')).toHaveLength(18)
    expect(reopened.table.textContent).toContain('0.32')
    const canvas = richContentToCanvasElements(latest()).find((element) => element.type === 'table')!
    expect(canvas.colgroup?.reduce((sum, column) => sum + column.width, 0)).toBe(455)
    expect(canvas.trList).toHaveLength(3)
    expect(canvas.trList?.every((row) => row.tdList.length === 6)).toBe(true)
  })
})
