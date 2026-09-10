/**
 * 界面文案里那些「同一个东西在多处出现」的名字。
 *
 * 为什么单独一个文件：产物类型此前有两套叫法——「生成」页说「标签页 / 分页页」，
 * 内容侧栏的「站点页面（生成）」组说「标签 / 分页」。指的是同一批文件，
 * 名字却不一样，用户会以为是两种东西（见 docs/ui-review.md 第一条）。
 *
 * 规则：任何会在两个面板里露面的名字都放这里，各面板不再各写一遍三元表达式。
 */
import type { FtpOverwrite, OutputKind } from './api'

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

/**
 * FTP「远端已存在同名文件时」的规则名与它的后果。
 *
 * 措辞照 FileZilla 的「文件已存在时」那一栏：用过 FTP 客户端的人一眼就认得，
 * 而这几条规则本来就是同一个问题的同一组答案。选项名说「做什么」，
 * 副标题说「什么时候选它」——只写规则名的话，`size` 与 `newer` 的区别看不出来。
 */
const FTP_OVERWRITE: Record<FtpOverwrite, { label: string; hint: string }> = {
  size_or_newer: {
    label: '大小不同或本地更新时覆盖',
    hint: '默认。两项材料都用，适合服务器时间可信的情况',
  },
  always: {
    label: '总是覆盖',
    hint: '不比对，全量重传。服务器时钟不准或不给时间戳时选它——最慢但最确定',
  },
  newer: { label: '仅本地更新时覆盖', hint: '只看时间。改动不一定改变长度时选它' },
  size: { label: '仅大小不同时覆盖', hint: '只看大小。服务器时间不可信时选它' },
  skip: { label: '已存在就跳过', hint: '只补新文件，不动线上已有的' },
}

export function ftpOverwriteLabel(rule: FtpOverwrite): string {
  return FTP_OVERWRITE[rule]?.label ?? rule
}

export function ftpOverwriteOptions(): { value: FtpOverwrite; label: string; hint: string }[] {
  return (Object.keys(FTP_OVERWRITE) as FtpOverwrite[]).map((value) => ({
    value,
    ...FTP_OVERWRITE[value],
  }))
}
