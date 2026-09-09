/**
 * 界面文案里那些「同一个东西在多处出现」的名字。
 *
 * 为什么单独一个文件：产物类型此前有两套叫法——「生成」页说「标签页 / 分页页」，
 * 内容侧栏的「站点页面（生成）」组说「标签 / 分页」。指的是同一批文件，
 * 名字却不一样，用户会以为是两种东西（见 docs/ui-review.md 第一条）。
 *
 * 规则：任何会在两个面板里露面的名字都放这里，各面板不再各写一遍三元表达式。
 */
import type { OutputKind } from './api'

/**
 * 产物类型的显示名。
 *
 * 一律带「页」字：这些是**生成出来的页面**，不是「标签」这个概念本身——
 * 侧栏里说「标签」会和文章的标签字段混在一起。
 */
const OUTPUT_KIND: Record<OutputKind, string> = {
  page: '内容页',
  pagination: '分页页',
  taxonomy: '标签页',
  sitemap: '站点地图',
  feed: '订阅源',
  asset: '静态资源',
}

export function outputKindLabel(kind: OutputKind): string {
  return OUTPUT_KIND[kind] ?? kind
}
