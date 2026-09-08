import type { TaxonomyOrderItem } from '../types/domain'

const TAXONOMY_SORT_STEP = 1024

export function buildTaxonomyOrderItems(
  orderedIds: readonly string[],
  expectedIds: readonly string[],
): TaxonomyOrderItem[] | null {
  if (orderedIds.length !== expectedIds.length) return null
  if (new Set(orderedIds).size !== orderedIds.length) return null
  const expected = new Set(expectedIds)
  if (orderedIds.some((id) => !expected.has(id))) return null
  return orderedIds.map((id, index) => ({
    id,
    sortOrder: (index + 1) * TAXONOMY_SORT_STEP,
  }))
}
