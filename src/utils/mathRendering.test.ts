import { describe, expect, it } from 'vitest'
import { renderMathInto, renderMathNodes } from './mathRendering'

describe('math rendering', () => {
  it('renders degree superscripts and fractions as KaTeX markup', () => {
    const degree = document.createElement('span')
    renderMathInto(degree, String.raw`675^{\circ}`)
    expect(degree.querySelector('.katex')).not.toBeNull()
    const degreeVisual = degree.querySelector('.katex-html')
    expect(degreeVisual?.textContent).toContain('675')
    expect(degreeVisual?.textContent).toContain('∘')
    expect(degreeVisual?.textContent).not.toContain('\\circ')

    const fraction = document.createElement('span')
    renderMathInto(fraction, String.raw`\frac{11}{4}\pi`)
    expect(fraction.querySelector('.frac-line')).not.toBeNull()
    expect(fraction.textContent).toContain('11')
    expect(fraction.textContent).toContain('4')
    expect(fraction.textContent).toContain('π')
  })

  it('hydrates stored editable formula nodes without changing their source', () => {
    const root = document.createElement('div')
    root.innerHTML = '<span class="math-node" data-latex="x^{2}">x^{2}</span>'
    renderMathNodes(root)

    const formula = root.querySelector<HTMLElement>('.math-node')
    expect(formula?.dataset.latex).toBe('x^{2}')
    expect(formula?.dataset.mathRendered).toBe('true')
    expect(formula?.querySelector('.katex')).not.toBeNull()
  })
})
