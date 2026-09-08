export function canPersistWordImportReview(
  desktopAvailable: boolean,
  documentEntryReview: boolean,
) {
  return desktopAvailable || documentEntryReview
}

export function expectedWordImportReviewParserVersion(
  documentEntryDraft: boolean,
  wordImportParserVersion: string,
  documentEntryParserVersion: string,
) {
  return documentEntryDraft ? documentEntryParserVersion : wordImportParserVersion
}

export function isDocumentEntryReviewDraft(
  sourceFileName: string,
  documentEntrySourceFileName: string,
) {
  return sourceFileName.normalize('NFKC').trim().toLocaleLowerCase('zh-CN')
    === documentEntrySourceFileName.normalize('NFKC').trim().toLocaleLowerCase('zh-CN')
}
