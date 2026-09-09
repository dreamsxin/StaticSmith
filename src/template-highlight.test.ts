import { describe, expect, it } from 'vitest'

import { highlight } from './template-highlight'

/** 与 source-highlight 那份同一个不变量：除了着色器自己的 span，不该有别的标签。 */
function markupOnlyFromSpans(html: string): boolean {
  const stripped = html.replace(/<span class="tok-[\w-]+">/g, '').replace(/<\/span>/g, '')
  return !stripped.includes('<') && !stripped.includes('>')
}

describe('template-highlight 的转义', () => {
  /**
   * 这个着色器在**原始文本**上分词（HTML 标签本身以 `<` 开头，先转义就没法分词），
   * 所以「每一段输出前都调 escapeHtml」是它唯一的防线——比 Markdown 那份更容易漏。
   */
  it('模板里的标签着色后仍是文本', () => {
    const html = highlight('<a href="/x" class="y">链接</a>\n')
    expect(markupOnlyFromSpans(html)).toBe(true)
    expect(html).toContain('&lt;')
    expect(html).not.toContain('<a ')
  })

  it('Tera 表达式、语句、注释里的标签都要转义', () => {
    const sources = [
      '{{ page.title }}<script>alert(1)</script>',
      '{% if x %}<img onerror=alert(1)>{% endif %}',
      '{# <b>注释里的标签</b> #}',
      '{{ "<script>" }}',
    ]
    for (const source of sources) {
      const html = highlight(`${source}\n`)
      expect(markupOnlyFromSpans(html), source).toBe(true)
      // 同上：payload 的**文字**该出现（这是源码视图），成为标签才是问题。
      expect(html).not.toContain('<script')
      expect(html).not.toContain('<img')
    }
  })

  it('属性值里的引号与尖括号不破坏结构', () => {
    const html = highlight('<div data-x="a<b>c" data-y=\'&\'>文字</div>\n')
    expect(markupOnlyFromSpans(html)).toBe(true)
  })

  it('没闭合的标签与注释也不会漏出去', () => {
    for (const source of ['<div class="x"', '<!-- 没闭合', '{% if', '{{ 未闭合']) {
      const html = highlight(`${source}\n`)
      expect(markupOnlyFromSpans(html), source).toBe(true)
    }
  })

  it('超长源码退化成纯转义', () => {
    const html = highlight(`<b>${'x'.repeat(200_001)}`)
    expect(html).not.toContain('<span')
    expect(html.startsWith('&lt;b&gt;')).toBe(true)
  })

  /** 字符位置要与原文对应，否则镜像与 textarea 对不齐。 */
  it('去掉 span 与转义后能还原出原文', () => {
    const source = '{% for p in pages %}\n  <a href="{{ p.url }}">{{ p.title }}</a>\n{% endfor %}'
    const restored = highlight(source)
      .replace(/<span class="tok-[\w-]+">/g, '')
      .replace(/<\/span>/g, '')
      .replace(/&lt;/g, '<')
      .replace(/&gt;/g, '>')
      .replace(/&amp;/g, '&')
    expect(restored).toBe(`${source}\n`)
  })
})
