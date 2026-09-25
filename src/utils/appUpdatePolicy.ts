export function safeUpdateNotes(notes: string, maxLength = 1_200): string {
  const normalized = notes.replace(/\r\n?/g, '\n').trim()
  if (normalized.length <= maxLength) return normalized
  return `${normalized.slice(0, maxLength).trimEnd()}…`
}
