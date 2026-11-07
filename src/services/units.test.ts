import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createQuestionFilters } from '../stores/questionBank'
import type { Question, QuestionDraft } from '../types/domain'
import { normalizeExcelImportRow } from '../utils/excelImport'
import { fallbackQuestionTypes } from '../utils/questionTypes'
import { applyWordImportBatchClassification, wordImportQuestionSemanticSignature } from '../utils/wordImportBatch'

let preview: typeof import('./mockBackend').mockBackend
function draft(question: Question, unitId?: string): QuestionDraft {
  return { ...question, questionId: question.id, baseContentVersion: question.contentVersion, unitId, tagIds: question.tags.map((tag) => tag.id) }
}
beforeEach(async () => {
  localStorage.clear()
  vi.resetModules()
  preview = (await import('./mockBackend')).mockBackend
})
afterEach(() => localStorage.clear())

describe('optional subject/chapter/unit classification', () => {
  it('persists units, scopes duplicate names by chapter, sorts and counts only active questions', async () => {
    const unitId = await preview.createUnit('chapter-edu-1', '知识单元')
    const second = await preview.createUnit('chapter-edu-1', '第二单元')
    await expect(preview.createUnit('chapter-edu-1', ' 知识单元 ')).rejects.toThrow('同名')
    await preview.createUnit('chapter-edu-2', '知识单元')
    const question = (await preview.getQuestion('question-1'))!
    await preview.saveQuestion(draft(question, unitId))
    await preview.saveUnitOrder('chapter-edu-1', [{ id: second, sortOrder: 1 }, { id: unitId, sortOrder: 2 }])
    const chapter = (await preview.initialize()).subjects[0]!.chapters[0]!
    expect(chapter.units?.map((unit) => [unit.id, unit.questionCount])).toEqual([[second, 0], [unitId, 1]])
    await expect(preview.saveUnitOrder(chapter.id, [{ id: unitId, sortOrder: 1 }])).rejects.toThrow('列表已发生变化')
    await preview.updateUnit(unitId, '重命名单元')
    expect((await preview.getQuestion(question.id))?.unitName).toBe('重命名单元')
    vi.resetModules()
    preview = (await import('./mockBackend')).mockBackend
    expect((await preview.initialize()).subjects[0]!.chapters[0]!.units?.[1]?.name).toBe('重命名单元')
    await preview.moveQuestionsToRecycle([question.id])
    expect((await preview.initialize()).subjects[0]!.chapters[0]!.units?.[1]?.questionCount).toBe(0)
    await expect(preview.deleteUnit(unitId)).rejects.toThrow('回收站')
    await expect(preview.deleteChapter('chapter-edu-2')).rejects.toThrow()
  })

  it('intersects unit, tag and chapter scopes and preserves legacy unassigned questions', async () => {
    const unitId = await preview.createUnit('chapter-edu-1', '基础单元')
    await preview.saveQuestion(draft((await preview.getQuestion('question-1'))!, unitId))
    const filters = createQuestionFilters({ subjectId: 'subject-education', chapterId: 'chapter-edu-1', unitId, tagIds: ['tag-foundation', 'tag-exam'] })
    expect((await preview.listQuestions(filters)).items.map((question) => question.id)).toEqual(['question-1'])
    expect((await preview.listQuestions({ ...filters, unitId: undefined, tagIds: [], unassignedUnit: true })).items.map((question) => question.id)).toEqual(['question-2'])
    expect((await preview.listQuestions({ ...filters, unitId: undefined, tagIds: [] })).total).toBe(2)
    const scope = { subjectIds: ['subject-education'], chapterIds: ['chapter-edu-1'], unitIds: [unitId], tagIds: ['tag-foundation'], tagMatchMode: 'all' as const, usage: 'all' as const, excludedQuestionIds: [] }
    expect((await preview.analyzeRandomDraw(scope)).availableTotal).toBe(1)
    expect((await preview.drawRandomQuestions({ scope, countMode: 'total', totalCount: 1, questionTypeCounts: {} }))[0]?.unitId).toBe(unitId)
    expect((await preview.analyzeRandomDraw({ ...scope, excludedQuestionIds: ['question-1'] })).availableTotal).toBe(0)
  })

  it('rejects cross-chapter saves and batches atomically, and explicitly clears a unit', async () => {
    const unitId = await preview.createUnit('chapter-edu-1', '基础单元')
    const otherUnit = await preview.createUnit('chapter-edu-2', '其他单元')
    const saved = await preview.saveQuestion(draft((await preview.getQuestion('question-1'))!, unitId))
    await expect(preview.saveQuestion(draft(saved, otherUnit))).rejects.toThrow('不属于')
    const request = { questions: [{ id: saved.id, expectedContentVersion: saved.contentVersion }, { id: 'question-2', expectedContentVersion: 1 }], classification: { subjectId: saved.subjectId, chapterId: saved.chapterId, unitId: otherUnit }, usageOperation: 'keep' as const, tagOperation: { mode: 'keep' as const, tagIds: [] } }
    await expect(preview.batchEditQuestions(request)).rejects.toThrow('不属于')
    expect((await preview.getQuestion(saved.id))?.contentVersion).toBe(saved.contentVersion)
    await preview.batchEditQuestions({ ...request, questions: [request.questions[0]!], classification: { ...request.classification, unitId: undefined } })
    expect((await preview.getQuestion(saved.id))?.unitId).toBeUndefined()
    await preview.deleteUnit(unitId)
  })

  it('distinguishes old Excel templates, blank unit cells and unknown unit names', async () => {
    const unitId = await preview.createUnit('chapter-edu-1', '基础单元')
    const { subjects, tags } = await preview.initialize()
    const row = { rowNumber: 2, questionType: '简答', subjectName: '', chapterName: '', stem: '题干', answer: '答案', explanation: '', options: [], tagNames: [] }
    const defaults = { subjectId: 'subject-education', chapterId: 'chapter-edu-1', unitId }
    const normalize = (unit: { unitName?: string } = {}) => normalizeExcelImportRow({ ...row, ...unit }, fallbackQuestionTypes, subjects, tags, defaults)
    expect(normalize().unitId).toBe(unitId)
    expect(normalize({ unitName: '' }).unitId).toBeUndefined()
    expect(normalize({ unitName: ' 基础单元 ' }).unitId).toBe(unitId)
    expect(normalize({ unitName: '不存在' }).unknownUnitName).toBe('不存在')
    const items = [{ id: '1', selected: true, subjectId: defaults.subjectId, chapterId: defaults.chapterId, unitId, unresolvedUnitName: '不存在' }]
    applyWordImportBatchClassification(items, defaults.subjectId, defaults.chapterId)
    expect(items[0]?.unitId).toBeUndefined()
    expect(items[0]?.unresolvedUnitName).toBeUndefined()
    const question = (await preview.getQuestion('question-1'))!
    expect(wordImportQuestionSemanticSignature(draft(question, unitId))).not.toBe(wordImportQuestionSemanticSignature(draft(question)))
  })

  it('retains optional units through autosave and reload without changing a saved question', async () => {
    const unitId = await preview.createUnit('chapter-edu-1', '草稿单元')
    const question = (await preview.getQuestion('question-1'))!
    await preview.saveQuestionDraft(draft(question, unitId))
    vi.resetModules()
    preview = (await import('./mockBackend')).mockBackend
    expect((await preview.getQuestionDraft(question.id))?.payload.unitId).toBe(unitId)
    expect((await preview.getQuestion(question.id))?.unitId).toBeUndefined()
    await preview.saveQuestionDraft(draft(question))
    expect((await preview.getQuestionDraft(question.id))?.payload.unitId).toBeUndefined()
  })
})
