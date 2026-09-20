import type { Paper } from '../types/domain'

export function hashString(value: string) {
  let hash = 0x811c9dc5
  for (let index = 0; index < value.length; index += 1) {
    hash ^= value.charCodeAt(index)
    hash = Math.imul(hash, 0x01000193)
  }
  return (hash >>> 0).toString(16).padStart(8, '0')
}

export function paperLayoutSourceSignature(paper: Paper) {
  const source = [
    paper.title,
    paper.exportContentMode,
    paper.preferredTemplateId ?? '',
    ...paper.items.map((item) => [
      item.id,
      item.position,
      item.snapshot.contentVersion,
      item.snapshot.updatedAt,
    ].join(':')),
  ].join('|')
  return `paper-v1:${paper.items.length}:${hashString(source)}`
}
