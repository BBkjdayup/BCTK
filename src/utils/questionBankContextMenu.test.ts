import { describe, expect, it } from 'vitest'
import { questionBankContextMenuItems } from './questionBankContextMenu'

describe('questionBankContextMenuItems', () => {
  it('offers category and tag editing for a single question', () => {
    const items = questionBankContextMenuItems(1)

    expect(items).toContainEqual({ id: 'batch-edit', label: '修改分类与标签' })
  })

  it('keeps batch category and tag editing for multiple selected questions', () => {
    const items = questionBankContextMenuItems(3)

    expect(items).toContainEqual({ id: 'batch-edit', label: '批量修改分类与标签' })
  })
})
