// @vitest-environment jsdom
import { mount } from '@vue/test-utils'
import { describe, expect, it } from 'vitest'

import { SCOPES, SHORTCUTS } from '../shortcuts'
import ShortcutsDialog from './ShortcutsDialog.vue'

/**
 * 快捷键一览的行为测试。
 *
 * 这个浮层的价值全在「一次看完、分组正确」：只要漏渲染一组或漏几行，
 * 它就退化成原先那个名不副实的入口（帮助菜单里点开只是命令面板）。
 * 所以第一条测试直接钉「表里有多少行，界面上就有多少行」。
 *
 * 焦点与关闭路径照 docs/ui.md「所有浮层的共同约定」，与 OutlineDialog 同一套断言。
 * 挂到真实 document 上（`attachTo`）：焦点断言在游离节点上永远是 `body`。
 */
function open() {
  const opener = document.createElement('button')
  document.body.appendChild(opener)
  opener.focus()
  const wrapper = mount(ShortcutsDialog, { props: { open: false }, attachTo: document.body })
  return { wrapper, opener }
}

describe('ShortcutsDialog', () => {
  it('每个作用域一组，表里的每个键位都露面——漏一行就等于没告诉用户', async () => {
    const { wrapper } = open()
    await wrapper.setProps({ open: true })

    const groups = wrapper.findAll('.shortcuts__group')
    expect(groups).toHaveLength(SCOPES.length)
    expect(groups.map((group) => group.get('.shortcuts__scope').text())).toEqual(
      SCOPES.map((scope) => expect.stringContaining(scope.label)),
    )

    const rows = wrapper.findAll('.shortcuts__row')
    expect(rows).toHaveLength(SHORTCUTS.length)
    for (const item of SHORTCUTS) {
      const row = rows.find((candidate) => candidate.get('.shortcuts__keys').text() === item.keys)
      expect(row, item.keys).toBeTruthy()
      expect(row!.text()).toContain(item.what)
    }
  })

  it('打开前什么都不渲染：常驻的隐藏浮层会把 Tab 停在看不见的地方', () => {
    const { wrapper } = open()
    expect(wrapper.find('.palette').exists()).toBe(false)
  })

  it('Esc 关闭；点遮罩空白也关，点框里不关', async () => {
    const { wrapper } = open()
    await wrapper.setProps({ open: true })

    await wrapper.get('[role="dialog"]').trigger('keydown', { key: 'Escape' })
    expect(wrapper.emitted('close')).toHaveLength(1)

    await wrapper.get('[role="dialog"]').trigger('click')
    expect(wrapper.emitted('close')).toHaveLength(1)

    await wrapper.get('.palette').trigger('click')
    expect(wrapper.emitted('close')).toHaveLength(2)
  })

  it('打开即把焦点放到关闭按钮上：这里没有可操作的列表，Tab 一圈就是「读完就走」', async () => {
    const { wrapper } = open()
    await wrapper.setProps({ open: true })
    await new Promise((resolve) => setTimeout(resolve))
    expect(document.activeElement).toBe(wrapper.get('.dialog__cancel').element)
  })

  it('关闭后焦点还给打开它的那个元素', async () => {
    const { wrapper, opener } = open()
    await wrapper.setProps({ open: true })
    await new Promise((resolve) => setTimeout(resolve))
    expect(document.activeElement).not.toBe(opener)

    await wrapper.setProps({ open: false })
    expect(document.activeElement).toBe(opener)
  })
})
