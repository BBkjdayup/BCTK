const INLINE_FORMULA_PATTERN = /\\\(([\s\S]*?)\\\)/g

function escapeHtml(value: string) {
  return value.replace(/[&<>"']/g, (character) => ({
    '&': '&amp;',
    '<': '&lt;',
    '>': '&gt;',
    '"': '&quot;',
    "'": '&#39;',
  })[character] ?? character)
}

export function plainTextQuestionSummaryHtml(value: string) {
  const parts: string[] = []
  let offset = 0
  INLINE_FORMULA_PATTERN.lastIndex = 0
  for (
    let match = INLINE_FORMULA_PATTERN.exec(value);
    match;
    match = INLINE_FORMULA_PATTERN.exec(value)
  ) {
    parts.push(escapeHtml(value.slice(offset, match.index)))
    const latex = match[1]?.trim() ?? ''
    if (latex) {
      const escapedLatex = escapeHtml(latex)
      parts.push(`<span class="math-node" data-latex="${escapedLatex}">${escapedLatex}</span>`)
    }
    offset = match.index + match[0].length
  }
  parts.push(escapeHtml(value.slice(offset)))
  return parts.join('').replace(/\r?\n/g, ' ')
}

function assetMarker(document: Document, kind: 'image' | 'table', title?: string | null) {
  const marker = document.createElement('span')
  marker.className = `question-stem-summary__asset question-stem-summary__asset--${kind}`
  marker.textContent = kind === 'image' ? '图片' : '表格'
  marker.setAttribute('aria-label', marker.textContent)
  if (title?.trim()) marker.title = title.trim()
  return marker
}

export function compactQuestionStemHtml(value: string) {
  if (!value.trim()) return ''
  if (typeof DOMParser === 'undefined') return value

  const document = new DOMParser().parseFromString(
    `<div data-question-stem-summary-root>${value}</div>`,
    'text/html',
  )
  const root = document.querySelector<HTMLElement>('[data-question-stem-summary-root]')
  if (!root) return value

  root.querySelectorAll('script, style, iframe, object, embed').forEach((node) => node.remove())
  root.querySelectorAll('table').forEach((table) => {
    table.replaceWith(assetMarker(document, 'table'))
  })
  root.querySelectorAll('img').forEach((image) => {
    const title = image.getAttribute('alt')
    image.replaceWith(assetMarker(document, 'image', title))
  })
  root.querySelectorAll('br').forEach((lineBreak) => lineBreak.replaceWith(' '))

  const blockSelector = 'p, div, h1, h2, h3, h4, h5, h6, li, ul, ol, blockquote, pre'
  const blockElements = [...root.querySelectorAll<HTMLElement>(blockSelector)].reverse()
  for (const element of blockElements) {
    const segment = document.createElement('span')
    segment.className = 'question-stem-summary__segment'
    while (element.firstChild) segment.append(element.firstChild)
    segment.append(document.createTextNode(' '))
    element.replaceWith(segment)
  }

  return root.innerHTML.trim()
}
