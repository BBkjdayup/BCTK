import { describe, expect, it } from 'vitest'
import { buildTaxonomyOrderItems } from './taxonomyOrder'

describe('buildTaxonomyOrderItems', () => {
  it('converts the dragged DOM order into stable spaced positions', () => {
    expect(buildTaxonomyOrderItems(
      ['chapter-2', 'chapter-1', 'chapter-3'],
      ['chapter-1', 'chapter-2', 'chapter-3'],
    )).toEqual([
      { id: 'chapter-2', sortOrder: 1024 },
      { id: 'chapter-1', sortOrder: 2048 },
      { id: 'chapter-3', sortOrder: 3072 },
    ])
  })

  it('rejects missing, duplicated, or unexpected DOM entries', () => {
    expect(buildTaxonomyOrderItems(['chapter-1'], ['chapter-1', 'chapter-2'])).toBeNull()
    expect(buildTaxonomyOrderItems(
      ['chapter-1', 'chapter-1'],
      ['chapter-1', 'chapter-2'],
    )).toBeNull()
    expect(buildTaxonomyOrderItems(
      ['chapter-1', 'other'],
      ['chapter-1', 'chapter-2'],
    )).toBeNull()
  })
})
