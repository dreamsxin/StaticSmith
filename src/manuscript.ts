/**
 * 文稿度量：大纲、字数、阅读时长。
 *
 * 写长文时真正缺的不是编辑器功能，而是「这篇有多长、结构长什么样、我现在在哪一节」——
 * 也就是纸质书的目录与页码。这三样都能从源文本直接算出来，与渲染无关，
 * 所以放在这里做成纯函数：既能被界面直接用，也能被测到（界面本身没有组件测试）。
 *
 * 三件事共用同一套「哪些内容不算正文」的判断（front matter、围栏代码块），
 * 拆成三个文件的话这套判断迟早会长出三个版本。
 */

export interface Heading {
  /** 1–6，对应 `#` 的个数 */
  level: number
  /** 标题文字，已去掉 `#` 与首尾空白 */
  text: string
  /** 这一行在源文本里的起始下标，跳转时直接拿它设光标 */
  offset: number
}

/** 围栏代码块的起止行：``` 或 ~~~，允许前面有缩进。 */
const FENCE = /^\s*(```|~~~)/

/** ATX 标题：`## 标题`。要求 `#` 后有空白，否则 `#tag` 这种也会被当成标题。 */
const ATX = /^(#{1,6})\s+(.*?)\s*#*\s*$/

/**
 * 逐行扫源文本，跳过不算正文的那些区段。
 *
 * front matter 只在**文件开头**成立：正文里出现一行 `+++` 是分隔线，不是元数据。
 * 围栏代码块必须跳过，否则示例代码里的注释（`# 安装依赖`）会跑进大纲，
 * 而那正是技术文档里最常见的写法。
 */
function* bodyLines(source: string): Generator<{ line: string; offset: number }> {
  const lines = source.split('\n')
  let offset = 0
  let inFrontMatter = lines[0]?.trim() === '+++'
  let inFence = false

  for (const [index, line] of lines.entries()) {
    const current = offset
    offset += line.length + 1

    if (inFrontMatter) {
      // 第一行就是 `+++`，从第二行起找结束的那一行
      if (index > 0 && line.trim() === '+++') inFrontMatter = false
      continue
    }
    if (FENCE.test(line)) {
      inFence = !inFence
      continue
    }
    if (inFence) continue

    yield { line, offset: current }
  }
}

/**
 * 提取标题，构成这一篇的目录。
 *
 * 只认 ATX（`#`）不认 Setext（下划线式）：后者在实际写作里罕见，而支持它需要向前看一行，
 * 会把这个函数从「逐行」变成「有状态」。真有人用，加的时候再说。
 */
export function parseOutline(source: string): Heading[] {
  const headings: Heading[] = []
  for (const { line, offset } of bodyLines(source)) {
    const match = ATX.exec(line)
    if (!match) continue
    const text = match[2].trim()
    if (!text) continue
    headings.push({ level: match[1].length, text, offset })
  }
  return headings
}

/** 行内代码、图片、链接、HTML 标签、Markdown 标记：算字数时要先剥掉。 */
const INLINE_CODE = /`[^`]*`/g
const IMAGE = /!\[[^\]]*\]\([^)]*\)/g
/** 链接保留可见文字，丢掉地址：URL 不是读者读的字。 */
const LINK = /\[([^\]]*)\]\([^)]*\)/g
const HTML_TAG = /<[^>]+>/g
const MARKERS = /[#>*_~|-]+/g

/** 中日韩文字：每个字算一个「字」。 */
const CJK = /[\u3040-\u30ff\u3400-\u4dbf\u4e00-\u9fff\uf900-\ufaff\uac00-\ud7af]/g

/**
 * 正文字数。
 *
 * 中文按**字**、西文按**词**——两种语言的「一个单位」差得太远，混在一起数字符会让
 * 中文文章的数字虚高三四倍，数词又会让中文几乎归零。这也是各家写作工具的通行做法。
 *
 * 不算的东西：front matter、围栏代码块、行内代码、图片、链接地址、HTML 标签、
 * Markdown 标记符号。判断标准是「读者会读到吗」——代码块与地址不会。
 */
export function countWords(source: string): number {
  let text = ''
  for (const { line } of bodyLines(source)) {
    text += `${line}\n`
  }
  const plain = text
    .replace(INLINE_CODE, ' ')
    .replace(IMAGE, ' ')
    .replace(LINK, '$1')
    .replace(HTML_TAG, ' ')
    .replace(MARKERS, ' ')

  const cjk = plain.match(CJK)?.length ?? 0
  const latin = plain
    .replace(CJK, ' ')
    .split(/[^\p{L}\p{N}'’-]+/u)
    .filter((word) => /[\p{L}\p{N}]/u.test(word)).length

  return cjk + latin
}

/**
 * 每分钟读多少字。
 *
 * 300 是中文默读的常见取值（英文约 200 词，而这里中英按同一个数计——
 * 混排文章里再分两档，误差还不如口径统一带来的可解释性值钱）。
 */
export const WORDS_PER_MINUTE = 300

/** 阅读时长（分钟），至少 1 分钟——「0 分钟」对读者没有意义。 */
export function readingMinutes(words: number): number {
  if (words <= 0) return 0
  return Math.max(1, Math.round(words / WORDS_PER_MINUTE))
}
