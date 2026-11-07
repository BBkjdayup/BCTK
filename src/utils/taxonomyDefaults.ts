import type { Chapter, Subject, Unit } from '../types/domain'

export interface TaxonomyDefault {
  subject?: Subject
  chapter?: Chapter
  unit?: Unit
}

export function resolveTaxonomyDefault(
  subjects: readonly Subject[],
  requestedSubjectId?: unknown,
  requestedChapterId?: unknown,
  requestedUnitId?: unknown,
): TaxonomyDefault {
  const subject = typeof requestedSubjectId === 'string'
    ? subjects.find((entry) => entry.id === requestedSubjectId) ?? subjects[0]
    : subjects[0]
  if (!subject) return {}
  const chapter = typeof requestedChapterId === 'string'
    ? subject.chapters.find((entry) => entry.id === requestedChapterId) ?? subject.chapters[0]
    : subject.chapters[0]
  const unit = typeof requestedUnitId === 'string' ? chapter?.units?.find((entry) => entry.id === requestedUnitId) : undefined
  return { subject, chapter, ...(unit ? { unit } : {}) }
}
