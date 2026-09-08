import type { JSONContent } from '@tiptap/core'
import type {
  DocumentEntryOptionStyle,
  DocumentEntryTemplate,
  DocumentEntryTemplateConfig,
  RichContent,
} from '../types/domain'
import { clonePlain } from './clonePlain'

export const DEFAULT_DOCUMENT_ENTRY_TEMPLATE_ID = 'labeled_fields_v1'

export const DEFAULT_DOCUMENT_ENTRY_TEMPLATE_CONFIG: DocumentEntryTemplateConfig = {
  schemaVersion: 1,
  typeMarker: '题型：',
  stemMarker: '题目：',
  answerMarker: '答案：',
  explanationMarker: '解析：',
  optionStyle: 'letter_dot',
  questionSeparator: '---',
  compatibleDefaultMarkers: true,
  recognizeQuestionNumbers: true,
  stripRecognizedQuestionNumbers: true,
  stemPrompt: '这里输入题目的题干',
  optionPrompt: '这里输入选项',
  answerPrompt: '这里输入参考答案',
  explanationPrompt: '这里输入解析，可以不填',
}

export function systemDocumentEntryTemplate(): DocumentEntryTemplate {
  return {
    id: DEFAULT_DOCUMENT_ENTRY_TEMPLATE_ID,
    name: '默认字段模板',
    config: clonePlain(DEFAULT_DOCUMENT_ENTRY_TEMPLATE_CONFIG),
    isBuiltIn: true,
    isDefault: true,
    createdAt: 0,
    updatedAt: 0,
  }
}

export function normalizeDocumentEntryTemplateConfig(
  config: DocumentEntryTemplateConfig,
): DocumentEntryTemplateConfig {
  return {
    ...clonePlain(config),
    recognizeQuestionNumbers: config.recognizeQuestionNumbers ?? true,
    stripRecognizedQuestionNumbers: config.stripRecognizedQuestionNumbers ?? true,
  }
}

export function optionMarker(style: DocumentEntryOptionStyle, letter: string) {
  if (style === 'letter_comma') return `${letter}、`
  if (style === 'letter_parentheses') return `（${letter}）`
  if (style === 'letter_brackets') return `【${letter}】`
  return `${letter}. `
}

export function buildDocumentEntryTemplateExample(config: DocumentEntryTemplateConfig) {
  const option = (letter: string) => (
    `${optionMarker(config.optionStyle, letter)}${config.optionPrompt} ${letter}`
  )
  const separator = config.questionSeparator ? `\n${config.questionSeparator}\n` : '\n'
  return [
    `${config.typeMarker}可不填，软件会根据选项和答案自动识别`,
    `${config.stemMarker}${config.stemPrompt}`,
    `${option('A')}（选择题时填写）`,
    `${option('B')}（选择题时填写）`,
    `${option('C')}（选择题时填写）`,
    `${option('D')}（选择题时填写）`,
    `${config.answerMarker}A`,
    `${config.explanationMarker}${config.explanationPrompt}`,
    separator,
    `${config.typeMarker}可不填`,
    config.recognizeQuestionNumbers !== false
      ? `2. ${config.stemPrompt}`
      : `${config.stemMarker}${config.stemPrompt}`,
    `${config.answerMarker}${config.answerPrompt}`,
    `${config.explanationMarker}${config.explanationPrompt}`,
  ].join('\n').replace(/\n{3,}/gu, '\n\n')
}

function normalizedComparable(value: string) {
  return value.normalize('NFKC').replace(/\s+/gu, '').toLocaleLowerCase('zh-CN')
}

function markerError(label: string, value: string) {
  if (!value.trim()) return `${label}不能为空。`
  if ([...value].length > 24) return `${label}不能超过 24 个字符。`
  if (/[\r\n\u0000-\u001f\u007f]/u.test(value)) return `${label}不能包含换行或控制字符。`
  return ''
}

export function validateDocumentEntryTemplateConfig(config: DocumentEntryTemplateConfig) {
  if (config.schemaVersion !== 1) return '录题模板的数据版本无效。'
  const markers: Array<[string, string]> = [
    ['题型标记', config.typeMarker],
    ['题目标记', config.stemMarker],
    ['答案标记', config.answerMarker],
    ['解析标记', config.explanationMarker],
  ]
  for (const [label, marker] of markers) {
    const error = markerError(label, marker)
    if (error) return error
  }
  const comparable = markers.map(([, marker]) => normalizedComparable(marker))
  if (new Set(comparable).size !== comparable.length) return '四个字段标记不能相同。'
  for (let left = 0; left < comparable.length; left += 1) {
    for (let right = 0; right < comparable.length; right += 1) {
      if (left !== right && comparable[left]?.startsWith(comparable[right] ?? '')) {
        return '字段标记之间不能互为开头，否则识别时无法区分。'
      }
    }
  }
  if (!['letter_dot', 'letter_comma', 'letter_parentheses', 'letter_brackets']
    .includes(config.optionStyle)) return '选项格式无效。'
  if ([...config.questionSeparator].length > 24) return '题目分隔线不能超过 24 个字符。'
  if (/[\r\n\u0000-\u001f\u007f]/u.test(config.questionSeparator)) {
    return '题目分隔线不能包含换行或控制字符。'
  }
  const prompts: Array<[string, string]> = [
    ['题目提示', config.stemPrompt],
    ['选项提示', config.optionPrompt],
    ['答案提示', config.answerPrompt],
    ['解析提示', config.explanationPrompt],
  ]
  for (const [label, prompt] of prompts) {
    if ([...prompt].length > 100) return `${label}不能超过 100 个字符。`
    if (/[\r\n\u0000-\u001f\u007f]/u.test(prompt)) return `${label}不能包含换行或控制字符。`
  }
  return ''
}

interface PrefixReplacement {
  consumed: number
  replacement: string
}

function leadingWhitespaceLength(value: string) {
  return value.length - value.trimStart().length
}

function primaryMarkerReplacement(
  line: string,
  from: DocumentEntryTemplateConfig,
  to: DocumentEntryTemplateConfig,
): PrefixReplacement | null {
  const indentationLength = leadingWhitespaceLength(line)
  const body = line.slice(indentationLength)
  const pairs: Array<[string, string]> = [
    [from.typeMarker, to.typeMarker],
    [from.stemMarker, to.stemMarker],
    [from.answerMarker, to.answerMarker],
    [from.explanationMarker, to.explanationMarker],
  ]
  for (const [oldMarker, newMarker] of pairs) {
    if (body.startsWith(oldMarker)) {
      return {
        consumed: indentationLength + oldMarker.length,
        replacement: `${line.slice(0, indentationLength)}${newMarker}`,
      }
    }
  }
  const option = matchOptionMarker(body, from.optionStyle)
  if (option) {
    return {
      consumed: indentationLength + option.consumed,
      replacement: `${line.slice(0, indentationLength)}${optionMarker(to.optionStyle, option.letter)}`,
    }
  }
  if (from.questionSeparator && body.trim() === from.questionSeparator) {
    return {
      consumed: line.length,
      replacement: to.questionSeparator,
    }
  }
  return null
}

export function matchOptionMarker(value: string, style: DocumentEntryOptionStyle) {
  const patterns: Record<DocumentEntryOptionStyle, RegExp> = {
    letter_dot: /^([A-HＡ-Ｈ])\s*[.．]\s*/iu,
    letter_comma: /^([A-HＡ-Ｈ])\s*、\s*/iu,
    letter_parentheses: /^[（(]\s*([A-HＡ-Ｈ])\s*[）)]\s*/iu,
    letter_brackets: /^【\s*([A-HＡ-Ｈ])\s*】\s*/iu,
  }
  const match = value.match(patterns[style])
  if (!match?.[1]) return null
  return {
    letter: match[1].normalize('NFKC').toUpperCase(),
    consumed: match[0].length,
  }
}

function replaceLineText(
  value: string,
  from: DocumentEntryTemplateConfig,
  to: DocumentEntryTemplateConfig,
) {
  let count = 0
  const text = value.split(/\r\n|\r|\n/u).map((line) => {
    const replacement = primaryMarkerReplacement(line, from, to)
    if (!replacement) return line
    count += 1
    return replacement.replacement + line.slice(replacement.consumed)
  }).join('\n')
  return { text, count }
}

function convertInlineLine(
  nodes: JSONContent[],
  from: DocumentEntryTemplateConfig,
  to: DocumentEntryTemplateConfig,
) {
  const text = nodes.map((node) => node.type === 'text' ? node.text ?? '' : '').join('')
  const replacement = primaryMarkerReplacement(text, from, to)
  if (!replacement) return { nodes, count: 0 }

  let remaining = replacement.consumed
  let inserted = false
  const converted = nodes.flatMap((node) => {
    if (node.type !== 'text' || remaining <= 0) return [node]
    const current = node.text ?? ''
    const removed = Math.min(current.length, remaining)
    remaining -= removed
    const suffix = current.slice(removed)
    if (!inserted) {
      inserted = true
      return [{ ...node, text: replacement.replacement + suffix }]
    }
    return suffix ? [{ ...node, text: suffix }] : []
  })
  if (!inserted) converted.unshift({ type: 'text', text: replacement.replacement })
  return { nodes: converted, count: 1 }
}

function convertBlock(
  block: JSONContent,
  from: DocumentEntryTemplateConfig,
  to: DocumentEntryTemplateConfig,
) {
  const cloned = clonePlain(block)
  if (!cloned.content?.length) return { block: cloned, count: 0 }
  const result: JSONContent[] = []
  let line: JSONContent[] = []
  let count = 0
  const flush = () => {
    const converted = convertInlineLine(line, from, to)
    result.push(...converted.nodes)
    count += converted.count
    line = []
  }
  for (const child of cloned.content) {
    if (child.type === 'hardBreak') {
      flush()
      result.push(child)
      continue
    }
    if (child.type === 'text' && /\r|\n/u.test(child.text ?? '')) {
      flush()
      const converted = replaceLineText(child.text ?? '', from, to)
      result.push({ ...child, text: converted.text })
      count += converted.count
      continue
    }
    line.push(child)
  }
  flush()
  cloned.content = result
  return { block: cloned, count }
}

export function convertDocumentEntryMarkers(
  source: RichContent,
  from: DocumentEntryTemplateConfig,
  to: DocumentEntryTemplateConfig,
) {
  if (
    source.schemaVersion !== 2
    || source.editor !== 'tiptap'
    || source.document?.type !== 'doc'
  ) {
    const converted = replaceLineText(source.plainText, from, to)
    return {
      source: { ...clonePlain(source), plainText: converted.text },
      replacementCount: converted.count,
    }
  }

  let replacementCount = 0
  const document = clonePlain(source.document) as JSONContent
  document.content = (document.content ?? []).map((block) => {
    const converted = convertBlock(block, from, to)
    replacementCount += converted.count
    return converted.block
  })
  const convertedPlainText = replaceLineText(source.plainText, from, to)
  return {
    source: {
      ...clonePlain(source),
      document,
      plainText: convertedPlainText.text,
    },
    replacementCount,
  }
}
