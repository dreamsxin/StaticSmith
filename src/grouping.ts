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
  /** 人排过的顺序，0 表示没排过 */
  readonly weight: number
  /** RFC3339，没写日期时为 null */
  readonly date: string | null
}

/** 栏目在排序里只用得到路径与权重。 */
export interface GroupableSection {
  readonly path: string
  readonly weight: number
}

/** 侧栏里的一组：一个栏目及它直属的文章。 */
export interface SectionGroup<T> {
  /** 栏目路径，根目录是空串。 */
  readonly section: string
  /** 缩进层级。根目录与顶层栏目都是 0，`posts/2026` 是 1。 */
  readonly depth: number
  /**
   * 它的父栏目**也在这份清单里**（就排在它上面）。
   *
   * 为真时名字只显示末段（`2026`）——缩进已经说明它属于谁。为假时必须显示完整路径：
   * 筛选或搜索会把没有命中的父栏目整组去掉，那时孤零零一个「2026」看不出是谁的。
   */
  readonly parentShown: boolean
  readonly pages: T[]
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
 * 读者看到的顺序：weight 升序 → 日期降序 → 标题升序。
 *
 * **这是 Rust 侧 `content::reading_order` 的孪生实现**，两边各有测试钉住同样的例子
 * （同 `skips.ts` ↔ `skips.rs` 的做法）。为什么要有两份：顺序既要在生成产物时用，
 * 也要在界面上列出来，而界面拿不到 Rust 的比较器。两边不一致的下场是
 * 「界面上第 3 篇、网站上第 7 篇」——那时侧栏就不再是目录，只是个文件夹。
 *
 * 日期用 `Date.parse` 比而不是比字符串：RFC3339 允许不同时区偏移，
 * `2026-01-01T00:00:00+08:00` 与 `2025-12-31T20:00:00Z` 是同一刻，字符串比会判反。
 * 没写日期的排在有日期的后面（与 Rust 里 `None < Some` 在降序下的效果一致）。
 *
 * 标题这一级只为让结果稳定，用码位序而不是 `localeCompare`：后者按语言习惯排，
 * 与 Rust 的字节序差得更远。固化过顺序之后 weight 各不相同，这一级根本不会走到。
 */
export function readingOrder(a: GroupablePage, b: GroupablePage): number {
  if (a.weight !== b.weight) return a.weight - b.weight
  const at = a.date ? Date.parse(a.date) : null
  const bt = b.date ? Date.parse(b.date) : null
  if (at !== bt) {
    if (at === null) return 1
    if (bt === null) return -1
    return bt - at
  }
  return a.title < b.title ? -1 : a.title > b.title ? 1 : 0
}

/**
 * 把某一篇在它那一栏里挪一位，返回**整栏**的新顺序（源文件路径）。
 *
 * 已经在头 / 尾时返回 `null`：调用方据此把菜单项置灰，而不是发一次什么也不改的写操作。
 *
 * 为什么返回整栏而不是「把 X 挪到第 3 位」：顺序是一件整体的事。新站点里每篇的
 * weight 都是 0，"往上挪一位" 在那种状态下无从表达（交换两个 0 什么也没变），
 * 所以核心要的是整栏的顺序，由这里算出来——它也是唯一知道「现在看到的顺序」的地方。
 */
export function moved(
  pages: readonly GroupablePage[],
  source: string,
  delta: -1 | 1,
): string[] | null {
  const ordered = [...pages].sort(readingOrder)
  const at = ordered.findIndex((page) => page.source === source)
  const to = at + delta
  if (at < 0 || to < 0 || to >= ordered.length) return null
  const swapped = ordered.map((page) => page.source)
  ;[swapped[at], swapped[to]] = [swapped[to], swapped[at]]
  return swapped
}

/**
 * 同一层栏目之间的顺序：`weight` 升序，再按路径。
 *
 * 与站点上 `sections` 列出的顺序一致（Rust 侧 `sections::list` 的结尾就是这么排的）。
 * 注意它与文章的 `readingOrder` **不是**一回事：栏目没有日期可比。
 */
export function siblingOrder(a: GroupableSection, b: GroupableSection): number {
  if (a.weight !== b.weight) return a.weight - b.weight
  return a.path < b.path ? -1 : a.path > b.path ? 1 : 0
}

/**
 * 与某个栏目同一层的全部栏目（含它自己）。
 *
 * 根目录返回空数组：它没有同级，也不该出现「上移根目录」这种菜单项。
 */
export function siblingsOf<T extends GroupableSection>(
  sections: readonly T[],
  path: string,
): T[] {
  if (path === '') return []
  const parent = parentOf(path)
  return sections.filter((section) => section.path !== '' && parentOf(section.path) === parent)
}

/**
 * 把一个栏目在它那一层里挪一位，返回**整层**的新顺序。
 *
 * 已经在头 / 尾、或者它根本不在这一层时返回 `null`：调用方据此把菜单项置灰，
 * 而不是发一次什么也不改的写操作。
 *
 * 与 [`moved`] 同构（那个管栏目内的文章）：一本书的目录既要能排章节之间的先后，
 * 也要能排章内小节的先后，两件事不该有两种手感。
 */
export function movedSection(
  siblings: readonly GroupableSection[],
  path: string,
  delta: -1 | 1,
): string[] | null {
  const ordered = [...siblings].sort(siblingOrder)
  const at = ordered.findIndex((section) => section.path === path)
  const to = at + delta
  if (at < 0 || to < 0 || to >= ordered.length) return null
  const swapped = ordered.map((section) => section.path)
  ;[swapped[at], swapped[to]] = [swapped[to], swapped[at]]
  return swapped
}

/**
 * 把一项挪到指定的插入位置，返回新的顺序；没有变化时返回 `null`。
 *
 * `index` 用的是**原数组的坐标**：0 表示「放到最前」，`order.length` 表示「放到最后」，
 * `i` 表示「放到第 i 项之前」。拖走自己之后它后面的位置都会往前挪一格，
 * 这个换算（`index > from ? index - 1 : index`）是拖拽排序最容易写错的一行——
 * 写错的表现是「往后拖一位没反应、拖两位才动」。
 *
 * 返回 `null` 的两种情况：拖回原处（前后邻居之间），以及越界。调用方据此不发写操作。
 */
export function reorderTo(
  order: readonly string[],
  source: string,
  index: number,
): string[] | null {
  const from = order.indexOf(source)
  if (from < 0 || index < 0 || index > order.length) return null
  const rest = order.filter((item) => item !== source)
  rest.splice(index > from ? index - 1 : index, 0, source)
  return rest.every((item, i) => item === order[i]) ? null : rest
}

/**
 * 拖到一行上时，落点算在它前面还是后面。
 *
 * 用中线判断而不是「一律插到前面」：不然「放到最后一位」永远做不到。
 * 抽成纯函数是为了能测——`clientY` 与元素矩形在测试环境里都是 0，
 * 留在组件里这一行就只能靠肉眼。
 */
export function dropSide(clientY: number, top: number, height: number): 'before' | 'after' {
  return clientY < top + height / 2 ? 'before' : 'after'
}

/**
 * 拖到列表上下边缘时，每一帧该滚多少（负数向上，`0` 表示别动）。
 *
 * 不做这个的后果：目录一长，把一篇拖到屏幕外的栏目上就**根本做不到**——
 * 手一直按着，列表不动，只能松手、滚、再拖一次。
 *
 * 两条取舍写在参数默认值里：
 * - `band`（48px）只在贴边那一小条里才滚。带太宽会让人没法安静地停在靠边的那一行
 * - `max`（18px/帧）是上限。指针拖到容器**外面**也不再加速——越界就一路窜到底，
 *   等于把「拖过头」变成「重来一次」
 *
 * 抽成纯函数的理由同 `dropSide`：`clientY` 与元素矩形在测试环境里都是 0，
 * 留在组件里这段只能靠肉眼。
 */
export function edgeScrollStep(
  clientY: number,
  top: number,
  height: number,
  band = 48,
  max = 18,
): number {
  const fromTop = clientY - top
  const fromBottom = top + height - clientY
  const near = Math.min(fromTop, fromBottom)
  if (near >= band) return 0
  // 指针拖到容器**外面**时 `near` 是负数，算出来的速度更大——被 `max` 截住，
  // 所以「拖过头」不会一路窜到底。这一层封顶是刻意的，别改成按距离线性外推
  const speed = Math.min(max, Math.max(1, Math.round((max * (band - near)) / band)))
  // 容器矮到两条带重叠时，靠哪边近就往哪边滚
  return fromTop <= fromBottom ? -speed : speed
}

/**
 * 能不能把 `from` 拖到 `to` 旁边（**同栏目内排序**）。
 *
 * 跨栏目在这条路上仍然不接：那一次拖拽会同时改归属和顺序，
 * 一句话说不清「会发生什么」，而这个工具没有跨文件撤销栈。
 * 改归属走 `canDropIntoSection`——落在**栏目**上，先干跑再确认。
 * 拖到自己身上同理不接。
 */
export function canDropBeside(
  from: GroupablePage | undefined,
  to: GroupablePage,
): from is GroupablePage {
  return !!from && from.source !== to.source && from.section === to.section
}

/** 拖动时会看一眼「是不是索引页」，那是唯一比 `GroupablePage` 多要的一个字段。 */
export interface MovablePage extends GroupablePage {
  /** 栏目的索引页（`_index.md`）。它的地址就是栏目名 */
  readonly is_index?: boolean
}

/**
 * 能不能把 `from` 拖进 `section` 这个栏目（**改归属**）。
 *
 * 这是「像编辑一本书」缺的最后一块：把一章从一部分挪到另一部分。落盘前必须干跑，
 * 因为它要补旧地址、还要改写**别人文章里**指向它的链接——背着人改文件比不改更糟。
 * 所以拖到栏目上只是发起确认，不直接写。
 *
 * 两种情况不接：
 * - 拖回自己所在的栏目：等于没动，不该弹一个干跑让人确认「什么都不会变」
 * - 栏目索引页：它的地址就是栏目名，搬它要走「栏目改名」。core 也会拒
 *   （`batch::MoveDecision::Blocked`），界面先别给出「能拖」的假象——
 *   判断与执行共用一份规矩，否则迟早出现「能拖、松手没反应」
 */
export function canDropIntoSection(
  from: MovablePage | undefined,
  section: string,
): from is MovablePage {
  return !!from && !from.is_index && from.section !== section
}

/** 某个栏目的父栏目路径。顶层栏目的父是根目录（空串）。 */
function parentOf(path: string): string {
  const at = path.lastIndexOf('/')
  return at < 0 ? '' : path.slice(0, at)
}

/**
 * 栏目的排序键：从根到自己每一层的「(权重, 名字)」。
 *
 * 逐层比较就得到**深度优先**的顺序——父栏目紧跟着自己的子栏目，正是一本书目录的样子。
 * 直接按整条路径字符串排是不行的：`posts` 与 `posts-old` 之间会插进 `posts/2026`，
 * 子栏目就跑到别人家里去了。根目录的键是空数组，因此永远排在最前（它是站点的根）。
 */
function sortKey(path: string, weightOf: (path: string) => number): Array<[number, string]> {
  if (path === '') return []
  const parts = path.split('/')
  return parts.map((name, i) => [weightOf(parts.slice(0, i + 1).join('/')), name])
}

/**
 * 按栏目分组并**排成一棵树**：栏目之间深度优先（父栏目紧跟自己的子栏目），
 * 同级之间按索引页 `weight` 再按名字，**组内按阅读顺序**。
 *
 * 四条容易被改坏的规则：
 *
 * - **空栏目也要摆出来**，否则新建完一个栏目它就「消失」了。但只在既没搜索也没筛选时补
 *   ——那时用户要的是完整结构；搜索时补空栏目等于在结果里塞进一堆噪音。
 * - 同级顺序跟着索引页的 `weight`，与站点上列出的顺序一致；没排过序的（weight 0）
 *   按名字排，免得顺序看起来随机。
 * - **子栏目跟在父栏目下面**（`depth` 表达缩进）。这里曾经把所有栏目平铺，
 *   `posts` 与 `posts/2026` 是两个并列的分组——站点结构本来是有层级的，
 *   平铺之后侧栏看着像一堆文件夹，而不像一本书的目录。
 * - **组内按 `readingOrder`**，也就是网站上的顺序。这里曾经直接用后端给的数组顺序，
 *   而那是**源文件名字母序**：`a.md` 永远在 `b.md` 前面，哪怕网站上是倒过来的。
 */
export function groupBySection<T extends GroupablePage>(
  pages: readonly T[],
  sections: readonly GroupableSection[],
  criteria: Criteria,
): Array<SectionGroup<T>> {
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

  for (const list of map.values()) list.sort(readingOrder)

  const weightOf = (path: string) => sections.find((s) => s.path === path)?.weight ?? 0
  const keys = new Map([...map.keys()].map((path) => [path, sortKey(path, weightOf)]))
  const order = [...map.keys()].sort((a, b) => {
    const ka = keys.get(a)!
    const kb = keys.get(b)!
    for (let i = 0; i < Math.min(ka.length, kb.length); i += 1) {
      if (ka[i][0] !== kb[i][0]) return ka[i][0] - kb[i][0]
      if (ka[i][1] !== kb[i][1]) return ka[i][1] < kb[i][1] ? -1 : 1
    }
    // 前缀短的在前：父栏目排在自己的子栏目之上
    return ka.length - kb.length
  })

  return order.map((section) => ({
    section,
    depth: section === '' ? 0 : section.split('/').length - 1,
    parentShown: section.includes('/') && map.has(parentOf(section)),
    pages: map.get(section)!,
  }))
}
