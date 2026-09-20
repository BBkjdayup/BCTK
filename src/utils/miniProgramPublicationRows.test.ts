import { describe, expect, it } from 'vitest'
import type { PaperSummary } from '../types/domain'
import { currentPublicationRows, publicationActions, publicationRow } from './miniProgramPublicationRows'

const local = (id: string, rowVersion = 2): PaperSummary => ({ id, rowVersion, title: '同名试卷', questionCount: 3, compositionMode: 'manual', status: 'saved', subjectSummaryText: '', createdAt: 1, updatedAt: 5 })
const cloud = (paperId: string, published = true) => ({ paperId, published, title: '同名试卷', questionCount: 4, paperRowVersion: 2, publishedAt: 10 })

describe('cross-device publication management', () => {
  it('shows cloud-only papers and deduplicates by identity rather than title', () => {
    const rows = currentPublicationRows([local('same-id'), local('local-only')], [cloud('same-id'), cloud('remote-only')], '')
    expect(rows).toHaveLength(3)
    const remote = rows.find(row => row.id === 'remote-only')!
    expect(remote.local).toBeNull()
    expect(remote.questionCount).toBe(4)
    expect(publicationActions(remote)).toMatchObject({ withdraw: true, publish: false })
    expect(rows.find(row => row.id === 'same-id')?.local).not.toBeNull()
  })
  it('hides withdrawn cloud-only records from the default view while keeping local originals', () => {
    const rows = currentPublicationRows([local('with-original')], [cloud('cloud-only', false), cloud('with-original', false)], '')
    expect(rows.map(row => row.id)).toEqual(['with-original'])
    expect(publicationActions(rows[0]!)).toMatchObject({ withdraw: false, publish: true })
  })
  it('keeps withdrawal available for newer and older local versions without allowing an older overwrite', () => {
    expect(publicationActions(publicationRow(local('id', 3), cloud('id')))).toMatchObject({ withdraw: true, publish: true, newerLocal: true })
    expect(publicationActions(publicationRow(local('id', 1), cloud('id')))).toMatchObject({ withdraw: true, publish: false, olderLocal: true })
    expect(publicationActions(publicationRow(local('id', 2), cloud('id')))).toMatchObject({ withdraw: true, publish: false })
  })
  it('uses cloud metadata for published content while allowing search by the renamed original', () => {
    const original = { ...local('id'), title: '本机改名' }
    const rows = currentPublicationRows([original], [cloud('id')], '本机改名')
    expect(rows[0]).toMatchObject({ title: '同名试卷', questionCount: 4, updatedAt: 10 })
    expect(currentPublicationRows([], [cloud('id')], '同名')).toHaveLength(1)
  })
})
