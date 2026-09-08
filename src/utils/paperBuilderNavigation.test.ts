import { describe, expect, it } from 'vitest'
import { resolvePaperBuilderEntryMode } from './paperBuilderNavigation'

describe('paper builder entry navigation', () => {
  it('returns to the selection workspace even when the current paper has items', () => {
    expect(resolvePaperBuilderEntryMode(undefined, true)).toBe('manual')
  })

  it('opens the editor only for an explicit history edit request with a current paper', () => {
    expect(resolvePaperBuilderEntryMode('edit', true)).toBe('edit')
    expect(resolvePaperBuilderEntryMode('edit', false)).toBe('manual')
  })
})
