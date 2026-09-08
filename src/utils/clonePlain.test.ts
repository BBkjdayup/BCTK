import { reactive } from 'vue'
import { describe, expect, it } from 'vitest'
import { clonePlain } from './clonePlain'

describe('clonePlain', () => {
  it('clones Vue reactive objects into independent plain data', () => {
    const source = reactive({ title: '试卷', nested: { count: 1 } })
    const cloned = clonePlain(source)

    cloned.nested.count = 2
    expect(source.nested.count).toBe(1)
    expect(cloned).toEqual({ title: '试卷', nested: { count: 2 } })
  })
})
