import { flushPromises, mount } from '@vue/test-utils'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import SettingsView from './SettingsView.vue'
import type { AppSettings } from '../types/domain'

const mocks = vi.hoisted(() => ({ getSettings: vi.fn(), saveSettings: vi.fn(), confirm: vi.fn() }))
vi.mock('../services/backend', () => ({ backend: mocks, isDesktopRuntime: () => false }))
vi.mock('../stores/app', () => ({ useAppStore: () => ({ appVersion: '0.1.85' }) }))
vi.mock('../utils/appUpdateFlow', () => ({ checkAndOfferAppUpdate: vi.fn() }))
vi.mock('vue-router', () => ({ onBeforeRouteLeave: vi.fn() }))
vi.mock('element-plus', async (importOriginal) => ({
  ...await importOriginal<typeof import('element-plus')>(),
  ElMessage: { success: vi.fn(), error: vi.fn(), warning: vi.fn(), info: vi.fn() },
  ElMessageBox: { confirm: mocks.confirm },
}))

const original: AppSettings = {
  defaultExportDirectory: 'C:/test/export', defaultTemplateId: 'template-original', exportFilenamePattern: '{title}_{date}',
  optionalConfirmations: true, recentUnusedDays: 90, recentAddedDays: 30, recentUsedDays: 30,
  recycleRetentionDays: 30, recyclePolicy: 'remind_only', automaticBackupEnabled: true,
  automaticBackupIntervalDays: 7, automaticBackupRetentionCount: 5,
}
function render(page: 'general' | 'backup' | 'maintenance' = 'general') {
  return mount(SettingsView, {
    props: { page },
    global: {
      directives: { loading: () => undefined },
      stubs: {
        ElInput: { props: ['modelValue'], emits: ['update:modelValue'], template: '<input :value="modelValue" @input="$emit(\'update:modelValue\', $event.target.value)" />' },
        ElInputNumber: { props: ['modelValue'], emits: ['update:modelValue'], template: '<input type="number" :value="modelValue" @input="$emit(\'update:modelValue\', Number($event.target.value))" />' },
        ElButton: { props: ['disabled'], template: '<button :disabled="disabled"><slot /></button>' },
        ElSwitch: true, ElIcon: true,
      },
    },
  })
}

beforeEach(() => {
  vi.clearAllMocks()
  mocks.getSettings.mockResolvedValue({ ...original })
  mocks.saveSettings.mockImplementation(async (settings: AppSettings) => settings)
})

describe('merged settings', () => {
  it('preserves hidden legacy policies and other preferences when saving maintenance settings', async () => {
    const wrapper = render('maintenance')
    await flushPromises()
    await wrapper.get('[aria-label="回收站参考保留天数"]').setValue('60')
    mocks.getSettings.mockResolvedValue({ ...original, recyclePolicy: 'manual_only', optionalConfirmations: false, automaticBackupIntervalDays: 14 })
    await wrapper.findAll('button').find((button) => button.text() === '保存设置')!.trigger('click')
    await flushPromises()
    expect(mocks.saveSettings).toHaveBeenCalledWith({ ...original, recycleRetentionDays: 60, recyclePolicy: 'manual_only', optionalConfirmations: false, automaticBackupIntervalDays: 14 })
    wrapper.unmount()
  })

  it('preserves the latest template and backup choices when saving general settings', async () => {
    const wrapper = render()
    await flushPromises()
    await wrapper.get('#export-filename').setValue('{title}_新版')
    mocks.confirm.mockRejectedValueOnce('cancel')
    expect(await wrapper.vm.canLeave()).toBe(false)
    mocks.getSettings.mockResolvedValue({ ...original, defaultTemplateId: 'template-new', automaticBackupIntervalDays: 14 })
    await wrapper.findAll('button').find((button) => button.text() === '保存设置')!.trigger('click')
    await flushPromises()
    expect(mocks.saveSettings).toHaveBeenCalledWith({ ...original, defaultTemplateId: 'template-new', automaticBackupIntervalDays: 14, exportFilenamePattern: '{title}_新版' })
    expect(await wrapper.vm.canLeave()).toBe(true)
    wrapper.unmount()
  })

  it('saves only backup preferences from the embedded backup form', async () => {
    const wrapper = render('backup')
    await flushPromises()
    await wrapper.get('[aria-label="自动备份间隔天数"]').setValue('21')
    mocks.getSettings.mockResolvedValue({ ...original, defaultTemplateId: 'template-new', exportFilenamePattern: '{date}_{title}' })
    await wrapper.findAll('button').find((button) => button.text() === '保存设置')!.trigger('click')
    await flushPromises()
    expect(mocks.saveSettings).toHaveBeenCalledWith({ ...original, defaultTemplateId: 'template-new', exportFilenamePattern: '{date}_{title}', automaticBackupIntervalDays: 21 })
    wrapper.unmount()
  })
})
