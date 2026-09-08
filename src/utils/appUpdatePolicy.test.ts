import { describe, expect, it } from 'vitest'
import {
  UPDATE_PROMPT_INTERVAL_MS,
  appUpdatePromptRecord,
  safeUpdateNotes,
  shouldPromptForAppUpdate,
} from './appUpdatePolicy'

describe('app update prompt policy', () => {
  it('prompts once per version within a 24 hour window', () => {
    const now = 1_000_000
    const stored = appUpdatePromptRecord('0.1.74', now)
    expect(shouldPromptForAppUpdate(stored, '0.1.74', now + UPDATE_PROMPT_INTERVAL_MS - 1)).toBe(false)
    expect(shouldPromptForAppUpdate(stored, '0.1.74', now + UPDATE_PROMPT_INTERVAL_MS)).toBe(true)
    expect(shouldPromptForAppUpdate(stored, '0.1.75', now + 1)).toBe(true)
  })

  it('recovers from invalid or future prompt state', () => {
    expect(shouldPromptForAppUpdate('invalid-json', '0.1.74', 10)).toBe(true)
    expect(shouldPromptForAppUpdate(appUpdatePromptRecord('0.1.74', 20), '0.1.74', 10)).toBe(true)
  })

  it('normalizes and bounds release notes as plain text', () => {
    expect(safeUpdateNotes('  第一行\r\n第二行  ')).toBe('第一行\n第二行')
    expect(safeUpdateNotes('abcdef', 4)).toBe('abcd…')
  })
})
