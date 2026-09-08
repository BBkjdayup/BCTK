import type { QuestionDuplicateScanResult } from '../types/domain'

export type DuplicateCheckTab = 'exact' | 'suspected'

export function reconcileDuplicateCheckTab(
  current: DuplicateCheckTab,
  result: Pick<QuestionDuplicateScanResult, 'exactGroups' | 'suspectedGroups'>,
): DuplicateCheckTab {
  if (current === 'exact' && !result.exactGroups.length && result.suspectedGroups.length) {
    return 'suspected'
  }
  if (current === 'suspected' && !result.suspectedGroups.length && result.exactGroups.length) {
    return 'exact'
  }
  return current
}
