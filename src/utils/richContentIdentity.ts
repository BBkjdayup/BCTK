import type { RichContent } from '../types/domain'
import { richContentSearchText } from './richContent'
const textIdentities = new WeakMap<RichContent, { source: string; identity: string }>()

/** Conservative browser-preview identity. Desktop computes SHA-256 in Rust. */
export function richContentIdentity(value: RichContent, imageContent: (id: string) => string | undefined): string {
  const source = JSON.stringify([value.html, value.plainText, value.document, value.sourceOoxml])
  const cacheable = !source.includes('<img') && !source.includes('"image"')
  const cached = textIdentities.get(value)
  if (cacheable && cached?.source === source) return cached.identity
  const tokens: [string, string][] = []
  const doc = new DOMParser().parseFromString(value.html || '<p></p>', 'text/html')
  const pending: { node: Node; closing: boolean }[] = [{ node: doc.body, closing: false }]
  while (pending.length) {
    const { node, closing } = pending.pop()!
    if (node.nodeType === Node.TEXT_NODE) {
      const text = (node.textContent ?? '').normalize('NFKC').toLowerCase().replace(/[\s\u0085\u200b\ufeff]/gu, '')
      if (text) {
        const previous = tokens[tokens.length - 1]
        if (previous?.[0] === 'text') previous[1] += text
        else tokens.push(['text', text])
      }
      continue
    }
    if (!(node instanceof Element)) continue
    const tag = node.tagName.toLowerCase()
    if (closing) { tokens.push([`close:${tag}`, '']); continue }
    if (node.hasAttribute('data-latex')) { tokens.push(['formula', node.getAttribute('data-latex')!.trim().normalize('NFKC')]); continue }
    if (tag === 'img') {
      const id = node.getAttribute('data-resource-id') ?? ''
      tokens.push(['image', imageContent(id) ?? `unresolved:${id}:${node.getAttribute('src') ?? ''}`])
      continue
    }
    if (['script', 'style', 'template'].includes(tag)) continue
    if (/^(p|h[1-6]|blockquote|[uo]l|li|table|tr|td|th|sup|sub|br)$/u.test(tag)) {
      tokens.push([`open:${tag}`, ['td', 'th'].includes(tag) ? `${node.getAttribute('colspan') ?? '1'},${node.getAttribute('rowspan') ?? '1'}` : ''])
      pending.push({ node, closing: true })
    }
    pending.push(...[...node.childNodes].reverse().map((child) => ({ node: child, closing: false })))
  }
  if (!value.html.trim()) tokens.push(['source-text', richContentSearchText(value).normalize('NFKC').toLowerCase().replace(/\s/gu, '')])
  if (value.sourceOoxml) tokens.push(['ooxml', value.sourceOoxml])
  const identity = JSON.stringify(tokens)
  if (cacheable) textIdentities.set(value, { source, identity })
  return identity
}
