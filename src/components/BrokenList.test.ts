// @vitest-environment jsdom
import { mount } from '@vue/test-utils'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { reactive } from 'vue'

/**
 * 「读不出来」那一组的行为测试。
 *
 * 这一组是上一轮留下的缺口：项目打得开了，坏文件却只在一条通知里露过一面——
 * 用户知道出了事，却在界面里点不到那一篇。所以要钉的是三件：列得出来、点得开、
 * 以及把「它不在站点里、生成会被拦下」说清楚。
 */
const state = reactive({ currentSource: null as string | null })

const actions = {
  openBroken: vi.fn(async () => {}),
}

vi.mock('../store', () => ({ store: state, actions }))

const BrokenList = (await import('./BrokenList.vue')).default

const items = [
  { source: 'posts/a.md', reason: 'front matter 缺少结束的 `+++`' },
  { source: 'notes/b.md', reason: 'front matter 不是合法的 TOML' },
]

function mountList(list = items) {
  return mount(BrokenList, { props: { items: list } })
}

describe('BrokenList', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    state.currentSource = null
  })

  it('没有坏文件时整组都不出现：那是正常状态，不该常驻一个空标题', () => {
    const wrapper = mountList([])
    expect(wrapper.find('h3').exists()).toBe(false)
  })

  it('列出每一篇与原因，并给出数量', () => {
    const wrapper = mountList()
    expect(wrapper.get('h3').text()).toContain('读不出来')
    expect(wrapper.get('h3').text()).toContain('2')

    const rows = wrapper.findAll('li')
    expect(rows).toHaveLength(2)
    expect(rows[0].text()).toContain('posts/a.md')
    expect(rows[0].text()).toContain('front matter 缺少结束的 `+++`')
    expect(rows[1].text()).toContain('notes/b.md')
  })

  it('说清后果：它不在内容清单里，而且生成会被拦下', () => {
    const wrapper = mountList()
    const text = wrapper.text()
    expect(text).toContain('没有进内容清单')
    expect(text).toContain('生成')
  })

  it('点一行就把它当纯文本打开——修它是这一组存在的理由', async () => {
    const wrapper = mountList()
    await wrapper.findAll('li')[1].get('button').trigger('click')
    expect(actions.openBroken).toHaveBeenCalledWith('notes/b.md')
  })

  it('正在编辑的那一篇要标出来', async () => {
    const wrapper = mountList()
    state.currentSource = 'posts/a.md'
    await wrapper.vm.$nextTick()

    const buttons = wrapper.findAll('li button')
    expect(buttons[0].classes()).toContain('active')
    expect(buttons[1].classes()).not.toContain('active')
  })
})
