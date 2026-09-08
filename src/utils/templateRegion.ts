import type { TemplateConfigurationParagraph, TemplateStyleSamples } from '../types/domain'

export interface TemplateRegionSuggestion {
  startParagraphIndex: number | null
  endParagraphIndex: number | null
  styleSamples: TemplateStyleSamples
}

const sectionHeadingPattern = /^\s*[一二三四五六七八九十百]+[、.．]\s*/
const questionPattern = /^\s*\d+\s*[.、．)）]\s*/
const optionPattern = /(?:^|\s)[A-HＡ-Ｈ]\s*[.、．)）]\s*/
const answerPattern = /^\s*(?:参考)?答案\s*[：:]?/
const explanationPattern = /^\s*(?:题目)?解析\s*[：:]?/
const trailingMetadataPattern = /^\s*(?:命题人|审核人|制卷人|出卷人|阅卷人)\s*[：:]/

export function suggestTemplateRegion(
  paragraphs: TemplateConfigurationParagraph[],
): TemplateRegionSuggestion {
  const nonEmpty = paragraphs.filter((paragraph) => paragraph.text.trim())
  if (!nonEmpty.length) {
    return {
      startParagraphIndex: null,
      endParagraphIndex: null,
      styleSamples: {},
    }
  }

  const sectionHeading = nonEmpty.find((paragraph) => sectionHeadingPattern.test(paragraph.text))
  const question = nonEmpty.find((paragraph) => questionPattern.test(paragraph.text))
  const option = nonEmpty.find((paragraph) => optionPattern.test(paragraph.text))
  const answer = nonEmpty.find((paragraph) => answerPattern.test(paragraph.text))
  const explanation = nonEmpty.find((paragraph) => explanationPattern.test(paragraph.text))
  const startCandidate = [sectionHeading, question]
    .filter((paragraph): paragraph is TemplateConfigurationParagraph => Boolean(paragraph))
    .sort((left, right) => left.index - right.index)[0] ?? nonEmpty[0]
  const endCandidate = [...nonEmpty]
    .reverse()
    .find((paragraph) => !trailingMetadataPattern.test(paragraph.text))
    ?? nonEmpty[nonEmpty.length - 1]

  return {
    startParagraphIndex: startCandidate.index,
    endParagraphIndex: Math.max(startCandidate.index, endCandidate.index),
    styleSamples: {
      sectionHeadingParagraphIndex: sectionHeading?.index,
      questionParagraphIndex: question?.index ?? startCandidate.index,
      optionParagraphIndex: option?.index,
      answerParagraphIndex: answer?.index,
      explanationParagraphIndex: explanation?.index,
    },
  }
}
