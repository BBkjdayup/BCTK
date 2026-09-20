import { expect, it, vi } from 'vitest'
import { prepareMedia } from './miniProgramMedia'
import { miniProgramPaper } from './miniProgramPaper'
import { plainTextRichContent } from './richContent'
import type { Paper, Question } from '../types/domain'
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
