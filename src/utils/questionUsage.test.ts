import { describe, expect, it } from 'vitest'
import { matchesQuestionUsage, SEMESTER_UNUSED_DAYS } from './questionUsage'

describe('question usage filters', () => {
  const now = new Date(2026, 8, 9, 12, 0, 0).getTime()

  it('uses a fixed rolling 180-day semester window and includes never-used questions', () => {
    const cutoff = now - SEMESTER_UNUSED_DAYS * 86_400_000

    expect(matchesQuestionUsage(null, 'unused_this_semester', now)).toBe(true)
    expect(matchesQuestionUsage(cutoff - 1, 'unused_this_semester', now)).toBe(true)
    expect(matchesQuestionUsage(cutoff, 'unused_this_semester', now)).toBe(false)
    expect(matchesQuestionUsage(now, 'unused_this_semester', now)).toBe(false)
  })

  it('uses local calendar boundaries for month, week and today', () => {
    const monthStart = new Date(2026, 8, 1, 0, 0, 0).getTime()
    const weekStart = new Date(2026, 8, 7, 0, 0, 0).getTime()
    const todayStart = new Date(2026, 8, 9, 0, 0, 0).getTime()

    expect(matchesQuestionUsage(monthStart - 1, 'unused_this_month', now)).toBe(true)
    expect(matchesQuestionUsage(monthStart, 'unused_this_month', now)).toBe(false)
    expect(matchesQuestionUsage(weekStart - 1, 'unused_this_week', now)).toBe(true)
    expect(matchesQuestionUsage(weekStart, 'unused_this_week', now)).toBe(false)
    expect(matchesQuestionUsage(todayStart - 1, 'unused_today', now)).toBe(true)
    expect(matchesQuestionUsage(todayStart, 'unused_today', now)).toBe(false)
  })

  it('keeps all and never-used semantics distinct', () => {
    expect(matchesQuestionUsage(now, 'all', now)).toBe(true)
    expect(matchesQuestionUsage(null, 'never', now)).toBe(true)
    expect(matchesQuestionUsage(now - 1, 'never', now)).toBe(false)
  })
})
