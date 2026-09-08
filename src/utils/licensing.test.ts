import { describe, expect, it } from 'vitest'
import { BASIC_DESKTOP_CAPABILITIES, basicLicenseOverview, remainingPaperCapacity } from './licensing'

describe('desktop licensing helpers', () => {
  it('keeps single-question editing and backup-safe basics while restricting paid features', () => {
    const overview = basicLicenseOverview('device-1')

    expect(overview.deviceId).toBe('device-1')
    expect(overview.capabilities).toEqual(BASIC_DESKTOP_CAPABILITIES)
    expect(overview.capabilities.canEditSingleQuestion).toBe(true)
    expect(overview.capabilities.canBatchImport).toBe(false)
    expect(overview.capabilities.canExportDocuments).toBe(false)
    expect(overview.capabilities.canPrint).toBe(false)
  })

  it('reports remaining capacity without limiting professional licenses', () => {
    expect(remainingPaperCapacity(7, 10)).toBe(3)
    expect(remainingPaperCapacity(12, 10)).toBe(0)
    expect(remainingPaperCapacity(12, null)).toBe(Number.POSITIVE_INFINITY)
  })
})
