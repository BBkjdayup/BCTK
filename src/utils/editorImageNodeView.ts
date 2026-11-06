import type { NodeViewRenderer } from '@tiptap/core'

export interface EditorImageSource {
  src: string
  width?: number | null
  height?: number | null
}

interface ImageSourceProvider {
  cached: (resourceId: string) => EditorImageSource | undefined
  load: (resourceId: string, retry: boolean) => Promise<EditorImageSource>
}

function positiveNumber(value: unknown) {
  const number = Number(value)
  return Number.isFinite(number) && number > 0 ? number : null
}

function contentWidth(element: HTMLElement) {
  const style = window.getComputedStyle(element)
  return element.clientWidth - (Number.parseFloat(style.paddingLeft) || 0)
    - (Number.parseFloat(style.paddingRight) || 0)
}

/** Images are atomic editor nodes: their controls and previews are never document content. */
export function createEditorImageNodeView(sources: ImageSourceProvider): NodeViewRenderer {
  return ({ node, editor, getPos }) => {
    let currentNode = node
    let destroyed = false
    let sourceIdentity = ''
    let sourceGeneration = 0
    let sourceState: 'idle' | 'loading' | 'ready' | 'error' = 'idle'
    let intrinsicSize: EditorImageSource | undefined
    let cleanupDrag: (() => void) | null = null
    const dom = document.createElement('span')
    const image = document.createElement('img')
    const handle = document.createElement('span')
    const sizeLabel = document.createElement('span')
    const status = document.createElement('span')
    const statusText = document.createElement('span')
    const retryButton = document.createElement('button')
    dom.className = 'rich-editor-image-node'
    dom.contentEditable = 'false'
    image.draggable = false
    handle.className = 'rich-editor-image-node__handle'
    handle.setAttribute('role', 'button')
    handle.setAttribute('aria-label', '拖动调整图片大小')
    handle.setAttribute('title', '拖动调整图片大小')
    handle.tabIndex = 0
    sizeLabel.className = 'rich-editor-image-node__size'
    status.className = 'rich-editor-image-node__status'
    status.setAttribute('role', 'status')
    retryButton.type = 'button'
    retryButton.textContent = '重新加载'
    retryButton.setAttribute('aria-label', '重新加载图片')
    status.append(statusText, retryButton)
    dom.append(image, handle, sizeLabel, status)

    const updateStatus = () => {
      dom.dataset.resourceState = sourceState
      status.hidden = sourceState !== 'error' && (sourceState !== 'loading' || Boolean(image.getAttribute('src')))
      statusText.textContent = sourceState === 'error' ? '图片读取失败' : '图片加载中'
      retryButton.hidden = sourceState !== 'error'
    }

    const renderDimensions = (width: number | null, height: number | null) => {
      if (width) image.setAttribute('width', String(Math.round(width)))
      else image.removeAttribute('width')
      if (height) image.setAttribute('height', String(Math.round(height)))
      else image.removeAttribute('height')
      const fallbackWidth = positiveNumber(intrinsicSize?.width) ?? positiveNumber(image.naturalWidth)
      const fallbackHeight = positiveNumber(intrinsicSize?.height) ?? positiveNumber(image.naturalHeight)
      const layoutWidth = width ?? fallbackWidth
      dom.style.width = layoutWidth ? `${layoutWidth}px` : ''
      dom.style.aspectRatio = width && height
        ? `${width} / ${height}`
        : fallbackWidth && fallbackHeight ? `${fallbackWidth} / ${fallbackHeight}` : ''
      sizeLabel.textContent = width ? `${Math.round(width)} px` : ''
    }

    const renderSavedDimensions = () => renderDimensions(
      positiveNumber(currentNode.attrs.width),
      positiveNumber(currentNode.attrs.height),
    )

    const applySource = (source: EditorImageSource) => {
      intrinsicSize = source
      if (image.getAttribute('src') !== source.src) image.setAttribute('src', source.src)
      sourceState = 'ready'
      updateStatus()
      if (!cleanupDrag) renderSavedDimensions()
    }

    const loadSource = (retry = false) => {
      if (!retry && sourceState !== 'idle') return
      const resourceId = String(currentNode.attrs.resourceId ?? '').trim()
      if (!resourceId) {
        const src = String(currentNode.attrs.src ?? '')
        if (src) {
          image.setAttribute('src', src)
          sourceState = 'ready'
          updateStatus()
        }
        return
      }
      const cached = retry ? undefined : sources.cached(resourceId)
      if (cached) {
        applySource(cached)
        return
      }
      const generation = ++sourceGeneration
      sourceState = 'loading'
      updateStatus()
      void sources.load(resourceId, retry).then((source) => {
        if (destroyed || generation !== sourceGeneration) return
        applySource(source)
      }).catch(() => {
        if (destroyed || generation !== sourceGeneration) return
        sourceState = image.getAttribute('src') ? 'ready' : 'error'
        updateStatus()
      })
    }

    const stopDragging = () => {
      cleanupDrag?.()
      cleanupDrag = null
    }

    const updateDom = () => {
      const attrs = currentNode.attrs as Record<string, unknown>
      const resourceId = String(attrs.resourceId ?? '').trim()
      const src = String(attrs.src ?? '')
      const identity = resourceId ? `resource:${resourceId}` : `src:${src}`
      if (identity !== sourceIdentity) {
        stopDragging()
        sourceGeneration += 1
        sourceIdentity = identity
        sourceState = 'idle'
        intrinsicSize = undefined
        image.removeAttribute('src')
        if (src) image.setAttribute('src', src)
      }
      if (resourceId) image.dataset.resourceId = resourceId
      else delete image.dataset.resourceId
      if (attrs.nodeId) image.dataset.nodeId = String(attrs.nodeId)
      else delete image.dataset.nodeId
      image.alt = String(attrs.alt ?? '')
      image.title = String(attrs.title ?? '')
      if (!cleanupDrag) renderSavedDimensions()
      loadSource()
      updateStatus()
    }

    const displayScale = () => {
      const root = editor.view.dom
      const scale = root.offsetWidth ? root.getBoundingClientRect().width / root.offsetWidth : 1
      return positiveNumber(scale) ?? 1
    }

    const maximumWidth = () => {
      const widths: number[] = []
      let ancestor = dom.parentElement
      while (ancestor) {
        const width = contentWidth(ancestor)
        if (width > 0) widths.push(width)
        if (ancestor === editor.view.dom) break
        ancestor = ancestor.parentElement
      }
      return widths.length ? Math.min(...widths) : 1600
    }

    const boundedWidth = (width: number) => {
      const maximum = Math.max(1, Math.floor(maximumWidth()))
      return Math.max(1, Math.round(Math.max(Math.min(40, maximum), Math.min(width, maximum))))
    }

    const renderedSize = () => {
      const rectangle = image.getBoundingClientRect()
      const scale = displayScale()
      const width = positiveNumber(rectangle.width / scale)
        ?? positiveNumber(currentNode.attrs.width)
        ?? positiveNumber(intrinsicSize?.width)
        ?? positiveNumber(image.naturalWidth) ?? 320
      const naturalWidth = positiveNumber(image.naturalWidth) ?? positiveNumber(intrinsicSize?.width)
      const naturalHeight = positiveNumber(image.naturalHeight) ?? positiveNumber(intrinsicSize?.height)
      const savedWidth = positiveNumber(currentNode.attrs.width)
      const savedHeight = positiveNumber(currentNode.attrs.height)
      const ratio = rectangle.width > 0 && rectangle.height > 0
        ? rectangle.width / rectangle.height
        : naturalWidth && naturalHeight ? naturalWidth / naturalHeight
          : savedWidth && savedHeight ? savedWidth / savedHeight : null
      return { width: boundedWidth(width), ratio, scale }
    }

    const commitSize = (width: number, ratio: number | null) => {
      const position = getPos()
      if (destroyed || editor.isDestroyed || typeof position !== 'number') return
      const atPosition = editor.state.doc.nodeAt(position)
      if (atPosition !== currentNode) return
      const nextWidth = boundedWidth(width)
      const nextHeight = ratio ? Math.max(1, Math.round(nextWidth / ratio)) : null
      if (Number(currentNode.attrs.width) === nextWidth && currentNode.attrs.height === nextHeight) {
        renderSavedDimensions()
        return
      }
      editor.view.dispatch(editor.state.tr.setNodeMarkup(position, undefined, {
        ...currentNode.attrs, width: nextWidth, height: nextHeight,
      }))
    }

    handle.addEventListener('pointerdown', (event) => {
      if (event.button > 0 || event.isPrimary === false || !image.getAttribute('src') || sourceState === 'error') return
      event.preventDefault()
      event.stopPropagation()
      stopDragging()
      const position = getPos()
      if (typeof position !== 'number') return
      editor.commands.setNodeSelection(position)
      const { width: startWidth, ratio, scale } = renderedSize()
      const startX = event.clientX
      let previewWidth = startWidth
      let moved = false
      const previousDraggable = dom.draggable
      dom.draggable = false
      dom.classList.add('is-resizing')
      const matches = (candidate: PointerEvent) => candidate.pointerId === event.pointerId
      const onMove = (moveEvent: PointerEvent) => {
        if (!matches(moveEvent)) return
        moveEvent.preventDefault()
        moved = true
        previewWidth = boundedWidth(startWidth + (moveEvent.clientX - startX) / scale)
        renderDimensions(previewWidth, ratio ? previewWidth / ratio : null)
      }
      const onUp = (upEvent: PointerEvent) => {
        if (!matches(upEvent)) return
        stopDragging()
        if (moved) commitSize(previewWidth, ratio)
        else renderSavedDimensions()
      }
      const onCancel = () => {
        stopDragging()
        renderSavedDimensions()
      }
      const onPointerCancel = (cancelEvent: PointerEvent) => {
        if (matches(cancelEvent)) onCancel()
      }
      window.addEventListener('pointermove', onMove, { passive: false })
      window.addEventListener('pointerup', onUp)
      window.addEventListener('pointercancel', onPointerCancel)
      window.addEventListener('blur', onCancel)
      handle.addEventListener('lostpointercapture', onCancel)
      cleanupDrag = () => {
        window.removeEventListener('pointermove', onMove)
        window.removeEventListener('pointerup', onUp)
        window.removeEventListener('pointercancel', onPointerCancel)
        window.removeEventListener('blur', onCancel)
        handle.removeEventListener('lostpointercapture', onCancel)
        dom.draggable = previousDraggable
        dom.classList.remove('is-resizing')
        if (handle.hasPointerCapture?.(event.pointerId)) handle.releasePointerCapture(event.pointerId)
      }
      try { handle.setPointerCapture?.(event.pointerId) } catch { /* Window listeners also cover an uncaptured pointer. */ }
    })
    handle.addEventListener('keydown', (event) => {
      if (event.key !== 'ArrowLeft' && event.key !== 'ArrowRight') return
      event.preventDefault()
      event.stopPropagation()
      if (!image.getAttribute('src') || sourceState === 'error') return
      const step = event.shiftKey ? 50 : 10
      const { width, ratio } = renderedSize()
      commitSize(width + (event.key === 'ArrowRight' ? step : -step), ratio)
    })
    image.addEventListener('click', () => {
      const position = getPos()
      if (typeof position === 'number') editor.commands.setNodeSelection(position)
    })
    image.addEventListener('load', () => {
      if (destroyed) return
      sourceState = 'ready'
      updateStatus()
      if (!cleanupDrag) renderSavedDimensions()
    })
    image.addEventListener('error', () => {
      if (destroyed) return
      sourceState = 'error'
      updateStatus()
    })
    retryButton.addEventListener('click', (event) => {
      event.preventDefault()
      event.stopPropagation()
      loadSource(true)
    })

    updateDom()
    return {
      dom,
      update: (updatedNode) => {
        if (updatedNode.type !== currentNode.type) return false
        if (updatedNode.attrs.width !== currentNode.attrs.width || updatedNode.attrs.height !== currentNode.attrs.height) stopDragging()
        currentNode = updatedNode
        updateDom()
        return true
      },
      selectNode: () => dom.classList.add('is-selected'),
      deselectNode: () => dom.classList.remove('is-selected'),
      stopEvent: (event) => event.target instanceof window.Node && (handle.contains(event.target) || status.contains(event.target)),
      ignoreMutation: (mutation) => mutation.type !== 'selection',
      destroy: () => {
        destroyed = true
        sourceGeneration += 1
        stopDragging()
      },
    }
  }
}
