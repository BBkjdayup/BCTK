import type { CloudAccountStatus } from '../types/domain'

type CloudBindingStatus = Pick<CloudAccountStatus, 'canSync' | 'databaseBound'>

export function shouldRunAutomaticCloudSync(status: CloudBindingStatus) {
  return status.canSync && status.databaseBound
}

export function requiresFirstCloudSyncConfirmation(status: CloudBindingStatus) {
  return status.canSync && !status.databaseBound
}
