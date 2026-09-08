import {
  type DocumentQuestionDraftItem,
  type QuestionDraft,
  type QuestionOption,
  type QuestionType,
} from '../types/domain'
import { emptyRichContent, plainTextRichContent } from './richContent'

export const MAX_DOCUMENT_QUESTION_ITEMS = 100

export function createDocumentQuestionOption(position: number): QuestionOption {
  return {
    id: crypto.randomUUID(),
    position,
    content: emptyRichContent(),
  }
}

export function createDocumentQuestionDraft(
  subjectId = '',
  chapterId = '',
  type: QuestionType = 'single_choice',
): QuestionDraft {
  return {
    type,
    stem: emptyRichContent(),
    options: type === 'single_choice' || type === 'multiple_choice' || type === 'true_false'
      ? (type === 'true_false'
          ? ['正确', '错误'].map((content, position) => ({
              ...createDocumentQuestionOption(position),
              content: plainTextRichContent(content),
            }))
          : [0, 1, 2, 3].map(createDocumentQuestionOption))
      : [],
    answer: emptyRichContent(),
    explanation: emptyRichContent(),
    subjectId,
    chapterId,
    tagIds: [],
    resourceRefs: [],
  }
}

export function createDocumentQuestionItem(
  ordinal: number,
  subjectId = '',
  chapterId = '',
  type: QuestionType = 'single_choice',
): DocumentQuestionDraftItem {
  return {
    id: crypto.randomUUID(),
    ordinal,
    payload: createDocumentQuestionDraft(subjectId, chapterId, type),
  }
}

export function isChoiceQuestion(type: QuestionType) {
  return type === 'single_choice' || type === 'multiple_choice' || type === 'true_false'
}

export function selectedChoiceLetters(draft: QuestionDraft) {
  const selected = new Set(
    draft.answer.plainText
      .toUpperCase()
      .split(/[^A-Z]+/)
      .filter(Boolean),
  )
  return draft.options
    .map((_, index) => String.fromCharCode(65 + index))
    .filter((letter) => selected.has(letter))
}

export function setChoiceAnswer(draft: QuestionDraft, letters: string[]) {
  const allowed = new Set(
    draft.options.map((_, index) => String.fromCharCode(65 + index)),
  )
  const normalized = [...new Set(letters.map((letter) => letter.toUpperCase()))]
    .filter((letter) => allowed.has(letter))
    .sort()
  draft.answer = plainTextRichContent(normalized.join('、'))
}

export function changeDocumentQuestionType(draft: QuestionDraft, type: QuestionType) {
  draft.type = type
  if (isChoiceQuestion(type)) {
    if (type === 'true_false') {
      const existingAnswer = selectedChoiceLetters(draft).find((letter) => letter === 'A' || letter === 'B')
      draft.options = ['正确', '错误'].map((content, position) => ({
        ...createDocumentQuestionOption(position),
        content: plainTextRichContent(content),
      }))
      setChoiceAnswer(draft, existingAnswer ? [existingAnswer] : [])
    } else {
      if (draft.options.length < 2) {
        draft.options = [0, 1, 2, 3].map(createDocumentQuestionOption)
      }
      if (type === 'single_choice') {
        setChoiceAnswer(draft, selectedChoiceLetters(draft).slice(0, 1))
      } else {
        setChoiceAnswer(draft, selectedChoiceLetters(draft))
      }
    }
  } else {
    draft.options = []
    draft.resourceRefs = (draft.resourceRefs ?? [])
      .filter((entry) => entry.contentSlot !== 'option')
  }
}

export function validateDocumentQuestion(
  draft: QuestionDraft,
  ordinal: number,
): string | null {
  if (!draft.type || draft.type.length > 64) return `第 ${ordinal} 题的题型无效`
  if (!draft.subjectId) return `第 ${ordinal} 题尚未选择学科`
  if (!draft.chapterId) return `第 ${ordinal} 题尚未选择章节`
  if (!draft.stem.plainText.trim()) return `第 ${ordinal} 题尚未填写题目`
  if (!draft.answer.plainText.trim()) return `第 ${ordinal} 题尚未填写答案`
  if (isChoiceQuestion(draft.type)) {
    if (draft.options.length < 2) return `第 ${ordinal} 题至少需要两个选项`
    if (draft.options.some((option) => !option.content.plainText.trim())) {
      return `第 ${ordinal} 题存在空白选项`
    }
    const selected = selectedChoiceLetters(draft)
    if ((draft.type === 'single_choice' || draft.type === 'true_false') && selected.length !== 1) {
      return `第 ${ordinal} 题必须选择一个正确答案`
    }
    if (draft.type === 'multiple_choice' && selected.length < 1) {
      return `第 ${ordinal} 题至少选择一个正确答案`
    }
  }
  return null
}

export function normalizeDocumentQuestionOrdinals(items: DocumentQuestionDraftItem[]) {
  items.forEach((item, index) => {
    item.ordinal = index + 1
    item.payload.options.forEach((option, position) => {
      option.position = position
    })
  })
}
