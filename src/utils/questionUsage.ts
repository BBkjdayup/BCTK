import type { UsageFilter } from '../types/domain'

export const SEMESTER_UNUSED_DAYS = 180

export const questionUsageFilterOptions: ReadonlyArray<{
  value: UsageFilter
  label: string
}> = [
  { value: 'all', label: '不限使用状态' },
  { value: 'never', label: '从未使用过' },
  { value: 'unused_this_semester', label: `本学期未使用（${SEMESTER_UNUSED_DAYS} 天）` },
  { value: 'unused_this_month', label: '本月未使用过' },
  { value: 'unused_this_week', label: '本周未使用过' },
  { value: 'unused_today', label: '当天未使用过' },
]

function usageCutoff(usage: Exclude<UsageFilter, 'all' | 'never'>, now: number) {
  if (usage === 'unused_this_semester') return now - SEMESTER_UNUSED_DAYS * 86_400_000

  const start = new Date(now)
  start.setHours(0, 0, 0, 0)
  if (usage === 'unused_this_week') {
    start.setDate(start.getDate() - ((start.getDay() + 6) % 7))
  } else if (usage === 'unused_this_month') {
    start.setDate(1)
  }
  return start.getTime()
}

export function matchesQuestionUsage(
  lastUsedAt: number | null | undefined,
  usage: UsageFilter,
  now = Date.now(),
) {
  if (usage === 'all') return true
  if (usage === 'never') return lastUsedAt == null
  return lastUsedAt == null || lastUsedAt < usageCutoff(usage, now)
}
