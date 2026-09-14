// @vitest-environment jsdom
import { mount } from '@vue/test-utils'
import { describe, expect, it } from 'vitest'

import LinkDialog from './LinkDialog.vue'
import { LINK_LIMIT, type LinkTarget } from '../crossref'

/**
 * 站内链接选择器的行为测试。
 *
 * 筛选规则本身在 `crossref.test.ts` 里已经钉住，这里只测「界面这一层」：
 * 焦点留在输入框、方向键只移动高亮、回车挑中当前那条、草稿标出来但不禁用、
 * 候选顶到上限时说一声。
 */
function page(source: string, extra: Partial<LinkTarget> = {}): LinkTarget {
  return {
    source,
    title: source,
    url: `/${source.replace(/\.md$/, '')}/`,
    draft: false,
    ...extra,
  }
}

const PAGES: LinkTarget[] = [
  page('posts/incremental.md', { title: '增量构建', url: '/posts/incremental/' }),
  page('posts/deploy.md', { title: '发布到 FTP', url: '/posts/deploy/' }),
  page('posts/draft.md', { title: '还没写完', url: '/posts/draft/', draft: true }),
]

function open(pages: LinkTarget[] = PAGES, exclude?: string) {
  const opener = document.createElement('button')
  document.body.appendChild(opener)
  opener.focus()
  const wrapper = mount(LinkDialog, {
    props: { open: false, pages, exclude },
    attachTo: document.body,
  })
  return { wrapper, opener }
}

async function tick() {
  await new Promise((resolve) => setTimeout(resolve))
}

describe('LinkDialog', () => {
  it('打开即聚焦输入框：候选要能边打字边换，手不该离开键盘中央', async () => {
    const { wrapper } = open()
    await wrapper.setProps({ open: true })
    await tick()
    expect(document.activeElement).toBe(wrapper.get('.palette__input').element)
  })

  it('按关键词筛，回车挑中当前高亮那条', async () => {
    const { wrapper } = open()
    await wrapper.setProps({ open: true })

    const input = wrapper.get('.palette__input')
    await input.setValue('增量')
    expect(wrapper.findAll('.outline__item')).toHaveLength(1)

    await input.trigger('keydown', { key: 'Enter' })
    expect(wrapper.emitted('pick')?.[0]?.[0]).toMatchObject({ url: '/posts/incremental/' })
    expect(wrapper.emitted('close')).toHaveLength(1)
  })

  it('方向键只移动高亮，焦点不离开输入框', async () => {
    const { wrapper } = open()
    await wrapper.setProps({ open: true })
    await tick()
    const input = wrapper.get('.palette__input')

    await input.trigger('keydown', { key: 'ArrowDown' })
    expect(wrapper.findAll('.outline__item')[1].classes()).toContain('active')
    expect(document.activeElement).toBe(input.element)
  })

  it('改关键词后高亮回到第一条：旧序号会指向另一篇', async () => {
    const { wrapper } = open()
    await wrapper.setProps({ open: true })
    const input = wrapper.get('.palette__input')

    await input.trigger('keydown', { key: 'ArrowDown' })
    await input.setValue('posts')
    expect(wrapper.findAll('.outline__item')[0].classes()).toContain('active')
  })

  it('草稿标出来但不禁用：先写互链、后补那一篇是常见顺序', async () => {
    const { wrapper } = open()
    await wrapper.setProps({ open: true })

    const draft = wrapper.findAll('.outline__item')[2]
    expect(draft.find('.link__draft').exists()).toBe(true)
    expect(draft.attributes('disabled')).toBeUndefined()
  })

  it('排除当前这一篇：链到自己几乎总是手滑', async () => {
    const { wrapper } = open(PAGES, 'posts/deploy.md')
    await wrapper.setProps({ open: true })
    expect(wrapper.text()).not.toContain('发布到 FTP')
  })

  it('候选顶到上限时说一句，否则「怎么找不到那一篇」会归咎于搜索坏了', async () => {
    const many = Array.from({ length: LINK_LIMIT + 5 }, (_, i) => page(`p${i}.md`))
    const { wrapper } = open(many)
    await wrapper.setProps({ open: true })
    expect(wrapper.text()).toContain(`只显示前 ${LINK_LIMIT} 条`)
  })

  it('没有命中时给空态而不是空列表框', async () => {
    const { wrapper } = open()
    await wrapper.setProps({ open: true })
    await wrapper.get('.palette__input').setValue('没有这样的文章')
    expect(wrapper.find('[role="listbox"]').exists()).toBe(false)
    expect(wrapper.text()).toContain('没有匹配的内容')
  })

  it('重开时清空上一次的关键词：这一次要链的与上一次几乎没关系', async () => {
    const { wrapper } = open()
    await wrapper.setProps({ open: true })
    await wrapper.get('.palette__input').setValue('增量')
    await wrapper.setProps({ open: false })
    await wrapper.setProps({ open: true })
    expect((wrapper.get('.palette__input').element as HTMLInputElement).value).toBe('')
  })

  it('Esc 关闭、点遮罩空白关闭，关闭后焦点还给打开它的元素', async () => {
    const { wrapper, opener } = open()
    await wrapper.setProps({ open: true })
    await tick()

    await wrapper.get('[role="dialog"]').trigger('keydown', { key: 'Escape' })
    expect(wrapper.emitted('close')).toHaveLength(1)
    await wrapper.get('.palette').trigger('click')
    expect(wrapper.emitted('close')).toHaveLength(2)

    await wrapper.setProps({ open: false })
    expect(document.activeElement).toBe(opener)
  })
})
