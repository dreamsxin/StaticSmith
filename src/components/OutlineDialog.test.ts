// @vitest-environment jsdom
import { mount } from '@vue/test-utils'
import { describe, expect, it } from 'vitest'

import OutlineDialog from './OutlineDialog.vue'

/**
 * 大纲浮层的行为测试。
 *
 * 这是第一批组件测试。之前整个前端只测纯函数，浮层的焦点、键盘、关闭路径全靠人工推理——
 * 而这几条恰恰是「写了 `aria-modal` 就必须真的圈住焦点」这类约定的落地处
 * （见 docs/ui.md「所有浮层的共同约定」）。
 *
 * 挂到真实 document 上（`attachTo`）：焦点相关的断言在游离节点上永远是 `body`。
 */
const SOURCE = ['+++', 'title = "甲"', '+++', '', '# 大标题', '', '## 一节', '', '正文'].join('\n')

function open(source = SOURCE) {
  const opener = document.createElement('button')
  document.body.appendChild(opener)
  opener.focus()
  const wrapper = mount(OutlineDialog, {
    props: { open: false, source },
    attachTo: document.body,
  })
  return { wrapper, opener }
}

describe('OutlineDialog', () => {
  it('列出标题，并标出层级', async () => {
    const { wrapper } = open()
    await wrapper.setProps({ open: true })

    const items = wrapper.findAll('.outline__item')
    expect(items).toHaveLength(2)
    expect(items[0].text()).toContain('大标题')
    expect(items[0].classes()).toContain('outline__item--h1')
    expect(items[1].classes()).toContain('outline__item--h2')
  })

  it('点一条就跳过去并关掉：跳完还留着浮层会挡住刚跳到的那一行', async () => {
    const { wrapper } = open()
    await wrapper.setProps({ open: true })

    await wrapper.findAll('.outline__item')[1].trigger('click')
    // 第二个标题在源文里的偏移
    expect(wrapper.emitted('jump')).toEqual([[SOURCE.indexOf('## 一节')]])
    expect(wrapper.emitted('close')).toHaveLength(1)
  })

  it('上下键移动高亮、回车跳转，且首尾相接', async () => {
    const { wrapper } = open()
    await wrapper.setProps({ open: true })
    const list = wrapper.get('.outline')

    await list.trigger('keydown', { key: 'ArrowDown' })
    expect(wrapper.findAll('.outline__item')[1].classes()).toContain('active')
    // 往下越界回到第一条
    await list.trigger('keydown', { key: 'ArrowDown' })
    expect(wrapper.findAll('.outline__item')[0].classes()).toContain('active')
    // 往上越界到最后一条
    await list.trigger('keydown', { key: 'ArrowUp' })
    expect(wrapper.findAll('.outline__item')[1].classes()).toContain('active')

    await list.trigger('keydown', { key: 'Enter' })
    expect(wrapper.emitted('jump')).toEqual([[SOURCE.indexOf('## 一节')]])
  })

  it('高亮项写进 aria-activedescendant：读屏器据此念出停在哪一条', async () => {
    const { wrapper } = open()
    await wrapper.setProps({ open: true })
    const list = wrapper.get('.outline')
    expect(list.attributes('aria-activedescendant')).toBe('outline-0')

    await list.trigger('keydown', { key: 'ArrowDown' })
    expect(list.attributes('aria-activedescendant')).toBe('outline-1')
  })

  it('Esc 关闭；点遮罩空白也关，点框里不关', async () => {
    const { wrapper } = open()
    await wrapper.setProps({ open: true })

    await wrapper.get('[role="dialog"]').trigger('keydown', { key: 'Escape' })
    expect(wrapper.emitted('close')).toHaveLength(1)

    // 点框内不该冒泡成「关闭」——click.self 只认落在遮罩本身的点击
    await wrapper.get('[role="dialog"]').trigger('click')
    expect(wrapper.emitted('close')).toHaveLength(1)

    await wrapper.get('.palette').trigger('click')
    expect(wrapper.emitted('close')).toHaveLength(2)
  })

  it('没有标题时给空态，不渲染一个空列表框', async () => {
    const { wrapper } = open('+++\n+++\n\n只有正文，没有标题。\n')
    await wrapper.setProps({ open: true })

    expect(wrapper.find('.outline').exists()).toBe(false)
    expect(wrapper.text()).toContain('还没有标题')
  })

  it('打开即把焦点放进浮层：有目录进列表，没目录进关闭按钮', async () => {
    const { wrapper } = open()
    await wrapper.setProps({ open: true })
    await new Promise((resolve) => setTimeout(resolve))
    expect(document.activeElement).toBe(wrapper.get('.outline').element)

    const empty = open('+++\n+++\n\n没有标题\n')
    await empty.wrapper.setProps({ open: true })
    await new Promise((resolve) => setTimeout(resolve))
    expect(document.activeElement).toBe(empty.wrapper.get('.dialog__cancel').element)
  })

  it('关闭后焦点还给打开它的那个元素：留在已消失的浮层上等于焦点丢了', async () => {
    const { wrapper, opener } = open()
    await wrapper.setProps({ open: true })
    await new Promise((resolve) => setTimeout(resolve))
    expect(document.activeElement).not.toBe(opener)

    await wrapper.setProps({ open: false })
    expect(document.activeElement).toBe(opener)
  })
})
