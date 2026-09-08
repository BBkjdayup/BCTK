const NON_TEXT_INPUT_TYPES = new Set([
  'button',
  'checkbox',
  'color',
  'file',
  'hidden',
  'image',
  'radio',
  'range',
  'reset',
  'submit',
])

export function isNativeTextContextTarget(target: EventTarget | null) {
  if (!(target instanceof Element)) return false
  const editable = target.closest('input, textarea, [contenteditable]:not([contenteditable="false"]), [role="textbox"]')
  if (!editable) return false
  if (editable instanceof HTMLInputElement) {
    return !NON_TEXT_INPUT_TYPES.has(editable.type.toLocaleLowerCase())
  }
  return true
}

export function suppressUnconfiguredContextMenu(event: MouseEvent) {
  if (event.defaultPrevented || isNativeTextContextTarget(event.target)) return
  event.preventDefault()
}
