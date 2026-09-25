import { describe, expect, it } from 'vitest'
import { safeUpdateNotes } from './appUpdatePolicy'

describe('app update display utilities', () => {
  it('normalizes and bounds release notes as plain text', () => {
    expect(safeUpdateNotes('  第一行\r\n第二行  ')).toBe('第一行\n第二行')
    expect(safeUpdateNotes('abcdef', 4)).toBe('abcd…')
  })
})
