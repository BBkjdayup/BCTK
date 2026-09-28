import type {
  DocxAnalysis,
  DocxFormulaOccurrence,
  DocxImageOccurrence,
  DocxTableOccurrence,
} from '../types/domain'
import { injectWordImportFormulaTokens } from './wordImportFormula'
import type { WordAnalysisLine, WordAnalysisTable } from './wordImportRecognition'

/**
 * Builds the exact logical lines consumed by Word question recognition.
 *
 * Keeping this conversion outside the Vue page lets real DOCX regression
 * fixtures exercise the same formula, image and table placement used by the
 * application instead of maintaining a second test-only parser.
 */
export function buildWordAnalysisLines(
  analysis: DocxAnalysis,
  images: readonly DocxImageOccurrence[],
  formulas: readonly DocxFormulaOccurrence[],
  tables: readonly DocxTableOccurrence[],
): WordAnalysisLine[] {
  const imagesByParagraph = new Map<number, DocxImageOccurrence[]>()
  for (const image of images) {
    const entries = imagesByParagraph.get(image.paragraphIndex) ?? []
    entries.push(image)
    imagesByParagraph.set(image.paragraphIndex, entries)
  }

  const formulasByParagraph = new Map<number, DocxFormulaOccurrence[]>()
  for (const formula of formulas) {
    const entries = formulasByParagraph.get(formula.paragraphIndex) ?? []
    entries.push(formula)
    formulasByParagraph.set(formula.paragraphIndex, entries)
  }

  const baseLines = new Map<number, WordAnalysisLine>(analysis.paragraphs
    .map((paragraph) => {
      const paragraphFormulas = formulasByParagraph.get(paragraph.index) ?? []
      const originalCharacters = [...paragraph.text]
      const injected = injectWordImportFormulaTokens(paragraph.text, paragraphFormulas)
      const leadingTrim = [...injected].length - [...injected.trimStart()].length
      const trimmed = injected.trim()
      const trimmedCharacters = [...trimmed]
      const paragraphImages = (imagesByParagraph.get(paragraph.index) ?? []).map((image) => {
        const precedingFormulaLength = paragraphFormulas
          .filter((formula) => formula.textCharOffset < image.textCharOffset)
          .reduce((total, formula) => {
            const tokenLength = [...`\uE000${formula.nodeId}\uE001`].length
            const replacedLength = originalCharacters[formula.textCharOffset] === '\uFFFC' ? 1 : 0
            return total + tokenLength - replacedLength
          }, 0)
        const charOffset = Math.max(0, image.textCharOffset + precedingFormulaLength - leadingTrim)
        return {
          ...image,
          // Option markers use RegExp indices (UTF-16 units), while the DOCX
          // reader records Unicode character offsets.
          textCharOffset: trimmedCharacters.slice(0, charOffset).join('').length,
        }
      })
      return [paragraph.index, {
        paragraphIndex: paragraph.index,
        text: trimmed,
        images: paragraphImages,
        tables: [],
      }] as const
    }))

  const tableParagraphIndices = new Set<number>()
  const tablesByFirstParagraph = new Map<number, WordAnalysisTable[]>()
  for (const table of tables) {
    const paragraphIndices = table.rows
      .flatMap((row) => row.flatMap((cell) => cell.paragraphIndices))
    const firstParagraphIndex = paragraphIndices[0]
    if (firstParagraphIndex === undefined) continue
    paragraphIndices.forEach((index) => tableParagraphIndices.add(index))
    const analysisTable: WordAnalysisTable = {
      rows: table.rows.map((row) => row.map((cell) => ({
        lines: cell.paragraphIndices
          .map((index) => baseLines.get(index))
          .filter((line): line is WordAnalysisLine => Boolean(line)),
      }))),
    }
    const entries = tablesByFirstParagraph.get(firstParagraphIndex) ?? []
    entries.push(analysisTable)
    tablesByFirstParagraph.set(firstParagraphIndex, entries)
  }

  return analysis.paragraphs
    .flatMap((paragraph) => {
      if (!tableParagraphIndices.has(paragraph.index)) {
        const line = baseLines.get(paragraph.index)
        return line ? [line] : []
      }
      const nestedTables = tablesByFirstParagraph.get(paragraph.index)
      return nestedTables?.length
        ? [{
            paragraphIndex: paragraph.index,
            text: '',
            images: [],
            tables: nestedTables,
          }]
        : []
    })
    .filter((line) => Boolean(line.text || line.images.length || line.tables?.length))
}
