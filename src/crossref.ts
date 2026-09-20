/**
 * 站内互链：把「链到另一篇」做成挑一下就好的事。
 *
 * 一本书里最常见的动作之一是交叉引用（「见第三章」）。以前这里只有 `Ctrl+K`，
 * 插出来是 `[文字](https://)`——想链到站内的某一篇，得自己记住它的地址，
 * 或者切到列表去看一眼再切回来手打。手打的地址就是死链的来源，
 * 而死链只能等到「体检」页才被发现。
 *
 * 这个文件只放**纯函数**：候选筛选与代码片段拼接。挑选界面在
 * `components/LinkDialog.vue`，插入落点在编辑器——纯函数才好逐条钉住那些
 * 一眼看不出来的转义规则。
 */
import type { SourceFormat } from './source-highlight'

/**
 * 一个可链接的目标。
 *
 * 刻意只声明用得上的四个字段、且都是 `readonly`：`store.project.pages` 是深只读的
 * （`reactive` + `readonly`），直接写 `PageSummary[]` 会在传参处被类型拒掉，
 * 而这里本来也不需要模板名、日期那些。`PageSummary` 结构上满足它。
 */
export interface LinkTarget {
  readonly source: string
  readonly title: string
  readonly url: string
  readonly draft: boolean
}

/**
 * 候选上限。
 *
 * 千篇站点全铺出来只会让人滚不到底，而人本来就该继续输入来缩小范围。
 * 挑这个数是因为它明显多于「一屏」，不至于让人以为候选被砍掉了。
 */
export const LINK_LIMIT = 50

/**
 * 按关键词筛出可链接的页面。
 *
 * 匹配规则与命令面板刻意一致（见 docs/ui.md）：**只做子串、不做模糊打分**，
 * 命中顺序就是站点顺序。打分排序是玄学，用户会怀疑「是不是漏了一篇」。
 * 标题、地址、源路径三者都参与匹配：记得住哪个就用哪个。
 *
 * `exclude` 用来去掉当前正在编辑的那一篇：链到自己几乎总是手滑。
 */
export function matchTargets<T extends LinkTarget>(
  pages: readonly T[],
  query: string,
  options: { exclude?: string } = {},
): T[] {
  const needle = query.trim().toLowerCase()
  const out: T[] = []
  for (const page of pages) {
    if (page.source === options.exclude) continue
    if (needle) {
      const hay = `${page.title}\n${page.url}\n${page.source}`.toLowerCase()
      if (!hay.includes(needle)) continue
    }
    out.push(page)
    if (out.length === LINK_LIMIT) break
  }
  return out
}

/** Markdown 的链接文字里，方括号会当场把语法断掉。 */
function escapeLinkText(text: string): string {
  return text.replace(/([[\]])/g, '\\$1')
}

/**
 * 地址里有空格或括号时用 `<>` 包住。
 *
 * `[图](/a (1).png)` 里的第一个 `)` 就把链接提前收尾了，剩下的字漏进正文。
 * 尖括号形式是 CommonMark 的标准写法，比逐字符百分号编码更不容易看错。
 */
function wrapUrl(url: string): string {
  return /[\s()]/.test(url) ? `<${url}>` : url
}

function escapeHtml(text: string): string {
  return text
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
}

/**
 * 拼出一条链接。
 *
 * 跟着站点的正文格式走：HTML 站点里插 Markdown 语法，渲染出来就是一行
 * 带方括号的纯文本——用户会以为链接功能坏了。着色器已经按这个字段分派
 * （`build.source_format`），插入也该分派。
 */
export function linkSnippet(text: string, url: string, format: SourceFormat): string {
  if (format === 'html') return `<a href="${escapeHtml(url)}">${escapeHtml(text)}</a>`
  return `[${escapeLinkText(text)}](${wrapUrl(url)})`
}

/** 栏目里做菜单目标要用到的那几个字段。 */
export interface SectionTarget {
  readonly path: string
  readonly title: string
  readonly url: string
  /** 栏目的索引页。为 `null` 表示这个栏目**没有**首页 */
  readonly index_source: string | null
}

/**
 * 导航菜单项可以指向的站内目标：**栏目在前，页面在后**。
 *
 * 栏目在前是因为菜单项要指的多半是栏目（「文章」「关于」这种入口），
 * 而站点里页面远多于栏目——按页面排在前面，想挑栏目得先翻半天。
 *
 * **没有索引页的栏目不给选**：那个地址本身就是 404（`build.rs` 往模板注入
 * `sections` 时同样滤掉了它们），挑了就是在每一页的导航上埋一条死链。
 * 手打地址正是死链的来源，而死链只能等「体检」才被发现——所以这里只给能挑的。
 *
 * 返回的是 `LinkTarget`，可以直接喂给 `matchTargets` 与 `LinkDialog`：
 * 挑地址这件事不该有第二套搜索规则。栏目没有草稿的概念，一律 `draft: false`。
 */
export function menuTargets(
  pages: readonly LinkTarget[],
  sections: readonly SectionTarget[],
): LinkTarget[] {
  const fromSections = sections
    .filter((section) => section.index_source)
    .map((section) => ({
      source: section.index_source as string,
      // 栏目没写标题就用目录名：空白项挑不出所以然
      title: section.title.trim() || section.path,
      url: section.url,
      draft: false,
    }))
  return [...fromSections, ...pages]
}

