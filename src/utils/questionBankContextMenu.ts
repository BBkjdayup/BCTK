import type { ContextMenuItem } from '../types/contextMenu'

export function questionBankContextMenuItems(selectionCount: number): ContextMenuItem[] {
  if (selectionCount > 1) {
    return [
      { id: 'add-to-paper', label: `加入当前试卷（${selectionCount}题）` },
      { id: 'batch-edit', label: '批量修改分类与标签' },
      { id: 'recycle', label: '批量移入回收站', danger: true, dividerBefore: true },
      { id: 'clear-selection', label: '取消全部选择', dividerBefore: true },
    ]
  }

  return [
    { id: 'preview', label: '预览题目' },
    { id: 'edit', label: '编辑题目' },
    { id: 'copy-question', label: '复制为新题' },
    { id: 'batch-edit', label: '修改分类与标签' },
    { id: 'add-to-paper', label: '加入当前试卷', dividerBefore: true },
    { id: 'copy-stem', label: '复制题干文字', dividerBefore: true },
    { id: 'recycle', label: '移入回收站', danger: true, dividerBefore: true },
  ]
}
