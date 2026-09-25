import { flushPromises, mount } from '@vue/test-utils'
import { nextTick, reactive } from 'vue'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import AppShell from './AppShell.vue'

const push = vi.fn()
const route = reactive({ path: '/questions' })
const windowApi = vi.hoisted(() => ({
  minimize: vi.fn(),
  toggleMaximize: vi.fn(),
  close: vi.fn(),
  destroy: vi.fn(),
  onCloseRequested: vi.fn().mockResolvedValue(() => undefined),
}))
const closeProtection = vi.hoisted(() => ({
  canCloseApplication: vi.fn(),
  registerCloseGuard: vi.fn(() => vi.fn()),
}))
const updater = vi.hoisted(() => ({ installStagedAppUpdateOnClose: vi.fn() }))
const appStore = {
  appVersion: '0.1.0',
  databaseHealthy: true,
  loading: false,
  initialized: true,
  error: null,
  createInitialSubject: vi.fn(),
  retryDatabase: vi.fn(),
}

vi.mock('vue-router', () => ({
  useRoute: () => route,
  useRouter: () => ({ push }),
}))

vi.mock('../stores/app', () => ({
  useAppStore: () => appStore,
}))

vi.mock('@tauri-apps/api/window', () => ({
  getCurrentWindow: () => windowApi,
}))

vi.mock('../services/closeProtection', () => closeProtection)
vi.mock('../utils/appUpdateFlow', () => updater)

function setDesktopRuntime(enabled: boolean) {
  const runtimeWindow = window as unknown as Record<string, unknown>
  if (enabled) {
    Object.defineProperty(runtimeWindow, '__TAURI_INTERNALS__', {
      configurable: true,
      value: {},
    })
  } else {
    Reflect.deleteProperty(runtimeWindow, '__TAURI_INTERNALS__')
  }
}

function renderShell() {
  return mount(AppShell, {
    global: {
      stubs: {
        RouterView: true,
        ElButton: { template: '<button><slot /></button>' },
        ElIcon: { template: '<i><slot /></i>' },
      },
    },
  })
}

beforeEach(() => {
  setActivePinia(createPinia())
  closeProtection.canCloseApplication.mockResolvedValue(true)
  updater.installStagedAppUpdateOnClose.mockResolvedValue(false)
  windowApi.onCloseRequested.mockResolvedValue(() => undefined)
})

afterEach(() => {
  setDesktopRuntime(false)
  route.path = '/questions'
  vi.clearAllMocks()
})

describe('AppShell runtime identity', () => {
  it('marks browser rendering as a non-file-writing interface demo', () => {
    setDesktopRuntime(false)
    const wrapper = renderShell()

    expect(wrapper.get('.browser-preview-banner').text()).toContain('浏览器界面演示')
    expect(wrapper.get('.browser-preview-banner').text()).toContain('不会读写真实文件')
    expect(wrapper.get('.titlebar__version').text()).not.toContain('Windows')
    expect(wrapper.get('.topnav__status').text()).toContain('浏览器演示模式')
    expect(wrapper.get('.topnav__status').text()).not.toContain('本地模式')
  })

  it('keeps the desktop identity when Tauri is available', () => {
    setDesktopRuntime(true)
    const wrapper = renderShell()

    expect(wrapper.find('.browser-preview-banner').exists()).toBe(false)
    expect(wrapper.get('.titlebar__version').text()).toContain('Windows 桌面版')
    expect(wrapper.get('.topnav__status').text()).toContain('免费离线版')
  })

  it('hands a staged update to the installer only after close protection passes', async () => {
    setDesktopRuntime(true)
    updater.installStagedAppUpdateOnClose.mockResolvedValue(true)
    const wrapper = renderShell()
    await flushPromises()
    const onClose = windowApi.onCloseRequested.mock.calls[0]?.[0] as ((event: { preventDefault: () => void }) => Promise<void>)
    const event = { preventDefault: vi.fn() }

    await onClose(event)

    expect(event.preventDefault).toHaveBeenCalledTimes(1)
    expect(closeProtection.canCloseApplication).toHaveBeenCalledTimes(1)
    expect(updater.installStagedAppUpdateOnClose).toHaveBeenCalledTimes(1)
    expect(windowApi.destroy).not.toHaveBeenCalled()
    wrapper.unmount()
  })

  it('allows system printing without a license or account', () => {
    setDesktopRuntime(true)
    const wrapper = renderShell()
    const event = new KeyboardEvent('keydown', { key: 'p', ctrlKey: true, cancelable: true })
    window.dispatchEvent(event)
    expect(event.defaultPrevented).toBe(false)
    expect(wrapper.text()).not.toContain('试用')
    wrapper.unmount()
  })

  it('gives the exact question-entry route priority over the question-bank prefix', async () => {
    const wrapper = renderShell()
    route.path = '/questions/new'
    await nextTick()

    const activeNavigation = wrapper.get('.topnav__item.is-active')
    expect(activeNavigation.text()).toBe('题目录入')
  })

  it('groups Word import under question entry instead of a separate top-level tab', async () => {
    const wrapper = renderShell()
    route.path = '/word-import'
    await nextTick()

    expect(wrapper.get('.topnav__item.is-active').text()).toBe('题目录入')
    expect(wrapper.findAll('.topnav__item').map((item) => item.text())).not.toContain('Word 导入')
  })

  it('keeps the duplicate-check workspace under question-bank management', async () => {
    const wrapper = renderShell()
    route.path = '/duplicate-check'
    await nextTick()

    expect(wrapper.get('.topnav__item.is-active').text()).toBe('题库管理')
  })
})
