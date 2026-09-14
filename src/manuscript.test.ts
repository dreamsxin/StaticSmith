import { describe, expect, it } from 'vitest'

import { countWords, parseOutline, readingMinutes } from './manuscript'

const WITH_FRONT_MATTER = `+++
title = "标题不算正文"
tags = ["# 这也不是标题"]
+++

# 第一章

一段中文正文。

## 一节

\`\`\`bash
# 安装依赖，这是代码注释不是标题
npm install
\`\`\`

### 更深一层

末尾一段。
`

describe('大纲', () => {
  it('按层级抽出标题，front matter 与代码块里的 # 都不算', () => {
    const outline = parseOutline(WITH_FRONT_MATTER)
    expect(outline.map((h) => [h.level, h.text])).toEqual([
      [1, '第一章'],
      [2, '一节'],
      [3, '更深一层'],
    ])
  })

  it('offset 指向标题那一行的开头，可以直接拿来定位光标', () => {
    const outline = parseOutline(WITH_FRONT_MATTER)
    for (const heading of outline) {
      expect(WITH_FRONT_MATTER.slice(heading.offset)).toMatch(/^#{1,6} /)
    }
  })

  it('`#tag` 不是标题：# 后面必须有空白', () => {
    expect(parseOutline('#hashtag\n\n# 真标题')).toEqual([
      { level: 1, text: '真标题', offset: 10 },
    ])
  })

  it('尾部的 # 是装饰，不进标题文字', () => {
    expect(parseOutline('## 居中写法 ##')[0].text).toBe('居中写法')
  })

  it('正文里出现的 +++ 是分隔线，不当 front matter', () => {
    const source = '# 开头\n\n+++\n\n## 后面还在'
    expect(parseOutline(source).map((h) => h.text)).toEqual(['开头', '后面还在'])
  })
})

describe('字数', () => {
  it('中文按字数', () => {
    expect(countWords('一段中文正文。')).toBe(6)
  })

  it('西文按词数', () => {
    expect(countWords('a quick brown fox')).toBe(4)
  })

  it('中英混排各按各的规则相加', () => {
    expect(countWords('用 Rust 写的 CLI')).toBe(5)
  })

  it('front matter、代码块、行内代码都不算', () => {
    const source = `+++
title = "十个字的标题在这里"
+++

正文两字

\`\`\`
code block words here
\`\`\`

\`inline\` 结束
`
    expect(countWords(source)).toBe(6)
  })

  it('链接只算可见文字，地址不算', () => {
    expect(countWords('见 [官方文档](https://example.com/very/long/path)')).toBe(5)
  })

  it('图片整块不算', () => {
    expect(countWords('![一张很长的图片说明](/media/a.png)')).toBe(0)
  })
})

describe('阅读时长', () => {
  it('空文稿是 0 分钟', () => {
    expect(readingMinutes(0)).toBe(0)
  })

  it('很短的文稿也算 1 分钟：0 分钟对读者没有意义', () => {
    expect(readingMinutes(12)).toBe(1)
  })

  it('按每分钟 300 字四舍五入', () => {
    expect(readingMinutes(300)).toBe(1)
    expect(readingMinutes(1500)).toBe(5)
  })
})
