import { describe, expect, it } from 'vitest'

import {
  canDropBeside,
  canDropIntoSection,
  edgeScrollStep,
  dropSide,
  groupBySection,
  matchesFilter,
  moved,
  movedSection,
  readingOrder,
  reorderTo,
  siblingsOf,
  worstSeoBySource,
} from './grouping'
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
    weight: 0,
    date: null,
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

  /** 分组结果里「哪个栏目 → 哪几篇」。 */
  const bySection = <T extends { source: string }>(
    groups: Array<{ section: string; pages: T[] }>,
  ) => new Map(groups.map((g) => [g.section, g.pages]))
  const names = (groups: Array<{ section: string }>) => groups.map((g) => g.section)

  it('按栏目分组，根目录的文章归到空栏目名下', () => {
    const map = bySection(groupBySection(pages, sections, criteria()))
    expect(map.get('posts')?.map((p) => p.source)).toEqual(['posts/a.md', 'posts/b.md'])
    expect(map.get('')?.map((p) => p.source)).toEqual(['about.md'])
  })

  it('空栏目也要摆出来：新建完一个栏目不该看起来像没建成', () => {
    const groups = groupBySection(pages, sections, criteria())
    expect(names(groups)).toContain('empty')
    expect(bySection(groups).get('empty')).toEqual([])
  })

  it('搜索或筛选时不补空栏目：那时要的是命中项，不是完整结构', () => {
    const searched = groupBySection(pages, sections, criteria({ keyword: 'a.md' }))
    expect(names(searched)).not.toContain('empty')

    const filtered = groupBySection(pages, sections, criteria({ filter: 'draft' }))
    expect(names(filtered)).not.toContain('empty')
  })

  it('栏目按索引页 weight 排，同权重按名字——顺序不能看起来随机', () => {
    const groups = groupBySection(pages, sections, criteria())
    // posts(1) → notes(2) → empty(3)，根目录（weight 0）在最前
    expect(names(groups)).toEqual(['', 'posts', 'notes', 'empty'])

    const flat = groupBySection(pages, [], criteria())
    expect(names(flat)).toEqual(['', 'notes', 'posts'])
  })

  it('搜索命中标题或路径，且不分大小写', () => {
    const items = [page('posts/hello.md', { title: '你好 World' }), page('posts/other.md')]

    expect(
      groupBySection(items, [], criteria({ keyword: 'WORLD' })).flatMap((g) => g.pages),
    ).toEqual([items[0]])
    // 想不起标题但记得放在哪：路径也该命中
    expect(
      groupBySection(items, [], criteria({ keyword: 'posts/other' })).flatMap((g) => g.pages),
    ).toEqual([items[1]])
  })

  /**
   * 栏目树。
   *
   * 站点结构本来是有层级的（栏目就是目录，可以任意层嵌套），而这里曾经把所有栏目平铺：
   * `posts` 与 `posts/2026` 是两个并列的分组。平铺之后侧栏看着像一堆文件夹，
   * 不像一本书的目录。
   */
  describe('栏目树', () => {
    const nested = [
      page('posts/top.md'),
      page('posts/2026/spring.md'),
      page('posts/2026/summer.md'),
      page('posts-old/legacy.md'),
      page('notes/n.md'),
    ]

    it('子栏目紧跟在父栏目下面，并给出缩进层级', () => {
      const groups = groupBySection(nested, [], criteria())

      expect(names(groups)).toEqual(['notes', 'posts', 'posts/2026', 'posts-old'])
      expect(groups.map((g) => g.depth)).toEqual([0, 0, 1, 0])
    })

    it('子栏目不会跑到别人家里去：不能按整条路径字符串排', () => {
      // 按字符串排的话 `posts/2026` 会插在 `posts` 与 `posts-old` 之间——看着对，
      // 但只要顶层栏目排过序（weight）就会露馅：子栏目得跟着父栏目走
      const groups = groupBySection(
        nested,
        [
          { path: 'posts', weight: 9 },
          { path: 'posts-old', weight: 1 },
        ],
        criteria(),
      )

      expect(names(groups)).toEqual(['notes', 'posts-old', 'posts', 'posts/2026'])
    })

    it('同级之间按 weight，父子关系不受影响', () => {
      const groups = groupBySection(
        nested,
        [
          { path: 'posts', weight: 1 },
          { path: 'notes', weight: 2 },
        ],
        criteria(),
      )

      // posts-old 没排过（0）所以在最前，posts(1) 带着自己的子栏目，notes(2) 收尾
      expect(names(groups)).toEqual(['posts-old', 'posts', 'posts/2026', 'notes'])
    })

    it('父栏目在清单里时子栏目只报末段，被筛掉时要报完整路径', () => {
      const all = groupBySection(nested, [], criteria())
      expect(all.find((g) => g.section === 'posts/2026')?.parentShown).toBe(true)

      // 只搜子栏目里的文章：父栏目 posts 整组被筛掉了，此时孤零零一个「2026」看不出是谁的
      const searched = groupBySection(nested, [], criteria({ keyword: 'spring' }))
      expect(names(searched)).toEqual(['posts/2026'])
      expect(searched[0].parentShown).toBe(false)
    })

    it('顶层栏目的名字本来就是完整的，不算「父栏目可见」', () => {
      const groups = groupBySection(nested, [], criteria())
      expect(groups.find((g) => g.section === 'posts')?.parentShown).toBe(false)
    })
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
      (g) => g.pages,
    )
    expect(found.map((p) => p.source)).toEqual(['posts/a.md'])
  })
})

/**
 * 阅读顺序 —— Rust 侧 `content::reading_order` 的孪生实现。
 *
 * 这几条例子与 `sections.rs` 的测试对着写：两边不一致的下场是
 * 「界面上第 3 篇、网站上第 7 篇」，那时侧栏就不再是目录，只是个文件夹。
 */
describe('readingOrder', () => {
  const sorted = (items: GroupablePage[]) =>
    [...items].sort(readingOrder).map((p) => p.source)

  it('排过序的在前，按 weight 升序', () => {
    expect(
      sorted([
        page('c.md', { weight: 3 }),
        page('a.md', { weight: 1 }),
        page('b.md', { weight: 2 }),
      ]),
    ).toEqual(['a.md', 'b.md', 'c.md'])
  })

  it('没排过序的（weight 0）按日期倒序：写文章的默认期望是新的在前', () => {
    expect(
      sorted([
        page('old.md', { date: '2026-01-01T00:00:00Z' }),
        page('new.md', { date: '2026-03-01T00:00:00Z' }),
      ]),
    ).toEqual(['new.md', 'old.md'])
  })

  it('固化过顺序之后新建的文章（weight 0）出现在最前面', () => {
    // weight 是升序的位次，0 排在 1、2、3… 之前。这是有意的：新写的东西该看得见，
    // 而且界面与网站是同一套顺序——挪一次就固化进去了。
    expect(
      sorted([
        page('ranked.md', { weight: 1, date: '2020-01-01T00:00:00Z' }),
        page('brand-new.md', { date: '2026-12-31T00:00:00Z' }),
      ]),
    ).toEqual(['brand-new.md', 'ranked.md'])
  })

  it('没写日期的排在有日期的后面', () => {
    expect(sorted([page('none.md'), page('dated.md', { date: '2020-01-01T00:00:00Z' })])).toEqual([
      'dated.md',
      'none.md',
    ])
  })

  it('时区不同也要比对刻，不是比字符串', () => {
    // 这两个是同一刻，先后由标题决定；比字符串会把 2026-01-01 判成更新
    const same = sorted([
      page('b.md', { date: '2026-01-01T00:00:00+08:00' }),
      page('a.md', { date: '2025-12-31T16:00:00Z' }),
    ])
    expect(same).toEqual(['a.md', 'b.md'])
  })

  it('同 weight 同日期时按标题定死，免得两次列出的顺序不一样', () => {
    // 按码位比，不按语言习惯：「乙」(U+4E59) 在「甲」(U+7532) 之前。
    // 这看着违反直觉，但与 Rust 侧的字节序一致——一致比「符合直觉」重要。
    expect(
      sorted([
        page('jia.md', { title: '甲', weight: 1, date: '2026-01-01T00:00:00Z' }),
        page('yi.md', { title: '乙', weight: 1, date: '2026-01-01T00:00:00Z' }),
      ]),
    ).toEqual(['yi.md', 'jia.md'])
  })
})

/**
 * 「上移 / 下移」算出来的整栏新顺序。
 *
 * 这是界面上唯一能改顺序的动作，而它必须给出**整栏**的清单：
 * 新站点里每篇的 weight 都是 0，「往上挪一位」在那种状态下无从表达。
 */
describe('moved', () => {
  const posts = [
    page('a.md', { date: '2026-01-03T00:00:00Z' }), // 日期倒序：a、b、c
    page('b.md', { date: '2026-01-02T00:00:00Z' }),
    page('c.md', { date: '2026-01-01T00:00:00Z' }),
  ]

  it('上移一位：给出整栏的新顺序', () => {
    expect(moved(posts, 'b.md', -1)).toEqual(['b.md', 'a.md', 'c.md'])
  })

  it('下移一位', () => {
    expect(moved(posts, 'b.md', 1)).toEqual(['a.md', 'c.md', 'b.md'])
  })

  it('已经在头 / 尾时动不了，返回 null 而不是发一次什么也不改的写操作', () => {
    expect(moved(posts, 'a.md', -1)).toBeNull()
    expect(moved(posts, 'c.md', 1)).toBeNull()
  })

  it('不在这一栏里的文章挪不动', () => {
    expect(moved(posts, 'ghost.md', -1)).toBeNull()
  })
})

/**
 * 挪动栏目在同一层里的位次。
 *
 * 与挪动文章同构：一本书的目录既要能排章节之间的先后，也要能排章内小节的先后，
 * 两件事不该有两种手感。界面上原先只有一个「排序」数字输入框。
 */
describe('movedSection 与 siblingsOf', () => {
  const sections = [
    { path: '', weight: 0 },
    { path: 'guides', weight: 0 },
    { path: 'notes', weight: 0 },
    { path: 'posts', weight: 0 },
    { path: 'posts/2026', weight: 0 },
    { path: 'posts/2025', weight: 0 },
  ]

  it('同一层的栏目才算同级，根目录没有同级', () => {
    expect(siblingsOf(sections, 'posts').map((s) => s.path)).toEqual([
      'guides',
      'notes',
      'posts',
    ])
    expect(siblingsOf(sections, 'posts/2026').map((s) => s.path)).toEqual([
      'posts/2026',
      'posts/2025',
    ])
    expect(siblingsOf(sections, '')).toEqual([])
  })

  it('上移一位：给出整层的新顺序', () => {
    const siblings = siblingsOf(sections, 'posts')
    expect(movedSection(siblings, 'notes', -1)).toEqual(['notes', 'guides', 'posts'])
  })

  it('下移一位', () => {
    const siblings = siblingsOf(sections, 'posts')
    expect(movedSection(siblings, 'guides', 1)).toEqual(['notes', 'guides', 'posts'])
  })

  it('已经在头 / 尾时动不了，返回 null 而不是发一次什么也不改的写操作', () => {
    const siblings = siblingsOf(sections, 'posts')
    expect(movedSection(siblings, 'guides', -1)).toBeNull()
    expect(movedSection(siblings, 'posts', 1)).toBeNull()
  })

  it('排过序的栏目按 weight 走，不再按名字', () => {
    const ranked = [
      { path: 'guides', weight: 3 },
      { path: 'notes', weight: 2 },
      { path: 'posts', weight: 1 },
    ]
    expect(movedSection(ranked, 'notes', -1)).toEqual(['notes', 'posts', 'guides'])
  })
})

/**
 * 拖拽排序的算术。
 *
 * 「往后拖一位没反应、拖两位才动」是这一块最典型的 bug：拖走自己之后，
 * 它后面的每个插入位都往前挪了一格。
 */
describe('reorderTo 与 dropSide', () => {
  const order = ['a.md', 'b.md', 'c.md']

  it('拖到最前与最后', () => {
    expect(reorderTo(order, 'c.md', 0)).toEqual(['c.md', 'a.md', 'b.md'])
    expect(reorderTo(order, 'a.md', 3)).toEqual(['b.md', 'c.md', 'a.md'])
  })

  it('往后挪一位：插入位要换算，不然要拖两位才动', () => {
    // 「放到 c 之前」= index 2，而 a 自己占着 0，所以实际落在 1
    expect(reorderTo(order, 'a.md', 2)).toEqual(['b.md', 'a.md', 'c.md'])
  })

  it('拖回原处不算改动，返回 null 而不是发一次写操作', () => {
    expect(reorderTo(order, 'b.md', 1)).toBeNull()
    expect(reorderTo(order, 'b.md', 2)).toBeNull()
  })

  it('越界或不在这份清单里都返回 null', () => {
    expect(reorderTo(order, 'a.md', -1)).toBeNull()
    expect(reorderTo(order, 'a.md', 4)).toBeNull()
    expect(reorderTo(order, 'ghost.md', 0)).toBeNull()
  })

  it('中线决定插在前面还是后面：不然「放到最后一位」做不到', () => {
    expect(dropSide(10, 0, 30)).toBe('before')
    expect(dropSide(20, 0, 30)).toBe('after')
    // 正好压线算后面，与 CSS 的半像素无关，只要两边一致
    expect(dropSide(15, 0, 30)).toBe('after')
  })

  it('同栏目内才排序：跨栏目不是排序，是改归属，走另一条路', () => {
    const a = page('posts/a.md')
    const b = page('posts/b.md')
    const other = page('notes/c.md')

    expect(canDropBeside(a, b)).toBe(true)
    // 跨栏目不给「插在这一行前后」的落点：那一次拖拽同时改了归属和顺序，
    // 说不清会发生什么。改归属走 canDropIntoSection（落在栏目上）
    expect(canDropBeside(a, other)).toBe(false)
    // 拖到自己身上不算落点，也别发写操作
    expect(canDropBeside(a, a)).toBe(false)
    expect(canDropBeside(undefined, b)).toBe(false)
  })

  it('拖到别的栏目上才是「改归属」：同栏目与索引页都不接', () => {
    const a = page('posts/a.md')

    expect(canDropIntoSection(a, 'notes')).toBe(true)
    // 拖回自己所在的栏目等于没动，不该弹干跑
    expect(canDropIntoSection(a, 'posts')).toBe(false)
    expect(canDropIntoSection(undefined, 'notes')).toBe(false)
    // 栏目索引页的地址就是栏目名，搬它要走「栏目改名」——core 也会拒，
    // 界面先别给出「能拖」的假象（判断与执行共用一份规矩）
    expect(canDropIntoSection({ ...page('posts/_index.md'), is_index: true }, 'notes')).toBe(false)
  })
  it('拖到列表边缘要自动滚：不然屏幕外的栏目根本拖不到', () => {
    // 容器：top = 100，高 400（所以下边缘在 500）
    const step = (y: number) => edgeScrollStep(y, 100, 400)

    // 中间不滚：一点风吹草动就滚起来会让人没法停在想要的那一行
    expect(step(300)).toBe(0)
    // 边缘带（默认 48px）以内才动，越靠边越快
    expect(step(140)).toBeLessThan(0)
    expect(step(110)).toBeLessThan(step(140))
    expect(step(460)).toBeGreaterThan(0)
    expect(step(490)).toBeGreaterThan(step(460))
    // 压在边线上就是最大速度；拖出容器之外不该再加速（否则一下窜到底）
    expect(step(100)).toBe(step(60))
    expect(step(500)).toBe(step(560))
    expect(step(100)).toBe(-step(500))
  })

  it('容器很矮时上下两条带会叠在一起：按更近的那边走', () => {
    // 高 40，比两条带加起来还窄
    expect(edgeScrollStep(105, 100, 40)).toBeLessThan(0)
    expect(edgeScrollStep(135, 100, 40)).toBeGreaterThan(0)
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
