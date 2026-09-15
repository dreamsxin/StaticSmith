import { describe, expect, it } from 'vitest'

import { groupBySection, matchesFilter, worstSeoBySource } from './grouping'
import type { Criteria, GroupablePage } from './grouping'

/**
 * 内容列表的筛选与分组。
 *
 * 这里最容易出**静默错误**：一篇文章因为条件写错而不出现在列表里，界面不报任何错，
 * 用户只会以为它丢了。所以每条规则都单独钉一次。
 */
function page(source: string, extra: Partial<GroupablePage> = {}): GroupablePage {
  return {
    source,
    title: source,
    section: source.includes('/') ? source.slice(0, source.lastIndexOf('/')) : '',
    draft: false,
    scheduled: false,
    ...extra,
  }
}

function criteria(extra: Partial<Criteria> = {}): Criteria {
  return {
    keyword: '',
    filter: 'all',
    dirty: new Set<string>(),
    seo: new Set<string>(),
    ...extra,
  }
}

describe('matchesFilter', () => {
  it('「全部」放过一切', () => {
    expect(matchesFilter(page('posts/a.md', { draft: true }), criteria())).toBe(true)
  })

  it('草稿、定时看的是文章自己的状态', () => {
    const draft = page('posts/a.md', { draft: true })
    const timed = page('posts/b.md', { scheduled: true })

    expect(matchesFilter(draft, criteria({ filter: 'draft' }))).toBe(true)
    expect(matchesFilter(timed, criteria({ filter: 'draft' }))).toBe(false)
    expect(matchesFilter(timed, criteria({ filter: 'scheduled' }))).toBe(true)
  })

  it('待生成与待补 SEO 看的是外面给的两份清单', () => {
    const one = page('posts/a.md')
    expect(matchesFilter(one, criteria({ filter: 'dirty', dirty: new Set(['posts/a.md']) }))).toBe(
      true,
    )
    expect(matchesFilter(one, criteria({ filter: 'dirty' }))).toBe(false)
    expect(matchesFilter(one, criteria({ filter: 'seo', seo: new Set(['posts/a.md']) }))).toBe(true)
  })
})

describe('groupBySection', () => {
  const pages = [page('posts/a.md'), page('posts/b.md'), page('notes/c.md'), page('about.md')]
  const sections = [
    { path: 'posts', weight: 1 },
    { path: 'notes', weight: 2 },
    { path: 'empty', weight: 3 },
  ]

  it('按栏目分组，根目录的文章归到空栏目名下', () => {
    const groups = groupBySection(pages, sections, criteria())
    const map = new Map(groups)
    expect(map.get('posts')?.map((p) => p.source)).toEqual(['posts/a.md', 'posts/b.md'])
    expect(map.get('')?.map((p) => p.source)).toEqual(['about.md'])
  })

  it('空栏目也要摆出来：新建完一个栏目不该看起来像没建成', () => {
    const groups = groupBySection(pages, sections, criteria())
    expect(groups.map(([name]) => name)).toContain('empty')
    expect(new Map(groups).get('empty')).toEqual([])
  })

  it('搜索或筛选时不补空栏目：那时要的是命中项，不是完整结构', () => {
    const searched = groupBySection(pages, sections, criteria({ keyword: 'a.md' }))
    expect(searched.map(([name]) => name)).not.toContain('empty')

    const filtered = groupBySection(pages, sections, criteria({ filter: 'draft' }))
    expect(filtered.map(([name]) => name)).not.toContain('empty')
  })

  it('栏目按索引页 weight 排，同权重按路径——顺序不能看起来随机', () => {
    const groups = groupBySection(pages, sections, criteria())
    // posts(1) → notes(2) → empty(3)，根目录（weight 0）在最前
    expect(groups.map(([name]) => name)).toEqual(['', 'posts', 'notes', 'empty'])

    const flat = groupBySection(pages, [], criteria())
    expect(flat.map(([name]) => name)).toEqual(['', 'notes', 'posts'])
  })

  it('搜索命中标题或路径，且不分大小写', () => {
    const items = [page('posts/hello.md', { title: '你好 World' }), page('posts/other.md')]

    expect(groupBySection(items, [], criteria({ keyword: 'WORLD' })).flatMap(([, p]) => p)).toEqual([
      items[0],
    ])
    // 想不起标题但记得放在哪：路径也该命中
    expect(
      groupBySection(items, [], criteria({ keyword: 'posts/other' })).flatMap(([, p]) => p),
    ).toEqual([items[1]])
  })

  it('搜索词首尾空白不算条件：多打一个空格不该把结果清空', () => {
    const items = [page('posts/hello.md', { title: '你好' })]
    expect(groupBySection(items, [], criteria({ keyword: '  你好 ' })).length).toBe(1)
  })

  it('标题结尾与路径开头不许凑出假命中', () => {
    const items = [page('posts/b.md', { title: '甲' })]
    // 「甲posts」只有在两段直接相连时才会命中——中间那个换行就是为了拦它
    expect(groupBySection(items, [], criteria({ keyword: '甲posts' })).length).toBe(0)
  })

  it('搜索与筛选是「且」的关系', () => {
    const items = [
      page('posts/a.md', { title: '草稿甲', draft: true }),
      page('posts/b.md', { title: '草稿乙' }),
    ]
    const found = groupBySection(items, [], criteria({ keyword: '草稿', filter: 'draft' })).flatMap(
      ([, p]) => p,
    )
    expect(found.map((p) => p.source)).toEqual(['posts/a.md'])
  })
})

describe('worstSeoBySource', () => {
  it('一篇有多条问题时留最重的那条，但原文全给', () => {
    const worst = worstSeoBySource([
      { source: 'posts/a.md', severity: 'hint', message: '标题偏短' },
      { source: 'posts/a.md', severity: 'error', message: '缺描述' },
      { source: 'posts/a.md', severity: 'warn', message: '关键词过多' },
    ])

    const entry = worst.get('posts/a.md')!
    expect(entry.severity).toBe('error')
    // 补描述时想看的是「还差什么」，而不是「最严重的是什么」
    expect(entry.messages).toEqual(['标题偏短', '缺描述', '关键词过多'])
  })

  it('站点级问题不算在任何一篇头上：它们要去「设置」里改', () => {
    const worst = worstSeoBySource([{ source: '', severity: 'error', message: '站点缺 base_url' }])
    expect(worst.size).toBe(0)
  })

  it('认不出的程度不许压过 error', () => {
    const worst = worstSeoBySource([
      { source: 'a.md', severity: 'error', message: '缺描述' },
      { source: 'a.md', severity: '未来新增的等级', message: '别的' },
    ])
    expect(worst.get('a.md')!.severity).toBe('error')
  })
})
