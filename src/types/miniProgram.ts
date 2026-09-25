export type MiniProgramMemberStatus = 'pending' | 'active' | 'stopped' | 'rejected'

export interface MiniProgramSettings {
  quota: number
  serviceStarts: string
  serviceEnds: string
  paused: boolean
  freeQuota?: number
  paidQuota?: number
  paidStatus?: 'none' | 'active' | 'paused' | 'scheduled' | 'expired'
  paidServiceStarts?: string | null
  paidServiceEnds?: string | null
}

export interface MiniProgramPublication {
  id?: string
  paperId: string
  paperRowVersion: number
  publishedAt: number
  // Optional fields keep local previews and older server responses readable.
  title?: string
  questionCount?: number
  published?: boolean
}

export interface MiniProgramInvite {
  id: string
  code: string
  label: string
  createdAt: number
  expiresAt: number
  active: boolean
}

export interface MiniProgramMember {
  id: string
  name: string
  account: string
  inviteId: string
  status: MiniProgramMemberStatus
  requestedAt: number
  joinedAt: number | null
  stoppedAt: number | null
  reason: string | null
  seatType?: 'free' | 'paid' | null
}
