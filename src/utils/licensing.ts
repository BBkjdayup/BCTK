import type { EffectiveCapabilities, LicenseOverview } from '../types/domain'

export const BASIC_DESKTOP_CAPABILITIES: EffectiveCapabilities = {
  canEditSingleQuestion: true,
  canBatchImport: false,
  maxQuestionsPerPaper: 10,
  canExportDocuments: false,
  canPrint: false,
  canUseCloudSync: false,
  canUseWebApp: false,
}

export function basicLicenseOverview(deviceId = ''): LicenseOverview {
  return {
    deviceId,
    desktop: {
      state: 'basic',
      plan: 'basic',
      licenseId: null,
      customerName: null,
      issuedAtMs: null,
      expiresAtMs: null,
      graceEndsAtMs: null,
      message: '当前为基础桌面模式。',
    },
    cloud: {
      state: 'notConfigured',
      syncEnabled: false,
      webAppEnabled: false,
      expiresAtMs: null,
      message: '云同步与网页版尚未接入。',
    },
    capabilities: { ...BASIC_DESKTOP_CAPABILITIES },
  }
}

export function remainingPaperCapacity(currentCount: number, maxQuestions: number | null) {
  return maxQuestions == null ? Number.POSITIVE_INFINITY : Math.max(0, maxQuestions - currentCount)
}
