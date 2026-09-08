import type { QuestionResourceRef, RichContent } from '../types/domain'

interface OptionResourceContent {
  id: string
  content: RichContent
}

export interface QuestionResourceContents {
  stem: RichContent
  options: OptionResourceContent[]
  answer: RichContent
  explanation: RichContent
}

interface ResourceLocation {
  contentSlot: QuestionResourceRef['contentSlot']
  optionId?: string
  html: string
}

function managedImages(html: string) {
  const template = document.createElement('template')
  template.innerHTML = html
  return [...template.content.querySelectorAll<HTMLImageElement>('img[data-resource-id]')]
}

/**
 * Keeps references for managed images that are still present in the rich HTML.
 * Older editor builds could strip data-node-id while resizing an image, so the
 * stable resource id is a constrained fallback within the same content slot.
 */
export function activeQuestionResourceRefs(
  refs: QuestionResourceRef[],
  contents: QuestionResourceContents,
) {
  const locations: ResourceLocation[] = [
    { contentSlot: 'stem', html: contents.stem.html },
    ...contents.options.map((option) => ({
      contentSlot: 'option' as const,
      optionId: option.id,
      html: option.content.html,
    })),
    { contentSlot: 'answer', html: contents.answer.html },
    { contentSlot: 'explanation', html: contents.explanation.html },
  ]
  const active: QuestionResourceRef[] = []
  const consumed = new Set<number>()

  for (const location of locations) {
    for (const image of managedImages(location.html)) {
      const resourceId = image.dataset.resourceId?.trim()
      if (!resourceId) continue
      const nodeId = image.dataset.nodeId?.trim()
      const matchesLocation = (reference: QuestionResourceRef) => (
        reference.contentSlot === location.contentSlot
        && (reference.optionId ?? undefined) === location.optionId
        && reference.resourceId === resourceId
      )
      let index = nodeId
        ? refs.findIndex((reference, candidate) => (
          !consumed.has(candidate)
          && reference.nodeId === nodeId
          && matchesLocation(reference)
        ))
        : -1
      if (index < 0) {
        index = refs.findIndex((reference, candidate) => (
          !consumed.has(candidate) && matchesLocation(reference)
        ))
      }
      if (index < 0) continue
      consumed.add(index)
      active.push(refs[index]!)
    }
  }

  return active
}
