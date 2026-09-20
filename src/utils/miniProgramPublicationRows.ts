import type { PaperSummary } from '../types/domain'
import type { MiniProgramPublication } from '../types/miniProgram'

export interface PublicationRow {
  id: string
  title: string
  questionCount: number | null
  updatedAt: number
  local: PaperSummary | null
  publication: MiniProgramPublication | null
}

export function publicationRow(local: PaperSummary | null, publication: MiniProgramPublication | null): PublicationRow {
  const id = publication?.paperId ?? local!.id
  return {
    id, local, publication,
    title: publication?.title || local?.title || `云端试卷（${id.slice(0, 8)}）`,
    questionCount: publication?.questionCount ?? local?.questionCount ?? null,
    updatedAt: publication?.publishedAt ?? local!.updatedAt,
  }
}

export function currentPublicationRows(locals: PaperSummary[], publications: MiniProgramPublication[], keyword: string) {
  const rows = new Map(locals.map(local => [local.id, publicationRow(local, null)]))
  for (const publication of publications) {
    if (publication.published === false) continue
    rows.set(publication.paperId, publicationRow(rows.get(publication.paperId)?.local ?? null, publication))
  }
  const search = keyword.trim().toLocaleLowerCase('zh-CN')
  return [...rows.values()].filter(row => !search || [row.title, row.local?.title ?? ''].some(title => title.toLocaleLowerCase('zh-CN').includes(search)))
    .sort((a, b) => b.updatedAt - a.updatedAt || a.id.localeCompare(b.id))
}

export function publicationActions(row: PublicationRow) {
  const published = !!row.publication && row.publication.published !== false
  const cloudVersion = row.publication?.paperRowVersion ?? 0
  return {
    withdraw: published,
    publish: !!row.local && row.local.rowVersion >= cloudVersion && (!published || row.local.rowVersion > cloudVersion),
    newerLocal: published && !!row.local && row.local.rowVersion > cloudVersion,
    olderLocal: !!row.local && row.local.rowVersion < cloudVersion,
  }
}
