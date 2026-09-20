// @vitest-environment jsdom
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'

import { mount } from '@vue/test-utils'
import { describe, expect, it } from 'vitest'

import RefsNote from './RefsNote.vue'

/**
 * 「这次改动会顺手改写别人文章里的链接」那两句提示。
 *
 * 它们原先在四个地方各写了一遍，措辞已经开始分叉（有的列文件名、有的不列）。
 * 同一件事在两个地方长得不一样，用户会以为是两件事——所以收进一个组件，
 * 并在这里钉住「谁还在自己写这句」。
 */
const read = (name: string) =>
  readFileSync(fileURLToPath(new URL(name, import.meta.url)), 'utf8')

/** 两句里信息量最大、也最容易被改走形的那一段。 */
const MANUAL = '处<strong>相对链接</strong>（<code>../a/</code> 这类）指向它，改写不到，需要手工改'

describe('RefsNote', () => {
  it('两句分开报：上面那句是知会，下面那句是待办', () => {
    const wrapper = mount(RefsNote, {
      props: {
        refs: [
          { source: 'posts/a.md', hits: 2 },
          { source: 'posts/b.md', hits: 1 },
        ],
        manual: [{ source: 'notes/c.md', hits: 4 }],
      },
    })

    const notes = wrapper.findAll('.refs-note')
    expect(notes).toHaveLength(2)
    // 处数是求和，不是篇数
    expect(notes[0].text()).toContain('2 篇里的 3 处')
    expect(notes[0].text()).toContain('posts/a.md、posts/b.md')
    expect(notes[1].text()).toContain('1 篇里的 4 处')
    expect(notes[1].text()).toContain('需要手工改')
  })

  it('没有就不渲染：留一个空段落会在确认块里撑出一条空行', () => {
    const wrapper = mount(RefsNote, { props: { refs: [], manual: [] } })
    expect(wrapper.find('.refs-note').exists()).toBe(false)
  })

  it('compact 只报数不列名：地方窄的时候列名会把确认块顶得很长', () => {
    const wrapper = mount(RefsNote, {
      props: { refs: [{ source: 'posts/a.md', hits: 1 }], manual: [], compact: true },
    })
    expect(wrapper.get('.refs-note').text()).toContain('1 篇里的 1 处')
    expect(wrapper.get('.refs-note').text()).not.toContain('posts/a.md')
  })

  /** Vue 把没传的布尔 prop 当成 false，所以「默认列名」必须写成反义的开关。 */
  it('不传 compact 就列名：默认值踩错了会悄悄少掉「改了哪几篇」', () => {
    const wrapper = mount(RefsNote, {
      props: { refs: [{ source: 'posts/a.md', hits: 1 }], manual: [] },
    })
    expect(wrapper.get('.refs-note').text()).toContain('posts/a.md')
  })

  /**
   * 编辑器里改地址那一处是**行内**文字（嵌在一句话中间），套这个组件会把紧凑的一行
   * 拆成两段，所以刻意没用。但措辞必须逐字一致——否则同一件事在两处长得不一样。
   * 这条测试拿源码对账，也顺便拦住「第四处又自己抄一遍」。
   */
  it('措辞只有两个出处：组件本身，和编辑器里那句行内的', () => {
    expect(read('./RefsNote.vue')).toContain(MANUAL)
    expect(read('./ContentEditor.vue')).toContain(MANUAL)

    for (const name of ['./BatchBar.vue', './SectionHeader.vue', './PageList.vue']) {
      expect(read(name), name).not.toContain(MANUAL)
    }
  })
})
