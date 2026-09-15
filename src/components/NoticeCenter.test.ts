// @vitest-environment jsdom
import { mount } from '@vue/test-utils'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { reactive } from 'vue'

/**
 * 消息中心的行为测试。
 *
 * 它是「事后还能读一遍」的唯一落点：气泡几秒就没了，而带行列号的报错、
 * 批量动作里被跳过的那十几篇，恰恰是要慢慢读的东西。要钉的是四件：
 * 打开即算看过（否则未读数一直挂着）、明细列得出来、清空真的清、
 * 以及浮层那套共同约定（Esc、点空白、焦点归还）。
 */
const state = reactive({
  notices: [] as Array<{
    id: number
    level: 'success' | 'info' | 'error'
    message: string
    at: number
    details?: string[]
  }>,
})

const actions = {
  markNoticesSeen: vi.fn(),
  clearNotices: vi.fn(),
}

vi.mock('../store', () => ({ store: state, actions }))

const NoticeCenter = (await import('./NoticeCenter.vue')).default

function mountCenter() {
  return mount(NoticeCenter, { props: { open: false }, attachTo: document.body })
}

async function tick() {
  await new Promise((resolve) => setTimeout(resolve))
}

function button(wrapper: ReturnType<typeof mountCenter>, text: string) {
  return wrapper.findAll('button').find((b) => b.text().includes(text))!
}

describe('NoticeCenter', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    state.notices = [
      {
        id: 2,
        level: 'info',
        message: '已处理 2 篇；跳过 3 篇：2 篇「front matter 读不出来」、1 篇「目标已存在」',
        at: 0,
        details: [
          'posts/a.md：目标已存在',
          'posts/b.md：front matter 读不出来',
          'posts/c.md：front matter 读不出来',
        ],
      },
      { id: 1, level: 'error', message: '第 3 行第 5 列：期望一个 =', at: 0 },
    ]
  })

  it('关着的时候什么也不渲染', () => {
    const wrapper = mountCenter()
    expect(wrapper.find('.palette').exists()).toBe(false)
  })

  it('打开即算看过：不然未读数会一直挂着', async () => {
    const wrapper = mountCenter()
    await wrapper.setProps({ open: true })
    expect(actions.markNoticesSeen).toHaveBeenCalled()
  })

  it('打开后焦点落在「关闭」上，关掉再还给打开它的那个元素', async () => {
    const opener = document.createElement('button')
    document.body.appendChild(opener)
    opener.focus()

    const wrapper = mountCenter()
    await wrapper.setProps({ open: true })
    await tick()
    expect(document.activeElement).toBe(button(wrapper, '关闭').element)

    await wrapper.setProps({ open: false })
    expect(document.activeElement).toBe(opener)
    opener.remove()
  })

  it('按时间倒序列出消息，并给出程度的中文名（只有颜色读屏器听不到）', async () => {
    const wrapper = mountCenter()
    await wrapper.setProps({ open: true })

    const items = wrapper.findAll('.notice')
    expect(items).toHaveLength(2)
    expect(items[0].text()).toContain('已处理 2 篇')
    expect(items[0].text()).toContain('提示')
    expect(items[1].text()).toContain('错误')
    expect(items[1].text()).toContain('第 3 行第 5 列')
  })

  it('带明细的那条能展开看每一行：批量跳过了哪几篇只有这里查得到', async () => {
    const wrapper = mountCenter()
    await wrapper.setProps({ open: true })

    const box = wrapper.findAll('.notice')[0].get('details')
    expect(box.get('summary').text()).toContain('3 条')
    expect(box.text()).toContain('posts/a.md：目标已存在')
    expect(box.text()).toContain('posts/c.md：front matter 读不出来')
  })

  it('没有明细的那条不摆一个空的展开入口', async () => {
    const wrapper = mountCenter()
    await wrapper.setProps({ open: true })
    expect(wrapper.findAll('.notice')[1].find('details').exists()).toBe(false)
  })

  it('Esc 与点空白都关闭', async () => {
    const wrapper = mountCenter()
    await wrapper.setProps({ open: true })

    await wrapper.get('[role="dialog"]').trigger('keydown.esc')
    expect(wrapper.emitted('close')).toHaveLength(1)

    await wrapper.get('.palette').trigger('click')
    expect(wrapper.emitted('close')).toHaveLength(2)
  })

  it('「清空」交给 store，「关闭」不清任何东西', async () => {
    const wrapper = mountCenter()
    await wrapper.setProps({ open: true })

    await button(wrapper, '清空').trigger('click')
    expect(actions.clearNotices).toHaveBeenCalled()

    await button(wrapper, '关闭').trigger('click')
    expect(actions.clearNotices).toHaveBeenCalledTimes(1)
  })

  it('一条消息都没有时说清楚，并且不摆「清空」', async () => {
    state.notices = []
    const wrapper = mountCenter()
    await wrapper.setProps({ open: true })

    expect(wrapper.text()).toContain('还没有消息')
    expect(wrapper.findAll('button').some((b) => b.text().includes('清空'))).toBe(false)
  })
})
