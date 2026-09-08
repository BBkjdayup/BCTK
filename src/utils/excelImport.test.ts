import { describe, expect, it } from 'vitest'
import { fallbackQuestionTypes } from './questionTypes'
import { normalizeExcelImportRow } from './excelImport'
import type { ExcelImportRow, Subject, Tag } from '../types/domain'

const subjects: Subject[] = [{
  id: 'subject-1',
  name: '网设',
  sortOrder: 1,
  questionCount: 0,
  chapters: [{ id: 'chapter-1', subjectId: 'subject-1', name: '第一章', sortOrder: 1, questionCount: 0 }],
}]
const tags: Tag[] = [{ id: 'tag-1', name: '重点', questionCount: 0, createdAt: 0, updatedAt: 0 }]

const row = (overrides: Partial<ExcelImportRow> = {}): ExcelImportRow => ({
  rowNumber: 2,
  questionType: '多选',
  subjectName: '网设',
  chapterName: '第一章',
  stem: '以下哪些是网络设备？',
  options: ['交换机', '路由器', '显示器'],
  answer: ' A, B ',
  explanation: '',
  tagNames: ['重点', '新标签'],
  ...overrides,
})

describe('normalizeExcelImportRow', () => {
  it('maps built-in aliases, taxonomy, labels and choice answers', () => {
    const result = normalizeExcelImportRow(
      row(),
      fallbackQuestionTypes,
      subjects,
      tags,
      { subjectId: 'subject-1', chapterId: 'chapter-1' },
    )
    expect(result).toMatchObject({
      type: 'multiple_choice',
      subjectId: 'subject-1',
      chapterId: 'chapter-1',
      answer: 'A、B',
      tagIds: ['tag-1'],
      unknownTagNames: ['新标签'],
    })
  })

  it('uses route classification defaults when cells are blank', () => {
    const result = normalizeExcelImportRow(
      row({ subjectName: '', chapterName: '', tagNames: [] }),
      fallbackQuestionTypes,
      subjects,
      tags,
      { subjectId: 'subject-1', chapterId: 'chapter-1' },
    )
    expect(result.subjectId).toBe('subject-1')
    expect(result.chapterId).toBe('chapter-1')
  })

  it('normalizes common judgment answers and default options', () => {
    const result = normalizeExcelImportRow(
      row({ questionType: '判断', options: [], answer: '√', tagNames: [] }),
      fallbackQuestionTypes,
      subjects,
      tags,
      { subjectId: 'subject-1', chapterId: 'chapter-1' },
    )
    expect(result.type).toBe('true_false')
    expect(result.options).toEqual(['正确', '错误'])
    expect(result.answer).toBe('正确')
  })

  it('keeps an unknown type for the existing mapping dialog', () => {
    const result = normalizeExcelImportRow(
      row({ questionType: '案例研判题', options: [], answer: '参考答案', tagNames: [] }),
      fallbackQuestionTypes,
      subjects,
      tags,
      { subjectId: 'subject-1', chapterId: 'chapter-1' },
    )
    expect(result.detectedUnknownTypeName).toBe('案例研判题')
    expect(result.suggestedBehavior).toBe('open_response')
  })
})
