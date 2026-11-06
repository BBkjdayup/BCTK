import { Table, TableRow } from '@tiptap/extension-table'
import { createEditorTableNodeView } from './editorTableNodeView'
import { tablePixelSize } from './tableGeometry'

export const EditorTable = Table.extend({
  addAttributes() {
    return {
      ...this.parent?.(),
      width: {
        default: null,
        parseHTML: (element) => tablePixelSize(element.dataset.tableWidth)
          ?? (/^\d+(?:\.\d+)?px$/.test(element.style.width) ? tablePixelSize(Number.parseFloat(element.style.width)) : null),
        renderHTML: (attrs) => {
          const width = tablePixelSize(attrs.width)
          return width ? { 'data-table-width': width, style: `width: ${width}px; max-width: 100%; table-layout: fixed` } : {}
        },
      },
    }
  },
  addNodeView() {
    return createEditorTableNodeView()
  },
}).configure({ resizable: false, allowTableNodeSelection: true, cellMinWidth: 40 })

export const EditorTableRow = TableRow.extend({
  addAttributes() {
    return {
      ...this.parent?.(),
      minHeight: {
        default: null,
        parseHTML: (element) => tablePixelSize(element.dataset.rowMinHeight)
          ?? (/^\d+(?:\.\d+)?px$/.test(element.style.height) ? tablePixelSize(Number.parseFloat(element.style.height)) : null),
        renderHTML: (attrs) => {
          const height = tablePixelSize(attrs.minHeight)
          return height ? { 'data-row-min-height': height, style: `height: ${height}px` } : {}
        },
      },
    }
  },
  addNodeView() {
    return ({ node }) => {
      let currentNode = node
      const dom = document.createElement('tr')
      const apply = () => {
        const height = tablePixelSize(currentNode.attrs.minHeight)
        const value = height ? `${height}px` : ''
        if (dom.style.height !== value) dom.style.height = value
      }
      apply()
      return {
        dom,
        contentDOM: dom,
        update: (updatedNode) => {
          if (updatedNode.type !== currentNode.type) return false
          currentNode = updatedNode
          apply()
          return true
        },
        // A temporary drag height must never be parsed as an edit to cell content.
        ignoreMutation: (mutation) => mutation.type === 'attributes'
          && mutation.target === dom && mutation.attributeName === 'style',
      }
    }
  },
})
