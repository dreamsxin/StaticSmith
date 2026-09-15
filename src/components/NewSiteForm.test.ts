// @vitest-environment jsdom
import { mount } from '@vue/test-utils'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { reactive } from 'vue'

/**
 * 「新建站点」表单的行为测试。
 *
 * 它是最后一个没测过的写入口：按下去会在磁盘上铺出一整个项目目录。两个宿主共用它
 * （起始页的卡片、菜单栏的对话框），所以「交给宿主的是什么」必须钉住——
 * 版式选错了，铺出来的是另一套模板与样式，而那时项目已经建好了。
 */
const state = reactive({
  presets: [] as Array<{ slug: string; title: string; description: string }>,
  busy: false,
})

const actions = {
  loadPresets: vi.fn(async () => {
    state.presets = [
      { slug: 'blog', title: '博客', description: '按时间倒序的文章流' },
      { slug: 'docs', title: '文档', description: '侧栏导航加正文' },
    ]
  }),
}

vi.mock('../store', () => ({ store: state, actions }))

const NewSiteForm = (await import('./NewSiteForm.vue')).default

function mountForm() {
  return mount(NewSiteForm, { props: { submitLabel: '选目录并创建' }, attachTo: document.body })
}

async function tick() {
  await new Promise((resolve) => setTimeout(resolve))
}

describe('NewSiteForm', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    state.presets = []
    state.busy = false
  })

  it('挂载即读版式清单，并默认选中第一项（与 Rust 侧的默认一致）', async () => {
    const wrapper = mountForm()
    await tick()

    expect(actions.loadPresets).toHaveBeenCalled()
    const radios = wrapper.findAll('input[type="radio"]')
    expect(radios).toHaveLength(2)
    expect((radios[0].element as HTMLInputElement).checked).toBe(true)
  })

  it('提交把去空白的名称与选中的版式交给宿主', async () => {
    const wrapper = mountForm()
    await tick()

    await wrapper.get('input[type="text"]').setValue('  我的站  ')
    await wrapper.findAll('input[type="radio"]')[1].setValue()
    await wrapper.get('form').trigger('submit')

    expect(wrapper.emitted('submit')).toEqual([['我的站', 'docs']])
  })

  it('名称为空不给提交，也不会偷偷交给宿主', async () => {
    const wrapper = mountForm()
    await tick()

    await wrapper.get('input[type="text"]').setValue('   ')
    expect(wrapper.get('button[type="submit"]').attributes('disabled')).toBeDefined()

    await wrapper.get('form').trigger('submit')
    expect(wrapper.emitted('submit')).toBeUndefined()
  })

  it('只有一套版式时不摆单选组：一个选项等于没有选择', async () => {
    actions.loadPresets.mockImplementationOnce(async () => {
      state.presets = [{ slug: 'blog', title: '博客', description: '只有这一套' }]
    })
    const wrapper = mountForm()
    await tick()

    expect(wrapper.find('fieldset').exists()).toBe(false)
    // 但它仍然是被选中的那一套，提交时不能变成空
    await wrapper.get('form').trigger('submit')
    expect(wrapper.emitted('submit')).toEqual([['我的静态站', 'blog']])
  })

  it('版式清单读不出来时仍然建得成：版式交空串，由 Rust 侧回落默认', async () => {
    actions.loadPresets.mockImplementationOnce(async () => {
      // store 那边会报一条提示，这里只关心表单还能不能用
      state.presets = []
    })
    const wrapper = mountForm()
    await tick()

    await wrapper.get('form').trigger('submit')
    expect(wrapper.emitted('submit')).toEqual([['我的静态站', '']])
  })

  it('忙的时候不让重复提交', async () => {
    const wrapper = mountForm()
    await tick()
    state.busy = true
    await wrapper.vm.$nextTick()

    expect(wrapper.get('button[type="submit"]').attributes('disabled')).toBeDefined()
  })

  it('把 focus 暴露给宿主：对话框打开后要把光标放进名称框', async () => {
    const wrapper = mountForm()
    await tick()
    ;(wrapper.vm as unknown as { focus: () => void }).focus()

    expect(document.activeElement).toBe(wrapper.get('input[type="text"]').element)
  })
})
