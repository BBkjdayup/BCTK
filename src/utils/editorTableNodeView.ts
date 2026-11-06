import type { NodeViewRenderer } from '@tiptap/core'
import { closeHistory } from '@tiptap/pm/history'
import { TableMap } from '@tiptap/pm/tables'
import { fitTableColumns as distribute } from './tableGeometry'

const positive = (value: unknown) => {
  const number = Number(value)
  return Number.isFinite(number) && number > 0 && number <= 10000 ? number : null
}
const sum = (values: readonly number[]) => values.reduce((total, value) => total + value, 0)

/** Editable tbody remains owned by ProseMirror; resizing controls live outside it. */
export function createEditorTableNodeView(): NodeViewRenderer {
  return ({ node, editor, getPos, view }) => {
    let currentNode = node
    let destroyed = false
    let cleanupDrag: (() => void) | null = null
    let queued = false
    const dom = document.createElement('div')
    dom.className = 'rich-editor-table-node tableWrapper'
    const table = document.createElement('table')
    const colgroup = document.createElement('colgroup')
    const contentDOM = document.createElement('tbody')
    table.append(colgroup, contentDOM)
    const controls = document.createElement('div')
    controls.className = 'rich-editor-table-node__controls'
    controls.contentEditable = 'false'
    const handle = document.createElement('button')
    handle.type = 'button'
    handle.className = 'rich-editor-table-node__handle'
    handle.setAttribute('aria-label', '拖动调整表格大小')
    handle.title = '拖动调整表格大小'
    const label = document.createElement('span')
    label.className = 'rich-editor-table-node__size'
    const toolbar = document.createElement('div')
    toolbar.className = 'rich-editor-table-node__toolbar'
    const fit = document.createElement('button')
    fit.type = 'button'
    fit.textContent = '适应页面'
    const reset = document.createElement('button')
    reset.type = 'button'
    reset.textContent = '恢复默认'
    toolbar.append(fit, reset)
    controls.append(handle, label, toolbar)
    dom.append(table, controls)
    const columnHandles: HTMLButtonElement[] = []
    const rowHandles: HTMLButtonElement[] = []

    const scale = () => {
      const root = view.dom
      return positive(root.offsetWidth ? root.getBoundingClientRect().width / root.offsetWidth : 1) ?? 1
    }
    const availableWidth = () => {
      let ancestor = dom.parentElement
      const widths: number[] = []
      while (ancestor) {
        const style = window.getComputedStyle(ancestor)
        const width = ancestor.clientWidth - (Number.parseFloat(style.paddingLeft) || 0) - (Number.parseFloat(style.paddingRight) || 0)
        if (width > 0) widths.push(width)
        if (ancestor === view.dom) break
        ancestor = ancestor.parentElement
      }
      return widths.length ? Math.min(...widths) : 650
    }
    const columnMinimum = (count: number) => Math.min(40, availableWidth() / Math.max(1, count))

    const savedColumns = () => {
      const map = TableMap.get(currentNode)
      const widths = Array.from({ length: Math.max(1, map.width) }, () => 0)
      currentNode.forEach((row, rowOffset) => row.forEach((cell, cellOffset) => {
        const rectangle = map.findCell(rowOffset + 1 + cellOffset)
        for (let index = 0; index < rectangle.right - rectangle.left; index += 1) {
          const width = positive(cell.attrs.colwidth?.[index])
          if (width) widths[rectangle.left + index] = width
        }
      }))
      const total = positive(currentNode.attrs.width) ?? (widths.every(Boolean) ? sum(widths) : availableWidth())
      const unspecified = widths.filter((width) => !width).length
      const remaining = Math.max(columnMinimum(widths.length), (total - sum(widths)) / (unspecified || 1))
      return distribute(widths.map((width) => width || remaining), Math.min(total, availableWidth()), columnMinimum(widths.length))
    }
    const savedRows = () => Array.from({ length: currentNode.childCount }, (_, index) => positive(currentNode.child(index).attrs.minHeight) ?? 32)
    const measuredRows = () => savedRows().map((fallback, index) => positive(table.rows[index]?.getBoundingClientRect().height / scale()) ?? fallback)
    const measuredColumns = () => {
      const widths = savedColumns()
      const displayed = positive(table.getBoundingClientRect().width / scale())
      return displayed ? distribute(widths, displayed, columnMinimum(widths.length)) : widths
    }
    const minimumRows = () => {
      const heights = savedRows().map(() => 24)
      Array.from(table.rows).forEach((row, rowIndex) => {
        Array.from(row.cells).forEach((cell) => {
          const style = window.getComputedStyle(cell)
          let height = (Number.parseFloat(style.paddingTop) || 0) + (Number.parseFloat(style.paddingBottom) || 0) + 2
          for (const child of Array.from(cell.children)) {
            const childStyle = window.getComputedStyle(child)
            height += child.getBoundingClientRect().height / scale()
              + (Number.parseFloat(childStyle.marginTop) || 0) + (Number.parseFloat(childStyle.marginBottom) || 0)
          }
          const span = Math.min(cell.rowSpan || 1, heights.length - rowIndex)
          for (let offset = 0; offset < span; offset += 1) heights[rowIndex + offset] = Math.max(heights[rowIndex + offset]!, Math.ceil(height / span))
        })
      })
      return heights
    }

    const positionHandles = () => {
      if (destroyed) return
      const currentScale = scale()
      const tableRect = table.getBoundingClientRect()
      const tableWidth = positive(tableRect.width / currentScale) ?? sum(savedColumns())
      const tableHeight = positive(tableRect.height / currentScale) ?? sum(measuredRows())
      const widths = Array.from(colgroup.children, (col) => Number.parseFloat((col as HTMLElement).style.width) || 0)
      let left = 0
      columnHandles.forEach((button, index) => {
        left += widths[index] ?? 0
        button.style.left = `${left}px`
        button.style.height = `${tableHeight}px`
      })
      let top = 0
      measuredRows().forEach((height, index) => {
        top += height
        const button = rowHandles[index]
        if (button) {
          button.style.top = `${top}px`
          button.style.width = `${tableWidth}px`
        }
      })
      label.textContent = `${Math.round(tableWidth)} × ${Math.round(tableHeight)} px`
      handle.style.top = `${tableHeight - 5}px`
      handle.style.left = `${tableWidth - 5}px`
    }
    const queuePosition = () => {
      if (queued) return
      queued = true
      queueMicrotask(() => { queued = false; if (!destroyed) positionHandles() })
    }
    const applyDimensions = (widths: number[], heights: Array<number | null>) => {
      while (colgroup.children.length < widths.length) colgroup.append(document.createElement('col'))
      while (colgroup.children.length > widths.length) colgroup.lastChild?.remove()
      widths.forEach((width, index) => {
        const col = colgroup.children[index] as HTMLElement
        if (col.style.width !== `${width}px`) col.style.width = `${width}px`
      })
      const width = `${sum(widths)}px`
      if (dom.style.width !== width) dom.style.width = width
      if (table.style.width !== width) table.style.width = width
      Array.from(table.rows).forEach((row, index) => {
        const height = heights[index] ? `${heights[index]}px` : ''
        if (row.style.height !== height) row.style.height = height
      })
      positionHandles()
    }
    const renderSaved = () => {
      if (destroyed) return
      applyDimensions(savedColumns(), Array.from({ length: currentNode.childCount }, (_, index) => positive(currentNode.child(index).attrs.minHeight)))
      queuePosition()
    }
    const commit = (widths: number[], heights: Array<number | null>, clear = false) => {
      const position = getPos()
      if (destroyed || editor.isDestroyed || !editor.isEditable || typeof position !== 'number' || editor.state.doc.nodeAt(position) !== currentNode) return
      const map = TableMap.get(currentNode)
      const tr = closeHistory(editor.state.tr)
      const tableWidth = clear ? null : Math.round(sum(widths))
      if (currentNode.attrs.width !== tableWidth) tr.setNodeMarkup(position, undefined, { ...currentNode.attrs, width: tableWidth })
      currentNode.forEach((row, rowOffset, rowIndex) => {
        const minHeight = clear ? null : heights[rowIndex] === null ? null : Math.min(10000, Math.ceil(heights[rowIndex] ?? 32))
        if (row.attrs.minHeight !== minHeight) tr.setNodeMarkup(position + 1 + rowOffset, undefined, { ...row.attrs, minHeight })
        row.forEach((cell, cellOffset) => {
          const relative = rowOffset + 1 + cellOffset
          const rectangle = map.findCell(relative)
          const colwidth = clear ? null : widths.slice(rectangle.left, rectangle.right).map(Math.round)
          if (JSON.stringify(cell.attrs.colwidth) !== JSON.stringify(colwidth)) {
            tr.setNodeMarkup(position + 1 + relative, undefined, { ...cell.attrs, colwidth })
          }
        })
      })
      if (tr.docChanged) view.dispatch(tr)
      else renderSaved()
    }
    const stopDragging = () => { cleanupDrag?.(); cleanupDrag = null }

    const beginDrag = (event: PointerEvent, kind: 'table' | 'column' | 'row', index = 0) => {
      if (event.button > 0 || event.isPrimary === false || !editor.isEditable) return
      event.preventDefault()
      event.stopPropagation()
      stopDragging()
      const target = event.currentTarget as HTMLElement
      const widths = measuredColumns()
      const heights = measuredRows()
      const startX = event.clientX
      const startY = event.clientY
      const currentScale = scale()
      const minimum = columnMinimum(widths.length)
      let nextWidths = [...widths]
      let nextHeights = [...heights]
      let moved = false
      dom.classList.add('is-resizing')
      const matches = (candidate: PointerEvent) => event.pointerId === candidate.pointerId
      const onMove = (candidate: PointerEvent) => {
        if (!matches(candidate)) return
        candidate.preventDefault()
        const dx = (candidate.clientX - startX) / currentScale
        const dy = (candidate.clientY - startY) / currentScale
        moved ||= Math.abs(dx) > 0.1 || Math.abs(dy) > 0.1
        if (kind === 'table') {
          const width = Math.min(availableWidth(), Math.max(minimum * widths.length, sum(widths) + dx))
          nextWidths = distribute(widths, width, minimum)
          const ratio = Math.max(24 * heights.length, sum(heights) + dy) / sum(heights)
          nextHeights = heights.map((height) => height * ratio)
        } else if (kind === 'column') {
          const pair = widths[index]! + widths[index + 1]!
          nextWidths[index] = Math.round(Math.max(minimum, Math.min(pair - minimum, widths[index]! + dx)))
          nextWidths[index + 1] = pair - nextWidths[index]!
        } else {
          nextHeights[index] = Math.max(24, heights[index]! + dy)
        }
        applyDimensions(nextWidths, nextHeights)
        const lowerBounds = minimumRows()
        nextHeights = nextHeights.map((height, row) => Math.min(10000, Math.max(height, lowerBounds[row] ?? 24)))
        applyDimensions(nextWidths, nextHeights)
      }
      const cancel = () => { stopDragging(); renderSaved() }
      const onCancel = (candidate: PointerEvent) => { if (matches(candidate)) cancel() }
      const onUp = (candidate: PointerEvent) => {
        if (!matches(candidate)) return
        stopDragging()
        if (moved) commit(nextWidths, nextHeights)
        else renderSaved()
      }
      window.addEventListener('pointermove', onMove, { passive: false })
      window.addEventListener('pointerup', onUp)
      window.addEventListener('pointercancel', onCancel)
      window.addEventListener('blur', cancel)
      target.addEventListener('lostpointercapture', cancel)
      cleanupDrag = () => {
        window.removeEventListener('pointermove', onMove)
        window.removeEventListener('pointerup', onUp)
        window.removeEventListener('pointercancel', onCancel)
        window.removeEventListener('blur', cancel)
        target.removeEventListener('lostpointercapture', cancel)
        dom.classList.remove('is-resizing')
        if (target.hasPointerCapture?.(event.pointerId)) target.releasePointerCapture(event.pointerId)
      }
      try { target.setPointerCapture?.(event.pointerId) } catch { /* Window listeners cover uncaptured pointers. */ }
    }
    const makeHandle = (kind: 'column' | 'row', index: number) => {
      const button = document.createElement('button')
      button.type = 'button'
      button.className = `rich-editor-table-node__${kind}-handle`
      button.setAttribute('aria-label', kind === 'column' ? `调整第 ${index + 1}、${index + 2} 列宽` : `调整第 ${index + 1} 行高度`)
      button.addEventListener('pointerdown', (event) => beginDrag(event, kind, index))
      button.addEventListener('keydown', (event) => {
        const direction = kind === 'column' ? { ArrowLeft: -1, ArrowRight: 1 } : { ArrowUp: -1, ArrowDown: 1 }
        const change = direction[event.key as keyof typeof direction]
        if (!change) return
        event.preventDefault()
        event.stopPropagation()
        const widths = measuredColumns()
        const heights = measuredRows()
        const step = change * (event.shiftKey ? 50 : 10)
        if (kind === 'column') {
          const pair = widths[index]! + widths[index + 1]!
          const minimum = columnMinimum(widths.length)
          widths[index] = Math.round(Math.max(minimum, Math.min(pair - minimum, widths[index]! + step)))
          widths[index + 1] = pair - widths[index]!
        } else heights[index] = Math.max(minimumRows()[index] ?? 24, heights[index]! + step)
        commit(widths, heights)
      })
      controls.append(button)
      return button
    }
    const updateHandles = () => {
      const count = Math.max(0, TableMap.get(currentNode).width - 1)
      while (columnHandles.length > count) columnHandles.pop()?.remove()
      while (columnHandles.length < count) columnHandles.push(makeHandle('column', columnHandles.length))
      while (rowHandles.length > currentNode.childCount) rowHandles.pop()?.remove()
      while (rowHandles.length < currentNode.childCount) rowHandles.push(makeHandle('row', rowHandles.length))
    }
    const updateSelection = () => {
      if (destroyed) return
      const position = getPos()
      const selection = editor.state.selection
      const selected = typeof position === 'number' && selection.from >= position && selection.to <= position + currentNode.nodeSize
      dom.classList.toggle('is-selected', selected)
      queuePosition()
    }
    controls.addEventListener('pointerdown', (event) => event.preventDefault())
    handle.addEventListener('pointerdown', (event) => beginDrag(event, 'table'))
    handle.addEventListener('keydown', (event) => {
      if (!['ArrowLeft', 'ArrowRight', 'ArrowUp', 'ArrowDown'].includes(event.key)) return
      event.preventDefault()
      event.stopPropagation()
      const widths = measuredColumns()
      const heights = measuredRows()
      const step = (event.shiftKey ? 50 : 10) * (event.key === 'ArrowLeft' || event.key === 'ArrowUp' ? -1 : 1)
      if (event.key === 'ArrowLeft' || event.key === 'ArrowRight') {
        commit(distribute(widths, Math.max(columnMinimum(widths.length) * widths.length, Math.min(availableWidth(), sum(widths) + step)), columnMinimum(widths.length)), heights)
      } else {
        const ratio = Math.max(24 * heights.length, sum(heights) + step) / sum(heights)
        const minimum = minimumRows()
        commit(widths, heights.map((height, index) => Math.max(minimum[index] ?? 24, height * ratio)))
      }
    })
    fit.addEventListener('click', () => commit(distribute(measuredColumns(), availableWidth(), columnMinimum(TableMap.get(currentNode).width)), savedRows()))
    reset.addEventListener('click', () => commit([], [], true))
    const observer = typeof ResizeObserver !== 'undefined' ? new ResizeObserver(() => {
      if (!cleanupDrag) renderSaved()
      else queuePosition()
    }) : null
    observer?.observe(dom)
    editor.on('selectionUpdate', updateSelection)
    updateHandles()
    renderSaved()
    queueMicrotask(() => { if (!destroyed) renderSaved() })
    return {
      dom,
      contentDOM,
      update: (updatedNode) => {
        if (updatedNode.type !== currentNode.type) return false
        stopDragging()
        currentNode = updatedNode
        updateHandles()
        renderSaved()
        updateSelection()
        return true
      },
      selectNode: () => dom.classList.add('is-selected'),
      deselectNode: updateSelection,
      stopEvent: (event) => event.target instanceof window.Node && controls.contains(event.target),
      ignoreMutation: (mutation) => mutation.type !== 'selection'
        && mutation.target instanceof window.Node && (!contentDOM.contains(mutation.target)
          || (mutation.type === 'attributes' && mutation.attributeName === 'style' && mutation.target instanceof HTMLTableRowElement)),
      destroy: () => {
        destroyed = true
        stopDragging()
        observer?.disconnect()
        editor.off('selectionUpdate', updateSelection)
      },
    }
  }
}
