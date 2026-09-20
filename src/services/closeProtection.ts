type CloseGuard = () => boolean | Promise<boolean>
const guards = new Set<{ guard: CloseGuard; priority: number }>()
let pending: Promise<boolean> | null = null

/** One ordered decision for native close and updater restart. Page guards do
 * not destroy the window independently and cannot bypass other editors. */
export function registerCloseGuard(guard: CloseGuard, priority = 0) {
  const entry = { guard, priority }
  guards.add(entry)
  return () => { guards.delete(entry) }
}

export async function canCloseApplication(): Promise<boolean> {
  if (pending) return pending
  const task = (async () => {
    for (const { guard } of [...guards].sort((a, b) => a.priority - b.priority)) {
      if (!(await guard())) return false
    }
    return true
  })()
  pending = task
  try { return await task } finally { if (pending === task) pending = null }
}
