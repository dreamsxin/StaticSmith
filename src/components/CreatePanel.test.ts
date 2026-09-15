// @vitest-environment jsdom
import { mount } from '@vue/test-utils'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { reactive } from 'vue'

/**
 * 「新建内容」「新建栏目」两张表单的行为测试。
 *
 * 这两张表单是站点里唯一的「从无到有」入口，出错的方式不是报错而是**建到不该建的地方**：
 * 栏目填 posts、路径填 notes/x.md 这种自相矛盾的输入，或者上一次「在此栏目新建」
 * 带进来的栏目没被清掉，于是文章落到隔壁栏目。所以要钉的是：
 * 交给 store 的三个参数、路径与栏目的互斥、以及两张表单本身不许同时出现。
 */
const state = reactive({ busy: false })

const actions = {
  createContent: vi.fn(async () => {}),
  createSection: vi.fn(async () => {}),
}

vi.mock('../store', () => ({ store: state, actions }))

const CreatePanel = (await import('./CreatePanel.vue')).default

type Mode = 'content' | 'section' | null

function mountPanel(mode: Mode = null, defaultSection = 'posts') {
  return mount(CreatePanel, {
    props: { mode, defaultSection },
    attachTo: document.body,
  })
}

async function tick() {
  await new Promise((resolve) => setTimeout(resolve))
}

function button(wrapper: ReturnType<typeof mountPanel>, text: string) {
  return wrapper.findAll('button').find((b) => b.text().includes(text))!
}

/** 按标签文字取输入框：表单里同类型的框有好几个，按顺序取容易挪一下就错。 */
function field(wrapper: ReturnType<typeof mountPanel>, label: string) {
  const box = wrapper.findAll('label').find((l) => l.text().includes(label))
  return box!.get('input')
}

describe('CreatePanel', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    state.busy = false
  })

  it('没展开时什么也不渲染', () => {
    const wrapper = mountPanel()
    expect(wrapper.find('form').exists()).toBe(false)
  })

  it('两张表单永远不会同时出现：mode 只能是其中一个', async () => {
    const wrapper = mountPanel('section')
    expect(wrapper.findAll('form')).toHaveLength(1)
    expect(wrapper.get('h3').text()).toBe('新建栏目')

    await wrapper.setProps({ mode: 'content' })
    expect(wrapper.findAll('form')).toHaveLength(1)
    expect(wrapper.get('h3').text()).toBe('新建内容')
  })

  it('展开「新建内容」即聚焦标题，少一次点击', async () => {
    const wrapper = mountPanel()
    await wrapper.setProps({ mode: 'content' })
    await tick()
    expect(document.activeElement).toBe(field(wrapper, '标题').element)
  })

  it('展开「新建栏目」聚焦目录名', async () => {
    const wrapper = mountPanel()
    await wrapper.setProps({ mode: 'section' })
    await tick()
    expect(document.activeElement).toBe(field(wrapper, '目录名').element)
  })

  it('提交把标题、栏目、路径去空白后交给 store，并收起表单', async () => {
    const wrapper = mountPanel('content')
    await field(wrapper, '标题').setValue('  你好  ')
    await field(wrapper, '路径').setValue('  posts/2026/hello.md ')
    await wrapper.get('form').trigger('submit')
    await tick()

    expect(actions.createContent).toHaveBeenCalledWith('你好', 'posts', 'posts/2026/hello.md')
    expect(wrapper.emitted('close')).toHaveLength(1)
  })

  it('「在此栏目新建文章」带进来的栏目要预填进表单', async () => {
    const wrapper = mountPanel(null, 'posts')
    await wrapper.setProps({ mode: 'content', defaultSection: 'notes/2026' })
    expect((field(wrapper, '栏目').element as HTMLInputElement).value).toBe('notes/2026')

    await field(wrapper, '标题').setValue('随手记')
    await wrapper.get('form').trigger('submit')
    await tick()
    expect(actions.createContent).toHaveBeenCalledWith('随手记', 'notes/2026', '')
  })

  it('填了路径就把栏目置灰：栏目 posts、路径 notes/x.md 是自相矛盾的输入', async () => {
    const wrapper = mountPanel('content')
    expect(field(wrapper, '栏目').attributes('disabled')).toBeUndefined()
    await field(wrapper, '路径').setValue('notes/x.md')
    expect(field(wrapper, '栏目').attributes('disabled')).toBeDefined()
  })

  it('标题为空不给提交，也不会偷偷调 store', async () => {
    const wrapper = mountPanel('content')
    expect(button(wrapper, '创建草稿').attributes('disabled')).toBeDefined()

    await field(wrapper, '标题').setValue('   ')
    await wrapper.get('form').trigger('submit')
    await tick()
    expect(actions.createContent).not.toHaveBeenCalled()
  })

  it('新建栏目：目录名、标题、简介三个参数都去空白后交给 store', async () => {
    const wrapper = mountPanel('section')
    await field(wrapper, '目录名').setValue(' notes ')
    await field(wrapper, '栏目标题').setValue(' 随手记 ')
    await field(wrapper, '栏目简介').setValue(' 平时的零碎想法 ')
    await wrapper.get('form').trigger('submit')
    await tick()

    expect(actions.createSection).toHaveBeenCalledWith('notes', '随手记', '平时的零碎想法')
    expect(wrapper.emitted('close')).toHaveLength(1)
  })

  it('目录名为空不给建栏目', async () => {
    const wrapper = mountPanel('section')
    expect(button(wrapper, '创建栏目').attributes('disabled')).toBeDefined()

    await wrapper.get('form').trigger('submit')
    await tick()
    expect(actions.createSection).not.toHaveBeenCalled()
  })

  it('建成之后清空输入：下一次展开不该接着上一篇的标题往下写', async () => {
    const wrapper = mountPanel('content')
    await field(wrapper, '标题').setValue('第一篇')
    await wrapper.get('form').trigger('submit')
    await tick()

    await wrapper.setProps({ mode: null })
    await wrapper.setProps({ mode: 'content' })
    expect((field(wrapper, '标题').element as HTMLInputElement).value).toBe('')
  })

  it('栏目记住上一次填的：连着往同一个栏目里写几篇不该每次重填', async () => {
    const wrapper = mountPanel('content')
    await field(wrapper, '栏目').setValue('notes')
    await field(wrapper, '标题').setValue('第一篇')
    await wrapper.get('form').trigger('submit')
    await tick()

    await wrapper.setProps({ mode: null })
    await wrapper.setProps({ mode: 'content' })
    expect((field(wrapper, '栏目').element as HTMLInputElement).value).toBe('notes')
  })

  it('取消保留已填的内容：误点一下不该把刚写的标题丢掉', async () => {
    const wrapper = mountPanel('content')
    await field(wrapper, '标题').setValue('写了一半')
    await button(wrapper, '取消').trigger('click')

    expect(actions.createContent).not.toHaveBeenCalled()
    expect(wrapper.emitted('close')).toHaveLength(1)

    await wrapper.setProps({ mode: null })
    await wrapper.setProps({ mode: 'content' })
    expect((field(wrapper, '标题').element as HTMLInputElement).value).toBe('写了一半')
  })


  it('栏目记住上一次填的：连着往同一个栏目里写几篇不该每次重填', async () => {
    const wrapper = mountPanel('content')
    await field(wrapper, '栏目').setValue('notes')
    await field(wrapper, '标题').setValue('第一篇')
    await wrapper.get('form').trigger('submit')
    await tick()

    await wrapper.setProps({ mode: 'content' })
    expect((field(wrapper, '栏目').element as HTMLInputElement).value).toBe('notes')
  })

  it('取消保留已填的内容：误点一下不该把刚写的标题丢掉', async () => {

    const wrapper = mountPanel('content')
    await field(wrapper, '标题').setValue('写了一半')
    await button(wrapper, '取消').trigger('click')

    expect(actions.createContent).not.toHaveBeenCalled()
    expect(wrapper.emitted('close')).toHaveLength(1)

    await wrapper.setProps({ mode: 'content' })
    expect((field(wrapper, '标题').element as HTMLInputElement).value).toBe('写了一半')
  })

  it('已经展开着又被要求新建一次时，光标要回到第一格', async () => {
    const wrapper = mountPanel('content')
    await tick()
    ;(field(wrapper, '路径').element as HTMLInputElement).focus()

    // 菜单里的「新建文章…」在表单已展开时按下：mode 没变，所以只能由上层叫这一下
    await (wrapper.vm as unknown as { focusFirst: () => Promise<void> }).focusFirst()
    expect(document.activeElement).toBe(field(wrapper, '标题').element)
  })

  it('忙的时候不让重复提交，但取消一直能按', async () => {
    const wrapper = mountPanel('content')
    await field(wrapper, '标题').setValue('你好')
    state.busy = true
    await wrapper.vm.$nextTick()

    expect(button(wrapper, '创建草稿').attributes('disabled')).toBeDefined()
    expect(button(wrapper, '取消').attributes('disabled')).toBeUndefined()
  })
})
