import type {
  ExportContentMode,
  Paper,
  QuestionContentSlot,
  RichContent,
  WordTemplate,
} from '../types/domain'

export const EXPORT_CONTENT_MODE_LABELS: Record<ExportContentMode, string> = {
  paper_only: '仅试卷',
  answers_only: '仅答案',
  paper_and_answers: '试卷 + 答案',
  paper_answers_explanations: '试卷 + 答案 + 解析',
}

export const EXPORT_CONTENT_MODE_HELP: Record<ExportContentMode, string> = {
  paper_only: '输出题目和选项，不输出答案与解析。',
  answers_only: '只输出题号和答案，适合单独制作答案页。',
  paper_and_answers: '先输出试卷，再输出答案。',
  paper_answers_explanations: '输出试卷、答案以及题目解析。',
}

export function requiredTemplateAnchors(_mode: ExportContentMode): string[] {
  // The first W1 exporter writes every selected section into one managed body
  // region. Optional dedicated answer/explanation anchors can be added later
  // without making today's compatible templates unusable.
  return ['ZT_QUESTIONS']
}

export interface TemplateExportAvailability {
  usable: boolean
  reason: string
  missingAnchors: string[]
}

const SUPPORTED_TEMPLATE_ANCHORS = new Set([
  'ZT_TITLE',
  'ZT_QUESTIONS',
  'ZT_ANSWERS',
  'ZT_EXPLANATIONS',
])

export function templateExportAvailability(
  template: WordTemplate,
  mode: ExportContentMode,
): TemplateExportAvailability {
  if (!template.fileAvailable) {
    return { usable: false, reason: '受管模板文件不存在，请重新导入。', missingAnchors: [] }
  }
  if (template.packageKind !== 'document') {
    return { usable: false, reason: '试卷导出只支持 .docx 文档模板。', missingAnchors: [] }
  }
  if (template.analysisStatus === 'failed') {
    return { usable: false, reason: '模板安全分析未通过。', missingAnchors: [] }
  }

  const counts = new Map<string, number>()
  for (const anchor of template.anchors) {
    const name = anchor.name.trim().toLocaleUpperCase('en-US')
    counts.set(name, (counts.get(name) ?? 0) + 1)
  }
  const required = requiredTemplateAnchors(mode)
  const missingAnchors = required.filter((name) => !counts.has(name))
  if (missingAnchors.length) {
    return {
      usable: false,
      reason: `缺少 ${missingAnchors.join('、')} 替换区域。`,
      missingAnchors,
    }
  }
  const duplicated = [...counts.entries()]
    .filter(([, count]) => count > 1)
    .map(([name]) => name)
  if (duplicated.length) {
    return {
      usable: false,
      reason: `${duplicated.join('、')} 替换区域重复，请修正模板后重新导入。`,
      missingAnchors: [],
    }
  }
  const unknownAnchors = [...counts.keys()]
    .filter((name) => name.startsWith('ZT_') && !SUPPORTED_TEMPLATE_ANCHORS.has(name))
  if (unknownAnchors.length) {
    return {
      usable: false,
      reason: `包含暂不支持的 ${unknownAnchors.join('、')} 替换区域。`,
      missingAnchors: [],
    }
  }
  return { usable: true, reason: '', missingAnchors: [] }
}

function safeFilenamePart(value: string) {
  return value
    .normalize('NFKC')
    .replace(/[<>:"/\\|?*\u0000-\u001f]/g, '_')
    .replace(/\s+/g, ' ')
    .trim()
    .replace(/[ .]+$/g, '')
}

export function suggestedPaperDocxName(
  title: string,
  pattern = '{title}_{date}',
  now = new Date(),
): string {
  const safeTitle = safeFilenamePart(title) || '未命名试卷'
  const date = [
    now.getFullYear(),
    String(now.getMonth() + 1).padStart(2, '0'),
    String(now.getDate()).padStart(2, '0'),
  ].join('-')
  let base = pattern
    .split('{title}').join(safeTitle)
    .split('{date}').join(date)
  base = safeFilenamePart(base.replace(/\.docx$/i, '')) || `${safeTitle}_${date}`
  base = [...base].slice(0, 180).join('').replace(/[ .]+$/g, '') || '试卷'
  if (/^(?:con|prn|aux|nul|com[1-9]|lpt[1-9])(?:\..*)?$/i.test(base)) base = `_${base}`
  return `${base}.docx`
}

function contentHasSourceOoxml(content: RichContent) {
  return Boolean(content.sourceOoxml?.trim())
}

function includedSlots(mode: ExportContentMode): Set<QuestionContentSlot> {
  if (mode === 'answers_only') return new Set(['answer'])
  if (mode === 'paper_and_answers') return new Set(['stem', 'option', 'answer'])
  if (mode === 'paper_answers_explanations') {
    return new Set(['stem', 'option', 'answer', 'explanation'])
  }
  return new Set(['stem', 'option'])
}

export interface PaperExportContentRisk {
  managedImageReferenceCount: number
  sourceOoxmlFragmentCount: number
  inlineImageCount: number
  tableCount: number
  mathNodeCount: number
  unresolvedImageNodeCount: number
  malformedMathNodeCount: number
  complexHtmlFragmentCount: number
  hasUnsupportedContent: boolean
}

function countMatches(value: string, pattern: RegExp) {
  return value.match(pattern)?.length ?? 0
}

const UNSUPPORTED_HTML_NEEDLES = [
  '<math',
  '<svg',
  '<canvas',
  '<video',
  '<audio',
  '<iframe',
  '<object',
  '<embed',
  'src="data:',
  "src='data:",
  'katex',
  'mathquill',
] as const

function inspectHtml(content: RichContent) {
  const html = content.html ?? ''
  const lower = html.toLowerCase()
  const imageTags = html.match(/<img\b[^>]*>/gi) ?? []
  const mathTags = html.match(/<(?:span|div)\b[^>]*(?:class\s*=\s*["'][^"']*\bmath-node\b|data-latex\s*=)[^>]*>/gi) ?? []
  const unresolvedImageNodeCount = imageTags
    .filter((tag) => !/\bdata-resource-id\s*=\s*["'][^"']+["']/i.test(tag))
    .length
  const malformedMathNodeCount = mathTags
    .filter((tag) => !/\bdata-latex\s*=\s*["'][^"']+?["']/i.test(tag))
    .length
  const unsupportedTableMerge = /<(?:td|th)\b[^>]*\browspan\s*=\s*["']?(?:[2-9]|[1-9]\d+)/i.test(html)
  return {
    inlineImageCount: imageTags.length,
    tableCount: countMatches(html, /<table\b/gi),
    mathNodeCount: mathTags.length,
    unresolvedImageNodeCount,
    malformedMathNodeCount,
    hasUnsupportedHtml: unresolvedImageNodeCount > 0
      || malformedMathNodeCount > 0
      || unsupportedTableMerge
      || UNSUPPORTED_HTML_NEEDLES.some((needle) => lower.includes(needle)),
  }
}

export function inspectPaperExportContent(
  paper: Paper,
  mode: ExportContentMode,
): PaperExportContentRisk {
  const slots = includedSlots(mode)
  let managedImageReferenceCount = 0
  let sourceOoxmlFragmentCount = 0
  let inlineImageCount = 0
  let tableCount = 0
  let mathNodeCount = 0
  let unresolvedImageNodeCount = 0
  let malformedMathNodeCount = 0
  let complexHtmlFragmentCount = 0
  const inspectContent = (content: RichContent) => {
    const inspected = inspectHtml(content)
    inlineImageCount += inspected.inlineImageCount
    tableCount += inspected.tableCount
    mathNodeCount += inspected.mathNodeCount
    unresolvedImageNodeCount += inspected.unresolvedImageNodeCount
    malformedMathNodeCount += inspected.malformedMathNodeCount
    if (inspected.hasUnsupportedHtml) complexHtmlFragmentCount += 1
  }
  for (const item of paper.items) {
    const question = item.snapshot
    managedImageReferenceCount += (question.resourceRefs ?? [])
      .filter((reference) => slots.has(reference.contentSlot))
      .length
    if (slots.has('stem')) {
      if (contentHasSourceOoxml(question.stem)) sourceOoxmlFragmentCount += 1
      inspectContent(question.stem)
    }
    if (slots.has('option')) {
      sourceOoxmlFragmentCount += question.options
        .filter((option) => contentHasSourceOoxml(option.content))
        .length
      question.options.forEach((option) => inspectContent(option.content))
    }
    if (slots.has('answer')) {
      if (contentHasSourceOoxml(question.answer)) sourceOoxmlFragmentCount += 1
      inspectContent(question.answer)
    }
    if (slots.has('explanation')) {
      if (contentHasSourceOoxml(question.explanation)) sourceOoxmlFragmentCount += 1
      inspectContent(question.explanation)
    }
  }
  return {
    managedImageReferenceCount,
    sourceOoxmlFragmentCount,
    inlineImageCount,
    tableCount,
    mathNodeCount,
    unresolvedImageNodeCount,
    malformedMathNodeCount,
    complexHtmlFragmentCount,
    hasUnsupportedContent: sourceOoxmlFragmentCount > 0
      || complexHtmlFragmentCount > 0,
  }
}
