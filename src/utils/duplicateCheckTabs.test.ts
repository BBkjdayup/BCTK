import { describe, expect, it } from 'vitest'
import { reconcileDuplicateCheckTab } from './duplicateCheckTabs'

const group = {} as never

describe('reconcileDuplicateCheckTab', () => {
  it('switches to the only tab that still has results after rescanning', () => {
    expect(reconcileDuplicateCheckTab('suspected', {
      exactGroups: [group],
      suspectedGroups: [],
    })).toBe('exact')
    expect(reconcileDuplicateCheckTab('exact', {
      exactGroups: [],
      suspectedGroups: [group],
    })).toBe('suspected')
  })

  it('keeps the user selection when both tabs have results', () => {
    expect(reconcileDuplicateCheckTab('suspected', {
      exactGroups: [group],
      suspectedGroups: [group],
    })).toBe('suspected')
  })
})
