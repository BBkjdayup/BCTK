import { mount } from '@vue/test-utils'
import { describe, expect, it } from 'vitest'
import UnitSelect from './UnitSelect.vue'

const chapter = { id: 'chapter-1', subjectId: 'subject-1', name: '章节', sortOrder: 1, questionCount: 1, units: [{ id: 'unit-1', chapterId: 'chapter-1', name: '单元', sortOrder: 1, questionCount: 1 }] }
const stubs = { ElSelect: { name: 'ElSelect', props: ['modelValue', 'disabled'], emits: ['update:modelValue'], template: '<div><slot /></div>' }, ElOption: true }

describe('optional unit selection', () => {
  it('clears a selected unit when the chapter changes and disables selection without a chapter', async () => {
    const wrapper = mount(UnitSelect, { props: { chapter, modelValue: 'unit-1' }, global: { stubs } })
    await wrapper.setProps({ chapter: { ...chapter, id: 'chapter-2', units: [] } })
    expect(wrapper.emitted('update:modelValue')).toEqual([[undefined]])
    await wrapper.setProps({ chapter: undefined, modelValue: undefined })
    expect(wrapper.findComponent({ name: 'ElSelect' }).props('disabled')).toBe(true)
  })

  it('uses an empty selection for unassigned questions while retaining an available unit', async () => {
    const wrapper = mount(UnitSelect, { props: { chapter, modelValue: 'unit-1' }, global: { stubs } })
    await wrapper.setProps({ chapter: { ...chapter, name: '重命名章节' } })
    expect(wrapper.emitted('update:modelValue')).toBeUndefined()
    wrapper.findComponent({ name: 'ElSelect' }).vm.$emit('update:modelValue', '')
    expect(wrapper.emitted('update:modelValue')).toEqual([[undefined]])
  })
})
