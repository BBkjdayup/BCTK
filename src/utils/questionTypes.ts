import {
  QUESTION_TYPE_LABELS,
  QUESTION_TYPES,
  type QuestionType,
  type QuestionTypeBehavior,
  type QuestionTypeDefinition,
} from '../types/domain'

const builtinBehaviors: Record<string, QuestionTypeBehavior> = {
  single_choice: 'single_choice',
  multiple_choice: 'multiple_choice',
  fill_blank: 'fill_blank',
  true_false: 'single_choice',
  short_answer: 'open_response',
}

export const fallbackQuestionTypes: QuestionTypeDefinition[] = QUESTION_TYPES.map((code, index) => ({
  code,
  name: QUESTION_TYPE_LABELS[code] ?? code,
  behavior: builtinBehaviors[code] ?? 'open_response',
  aliases: [],
  defaultOptions: code === 'true_false' ? ['正确', '错误'] : [],
  isBuiltin: true,
  isEnabled: true,
  sortOrder: (index + 1) * 10,
  questionCount: 0,
  paperItemCount: 0,
  createdAt: 0,
  updatedAt: 0,
}))

export function effectiveQuestionTypes(definitions?: readonly QuestionTypeDefinition[]) {
  return definitions?.length ? [...definitions].sort((left, right) => (
    left.sortOrder - right.sortOrder || left.code.localeCompare(right.code)
  )) : fallbackQuestionTypes
}

export function enabledQuestionTypes(definitions?: readonly QuestionTypeDefinition[]) {
  return effectiveQuestionTypes(definitions).filter((definition) => definition.isEnabled)
}

export function questionTypeDefinition(
  code: QuestionType,
  definitions?: readonly QuestionTypeDefinition[],
) {
  return effectiveQuestionTypes(definitions).find((definition) => definition.code === code)
}

export function questionTypeLabel(
  code: QuestionType,
  definitions?: readonly QuestionTypeDefinition[],
) {
  return questionTypeDefinition(code, definitions)?.name ?? QUESTION_TYPE_LABELS[code] ?? code
}

export function questionTypeBehavior(
  code: QuestionType,
  definitions?: readonly QuestionTypeDefinition[],
): QuestionTypeBehavior {
  return questionTypeDefinition(code, definitions)?.behavior ?? builtinBehaviors[code] ?? 'open_response'
}

export function isChoiceQuestionType(
  code: QuestionType,
  definitions?: readonly QuestionTypeDefinition[],
) {
  return matchesChoiceBehavior(questionTypeBehavior(code, definitions))
}

export function matchesChoiceBehavior(behavior: QuestionTypeBehavior) {
  return behavior === 'single_choice' || behavior === 'multiple_choice'
}

export function normalizeQuestionTypeName(value: string) {
  return value.normalize('NFKC').trim().toLocaleLowerCase('zh-CN').replace(/\s+/gu, '')
}

export function matchQuestionTypeName(
  value: string,
  definitions?: readonly QuestionTypeDefinition[],
) {
  const key = normalizeQuestionTypeName(value)
  return effectiveQuestionTypes(definitions).find((definition) => (
    normalizeQuestionTypeName(definition.name) === key
    || definition.aliases.some((alias) => normalizeQuestionTypeName(alias) === key)
  ))
}
