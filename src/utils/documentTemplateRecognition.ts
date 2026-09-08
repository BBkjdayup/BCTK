import type { JSONContent } from '@tiptap/core'
import type {
  DocumentEntryTemplateConfig,
  QuestionDraft,
  QuestionType,
  RichContent,
} from '../types/domain'
import {
  DEFAULT_DOCUMENT_ENTRY_TEMPLATE_CONFIG,
  DEFAULT_DOCUMENT_ENTRY_TEMPLATE_ID,
  matchOptionMarker,
} from './documentEntryTemplates'
import {
  emptyRichContent,
  TIPTAP_CONTENT_EDITOR_VERSION,
} from './richContent'

export const DOCUMENT_ENTRY_TEMPLATE_ID = DEFAULT_DOCUMENT_ENTRY_TEMPLATE_ID
export const DOCUMENT_ENTRY_SOURCE_FILE_NAME = '文档式批量录入.docx'
export const DOCUMENT_ENTRY_PARSER_VERSION = 'w1-ooxml-images-5'

export interface DocumentTemplateIssue {
  severity: 'warning' | 'error'
  line: number
  message: string
}

export interface RecognizedDocumentTemplateQuestion {
  ordinal: number
  sourceLine: number
  type: QuestionType
  draft: Pick<QuestionDraft, 'type' | 'stem' | 'options' | 'answer' | 'explanation'>
  issues: DocumentTemplateIssue[]
}

export interface DocumentTemplateRecognitionResult {
  questions: RecognizedDocumentTemplateQuestion[]
  globalIssues: DocumentTemplateIssue[]
  sourceBlockCount: number
}

type Target =
  | { kind: 'stem' }
  | { kind: 'answer' }
  | { kind: 'explanation' }
  | { kind: 'option'; label: string }

interface SourceBlock {
  line: number
  node: JSONContent
  text: string
}

interface WorkingQuestion {
  sourceLine: number
  explicitType: QuestionType | null
  typeLabelLine: number | null
  stemNodes: JSONContent[]
  optionNodes: Map<string, JSONContent[]>
  answerNodes: JSONContent[]
  explanationNodes: JSONContent[]
  issues: DocumentTemplateIssue[]
  target: Target | null
}

const questionTypeAliases: Array<[RegExp, QuestionType]> = [
  [/^(?:单选题|单项选择题|单选)$/u, 'single_choice'],
  [/^(?:多选题|多项选择题|多选)$/u, 'multiple_choice'],
  [/^(?:填空题|填空)$/u, 'fill_blank'],
  [/^(?:判断题|判断|正误题|是非题)$/u, 'true_false'],
  [/^(?:简答题|问答题|简答)$/u, 'short_answer'],
  [/^(?:综合应用题|综合题|应用题|案例分析题|案例题|案例分析)$/u, 'short_answer'],
]

const typePrefix = /^\s*(?:题型|题目类型)\s*[:：]\s*/u
const stemPrefix = /^\s*(?:题目|题干)\s*[:：]\s*/u
const optionPrefix = /^\s*(?:选项\s*)?([A-HＡ-Ｈ])\s*[.．、:：）)]\s*/iu
const answerPrefix = /^\s*(?:答案|正确答案|参考答案)\s*[:：]\s*/u
const explanationPrefix = /^\s*(?:解析|答案解析|解题思路|评分要点)\s*[:：]\s*/u
const defaultSeparatorPattern = /^\s*(?:-{3,}|={3,}|—{3,})\s*$/u

interface PrefixMatch {
  length: number
}

interface QuestionNumberPrefixMatch extends PrefixMatch {
  number: number
}

function literalPrefix(text: string, marker: string): PrefixMatch | null {
  const indentationLength = text.length - text.trimStart().length
  return text.slice(indentationLength).startsWith(marker)
    ? { length: indentationLength + marker.length }
    : null
}

function fieldPrefix(
  text: string,
  marker: string,
  defaultPattern: RegExp,
  compatibleDefaultMarkers: boolean,
) {
  return literalPrefix(text, marker)
    ?? (compatibleDefaultMarkers
      ? (() => {
          const match = text.match(defaultPattern)
          return match ? { length: match[0].length } : null
        })()
      : null)
}

function optionPrefixMatch(text: string, config: DocumentEntryTemplateConfig) {
  const indentationLength = text.length - text.trimStart().length
  const custom = matchOptionMarker(text.slice(indentationLength), config.optionStyle)
  if (custom) {
    return {
      label: custom.letter,
      length: indentationLength + custom.consumed,
    }
  }
  if (!config.compatibleDefaultMarkers) return null
  const match = text.match(optionPrefix)
  if (!match?.[1]) return null
  return {
    label: normalizedOptionLabel(match[1]),
    length: match[0].length,
  }
}

function isQuestionSeparator(text: string, config: DocumentEntryTemplateConfig) {
  const trimmed = text.trim()
  if (config.questionSeparator && trimmed === config.questionSeparator) return true
  return config.compatibleDefaultMarkers && defaultSeparatorPattern.test(text)
}

function cloneNode<T>(value: T): T {
  return JSON.parse(JSON.stringify(value)) as T
}

function escapeHtml(value: string) {
  return value
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;')
}

function renderAttributes(attributes: Record<string, unknown> | undefined, allowed: readonly string[]) {
  if (!attributes) return ''
  return allowed.flatMap((name) => {
    const raw = attributes[name]
    if (raw === null || raw === undefined || raw === false || raw === '') return []
    if (raw === true) return [` ${name}`]
    return [` ${name}="${escapeHtml(String(raw))}"`]
  }).join('')
}

function renderMarks(text: string, marks: JSONContent['marks']) {
  return (marks ?? []).reduce((html, mark) => {
    if (mark.type === 'bold') return `<strong>${html}</strong>`
    if (mark.type === 'italic') return `<em>${html}</em>`
    if (mark.type === 'underline') return `<u>${html}</u>`
    if (mark.type === 'strike') return `<s>${html}</s>`
    if (mark.type === 'code') return `<code>${html}</code>`
    if (mark.type === 'superscript') return `<sup>${html}</sup>`
    if (mark.type === 'subscript') return `<sub>${html}</sub>`
    if (mark.type === 'link') {
      const href = escapeHtml(String(mark.attrs?.href ?? ''))
      return href ? `<a href="${href}">${html}</a>` : html
    }
    return html
  }, escapeHtml(text))
}

function renderNode(node: JSONContent): string {
  if (node.type === 'text') return renderMarks(node.text ?? '', node.marks)
  if (node.type === 'hardBreak') return '<br>'
  if (node.type === 'horizontalRule') return '<hr>'
  if (node.type === 'image') {
    return `<img${renderAttributes(node.attrs, ['src', 'alt', 'title', 'data-resource-id', 'data-node-id', 'width', 'height'])}>`
  }
  if (node.type === 'mathNode') {
    const latex = escapeHtml(String(node.attrs?.latex ?? ''))
    return `<span class="math-node" data-latex="${latex}">${latex}</span>`
  }

  const children = (node.content ?? []).map(renderNode).join('')
  if (node.type === 'paragraph') return `<p>${children}</p>`
  if (node.type === 'heading') {
    const level = Math.max(1, Math.min(6, Number(node.attrs?.level) || 1))
    return `<h${level}>${children}</h${level}>`
  }
  if (node.type === 'blockquote') return `<blockquote>${children}</blockquote>`
  if (node.type === 'bulletList') return `<ul>${children}</ul>`
  if (node.type === 'orderedList') {
    const start = Math.max(1, Number(node.attrs?.start) || 1)
    return `<ol${start === 1 ? '' : ` start="${start}"`}>${children}</ol>`
  }
  if (node.type === 'listItem') return `<li>${children}</li>`
  if (node.type === 'codeBlock') return `<pre><code>${children}</code></pre>`
  if (node.type === 'table') return `<table><tbody>${children}</tbody></table>`
  if (node.type === 'tableRow') return `<tr>${children}</tr>`
  if (node.type === 'tableHeader') return `<th>${children}</th>`
  if (node.type === 'tableCell') return `<td>${children}</td>`
  return children
}

function nodePlainText(node: JSONContent): string {
  if (node.type === 'text') return node.text ?? ''
  if (node.type === 'hardBreak') return '\n'
  if (node.type === 'horizontalRule') return '---'
  if (node.type === 'mathNode') return String(node.attrs?.latex ?? '')
  if (node.type === 'image') return String(node.attrs?.alt ?? '')
  const separator = ['doc', 'bulletList', 'orderedList', 'listItem', 'table', 'tableRow'].includes(node.type ?? '')
    ? '\n'
    : ''
  return (node.content ?? []).map(nodePlainText).filter(Boolean).join(separator)
}

function stripLeadingText(node: JSONContent, characterCount: number): JSONContent {
  const cloned = cloneNode(node)
  let remaining = characterCount
  const visit = (entry: JSONContent) => {
    if (remaining <= 0) return
    if (entry.type === 'text') {
      const text = entry.text ?? ''
      const removed = Math.min(remaining, text.length)
      entry.text = text.slice(removed)
      remaining -= removed
      return
    }
    for (const child of entry.content ?? []) visit(child)
    if (entry.content) {
      entry.content = entry.content.filter((child) => child.type !== 'text' || Boolean(child.text))
    }
  }
  visit(cloned)
  return cloned
}

function hasNodeContent(node: JSONContent) {
  if (nodePlainText(node).trim()) return true
  if (node.type === 'image' || node.type === 'mathNode' || node.type === 'table') return true
  return (node.content ?? []).some(hasNodeContent)
}

function splitNodeAtInlineBreaks(node: JSONContent): JSONContent[] {
  const content = node.content ?? []
  const hasInlineBreak = content.some((child) => (
    child.type === 'hardBreak'
    || (child.type === 'text' && /\r|\n/u.test(child.text ?? ''))
  ))
  if (!hasInlineBreak) return [cloneNode(node)]

  const lines: JSONContent[][] = [[]]
  const startNextLine = () => lines.push([])
  for (const child of content) {
    if (child.type === 'hardBreak') {
      startNextLine()
      continue
    }
    if (child.type === 'text' && /\r|\n/u.test(child.text ?? '')) {
      const parts = (child.text ?? '').split(/\r\n|\r|\n/u)
      parts.forEach((part, index) => {
        if (part) lines[lines.length - 1]?.push({ ...cloneNode(child), text: part })
        if (index < parts.length - 1) startNextLine()
      })
      continue
    }
    lines[lines.length - 1]?.push(cloneNode(child))
  }

  return lines.map((lineContent) => {
    const line = cloneNode(node)
    if (lineContent.length) line.content = lineContent
    else delete line.content
    return line
  })
}

function sourceBlocks(source: RichContent): SourceBlock[] {
  const document = source.schemaVersion === 2
    && source.editor === 'tiptap'
    && source.document?.type === 'doc'
    ? source.document as JSONContent
    : null
  const nodes = document?.content?.length
    ? document.content.map((node) => cloneNode(node))
    : source.plainText.split(/\r?\n/u).map((text) => ({
      type: 'paragraph',
      content: text ? [{ type: 'text', text }] : undefined,
    } satisfies JSONContent))
  const logicalLines = nodes.flatMap(splitNodeAtInlineBreaks)
  return logicalLines.map((node, index) => ({
    line: index + 1,
    node,
    text: nodePlainText(node),
  }))
}

function richContentFromNodes(nodes: readonly JSONContent[]): RichContent {
  const meaningful = nodes.map((node) => cloneNode(node)).filter(hasNodeContent)
  if (!meaningful.length) return emptyRichContent()
  return {
    schemaVersion: 2,
    editor: 'tiptap',
    editorVersion: TIPTAP_CONTENT_EDITOR_VERSION,
    document: {
      type: 'doc',
      content: meaningful,
    },
    html: meaningful.map(renderNode).join(''),
    plainText: meaningful.map(nodePlainText).join('\n').trim(),
  }
}

function newWorkingQuestion(line: number): WorkingQuestion {
  return {
    sourceLine: line,
    explicitType: null,
    typeLabelLine: null,
    stemNodes: [],
    optionNodes: new Map(),
    answerNodes: [],
    explanationNodes: [],
    issues: [],
    target: null,
  }
}

function hasQuestionContent(question: WorkingQuestion | null) {
  return Boolean(question && (
    question.explicitType
    || question.stemNodes.length
    || question.optionNodes.size
    || question.answerNodes.length
    || question.explanationNodes.length
  ))
}

function hasQuestionBodyContent(question: WorkingQuestion | null) {
  return Boolean(question && (
    question.stemNodes.length
    || question.optionNodes.size
    || question.answerNodes.length
    || question.explanationNodes.length
  ))
}

function normalizedOptionLabel(value: string) {
  return value.normalize('NFKC').toUpperCase()
}

function parseQuestionType(value: string): QuestionType | null {
  const normalized = value.normalize('NFKC').replace(/\s+/g, '').trim()
  return questionTypeAliases.find(([pattern]) => pattern.test(normalized))?.[1] ?? null
}

function meaningfulOptionEntries(question: WorkingQuestion) {
  return [...question.optionNodes.entries()]
    .filter(([, nodes]) => nodes.some(hasNodeContent))
}

function questionNumberPrefixMatch(
  text: string,
  config: DocumentEntryTemplateConfig,
): QuestionNumberPrefixMatch | null {
  if (config.recognizeQuestionNumbers === false) return null
  const match = text.match(/^(\s*([0-9０-９]{1,4})\s*([.．、])([ \t]*))(\S.*)$/u)
  if (!match?.[1] || !match[2] || !match[3] || match[4] === undefined || !match[5]) return null
  const separator = match[3]
  const spacing = match[4]
  const remainder = match[5]
  if ((separator === '.' || separator === '．') && !spacing && /^[0-9０-９]/u.test(remainder)) {
    return null
  }
  const number = Number(match[2].normalize('NFKC'))
  if (!Number.isSafeInteger(number) || number <= 0) return null
  return { number, length: match[1].length }
}

function declaredOptionEntries(question: WorkingQuestion) {
  return [...question.optionNodes.entries()]
}

function strictChoiceAnswerLabels(question: WorkingQuestion, optionLabels: readonly string[]) {
  const raw = question.answerNodes
    .map(nodePlainText)
    .join('')
    .normalize('NFKC')
    .toUpperCase()
    .trim()
  if (!raw) return null
  const compact = raw.replace(/[\s,，、;；/|]+/gu, '')
  if (!compact || !/^[A-H]+$/u.test(compact)) return null
  const allowed = new Set(optionLabels)
  const labels = [...new Set([...compact])]
  return labels.every((label) => allowed.has(label)) ? labels : null
}

function resemblesChoiceAnswer(question: WorkingQuestion) {
  const raw = question.answerNodes
    .map(nodePlainText)
    .join('')
    .normalize('NFKC')
    .toUpperCase()
    .trim()
  return /^[A-H](?:[\s,，、;；/|]*[A-H])*$/u.test(raw)
}

function trueFalseAnswerLabel(question: WorkingQuestion): 'A' | 'B' | null {
  const answer = question.answerNodes
    .map(nodePlainText)
    .join('')
    .normalize('NFKC')
    .replace(/\s+/gu, '')
    .toUpperCase()
  if (/^(?:A|T|TRUE|正确|对|是|√)$/u.test(answer)) return 'A'
  if (/^(?:B|F|FALSE|错误|错|否|×)$/u.test(answer)) return 'B'
  return null
}

function plainTextNodes(value: string): JSONContent[] {
  return [{ type: 'paragraph', content: [{ type: 'text', text: value }] }]
}

interface InferredQuestionType {
  type: QuestionType
  choiceAnswerLabels: string[] | null
}

function inferredQuestionType(
  question: WorkingQuestion,
  optionEntries = meaningfulOptionEntries(question),
): InferredQuestionType {
  if (optionEntries.length >= 2) {
    const letters = strictChoiceAnswerLabels(question, optionEntries.map(([label]) => label))
    return {
      type: letters && letters.length > 1 ? 'multiple_choice' : 'single_choice',
      choiceAnswerLabels: letters,
    }
  }
  const stem = question.stemNodes.map(nodePlainText).join('\n')
  const answer = question.answerNodes.map(nodePlainText).join('').normalize('NFKC').trim()
  return {
    type: /^(?:正确|错误|对|错|√|×|T|F)$/iu.test(answer)
      ? 'true_false'
      : /_{2,}|（\s*）|\(\s*\)|填空/u.test(stem) ? 'fill_blank' : 'short_answer',
    choiceAnswerLabels: null,
  }
}

function targetNodes(question: WorkingQuestion, target: Target): JSONContent[] {
  if (target.kind === 'stem') return question.stemNodes
  if (target.kind === 'answer') return question.answerNodes
  if (target.kind === 'explanation') return question.explanationNodes
  const existing = question.optionNodes.get(target.label)
  if (existing) return existing
  const created: JSONContent[] = []
  question.optionNodes.set(target.label, created)
  return created
}

function issue(
  question: WorkingQuestion,
  severity: DocumentTemplateIssue['severity'],
  line: number,
  message: string,
) {
  question.issues.push({ severity, line, message })
}

export function recognizeLabeledQuestionDocument(
  source: RichContent,
  config: DocumentEntryTemplateConfig = DEFAULT_DOCUMENT_ENTRY_TEMPLATE_CONFIG,
): DocumentTemplateRecognitionResult {
  const blocks = sourceBlocks(source)
  const questions: RecognizedDocumentTemplateQuestion[] = []
  const globalIssues: DocumentTemplateIssue[] = []
  let current: WorkingQuestion | null = null

  const ensureCurrent = (line: number) => {
    current ??= newWorkingQuestion(line)
    return current
  }

  const finishCurrent = () => {
    if (!hasQuestionContent(current)) {
      current = null
      return
    }
    const question = current as WorkingQuestion
    const allOptionEntries = declaredOptionEntries(question)
    const optionEntries = meaningfulOptionEntries(question)
    const inferred = inferredQuestionType(question, optionEntries)
    const type = question.explicitType ?? inferred.type
    if (question.explicitType === 'single_choice' || question.explicitType === 'multiple_choice') {
      if (allOptionEntries.length < 2) {
        issue(
          question,
          'error',
          question.typeLabelLine ?? question.sourceLine,
          '题型已经明确为选择题，但识别到的选项不足两个；请在审查页补全选项。',
        )
      }
      for (const [label, nodes] of allOptionEntries) {
        if (!nodes.some(hasNodeContent)) {
          issue(
            question,
            'error',
            question.sourceLine,
            `选项 ${label} 没有内容，请在审查页补充或删除该选项。`,
          )
        }
      }
    }
    if (!question.explicitType) {
      issue(
        question,
        'warning',
        question.typeLabelLine ?? question.sourceLine,
        `未提供有效题型，软件自动识别为“${type === 'single_choice' ? '单选题' : type === 'multiple_choice' ? '多选题' : type === 'fill_blank' ? '填空题' : type === 'true_false' ? '判断题' : '简答题'}”。`,
      )
      if (optionEntries.length >= 2 && !inferred.choiceAnswerLabels) {
        issue(
          question,
          'warning',
          question.sourceLine,
          '识别到了选项，但答案不是纯选项编号格式，暂时按单选题处理，请在审查页确认。',
        )
      } else if (type === 'short_answer' && optionEntries.length < 2 && resemblesChoiceAnswer(question)) {
        issue(
          question,
          'warning',
          question.sourceLine,
          '答案看起来像选择题编号，但没有识别到至少两个非空选项，暂时按简答题处理。',
        )
      }
    }
    if (!question.stemNodes.some(hasNodeContent)) {
      issue(question, 'error', question.sourceLine, `没有识别到“${config.stemMarker}”后的题干内容。`)
    }
    if (!question.answerNodes.some(hasNodeContent)) {
      issue(question, 'error', question.sourceLine, `没有识别到“${config.answerMarker}”后的答案内容。`)
    }
    const choice = type === 'single_choice' || type === 'multiple_choice' || type === 'true_false'
    if (!choice && optionEntries.length) {
      issue(question, 'warning', question.sourceLine, '当前题型不是选择题，但识别到了选项，请在审查页确认题型。')
    }

    const resolvedOptionEntries = type === 'true_false'
      ? ([
          ['A', plainTextNodes('正确')],
          ['B', plainTextNodes('错误')],
        ] as Array<[string, JSONContent[]]>)
      : choice
        ? (question.explicitType ? allOptionEntries : optionEntries)
        : []
    const options = resolvedOptionEntries
      .sort(([left], [right]) => left.localeCompare(right))
      .map(([, nodes], position) => ({
        id: crypto.randomUUID(),
        position,
        content: richContentFromNodes(nodes),
      }))
    let answer = richContentFromNodes(question.answerNodes)
    if (type === 'true_false' && question.answerNodes.some(hasNodeContent)) {
      const normalizedAnswer = trueFalseAnswerLabel(question)
      if (normalizedAnswer) {
        answer = richContentFromNodes(plainTextNodes(normalizedAnswer))
      } else {
        issue(
          question,
          'error',
          question.sourceLine,
          '判断题答案无法识别，请使用“正确/错误”“对/错”“T/F”或“A/B”。',
        )
      }
    }
    questions.push({
      ordinal: questions.length + 1,
      sourceLine: question.sourceLine,
      type,
      draft: {
        type,
        stem: richContentFromNodes(question.stemNodes),
        options,
        answer,
        explanation: richContentFromNodes(question.explanationNodes),
      },
      issues: question.issues,
    })
    current = null
  }

  for (const block of blocks) {
    const text = block.text
    if (!text.trim() && !hasNodeContent(block.node)) continue
    if (isQuestionSeparator(text, config)) {
      finishCurrent()
      continue
    }

    const typeMatch = fieldPrefix(
      text,
      config.typeMarker,
      typePrefix,
      config.compatibleDefaultMarkers,
    )
    if (typeMatch) {
      if (hasQuestionContent(current)) finishCurrent()
      const question = ensureCurrent(block.line)
      question.typeLabelLine = block.line
      const typeText = text.slice(typeMatch.length).trim()
      question.explicitType = parseQuestionType(typeText)
      if (typeText && !question.explicitType) {
        issue(question, 'warning', block.line, `无法识别题型“${typeText}”，将根据选项和答案自动判断。`)
      }
      question.target = null
      continue
    }

    const stemMatch = fieldPrefix(
      text,
      config.stemMarker,
      stemPrefix,
      config.compatibleDefaultMarkers,
    )
    if (stemMatch) {
      const previous = current as WorkingQuestion | null
      if (previous?.stemNodes.some(hasNodeContent)
        && (previous.answerNodes.length || previous.explanationNodes.length || previous.optionNodes.size)) {
        finishCurrent()
      }
      const question = ensureCurrent(block.line)
      question.target = { kind: 'stem' }
      const numberedStem = questionNumberPrefixMatch(text.slice(stemMatch.length), config)
      const removeNumber = numberedStem && config.stripRecognizedQuestionNumbers !== false
      const valueNode = stripLeadingText(
        block.node,
        stemMatch.length + (removeNumber ? numberedStem.length : 0),
      )
      if (hasNodeContent(valueNode)) question.stemNodes.push(valueNode)
      continue
    }

    const questionNumber = questionNumberPrefixMatch(text, config)
    if (questionNumber) {
      if (hasQuestionBodyContent(current)) finishCurrent()
      const question = ensureCurrent(block.line)
      question.sourceLine = block.line
      question.target = { kind: 'stem' }
      const valueNode = config.stripRecognizedQuestionNumbers !== false
        ? stripLeadingText(block.node, questionNumber.length)
        : cloneNode(block.node)
      if (hasNodeContent(valueNode)) question.stemNodes.push(valueNode)
      continue
    }

    const optionMatch = optionPrefixMatch(text, config)
    if (optionMatch) {
      const question = ensureCurrent(block.line)
      const label = optionMatch.label
      question.target = { kind: 'option', label }
      const valueNode = stripLeadingText(block.node, optionMatch.length)
      targetNodes(question, question.target).push(valueNode)
      continue
    }

    const answerMatch = fieldPrefix(
      text,
      config.answerMarker,
      answerPrefix,
      config.compatibleDefaultMarkers,
    )
    if (answerMatch) {
      const question = ensureCurrent(block.line)
      question.target = { kind: 'answer' }
      const valueNode = stripLeadingText(block.node, answerMatch.length)
      if (hasNodeContent(valueNode)) question.answerNodes.push(valueNode)
      continue
    }

    const explanationMatch = fieldPrefix(
      text,
      config.explanationMarker,
      explanationPrefix,
      config.compatibleDefaultMarkers,
    )
    if (explanationMatch) {
      const question = ensureCurrent(block.line)
      question.target = { kind: 'explanation' }
      const valueNode = stripLeadingText(block.node, explanationMatch.length)
      if (hasNodeContent(valueNode)) question.explanationNodes.push(valueNode)
      continue
    }

    const continuation = current as WorkingQuestion | null
    if (!continuation?.target) {
      globalIssues.push({
        severity: 'warning',
        line: block.line,
        message: `第 ${block.line} 行不在当前模板的题目、选项、答案或解析字段中，未参与识别。`,
      })
      continue
    }
    targetNodes(continuation, continuation.target).push(cloneNode(block.node))
  }
  finishCurrent()

  if (!questions.length && source.plainText.trim()) {
    globalIssues.push({
      severity: 'error',
      line: 1,
      message: `没有找到“${config.typeMarker}”“${config.stemMarker}”“${config.answerMarker}”等模板标签，请按照编辑区内的灰色模板提示逐行输入。`,
    })
  }
  return {
    questions,
    globalIssues,
    sourceBlockCount: blocks.length,
  }
}
