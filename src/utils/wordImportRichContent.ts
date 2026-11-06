import type {
  DocxFormulaOccurrence,
  DocxImageOccurrence,
  QuestionResourceRef,
  RichContent,
} from '../types/domain'
import type { WordAnalysisLine, WordAnalysisTable } from './wordImportRecognition'
import { splitWordImportFormulaSegments, wordImportFormulaPlainText } from './wordImportFormula'
import { fitTableColumns, tablePixelSize } from './tableGeometry'

const escapeHtml = (text: string) => text.replace(/[&<>"']/g, (character) => ({
  '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;',
}[character] ?? character))

function managedImageHtml(image: DocxImageOccurrence) {
  const alt = escapeHtml(image.originalFilename?.trim() || 'Word 图片')
  return `<img data-resource-id="${image.resourceId}" data-node-id="${image.nodeId}" alt="${alt}">`
}

function managedFormulaHtml(formula: DocxFormulaOccurrence) {
  const latex = escapeHtml(formula.latex)
  return `<span class="math-node" data-latex="${latex}" data-node-id="${formula.nodeId}">${latex}</span>`
}

function formulaRichHtml(
  text: string,
  formulasByNodeId: ReadonlyMap<string, DocxFormulaOccurrence>,
) {
  return splitWordImportFormulaSegments(text, formulasByNodeId)
    .map((segment) => segment.kind === 'formula'
      ? managedFormulaHtml(segment.formula)
      : escapeHtml(segment.text))
    .join('')
}

function analysisTableHtml(
  table: WordAnalysisTable,
  formulasByNodeId: ReadonlyMap<string, DocxFormulaOccurrence>,
) {
  const width = tablePixelSize(table.widthPx)
  const sourceWidths = (table.columnWidthsPx ?? []).map(tablePixelSize)
  const widths = width && sourceWidths.length && sourceWidths.every((value): value is number => value !== null)
    ? fitTableColumns(sourceWidths, width)
    : sourceWidths
  const rows = table.rows.map((row, rowIndex) => {
    const cells = row.map((cell, columnIndex) => {
      const content = cell.lines
        .map((line) => analysisLineHtml(line, formulasByNodeId))
        .join('')
      const columnWidth = widths[columnIndex]
      return `<td${columnWidth ? ` colwidth="${columnWidth}"` : ''}>${content || '<p><br></p>'}</td>`
    }).join('')
    const height = tablePixelSize(table.rowMinHeightsPx?.[rowIndex])
    return `<tr${height ? ` data-row-min-height="${height}" style="height: ${height}px"` : ''}>${cells}</tr>`
  }).join('')
  const columns = widths.length && widths.every(Boolean)
    ? `<colgroup>${widths.map((columnWidth) => `<col width="${columnWidth}" style="width: ${columnWidth}px">`).join('')}</colgroup>`
    : ''
  const dimensions = width ? ` data-table-width="${width}" style="width: ${width}px; max-width: 100%; table-layout: fixed"` : ''
  return `<table${dimensions}>${columns}<tbody>${rows}</tbody></table>`
}

function analysisLineHtml(
  line: WordAnalysisLine,
  formulasByNodeId: ReadonlyMap<string, DocxFormulaOccurrence>,
): string {
  const textHtml = formulaRichHtml(line.text, formulasByNodeId)
  const imageHtml = line.images.map(managedImageHtml).join('')
  const paragraphHtml = textHtml || imageHtml ? `<p>${textHtml}${imageHtml}</p>` : ''
  const tableHtml = (line.tables ?? [])
    .map((table) => analysisTableHtml(table, formulasByNodeId))
    .join('')
  return `${paragraphHtml}${tableHtml}`
}

function analysisLineImages(line: WordAnalysisLine): DocxImageOccurrence[] {
  return [
    ...line.images,
    ...(line.tables ?? []).flatMap((table) => table.rows.flatMap(
      (row) => row.flatMap((cell) => cell.lines.flatMap(analysisLineImages)),
    )),
  ]
}

function analysisLinePlainText(
  line: WordAnalysisLine,
  formulasByNodeId: ReadonlyMap<string, DocxFormulaOccurrence>,
): string {
  const text = wordImportFormulaPlainText(line.text, formulasByNodeId)
  const imageText = line.images.map(() => '[图片]').join(' ')
  const tableText = (line.tables ?? []).map((table) => {
    const cells = table.rows.map((row) => row.map((cell) => cell.lines
      .map((cellLine) => analysisLinePlainText(cellLine, formulasByNodeId))
      .filter(Boolean)
      .join(' ')).join(' | ')).join('\n')
    return `[表格]\n${cells}`
  }).join('\n')
  return [text, imageText, tableText].filter(Boolean).join(' ')
}

export function buildWordImportRichContent(
  lines: WordAnalysisLine[],
  formulas: readonly DocxFormulaOccurrence[],
  resourceRefs: QuestionResourceRef[],
  contentSlot: QuestionResourceRef['contentSlot'],
  optionId?: string,
): RichContent {
  const formulasByNodeId = new Map(formulas.map((formula) => [formula.nodeId, formula]))
  const html = lines.map((line) => {
    for (const image of analysisLineImages(line)) {
      resourceRefs.push({
        nodeId: image.nodeId,
        resourceId: image.resourceId,
        contentSlot,
        optionId: contentSlot === 'option' ? optionId ?? null : null,
      })
    }
    return analysisLineHtml(line, formulasByNodeId)
  }).join('')
  const plainText = lines
    .map((line) => analysisLinePlainText(line, formulasByNodeId))
    .join('\n')
    .trim()
  return { schemaVersion: 1, html, plainText }
}
