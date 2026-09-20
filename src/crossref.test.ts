import { describe, expect, it } from 'vitest'

import { LINK_LIMIT, linkSnippet, matchTargets, menuTargets } from './crossref'
import type { PageSummary } from './api'

function page(partial: Partial<PageSummary> & { source: string }): PageSummary {
  return {
    title: partial.source,
    url: `/${partial.source.replace(/\.md$/, '')}/`,
    template: 'page.html',
    section: '',
    is_index: false,
    draft: false,
    date: null,
    scheduled: false,
    tags: [],
    weight: 0,
    ...partial,
  }
}

const pages = [
  page({ source: 'posts/incremental.md', title: '增量构建', url: '/posts/incremental/' }),
  page({ source: 'posts/deploy.md', title: '发布到 FTP', url: '/posts/deploy/' }),
  page({ source: 'about.md', title: '关于', url: '/about/' }),
  page({ source: 'posts/draft.md', title: '还没写完', url: '/posts/draft/', draft: true }),
]

describe('matchTargets', () => {
  it('空关键词给全部候选，顺序不动', () => {
    expect(matchTargets(pages, '').map((p) => p.source)).toEqual([
      'posts/incremental.md',
      'posts/deploy.md',
      'about.md',
      'posts/draft.md',
    ])
  })

  it('标题、地址、源路径都能命中', () => {
    expect(matchTargets(pages, '增量').map((p) => p.source)).toEqual(['posts/incremental.md'])
    expect(matchTargets(pages, '/about/').map((p) => p.source)).toEqual(['about.md'])
    expect(matchTargets(pages, 'posts/deploy').map((p) => p.source)).toEqual(['posts/deploy.md'])
  })

  it('忽略大小写', () => {
    expect(matchTargets(pages, 'FTP').map((p) => p.source)).toEqual(['posts/deploy.md'])
    expect(matchTargets(pages, 'ftp').map((p) => p.source)).toEqual(['posts/deploy.md'])
  })

  it('不做模糊打分：命中顺序就是站点顺序', () => {
    // 「posts」同时命中三篇，不按标题长度或前缀优先重排——排序玄学会让人怀疑漏了东西
    expect(matchTargets(pages, 'posts').map((p) => p.source)).toEqual([
      'posts/incremental.md',
      'posts/deploy.md',
      'posts/draft.md',
    ])
  })

  it('排除自己：链到当前这一篇几乎总是手滑', () => {
    const out = matchTargets(pages, '', { exclude: 'about.md' })
    expect(out.map((p) => p.source)).not.toContain('about.md')
  })

  it('候选有上限，避免一屏铺满几千个文件名', () => {
    const many = Array.from({ length: LINK_LIMIT + 10 }, (_, i) => page({ source: `p${i}.md` }))
    expect(matchTargets(many, '').length).toBe(LINK_LIMIT)
  })
})

describe('linkSnippet', () => {
  it('Markdown 站点给 Markdown 链接', () => {
    expect(linkSnippet('增量构建', '/posts/incremental/', 'markdown')).toBe(
      '[增量构建](/posts/incremental/)',
    )
  })

  it('HTML 站点给 a 标签：正文格式是 HTML 时插 Markdown 等于插一段纯文本', () => {
    expect(linkSnippet('关于', '/about/', 'html')).toBe('<a href="/about/">关于</a>')
  })

  it('链接文字里的方括号要转义，否则语法当场断掉', () => {
    expect(linkSnippet('见 [附录]', '/a/', 'markdown')).toBe('[见 \\[附录\\]](/a/)')
  })

  it('地址里有括号或空格时用尖括号包住', () => {
    expect(linkSnippet('图', '/img/a (1).png', 'markdown')).toBe('[图](</img/a (1).png>)')
  })

  it('HTML 里的引号与尖括号要转义，不能让标题把属性顶开', () => {
    expect(linkSnippet('他说"好"', '/a/?x=1&y=2', 'html')).toBe(
      '<a href="/a/?x=1&amp;y=2">他说&quot;好&quot;</a>',
    )
  })
})

describe('menuTargets', () => {
  const sections = [
    { path: 'posts', title: '文章', url: '/posts/', index_source: 'posts/_index.md' },
    { path: 'notes', title: '', url: '/notes/', index_source: 'notes/_index.md' },
    // 没有索引页的栏目：它的地址本身就是 404
    { path: 'drafts', title: '草稿堆', url: '/drafts/', index_source: null },
  ]

  it('栏目排在页面前面：菜单项要指的多半是栏目', () => {
    const targets = menuTargets([page({ source: 'posts/a.md' })], sections)
    expect(targets.map((item) => item.url)).toEqual(['/posts/', '/notes/', '/posts/a/'])
  })

  it('没有索引页的栏目不给选：那个地址是 404，挑了就是埋一条死链', () => {
    const targets = menuTargets([], sections)
    expect(targets.some((item) => item.url === '/drafts/')).toBe(false)
  })

  it('栏目没写标题就用目录名，不给空白项', () => {
    const targets = menuTargets([], sections)
    expect(targets.find((item) => item.url === '/notes/')?.title).toBe('notes')
  })

  it('挑出来的东西能直接喂给现成的搜索：栏目也按标题和地址匹配得到', () => {
    const targets = menuTargets([page({ source: 'posts/a.md' })], sections)
    expect(matchTargets(targets, '文章').map((item) => item.url)).toEqual(['/posts/'])
    expect(matchTargets(targets, '/notes').map((item) => item.url)).toEqual(['/notes/'])
  })
})

