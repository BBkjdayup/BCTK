import katex from 'katex'

const KATEX_OPTIONS = {
  displayMode: false,
  throwOnError: false,
  strict: false,
  trust: false,
  maxExpand: 1_000,
  errorColor: '#b91c1c',
} as const

export function renderMathInto(element: HTMLElement, latex: string) {
  const source = latex.trim()
  element.replaceChildren()
  element.dataset.mathRendered = 'true'
  element.setAttribute('aria-label', source || '空公式')
  if (!source) return
  katex.render(source, element, KATEX_OPTIONS)
}

export function renderMathNodes(root: ParentNode) {
  const nodes = root instanceof HTMLElement && root.matches('.math-node[data-latex]')
    ? [root]
    : [...root.querySelectorAll<HTMLElement>('.math-node[data-latex]')]
  for (const node of nodes) {
    renderMathInto(node, node.dataset.latex ?? '')
  }
}
