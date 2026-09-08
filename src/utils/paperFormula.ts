import type { Paper, RichContent } from '../types/domain'
import type { PaperCanvasFormulaChange, PaperCanvasFormulaReference } from './paperLayout'

const INLINE_MATH_PATTERN = /\\\(([\s\S]*?)\\\)/g

function updateFormulaHtml(
  html: string,
  reference: PaperCanvasFormulaReference,
  latex: string,
) {
  if (!html.trim() || typeof DOMParser === 'undefined') return { value: html, changed: false }
  const document = new DOMParser().parseFromString(
    `<div data-paper-formula-root>${html}</div>`,
    'text/html',
  )
  const root = document.querySelector<HTMLElement>('[data-paper-formula-root]')
  if (!root) return { value: html, changed: false }
  const formulas = [...root.querySelectorAll<HTMLElement>('.math-node[data-latex]')]
  const target = (
    reference.nodeId
      ? formulas.find((node) => node.dataset.nodeId === reference.nodeId)
      : undefined
  ) ?? formulas[reference.ordinal]
  if (!target) return { value: html, changed: false }
  target.dataset.latex = latex
  target.title = latex
  target.textContent = latex
  return { value: root.innerHTML, changed: true }
}

function updateFormulaDocument(
  source: Record<string, unknown> | undefined,
  reference: PaperCanvasFormulaReference,
  latex: string,
) {
  if (!source) return { value: source, changed: false }
  const document = structuredClone(source)
  const formulas: Record<string, unknown>[] = []
  function visit(node: unknown) {
    if (Array.isArray(node)) {
      node.forEach(visit)
      return
    }
    if (!node || typeof node !== 'object') return
    if (Reflect.get(node, 'type') === 'mathNode') formulas.push(node as Record<string, unknown>)
    visit(Reflect.get(node, 'content'))
  }
  visit(document)
  const target = (
    reference.nodeId
      ? formulas.find((node) => {
          const attrs = Reflect.get(node, 'attrs')
          return attrs && typeof attrs === 'object' && Reflect.get(attrs, 'nodeId') === reference.nodeId
        })
      : undefined
  ) ?? formulas[reference.ordinal]
  if (!target) return { value: source, changed: false }
  const attrs = Reflect.get(target, 'attrs')
  Reflect.set(target, 'attrs', {
    ...(attrs && typeof attrs === 'object' ? attrs : {}),
    latex,
  })
  return { value: document, changed: true }
}

function updateFormulaPlainText(value: string, ordinal: number, latex: string) {
  let index = 0
  let changed = false
  const updated = value.replace(INLINE_MATH_PATTERN, (match) => {
    if (index !== ordinal) {
      index += 1
      return match
    }
    index += 1
    changed = true
    return `\\(${latex}\\)`
  })
  return { value: updated, changed }
}

export function updateRichContentFormula(
  content: RichContent,
  reference: PaperCanvasFormulaReference,
  latex: string,
): RichContent | null {
  const html = updateFormulaHtml(content.html, reference, latex)
  const document = updateFormulaDocument(content.document, reference, latex)
  const plainText = updateFormulaPlainText(content.plainText, reference.ordinal, latex)
  if (!html.changed && !document.changed && !plainText.changed) return null
  const { sourceOoxml: _discardStaleSourceOoxml, ...rest } = content
  return {
    ...rest,
    html: html.value,
    plainText: plainText.value,
    document: document.value,
  }
}

/**
 * Updates only the paper snapshot. The source question in the question bank is
 * deliberately left untouched, so a layout-time edit belongs to this paper.
 */
export function applyPaperFormulaChange(paper: Paper, change: PaperCanvasFormulaChange) {
  const itemIndex = paper.items.findIndex((item) => item.id === change.reference.paperItemId)
  if (itemIndex < 0) return false
  const sourceItem = paper.items[itemIndex]
  if (!sourceItem) return false
  const snapshot = structuredClone(sourceItem.snapshot)
  let updated: RichContent | null = null
  if (change.reference.contentSlot === 'stem') {
    updated = updateRichContentFormula(snapshot.stem, change.reference, change.latex)
    if (updated) snapshot.stem = updated
  } else if (change.reference.contentSlot === 'answer') {
    updated = updateRichContentFormula(snapshot.answer, change.reference, change.latex)
    if (updated) snapshot.answer = updated
  } else if (change.reference.contentSlot === 'explanation') {
    updated = updateRichContentFormula(snapshot.explanation, change.reference, change.latex)
    if (updated) snapshot.explanation = updated
  } else {
    const optionIndex = snapshot.options.findIndex((option) => option.id === change.reference.optionId)
    const option = snapshot.options[optionIndex]
    if (!option) return false
    updated = updateRichContentFormula(option.content, change.reference, change.latex)
    if (updated) snapshot.options[optionIndex] = { ...option, content: updated }
  }
  if (!updated) return false

  const timestamp = Date.now()
  snapshot.updatedAt = timestamp
  snapshot.contentVersion += 1
  paper.items = paper.items.map((item, index) => (
    index === itemIndex ? { ...item, snapshot } : item
  ))
  paper.updatedAt = timestamp
  return true
}
