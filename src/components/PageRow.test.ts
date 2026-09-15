// @vitest-environment jsdom
import { mount } from '@vue/test-utils'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { computed, reactive } from 'vue'

/**
 * 文章行的行为测试。
 *
 * 这是整个界面里点得最多的一块，却是最后一块没有测试的：它长在 `PageList` 的
 * `v-for` 里，与选择态、筛选态、右键菜单缠在一起，测不动。抽成组件的第一目的
 * 就是为了能钉住这几件事：
 *
 * - 徽标的**优先级**：未保存 > 草稿 > 定时。三个都用同一个位置，判断写错就会
 *   出现「明明有未保存改动，行上却显示草稿」——而未保存是唯一会丢东西的那个。
 * - 删除只能从菜单进入，且进入后仍要再点一次「删除」；忙态里取消永远能按。
 * - 右键与「⋯」是同一个入口（右键是看不见的，必须有可见的孪生入口）。
 */
const state = reactive({ currentSource: null as string | null, busy: false })

vi.mock('../store', () => ({
  store: state,
  actions: {},
  isDirty: computed(() => state.currentSource === 'posts/dirty.md'),
}))

const PageRow = (await import('./PageRow.vue')).default

const page = {
  source: 'posts/a.md',
  title: '第一篇',
  url: '/posts/a/',
  draft: false,
  scheduled: false,
  is_index: false,
  date: null,
}

function mountRow(props: Record<string, unknown> = {}) {
  return mount(PageRow, {
    props: {
      page,
      selecting: false,
      checked: false,
      dirty: false,
      seo: null,
      confirming: false,
      ...props,
    },
  })
}

describe('PageRow', () => {
  beforeEach(() => {
    state.currentSource = null
    state.busy = false
  })

  it('显示标题，点一下就要求打开这一篇', async () => {
    const wrapper = mountRow()
    expect(wrapper.get('.page-list__title').text()).toBe('第一篇')

    await wrapper.get('button').trigger('click')
    expect(wrapper.emitted('open')).toHaveLength(1)
  })

  it('多选态才出现勾选框，且读屏器听得出勾的是哪一篇', async () => {
    expect(mountRow().find('input[type="checkbox"]').exists()).toBe(false)

    const wrapper = mountRow({ selecting: true, checked: true })
    const box = wrapper.get('input[type="checkbox"]')
    expect((box.element as HTMLInputElement).checked).toBe(true)
    expect(box.attributes('aria-label')).toContain('第一篇')

    await box.trigger('change')
    expect(wrapper.emitted('toggle')).toHaveLength(1)
  })

  it('未保存压过草稿：会丢东西的那个必须先说', () => {
    state.currentSource = 'posts/dirty.md'
    const wrapper = mountRow({ page: { ...page, source: 'posts/dirty.md', draft: true } })

    expect(wrapper.find('.badge--unsaved').exists()).toBe(true)
    expect(wrapper.find('.badge--draft').exists()).toBe(false)
  })

  it('没在编辑时照常显示草稿 / 定时 / 栏目页 / 待生成 / SEO', () => {
    const wrapper = mountRow({
      page: { ...page, draft: true, is_index: true },
      dirty: true,
      seo: { severity: 'warn', messages: ['缺描述'] },
    })

    expect(wrapper.find('.badge--draft').text()).toBe('草稿')
    expect(wrapper.find('.badge--section').exists()).toBe(true)
    expect(wrapper.find('.badge--dirty').exists()).toBe(true)
    const seo = wrapper.get('.badge--seo-warn')
    expect(seo.attributes('title')).toContain('缺描述')
  })

  it('定时只在没被草稿占位时出现，并把到点时间写进提示', () => {
    const wrapper = mountRow({
      page: { ...page, scheduled: true, date: '2026-12-01' },
    })
    const badge = wrapper.get('.badge--draft')
    expect(badge.text()).toBe('定时')
    expect(badge.attributes('title')).toContain('2026-12-01')
  })

  it('右键与「⋯」是同一个入口', async () => {
    const wrapper = mountRow()
    await wrapper.get('.page-list__icon').trigger('click')
    await wrapper.get('li').trigger('contextmenu')
    expect(wrapper.emitted('menu')).toHaveLength(2)

  })

  it('平时没有删除按钮：删除只能从菜单进', () => {
    const wrapper = mountRow()
    expect(wrapper.find('.page-list__danger').exists()).toBe(false)
  })

  it('确认态里再点一次才删，取消回到平常', async () => {
    const wrapper = mountRow({ confirming: true })

    await wrapper.get('.page-list__danger').trigger('click')
    expect(wrapper.emitted('remove')).toHaveLength(1)

    await wrapper.get('.page-list__confirm-cancel').trigger('click')
    expect(wrapper.emitted('cancel')).toHaveLength(1)
    // 确认态里「⋯」让位给这两颗按钮，免得三颗挤在一行
    expect(wrapper.find('.page-list__icon:not(.page-list__confirm-cancel)').exists()).toBe(false)
  })

  it('忙的时候删不动，但取消一直能按', async () => {
    state.busy = true
    const wrapper = mountRow({ confirming: true })

    expect(wrapper.get('.page-list__danger').attributes('disabled')).toBeDefined()
    expect(wrapper.get('.page-list__confirm-cancel').attributes('disabled')).toBeUndefined()
  })

  it('正在编辑的那一篇要标出来', () => {
    state.currentSource = 'posts/a.md'
    expect(mountRow().get('button').classes()).toContain('active')
  })
})
