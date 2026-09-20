import { mount } from '@vue/test-utils'
import { nextTick } from 'vue'
import { describe, expect, it } from 'vitest'
import type { RichContent } from '../types/domain'
import QuestionStemSummary from './QuestionStemSummary.vue'

function content(html: string, plainText: string): RichContent {
  return { schemaVersion: 1, html, plainText }
}

describe('QuestionStemSummary', () => {
  it('keeps long formulas intact and uses a readable marker for malformed formulas', async () => {
    const latex = String.raw`\frac{${'a+'.repeat(100)}b}{\sqrt{x_1}}`
    const wrapper = mount(QuestionStemSummary, { props: { content: content(
      `<p><span data-latex="${latex}"></span><span data-latex="\\frac{">bad</span></p><div data-latex="x^2"></div>`, '',
    ) } })
    await nextTick()
    expect(wrapper.find('.math-node').attributes('data-latex')).toBe(latex)
    expect(wrapper.find('.frac-line').exists()).toBe(true)
    expect(wrapper.findAll('.katex')).toHaveLength(2)
    expect(wrapper.text()).toContain('〔公式〕')
    expect(wrapper.find('.katex-error').exists()).toBe(false)
  })

  it('removes active HTML and styling from legacy summaries', async () => {
    const wrapper = mount(QuestionStemSummary, { props: { content: content(
      '<p><a href="javascript:alert(1)">题目</a><span onclick="alert(1)" style="position:fixed">内容</span><svg onload="alert(1)"></svg></p>', '题目内容',
    ) } })
    await nextTick()
    expect(wrapper.find('[onclick], [onload], a, svg, [style*="fixed"]').exists()).toBe(false)
    expect(wrapper.text()).toContain('题目内容')
  })

  it('renders stored formula nodes instead of exposing LaTeX source', async () => {
    const wrapper = mount(QuestionStemSummary, {
      props: {
        content: content(
          '<p><span class="math-node" data-latex="675^{\\circ}">675^{\\circ}</span>用弧度制表示为（　）</p>',
          String.raw`\(675^{\circ}\)用弧度制表示为（　）`,
        ),
      },
    })
    await nextTick()

    const formula = wrapper.find('.math-node')
    expect(formula.find('.katex').exists()).toBe(true)
    expect(formula.find('.katex-html').text()).toContain('675')
    expect(formula.find('.katex-html').text()).toContain('∘')
    expect(wrapper.text()).not.toContain(String.raw`\(675^{\circ}\)`)
  })

  it('compacts images and tables into list-safe markers', async () => {
    const wrapper = mount(QuestionStemSummary, {
      props: {
        content: content(
          '<p>根据下图作答<img data-resource-id="image-1" alt="函数图像"></p><table><tbody><tr><td>x</td></tr></tbody></table>',
          '根据下图作答 [图片] [表格]',
        ),
      },
    })
    await nextTick()

    expect(wrapper.find('img').exists()).toBe(false)
    expect(wrapper.find('table').exists()).toBe(false)
    expect(wrapper.find('.question-stem-summary__asset--image').text()).toBe('图片')
    expect(wrapper.find('.question-stem-summary__asset--table').text()).toBe('表格')
  })

  it('renders formula delimiters from a plain-text-only fallback', async () => {
    const wrapper = mount(QuestionStemSummary, {
      props: { text: String.raw`函数 \(y=\sin x\) 的图像` },
    })
    await nextTick()

    expect(wrapper.find('.katex').exists()).toBe(true)
    expect(wrapper.find('.katex-html').text()).toContain('y=sinx')
    expect(wrapper.text()).not.toContain(String.raw`\(y=\sin x\)`)
  })
})
