// @vitest-environment jsdom
import { mount } from '@vue/test-utils'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { reactive } from 'vue'

/**
 * 批量动作条的行为测试。
 *
 * 这里有两条会改地址或不可逆的路径（移动、删除），各带一个干跑确认态；
 * 另外两条（加减标签、发布/草稿）反手能改回来，所以刻意不加确认。
 * 要钉住的正是这个区别，以及「确认之前一律不落盘」。
 */
const movePreview = {
  changes: [
    { source: 'posts/a.md', changes: true, effect: '搬到 notes/a.md，旧地址 /posts/a/' },
    { source: 'posts/b.md', changes: false, effect: '已经在这个栏目里' },
  ],
  affected: 1,
  refs: [{ source: 'notes/c.md', hits: 2 }],
  refs_manual: [{ source: 'notes/d.md', hits: 1 }],
}

const deletePreview = {
  changes: [{ source: 'posts/a.md', changes: true, effect: '删除' }],
  affected: 1,
  refs: [],
  refs_manual: [],
}

const state = reactive({ busy: false })

const actions = {
  batchEditTags: vi.fn(async () => {}),
  batchSetDraft: vi.fn(async () => {}),
  batchPreview: vi.fn(async () => movePreview as unknown),
  batchMove: vi.fn(async () => {}),
  batchDelete: vi.fn(async () => {}),
}

vi.mock('../store', () => ({ store: state, actions }))

const BatchBar = (await import('./BatchBar.vue')).default

const SELECTED = ['posts/a.md', 'posts/b.md']

function mountBar(selected: string[] = SELECTED) {
  return mount(BatchBar, {
    props: { selected, knownTags: ['随笔', '构建'] },
    attachTo: document.body,
  })
}

function button(wrapper: ReturnType<typeof mountBar>, text: string) {
  return wrapper.findAll('button').find((b) => b.text().includes(text))!
}

async function tick() {
  await new Promise((resolve) => setTimeout(resolve))
}

describe('BatchBar', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    actions.batchPreview.mockResolvedValue(movePreview)
    state.busy = false
  })

  it('一篇都没勾时只给指路，不摆出动作按钮', () => {
    const wrapper = mountBar([])
    expect(wrapper.text()).toContain('勾选左侧条目')
    expect(wrapper.find('input[aria-label="批量标签"]').exists()).toBe(false)
  })

  it('「全选当前」与「清空」交回上层：勾选框长在列表每一行上，不在这里', async () => {
    const wrapper = mountBar()
    await button(wrapper, '全选当前').trigger('click')
    expect(wrapper.emitted('selectVisible')).toHaveLength(1)

    await button(wrapper, '清空').trigger('click')
    expect(wrapper.emitted('clear')).toHaveLength(1)
  })

  it('加减标签反手能改回来，所以不设确认；做完清空输入与选择', async () => {
    const wrapper = mountBar()
    await wrapper.get('input[aria-label="批量标签"]').setValue('随笔, 构建')

    await button(wrapper, '加').trigger('click')
    await tick()
    expect(actions.batchEditTags).toHaveBeenCalledWith(SELECTED, ['随笔', '构建'], [])
    expect((wrapper.get('input[aria-label="批量标签"]').element as HTMLInputElement).value).toBe('')
    expect(wrapper.emitted('clear')).toHaveLength(1)

    await wrapper.get('input[aria-label="批量标签"]').setValue('随笔')
    await button(wrapper, '去').trigger('click')
    await tick()
    expect(actions.batchEditTags).toHaveBeenLastCalledWith(SELECTED, [], ['随笔'])
  })

  it('标签框空着时按钮置灰，也不会发出空操作', async () => {
    const wrapper = mountBar()
    expect(button(wrapper, '加').attributes('disabled')).toBeDefined()
    expect(actions.batchEditTags).not.toHaveBeenCalled()
  })

  it('发布与设为草稿直接执行', async () => {
    const wrapper = mountBar()
    await button(wrapper, '发布').trigger('click')
    await tick()
    expect(actions.batchSetDraft).toHaveBeenCalledWith(SELECTED, false)

    await button(wrapper, '设为草稿').trigger('click')
    await tick()
    expect(actions.batchSetDraft).toHaveBeenLastCalledWith(SELECTED, true)
  })

  it('移动先干跑：清单要说清哪几篇会动、哪几篇不动，以及会改别人文件里的链接', async () => {
    const wrapper = mountBar()
    await wrapper.get('input[aria-label="目标栏目"]').setValue('notes')
    await button(wrapper, '移动…').trigger('click')
    await tick()

    expect(actions.batchPreview).toHaveBeenCalledWith(SELECTED, {
      kind: 'move',
      to_section: 'notes',
    })
    expect(actions.batchMove).not.toHaveBeenCalled()

    const dry = wrapper.get('.page-list__dry')
    expect(dry.text()).toContain('将搬动 1 / 2 篇')
    expect(dry.text()).toContain('旧地址 /posts/a/')
    // 不会动的那篇标成 skip，而不是从清单里消失
    expect(dry.findAll('li.skip')).toHaveLength(1)
    // 改到用户没勾的文件上这件事必须说出来
    expect(dry.text()).toContain('1 篇里的 2 处站内链接')
    expect(dry.text()).toContain('notes/c.md')
    // 相对链接改写不到，得人工看一眼——以前只能等死链体检
    expect(dry.text()).toContain('相对链接')
    expect(dry.text()).toContain('notes/d.md')
  })

  it('确认移动才落盘，且把「保留旧地址」一起带上', async () => {
    const wrapper = mountBar()
    await wrapper.get('input[aria-label="目标栏目"]').setValue('notes')
    await button(wrapper, '移动…').trigger('click')
    await tick()
    await button(wrapper, '确认移动').trigger('click')
    await tick()

    expect(actions.batchMove).toHaveBeenCalledWith(SELECTED, 'notes', true)
    expect(wrapper.emitted('clear')).toHaveLength(1)
    expect(wrapper.find('.page-list__dry').exists()).toBe(false)
  })

  it('删除的确认按钮用危险样式，确认后才真删', async () => {
    actions.batchPreview.mockResolvedValue(deletePreview)
    const wrapper = mountBar()
    await button(wrapper, '删除…').trigger('click')
    await tick()

    expect(actions.batchPreview).toHaveBeenCalledWith(SELECTED, { kind: 'delete' })
    const confirm = button(wrapper, '确认删除')
    expect(confirm.classes()).toContain('page-list__danger')
    expect(actions.batchDelete).not.toHaveBeenCalled()

    await confirm.trigger('click')
    await tick()
    expect(actions.batchDelete).toHaveBeenCalledWith(SELECTED)
  })

  it('一篇都不会变时不让确认：那次操作什么也不会发生', async () => {
    actions.batchPreview.mockResolvedValue({ ...movePreview, affected: 0 })
    const wrapper = mountBar()
    await button(wrapper, '移动…').trigger('click')
    await tick()

    expect(button(wrapper, '确认移动').attributes('disabled')).toBeDefined()
  })

  it('取消只收起清单，选择留着：好让人改个条件重试', async () => {
    const wrapper = mountBar()
    await button(wrapper, '移动…').trigger('click')
    await tick()
    await button(wrapper, '取消').trigger('click')

    expect(wrapper.find('.page-list__dry').exists()).toBe(false)
    expect(actions.batchMove).not.toHaveBeenCalled()
    expect(wrapper.emitted('clear')).toBeUndefined()
  })

  it('干跑失败（比如目标栏目非法）不进确认态', async () => {
    actions.batchPreview.mockResolvedValue(undefined)
    const wrapper = mountBar()
    await button(wrapper, '移动…').trigger('click')
    await tick()

    expect(wrapper.find('.page-list__dry').exists()).toBe(false)
  })

  it('忙的时候所有会写盘的按钮都置灰', async () => {
    const wrapper = mountBar()
    await wrapper.get('input[aria-label="批量标签"]').setValue('随笔')
    state.busy = true
    await wrapper.vm.$nextTick()

    for (const label of ['加', '去', '移动…', '发布', '设为草稿']) {
      expect(button(wrapper, label).attributes('disabled')).toBeDefined()
    }
  })

  /**
   * 事故复盘：「删除…」原先带的是 `page-list__icon`（行内次要动作的幽灵类）。
   *
   * 那个类在 `li` 之外没有任何点亮规则，于是这颗按钮**永久不可见却仍能点**，
   * Tab 也会停在上面——而删除是不可逆的。样式表那一侧由 `src/styles.test.ts` 把守，
   * 这里钉住组件的选择：不可逆的动作用危险样式，不用灰色的次要动作样式。
   */
  it('「删除…」用危险样式，不是灰色的次要动作', () => {
    const remove = button(mountBar(), '删除…')

    expect(remove.classes()).toContain('page-list__danger')
    expect(remove.classes()).not.toContain('page-list__icon')
  })
})
