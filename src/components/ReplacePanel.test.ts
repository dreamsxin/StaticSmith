// @vitest-environment jsdom
import { mount } from '@vue/test-utils'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { reactive } from 'vue'

/**
 * 跨文件替换的行为测试。
 *
 * 这个动作**没有撤销栈**：改错一个词不会报错，只会安静地把内容改坏。所以要钉住的
 * 不是渲染，而是三件保证：没预览过不许替换、预览与当前输入必须对得上、
 * 范围（全站还是只改选中）传对。
 */
const result = {
  files: [
    {
      source: 'posts/a.md',
      hits: 3,
      lines: [{ line: 4, before: '旧称呼在这', after: '新称呼在这' }],
    },
  ],
  hits: 3,
  skipped: [{ source: 'posts/broken.md', reason: 'front matter 读不出来' }],
}

const state = reactive({ busy: false })

const actions = {
  previewReplace: vi.fn(async () => result as typeof result | undefined),
  applyReplace: vi.fn(async () => {}),
}

vi.mock('../store', () => ({ store: state, actions }))

const ReplacePanel = (await import('./ReplacePanel.vue')).default

function mountPanel(selected: string[] = []) {
  return mount(ReplacePanel, {
    props: { open: false, selected },
    attachTo: document.body,
  })
}

async function tick() {
  await new Promise((resolve) => setTimeout(resolve))
}

/** 展开面板、填好查找词、按「预览…」，走到「等确认」那一步。 */
async function askPreview(wrapper: ReturnType<typeof mountPanel>, find = '旧称呼') {
  await wrapper.setProps({ open: true })
  await wrapper.get('input[type="text"]').setValue(find)
  await wrapper.get('form').trigger('submit')
  await tick()
}

function button(wrapper: ReturnType<typeof mountPanel>, text: string) {
  return wrapper.findAll('button').find((b) => b.text().includes(text))!
}

describe('ReplacePanel', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    actions.previewReplace.mockResolvedValue(result)
    state.busy = false
  })

  it('展开即聚焦「查找」，少一次点击', async () => {
    const wrapper = mountPanel()
    await wrapper.setProps({ open: true })
    await tick()
    expect(document.activeElement).toBe(wrapper.get('input[type="text"]').element)
  })

  it('提交只干跑，落盘要再点一次；清单里要能看清哪几行怎么变', async () => {
    const wrapper = mountPanel()
    await askPreview(wrapper)

    expect(actions.previewReplace).toHaveBeenCalledWith({
      find: '旧称呼',
      replace: '',
      ignore_case: false,
      sources: [],
    })
    expect(actions.applyReplace).not.toHaveBeenCalled()

    const dry = wrapper.get('.page-list__dry').text()
    expect(dry).toContain('1 篇、共 3 处')
    expect(dry).toContain('posts/a.md')
    expect(dry).toContain('第 4 行')
    expect(dry).toContain('旧称呼在这')
    expect(dry).toContain('新称呼在这')
    // 只列了 1 行、实际 3 处，差额要说出来
    expect(dry).toContain('另有 2 处未列出')
    // 读不出来的那篇也要报，别让它悄悄不算
    expect(dry).toContain('posts/broken.md')
  })

  it('确认之后才落盘，并收起面板', async () => {
    const wrapper = mountPanel()
    await askPreview(wrapper)
    await button(wrapper, '替换这 3 处').trigger('click')

    expect(actions.applyReplace).toHaveBeenCalledWith({
      find: '旧称呼',
      replace: '',
      ignore_case: false,
      sources: [],
    })
    expect(wrapper.emitted('close')).toHaveLength(1)
  })

  it('改了任一条件，上一份预览立刻作废：不然确认的是屏幕上那份、执行的是新条件', async () => {
    const wrapper = mountPanel()
    await askPreview(wrapper)
    expect(wrapper.find('.page-list__dry').exists()).toBe(true)

    await wrapper.get('input[type="text"]').setValue('换了个词')
    expect(wrapper.find('.page-list__dry').exists()).toBe(false)

    // 勾「忽略大小写」同样作废
    await askPreview(wrapper, '旧称呼')
    await wrapper.get('input[type="checkbox"]').setValue(true)
    expect(wrapper.find('.page-list__dry').exists()).toBe(false)
  })

  it('外部选中集合变了也作废：范围变了，命中清单当然不作数', async () => {
    const wrapper = mountPanel(['posts/a.md'])
    await askPreview(wrapper)
    expect(wrapper.find('.page-list__dry').exists()).toBe(true)

    await wrapper.setProps({ selected: ['posts/a.md', 'posts/b.md'] })
    expect(wrapper.find('.page-list__dry').exists()).toBe(false)
  })

  it('勾了「只改选中的」才把范围收窄，不勾就是全站（空数组）', async () => {
    const wrapper = mountPanel(['posts/a.md', 'posts/b.md'])
    await wrapper.setProps({ open: true })
    await wrapper.get('input[type="text"]').setValue('旧称呼')

    const boxes = wrapper.findAll('input[type="checkbox"]')
    await boxes[1].setValue(true)
    await wrapper.get('form').trigger('submit')
    await tick()

    expect(actions.previewReplace).toHaveBeenLastCalledWith({
      find: '旧称呼',
      replace: '',
      ignore_case: false,
      sources: ['posts/a.md', 'posts/b.md'],
    })
  })

  it('没有选中任何文章时，「只改选中的」置灰', async () => {
    const wrapper = mountPanel()
    await wrapper.setProps({ open: true })
    const boxes = wrapper.findAll('input[type="checkbox"]')
    expect(boxes[1].attributes('disabled')).toBeDefined()
  })

  it('一处都没命中时说清楚，并且不给按「替换」', async () => {
    actions.previewReplace.mockResolvedValue({ files: [], hits: 0, skipped: [] })
    const wrapper = mountPanel()
    await askPreview(wrapper, '不存在的词')

    expect(wrapper.get('.page-list__dry').text()).toContain('没有找到这段文字')
    expect(button(wrapper, '替换这 0 处').attributes('disabled')).toBeDefined()
  })

  it('查找词为空时连预览都不给按：那会命中整个站点', async () => {
    const wrapper = mountPanel()
    await wrapper.setProps({ open: true })
    expect(button(wrapper, '预览…').attributes('disabled')).toBeDefined()
  })

  it('取消什么也不做', async () => {
    const wrapper = mountPanel()
    await askPreview(wrapper)
    await button(wrapper, '取消').trigger('click')

    expect(actions.applyReplace).not.toHaveBeenCalled()
    expect(wrapper.emitted('close')).toHaveLength(1)
  })

  it('关掉再打开时不留上一次的清单', async () => {
    const wrapper = mountPanel()
    await askPreview(wrapper)
    await wrapper.setProps({ open: false })
    await wrapper.setProps({ open: true })

    expect(wrapper.find('.page-list__dry').exists()).toBe(false)
  })

  it('忙的时候不让重复提交', async () => {
    const wrapper = mountPanel()
    await askPreview(wrapper)
    state.busy = true
    await wrapper.vm.$nextTick()

    expect(button(wrapper, '预览…').attributes('disabled')).toBeDefined()
    expect(button(wrapper, '替换这 3 处').attributes('disabled')).toBeDefined()
  })
})
