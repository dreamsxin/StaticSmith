/**
 * 源文着色（Markdown 与 HTML 两种正文格式共用一份）。
 *
 * 编辑器保持「源文是唯一真相」：不引入富文本模型，只在 `textarea` 背后垫一层
 * 只读的高亮镜像，让标记本身看得清——标题、代码、链接、标签、front matter 各有其色，
 * 但你改的仍然是纯文本。
 *
 * 为什么自己写而不引入 CodeMirror/Prism：编辑器只需要「看清结构」，
 * 不需要折叠、补全、语言服务。为此塞进几百 KB 依赖并不值得，
 * 而且高亮的规则必须与站点实际支持的语法子集一致。
 *
 * 为什么两种格式合在一个文件里、由参数分派：front matter 那段是**完全相同**的
 * （两种格式的文件头都是 `+++` 包起来的 TOML）。拆成两个文件就要复制那段逻辑，
 * 而它已经踩过坑——「文件开头的 `+++` 才是 front matter，正文中间的是普通文本」。
 *
 * 安全：结果会经 `v-html` 插入，所以**一切文本都先转义**，
 * 只有本文件产生的 `<span class="tok-*">` 是标签。
 */

/** 正文格式，与 Rust 侧 `build.source_format` 一一对应。 */
export type SourceFormat = 'markdown' | 'html'

const ESCAPES: Record<string, string> = { '&': '&amp;', '<': '&lt;', '>': '&gt;' }

function escapeHtml(text: string): string {
  return text.replace(/[&<>]/g, (c) => ESCAPES[c] ?? c)
}

function span(kind: string, escaped: string): string {
  return `<span class="tok-${kind}">${escaped}</span>`
}

/** 行内标记。参数必须是**已转义**的文本。 */
const INLINE =
  /(`[^`\n]+`)|(!?\[[^\]\n]*\]\([^)\n]*\))|(\*\*[^*\n]+\*\*)|(\*[^*\n]+\*)|(_[^_\n]+_)/g

function inline(escaped: string): string {
  return escaped.replace(INLINE, (match) => {
    if (match.startsWith('`')) return span('code', match)
    if (match.startsWith('[') || match.startsWith('![')) {
      const split = match.indexOf('](')
      // 正则已经保证形如 [label](url)，这里只是把两半分开着色
      if (split === -1) return span('link', match)
      return span('link', match.slice(0, split + 1)) + span('url', match.slice(split + 1))
    }
    if (match.startsWith('**')) return span('strong', match)
    return span('em', match)
  })
}

/** 超过这个长度就不着色：整篇重新扫描的开销开始能被察觉，而收益接近零。 */
const MAX_LENGTH = 200_000

/**
 * 把源文转成带 `<span>` 的 HTML，行数与原文一一对应。
 *
 * 行数必须一致，否则镜像与 `textarea` 的滚动会错位。
 */
export function highlight(raw: string, format: SourceFormat = 'markdown'): string {
  if (raw.length > MAX_LENGTH) return escapeHtml(raw)

  const out: string[] = []
  let inFence = false
  let frontMatter: 'none' | 'open' | 'done' = 'none'

  raw.split('\n').forEach((line, index) => {
    const escaped = escapeHtml(line)
    const trimmed = line.trimEnd()

    // front matter 只认文件开头的 +++，正文中间的 +++ 是普通文本
    if (index === 0 && trimmed === '+++') {
      frontMatter = 'open'
      out.push(span('fence', escaped))
      return
    }
    if (frontMatter === 'open') {
      if (trimmed === '+++') {
        frontMatter = 'done'
        out.push(span('fence', escaped))
      } else {
        out.push(highlightFrontMatterLine(line))
      }
      return
    }

    // HTML 站点的正文里没有围栏代码块与 Markdown 标记，按标签着色
    if (format === 'html') {
      out.push(highlightHtmlLine(line))
      return
    }

    if (/^\s*(```|~~~)/.test(line)) {
      inFence = !inFence
      out.push(span('fence', escaped))
      return
    }
    if (inFence) {
      out.push(span('code', escaped))
      return
    }

    if (/^#{1,6}\s/.test(line)) {
      out.push(span('heading', escaped))
      return
    }
    if (/^\s*>/.test(line)) {
      out.push(span('quote', inline(escaped)))
      return
    }
    if (/^\s*(-{3,}|\*{3,}|_{3,})\s*$/.test(line)) {
      out.push(span('marker', escaped))
      return
    }

    const list = line.match(/^(\s*(?:[-*+]|\d+\.)\s)(.*)$/)
    if (list) {
      out.push(span('marker', escapeHtml(list[1])) + inline(escapeHtml(list[2])))
      return
    }

    out.push(inline(escaped))
  })

  // 末尾补一行：`textarea` 在最后一行滚动时会多留一行高度，镜像要跟上
  return `${out.join('\n')}\n`
}

/** front matter 里区分键与值，找错字段名时省一次瞪眼。 */
function highlightFrontMatterLine(line: string): string {
  const pair = line.match(/^(\s*[A-Za-z0-9_.-]+\s*=\s*)(.*)$/)
  if (!pair) return span('fm', escapeHtml(line))
  return span('fm-key', escapeHtml(pair[1])) + span('fm-value', escapeHtml(pair[2]))
}

/**
 * HTML 正文的一行：标签与注释着色，文字保持本色。
 *
 * 逐字符扫**原文**而不是先转义再用正则：转义之后 `<` 变成 `&lt;`，
 * 任何「找标签」的正则都会失效，而在转义后的文本里重新拼 `&lt;` 只会把边界搞乱。
 * 跨行的标签或注释按「到行尾为止」处理——镜像要的是看清结构，不是解析文档。
 */
function highlightHtmlLine(line: string): string {
  let out = ''
  let at = 0
  while (at < line.length) {
    const open = line.indexOf('<', at)
    if (open === -1) {
      out += escapeHtml(line.slice(at))
      break
    }
    out += escapeHtml(line.slice(at, open))

    if (line.startsWith('<!--', open)) {
      const end = line.indexOf('-->', open)
      const stop = end === -1 ? line.length : end + 3
      out += span('comment', escapeHtml(line.slice(open, stop)))
      at = stop
      continue
    }

    const close = line.indexOf('>', open)
    const stop = close === -1 ? line.length : close + 1
    out += highlightTag(line.slice(open, stop))
    at = stop
  }
  return out
}

/** 一个标签内部：属性名与引号里的值各自着色，其余（尖括号与标签名）算语句。 */
function highlightTag(raw: string): string {
  const ATTR = /([A-Za-z_:][\w:.-]*)(\s*=\s*)("[^"]*"|'[^']*')/g
  let out = ''
  let last = 0
  for (const match of raw.matchAll(ATTR)) {
    const at = match.index ?? 0
    out += span('stmt', escapeHtml(raw.slice(last, at)))
    out +=
      span('attr', escapeHtml(match[1])) +
      span('stmt', escapeHtml(match[2])) +
      span('string', escapeHtml(match[3]))
    last = at + match[0].length
  }
  out += span('stmt', escapeHtml(raw.slice(last)))
  return out
}
