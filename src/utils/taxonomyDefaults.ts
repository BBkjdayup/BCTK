import type { Chapter, Subject } from '../types/domain'

export interface TaxonomyDefault {
  subject?: Subject
  chapter?: Chapter
}

export function resolveTaxonomyDefault(
  subjects: readonly Subject[],
  requestedSubjectId?: unknown,
  requestedChapterId?: unknown,
): TaxonomyDefault {
  const subject = typeof requestedSubjectId === 'string'
    ? subjects.find((entry) => entry.id === requestedSubjectId) ?? subjects[0]
    : subjects[0]
  if (!subject) return {}
  const chapter = typeof requestedChapterId === 'string'
    ? subject.chapters.find((entry) => entry.id === requestedChapterId) ?? subject.chapters[0]
    : subject.chapters[0]
  return { subject, chapter }
}
