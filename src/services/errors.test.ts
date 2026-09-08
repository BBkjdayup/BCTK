import { describe, expect, it } from 'vitest'
import { errorMessage, isCommandErrorPayload } from './errors'

describe('errorMessage', () => {
  it('reads a serialized Tauri command error', () => {
    const error = { code: 'VALIDATION_ERROR', message: '题干不能为空。' }
    expect(isCommandErrorPayload(error)).toBe(true)
    expect(errorMessage(error, '保存失败')).toBe('题干不能为空。')
  })

  it('falls back for unknown values', () => {
    expect(errorMessage({ message: 12 }, '操作失败')).toBe('操作失败')
  })
})

