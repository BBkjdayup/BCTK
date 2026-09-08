export const UPDATE_PROMPT_INTERVAL_MS = 24 * 60 * 60 * 1000
export const UPDATE_PROMPT_STORAGE_KEY = 'tktiku.app-update.last-prompt.v1'

interface PromptRecord {
  version: string
  promptedAt: number
}
export function shouldPromptForAppUpdate(
  storedValue: string | null,
  version: string,
  now: number = Date.now(),
): boolean {
  if (!storedValue) return true
  try {
    const record = JSON.parse(storedValue) as Partial<PromptRecord>
    if (record.version !== version || typeof record.promptedAt !== 'number') return true
    return now - record.promptedAt >= UPDATE_PROMPT_INTERVAL_MS || now < record.promptedAt
  } catch {
    return true
  }
}

export function appUpdatePromptRecord(version: string, now: number = Date.now()): string {
  return JSON.stringify({ version, promptedAt: now } satisfies PromptRecord)
}

export function safeUpdateNotes(notes: string, maxLength = 1_200): string {
  const normalized = notes.replace(/\r\n?/g, '\n').trim()
  if (normalized.length <= maxLength) return normalized
  return `${normalized.slice(0, maxLength).trimEnd()}…`
}
