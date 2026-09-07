/**
 * Markdown 源文着色。
 *
 * 编辑器保持「源文是唯一真相」：不引入富文本模型，只在 `textarea` 背后垫一层
 * 只读的高亮镜像，让标记本身看得清——标题、代码、链接、front matter 各有其色，
 * 但你改的仍然是纯文本。
 *
 * 为什么自己写而不引入 CodeMirror/Prism：编辑器只需要「看清结构」，
 * 不需要折叠、补全、语言服务。为此塞进几百 KB 依赖并不值得，
 * 而且高亮的规则必须与站点实际支持的 Markdown 子集一致。
 *
 * 安全：结果会经 `v-html` 插入，所以**一切文本都先转义**，
 * 只有本文件产生的 `<span class="tok-*">` 是标签。
 */

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
export function highlight(raw: string): string {
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
