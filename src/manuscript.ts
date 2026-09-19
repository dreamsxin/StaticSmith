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

/**
 * 一个标题连同它下属的全部内容在源文里的区间（含标题行本身）。
 *
 * 结束位置是**下一个同级或更高级标题的开头**，所以子标题与正文都算在里面
 * ——挪一节就该把整节挪走，这正是纸质书里「调整章节顺序」的意思。
 */
export interface Block {
  readonly start: number
  readonly end: number
}

/** 某个标题（按下标）的整块区间。 */
function blockOf(source: string, headings: readonly Heading[], at: number): Block {
  const level = headings[at].level
  const next = headings.findIndex((h, i) => i > at && h.level <= level)
  return {
    start: headings[at].offset,
    end: next < 0 ? source.length : headings[next].offset,
  }
}

/**
 * 同级的上一个 / 下一个兄弟标题的下标；没有时返回 `-1`。
 *
 * **不跨父标题**：往前 / 往后找的时候一旦撞上更高一级的标题就停手。
 * 「把 2.1 挪到第 1 章下面」改变的是它属于谁，那是搬动而不是排序——与列表里
 * 「跨栏目拖动不做」是同一条取舍：不做，比做一半更好解释。
 *
 * 单独导出是为了让界面能**只用标题表**就判断出「这一节还能不能挪」：
 * 拿 `moveHeading` 去试算要把整篇重排一遍，而那是每敲一个字都要重算的东西。
 */
export function peerIndex(headings: readonly Heading[], at: number, delta: -1 | 1): number {
  const me = headings[at]
  if (!me) return -1
  for (let i = at + delta; i >= 0 && i < headings.length; i += delta) {
    if (headings[i].level < me.level) break
    if (headings[i].level === me.level) return i
  }
  return -1
}

/**
 * 把某一节（标题 + 它下属的全部内容）与**同级的上一个 / 下一个兄弟**整块互换。
 *
 * 返回新的全文与这个标题挪动之后的位置；挪不动时返回 `null`（调用方据此置灰，
 * 而不是发一次什么也不改的改动）。能不能挪由 [`peerIndex`] 说，两处共用一份判断。
 *
 * 标题从 `parseOutline` 来，所以围栏代码块里的 `## 看起来像标题` 不会把它骗到。
 */
export function moveHeading(
  source: string,
  offset: number,
  delta: -1 | 1,
): { text: string; offset: number } | null {
  const headings = parseOutline(source)
  const at = headings.findIndex((h) => h.offset === offset)
  if (at < 0) return null
  const peer = peerIndex(headings, at, delta)
  if (peer < 0) return null

  // 同级兄弟之间没有空隙：前一块的结束就是后一块的开头（它的 level <= 前者）
  const first = blockOf(source, headings, Math.min(at, peer))
  const second = blockOf(source, headings, Math.max(at, peer))

  // 把每一块拆成「内容」与「尾部空行」，只换内容。
  //
  // 尾部空行不是内容的一部分，而是**这个位置的排版**：前一块的尾巴是两块之间的空行，
  // 后一块的尾巴是文件的结尾（可能一个换行都没有）。整块连尾巴一起换的话，
  // 「最后一节」的单换行会被搬到中间，两节之间的空行就没了；文件末尾没有换行时，
  // 下一节的标题还会被黏在上一节的最后一行后面。拆开换，两处排版都留在原地，
  // 而且再挪回来一字不差。
  const split = (block: Block) => {
    const text = source.slice(block.start, block.end)
    const content = text.replace(/\n+$/, '')
    return { content, gap: text.slice(content.length) }
  }
  const a = split(first)
  const b = split(second)
  const head = b.content + a.gap

  return {
    text: source.slice(0, first.start) + head + a.content + b.gap + source.slice(second.end),
    // 上移的是后一块（它落到最前面），下移的是前一块（它落到 head 之后）
    offset: at > peer ? first.start : first.start + head.length,
  }
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
