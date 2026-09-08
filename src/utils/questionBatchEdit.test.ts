import { describe, expect, it } from 'vitest'
import type { Question } from '../types/domain'
import {
  buildQuestionBatchEditRequest,
  createQuestionBatchEditForm,
  questionBatchEditSummary,
  questionBatchEditValidation,
} from './questionBatchEdit'

function question(id: string, version: number): Question {
  const rich = { schemaVersion: 1 as const, html: '<p>内容</p>', plainText: '内容' }
  return {
    id,
    type: 'short_answer',
    stem: rich,
    options: [],
    answer: rich,
    explanation: { schemaVersion: 1, html: '', plainText: '' },
    subjectId: 'subject-old',
    chapterId: 'chapter-old',
    subjectName: '旧学科',
    chapterName: '旧章节',
    tags: [],
    createdAt: 1,
    updatedAt: 1,
    contentVersion: version,
  }
}

describe('question batch edit request', () => {
  it('requires at least one explicit operation', () => {
    const form = createQuestionBatchEditForm()
    expect(questionBatchEditValidation([question('q1', 2)], form)).toContain('尚未选择')
  })

  it('captures every expected content version and a complete classification', () => {
    const form = createQuestionBatchEditForm()
    form.classificationMode = 'change'
    form.subjectId = 'subject-new'
    form.chapterId = 'chapter-new'
    const request = buildQuestionBatchEditRequest([question('q1', 2), question('q2', 7)], form)
    expect(request.questions).toEqual([
      { id: 'q1', expectedContentVersion: 2 },
      { id: 'q2', expectedContentVersion: 7 },
    ])
    expect(request.classification).toEqual({ subjectId: 'subject-new', chapterId: 'chapter-new' })
  })

  it('distinguishes append, replace-empty, and remove operations', () => {
    const form = createQuestionBatchEditForm()
    form.tagMode = 'append'
    form.tagIds = ['tag-a']
    expect(buildQuestionBatchEditRequest([question('q1', 1)], form).tagOperation).toEqual({
      mode: 'append', tagIds: ['tag-a'],
    })

    form.tagMode = 'replace'
    form.tagIds = []
    expect(questionBatchEditSummary(form)).toContain('清空全部标签')

    form.tagMode = 'remove'
    expect(questionBatchEditValidation([question('q1', 1)], form)).toContain('请选择要删除')
  })

  it('represents the derived usage state as an explicit reset operation', () => {
    const form = createQuestionBatchEditForm()
    form.usageOperation = 'reset_never'
    expect(buildQuestionBatchEditRequest([question('q1', 3)], form).usageOperation).toBe('reset_never')
    expect(questionBatchEditSummary(form)[0]).toContain('从未使用')
  })
})
