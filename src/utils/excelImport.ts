import type {
  ExcelImportRow,
  QuestionTypeBehavior,
  QuestionTypeDefinition,
  Subject,
  Tag,
} from '../types/domain'
import {
  effectiveQuestionTypes,
  matchQuestionTypeName,
  matchesChoiceBehavior,
  normalizeQuestionTypeName,
} from './questionTypes'

export interface ExcelImportDefaults {
  subjectId: string
  chapterId: string
}

export interface NormalizedExcelImportRow {
  type: string
  detectedUnknownTypeName: string | null
  suggestedBehavior: QuestionTypeBehavior | null
  stem: string
  options: string[]
  answer: string
  explanation: string
  subjectId: string
  chapterId: string
  tagIds: string[]
  unknownTagNames: string[]
}

const builtinAliases: Record<string, string> = {
  单选: 'single_choice',
  单项选择: 'single_choice',
  单项选择题: 'single_choice',
  多选: 'multiple_choice',
  多项选择: 'multiple_choice',
  多项选择题: 'multiple_choice',
  填空: 'fill_blank',
  判断: 'true_false',
  正误: 'true_false',
  正误题: 'true_false',
  是非: 'true_false',
  是非题: 'true_false',
  简答: 'short_answer',
  问答题: 'short_answer',
  解答题: 'short_answer',
}

const key = (value: string) => value.normalize('NFKC').trim().toLocaleLowerCase('zh-CN')

function suggestedBehavior(value: string): QuestionTypeBehavior {
  const normalized = normalizeQuestionTypeName(value)
  if (/多选|多项/u.test(normalized)) return 'multiple_choice'
  if (/单选|选择/u.test(normalized)) return 'single_choice'
  if (/填空/u.test(normalized)) return 'fill_blank'
  if (/判断|正误|是非/u.test(normalized)) return 'single_choice'
  return 'open_response'
}

function resolveType(value: string, definitions: readonly QuestionTypeDefinition[]) {
  const normalized = normalizeQuestionTypeName(value)
  const all = effectiveQuestionTypes(definitions)
  const direct = all.find((definition) => normalizeQuestionTypeName(definition.code) === normalized)
    ?? matchQuestionTypeName(value, definitions)
    ?? all.find((definition) => definition.code === builtinAliases[normalized])
  if (direct) {
    return { definition: direct, unknownName: null, behavior: null }
  }
  const behavior = suggestedBehavior(value)
  const fallback = all.find((definition) => definition.behavior === behavior && definition.isEnabled)
    ?? all.find((definition) => definition.code === 'short_answer')
    ?? all[0]
  return {
    definition: fallback,
    unknownName: value.trim() || '未填写题型',
    behavior,
  }
}

function normalizeChoiceAnswer(value: string, typeCode: string) {
  const normalized = value.normalize('NFKC').trim()
  if (typeCode === 'true_false') {
    if (/^(?:正确|对|是|√|true)$/iu.test(normalized)) return '正确'
    if (/^(?:错误|错|否|×|false)$/iu.test(normalized)) return '错误'
  }
  if (!/^[A-Z\s,，、;；]+$/iu.test(normalized)) return value.trim()
  const labels = [...new Set(normalized.toUpperCase().match(/[A-Z]/gu) ?? [])]
  return labels.length ? labels.join('、') : value.trim()
}

export function normalizeExcelImportRow(
  row: ExcelImportRow,
  definitions: readonly QuestionTypeDefinition[],
  subjects: readonly Subject[],
  tags: readonly Tag[],
  defaults: ExcelImportDefaults,
): NormalizedExcelImportRow {
  const resolvedType = resolveType(row.questionType, definitions)
  const defaultSubject = subjects.find((subject) => subject.id === defaults.subjectId)
  const subject = row.subjectName.trim()
    ? subjects.find((candidate) => key(candidate.name) === key(row.subjectName))
    : defaultSubject
  const defaultChapter = subject?.id === defaultSubject?.id
    ? subject?.chapters.find((chapter) => chapter.id === defaults.chapterId)
    : undefined
  const chapter = row.chapterName.trim()
    ? subject?.chapters.find((candidate) => key(candidate.name) === key(row.chapterName))
    : defaultChapter
  const tagIds: string[] = []
  const unknownTagNames: string[] = []
  for (const name of row.tagNames) {
    const matched = tags.find((tag) => key(tag.name) === key(name))
    if (matched) tagIds.push(matched.id)
    else unknownTagNames.push(name)
  }
  let options = row.options.map((option) => option.trim()).filter(Boolean)
  if (!options.length && resolvedType.definition?.defaultOptions.length) {
    options = [...resolvedType.definition.defaultOptions]
  }
  const behavior = resolvedType.definition?.behavior ?? resolvedType.behavior ?? 'open_response'
  return {
    type: resolvedType.definition?.code ?? 'short_answer',
    detectedUnknownTypeName: resolvedType.unknownName,
    suggestedBehavior: resolvedType.behavior,
    stem: row.stem.trim(),
    options: matchesChoiceBehavior(behavior) ? options : [],
    answer: matchesChoiceBehavior(behavior)
      ? normalizeChoiceAnswer(row.answer, resolvedType.definition?.code ?? '')
      : row.answer.trim(),
    explanation: row.explanation.trim(),
    subjectId: subject?.id ?? '',
    chapterId: chapter?.id ?? '',
    tagIds: [...new Set(tagIds)],
    unknownTagNames: [...new Set(unknownTagNames)],
  }
}
