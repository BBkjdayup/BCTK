import { mount } from '@vue/test-utils'
import { nextTick, reactive } from 'vue'
import { afterEach, describe, expect, it, vi } from 'vitest'
import AppShell from './AppShell.vue'

const push = vi.fn()
const route = reactive({ path: '/questions' })
const appStore = {
  appVersion: '0.1.0',
  databaseHealthy: true,
  loading: false,
  initialized: true,
  license: {
    desktop: { state: 'basic', plan: 'basic' },
    capabilities: { canPrint: false },
  },
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
  getCurrentWindow: () => ({
    minimize: vi.fn(),
    toggleMaximize: vi.fn(),
    close: vi.fn(),
  }),
}))

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

afterEach(() => {
  setDesktopRuntime(false)
  route.path = '/questions'
  appStore.license.desktop.state = 'basic'
  appStore.license.desktop.plan = 'basic'
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
    expect(wrapper.get('.topnav__status').text()).toContain('基础桌面模式')
  })

  it('identifies the built-in professional trial separately from a paid license', async () => {
    setDesktopRuntime(true)
    appStore.license.desktop.state = 'active'
    appStore.license.desktop.plan = 'trial'
    const wrapper = renderShell()
    await nextTick()

    expect(wrapper.get('.topnav__status').text()).toContain('桌面专业版试用')
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
