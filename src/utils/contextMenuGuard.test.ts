import { describe, expect, it, vi } from 'vitest'
import {
  isNativeTextContextTarget,
  suppressUnconfiguredContextMenu,
} from './contextMenuGuard'

function contextMenuEvent(target: Element) {
  const event = new MouseEvent('contextmenu', { bubbles: true, cancelable: true })
  Object.defineProperty(event, 'target', { configurable: true, value: target })
  return event
}

describe('global context-menu guard', () => {
  it('allows native editing menus for text inputs, textareas, and rich text', () => {
    const input = document.createElement('input')
    const textarea = document.createElement('textarea')
    const richText = document.createElement('div')
    richText.setAttribute('contenteditable', 'true')

    expect(isNativeTextContextTarget(input)).toBe(true)
    expect(isNativeTextContextTarget(textarea)).toBe(true)
    expect(isNativeTextContextTarget(richText)).toBe(true)
  })

  it('blocks the native menu on ordinary application controls and blank areas', () => {
    const button = document.createElement('button')
    const blankArea = document.createElement('div')

    for (const target of [button, blankArea]) {
      const event = contextMenuEvent(target)
      suppressUnconfiguredContextMenu(event)
      expect(event.defaultPrevented).toBe(true)
    }
  })

  it('does not interfere after a configured custom context menu handled the event', () => {
    const event = contextMenuEvent(document.createElement('div'))
    event.preventDefault()
    const preventDefault = vi.spyOn(event, 'preventDefault')

    suppressUnconfiguredContextMenu(event)

    expect(preventDefault).not.toHaveBeenCalled()
  })
})
