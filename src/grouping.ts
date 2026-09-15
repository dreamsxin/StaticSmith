/**
 * 内容列表的筛选、分组与排序。
 *
 * 从 `PageList.vue` 搬出来的**纯逻辑**：不碰 DOM、不读 store，只把「所有文章 + 一组条件」
 * 变成「按栏目分好的几组」。搬出来的理由是这里最容易出**静默错误**——一篇文章因为条件
 * 写错而不出现在列表里，界面不会报任何错，用户只会以为它丢了。而它本来就不需要 DOM，
 * 挂在组件里反而测不动。
 */

/** 运营视角的筛选。体检面板给结论，这一排把结论接回工作列表。 */
export type Filter = 'all' | 'draft' | 'dirty' | 'seo' | 'scheduled'

/** 分组只用得到这几个字段。声明成只读：store 导出的状态是深只读的。 */
export interface GroupablePage {
  readonly source: string
  readonly title: string
  readonly section: string
  readonly draft: boolean
  readonly scheduled: boolean
}

/** 栏目在排序里只用得到路径与权重。 */
export interface GroupableSection {
  readonly path: string
  readonly weight: number
}

/** 一次筛选的全部条件。 */
export interface Criteria {
  /** 搜索词。大小写与首尾空白由这里统一处理，调用方不必先规整 */
  readonly keyword: string
  readonly filter: Filter
  /** 待重新生成的源文件 */
  readonly dirty: ReadonlySet<string>
  /** 有 SEO 问题的源文件 */
  readonly seo: ReadonlySet<string>
}

/** 一条 SEO 问题里分组要用到的部分。 */
export interface SeoLike {
  readonly source: string
  readonly severity: string
  readonly message: string
}

/** 某一篇最严重的那条问题，以及它全部问题的原文。 */
export interface WorstSeo {
  severity: string
  messages: string[]
}

/** error 最重。数字小的赢，与 `seo.rs` 里 `Severity` 的顺序一致。 */
const RANK: Record<string, number> = { error: 0, warn: 1, hint: 2 }

/**
 * 每篇文章最严重的那条 SEO 问题。
 *
 * 站点级问题（`source` 为空）自然被排除：它们要去「设置」里改，不属于某一篇。
 * 徽标只有一个，所以要挑最重的那条；但提示里给全部原文——补描述时想看的是「还差什么」，
 * 而不是「最严重的是什么」。
 */
export function worstSeoBySource(issues: readonly SeoLike[]): Map<string, WorstSeo> {
  const worst = new Map<string, WorstSeo>()
  for (const issue of issues) {
    if (!issue.source) continue
    const current = worst.get(issue.source)
    if (!current) {
      worst.set(issue.source, { severity: issue.severity, messages: [issue.message] })
      continue
    }
    current.messages.push(issue.message)
    if ((RANK[issue.severity] ?? 9) < (RANK[current.severity] ?? 9)) {
      current.severity = issue.severity
    }
  }
  return worst
}

/** 这一篇过不过筛选（不含搜索词）。 */
export function matchesFilter(page: GroupablePage, criteria: Criteria): boolean {
  switch (criteria.filter) {
    case 'draft':
      return page.draft
    case 'dirty':
      return criteria.dirty.has(page.source)
    case 'seo':
      return criteria.seo.has(page.source)
    case 'scheduled':
      return page.scheduled
    default:
      return true
  }
}

/**
 * 搜索词命中标题或源文件路径。
 *
 * 路径也算：一篇文章的标题可能想不起来，但「在 posts/2026 里」往往记得。
 * 两者拼起来一次判断，中间用换行隔开，免得「标题结尾 + 路径开头」凑出假命中。
 */
function matchesKeyword(page: GroupablePage, keyword: string): boolean {
  if (!keyword) return true
  return `${page.title}\n${page.source}`.toLowerCase().includes(keyword)
}

/**
 * 按栏目分组，并按「索引页 weight，然后路径」排序。
 *
 * 两条容易被改坏的规则：
 *
 * - **空栏目也要摆出来**，否则新建完一个栏目它就「消失」了。但只在既没搜索也没筛选时补
 *   ——那时用户要的是完整结构；搜索时补空栏目等于在结果里塞进一堆噪音。
 * - 栏目顺序跟着索引页的 `weight`，与站点上列出的顺序一致；没排过序的（weight 0）
 *   按路径排，免得顺序看起来随机。
 */
export function groupBySection<T extends GroupablePage>(
  pages: readonly T[],
  sections: readonly GroupableSection[],
  criteria: Criteria,
): Array<[string, T[]]> {
  const keyword = criteria.keyword.trim().toLowerCase()
  const map = new Map<string, T[]>()


  if (!keyword && criteria.filter === 'all') {
    for (const section of sections) map.set(section.path, [])
  }

  for (const page of pages) {
    if (!matchesKeyword(page, keyword)) continue
    if (!matchesFilter(page, criteria)) continue
    const list = map.get(page.section) ?? []
    list.push(page)
    map.set(page.section, list)
  }

  const weightOf = (path: string) => sections.find((s) => s.path === path)?.weight ?? 0
  return [...map.entries()].sort(([a], [b]) => weightOf(a) - weightOf(b) || a.localeCompare(b))
}
