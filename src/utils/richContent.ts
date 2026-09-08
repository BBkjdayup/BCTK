import type { JSONContent } from '@tiptap/core'
import type { RichContent } from '../types/domain'

export const TIPTAP_CONTENT_EDITOR_VERSION = '3.28.0'

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
