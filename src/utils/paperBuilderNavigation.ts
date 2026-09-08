export type PaperBuilderEntryMode = 'manual' | 'edit'

export function resolvePaperBuilderEntryMode(
  requestedView: unknown,
  hasCurrentPaper: boolean,
): PaperBuilderEntryMode {
  return requestedView === 'edit' && hasCurrentPaper ? 'edit' : 'manual'
}
