import { describe, expect, it } from 'vitest'

import { countWords, moveHeading, parseOutline, readingMinutes } from './manuscript'

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

/**
 * 整节挪动。
 *
 * 目录能看出「第一节在第二节前面」之后，紧接着的要求就是**调整它们的先后**，
 * 而这要搬的是「标题 + 它下属的全部内容」——挪标题不挪正文只会把文章拆散。
 */
describe('整节挪动', () => {
  const BOOK = `# 第一章

引子。

## 甲

甲的内容。

### 甲之一

更深的一层。

## 乙

乙的内容。
`

  /** 某个标题的位置。 */
  const offsetOf = (source: string, text: string) =>
    parseOutline(source).find((h) => h.text === text)!.offset

  it('上移：整节（含子标题与正文）与上一个同级兄弟互换', () => {
    const moved = moveHeading(BOOK, offsetOf(BOOK, '乙'), -1)!

    expect(parseOutline(moved.text).map((h) => h.text)).toEqual([
      '第一章',
      '乙',
      '甲',
      '甲之一',
    ])
    // 子标题与正文跟着走，没有被留在原处
    expect(moved.text).toContain('## 乙\n\n乙的内容。\n\n## 甲\n\n甲的内容。\n\n### 甲之一')
    // 光标落点就是这个标题的新位置
    expect(moved.offset).toBe(offsetOf(moved.text, '乙'))
  })

  it('下移是上移的逆操作：挪回去应当一字不差', () => {
    const up = moveHeading(BOOK, offsetOf(BOOK, '乙'), -1)!
    const back = moveHeading(up.text, offsetOf(up.text, '乙'), 1)!
    expect(back.text).toBe(BOOK)
  })

  it('撞上更高一级的标题就不动：那是搬动，不是排序', () => {
    const nested = `## 甲

### 一

## 乙

### 二
`
    // 「二」的上一个同级是「一」，但中间隔着 `## 乙` —— 挪过去等于换了爹
    expect(moveHeading(nested, offsetOf(nested, '二'), -1)).toBeNull()
  })

  it('头一个与最末一个同级兄弟挪不动，返回 null 而不是原样返回', () => {
    expect(moveHeading(BOOK, offsetOf(BOOK, '甲'), -1)).toBeNull()
    expect(moveHeading(BOOK, offsetOf(BOOK, '乙'), 1)).toBeNull()
    // 只有一个一级标题，它自己也没处挪
    expect(moveHeading(BOOK, offsetOf(BOOK, '第一章'), 1)).toBeNull()
  })

  it('末尾没有换行时不许把两行黏在一起，而且挪回去能复原', () => {
    const source = '## 甲\n甲的内容\n\n## 乙\n乙的内容'
    const up = moveHeading(source, offsetOf(source, '乙'), -1)!

    // 两节之间的空行留在两节之间，文件末尾照旧没有换行——两处排版都没被搬走
    expect(up.text).toBe('## 乙\n乙的内容\n\n## 甲\n甲的内容')
    expect(up.text).not.toContain('乙的内容## 甲')
    expect(moveHeading(up.text, offsetOf(up.text, '乙'), 1)!.text).toBe(source)
  })

  it('代码块里长得像标题的行骗不到它', () => {
    const source = `## 甲

\`\`\`md
## 假的
\`\`\`

## 乙
`
    const moved = moveHeading(source, offsetOf(source, '乙'), -1)!
    // 「假的」不是标题，所以整块（含代码块）跟着「甲」一起走
    expect(parseOutline(moved.text).map((h) => h.text)).toEqual(['乙', '甲'])
    expect(moved.text).toContain('```md\n## 假的\n```')
  })

  it('认不出的位置不动手', () => {
    expect(moveHeading(BOOK, 9999, -1)).toBeNull()
  })
})
