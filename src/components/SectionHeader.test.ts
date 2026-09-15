// @vitest-environment jsdom
import { mount } from '@vue/test-utils'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { reactive } from 'vue'

import type { MenuEntry } from '../commands'

/**
 * 栏目头的行为测试。
 *
 * 这一块刚从 `PageList.vue`（1298 行、十来个职责）里抽出来，抽的动机就是它测不动：
 * 改名的确认态有三个出口（确认 / 取消 / 干跑被拒），而我在同构的地方漏掉一个出口
 * 就写出过真 bug（改地址的待确认行换文章后不作废）。
 */
const preview = {
  from: 'posts',
  to: 'essays',
  files: 3,
  aliases: 2,
  refs: [{ source: 'notes/b.md', hits: 2 }],
  refs_manual: [{ source: 'notes/c.md', hits: 1 }],
}

const state = reactive({ busy: false })

const actions = {
  previewRenameSection: vi.fn(async () => preview as typeof preview | undefined),
  renameSection: vi.fn(async () => {}),
  saveSectionMeta: vi.fn(async () => {}),
  removeSection: vi.fn(async () => {}),
}

/** 右键菜单只记下条目：菜单本身是全局单例，这里要验的是「给了哪几项」。 */
const openContextMenu = vi.fn<(event: MouseEvent, entries: MenuEntry[]) => void>()

vi.mock('../store', () => ({ store: state, actions }))
vi.mock('../commands', () => ({ openContextMenu }))

const SectionHeader = (await import('./SectionHeader.vue')).default

const META = {
  title: '文章',
  description: '随手写的',
  weight: 10,
  index_source: 'posts/index.md',
  pages: 3,
}

function mountHeader(props: Record<string, unknown> = {}) {
  return mount(SectionHeader, {
    props: { section: 'posts', count: 3, meta: META, ...props },
    attachTo: document.body,
  })
}

function button(wrapper: ReturnType<typeof mountHeader>, text: string) {
  return wrapper.findAll('button').find((b) => b.text().includes(text))!
}

/** 在已经打开的改名表单里填新名字并提交，走到「干跑回来了、等确认」这一步。 */
async function askRename(wrapper: ReturnType<typeof mountHeader>, to = 'essays') {
  await wrapper.get('.page-list__rename input[type="text"]').setValue(to)
  await wrapper.get('form.page-list__rename').trigger('submit')
  await new Promise((resolve) => setTimeout(resolve))
}

/** 最近一次菜单里带 id 的条目（`MenuEntry` 是联合类型，分隔线没有 id）。 */
function entries() {
  return openContextMenu.mock.calls.at(-1)![1].filter((item) => 'id' in item)
}

function entry(id: string) {
  const found = entries().find((item) => item.id === id)
  if (!found) throw new Error(`菜单里没有 ${id}`)
  return found
}

/** 打开改名表单（菜单项里的「栏目改名…」）。 */
async function openRenameForm(wrapper: ReturnType<typeof mountHeader>) {
  await button(wrapper, '⋯').trigger('click')
  entry('section.rename').run?.()
  await wrapper.vm.$nextTick()
}

describe('SectionHeader', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    actions.previewRenameSection.mockResolvedValue(preview)
    state.busy = false
  })

  it('显示栏目名与篇数；根目录有自己的名字', () => {
    expect(mountHeader().get('.page-list__section-name').text()).toBe('posts')
    expect(mountHeader({ section: '' }).get('.page-list__section-name').text()).toBe('根目录')
  })

  it('没有列表页的栏目要标出来：栏目地址打不开列表页是个真问题', () => {
    expect(mountHeader().find('.badge--seo-warn').exists()).toBe(false)
    const missing = mountHeader({ meta: { ...META, index_source: null } })
    expect(missing.get('.badge--seo-warn').text()).toBe('缺列表页')
  })

  it('根目录不给「改名」与「删除」两项：它俩对根目录没有意义', async () => {
    const wrapper = mountHeader({ section: '', meta: undefined })
    await button(wrapper, '⋯').trigger('click')
    const ids = entries().map((e) => e.id)
    expect(ids).toContain('section.new')
    expect(ids).toContain('section.meta')
    expect(ids).not.toContain('section.rename')
    expect(ids).not.toContain('section.remove')
  })

  it('栏目里还有文章时「删除空栏目」置灰并说明原因', async () => {
    const wrapper = mountHeader()
    await button(wrapper, '⋯').trigger('click')
    const remove = entry('section.remove')
    expect(remove.disabled).toBe(true)
    expect(remove.hint).toContain('还有文章')

    const empty = mountHeader({ count: 0, meta: { ...META, pages: 0 } })
    await button(empty, '⋯').trigger('click')
    expect(entry('section.remove').disabled).toBe(false)
  })

  it('「在此栏目新建文章」把意图递给上层：新建表单不在这个组件里', async () => {
    const wrapper = mountHeader()
    await button(wrapper, '⋯').trigger('click')
    entry('section.new').run?.()
    expect(wrapper.emitted('newContent')).toEqual([['posts']])
  })

  it('栏目信息表单预填现有值，保存后交给 store', async () => {
    const wrapper = mountHeader()
    await button(wrapper, '信息').trigger('click')

    const inputs = wrapper.findAll('.page-list__rename input')
    expect((inputs[0].element as HTMLInputElement).value).toBe('文章')
    expect((inputs[1].element as HTMLInputElement).value).toBe('随手写的')

    await inputs[0].setValue('新标题')
    await wrapper.get('form.page-list__rename').trigger('submit')
    expect(actions.saveSectionMeta).toHaveBeenCalledWith('posts', {
      title: '新标题',
      description: '随手写的',
      weight: 10,
    })
  })

  it('改名先干跑，没确认之前不写盘，并把「还会改哪几篇」说清楚', async () => {
    const wrapper = mountHeader()
    await openRenameForm(wrapper)
    await askRename(wrapper)

    expect(actions.previewRenameSection).toHaveBeenCalledWith('posts', 'essays', true)
    expect(actions.renameSection).not.toHaveBeenCalled()

    const text = wrapper.get('.page-list__dry').text()
    expect(text).toContain('搬动 3 个文件')
    expect(text).toContain('给 2 篇补旧地址')
    expect(text).toContain('1 篇里的 2 处')
    expect(text).toContain('notes/b.md')
    // 相对链接改写不到，得人工看一眼
    expect(text).toContain('相对链接')
    expect(text).toContain('notes/c.md')
  })

  it('确认之后才改名', async () => {
    const wrapper = mountHeader()
    await openRenameForm(wrapper)
    await askRename(wrapper)
    await button(wrapper, '确认改名').trigger('click')

    expect(actions.renameSection).toHaveBeenCalledWith('posts', 'essays', true)
    expect(wrapper.find('form.page-list__rename').exists()).toBe(false)
  })

  it('取消什么也不做，表单一起收起', async () => {
    const wrapper = mountHeader()
    await openRenameForm(wrapper)
    await askRename(wrapper)
    await wrapper.findAll('.page-list__dry button').find((b) => b.text() === '取消')!.trigger('click')

    expect(actions.renameSection).not.toHaveBeenCalled()
    expect(wrapper.find('form.page-list__rename').exists()).toBe(false)
  })

  it('干跑被拦下时不进确认态（同名、目标已存在这类错误由 store 弹通知）', async () => {
    actions.previewRenameSection.mockResolvedValue(undefined)
    const wrapper = mountHeader()
    await openRenameForm(wrapper)
    await askRename(wrapper)

    expect(wrapper.find('.page-list__dry').exists()).toBe(false)
    expect(wrapper.find('form.page-list__rename').exists()).toBe(false)
    expect(actions.renameSection).not.toHaveBeenCalled()
  })

  it('名字没改就直接收起，不白跑一次干跑', async () => {
    const wrapper = mountHeader()
    await openRenameForm(wrapper)
    await wrapper.get('form.page-list__rename').trigger('submit')
    await new Promise((resolve) => setTimeout(resolve))

    expect(actions.previewRenameSection).not.toHaveBeenCalled()
    expect(wrapper.find('form.page-list__rename').exists()).toBe(false)
  })

  it('两张表单互斥：开了改名就不该还留着信息表单', async () => {
    const wrapper = mountHeader()
    await button(wrapper, '信息').trigger('click')
    expect(wrapper.findAll('form.page-list__rename')).toHaveLength(1)

    await openRenameForm(wrapper)
    const forms = wrapper.findAll('form.page-list__rename')
    expect(forms).toHaveLength(1)
    expect(forms[0].find('input[aria-label="新栏目名"]').exists()).toBe(true)
  })

  it('删除要就地再确认一次，确认前不调 store', async () => {
    const wrapper = mountHeader({ count: 0, meta: { ...META, pages: 0 } })
    await button(wrapper, '⋯').trigger('click')
    entry('section.remove').run?.()
    await wrapper.vm.$nextTick()

    expect(actions.removeSection).not.toHaveBeenCalled()
    await button(wrapper, '删除栏目').trigger('click')
    expect(actions.removeSection).toHaveBeenCalledWith('posts')
  })
})
