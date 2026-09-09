import { describe, expect, it } from 'vitest'

import { highlight } from './source-highlight'

/**
 * 把本文件产生的 `<span class="tok-*">` 与 `</span>` 去掉，剩下的文本里
 * **不应该再有任何裸的 `<` 或 `>`**。
 *
 * 这是着色器唯一真正要守的安全不变量：结果经 `v-html` 插入，
 * 只有着色器自己生成的 span 才是标签，源文里的一切都必须是文本。
 * 逐个断言「这个 payload 被转义了」永远追不上新的 payload，
 * 所以这里断言的是「除了我们的 span，没有别的标签」。
 */
function markupOnlyFromSpans(html: string): boolean {
  const stripped = html.replace(/<span class="tok-[\w-]+">/g, '').replace(/<\/span>/g, '')
  return !stripped.includes('<') && !stripped.includes('>')
}

describe('source-highlight 的转义', () => {
  const payloads = [
    '<script>alert(1)</script>',
    '<img src=x onerror=alert(1)>',
    '<!-- <b>注释里的标签</b> -->',
    '&lt;已经转义过的也不能双重解释&gt;',
    '<a href="javascript:alert(1)">点我</a>',
  ]

  it('Markdown 正文里的标签一律变成文本', () => {
    for (const payload of payloads) {
      const html = highlight(`# 标题\n\n${payload}\n`, 'markdown')
      expect(markupOnlyFromSpans(html), payload).toBe(true)
      // 注意断言的是「没有裸标签」，而不是「看不到 payload 的字」：
      // 编辑器镜像本来就要把源文原样显示出来，`onerror=...` 作为**文本**出现是对的。
      expect(html).not.toContain('<script')
      expect(html).not.toContain('<img')
    }
  })

  it('围栏代码块里的标签也要转义', () => {
    const html = highlight('```html\n<script>alert(1)</script>\n```\n', 'markdown')
    expect(markupOnlyFromSpans(html)).toBe(true)
    expect(html).toContain('&lt;script&gt;')
  })

  it('front matter 的键值都要转义', () => {
    const html = highlight('+++\ntitle = "<img onerror=x>"\n+++\n正文\n', 'markdown')
    expect(markupOnlyFromSpans(html)).toBe(true)
    expect(html).not.toContain('<img')
  })

  /** HTML 站点的正文会给标签着色——**着色不等于放行**，标签仍然是转义后的文本。 */
  it('HTML 格式给标签着色但不放行标签', () => {
    const html = highlight('<script>alert(1)</script>\n', 'html')
    expect(markupOnlyFromSpans(html)).toBe(true)
    expect(html).toContain('&lt;script&gt;')
    expect(html).not.toContain('<script')
  })

  it('HTML 注释与属性值同样是文本', () => {
    for (const format of ['markdown', 'html'] as const) {
      const html = highlight('<!-- <b>x</b> --><a href="&<>">y</a>\n', format)
      expect(markupOnlyFromSpans(html), format).toBe(true)
    }
  })

  it('超长源文退化成纯转义，不再着色', () => {
    const raw = `<b>${'x'.repeat(200_001)}`
    const html = highlight(raw, 'markdown')
    expect(html).not.toContain('<span')
    expect(html.startsWith('&lt;b&gt;')).toBe(true)
  })
})

describe('source-highlight 的行数', () => {
  /**
   * 镜像与 `textarea` 的滚动靠行数一一对应，差一行就错位。
   * 末尾那一行是刻意补的（`textarea` 滚到底时会多留一行高度）。
   */
  it('输出行数 = 源文行数 + 1', () => {
    for (const raw of ['', '一行', 'a\nb\nc', '+++\ntitle = "t"\n+++\n正文\n']) {
      for (const format of ['markdown', 'html'] as const) {
        const lines = highlight(raw, format).split('\n').length
        expect(lines, `${format}: ${JSON.stringify(raw)}`).toBe(raw.split('\n').length + 1)
      }
    }
  })
})
