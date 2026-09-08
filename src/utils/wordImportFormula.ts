import type { DocxFormulaOccurrence } from '../types/domain'

const FORMULA_TOKEN_START = '\uE000'
const FORMULA_TOKEN_END = '\uE001'
const FORMULA_TOKEN_PATTERN = /\uE000([0-9a-f-]{36})\uE001/gi
const OBJECT_PLACEHOLDER = '\uFFFC'

function replaceObjectPlaceholders(text: string) {
  return text.split(OBJECT_PLACEHOLDER).join('[公式]')
}

export type WordImportFormulaSegment =
  | { kind: 'text'; text: string }
  | { kind: 'formula'; formula: DocxFormulaOccurrence }

export function injectWordImportFormulaTokens(
  text: string,
  formulas: readonly DocxFormulaOccurrence[],
) {
  const characters = [...text]
  const ordered = [...formulas].sort((left, right) => right.textCharOffset - left.textCharOffset)
  for (const formula of ordered) {
    const token = `${FORMULA_TOKEN_START}${formula.nodeId}${FORMULA_TOKEN_END}`
    if (characters[formula.textCharOffset] === OBJECT_PLACEHOLDER) {
      characters.splice(formula.textCharOffset, 1, token)
    } else {
      characters.splice(formula.textCharOffset, 0, token)
    }
  }
  return characters.join('')
}

export function splitWordImportFormulaSegments(
  text: string,
  formulasByNodeId: ReadonlyMap<string, DocxFormulaOccurrence>,
): WordImportFormulaSegment[] {
  const output: WordImportFormulaSegment[] = []
  let offset = 0
  FORMULA_TOKEN_PATTERN.lastIndex = 0
  for (
    let match = FORMULA_TOKEN_PATTERN.exec(text);
    match;
    match = FORMULA_TOKEN_PATTERN.exec(text)
  ) {
    const plain = replaceObjectPlaceholders(text.slice(offset, match.index))
    if (plain) output.push({ kind: 'text', text: plain })
    const formula = formulasByNodeId.get(match[1] ?? '')
    output.push(formula ? { kind: 'formula', formula } : { kind: 'text', text: '[公式]' })
    offset = match.index + match[0].length
  }
  const tail = replaceObjectPlaceholders(text.slice(offset))
  if (tail) output.push({ kind: 'text', text: tail })
  return output
}

export function wordImportFormulaPlainText(
  text: string,
  formulasByNodeId: ReadonlyMap<string, DocxFormulaOccurrence>,
) {
  return splitWordImportFormulaSegments(text, formulasByNodeId)
    .map((segment) => (
      segment.kind === 'formula' ? `\\(${segment.formula.latex}\\)` : segment.text
    ))
    .join('')
}
