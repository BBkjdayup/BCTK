import type { JSONContent } from '@tiptap/core'
import type { RichContent } from '../types/domain'

export const TIPTAP_CONTENT_EDITOR_VERSION = '3.31.0'

export const EMPTY_TIPTAP_DOCUMENT: JSONContent = {
  type: 'doc',
  content: [{ type: 'paragraph' }],
}

export function emptyRichContent(): RichContent {
  return {
    schemaVersion: 2,
    editor: 'tiptap',
    editorVersion: TIPTAP_CONTENT_EDITOR_VERSION,
    document: structuredClone(EMPTY_TIPTAP_DOCUMENT) as Record<string, unknown>,
    html: '',
    plainText: '',
  }
}

export function plainTextRichContent(value: string): RichContent {
  const plainText = value.trim()
  if (!plainText) return emptyRichContent()
  return {
    schemaVersion: 2,
    editor: 'tiptap',
    editorVersion: TIPTAP_CONTENT_EDITOR_VERSION,
    document: {
      type: 'doc',
      content: [{
        type: 'paragraph',
        content: [{ type: 'text', text: plainText }],
      }],
    },
    html: `<p>${plainText
      .replace(/&/g, '&amp;')
      .replace(/</g, '&lt;')
      .replace(/>/g, '&gt;')}</p>`,
    plainText,
  }
}

export function plainTextLinesRichContent(lines: readonly string[]): RichContent {
  const normalized = lines.map((line) => line.replace(/\r?\n/g, ''))
  const meaningful = normalized.some((line) => line.trim())
  if (!meaningful) return emptyRichContent()
  const content = normalized.map((line) => ({
    type: 'paragraph',
    content: line ? [{ type: 'text', text: line }] : undefined,
  }))
  return {
    schemaVersion: 2,
    editor: 'tiptap',
    editorVersion: TIPTAP_CONTENT_EDITOR_VERSION,
    document: { type: 'doc', content },
    html: normalized.map((line) => `<p>${line
      .replace(/&/g, '&amp;')
      .replace(/</g, '&lt;')
      .replace(/>/g, '&gt;') || '<br>'}</p>`).join(''),
    plainText: normalized.join('\n').trim(),
  }
}

export function richContentToTiptapDocument(content: RichContent): JSONContent | string {
  if (content.schemaVersion === 2 && content.editor === 'tiptap' && content.document) {
    return content.document as JSONContent
  }
  if (content.html.trim()) return content.html
  if (!content.plainText) return structuredClone(EMPTY_TIPTAP_DOCUMENT)
  return {
    type: 'doc',
    content: [{
      type: 'paragraph',
      content: [{ type: 'text', text: content.plainText }],
    }],
  }
}

/** Search text includes the actual LaTeX, never a synthetic nonempty marker. */
const legacyTextCache = new WeakMap<RichContent, { html: string; text: string }>()

export function richContentSearchText(content: RichContent): string {
  if (content.schemaVersion === 2 && content.document) {
    const read = (node: JSONContent): string => {
      if (node.type === 'text') return node.text ?? ''
      if (node.type === 'mathNode') return String(node.attrs?.latex ?? '').trim()
      if (node.type === 'image') return String(node.attrs?.alt ?? '')
      if (node.type === 'hardBreak') return '\n'
      const text = (node.content ?? []).map(read).join('')
      return ['paragraph', 'heading', 'tableCell', 'tableHeader'].includes(node.type ?? '') ? `${text}\n` : text
    }
    return read(content.document as JSONContent).trim()
  }
  if (!content.html.trim()) return content.plainText.trim()
  const cached = legacyTextCache.get(content)
  if (cached?.html === content.html) return cached.text
  const doc = new DOMParser().parseFromString(content.html, 'text/html')
  doc.querySelectorAll('[data-latex]').forEach((node) => {
    node.replaceWith(doc.createTextNode(node.getAttribute('data-latex')?.trim() ?? ''))
  })
  doc.querySelectorAll('img').forEach((node) => node.replaceWith(doc.createTextNode(node.alt)))
  doc.querySelectorAll('br').forEach((node) => node.replaceWith(doc.createTextNode('\n')))
  const text = (doc.body.textContent ?? '').trim()
  legacyTextCache.set(content, { html: content.html, text })
  return text
}

export function hasMeaningfulRichContent(content: RichContent): boolean {
  if (richContentSearchText(content).replace(/[\s\u200b\ufeff]/gu, '')) return true
  if (content.schemaVersion === 2 && content.document) {
    const hasImage = (node: JSONContent): boolean => (
      node.type === 'image' && Boolean(String(node.attrs?.resourceId ?? '').trim())
    ) || (node.content ?? []).some(hasImage)
    return hasImage(content.document as JSONContent)
  }
  const doc = new DOMParser().parseFromString(content.html, 'text/html')
  return [...doc.querySelectorAll('img[data-resource-id]')]
    .some((node) => Boolean(node.getAttribute('data-resource-id')?.trim()))
}
