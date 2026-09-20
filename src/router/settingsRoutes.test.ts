import { describe, expect, it } from 'vitest'
import { createMemoryHistory, createRouter, type RouteRecordRaw } from 'vue-router'
import appRouter from './index'

function stubViews(routes: readonly RouteRecordRaw[]): RouteRecordRaw[] {
  return routes.map((route) => ({
    ...route,
    ...('component' in route ? { component: { template: '<div />' } } : {}),
    ...(route.children ? { children: stubViews(route.children) } : {}),
  } as RouteRecordRaw))
}

describe('settings route compatibility', () => {
  it.each([
    ['/settings', '/settings/general'],
    ['/templates', '/settings/templates'],
    ['/data', '/settings/backup'],
    ['/data?section=backup', '/settings/backup'],
    ['/data?section=export', '/settings/general'],
    ['/data?section=directory', '/settings/backup?panel=location'],
    ['/data?section=resources', '/settings/maintenance'],
    ['/settings/export', '/settings/general'],
    ['/settings/data', '/settings/backup?panel=location'],
  ])('redirects %s into the unified settings workspace', async (from, expected) => {
    const router = createRouter({ history: createMemoryHistory(), routes: stubViews(appRouter.options.routes) })
    await router.push(from)
    expect(router.currentRoute.value.fullPath).toBe(expected)
    expect(router.currentRoute.value.matched).toHaveLength(2)
    router.options.history.destroy()
  })
})
