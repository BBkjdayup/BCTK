import {
  ElementType,
  getElementListByHTML,
  ImageDisplay,
  PaperDirection,
  RowFlex,
  type IEditorData,
  type IEditorOption,
  type IElement,
} from '@hufe921/canvas-editor'
import {
  QUESTION_TYPE_LABELS,
  type Paper,
  type PaperItem,
  type PaperPageSetup,
  type QuestionContentSlot,
  type QuestionType,
  type QuestionTypeDefinition,
  type RichContent,
  type TemplatePageSetup,
  type TemplateLayoutPreview,
  type TemplateFontTheme,
  type TemplateStylePrototype,
} from '../types/domain'
import { effectiveQuestionTypes, questionTypeLabel } from './questionTypes'

// This identifies the local adapter as well as the upstream editor. Changing
// it lets old template previews be regenerated when our OOXML mapping improves.
export const CANVAS_EDITOR_VERSION = '0.9.137-zhitiku.12'

export const PAPER_FORMULA_RENDER_SCALE_VERSION = 1

export interface PaperCanvasFormulaReference {
  kind: 'paper-formula'
  paperItemId: string
  contentSlot: QuestionContentSlot
  optionId?: string
  nodeId?: string
  ordinal: number
  sourceLatex: string
}

export interface PaperCanvasFormulaExtension extends PaperCanvasFormulaReference {
  renderScaleVersion?: number
}

export interface PaperCanvasFormulaChange {
  reference: PaperCanvasFormulaReference
  latex: string
}

export interface PaperCanvasImageSource {
  src: string
  widthPx?: number | null
  heightPx?: number | null
}

export type PaperCanvasImageSources = ReadonlyMap<string, PaperCanvasImageSource>

export const DEFAULT_CANVAS_EDITOR_OPTIONS: IEditorOption = {
  locale: 'zhCN',
  defaultFont: 'Microsoft YaHei',
  defaultSize: 16,
  width: 794,
  height: 1123,
  scale: 1,
  pageGap: 20,
  margins: [72, 72, 72, 72],
  defaultRowMargin: 1,
  printPixelRatio: 2,
}

function hashString(value: string) {
  let hash = 0x811c9dc5
  for (let index = 0; index < value.length; index += 1) {
    hash ^= value.charCodeAt(index)
    hash = Math.imul(hash, 0x01000193)
  }
  return (hash >>> 0).toString(16).padStart(8, '0')
}

export function paperLayoutSourceSignature(paper: Paper) {
  const source = [
    paper.title,
    paper.exportContentMode,
    paper.preferredTemplateId ?? '',
    ...paper.items.map((item) => [
      item.id,
      item.position,
      item.snapshot.contentVersion,
      item.snapshot.updatedAt,
    ].join(':')),
  ].join('|')
  return `paper-v1:${paper.items.length}:${hashString(source)}`
}

const WORD_NAMESPACE = 'http://schemas.openxmlformats.org/wordprocessingml/2006/main'

function decodeHexXml(value: string) {
  if (!value || value.length % 2 !== 0) return ''
  const bytes = new Uint8Array(value.length / 2)
  for (let index = 0; index < value.length; index += 2) {
    const byte = Number.parseInt(value.slice(index, index + 2), 16)
    if (!Number.isFinite(byte)) return ''
    bytes[index / 2] = byte
  }
  return new TextDecoder().decode(bytes)
}

function fragmentDocument(value: string) {
  const xml = decodeHexXml(value)
  if (!xml) return null
  const document = new DOMParser().parseFromString(
    `<root xmlns:w="${WORD_NAMESPACE}">${xml}</root>`,
    'application/xml',
  )
  return document.querySelector('parsererror') ? null : document
}

function firstElement(document: Document | null, localName: string) {
  return document?.getElementsByTagNameNS('*', localName).item(0) ?? null
}

function wordAttribute(element: Element | null, localName = 'val') {
  return element?.getAttributeNS(WORD_NAMESPACE, localName)
    ?? element?.getAttribute(`w:${localName}`)
    ?? element?.getAttribute(localName)
    ?? null
}

function wordToggle(document: Document | null, localName: string) {
  const element = firstElement(document, localName)
  if (!element) return undefined
  const value = (wordAttribute(element) ?? '1').toLowerCase()
  return !['0', 'false', 'off', 'none'].includes(value)
}

function themeFont(themeName: string | null, theme?: TemplateFontTheme | null) {
  if (!themeName || !theme) return undefined
  const normalized = themeName.toLowerCase()
  if (normalized.startsWith('major')) {
    return normalized.includes('eastasia')
      ? theme.majorEastAsia ?? theme.majorLatin ?? undefined
      : theme.majorLatin ?? theme.majorEastAsia ?? undefined
  }
  if (normalized.startsWith('minor')) {
    return normalized.includes('eastasia')
      ? theme.minorEastAsia ?? theme.minorLatin ?? undefined
      : theme.minorLatin ?? theme.minorEastAsia ?? undefined
  }
  return undefined
}

function wordColor(value: string | null) {
  if (!value || value === 'auto' || value === 'none') return undefined
  return /^[0-9a-f]{6}$/i.test(value) ? `#${value}` : undefined
}

function wordHighlight(value: string | null) {
  if (!value || value === 'none') return undefined
  const colors: Record<string, string> = {
    black: '#000000',
    blue: '#0000ff',
    cyan: '#00ffff',
    green: '#00a000',
    magenta: '#ff00ff',
    red: '#ff0000',
    yellow: '#ffff00',
    white: '#ffffff',
    darkBlue: '#000080',
    darkCyan: '#008080',
    darkGreen: '#008000',
    darkMagenta: '#800080',
    darkRed: '#800000',
    darkYellow: '#808000',
    darkGray: '#808080',
    lightGray: '#c0c0c0',
  }
  return colors[value]
}

function templateStyle(
  prototype?: TemplateStylePrototype | null,
  fontTheme?: TemplateFontTheme | null,
  page?: TemplatePageSetup | null,
): Partial<IElement> {
  if (!prototype) return {}
  const paragraph = fragmentDocument(prototype.paragraphProperties)
  const run = fragmentDocument(prototype.runProperties)
  const fonts = firstElement(run, 'rFonts')
  const font = wordAttribute(fonts, 'eastAsia')
    ?? wordAttribute(fonts, 'ascii')
    ?? wordAttribute(fonts, 'hAnsi')
    ?? themeFont(
      wordAttribute(fonts, 'eastAsiaTheme')
        ?? wordAttribute(fonts, 'asciiTheme')
        ?? wordAttribute(fonts, 'hAnsiTheme'),
      fontTheme,
    )
    ?? undefined
  const halfPoints = Number(wordAttribute(firstElement(run, 'sz')))
  const size = Number.isFinite(halfPoints) && halfPoints > 0
    ? Math.round((halfPoints / 2) * (96 / 72))
    : undefined
  const spacing = firstElement(paragraph, 'spacing')
  const line = Number(wordAttribute(spacing, 'line'))
  const lineRule = wordAttribute(spacing, 'lineRule')?.toLowerCase()
  const spacingLineHeight = Number.isFinite(line) && line > 0
    ? lineRule === 'auto' || !lineRule
      ? (size ?? 16) * (line / 240)
      : twipsToPixels(line)
    : undefined
  const gridType = page?.documentGridType?.toLowerCase()
  const gridLinePitchTwips = page?.documentGridLinePitchTwips ?? 0
  const gridApplies = Boolean(
    gridLinePitchTwips > 0
      && (!gridType || gridType === 'lines' || gridType === 'linesandchars')
      && wordToggle(paragraph, 'snapToGrid') !== false,
  )
  const gridLineHeight = gridApplies
    ? twipsToPixels(gridLinePitchTwips)
    : undefined
  const targetLineHeight = spacingLineHeight == null
    ? gridLineHeight
    : gridLineHeight == null
      ? spacingLineHeight
      : Math.max(spacingLineHeight, gridLineHeight)
  // canvas-editor applies its basic 8 px row margin both above and below a
  // row. Word's line height is the total row height, so divide the difference
  // by 16 rather than by 8.
  const rowMargin = targetLineHeight == null
    ? undefined
    : Math.min(5, Math.max(0, (targetLineHeight - (size ?? 16)) / 16))
  const alignment = wordAttribute(firstElement(paragraph, 'jc'))
  const rowFlex = alignment === 'center'
    ? RowFlex.CENTER
    : alignment === 'right' || alignment === 'end'
      ? RowFlex.RIGHT
      : alignment === 'both' || alignment === 'distribute'
        ? RowFlex.JUSTIFY
        : alignment === 'left' || alignment === 'start'
          ? RowFlex.LEFT
          : undefined
  return {
    font,
    size,
    bold: wordToggle(run, 'b'),
    italic: wordToggle(run, 'i'),
    underline: wordToggle(run, 'u'),
    strikeout: wordToggle(run, 'strike'),
    color: wordColor(wordAttribute(firstElement(run, 'color'))),
    highlight: wordHighlight(wordAttribute(firstElement(run, 'highlight'))),
    rowFlex,
    rowMargin,
  }
}

function mergeElementStyle(style: Partial<IElement>, element: IElement): IElement {
  const result = { ...style } as IElement
  for (const [key, value] of Object.entries(element)) {
    if (value !== undefined) Reflect.set(result, key, value)
  }
  return result
}

function twipsToPixels(value: number) {
  return value * (96 / 1440)
}

function emuToPixels(value: number) {
  return value / 9525
}

export function createPaperCanvasOptions(
  template?: TemplateLayoutPreview | null,
  pageSetup?: PaperPageSetup | null,
): IEditorOption {
  const overrideWidth = pageSetup ? Math.round(pageSetup.widthMm * (96 / 25.4)) : null
  const overrideHeight = pageSetup ? Math.round(pageSetup.heightMm * (96 / 25.4)) : null
  if (!template) {
    if (!overrideWidth || !overrideHeight) return { ...DEFAULT_CANVAS_EDITOR_OPTIONS }
    const isLandscape = overrideWidth > overrideHeight
    return {
      ...DEFAULT_CANVAS_EDITOR_OPTIONS,
      width: Math.min(overrideWidth, overrideHeight),
      height: Math.max(overrideWidth, overrideHeight),
      paperDirection: isLandscape ? PaperDirection.HORIZONTAL : PaperDirection.VERTICAL,
    }
  }
  const templateWidth = Math.round(twipsToPixels(template.page.widthTwips))
  const templateHeight = Math.round(twipsToPixels(template.page.heightTwips))
  const sourceWidth = overrideWidth ?? templateWidth
  const sourceHeight = overrideHeight ?? templateHeight
  const validPageSize = Number.isFinite(sourceWidth)
    && Number.isFinite(sourceHeight)
    && sourceWidth >= 300
    && sourceWidth <= 3000
    && sourceHeight >= 300
    && sourceHeight <= 3000
  const resolvedWidth = validPageSize
    ? sourceWidth
    : Number(DEFAULT_CANVAS_EDITOR_OPTIONS.width)
  const resolvedHeight = validPageSize
    ? sourceHeight
    : Number(DEFAULT_CANVAS_EDITOR_OPTIONS.height)
  const isLandscape = resolvedWidth > resolvedHeight
  // canvas-editor stores the unrotated portrait dimensions and swaps them
  // internally when paperDirection is horizontal.
  const safeWidth = Math.min(resolvedWidth, resolvedHeight)
  const safeHeight = Math.max(resolvedWidth, resolvedHeight)
  const sourceMargins = [
    template.page.marginTopTwips,
    template.page.marginRightTwips,
    template.page.marginBottomTwips,
    template.page.marginLeftTwips,
  ].map((value, index) => {
    const pixels = Math.max(0, Math.round(twipsToPixels(value)))
    const dimension = index % 2 === 0 ? resolvedHeight : resolvedWidth
    return Math.min(pixels, Math.floor(dimension * 0.4))
  }) as [number, number, number, number]
  // For horizontal paper canvas-editor rotates [top,right,bottom,left] to
  // [right,bottom,left,top], so store the inverse rotation here.
  const margins = isLandscape
    ? [
        sourceMargins[3],
        sourceMargins[0],
        sourceMargins[1],
        sourceMargins[2],
      ] as [number, number, number, number]
    : sourceMargins
  const baseStyle = templateStyle(
    template.styleProfile?.roles.question
      ?? template.blocks.find((block) => block.style)?.style,
    template.fontTheme,
    template.page,
  )
  return {
    ...DEFAULT_CANVAS_EDITOR_OPTIONS,
    width: safeWidth,
    height: safeHeight,
    margins,
    paperDirection: isLandscape ? PaperDirection.HORIZONTAL : PaperDirection.VERTICAL,
    column: template.page.columnCount > 1
      ? {
          count: Math.min(Math.max(Math.round(template.page.columnCount), 1), 8),
          gap: Math.max(0, Math.round(twipsToPixels(template.page.columnGapTwips))),
          separator: template.page.columnSeparator,
        }
      : undefined,
    header: {
      top: Math.max(0, Math.round(twipsToPixels(template.page.headerTwips))),
      editable: true,
    },
    footer: {
      bottom: Math.max(0, Math.round(twipsToPixels(template.page.footerTwips))),
      editable: true,
    },
    pageNumber: template.pageNumberFormat
      ? {
          disabled: false,
          format: template.pageNumberFormat,
          rowFlex: RowFlex.CENTER,
          font: baseStyle.font ?? DEFAULT_CANVAS_EDITOR_OPTIONS.defaultFont,
          size: Math.max(12, Math.round((baseStyle.size ?? 16) * 0.8)),
          bottom: Math.max(12, Math.round(twipsToPixels(template.page.footerTwips))),
        }
      : { disabled: true },
    defaultFont: baseStyle.font ?? DEFAULT_CANVAS_EDITOR_OPTIONS.defaultFont,
    defaultSize: baseStyle.size ?? DEFAULT_CANVAS_EDITOR_OPTIONS.defaultSize,
    defaultRowMargin: baseStyle.rowMargin ?? DEFAULT_CANVAS_EDITOR_OPTIONS.defaultRowMargin,
  }
}

function marksToStyle(marks: unknown): Partial<IElement> {
  if (!Array.isArray(marks)) return {}
  const types = new Set(marks
    .map((mark) => mark && typeof mark === 'object' ? Reflect.get(mark, 'type') : null)
    .filter((type): type is string => typeof type === 'string'))
  return {
    bold: types.has('bold') || undefined,
    italic: types.has('italic') || undefined,
    underline: types.has('underline') || undefined,
    strikeout: types.has('strike') || undefined,
    type: types.has('superscript')
      ? ElementType.SUPERSCRIPT
      : types.has('subscript')
        ? ElementType.SUBSCRIPT
        : undefined,
  }
}

function plainTextElement(value: string, style: Partial<IElement> = {}): IElement[] {
  return value ? [{ value, ...style }] : []
}

interface FormulaContext {
  paperItemId: string
  contentSlot: QuestionContentSlot
  optionId?: string
}

interface FormulaTraversalState {
  context?: FormulaContext
  ordinal: number
}

interface FormulaToken {
  latex: string
  reference?: PaperCanvasFormulaReference
}

/** canvas-editor 0.9.137 recognizes the long inequality aliases only. */
export function normalizeCanvasLatex(value: string) {
  return value
    .replace(/\\le(?![A-Za-z])/g, '\\leq')
    .replace(/\\ge(?![A-Za-z])/g, '\\geq')
}

export function isPaperCanvasFormulaReference(value: unknown): value is PaperCanvasFormulaReference {
  if (!value || typeof value !== 'object') return false
  return Reflect.get(value, 'kind') === 'paper-formula'
    && typeof Reflect.get(value, 'paperItemId') === 'string'
    && ['stem', 'option', 'answer', 'explanation'].includes(String(Reflect.get(value, 'contentSlot')))
    && Number.isInteger(Reflect.get(value, 'ordinal'))
}

function createFormulaReference(
  state: FormulaTraversalState,
  latex: string,
  nodeId?: string | null,
): PaperCanvasFormulaReference | undefined {
  const ordinal = state.ordinal
  state.ordinal += 1
  if (!state.context) return undefined
  return {
    kind: 'paper-formula',
    ...state.context,
    nodeId: nodeId?.trim() || undefined,
    ordinal,
    sourceLatex: latex,
  }
}

function paperFormulaElement(latex: string, reference?: PaperCanvasFormulaReference): IElement {
  const value = normalizeCanvasLatex(latex)
  return {
    type: ElementType.LATEX,
    value,
    id: reference
      ? `paper-formula-${hashString([
          reference.paperItemId,
          reference.contentSlot,
          reference.optionId ?? '',
          reference.nodeId ?? reference.ordinal,
        ].join('|'))}`
      : undefined,
    externalId: reference ? `paper-formula:${reference.paperItemId}:${reference.contentSlot}:${reference.ordinal}` : undefined,
    extension: reference,
    imgPreviewDisabled: true,
    imgToolDisabled: true,
  }
}

const LEGACY_MATH_TOKEN_PREFIX = '\uE000zhitiku-math-'
const LEGACY_MATH_TOKEN_SUFFIX = '\uE001'
const LEGACY_MATH_TOKEN_PATTERN = /\uE000zhitiku-math-(\d+)\uE001/g
const INLINE_MATH_PATTERN = /\\\(([\s\S]*?)\\\)/g
const LEGACY_HTML_INNER_WIDTH = (
  (DEFAULT_CANVAS_EDITOR_OPTIONS.width ?? 794)
  - (DEFAULT_CANVAS_EDITOR_OPTIONS.margins?.[1] ?? 72)
  - (DEFAULT_CANVAS_EDITOR_OPTIONS.margins?.[3] ?? 72)
)

function splitLegacyMathTokens(element: IElement, formulas: readonly FormulaToken[]): IElement[] {
  const value = element.value
  if (!value.includes(LEGACY_MATH_TOKEN_PREFIX)) return [element]

  const result: IElement[] = []
  let offset = 0
  LEGACY_MATH_TOKEN_PATTERN.lastIndex = 0
  for (
    let match = LEGACY_MATH_TOKEN_PATTERN.exec(value);
    match;
    match = LEGACY_MATH_TOKEN_PATTERN.exec(value)
  ) {
    if (match.index > offset) {
      result.push({ ...element, value: value.slice(offset, match.index) })
    }
    const formula = formulas[Number(match[1])]
    if (formula?.latex) result.push({ ...element, ...paperFormulaElement(formula.latex, formula.reference) })
    offset = match.index + match[0].length
  }
  if (offset < value.length) result.push({ ...element, value: value.slice(offset) })
  return result
}

function restoreLegacyMathElements(elements: readonly IElement[], formulas: readonly FormulaToken[]): IElement[] {
  return elements.flatMap((source) => {
    const element = { ...source }
    if (Array.isArray(element.valueList)) {
      element.valueList = restoreLegacyMathElements(element.valueList, formulas)
    }
    if (Array.isArray(element.trList)) {
      element.trList = element.trList.map((row) => ({
        ...row,
        tdList: row.tdList.map((cell) => ({
          ...cell,
          value: restoreLegacyMathElements(cell.value ?? [], formulas),
        })),
      }))
    }
    if (element.control && Array.isArray(element.control.value)) {
      element.control = {
        ...element.control,
        value: restoreLegacyMathElements(element.control.value, formulas),
      }
    }
    return splitLegacyMathTokens(element, formulas)
  })
}

function positiveNumber(value: string | null | undefined) {
  const parsed = Number.parseFloat(value ?? '')
  return Number.isFinite(parsed) && parsed > 0 ? parsed : null
}

function prepareHtmlImage(
  document: Document,
  image: HTMLImageElement,
  imageSources?: PaperCanvasImageSources,
) {
  const resourceId = image.dataset.resourceId?.trim() ?? ''
  const source = resourceId ? imageSources?.get(resourceId) : undefined
  if (source?.src) image.src = source.src

  if (!image.src) {
    image.replaceWith(document.createTextNode('[图片暂时无法读取]'))
    return
  }

  const intrinsicWidth = source?.widthPx && source.widthPx > 0 ? source.widthPx : null
  const intrinsicHeight = source?.heightPx && source.heightPx > 0 ? source.heightPx : null
  let width = positiveNumber(image.getAttribute('width')) ?? intrinsicWidth ?? 240
  let height = positiveNumber(image.getAttribute('height'))
    ?? (intrinsicWidth && intrinsicHeight ? width * intrinsicHeight / intrinsicWidth : 160)
  if (width > LEGACY_HTML_INNER_WIDTH) {
    const scale = LEGACY_HTML_INNER_WIDTH / width
    width = LEGACY_HTML_INNER_WIDTH
    height *= scale
  }
  image.width = Math.max(1, Math.round(width))
  image.height = Math.max(1, Math.round(height))
}

function legacyHtmlElements(
  value: string,
  imageSources?: PaperCanvasImageSources,
  formulaContext?: FormulaContext,
): IElement[] {
  if (!value.trim() || typeof DOMParser === 'undefined') return []
  const document = new DOMParser().parseFromString(
    `<div data-legacy-rich-content-root>${value}</div>`,
    'text/html',
  )
  const root = document.querySelector<HTMLElement>('[data-legacy-rich-content-root]')
  if (!root) return []

  root.querySelectorAll('script, style, iframe, object, embed').forEach((node) => node.remove())
  root.querySelectorAll<HTMLImageElement>('img').forEach((image) => {
    prepareHtmlImage(document, image, imageSources)
  })
  root.querySelectorAll<HTMLTableRowElement>('table tr').forEach((row) => {
    if (!positiveNumber(row.style.height)) row.style.height = '32px'
  })
  const formulas: FormulaToken[] = []
  const formulaState: FormulaTraversalState = { context: formulaContext, ordinal: 0 }
  root.querySelectorAll<HTMLElement>('.math-node[data-latex]').forEach((node) => {
    const latex = node.getAttribute('data-latex')?.trim() ?? ''
    if (!latex) {
      node.replaceWith(document.createTextNode(node.textContent ?? ''))
      return
    }
    const token = `${LEGACY_MATH_TOKEN_PREFIX}${formulas.length}${LEGACY_MATH_TOKEN_SUFFIX}`
    formulas.push({
      latex,
      reference: createFormulaReference(formulaState, latex, node.dataset.nodeId),
    })
    node.replaceWith(document.createTextNode(token))
  })

  const elements = getElementListByHTML(root.innerHTML, {
    innerWidth: LEGACY_HTML_INNER_WIDTH,
  })
  return restoreLegacyMathElements(elements, formulas)
}

function plainTextMathElements(value: string, formulaContext?: FormulaContext): IElement[] {
  if (!value) return []
  const result: IElement[] = []
  const formulaState: FormulaTraversalState = { context: formulaContext, ordinal: 0 }
  let offset = 0
  INLINE_MATH_PATTERN.lastIndex = 0
  for (
    let match = INLINE_MATH_PATTERN.exec(value);
    match;
    match = INLINE_MATH_PATTERN.exec(value)
  ) {
    result.push(...plainTextElement(value.slice(offset, match.index)))
    const latex = match[1]?.trim() ?? ''
    if (latex) result.push(paperFormulaElement(
      latex,
      createFormulaReference(formulaState, latex),
    ))
    offset = match.index + match[0].length
  }
  result.push(...plainTextElement(value.slice(offset)))
  return result
}

function documentElements(node: unknown, formulaState: FormulaTraversalState): IElement[] {
  if (!node || typeof node !== 'object') return []
  const type = Reflect.get(node, 'type')
  const attrs = Reflect.get(node, 'attrs')
  const content = Reflect.get(node, 'content')
  if (type === 'text') {
    return plainTextElement(String(Reflect.get(node, 'text') ?? ''), marksToStyle(Reflect.get(node, 'marks')))
  }
  if (type === 'hardBreak') return [{ value: '\n' }]
  if (type === 'mathNode') {
    const latex = attrs && typeof attrs === 'object' ? String(Reflect.get(attrs, 'latex') ?? '') : ''
    const nodeId = attrs && typeof attrs === 'object' ? String(Reflect.get(attrs, 'nodeId') ?? '') : ''
    return latex ? [paperFormulaElement(
      latex,
      createFormulaReference(formulaState, latex, nodeId),
    )] : []
  }
  if (type === 'image') {
    const src = attrs && typeof attrs === 'object' ? Reflect.get(attrs, 'src') : null
    if (typeof src === 'string' && src) {
      return [{ type: ElementType.IMAGE, value: src, width: 240, height: 160 }]
    }
    return [{ value: '[图片]' }]
  }

  const children = Array.isArray(content) ? content : []
  if (type === 'bulletList' || type === 'orderedList') {
    return children.flatMap((child, index) => [
      { value: type === 'bulletList' ? '• ' : `${index + 1}. ` },
      ...documentElements(child, formulaState),
    ])
  }
  if (type === 'table') {
    const rows = children.flatMap((row) => {
      const cells = row && typeof row === 'object' && Array.isArray(Reflect.get(row, 'content'))
        ? Reflect.get(row, 'content') as unknown[]
        : []
      return [
        ...cells.flatMap((cell, index) => [
          ...documentElements(cell, formulaState),
          ...plainTextElement(index === cells.length - 1 ? '' : '\t'),
        ]),
        { value: '\n' },
      ]
    })
    return rows
  }

  const result = children.flatMap((child) => documentElements(child, formulaState))
  if (['paragraph', 'heading', 'listItem', 'blockquote'].includes(String(type))) {
    result.push({ value: '\n' })
  }
  return result
}

export function richContentToCanvasElements(
  content: RichContent,
  imageSources?: PaperCanvasImageSources,
  formulaContext?: FormulaContext,
): IElement[] {
  if (/<(?:table|img)\b/i.test(content.html)) {
    const structuredElements = legacyHtmlElements(content.html, imageSources, formulaContext)
    if (structuredElements.length) return structuredElements
  }
  if (content.schemaVersion === 2 && content.editor === 'tiptap' && content.document) {
    const elements = documentElements(content.document, { context: formulaContext, ordinal: 0 })
    if (elements.length) return elements
  }
  const htmlElements = legacyHtmlElements(content.html, imageSources, formulaContext)
  if (htmlElements.length) return htmlElements
  return plainTextMathElements(content.plainText, formulaContext)
}

function collectRichContentImageIds(content: RichContent, target: Set<string>) {
  const pattern = /<img\b[^>]*\bdata-resource-id\s*=\s*["']([^"']+)["'][^>]*>/gi
  for (let match = pattern.exec(content.html); match; match = pattern.exec(content.html)) {
    const resourceId = match[1]?.trim()
    if (resourceId) target.add(resourceId)
  }

  function visit(node: unknown) {
    if (Array.isArray(node)) {
      node.forEach(visit)
      return
    }
    if (!node || typeof node !== 'object') return
    const attrs = Reflect.get(node, 'attrs')
    if (Reflect.get(node, 'type') === 'image' && attrs && typeof attrs === 'object') {
      const resourceId = Reflect.get(attrs, 'resourceId')
      if (typeof resourceId === 'string' && resourceId.trim()) target.add(resourceId.trim())
    }
    visit(Reflect.get(node, 'content'))
  }
  visit(content.document)
}

export function paperManagedImageResourceIds(paper: Paper) {
  const ids = new Set<string>()
  for (const item of paper.items) {
    collectRichContentImageIds(item.snapshot.stem, ids)
    item.snapshot.options.forEach((option) => collectRichContentImageIds(option.content, ids))
    collectRichContentImageIds(item.snapshot.answer, ids)
    collectRichContentImageIds(item.snapshot.explanation, ids)
  }
  return [...ids]
}

function appendRichLine(
  target: IElement[],
  prefix: string,
  content: RichContent,
  style: Partial<IElement> = {},
  boldPrefix = true,
  imageSources?: PaperCanvasImageSources,
  formulaContext?: FormulaContext,
) {
  target.push(mergeElementStyle(style, { value: prefix, bold: boldPrefix || style.bold }))
  target.push(...richContentToCanvasElements(content, imageSources, formulaContext)
    .map((element) => mergeElementStyle(style, element)))
  if (!target[target.length - 1]?.value.endsWith('\n')) {
    target.push(mergeElementStyle(style, { value: '\n' }))
  }
}

function appendPaperBody(
  main: IElement[],
  paper: Paper,
  styles: Partial<Record<'sectionHeading' | 'question' | 'option' | 'answer' | 'explanation', Partial<IElement>>> = {},
  templateMode = false,
  imageSources?: PaperCanvasImageSources,
  questionTypes?: readonly QuestionTypeDefinition[],
) {
  const showQuestions = paper.exportContentMode !== 'answers_only'
  const showAnswers = paper.exportContentMode !== 'paper_only'
  const showExplanations = paper.exportContentMode === 'paper_answers_explanations'

  if (showQuestions) {
    if (!templateMode) {
      main.push({ value: '姓名：__________　班级：__________　日期：__________\n\n', rowFlex: RowFlex.CENTER })
    }
    let sectionIndex = 0
    const itemsByType = new Map<QuestionType, PaperItem[]>()
    for (const item of paper.items) {
      const items = itemsByType.get(item.snapshot.type)
      if (items) items.push(item)
      else itemsByType.set(item.snapshot.type, [item])
    }
    const orderedTypes = [...new Set([
      ...effectiveQuestionTypes(questionTypes).map((definition) => definition.code),
      ...itemsByType.keys(),
    ])]
    for (const questionType of orderedTypes) {
      const items = itemsByType.get(questionType) ?? []
      if (!items.length) continue
      sectionIndex += 1
      const headingStyle = styles.sectionHeading ?? { bold: true, size: 20 }
      main.push(mergeElementStyle(headingStyle, {
        value: `${questionSectionHeading(sectionIndex, questionType, items.length, questionTypes)}\n`,
        bold: headingStyle.bold ?? true,
      }))
      for (const item of items) {
        appendRichLine(
          main,
          `${item.position + 1}. `,
          item.snapshot.stem,
          styles.question,
          !templateMode,
          imageSources,
          { paperItemId: item.id, contentSlot: 'stem' },
        )
        item.snapshot.options.forEach((option, index) => {
          const optionStyle = styles.option ?? {}
          main.push(mergeElementStyle(optionStyle, { value: `    ${String.fromCharCode(65 + index)}. ` }))
          main.push(...richContentToCanvasElements(option.content, imageSources, {
            paperItemId: item.id,
            contentSlot: 'option',
            optionId: option.id,
          })
            .map((element) => mergeElementStyle(optionStyle, element)))
          if (!main[main.length - 1]?.value.endsWith('\n')) {
            main.push(mergeElementStyle(optionStyle, { value: '\n' }))
          }
        })
      }
    }
  }

  if (showAnswers) {
    const answerHeadingStyle = styles.sectionHeading ?? { bold: true, size: 20 }
    main.push(mergeElementStyle(answerHeadingStyle, {
      value: '参考答案\n',
      bold: answerHeadingStyle.bold ?? true,
    }))
    paper.items.forEach((item) => appendRichLine(
      main,
      `${item.position + 1}. `,
      item.snapshot.answer.plainText.trim()
        ? item.snapshot.answer
        : { schemaVersion: 1, html: '', plainText: '（未填写）' },
      styles.answer,
      !templateMode,
      imageSources,
      { paperItemId: item.id, contentSlot: 'answer' },
    ))
  }

  if (showExplanations) {
    const explanationHeadingStyle = styles.sectionHeading ?? { bold: true, size: 20 }
    main.push(mergeElementStyle(explanationHeadingStyle, {
      value: '\n题目解析\n',
      bold: explanationHeadingStyle.bold ?? true,
    }))
    paper.items.forEach((item) => appendRichLine(
      main,
      `${item.position + 1}. `,
      item.snapshot.explanation.plainText.trim()
        ? item.snapshot.explanation
        : { schemaVersion: 1, html: '', plainText: '（未填写）' },
      styles.explanation,
      !templateMode,
      imageSources,
      { paperItemId: item.id, contentSlot: 'explanation' },
    ))
  }
}

function chineseSectionNumber(index: number) {
  const digits = ['零', '一', '二', '三', '四', '五', '六', '七', '八', '九']
  if (index <= 9) return digits[index] ?? String(index)
  if (index === 10) return '十'
  if (index < 20) return `十${digits[index % 10]}`
  if (index < 100 && index % 10 === 0) return `${digits[Math.floor(index / 10)]}十`
  if (index < 100) return `${digits[Math.floor(index / 10)]}十${digits[index % 10]}`
  return String(index)
}

const PAPER_SECTION_LABELS: Record<string, string> = {
  ...QUESTION_TYPE_LABELS,
  single_choice: '选择题',
  multiple_choice: '多项选择题',
}

function questionSectionHeading(
  sectionIndex: number,
  questionType: QuestionType,
  questionCount: number,
  definitions?: readonly QuestionTypeDefinition[],
) {
  const label = PAPER_SECTION_LABELS[questionType] ?? questionTypeLabel(questionType, definitions)
  return `${chineseSectionNumber(sectionIndex)}、${label}：本题共${questionCount}个小题，每小题      分。共      分。`
}

function escapeSvgText(value: string) {
  return value
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
}

function sideSealTspans(
  runs: NonNullable<TemplateLayoutPreview['blocks'][number]['sideSeal']>['lines'][number]['runs'],
) {
  if (!runs.length) return ''
  return runs.map((run) => {
    const size = run.sizeHalfPoints && run.sizeHalfPoints > 0
      ? (run.sizeHalfPoints / 2) * (96 / 72)
      : 16
    const decoration = run.underline ? ' text-decoration="underline"' : ''
    return `<tspan font-size="${size.toFixed(2)}"${decoration}>${escapeSvgText(run.text)}</tspan>`
  }).join('')
}

function sideSealTextPlacement(
  alignment: string | null | undefined,
  vertical270: boolean,
  height: number,
) {
  const normalized = alignment?.toLowerCase()
  if (normalized === 'center') {
    return {
      anchor: 'middle',
      y: height / 2,
    }
  }
  if (normalized === 'right' || normalized === 'end') {
    return {
      anchor: 'end',
      y: vertical270 ? 24 : height - 24,
    }
  }
  return {
    anchor: 'start',
    y: vertical270 ? height - 24 : 24,
  }
}

function normalizeParagraphBreakStyles(elements: IElement[]) {
  const normalized: IElement[] = []
  for (const element of elements) {
    const parts = element.value.split(/(\n)/).filter(Boolean)
    if (parts.length <= 1) {
      normalized.push(element)
      continue
    }
    normalized.push(...parts.map((value) => ({ ...element, value })))
  }

  let nextRowFlex: RowFlex | undefined
  let nextRowMargin: number | undefined
  for (let index = normalized.length - 1; index >= 0; index -= 1) {
    const element = normalized[index]
    if (element.value !== '\n') {
      nextRowFlex = element.rowFlex
      nextRowMargin = element.rowMargin
      continue
    }
    element.rowFlex = nextRowFlex ?? RowFlex.LEFT
    if (nextRowMargin != null) element.rowMargin = nextRowMargin
  }
  return normalized
}

function sideSealElement(
  template: TemplateLayoutPreview,
  block: TemplateLayoutPreview['blocks'][number],
): IElement | null {
  if (block.kind !== 'sideSeal') return null
  const pageHeight = Math.max(300, Math.round(twipsToPixels(template.page.heightTwips)))
  const pageWidth = Math.max(300, Math.round(twipsToPixels(template.page.widthTwips)))
  const geometry = block.sideSeal
  const sourceWidth = geometry?.widthEmu ? emuToPixels(geometry.widthEmu) : 102
  const sourceHeight = geometry?.heightEmu ? emuToPixels(geometry.heightEmu) : pageHeight
  const width = Math.min(Math.max(sourceWidth, 24), pageWidth * 0.4)
  const height = Math.min(Math.max(sourceHeight, 100), pageHeight * 1.5)
  const lineRatio = geometry?.lineXEmu != null && geometry.widthEmu > 0
    ? geometry.lineXEmu / geometry.widthEmu
    : 0.64
  const lineX = Math.min(width - 2, Math.max(2, width * lineRatio))
  const lineWidth = geometry?.lineWidthEmu
    ? Math.min(6, Math.max(0.75, emuToPixels(geometry.lineWidthEmu)))
    : 1
  const lineDash = geometry?.lineDash && geometry.lineDash !== 'solid'
    ? '8 7'
    : undefined
  const lines = geometry?.lines ?? []
  const vertical270 = geometry?.textDirection?.toLowerCase() === 'vert270'
  const identityRuns = lines[0]?.runs?.length
    ? sideSealTspans(lines[0].runs)
    : '<tspan>班级：____________　姓名：____________　考号：____________</tspan>'
  const warningRuns = lines[1]?.runs?.length
    ? sideSealTspans(lines[1].runs)
    : '<tspan>密              封              线              内              不              准              答              题</tspan>'
  const identityX = Math.max(12, lineX * 0.45)
  const warningX = Math.max(identityX + 18, lineX - Math.max(3, width * 0.06))
  const identityPlacement = sideSealTextPlacement(lines[0]?.alignment, vertical270, height)
  const warningPlacement = sideSealTextPlacement(lines[1]?.alignment, vertical270, height)
  const rotation = vertical270 ? -90 : 90
  const identityTransform = `translate(${identityX.toFixed(2)} ${identityPlacement.y.toFixed(2)}) rotate(${rotation})`
  const warningTransform = `translate(${warningX.toFixed(2)} ${warningPlacement.y.toFixed(2)}) rotate(${rotation})`
  const svg = [
    `<svg xmlns="http://www.w3.org/2000/svg" width="${width}" height="${height}" viewBox="0 0 ${width} ${height}">`,
    `<line x1="${lineX.toFixed(2)}" y1="2" x2="${lineX.toFixed(2)}" y2="${(height - 2).toFixed(2)}" stroke="#111827" stroke-width="${lineWidth.toFixed(2)}"${lineDash ? ` stroke-dasharray="${lineDash}"` : ''}/>`,
    '<g fill="#111827" font-family="SimSun,宋体,serif" font-size="16">',
    `<text xml:space="preserve" text-anchor="${identityPlacement.anchor}" transform="${identityTransform}">${identityRuns}</text>`,
    `<text xml:space="preserve" text-anchor="${warningPlacement.anchor}" transform="${warningTransform}">${warningRuns}</text>`,
    '</g></svg>',
  ].join('')
  const x = geometry?.pageXEmu != null ? emuToPixels(geometry.pageXEmu) : 0
  const y = geometry?.pageYEmu != null ? emuToPixels(geometry.pageYEmu) : 0
  return {
    type: ElementType.IMAGE,
    value: `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`,
    width,
    height,
    imgDisplay: ImageDisplay.FLOAT_TOP,
    imgFloatPosition: {
      x: Math.min(pageWidth, Math.max(-width, x)),
      y: Math.min(pageHeight, Math.max(-height, y)),
      pageNo: 0,
    },
    imgToolDisabled: true,
    imgPreviewDisabled: true,
  }
}

export function createPaperCanvasData(
  paper: Paper,
  template?: TemplateLayoutPreview | null,
  imageSources?: PaperCanvasImageSources,
  questionTypes?: readonly QuestionTypeDefinition[],
): IEditorData {
  if (!template) {
    const main: IElement[] = [
      { value: `${paper.title.trim() || '未命名试卷'}\n`, bold: true, size: 26, rowFlex: RowFlex.CENTER },
    ]
    appendPaperBody(main, paper, {}, false, imageSources, questionTypes)
    return { main: normalizeParagraphBreakStyles(main) }
  }

  const roleStyles = {
    sectionHeading: templateStyle(
      template.styleProfile?.roles.sectionHeading,
      template.fontTheme,
      template.page,
    ),
    question: templateStyle(template.styleProfile?.roles.question, template.fontTheme, template.page),
    option: templateStyle(template.styleProfile?.roles.option, template.fontTheme, template.page),
    answer: templateStyle(template.styleProfile?.roles.answer, template.fontTheme, template.page),
    explanation: templateStyle(
      template.styleProfile?.roles.explanation,
      template.fontTheme,
      template.page,
    ),
  }
  const titleStyle = templateStyle(
    template.styleProfile?.roles.title,
    template.fontTheme,
    template.page,
  )
  const main: IElement[] = []
  const seals = template.blocks
    .map((block) => sideSealElement(template, block))
    .filter((element): element is IElement => Boolean(element))
  if (seals.length) {
    main.push(...seals)
    // Word keeps the drawing in a collapsed anchor paragraph. Give the canvas
    // float its own minimal row so it cannot consume the title's alignment.
    main.push({ value: '\n', size: 1, rowMargin: 0 })
  }
  let insertedPaperBody = false
  for (const block of template.blocks) {
    if (block.kind === 'static') {
      const style = templateStyle(block.style, template.fontTheme, template.page)
      main.push(mergeElementStyle(style, { value: `${block.text}\n` }))
    } else if (block.kind === 'title') {
      main.push(mergeElementStyle(titleStyle, {
        value: `${paper.title.trim() || '未命名试卷'}\n`,
        bold: titleStyle.bold ?? true,
        rowFlex: titleStyle.rowFlex ?? RowFlex.CENTER,
      }))
    } else if (block.kind === 'questions') {
      appendPaperBody(main, paper, roleStyles, true, imageSources, questionTypes)
      insertedPaperBody = true
    }
  }
  if (!insertedPaperBody) appendPaperBody(main, paper, roleStyles, true, imageSources, questionTypes)
  return { main: normalizeParagraphBreakStyles(main) }
}
