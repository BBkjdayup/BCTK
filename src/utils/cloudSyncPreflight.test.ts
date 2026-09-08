import { describe, expect, it } from 'vitest'
import type { CloudSyncPreflight } from '../types/domain'
import { cloudSyncPreflightCopy } from './cloudSyncPreflight'

function preflight(overrides: Partial<CloudSyncPreflight>): CloudSyncPreflight {
  return {
    localEntityCount: 0,
    localQuestionCount: 0,
    cloudEntityCount: 0,
    cloudQuestionCount: 0,
    localHasData: false,
    cloudHasData: false,
    bothNonEmpty: false,
    recommendedMode: 'merge',
    ...overrides,
  }
}

describe('cloud sync preflight copy', () => {
  it('defaults non-empty local and cloud databases to a non-destructive merge', () => {
    const copy = cloudSyncPreflightCopy(preflight({
      localHasData: true,
      cloudHasData: true,
      bothNonEmpty: true,
      localQuestionCount: 12,
      cloudQuestionCount: 8,
    }))
    expect(copy.title).toBe('确认智能合并')
    expect(copy.detail).toContain('不同题目全部保留')
    expect(copy.confirmText).toBe('合并并同步')
  })

  it('describes one-sided initialization without claiming a merge', () => {
    expect(cloudSyncPreflightCopy(preflight({
      localHasData: true,
      localQuestionCount: 3,
    })).detail).toContain('云端目前为空')
    expect(cloudSyncPreflightCopy(preflight({
      cloudHasData: true,
      cloudQuestionCount: 4,
    })).detail).toContain('下载云端题库')
  })
})
