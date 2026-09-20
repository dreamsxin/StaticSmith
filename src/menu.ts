/**
 * 导航菜单的纯逻辑。
 *
 * 菜单是**配置**而不是内容（理由在 `docs/configuration.md`），但它渲染在每一页的头部，
 * 所以一个错地址就是全站死链——而死链只能等「体检」才被发现，那已经是生成之后。
 * 这里两个函数都是为了把问题提前到「编辑菜单的那一刻」。
 *
 * 抽成纯函数是因为判断规则（末尾斜杠、锚点、站外链接）全是边界情况，
 * 留在组件里只能靠肉眼。
 */
import type { LinkTarget, SectionTarget } from './crossref'

/** 菜单项里这里只关心地址。 */
interface UrlOnly {
  readonly url: string
}

/**
 * 比较地址时先规整：**末尾斜杠不算差异**。
 *
 * 产物里 `/about` 与 `/about/` 打开的是同一个 `index.html`（`links.rs` 的死链体检
 * 也是这么宽容的），把它们当两个地址会报出一堆假问题。锚点一并去掉：
 * `/about/#history` 指的还是 `/about/`。
 */
function normalize(url: string): string {
  const withoutHash = url.split('#')[0].trim()
  return withoutHash.length > 1 ? withoutHash.replace(/\/+$/, '') : withoutHash
}

/** 站外地址（它的正确性不在这个站里，管不了也不该管）。 */
function isExternal(url: string): boolean {
  return /^(https?:)?\/\//.test(url) || url.startsWith('mailto:')
}

/**
 * 还没出现在菜单里的栏目。
 *
 * 新建一个栏目之后最容易忘的就是「去导航里加一行」——文档里专门写了这一步，
 * 说明它确实要靠人记。这里把「还缺哪些」算出来，界面可以一键补齐。
 *
 * 没有索引页的栏目不算：那个地址本身是 404（`menuTargets` 同样滤掉它们）。
 */
export function missingSections(
  menu: readonly UrlOnly[],
  sections: readonly SectionTarget[],
): SectionTarget[] {
  const taken = new Set(menu.map((item) => normalize(item.url)))
  return sections.filter(
    (section) => section.index_source && !taken.has(normalize(section.url)),
  )
}

/**
 * 菜单里指向站内、但站内找不到的地址。
 *
 * 只报**站内**的：站外链接与纯锚点的正确性不在这个站里。空地址也不报——那是还没填完的
 * 行，保存时 `MenuItem::validate` 自会拦下，在这里再喊一遍只是噪音。
 *
 * **首页也要真有 `index.md` 才算存在**：没有它，站点根就是 404，报出来是对的。
 * core 侧（`menu.rs`）拿产物地址做同一件事，两边不该有一边格外宽容。
 *
 * 返回的是**去重后**的地址列表：同一个错地址报两遍不会让人更快改对。
 */
export function unknownMenuUrls(
  menu: readonly UrlOnly[],
  targets: readonly LinkTarget[],
): string[] {
  const known = new Set(targets.map((item) => normalize(item.url)))
  const bad = new Set<string>()
  for (const item of menu) {
    const url = item.url.trim()
    if (!url || isExternal(url) || url.startsWith('#')) continue
    if (!known.has(normalize(url))) bad.add(item.url)
  }
  return [...bad]
}
