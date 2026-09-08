export interface CommandErrorPayload {
  code: string
  message: string
}

export function isCommandErrorPayload(value: unknown): value is CommandErrorPayload {
  if (!value || typeof value !== 'object') return false
  const candidate = value as Record<string, unknown>
  return typeof candidate.code === 'string' && typeof candidate.message === 'string'
}

export function errorMessage(reason: unknown, fallback: string): string {
  if (reason instanceof Error && reason.message.trim()) return reason.message
  if (isCommandErrorPayload(reason) && reason.message.trim()) return reason.message
  if (typeof reason === 'string' && reason.trim()) return reason
  return fallback
}

