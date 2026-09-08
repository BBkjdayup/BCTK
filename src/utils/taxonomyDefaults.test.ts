import { describe, expect, it } from 'vitest'
import type { Subject } from '../types/domain'
import { resolveTaxonomyDefault } from './taxonomyDefaults'

const subjects: Subject[] = [
  {
    id: 'subject-1',
    name: '学科一',
    sortOrder: 1,
    questionCount: 0,
    chapters: [
      { id: 'chapter-1', subjectId: 'subject-1', name: '章节一', sortOrder: 1, questionCount: 0 },
    ],
  },
  {
    id: 'subject-2',
    name: '学科二',
    sortOrder: 2,
    questionCount: 0,
    chapters: [
      { id: 'chapter-2', subjectId: 'subject-2', name: '章节二', sortOrder: 1, questionCount: 0 },
      { id: 'chapter-3', subjectId: 'subject-2', name: '章节三', sortOrder: 2, questionCount: 0 },
    ],
  },
]

describe('taxonomy route defaults', () => {
  it('uses the requested subject and chapter when both are valid', () => {
    const result = resolveTaxonomyDefault(subjects, 'subject-2', 'chapter-3')
    expect(result.subject?.id).toBe('subject-2')
    expect(result.chapter?.id).toBe('chapter-3')
  })

  it('never accepts a chapter from another subject', () => {
    const result = resolveTaxonomyDefault(subjects, 'subject-2', 'chapter-1')
    expect(result.subject?.id).toBe('subject-2')
    expect(result.chapter?.id).toBe('chapter-2')
  })

  it('falls back to the first available classification for invalid query values', () => {
    const result = resolveTaxonomyDefault(subjects, 'missing', ['chapter-3'])
    expect(result.subject?.id).toBe('subject-1')
    expect(result.chapter?.id).toBe('chapter-1')
  })
})
