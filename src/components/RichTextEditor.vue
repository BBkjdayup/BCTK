<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, watch } from 'vue'
import { Grid, Operation, Picture } from '@element-plus/icons-vue'
import { ElMessage } from 'element-plus'
import { mergeAttributes, Node } from '@tiptap/core'
import Image from '@tiptap/extension-image'
import Placeholder from '@tiptap/extension-placeholder'
import Subscript from '@tiptap/extension-subscript'
import Superscript from '@tiptap/extension-superscript'
import { TableKit } from '@tiptap/extension-table'
import StarterKit from '@tiptap/starter-kit'
import { EditorContent, useEditor } from '@tiptap/vue-3'
import AppContextMenu from './AppContextMenu.vue'
import { backend } from '../services/backend'
import { errorMessage } from '../services/errors'
import type { RichContent } from '../types/domain'
import type { ContextMenuItem } from '../types/contextMenu'
import {
  richContentToTiptapDocument,
  TIPTAP_CONTENT_EDITOR_VERSION,
} from '../utils/richContent'
import { renderMathInto } from '../utils/mathRendering'
import { embedWpsClipboardImages, wpsClipboardImagePaths } from '../utils/wpsClipboard'

type ToolbarMode = 'full' | 'compact' | 'hidden'
type FormulaTemplate = 'superscript' | 'subscript' | 'fraction' | 'root' | 'plusMinus' | 'multiply' | 'divide'
type EditorSelectionRange = { from: number; to: number }
type ExternalEditorCommand =
  | 'undo'
  | 'redo'
  | 'bold'
  | 'italic'
  | 'underline'
  | 'strike'
  | 'superscript'
  | 'subscript'
  | 'bulletList'
  | 'orderedList'
  | 'clearFormat'
  | 'image'
  | 'table'
  | 'formula'

const model = defineModel<RichContent>({ required: true })
const emit = defineEmits<{
  focus: []
  formatChange: [state: Record<string, boolean>]
}>()

const props = withDefaults(defineProps<{
  placeholder?: string
  minHeight?: number
  toolbarMode?: ToolbarMode
  contextTools?: boolean
}>(), {
  placeholder: '请输入内容',
  minHeight: 120,
  toolbarMode: 'full',
  contextTools: false,
})

const editorHost = ref<HTMLElement | null>(null)
const fileInput = ref<HTMLInputElement | null>(null)
const boldActive = ref(false)
const italicActive = ref(false)
const tableDialogOpen = ref(false)
const tableRows = ref(2)
const tableColumns = ref(2)
const formulaDialogOpen = ref(false)
const formulaInput = ref('')
const formulaEditing = ref(false)
const formulaPreviewHost = ref<HTMLElement | null>(null)
const contextToolsOpen = ref(false)
const contextToolsX = ref(0)
const contextToolsY = ref(0)
let updatingFromModel = false
let imageHydrationGeneration = 0
let contextSelection: EditorSelectionRange | null = null
let pendingImageSelection: EditorSelectionRange | null = null
let pendingTableSelection: EditorSelectionRange | null = null
let pendingFormulaSelection: EditorSelectionRange | null = null
const managedImageCache = new Map<string, string>()

const ManagedImage = Image.extend({
  addAttributes() {
    return {
      ...this.parent?.(),
      src: { default: null },
      resourceId: {
        default: null,
        parseHTML: (element) => element.getAttribute('data-resource-id'),
        renderHTML: (attributes) => attributes.resourceId
          ? { 'data-resource-id': attributes.resourceId }
          : {},
      },
      nodeId: {
        default: null,
        parseHTML: (element) => element.getAttribute('data-node-id'),
        renderHTML: (attributes) => attributes.nodeId
          ? { 'data-node-id': attributes.nodeId }
          : {},
      },
      width: {
        default: null,
        parseHTML: (element) => element.getAttribute('width'),
        renderHTML: (attributes) => attributes.width ? { width: attributes.width } : {},
      },
      height: {
        default: null,
        parseHTML: (element) => element.getAttribute('height'),
        renderHTML: (attributes) => attributes.height ? { height: attributes.height } : {},
      },
    }
  },
  parseHTML() {
    return [{ tag: 'img[src]' }, { tag: 'img[data-resource-id]' }]
  },
  renderHTML({ HTMLAttributes }) {
    const attributes = mergeAttributes(this.options.HTMLAttributes, HTMLAttributes)
    if (!attributes.src) delete attributes.src
    if (!attributes['data-resource-id']) delete attributes['data-resource-id']
    if (!attributes['data-node-id']) delete attributes['data-node-id']
    return ['img', attributes]
  },
  addNodeView() {
    return ({ node, editor, getPos }) => {
      let currentNode = node
      let cleanupDrag: (() => void) | null = null
      const dom = document.createElement('span')
      const image = document.createElement('img')
      const handle = document.createElement('span')
      const sizeLabel = document.createElement('span')
      dom.className = 'rich-editor-image-node'
      dom.contentEditable = 'false'
      handle.className = 'rich-editor-image-node__handle'
      handle.setAttribute('role', 'button')
      handle.setAttribute('aria-label', '拖动调整图片大小')
      handle.setAttribute('title', '拖动调整图片大小')
      handle.tabIndex = 0
      sizeLabel.className = 'rich-editor-image-node__size'
      dom.append(image, handle, sizeLabel)

      const renderedWidth = () => {
        const attributeWidth = Number(currentNode.attrs.width)
        if (Number.isFinite(attributeWidth) && attributeWidth > 0) return attributeWidth
        const rectangleWidth = image.getBoundingClientRect().width
        if (rectangleWidth > 0) return rectangleWidth
        return image.naturalWidth || 320
      }

      const updateDom = () => {
        const attributes = currentNode.attrs as Record<string, unknown>
        const src = typeof attributes.src === 'string' ? attributes.src : ''
        const resourceId = typeof attributes.resourceId === 'string' ? attributes.resourceId : ''
        const nodeId = typeof attributes.nodeId === 'string' ? attributes.nodeId : ''
        const alt = typeof attributes.alt === 'string' ? attributes.alt : ''
        const title = typeof attributes.title === 'string' ? attributes.title : ''
        const width = Number(attributes.width)
        const height = Number(attributes.height)
        if (src) image.src = src
        else image.removeAttribute('src')
        if (resourceId) image.dataset.resourceId = resourceId
        else delete image.dataset.resourceId
        if (nodeId) image.dataset.nodeId = nodeId
        else delete image.dataset.nodeId
        image.alt = alt
        image.title = title
        if (Number.isFinite(width) && width > 0) image.setAttribute('width', String(Math.round(width)))
        else image.removeAttribute('width')
        if (Number.isFinite(height) && height > 0) image.setAttribute('height', String(Math.round(height)))
        else image.removeAttribute('height')
        sizeLabel.textContent = Number.isFinite(width) && width > 0 ? `${Math.round(width)} px` : ''
      }

      const commitWidth = (width: number) => {
        const position = getPos()
        if (typeof position !== 'number') return
        const boundedWidth = Math.max(
          40,
          Math.min(Math.round(width), editorHost.value?.clientWidth || 1600),
        )
        editor.view.dispatch(editor.view.state.tr.setNodeMarkup(
          position,
          undefined,
          { ...currentNode.attrs, width: boundedWidth, height: null },
        ))
      }

      const stopDragging = () => {
        cleanupDrag?.()
        cleanupDrag = null
      }

      handle.addEventListener('pointerdown', (event) => {
        event.preventDefault()
        event.stopPropagation()
        stopDragging()
        const position = getPos()
        if (typeof position === 'number') editor.commands.setNodeSelection(position)
        const startX = event.clientX
        const startWidth = renderedWidth()
        let previewWidth = startWidth
        const onMove = (moveEvent: PointerEvent) => {
          previewWidth = Math.max(
            40,
            Math.min(startWidth + moveEvent.clientX - startX, editorHost.value?.clientWidth || 1600),
          )
          image.setAttribute('width', String(Math.round(previewWidth)))
          image.removeAttribute('height')
          sizeLabel.textContent = `${Math.round(previewWidth)} px`
        }
        const onUp = () => {
          stopDragging()
          commitWidth(previewWidth)
        }
        window.addEventListener('pointermove', onMove)
        window.addEventListener('pointerup', onUp, { once: true })
        window.addEventListener('pointercancel', onUp, { once: true })
        cleanupDrag = () => {
          window.removeEventListener('pointermove', onMove)
          window.removeEventListener('pointerup', onUp)
          window.removeEventListener('pointercancel', onUp)
        }
      })
      handle.addEventListener('keydown', (event) => {
        if (event.key !== 'ArrowLeft' && event.key !== 'ArrowRight') return
        event.preventDefault()
        const step = event.shiftKey ? 50 : 10
        commitWidth(renderedWidth() + (event.key === 'ArrowRight' ? step : -step))
      })
      image.addEventListener('click', () => {
        const position = getPos()
        if (typeof position === 'number') editor.commands.setNodeSelection(position)
      })

      updateDom()
      return {
        dom,
        update: (updatedNode) => {
          if (updatedNode.type.name !== this.name) return false
          currentNode = updatedNode
          updateDom()
          return true
        },
        selectNode: () => dom.classList.add('is-selected'),
        deselectNode: () => dom.classList.remove('is-selected'),
        stopEvent: (event) => event.target === handle,
        ignoreMutation: (mutation) => mutation.type === 'attributes' && mutation.target === image,
        destroy: stopDragging,
      }
    }
  },
}).configure({ allowBase64: true, inline: true })

const MathNode = Node.create({
  name: 'mathNode',
  group: 'inline',
  inline: true,
  atom: true,
  selectable: true,
  addAttributes() {
    return {
      latex: {
        default: '',
        parseHTML: (element) => element.getAttribute('data-latex') ?? '',
      },
    }
  },
  parseHTML() {
    return [{ tag: 'span.math-node[data-latex]' }]
  },
  renderHTML({ node }) {
    return ['span', {
      class: 'math-node',
      'data-latex': node.attrs.latex,
      title: node.attrs.latex,
    }, node.attrs.latex]
  },
  addNodeView() {
    return ({ node }) => {
      const dom = document.createElement('span')
      dom.className = 'math-node'
      dom.dataset.latex = String(node.attrs.latex ?? '')
      renderMathInto(dom, dom.dataset.latex)
      return {
        dom,
        update: (updatedNode) => {
          if (updatedNode.type.name !== this.name) return false
          const latex = String(updatedNode.attrs.latex ?? '')
          dom.dataset.latex = latex
          renderMathInto(dom, latex)
          return true
        },
        selectNode: () => dom.classList.add('is-selected'),
        deselectNode: () => dom.classList.remove('is-selected'),
      }
    }
  },
})

watch([formulaInput, formulaDialogOpen], async ([latex, open]) => {
  if (!open) return
  await nextTick()
  if (formulaPreviewHost.value) renderMathInto(formulaPreviewHost.value, latex)
}, { immediate: true })

function updateFormatState() {
  boldActive.value = tiptap.value?.isActive('bold') ?? false
  italicActive.value = tiptap.value?.isActive('italic') ?? false
  emit('formatChange', {
    undo: tiptap.value?.can().undo() ?? false,
    redo: tiptap.value?.can().redo() ?? false,
    bold: boldActive.value,
    italic: italicActive.value,
    underline: tiptap.value?.isActive('underline') ?? false,
    strike: tiptap.value?.isActive('strike') ?? false,
    superscript: tiptap.value?.isActive('superscript') ?? false,
    subscript: tiptap.value?.isActive('subscript') ?? false,
    bulletList: tiptap.value?.isActive('bulletList') ?? false,
    orderedList: tiptap.value?.isActive('orderedList') ?? false,
  })
}

const contextToolItems = computed<ContextMenuItem[]>(() => [
  {
    id: 'bold',
    label: boldActive.value ? '✓ 加粗' : '加粗',
    hint: 'Ctrl+B',
  },
  {
    id: 'bulletList',
    label: tiptap.value?.isActive('bulletList') ? '✓ 项目符号' : '项目符号',
  },
  {
    id: 'clearFormat',
    label: '清除格式',
  },
  {
    id: 'image',
    label: '插入图片',
    dividerBefore: true,
  },
  {
    id: 'table',
    label: '插入表格',
  },
  {
    id: 'formula',
    label: '插入公式',
  },
])

function showContextTools(x: number, y: number) {
  const selection = tiptap.value?.state.selection
  contextSelection = selection ? { from: selection.from, to: selection.to } : null
  updateFormatState()
  contextToolsX.value = x
  contextToolsY.value = y
  contextToolsOpen.value = true
}

function openContextToolsFromButton(event: MouseEvent) {
  if (!props.contextTools) return
  const button = event.currentTarget as HTMLElement
  const rect = button.getBoundingClientRect()
  showContextTools(rect.right, rect.bottom + 4)
}

function onEditorContextMenu(event: MouseEvent) {
  if (!props.contextTools || !event.ctrlKey) return
  event.preventDefault()
  event.stopPropagation()
  showContextTools(event.clientX, event.clientY)
}

function selectContextTool(item: ContextMenuItem) {
  execute(item.id as ExternalEditorCommand, contextSelection)
  updateFormatState()
}

function syncModel() {
  const instance = tiptap.value
  if (!instance || updatingFromModel) return
  const document = instance.getJSON()
  const html = instance.isEmpty ? '' : instance.getHTML()
  model.value = {
    ...model.value,
    schemaVersion: 2,
    editor: 'tiptap',
    editorVersion: TIPTAP_CONTENT_EDITOR_VERSION,
    document: document as Record<string, unknown>,
    html,
    plainText: instance.getText().trim(),
  }
  void hydrateManagedImages()
}

const tiptap = useEditor({
  content: richContentToTiptapDocument(model.value),
  extensions: [
    StarterKit,
    ManagedImage,
    MathNode,
    TableKit.configure({ table: { resizable: true } }),
    Placeholder.configure({ placeholder: () => props.placeholder }),
    Superscript,
    Subscript,
  ],
  editorProps: {
    attributes: {
      role: 'textbox',
      'aria-multiline': 'true',
      class: 'tiptap',
    },
  },
  onCreate: () => { void hydrateManagedImages() },
  onUpdate: syncModel,
  onSelectionUpdate: updateFormatState,
  onTransaction: updateFormatState,
})

async function hydrateManagedImages() {
  await nextTick()
  const generation = ++imageHydrationGeneration
  const images = [...(editorHost.value?.querySelectorAll<HTMLImageElement>('img[data-resource-id]') ?? [])]
  await Promise.all(images.map(async (image) => {
    const resourceId = image.dataset.resourceId?.trim()
    if (!resourceId) return
    try {
      let dataUrl = managedImageCache.get(resourceId)
      if (!dataUrl) {
        const payload = await backend.getManagedImage(resourceId)
        dataUrl = `data:${payload.mimeType};base64,${payload.dataBase64}`
        managedImageCache.set(resourceId, dataUrl)
      }
      if (generation !== imageHydrationGeneration || !editorHost.value?.contains(image)) return
      image.src = dataUrl
      image.removeAttribute('data-resource-load-error')
    } catch {
      if (generation !== imageHydrationGeneration || !editorHost.value?.contains(image)) return
      image.removeAttribute('src')
      image.dataset.resourceLoadError = 'true'
      if (!image.alt) image.alt = '图片资源暂时无法读取'
    }
  }))
}

function run(command: () => boolean) {
  command()
  updateFormatState()
}

function focusedChain(instance: NonNullable<typeof tiptap.value>, selection?: EditorSelectionRange | null) {
  let chain = instance.chain().focus()
  if (!selection) return chain
  const maximum = instance.state.doc.content.size
  const from = Math.max(0, Math.min(selection.from, maximum))
  const to = Math.max(from, Math.min(selection.to, maximum))
  chain = chain.setTextSelection({ from, to })
  return chain
}

function execute(command: ExternalEditorCommand, selection?: EditorSelectionRange | null) {
  const instance = tiptap.value
  if (!instance) return false
  if (command === 'image') {
    chooseImage(selection)
    return true
  }
  if (command === 'table') {
    openTableDialog(selection)
    return true
  }
  if (command === 'formula') {
    openFormulaDialog(selection)
    return true
  }
  const chain = focusedChain(instance, selection)
  if (command === 'undo') return chain.undo().run()
  if (command === 'redo') return chain.redo().run()
  if (command === 'bold') return chain.toggleBold().run()
  if (command === 'italic') return chain.toggleItalic().run()
  if (command === 'underline') return chain.toggleUnderline().run()
  if (command === 'strike') return chain.toggleStrike().run()
  if (command === 'superscript') return chain.toggleSuperscript().run()
  if (command === 'subscript') return chain.toggleSubscript().run()
  if (command === 'bulletList') return chain.toggleBulletList().run()
  if (command === 'orderedList') return chain.toggleOrderedList().run()
  if (command === 'clearFormat') return chain.unsetAllMarks().clearNodes().run()
  return false
}

function openTableDialog(selection?: EditorSelectionRange | null) {
  pendingTableSelection = selection ?? null
  tableRows.value = 2
  tableColumns.value = 2
  tableDialogOpen.value = true
}

function insertTable() {
  const rows = Math.max(1, Math.min(10, Number(tableRows.value) || 1))
  const cols = Math.max(1, Math.min(10, Number(tableColumns.value) || 1))
  tableDialogOpen.value = false
  const selection = pendingTableSelection
  pendingTableSelection = null
  run(() => {
    const instance = tiptap.value
    return instance
      ? focusedChain(instance, selection).insertTable({ rows, cols, withHeaderRow: false }).run()
      : false
  })
}

function openFormulaDialog(selection?: EditorSelectionRange | null) {
  const instance = tiptap.value
  pendingFormulaSelection = selection ?? null
  if (instance && selection) focusedChain(instance, selection).run()
  formulaEditing.value = instance?.isActive('mathNode') ?? false
  formulaInput.value = formulaEditing.value
    ? String(instance?.getAttributes('mathNode').latex ?? '')
    : ''
  formulaDialogOpen.value = true
}

function applyFormulaTemplate(template: FormulaTemplate) {
  const current = formulaInput.value.trim()
  if (template === 'superscript') formulaInput.value = current ? `${current}^{2}` : 'x^{2}'
  if (template === 'subscript') formulaInput.value = current ? `${current}_{1}` : 'x_{1}'
  if (template === 'fraction') formulaInput.value = current ? `\\frac{${current}}{b}` : '\\frac{a}{b}'
  if (template === 'root') formulaInput.value = current ? `\\sqrt{${current}}` : '\\sqrt{x}'
  if (template === 'plusMinus') formulaInput.value = `${current}\\pm`
  if (template === 'multiply') formulaInput.value = `${current}\\times`
  if (template === 'divide') formulaInput.value = `${current}\\div`
}

function insertFormula() {
  const latex = formulaInput.value.trim()
  if (!latex) return
  formulaDialogOpen.value = false
  const selection = pendingFormulaSelection
  pendingFormulaSelection = null
  if (formulaEditing.value) {
    formulaEditing.value = false
    run(() => {
      const instance = tiptap.value
      return instance
        ? focusedChain(instance, selection).updateAttributes('mathNode', { latex }).run()
        : false
    })
    return
  }
  run(() => {
    const instance = tiptap.value
    return instance
      ? focusedChain(instance, selection).insertContent([
          { type: 'mathNode', attrs: { latex } },
          { type: 'text', text: ' ' },
        ]).run()
      : false
  })
}

function chooseImage(selection?: EditorSelectionRange | null) {
  pendingImageSelection = selection ?? null
  fileInput.value?.click()
}

function fileDataBase64(file: File) {
  return new Promise<string>((resolve, reject) => {
    const reader = new FileReader()
    reader.onerror = () => reject(reader.error ?? new Error('图片读取失败'))
    reader.onload = () => {
      const result = String(reader.result ?? '')
      const comma = result.indexOf(',')
      if (comma < 0) reject(new Error('图片数据格式无效'))
      else resolve(result.slice(comma + 1))
    }
    reader.readAsDataURL(file)
  })
}

function insertManagedImage(
  payload: Awaited<ReturnType<typeof backend.storeManagedImages>>[number],
  alt: string,
  selection?: EditorSelectionRange | null,
) {
  managedImageCache.set(
    payload.resourceId,
    `data:${payload.mimeType};base64,${payload.dataBase64}`,
  )
  const nodeId = crypto.randomUUID()
  run(() => {
    const instance = tiptap.value
    return instance
      ? focusedChain(instance, selection).insertContent({
          type: 'image',
          attrs: {
            src: null,
            alt,
            resourceId: payload.resourceId,
            nodeId,
            width: payload.widthPx ?? null,
            height: payload.heightPx ?? null,
          },
        }).run()
      : false
  })
}

async function insertImageFile(file: File, selection?: EditorSelectionRange | null) {
  try {
    const dataBase64 = await fileDataBase64(file)
    const [payload] = await backend.storeManagedImages([{
      dataBase64,
      originalFilename: file.name || null,
    }])
    if (!payload) throw new Error('图片保存后没有返回资源信息')
    insertManagedImage(payload, file.name || '插入图片', selection)
  } catch (reason) {
    ElMessage.error(errorMessage(reason, '图片保存失败，请确认图片为有效的 PNG 或 JPEG。'))
  }
}

function onImageSelected(event: Event) {
  const input = event.target as HTMLInputElement
  const file = input.files?.[0]
  const selection = pendingImageSelection
  pendingImageSelection = null
  if (file) void insertImageFile(file, selection)
  input.value = ''
}

async function onPaste(event: ClipboardEvent) {
  const html = event.clipboardData?.getData('text/html') ?? ''
  const wpsPaths = html ? wpsClipboardImagePaths(html) : []
  if (wpsPaths.length) {
    const instance = tiptap.value
    if (!instance) return
    event.preventDefault()
    const range = {
      from: instance.state.selection.from,
      to: instance.state.selection.to,
    }
    const plainText = event.clipboardData?.getData('text/plain') ?? ''
    try {
      const payloads = await backend.readWpsClipboardImages(wpsPaths)
      const stored = await backend.storeManagedImages(payloads.map((payload) => ({
        dataBase64: payload.dataBase64,
        originalFilename: payload.path.split(/[\\/]/u).at(-1) ?? null,
      })))
      const managedPayloads = stored.map((payload, index) => ({
        ...payload,
        path: payloads[index]!.path,
      }))
      for (const payload of managedPayloads) {
        managedImageCache.set(
          payload.resourceId,
          `data:${payload.mimeType};base64,${payload.dataBase64}`,
        )
      }
      const embeddedHtml = embedWpsClipboardImages(html, managedPayloads)
      run(() => instance.chain().focus().insertContentAt(range, embeddedHtml).run())
    } catch (reason) {
      if (plainText) {
        run(() => instance.chain().focus().insertContentAt(range, plainText).run())
      }
      ElMessage.error(errorMessage(
        reason,
        'WPS 图片保存失败，请在 WPS 中重新复制后立即粘贴。',
      ))
    }
    return
  }
  const imageItem = [...(event.clipboardData?.items ?? [])]
    .find((item) => item.kind === 'file' && item.type.startsWith('image/'))
  const imageFile = imageItem?.getAsFile()
  if (!imageFile) return
  event.preventDefault()
  const range = tiptap.value
    ? { from: tiptap.value.state.selection.from, to: tiptap.value.state.selection.to }
    : null
  void insertImageFile(imageFile, range)
}

watch(
  () => props.placeholder,
  () => {
    const instance = tiptap.value
    if (!instance) return
    instance.view.dispatch(instance.state.tr)
  },
)

watch(
  () => model.value,
  (content) => {
    const instance = tiptap.value
    if (!instance) return
    const desired = richContentToTiptapDocument(content)
    const desiredDocument = typeof desired === 'string' ? null : JSON.stringify(desired)
    if (desiredDocument && desiredDocument === JSON.stringify(instance.getJSON())) return
    updatingFromModel = true
    instance.commands.setContent(desired, { emitUpdate: false })
    updatingFromModel = false
    updateFormatState()
    void hydrateManagedImages()
  },
  { deep: true },
)

onBeforeUnmount(() => {
  imageHydrationGeneration += 1
})

defineExpose({
  execute,
  focus: () => tiptap.value?.commands.focus(),
})
</script>

<template>
  <div
    class="rich-editor"
    :class="{ 'has-context-tools': props.toolbarMode === 'hidden' && props.contextTools }"
  >
    <div
      v-if="props.toolbarMode !== 'hidden'"
      class="rich-editor__toolbar"
      :class="{ 'is-compact': props.toolbarMode === 'compact' }"
    >
      <el-tooltip content="加粗" placement="top">
        <button
          type="button"
          aria-label="加粗"
          :aria-pressed="boldActive"
          :class="{ 'is-active': boldActive }"
          @mousedown.prevent
          @click="run(() => tiptap?.chain().focus().toggleBold().run() ?? false)"
        ><strong>B</strong></button>
      </el-tooltip>

      <template v-if="props.toolbarMode === 'full'">
        <el-tooltip content="斜体" placement="top">
          <button
            type="button"
            aria-label="斜体"
            :aria-pressed="italicActive"
            :class="{ 'is-active': italicActive }"
            @mousedown.prevent
            @click="run(() => tiptap?.chain().focus().toggleItalic().run() ?? false)"
          ><em>I</em></button>
        </el-tooltip>
        <el-tooltip content="项目符号列表" placement="top">
          <button
            type="button"
            aria-label="项目符号列表"
            @mousedown.prevent
            @click="run(() => tiptap?.chain().focus().toggleBulletList().run() ?? false)"
          ><span class="bullet-list-icon" aria-hidden="true"><i /><i /><i /></span></button>
        </el-tooltip>
      </template>

      <span class="rich-editor__divider" />

      <el-tooltip content="插入图片" placement="top">
        <button type="button" aria-label="插入图片" @mousedown.prevent @click="chooseImage()">
          <el-icon><Picture /></el-icon>
        </button>
      </el-tooltip>
      <el-tooltip v-if="props.toolbarMode === 'full'" content="插入表格" placement="top">
        <button type="button" aria-label="插入表格" @mousedown.prevent @click="openTableDialog()">
          <el-icon><Grid /></el-icon>
        </button>
      </el-tooltip>
      <el-tooltip content="插入公式" placement="top">
        <button type="button" aria-label="插入公式" @mousedown.prevent @click="openFormulaDialog()">
          <el-icon><Operation /></el-icon>
        </button>
      </el-tooltip>
    </div>

    <input ref="fileInput" class="rich-editor__file" type="file" accept="image/png,image/jpeg" @change="onImageSelected">

    <div
      ref="editorHost"
      class="rich-editor__content"
      :style="{ minHeight: `${props.minHeight}px` }"
      @paste.capture="onPaste"
      @focusin="emit('focus')"
      @contextmenu="onEditorContextMenu"
    >
      <EditorContent :editor="tiptap" />
    </div>

    <button
      v-if="props.toolbarMode === 'hidden' && props.contextTools"
      type="button"
      class="rich-editor__more"
      :class="{ 'is-open': contextToolsOpen }"
      aria-label="更多编辑工具"
      title="更多编辑工具（Ctrl + 右键）"
      @mousedown.prevent
      @click="openContextToolsFromButton"
    >⋯</button>
  </div>

  <AppContextMenu
    v-if="props.contextTools"
    v-model="contextToolsOpen"
    :x="contextToolsX"
    :y="contextToolsY"
    :items="contextToolItems"
    @select="selectContextTool"
  />

  <el-dialog v-model="tableDialogOpen" title="插入表格" width="420px" append-to-body :close-on-click-modal="false">
    <div class="table-settings">
      <label><span>行数</span><el-input-number v-model="tableRows" :min="1" :max="10" controls-position="right" /></label>
      <label><span>列数</span><el-input-number v-model="tableColumns" :min="1" :max="10" controls-position="right" /></label>
    </div>
    <p class="dialog-hint">最多可插入 10 行 × 10 列，插入后可直接在单元格中输入内容。</p>
    <template #footer>
      <el-button @click="tableDialogOpen = false">取消</el-button>
      <el-button type="primary" @click="insertTable">插入表格</el-button>
    </template>
  </el-dialog>

  <el-dialog
    v-model="formulaDialogOpen"
    :title="formulaEditing ? '编辑公式' : '插入公式'"
    width="520px"
    append-to-body
    :close-on-click-modal="false"
  >
    <div class="formula-dialog">
      <label for="formula-input">公式内容</label>
      <el-input
        id="formula-input"
        v-model="formulaInput"
        placeholder="例如：x^{2}、\frac{a}{b}、\sqrt{x}"
        clearable
        @keyup.enter="insertFormula"
      />
      <div class="formula-templates" aria-label="常用公式模板">
        <el-button size="small" @click="applyFormulaTemplate('superscript')">上标</el-button>
        <el-button size="small" @click="applyFormulaTemplate('subscript')">下标</el-button>
        <el-button size="small" @click="applyFormulaTemplate('fraction')">分数</el-button>
        <el-button size="small" @click="applyFormulaTemplate('root')">根号</el-button>
        <el-button size="small" @click="applyFormulaTemplate('plusMinus')">±</el-button>
        <el-button size="small" @click="applyFormulaTemplate('multiply')">×</el-button>
        <el-button size="small" @click="applyFormulaTemplate('divide')">÷</el-button>
      </div>
      <div class="formula-preview">
        <span>预览</span>
        <strong
          v-if="formulaInput.trim()"
          ref="formulaPreviewHost"
          class="math-node"
          :data-latex="formulaInput.trim()"
        />
        <em v-else>输入公式后可在这里预览</em>
      </div>
      <p class="dialog-hint">原始公式与 Tiptap JSON 会随题目保存；纯文本预览继续供搜索和 DOCX 导出使用。</p>
    </div>
    <template #footer>
      <el-button @click="formulaDialogOpen = false">取消</el-button>
      <el-button type="primary" :disabled="!formulaInput.trim()" @click="insertFormula">
        {{ formulaEditing ? '保存修改' : '插入公式' }}
      </el-button>
    </template>
  </el-dialog>
</template>

<style scoped>
.rich-editor {
  position: relative;
  overflow: hidden;
  border: 1px solid #dbe3ee;
  border-radius: 8px;
  background: #fff;
  transition: border-color .2s, box-shadow .2s;
}

.rich-editor.has-context-tools .rich-editor__content {
  padding-right: 48px;
}

.rich-editor__more {
  position: absolute;
  z-index: 3;
  top: 6px;
  right: 6px;
  width: 28px;
  height: 28px;
  display: grid;
  place-items: center;
  border: 1px solid #dbe3ee;
  border-radius: 6px;
  background: rgb(255 255 255 / 96%);
  box-shadow: 0 2px 7px rgb(15 23 42 / 8%);
  color: #64748b;
  cursor: pointer;
  font-size: 18px;
  line-height: 1;
  opacity: 0;
  pointer-events: none;
  transition: opacity .16s, color .16s, border-color .16s, background .16s;
}

.rich-editor:hover .rich-editor__more,
.rich-editor:focus-within .rich-editor__more,
.rich-editor__more.is-open {
  opacity: 1;
  pointer-events: auto;
}

.rich-editor__more:hover,
.rich-editor__more:focus-visible,
.rich-editor__more.is-open {
  border-color: #93c5fd;
  outline: none;
  background: #eff6ff;
  color: #2563eb;
}

.rich-editor:focus-within {
  border-color: #60a5fa;
  box-shadow: 0 0 0 2px rgb(59 130 246 / 10%);
}

.rich-editor__toolbar {
  min-height: 38px;
  padding: 0 8px;
  display: flex;
  align-items: center;
  gap: 2px;
  border-bottom: 1px solid #edf1f5;
  background: #fbfcfe;
}

.rich-editor__toolbar.is-compact { min-height: 36px; }

.rich-editor__toolbar button {
  width: 30px;
  height: 28px;
  display: grid;
  place-items: center;
  border: 0;
  border-radius: 5px;
  background: transparent;
  color: #64748b;
  cursor: pointer;
}

.rich-editor__toolbar button:hover,
.rich-editor__toolbar button.is-active { background: #e8f1ff; color: #2563eb; }
.rich-editor__toolbar button.is-active { box-shadow: inset 0 0 0 1px #bfdbfe; }
.rich-editor__divider { width: 1px; height: 18px; margin: 0 5px; background: #e2e8f0; }
.rich-editor__file { display: none; }

.bullet-list-icon { width: 17px; display: grid; gap: 3px; }
.bullet-list-icon i { position: relative; height: 2px; margin-left: 6px; border-radius: 2px; background: currentColor; }
.bullet-list-icon i::before { position: absolute; top: 0; left: -6px; width: 2px; height: 2px; border-radius: 50%; background: currentColor; content: ''; }

.rich-editor__content { padding: 12px 14px; color: #1f2937; font-size: 13px; line-height: 1.75; }
.rich-editor__content :deep(.tiptap) { min-height: inherit; outline: 0; }
.rich-editor__content :deep(.tiptap p) { margin: 0 0 8px; }
.rich-editor__content :deep(.tiptap p:last-child) { margin-bottom: 0; }
.rich-editor__content :deep(.tiptap p.is-editor-empty:first-child::before) { float: left; height: 0; color: #a8b1be; content: attr(data-placeholder); pointer-events: none; }
.rich-editor__content :deep(.tiptap img) { max-width: 100%; height: auto; }
.rich-editor__content :deep(.rich-editor-image-node) {
  position: relative;
  display: inline-block;
  max-width: 100%;
  line-height: 0;
  vertical-align: middle;
}
.rich-editor__content :deep(.rich-editor-image-node.is-selected) {
  outline: 2px solid #3b82f6;
  outline-offset: 2px;
}
.rich-editor__content :deep(.rich-editor-image-node__handle) {
  position: absolute;
  right: -6px;
  bottom: -6px;
  width: 12px;
  height: 12px;
  display: none;
  box-sizing: border-box;
  border: 2px solid #fff;
  border-radius: 3px;
  background: #2563eb;
  box-shadow: 0 0 0 1px #2563eb;
  cursor: nwse-resize;
}
.rich-editor__content :deep(.rich-editor-image-node__size) {
  position: absolute;
  right: 0;
  bottom: 10px;
  display: none;
  padding: 2px 5px;
  border-radius: 4px;
  background: rgb(15 23 42 / 82%);
  color: #fff;
  font-size: 10px;
  line-height: 1.4;
  white-space: nowrap;
}
.rich-editor__content :deep(.rich-editor-image-node.is-selected .rich-editor-image-node__handle),
.rich-editor__content :deep(.rich-editor-image-node.is-selected .rich-editor-image-node__size) {
  display: block;
}
.rich-editor__content :deep(.tiptap table) { width: 100%; border-collapse: collapse; table-layout: fixed; }
.rich-editor__content :deep(.tiptap td),
.rich-editor__content :deep(.tiptap th) { min-width: 40px; padding: 5px 7px; border: 1px solid #cbd5e1; vertical-align: top; }
.rich-editor__content :deep(.math-node) {
  display: inline-block;
  min-width: 2px;
  padding: 0 1px;
  border-radius: 3px;
  color: inherit;
  line-height: 1;
  vertical-align: -0.12em;
}
.rich-editor__content :deep(.math-node.is-selected) {
  background: #eff6ff;
  outline: 1px solid #93c5fd;
}

.table-settings { display: grid; grid-template-columns: 1fr 1fr; gap: 16px; }
.table-settings label { display: grid; gap: 8px; color: #475569; font-size: 13px; }
.dialog-hint { margin: 16px 0 0; color: #7c8797; font-size: 12px; line-height: 1.6; }
.formula-dialog { display: grid; gap: 12px; }
.formula-dialog > label { color: #475569; font-size: 13px; font-weight: 600; }
.formula-templates { display: flex; flex-wrap: wrap; gap: 6px; }
.formula-preview { min-height: 64px; padding: 10px 14px; display: grid; gap: 6px; border: 1px solid #dbeafe; border-radius: 8px; background: #f8fbff; }
.formula-preview span { color: #64748b; font-size: 12px; }
.formula-preview strong { color: #1e3a8a; font-family: Cambria Math, STIX Two Math, serif; font-size: 20px; font-weight: 500; }
.formula-preview em { color: #94a3b8; font-size: 13px; font-style: normal; }
</style>
