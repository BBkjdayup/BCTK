import { describe, expect, it } from 'vitest'
import {
  changeDocumentQuestionType,
  createDocumentQuestionDraft,
  selectedChoiceLetters,
  setChoiceAnswer,
  validateDocumentQuestion,
} from './documentQuestionEntry'
import { plainTextRichContent } from './richContent'

describe('document question entry', () => {
  it('keeps choice answers canonical and removes unavailable letters', () => {
    const draft = createDocumentQuestionDraft('subject-1', 'chapter-1', 'multiple_choice')

    setChoiceAnswer(draft, ['C', 'A', 'A', 'Z'])

    expect(draft.answer.plainText).toBe('A、C')
    expect(selectedChoiceLetters(draft)).toEqual(['A', 'C'])
  })

  it('changes option structure when the question type changes', () => {
    const draft = createDocumentQuestionDraft('subject-1', 'chapter-1', 'short_answer')
    expect(draft.options).toHaveLength(0)

    changeDocumentQuestionType(draft, 'single_choice')
    expect(draft.options).toHaveLength(4)

    changeDocumentQuestionType(draft, 'short_answer')
    expect(draft.options).toHaveLength(0)

    changeDocumentQuestionType(draft, 'true_false')
    expect(draft.options.map((option) => option.content.plainText)).toEqual(['正确', '错误'])
  })

  it('replaces old choices and keeps only an A/B answer when changing to judgment', () => {
    const draft = createDocumentQuestionDraft('subject-1', 'chapter-1', 'multiple_choice')
    draft.options[0]!.content = plainTextRichContent('旧选项 A')
    setChoiceAnswer(draft, ['A', 'C'])

    changeDocumentQuestionType(draft, 'true_false')

    expect(draft.options.map((option) => option.content.plainText)).toEqual(['正确', '错误'])
    expect(draft.answer.plainText).toBe('A')
  })

  it('clears an answer outside A/B when changing to judgment', () => {
    const draft = createDocumentQuestionDraft('subject-1', 'chapter-1', 'multiple_choice')
    setChoiceAnswer(draft, ['C'])

    changeDocumentQuestionType(draft, 'true_false')

    expect(draft.answer.plainText).toBe('')
  })

  it('reports the exact incomplete question before batch save', () => {
    const draft = createDocumentQuestionDraft('subject-1', 'chapter-1', 'short_answer')
    expect(validateDocumentQuestion(draft, 7)).toBe('第 7 题尚未填写题目')

    draft.stem = plainTextRichContent('测试题干')
    expect(validateDocumentQuestion(draft, 7)).toBe('第 7 题尚未填写答案')

    draft.answer = plainTextRichContent('测试答案')
    expect(validateDocumentQuestion(draft, 7)).toBeNull()
  })
})
