/** A bounded, session-only cache. Each cloud account owns a separate instance. */
export class MediaCache<T> {
  private entries = new Map<string, { value: T; bytes: number }>()
  private bytes = 0
  constructor(private maxBytes: number, private maxEntries = 256) {}
  get(key: string) {
    const entry = this.entries.get(key)
    if (!entry) return undefined
    this.entries.delete(key); this.entries.set(key, entry)
    return entry.value
  }
  delete(key: string) {
    const entry = this.entries.get(key)
    if (entry) { this.bytes -= entry.bytes; this.entries.delete(key) }
  }
  set(key: string, value: T) {
    this.delete(key)
    const bytes = 2 * (key.length + JSON.stringify(value).length)
    if (bytes > this.maxBytes) return
    this.entries.set(key, { value, bytes }); this.bytes += bytes
    while (this.bytes > this.maxBytes || this.entries.size > this.maxEntries) this.delete(this.entries.keys().next().value!)
  }
}
export function createMediaPreparationCache() {
  return {
    formulas: new MediaCache<{ mimeType: string; dataBase64: string; width: number }>(24 * 1024 * 1024),
    uploads: new MediaCache<{ id: string; width: number; height?: number }>(4 * 1024 * 1024, 1000),
  }
}
export type MediaPreparationCache = ReturnType<typeof createMediaPreparationCache>
