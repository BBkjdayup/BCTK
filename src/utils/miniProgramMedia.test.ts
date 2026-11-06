import { afterEach, expect, it, vi } from 'vitest'
import { prepareMedia } from './miniProgramMedia'
import { miniProgramPaper } from './miniProgramPaper'
import { plainTextRichContent } from './richContent'
import type { Paper, Question } from '../types/domain'
import { createMediaPreparationCache, MediaCache } from './mediaCache'
afterEach(() => vi.useRealTimers())
const imagePaper = (count: number) => ({ items: [{ snapshot: {
  stem: { html: Array.from({ length: count }, (_, i) => `<img data-resource-id="id-${i}">`).join('') },
  answer: { html: '' }, explanation: { html: '' }, options: [],
} }] } as unknown as Paper)
const limited = () => Object.assign(new Error('busy'), { status: 429, code: 'RATE_LIMITED', retryAfterSeconds: 7 })
const getImage = vi.fn(async (id: string) => ({ resourceId: id, mimeType: 'image/png' as const, dataBase64: id }))

it('waits at the 121st resource and finishes without re-uploading the first 120', async () => {
  vi.useFakeTimers()
  let attempts = 0
  const upload = vi.fn(async (image: { dataBase64: string }) => {
    if (++attempts === 121) throw limited()
    return { id: image.dataBase64, width: 100, height: 100 }
  })
  const onProgress = vi.fn()
  const result = prepareMedia(imagePaper(121), { getImage, upload, onProgress })
  await vi.advanceTimersByTimeAsync(0)
  expect(upload).toHaveBeenCalledTimes(121)
  expect(onProgress).toHaveBeenLastCalledWith({ completed: 120, total: 121, phase: 'waiting', retryAfterSeconds: 7 })
  await vi.advanceTimersByTimeAsync(6999)
  expect(upload).toHaveBeenCalledTimes(121)
  await vi.advanceTimersByTimeAsync(1)
  expect((await result).size).toBe(121)
  expect(upload).toHaveBeenCalledTimes(122)
})

it('cancels a rate-limit wait immediately and reuses completed assets on retry', async () => {
  vi.useFakeTimers()
  const controller = new AbortController(), cache = createMediaPreparationCache()
  const upload = vi.fn(async (image: { dataBase64: string }) => ({ id: image.dataBase64, width: 100, height: 100 }))
    .mockResolvedValueOnce({ id: 'id-0', width: 100, height: 100 }).mockRejectedValueOnce(limited())
  const dependencies = { getImage, upload, cache, checkAssets: async () => ['id-0'] }
  const rejected = expect(prepareMedia(imagePaper(2), { ...dependencies, signal: controller.signal })).rejects.toThrow('已取消发布')
  await vi.advanceTimersByTimeAsync(0)
  controller.abort(); await rejected
  await vi.advanceTimersByTimeAsync(120_000)
  expect(upload).toHaveBeenCalledTimes(2)
  expect((await prepareMedia(imagePaper(2), dependencies)).size).toBe(2)
  expect(upload).toHaveBeenCalledTimes(3)
  expect(upload.mock.calls[2]![0].dataBase64).toBe('id-1')
})

it('bounds rate-limit retries and does not retry failures with an uncertain upload result', async () => {
  vi.useFakeTimers()
  const upload = vi.fn().mockRejectedValue(limited())
  const rejected = expect(prepareMedia(imagePaper(1), { getImage, upload })).rejects.toThrow('服务持续繁忙')
  await vi.advanceTimersByTimeAsync(21_000); await rejected
  expect(upload).toHaveBeenCalledTimes(4)
  upload.mockReset().mockRejectedValue(new Error('network disconnected'))
  await expect(prepareMedia(imagePaper(1), { getImage, upload })).rejects.toThrow('network disconnected')
  expect(upload).toHaveBeenCalledOnce()
})
it('uploads local images and formula PNGs, deduplicates references, and emits no local paths', async () => {
  const field = { ...plainTextRichContent('图和公式'), html: '<p><img data-resource-id="local-1"/><span data-latex="x^2"></span></p>' }
  const question={type:'short_answer',stem:field,options:[],answer:field,explanation:plainTextRichContent('解析')} as unknown as Question
  const paper={status:'saved',title:'试卷',items:[{id:'q1',position:0,snapshot:question}]} as unknown as Paper
  const getImage=vi.fn(async()=>({resourceId:'local-1',mimeType:'image/png' as const,dataBase64:'synthetic'}));
  const upload=vi.fn(async()=>({id:'10000000-0000-4000-8000-000000000001',width:200,height:100}));
  const formula=vi.fn(async()=>({mimeType:'image/png',dataBase64:'synthetic-formula',width:40}));
  const media=await prepareMedia(paper,{getImage,upload,formula});const result=miniProgramPaper(paper,[],media);
  expect(upload).toHaveBeenCalledTimes(2);expect(formula).toHaveBeenCalledOnce();
  expect(result.questions[0]!.stem).toContain('tkasset:');expect(result.questions[0]!.stem).not.toContain('local-1');
  expect(result.questions[0]!.answer).toContain('data-width="40"');
});
it('does not fetch external image URLs or turn failed formula conversions into a publication', async () => {
  const make=(html:string)=>({items:[{snapshot:{stem:{html},options:[],answer:{html:''},explanation:{html:''}}}]} as unknown as Paper);
  const upload=vi.fn(),getImage=vi.fn(),formula=vi.fn(async()=>{throw new Error('invalid formula')});
  await expect(prepareMedia(make('<img src="https://external.invalid/private">'),{upload,getImage,formula})).rejects.toThrow('外部图片');
  await expect(prepareMedia(make('<span data-latex="bad"></span>'),{upload,getImage,formula})).rejects.toThrow('invalid formula');
  expect(upload).not.toHaveBeenCalled();expect(getImage).not.toHaveBeenCalled();
});

it('uploads one image while preserving each occurrence width and bounding invalid widths', async () => {
  const rich = (width?: string) => ({ ...plainTextRichContent(''), html: `<p><img data-resource-id="shared"${width === undefined ? '' : ` width="${width}"`}/></p>` })
  const question = { type: 'short_answer', stem: rich('120'), answer: rich('480'), explanation: rich('Infinity'), options: [] } as unknown as Question
  const paper = { status: 'saved', title: '图片', items: [{ id: 'q1', position: 0, snapshot: question }, { id: 'q2', position: 1, snapshot: { ...question, stem: rich('9999'), answer: rich('-1'), explanation: rich() } }] } as unknown as Paper
  const upload = vi.fn(async () => ({ id: 'cloud-id', width: 640, height: 320 }))
  const media = await prepareMedia(paper, { upload, getImage: vi.fn(async () => ({ resourceId: 'shared', mimeType: 'image/png' as const, dataBase64: 'synthetic' })) })
  const result = miniProgramPaper(paper, [], media)
  expect(upload).toHaveBeenCalledOnce()
  expect(result.questions[0]!.stem).toContain('data-width="120"')
  expect(result.questions[0]!.answer).toContain('data-width="480"')
  expect(result.questions[0]!.explanation).toContain('data-width="640"')
  for (const field of ['stem', 'answer', 'explanation'] as const) expect(result.questions[1]![field]).toContain('data-width="640"')
})

it('reuses checked upload receipts and formula rendering, and repairs a stale cloud resource', async () => {
  const empty = plainTextRichContent('')
  const paper = { items: [{ snapshot: { stem: { ...empty, html: '<span data-latex="x^2"></span>' }, answer: empty, explanation: empty, options: [] } }] } as unknown as Paper
  const cache = createMediaPreparationCache()
  const formula = vi.fn(async () => ({ mimeType: 'image/png', dataBase64: 'rendered', width: 30 }))
  const upload = vi.fn(async () => ({ id: 'cloud', width: 60, height: 30 }))
  const checkAssets = vi.fn(async () => ['cloud'])
  const dependencies = { cache, formula, upload, checkAssets, getImage: vi.fn() }
  await prepareMedia(paper, dependencies)
  await prepareMedia(paper, dependencies)
  expect(formula).toHaveBeenCalledOnce(); expect(upload).toHaveBeenCalledOnce()
  expect(checkAssets).toHaveBeenCalledWith(['cloud'])
  checkAssets.mockResolvedValueOnce([])
  await prepareMedia(paper, dependencies)
  expect(upload).toHaveBeenCalledTimes(2); expect(formula).toHaveBeenCalledOnce()
  await prepareMedia(paper, { ...dependencies, cache: createMediaPreparationCache() })
  expect(upload).toHaveBeenCalledTimes(3)
})

it('checks every formula before uploading any preceding image', async () => {
  const empty = plainTextRichContent('')
  const paper = { items: [{ snapshot: { stem: { ...empty, html: '<img data-resource-id="id"/><span data-latex="\\notARealCommand{x}"></span>' }, answer: empty, explanation: empty, options: [] } }] } as unknown as Paper
  const upload = vi.fn()
  await expect(prepareMedia(paper, { getImage: vi.fn(), upload })).rejects.toThrow()
  expect(upload).not.toHaveBeenCalled()
})

it('bounds cache storage and evicts the least recently used result', () => {
  const cache = new MediaCache<string>(100, 2)
  cache.set('a', 'A'); cache.set('b', 'B'); cache.get('a'); cache.set('c', 'C')
  expect(cache.get('b')).toBeUndefined(); expect(cache.get('a')).toBe('A')
  cache.set('large', 'x'.repeat(100)); expect(cache.get('large')).toBeUndefined()
})
