/**
 * 模板源码着色（HTML + Tera）。
 *
 * 与 Markdown 着色同一套机制（`textarea` 透明 + 背后的高亮镜像），但规则不同：
 * 模板里最容易看错的是「这段是 HTML 还是 Tera」——`{% %}`、`{{ }}`、`{# #}`
 * 三种定界符长得像，混在标签里更难分。所以模板着色优先把 Tera 从 HTML 里挑出来。
 *
 * 与 Markdown 着色不同，这里在**原始文本**上分词、逐段转义后输出：
 * HTML 标签本身就以 `<` 开头，先转义会让分词无从下手。
 *
 * 安全：结果会经 `v-html` 插入，所以每段文本都调 `escapeHtml`，
 * 只有本文件产生的 `<span class="tok-*">` 是标签。
 */

const ESCAPES: Record<string, string> = { '&': '&amp;', '<': '&lt;', '>': '&gt;' }

function escapeHtml(text: string): string {
  return text.replace(/[&<>]/g, (c) => ESCAPES[c] ?? c)
}

function span(kind: string, raw: string): string {
  return `<span class="tok-${kind}">${escapeHtml(raw)}</span>`
}

/** Tera 注释、语句、表达式，HTML 注释，HTML 标签。顺序即优先级。 */
const TOKEN =
  /\{#[\s\S]*?#\}|\{%[\s\S]*?%\}|\{\{[\s\S]*?\}\}|<!--[\s\S]*?-->|<!\w[^>]*>|<\/?[A-Za-z][^>]*>/g

/** 超过这个长度就不着色，理由同 Markdown 着色。 */
const MAX_LENGTH = 200_000

/** 把模板源码转成带 `<span>` 的 HTML，字符位置与原文一一对应。 */
export function highlight(source: string): string {
  if (source.length > MAX_LENGTH) return escapeHtml(source)

  let out = ''
  let last = 0
  for (const match of source.matchAll(TOKEN)) {
    const raw = match[0]
    const at = match.index
    out += escapeHtml(source.slice(last, at))
    out += token(raw)
    last = at + raw.length
  }
  out += escapeHtml(source.slice(last))
  // 末尾补一行，理由同 Markdown 着色：textarea 滚到底时会多留一行
  return `${out}\n`
}

function token(raw: string): string {
  if (raw.startsWith('{#') || raw.startsWith('<!--')) return span('comment', raw)
  if (raw.startsWith('{%')) return statement(raw)
  if (raw.startsWith('{{')) return span('expr', raw)
  return tag(raw)
}

/** `{% for x in y %}`：定界符与关键字分开，扫结构时靠关键字定位。 */
function statement(raw: string): string {
  const parts = raw.match(/^(\{%-?\s*)(\w+)([\s\S]*)$/)
  if (!parts) return span('stmt', raw)
  return span('stmt', parts[1]) + span('keyword', parts[2]) + span('stmt', parts[3])
}

/** `<a href="…" class="…">`：标签名、属性名、属性值分色。 */
const ATTR = /([A-Za-z_:][-\w:.]*)(\s*=\s*)("[^"]*"|'[^']*')?/g

function tag(raw: string): string {
  const head = raw.match(/^(<\/?)([A-Za-z][-\w:.]*)([\s\S]*?)(\/?>)$/)
  if (!head) return span('tagname', raw)

  let attrs = ''
  let last = 0
  for (const match of head[3].matchAll(ATTR)) {
    attrs += escapeHtml(head[3].slice(last, match.index))
    attrs += span('attr', match[1]) + escapeHtml(match[2])
    if (match[3]) attrs += span('string', match[3])
    last = match.index + match[0].length
  }
  attrs += escapeHtml(head[3].slice(last))

  return escapeHtml(head[1]) + span('tagname', head[2]) + attrs + escapeHtml(head[4])
}
