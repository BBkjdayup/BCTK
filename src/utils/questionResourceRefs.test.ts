import { describe, expect, it } from 'vitest'
import type { QuestionResourceRef, RichContent } from '../types/domain'
import { activeQuestionResourceRefs } from './questionResourceRefs'

const rich = (html: string): RichContent => ({ schemaVersion: 1, html, plainText: '' })

describe('activeQuestionResourceRefs', () => {
  const nodeId = '019fa7fc-2c03-7111-a8b6-4166afe943bd'
  const resourceId = '019fa7fc-2c03-7111-a8b6-4166afe943bc'
  const reference: QuestionResourceRef = {
    nodeId,
    resourceId,
    contentSlot: 'stem',
  }

  it('keeps an exact node reference', () => {
    expect(activeQuestionResourceRefs([reference], {
      stem: rich(`<p><img data-resource-id="${resourceId}" data-node-id="${nodeId}"></p>`),
      options: [],
      answer: rich(''),
      explanation: rich(''),
    })).toEqual([reference])
  })

  it('recovers a reference by stable resource id when an older editor stripped node id', () => {
    expect(activeQuestionResourceRefs([reference], {
      stem: rich(`<p><img data-resource-id="${resourceId}" width="458"></p>`),
      options: [],
      answer: rich(''),
      explanation: rich(''),
    })).toEqual([reference])
  })

  it('does not reuse one reference for duplicate image nodes', () => {
    expect(activeQuestionResourceRefs([reference], {
      stem: rich(`<p><img data-resource-id="${resourceId}"><img data-resource-id="${resourceId}"></p>`),
      options: [],
      answer: rich(''),
      explanation: rich(''),
    })).toEqual([reference])
  })
})
