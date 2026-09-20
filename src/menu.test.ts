import { describe, expect, it } from 'vitest'

import { missingSections, unknownMenuUrls } from './menu'

const sections = [
  { path: 'posts', title: '文章', url: '/posts/', index_source: 'posts/_index.md' },
  { path: 'notes', title: '随笔', url: '/notes/', index_source: 'notes/_index.md' },
  // 没有索引页：这个栏目的地址本身是 404，不该被推荐进导航
  { path: 'drafts', title: '草稿', url: '/drafts/', index_source: null },
]

describe('missingSections', () => {
  it('列出还没进导航的栏目：新建栏目之后最容易忘的就是这一步', () => {
    const menu = [{ url: '/' }, { url: '/posts/' }]
    expect(missingSections(menu, sections).map((item) => item.path)).toEqual(['notes'])
  })

  it('没有索引页的栏目不推荐：它的地址是 404', () => {
    expect(missingSections([], sections).map((item) => item.path)).toEqual(['posts', 'notes'])
  })

  it('末尾斜杠不该算成两个地址：`/posts` 与 `/posts/` 是同一个栏目', () => {
    expect(missingSections([{ url: '/posts' }], sections).map((item) => item.path)).toEqual([
      'notes',
    ])
  })
})

describe('unknownMenuUrls', () => {
  const targets = [
    { source: 'posts/_index.md', title: '文章', url: '/posts/', draft: false },
    { source: 'about.md', title: '关于', url: '/about/', draft: false },
  ]

  it('站内地址找不到对应页面就报出来：菜单渲染在每一页，错一个字就是全站死链', () => {
    const menu = [{ url: '/posts/' }, { url: '/abuot/' }]
    expect(unknownMenuUrls(menu, targets)).toEqual(['/abuot/'])
  })

  it('站外链接与锚点不管：它们的正确性不在这个站里', () => {
    const menu = [
      { url: 'https://example.com/' },
      { url: 'mailto:a@b.c' },
      { url: '#top' },
      { url: '/about/#history' },
    ]
    expect(unknownMenuUrls(menu, targets)).toEqual([])
  })

  it('末尾斜杠差异不算错：产物里 `/about` 也能打开 `/about/index.html`', () => {
    expect(unknownMenuUrls([{ url: '/about' }], targets)).toEqual([])
  })

  it('首页也要真有 index.md 才算存在：与 core 的构建警告同一套规则', () => {
    expect(unknownMenuUrls([{ url: '/' }], targets)).toEqual(['/'])
    const withHome = [...targets, { source: 'index.md', title: '首页', url: '/', draft: false }]
    expect(unknownMenuUrls([{ url: '/' }], withHome)).toEqual([])
  })

  it('空地址不报：那是还没填完的行，保存时自会拦下', () => {
    expect(unknownMenuUrls([{ url: '' }, { url: '   ' }], targets)).toEqual([])
  })

  it('同一个错地址只报一次：重复提示不会让人更快改对', () => {
    expect(unknownMenuUrls([{ url: '/nope/' }, { url: '/nope/' }], targets)).toEqual(['/nope/'])
  })
})
