import type { Paper } from '../types/domain'
import { clonePlain } from './clonePlain'
import { paperLayoutSourceSignature } from './paperSourceSignature'

/** Rebind the editable canvas and item IDs together when creating a new archive. */
export function reidentifyPaper(paper: Paper, id: string, itemIds: ReadonlyMap<string, string>): Paper {
  const result = clonePlain(paper)
  const signature = paperLayoutSourceSignature(result)
  result.id = id
  result.items = result.items.map((item) => ({ ...item, id: itemIds.get(item.id) ?? item.id }))
  if (result.layout) {
    const pending: unknown[] = [result.layout.data]
    while (pending.length) {
      const value = pending.pop()
      if (!value || typeof value !== 'object') continue
      const node = value as Record<string, unknown>
      if (typeof node.paperItemId === 'string' && itemIds.has(node.paperItemId)) {
        node.paperItemId = itemIds.get(node.paperItemId)!
      }
      pending.push(...Object.values(node))
    }
    // A stale layout must stay stale so the editor will regenerate it.
    if (result.layout.sourceSignature === signature) result.layout.sourceSignature = paperLayoutSourceSignature(result)
  }
  return result
}
