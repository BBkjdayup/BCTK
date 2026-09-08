import { describe, expect, it } from 'vitest'
import {
  requiresFirstCloudSyncConfirmation,
  shouldRunAutomaticCloudSync,
} from './cloudSyncPolicy'

describe('cloud sync binding policy', () => {
  it('never starts background sync before the local database is bound', () => {
    expect(shouldRunAutomaticCloudSync({ canSync: true, databaseBound: false })).toBe(false)
    expect(requiresFirstCloudSyncConfirmation({ canSync: true, databaseBound: false })).toBe(true)
  })

  it('allows background sync only after binding', () => {
    expect(shouldRunAutomaticCloudSync({ canSync: true, databaseBound: true })).toBe(true)
    expect(requiresFirstCloudSyncConfirmation({ canSync: true, databaseBound: true })).toBe(false)
  })

  it('does not ask to bind when cloud sync is unavailable', () => {
    expect(shouldRunAutomaticCloudSync({ canSync: false, databaseBound: false })).toBe(false)
    expect(requiresFirstCloudSyncConfirmation({ canSync: false, databaseBound: false })).toBe(false)
  })
})
