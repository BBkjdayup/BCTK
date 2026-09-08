import { describe, expect, it } from 'vitest'
import type { DocxFormulaOccurrence } from '../types/domain'
import {
  injectWordImportFormulaTokens,
  splitWordImportFormulaSegments,
  wordImportFormulaPlainText,
} from './wordImportFormula'

const formula = (
  nodeId: string,
  textCharOffset: number,
  latex: string,
): DocxFormulaOccurrence => ({
  nodeId,
  paragraphIndex: 0,
  textCharOffset,
  latex,
  sourceKind: 'mathtype_mtef5',
  productVersion: 7,
  productSubversion: 0,
})

describe('Word import MathType placement', () => {
  it('keeps multiple converted formulas at their exact text positions', () => {
    const first = formula('019faaaa-1111-7111-8111-111111111111', 1, 'x^{2}')
    const second = formula('019fbbbb-2222-7222-8222-222222222222', 3, '\\frac{1}{2}')
    const text = injectWordImportFormulaTokens(`甲\uFFFC＋\uFFFC乙`, [first, second])
    const byId = new Map([[first.nodeId, first], [second.nodeId, second]])

    expect(wordImportFormulaPlainText(text, byId)).toBe('甲\\(x^{2}\\)＋\\(\\frac{1}{2}\\)乙')
    expect(splitWordImportFormulaSegments(text, byId).map((segment) => segment.kind)).toEqual([
      'text',
      'formula',
      'text',
      'formula',
      'text',
    ])
  })

  it('keeps an unconverted native Word placeholder visible for review', () => {
    expect(wordImportFormulaPlainText('前\uFFFC后', new Map())).toBe('前[公式]后')
  })
})
