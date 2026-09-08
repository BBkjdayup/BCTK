import { describe, expect, it } from 'vitest'
import {
  canPersistWordImportReview,
  expectedWordImportReviewParserVersion,
  isDocumentEntryReviewDraft,
} from './wordImportReviewAvailability'

describe('canPersistWordImportReview', () => {
  it('allows every review flow in the desktop app', () => {
    expect(canPersistWordImportReview(true, false)).toBe(true)
    expect(canPersistWordImportReview(true, true)).toBe(true)
  })

  it('allows document-entry review to write browser demonstration data', () => {
    expect(canPersistWordImportReview(false, true)).toBe(true)
  })

  it('keeps real Word import unavailable in the browser demonstration', () => {
    expect(canPersistWordImportReview(false, false)).toBe(false)
  })
})

describe('expectedWordImportReviewParserVersion', () => {
  it('uses the document-entry parser for document-entry drafts', () => {
    expect(expectedWordImportReviewParserVersion(true, 'word-v5', 'document-v3')).toBe('document-v3')
  })

  it('uses the Word parser for regular Word-import drafts', () => {
    expect(expectedWordImportReviewParserVersion(false, 'word-v5', 'document-v3')).toBe('word-v5')
  })
})

describe('isDocumentEntryReviewDraft', () => {
  it('recognizes the draft by its stored source instead of the current route', () => {
    expect(isDocumentEntryReviewDraft('文档式批量录入.docx', '文档式批量录入.docx')).toBe(true)
  })

  it('treats the DOCX extension as case-insensitive on Windows', () => {
    expect(isDocumentEntryReviewDraft(' 文档式批量录入.DOCX ', '文档式批量录入.docx')).toBe(true)
  })

  it('does not classify a real Word source as a document-entry draft', () => {
    expect(isDocumentEntryReviewDraft('期末试题.docx', '文档式批量录入.docx')).toBe(false)
  })
})
