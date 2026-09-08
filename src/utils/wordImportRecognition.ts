import type {
  DocxImageOccurrence,
  QuestionType,
  QuestionTypeBehavior,
  QuestionTypeDefinition,
} from '../types/domain'
import { matchQuestionTypeName, questionTypeBehavior } from './questionTypes'

export interface WordAnalysisLine {
  paragraphIndex: number
  text: string
  images: DocxImageOccurrence[]
  tables?: WordAnalysisTable[]
}

export interface WordAnalysisTableCell {
  lines: WordAnalysisLine[]
}

export interface WordAnalysisTable {
  rows: WordAnalysisTableCell[][]
}

export type WordQuestionSection =
  | 'choice'
  | 'fill_blank'
  | 'true_false'
  | 'short_answer'
  | 'case_analysis'
  | 'application'

export interface RecognizedWordQuestion {
  section: WordQuestionSection | null
  sourceNumber: number | null
  type: QuestionType
  stemLines: WordAnalysisLine[]
  options: WordAnalysisLine[][]
  answerLines: WordAnalysisLine[]
  explanationLines: WordAnalysisLine[]
  unknownTypeName?: string | null
  suggestedBehavior?: QuestionTypeBehavior | null
}

interface SectionHeading {
  section: WordQuestionSection
  expectedCount: number | null
  explicitType: QuestionType | null
  unknownTypeName: string | null
}

interface NumberedLine {
  number: number
  remainder: string
}

interface RawQuestion {
  section: WordQuestionSection | null
  sourceNumber: number | null
  lines: WordAnalysisLine[]
  explicitType: QuestionType | null
  unknownTypeName: string | null
}

interface OptionMarker {
  label: string
  markerStart: number
  contentStart: number
}

interface PendingAnswer {
  section: WordQuestionSection | null
  sourceNumber: number
  answerLines: WordAnalysisLine[]
  acceptingAnswer: boolean
}

const answerHeadingPattern = /^\s*(?:参考答案(?:与|及)评分要点|参考答案及解析|试题参考答案|参考答案与解析|参考答案)\s*[：:]?\s*$/
const explanationHeadingPattern = /^\s*(?:题目解析|试题解析|答案解析|解析)\s*[：:]?\s*$/
const answerContentPattern = /^\s*(?:参考要点|参考答案|答案|评分要点|答)\s*[：:]\s*(.*)$/
const explanationContentPattern = /^\s*(?:题目解析|答案解析|解析)\s*[：:]\s*(.*)$/
const optionMarkerPattern = /([A-HＡ-Ｈ])\s*[.．、:：)）]\s*/gi
const instructionPattern = /^\s*(?:说明|注意(?:事项)?|答题要求|本大题|每题|考点覆盖|正式组卷时)\s*[：:]/
const emptyExportFieldPattern = /^\(\s*未填写\s*\)[。.．]?$/u

function cloneLine(line: WordAnalysisLine, text = line.text, images = line.images): WordAnalysisLine {
  return { ...line, text, images: [...images] }
}

function hasLineContent(line: WordAnalysisLine) {
  return Boolean(line.text.trim() || line.images.length || line.tables?.length)
}

function normalizedLetter(value: string) {
  return value.normalize('NFKC').toUpperCase()
}

function chineseNumber(value: string): number | null {
  const normalized = value.normalize('NFKC').trim()
  if (/^\d+$/.test(normalized)) {
    const parsed = Number(normalized)
    return Number.isSafeInteger(parsed) && parsed > 0 ? parsed : null
  }
  if (!/^[一二三四五六七八九十百]+$/.test(normalized)) return null

  const digits: Record<string, number> = {
    一: 1, 二: 2, 三: 3, 四: 4, 五: 5, 六: 6, 七: 7, 八: 8, 九: 9,
  }
  let total = 0
  let current = 0
  for (const character of normalized) {
    if (character === '十' || character === '百') {
      const unit = character === '百' ? 100 : 10
      total += (current || 1) * unit
      current = 0
    } else {
      current = digits[character] ?? 0
    }
  }
  return total + current || null
}

function parseExpectedCount(text: string) {
  const normalized = text.normalize('NFKC')
  const match = normalized.match(/(?:共\s*|备选\s*)(\d+)\s*(?:个\s*)?(?:小题|题|道)/)
    ?? normalized.match(/(\d+)\s*道(?:选择|简答|案例|综合)/)
  return match?.[1] ? Number(match[1]) : null
}

function extractedSectionTypeName(text: string) {
  const withoutOrdinal = text.normalize('NFKC').trim()
    .replace(/^(?:[一二三四五六七八九十]+\s*[、.．]|第\s*[一二三四五六七八九十]+\s*(?:部分|大题)\s*)/u, '')
    .trim()
  const match = withoutOrdinal.match(/^([^：:（(]{1,24}?题)(?=\s|[：:（(]|$)/u)
  const headingName = match?.[1]?.trim()
  if (!headingName) return null

  // Question-bank Word exports use "subject / chapter / question type" as
  // their section heading. Only the final segment is the type name. Treating
  // the whole heading as a custom type can turn choice questions into open
  // responses and leave A/B/C/D inside the stem, which also defeats duplicate
  // detection after a round trip.
  const segments = headingName.split(/\s*[/／]\s*/u).map((segment) => segment.trim()).filter(Boolean)
  return segments.at(-1) ?? headingName
}

function withoutEmptyExportPlaceholder(lines: readonly WordAnalysisLine[]) {
  const cloned = lines.map((line) => cloneLine(line))
  if (cloned.some((line) => line.images.length || line.tables?.length)) return cloned
  const text = cloned.map((line) => line.text).join('\n').trim().normalize('NFKC')
  return emptyExportFieldPattern.test(text) ? [] : cloned
}

function parseSectionHeading(
  text: string,
  definitions?: readonly QuestionTypeDefinition[],
): SectionHeading | null {
  const normalized = text.normalize('NFKC').trim()
  if (!/^(?:[一二三四五六七八九十]+\s*[、.．]|第\s*[一二三四五六七八九十]+\s*(?:部分|大题))/.test(normalized)) {
    return null
  }

  let section: WordQuestionSection | null = null
  let explicitType: QuestionType | null = null
  let unknownTypeName: string | null = null
  if (/(?:选择题|单选题|多选题)/.test(normalized)) section = 'choice'
  else if (/填空题/.test(normalized)) section = 'fill_blank'
  else if (/(?:判断题|正误题|是非题)/.test(normalized)) section = 'true_false'
  else if (/(?:简答题|解答题)/.test(normalized)) section = 'short_answer'
  else if (/(?:案例分析题|案例题)/.test(normalized)) section = 'case_analysis'
  else if (/(?:综合应用题|综合题|应用题)/.test(normalized)) section = 'application'
  const typeName = extractedSectionTypeName(normalized)
  const matched = typeName && typeName !== '选择题'
    ? matchQuestionTypeName(typeName, definitions)
    : undefined
  if (matched) {
    // Keep the mature built-in heuristics (for example an application section
    // whose individual stem is clearly a case-analysis question). Custom
    // types, however, must retain the exact user-created type code.
    if (!matched.isBuiltin) {
      explicitType = matched.code
      section = matched.behavior === 'single_choice' || matched.behavior === 'multiple_choice'
        ? 'choice'
        : matched.behavior === 'fill_blank' ? 'fill_blank' : 'short_answer'
    }
  } else if (!section && typeName) {
    unknownTypeName = typeName
    section = 'short_answer'
  }
  if (!section) return null
  return {
    section,
    expectedCount: parseExpectedCount(normalized),
    explicitType,
    unknownTypeName,
  }
}

function parseTopLevelNumber(text: string): NumberedLine | null {
  const direct = text.match(/^\s*(\d{1,4})\s*([.．、])(?!\d)\s*/)
  if (direct?.[1]) {
    return { number: Number(direct[1]), remainder: text.slice(direct[0].length).trim() }
  }

  const named = text.match(/^\s*第\s*([\d一二三四五六七八九十百]+)\s*题\s*(?:[.．、:：])?\s*/)
  if (!named?.[1]) return null
  const number = chineseNumber(named[1])
  return number ? { number, remainder: text.slice(named[0].length).trim() } : null
}

function containsContent(lines: readonly WordAnalysisLine[]) {
  return lines.some(hasLineContent)
}

function isInstruction(line: WordAnalysisLine) {
  return !line.images.length && !line.tables?.length && instructionPattern.test(line.text)
}

function usefulPrelude(lines: readonly WordAnalysisLine[]) {
  return lines.filter((line) => !isInstruction(line) && hasLineContent(line))
}

function parseBody(
  lines: readonly WordAnalysisLine[],
  definitions?: readonly QuestionTypeDefinition[],
): RawQuestion[] {
  const questions: RawQuestion[] = []
  let section: WordQuestionSection | null = null
  let explicitType: QuestionType | null = null
  let unknownTypeName: string | null = null
  let expectedCount: number | null = null
  let current: RawQuestion | null = null
  let prelude: WordAnalysisLine[] = []
  let sawNumberInSection = false

  const finishCurrent = () => {
    if (current && containsContent(current.lines)) questions.push(current)
    current = null
  }

  const inferPreludeQuestion = () => {
    const useful = usefulPrelude(prelude)
    if (!useful.length) return false
    questions.push({
      section,
      sourceNumber: 1,
      lines: useful.map((line) => cloneLine(line)),
      explicitType,
      unknownTypeName,
    })
    prelude = []
    return true
  }

  for (const sourceLine of lines) {
    const line = cloneLine(sourceLine, sourceLine.text.trim())
    if (!hasLineContent(line)) continue

    const heading = parseSectionHeading(line.text, definitions)
    if (heading) {
      finishCurrent()
      if (!sawNumberInSection && expectedCount === 1) inferPreludeQuestion()
      section = heading.section
      explicitType = heading.explicitType
      unknownTypeName = heading.unknownTypeName
      expectedCount = heading.expectedCount
      current = null
      prelude = []
      sawNumberInSection = false
      continue
    }

    const numbered = parseTopLevelNumber(line.text)
    if (numbered) {
      finishCurrent()
      if (section && !sawNumberInSection && numbered.number === 2 && (expectedCount ?? 0) >= 2) {
        inferPreludeQuestion()
      } else {
        prelude = []
      }
      sawNumberInSection = true
      current = {
        section,
        sourceNumber: numbered.number,
        explicitType,
        unknownTypeName,
        lines: numbered.remainder || line.images.length
          ? [cloneLine(line, numbered.remainder)]
          : [],
      }
      continue
    }

    if (current) current.lines.push(line)
    else prelude.push(line)
  }

  finishCurrent()
  if (!sawNumberInSection && (expectedCount === 1 || !questions.length)) inferPreludeQuestion()
  return questions
}

function optionMarkers(text: string): OptionMarker[] {
  const matches: OptionMarker[] = []
  optionMarkerPattern.lastIndex = 0
  let match = optionMarkerPattern.exec(text)
  while (match) {
    const markerStart = match.index
    const previous = markerStart > 0 ? text[markerStart - 1] ?? '' : ''
    const atBoundary = markerStart === 0 || !/[A-Za-z0-9]/.test(previous)
    if (atBoundary) {
      matches.push({
        label: normalizedLetter(match[1] ?? ''),
        markerStart,
        contentStart: optionMarkerPattern.lastIndex,
      })
    }
    match = optionMarkerPattern.exec(text)
  }
  return matches
}

function splitOptionLine(line: WordAnalysisLine) {
  const markers = optionMarkers(line.text)
  if (!markers.length) return null
  const prefix = line.text.slice(0, markers[0]?.markerStart ?? 0).trim()
  const imagesByOption = markers.map(() => [] as DocxImageOccurrence[])
  const prefixImages: DocxImageOccurrence[] = []
  for (const image of line.images) {
    const optionIndex = markers.findIndex((marker, index) => {
      const nextMarker = markers[index + 1]
      return image.textCharOffset > marker.markerStart
        && (!nextMarker || image.textCharOffset <= nextMarker.markerStart)
    })
    if (optionIndex >= 0) imagesByOption[optionIndex]?.push(image)
    else prefixImages.push(image)
  }
  const options = markers.map((marker, index) => ({
    label: marker.label,
    line: cloneLine(
      line,
      line.text.slice(marker.contentStart, markers[index + 1]?.markerStart ?? line.text.length).trim(),
      imagesByOption[index] ?? [],
    ),
  }))
  return { prefix, prefixImages, options }
}

function inferType(
  raw: RawQuestion,
  stemLines: readonly WordAnalysisLine[],
  options: readonly WordAnalysisLine[][],
  answerLines: readonly WordAnalysisLine[],
): QuestionType {
  if (raw.explicitType) return raw.explicitType
  const stemText = stemLines.map((line) => line.text).join('\n')
  if (raw.section === 'choice') {
    const answerLetters = new Set(answerLines.flatMap((line) => (
      line.text.normalize('NFKC').toUpperCase().match(/[A-H]/g) ?? []
    )))
    return answerLetters.size > 1 ? 'multiple_choice' : 'single_choice'
  }
  if (raw.section === 'fill_blank') return 'fill_blank'
  if (raw.section === 'true_false') return 'true_false'
  if (raw.section === 'short_answer') return 'short_answer'
  if (raw.section === 'case_analysis' || raw.section === 'application') return 'short_answer'
  if (options.length >= 2) return 'single_choice'
  const answerText = answerLines.map((line) => line.text).join('').normalize('NFKC').trim()
  if (/^(?:正确|错误|对|错|√|×|T|F)$/iu.test(answerText)) return 'true_false'
  if (/_{2,}|填空/.test(stemText)) return 'fill_blank'
  return 'short_answer'
}

function answerKey(section: WordQuestionSection | null, sourceNumber: number) {
  return `${section ?? 'unsectioned'}:${sourceNumber}`
}

function choiceAnswerPairs(line: WordAnalysisLine) {
  const normalized = line.text.normalize('NFKC').toUpperCase()
  const pattern = /(?:^|\s)(\d{1,4})\s*[.、]\s*([A-H](?:\s*[,，、/]?\s*[A-H])*)/g
  return [...normalized.matchAll(pattern)].map((match) => ({
    sourceNumber: Number(match[1]),
    answer: (match[2] ?? '').replace(/[\s,，、/]/g, ''),
  })).filter((item) => item.sourceNumber > 0 && item.answer)
}

function matchingQuestionSection(
  questions: readonly RawQuestion[],
  sourceNumber: number,
  repeatedStem: string,
): WordQuestionSection | null {
  const candidates = questions.filter((question) => question.sourceNumber === sourceNumber)
  if (candidates.length === 1) return candidates[0]?.section ?? null
  const normalizedStem = repeatedStem.replace(/\s/g, '')
  const matching = candidates.find((question) => {
    const questionStem = question.lines.map((line) => line.text).join('').replace(/\s/g, '')
    return normalizedStem.length >= 4 && (
      questionStem.startsWith(normalizedStem) || normalizedStem.startsWith(questionStem.slice(0, normalizedStem.length))
    )
  })
  return matching?.section ?? null
}

function isRepeatedQuestionStem(
  questions: readonly RawQuestion[],
  section: WordQuestionSection | null,
  sourceNumber: number,
  candidate: string,
) {
  const normalizedCandidate = candidate.normalize('NFKC').replace(/\s/g, '')
  if (normalizedCandidate.length < 4) return false
  return questions
    .filter((question) => question.sourceNumber === sourceNumber && (section === null || question.section === section))
    .some((question) => {
      const normalizedStem = question.lines.map((line) => line.text).join('').normalize('NFKC').replace(/\s/g, '')
      return normalizedStem.startsWith(normalizedCandidate)
        || normalizedCandidate.startsWith(normalizedStem.slice(0, normalizedCandidate.length))
    })
}

function parseAnswers(lines: readonly WordAnalysisLine[], questions: readonly RawQuestion[]) {
  const answers = new Map<string, WordAnalysisLine[]>()
  let section: WordQuestionSection | null = null
  let pending: PendingAnswer | null = null

  const finishPending = () => {
    if (pending?.answerLines.length) {
      answers.set(answerKey(pending.section, pending.sourceNumber), pending.answerLines)
    }
    pending = null
  }

  for (const sourceLine of lines) {
    const line = cloneLine(sourceLine, sourceLine.text.trim())
    if (!hasLineContent(line)) continue
    const heading = parseSectionHeading(line.text)
    if (heading) {
      finishPending()
      section = heading.section
      continue
    }

    const pairs = choiceAnswerPairs(line)
    if ((section === 'choice' || section === null) && pairs.length) {
      finishPending()
      for (const pair of pairs) {
        const resolvedSection = section
          ?? matchingQuestionSection(questions, pair.sourceNumber, '')
          ?? 'choice'
        answers.set(answerKey(resolvedSection, pair.sourceNumber), [cloneLine(line, pair.answer, [])])
      }
      continue
    }

    const numbered = parseTopLevelNumber(line.text)
    if (numbered) {
      finishPending()
      const resolvedSection = section ?? matchingQuestionSection(questions, numbered.number, numbered.remainder)
      pending = {
        section: resolvedSection,
        sourceNumber: numbered.number,
        answerLines: [],
        acceptingAnswer: false,
      }
      const inlineAnswer = numbered.remainder.match(answerContentPattern)
      if (inlineAnswer) {
        pending.acceptingAnswer = true
        if ((inlineAnswer[1] ?? '').trim() || line.images.length) {
          pending.answerLines.push(cloneLine(line, (inlineAnswer[1] ?? '').trim()))
        }
      } else if (
        (numbered.remainder || line.images.length)
        && !isRepeatedQuestionStem(questions, resolvedSection, numbered.number, numbered.remainder)
      ) {
        pending.acceptingAnswer = true
        pending.answerLines.push(cloneLine(line, numbered.remainder))
      }
      continue
    }

    if (!pending) continue
    const answerContent = line.text.match(answerContentPattern)
    if (answerContent) {
      pending.acceptingAnswer = true
      if ((answerContent[1] ?? '').trim() || line.images.length) {
        pending.answerLines.push(cloneLine(line, (answerContent[1] ?? '').trim()))
      }
    } else if (pending.acceptingAnswer) {
      pending.answerLines.push(line)
    }
  }
  finishPending()
  return answers
}

function parseExplanations(lines: readonly WordAnalysisLine[], questions: readonly RawQuestion[]) {
  const explanations = new Map<string, WordAnalysisLine[]>()
  let section: WordQuestionSection | null = null
  let pending: PendingAnswer | null = null

  const finishPending = () => {
    if (pending?.answerLines.length) {
      explanations.set(answerKey(pending.section, pending.sourceNumber), pending.answerLines)
    }
    pending = null
  }

  for (const sourceLine of lines) {
    const line = cloneLine(sourceLine, sourceLine.text.trim())
    if (!hasLineContent(line)) continue
    const heading = parseSectionHeading(line.text)
    if (heading) {
      finishPending()
      section = heading.section
      continue
    }

    const numbered = parseTopLevelNumber(line.text)
    if (numbered) {
      finishPending()
      const resolvedSection = section
        ?? matchingQuestionSection(questions, numbered.number, numbered.remainder)
      pending = {
        section: resolvedSection,
        sourceNumber: numbered.number,
        answerLines: [],
        acceptingAnswer: true,
      }
      const inlineExplanation = numbered.remainder.match(explanationContentPattern)
      const content = inlineExplanation ? (inlineExplanation[1] ?? '').trim() : numbered.remainder
      if (content || line.images.length || line.tables?.length) {
        pending.answerLines.push(cloneLine(line, content))
      }
      continue
    }

    if (!pending) continue
    const explanationContent = line.text.match(explanationContentPattern)
    if (explanationContent) {
      if ((explanationContent[1] ?? '').trim() || line.images.length || line.tables?.length) {
        pending.answerLines.push(cloneLine(line, (explanationContent[1] ?? '').trim()))
      }
    } else {
      pending.answerLines.push(line)
    }
  }
  finishPending()
  return explanations
}

function buildQuestion(
  raw: RawQuestion,
  answers: ReadonlyMap<string, WordAnalysisLine[]>,
  explanations: ReadonlyMap<string, WordAnalysisLine[]>,
): RecognizedWordQuestion {
  const stemLines: WordAnalysisLine[] = []
  const optionEntries: Array<{ label: string; lines: WordAnalysisLine[] }> = []
  const inlineAnswerLines: WordAnalysisLine[] = []
  const inlineExplanationLines: WordAnalysisLine[] = []
  let inlineSection: 'answer' | 'explanation' | null = null
  const parseOptions = raw.section === 'choice' || raw.section === 'true_false' || raw.section === null

  for (const line of raw.lines) {
    const inlineExplanation = line.text.match(explanationContentPattern)
    if (inlineExplanation) {
      inlineSection = 'explanation'
      const content = (inlineExplanation[1] ?? '').trim()
      if (content || line.images.length || line.tables?.length) {
        inlineExplanationLines.push(cloneLine(line, content))
      }
      continue
    }

    const inlineAnswer = line.text.match(answerContentPattern)
    if (inlineAnswer) {
      inlineSection = 'answer'
      const content = (inlineAnswer[1] ?? '').trim()
      if (content || line.images.length || line.tables?.length) {
        inlineAnswerLines.push(cloneLine(line, content))
      }
      continue
    }

    if (inlineSection) {
      const target = inlineSection === 'answer' ? inlineAnswerLines : inlineExplanationLines
      target.push(cloneLine(line))
      continue
    }

    const split = parseOptions ? splitOptionLine(line) : null
    if (!split) {
      if (optionEntries.length && !line.images.length && !line.tables?.length) {
        optionEntries[optionEntries.length - 1]?.lines.push(cloneLine(line))
      }
      else stemLines.push(cloneLine(line))
      continue
    }

    if (split.prefix || split.prefixImages.length) {
      stemLines.push(cloneLine(line, split.prefix, split.prefixImages))
    }
    for (const option of split.options) {
      const existing = optionEntries.find((entry) => entry.label === option.label)
      if (existing) existing.lines.push(option.line)
      else optionEntries.push({ label: option.label, lines: [option.line] })
    }
  }

  let options = optionEntries
    .sort((left, right) => left.label.localeCompare(right.label))
    .map((entry) => entry.lines)
  const hasStemImage = stemLines.some((line) => line.images.length)
  const hasTextOptions = options.some((option) => option.some((line) => line.text.trim()))
  if (raw.section === 'choice' && hasStemImage && !hasTextOptions) {
    const paragraphIndex = stemLines[0]?.paragraphIndex ?? raw.lines[0]?.paragraphIndex ?? -1
    options = ['A', 'B', 'C', 'D'].map((label) => [{
      paragraphIndex,
      text: `见题干图片中的 ${label} 项`,
      images: [],
    }])
  }

  const mappedAnswerLines = raw.sourceNumber === null
    ? []
    : withoutEmptyExportPlaceholder(answers.get(answerKey(raw.section, raw.sourceNumber)) ?? [])
  const mappedExplanationLines = raw.sourceNumber === null
    ? []
    : withoutEmptyExportPlaceholder(explanations.get(answerKey(raw.section, raw.sourceNumber)) ?? [])
  const filteredInlineAnswerLines = withoutEmptyExportPlaceholder(inlineAnswerLines)
  const filteredInlineExplanationLines = withoutEmptyExportPlaceholder(inlineExplanationLines)
  const answerLines = filteredInlineAnswerLines.length ? filteredInlineAnswerLines : mappedAnswerLines
  const explanationLines = filteredInlineExplanationLines.length
    ? filteredInlineExplanationLines
    : mappedExplanationLines
  return {
    section: raw.section,
    sourceNumber: raw.sourceNumber,
    type: inferType(raw, stemLines, options, answerLines),
    stemLines,
    options,
    answerLines,
    explanationLines,
    unknownTypeName: raw.unknownTypeName,
    suggestedBehavior: raw.unknownTypeName
      ? (options.length >= 2 ? 'single_choice' : raw.section === 'fill_blank' ? 'fill_blank' : 'open_response')
      : null,
  }
}

export function recognizeWordQuestions(
  lines: readonly WordAnalysisLine[],
  definitions?: readonly QuestionTypeDefinition[],
): RecognizedWordQuestion[] {
  const normalizedLines = lines
    .map((line) => cloneLine(line, line.text.trim()))
    .filter(hasLineContent)
  const answerHeadingIndex = normalizedLines.findIndex((line) => answerHeadingPattern.test(line.text))
  const explanationHeadingIndex = normalizedLines.findIndex((line) => explanationHeadingPattern.test(line.text))
  const supplementalHeadingIndices = [answerHeadingIndex, explanationHeadingIndex].filter((index) => index >= 0)
  const bodyEnd = supplementalHeadingIndices.length ? Math.min(...supplementalHeadingIndices) : normalizedLines.length
  const bodyLines = normalizedLines.slice(0, bodyEnd)
  const answerEnd = explanationHeadingIndex > answerHeadingIndex
    ? explanationHeadingIndex
    : normalizedLines.length
  const answerLines = answerHeadingIndex >= 0
    ? normalizedLines.slice(answerHeadingIndex + 1, answerEnd)
    : []
  const explanationEnd = answerHeadingIndex > explanationHeadingIndex
    ? answerHeadingIndex
    : normalizedLines.length
  const explanationLines = explanationHeadingIndex >= 0
    ? normalizedLines.slice(explanationHeadingIndex + 1, explanationEnd)
    : []
  const rawQuestions = parseBody(bodyLines, definitions)
  const answers = parseAnswers(answerLines, rawQuestions)
  const explanations = parseExplanations(explanationLines, rawQuestions)
  return rawQuestions.map((question) => {
    const recognized = buildQuestion(question, answers, explanations)
    if (question.explicitType && !recognized.unknownTypeName) {
      recognized.suggestedBehavior = questionTypeBehavior(question.explicitType, definitions)
    }
    return recognized
  })
}
