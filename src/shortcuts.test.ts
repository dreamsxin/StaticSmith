import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'

import { describe, expect, it } from 'vitest'

import { footerShortcuts, SCOPES, SHORTCUTS, shortcutsIn } from './shortcuts'

/**
 * 快捷键表。
 *
 * 这份表是**文档**，而按键的实现在各自的 `keydown` 里——文档与实现分开就会漂。
 * 所以除了自身一致性，这里还读 `App.vue` 的源码做一次**对账**：
 * 表里说「随处可用」的键，window 监听里必须真的有。
 * 漂了的后果不是报错，是「一览里写着，按下去没反应」——比不写更糟。
 */
const read = (name: string) =>
  readFileSync(fileURLToPath(new URL(name, import.meta.url)), 'utf8')

describe('快捷键表', () => {
  it('每个作用域都有内容，而且没有表外的作用域', () => {
    const declared = new Set(SCOPES.map((scope) => scope.id))
    const used = new Set(SHORTCUTS.map((item) => item.scope))

    expect([...used].every((scope) => declared.has(scope))).toBe(true)
    for (const scope of SCOPES) {
      expect(shortcutsIn(scope.id).length, scope.label).toBeGreaterThan(0)
    }
  })

  it('键位不重复：同一个组合出现两次，一览里就会自相矛盾', () => {
    const keys = SHORTCUTS.map((item) => item.keys)
    expect(new Set(keys).size).toBe(keys.length)
  })

  it('页脚点名的键位都在表里（点名了不存在的会直接抛）', () => {
    const footer = footerShortcuts()
    expect(footer.length).toBeGreaterThan(4)
    expect(footer.every((item) => SHORTCUTS.includes(item))).toBe(true)
  })

  /**
   * 对账：表里「随处可用」的 Ctrl 组合，必须在 `App.vue` 的 window 监听里真的处理了。
   *
   * 那段代码的写法是 `if (key === 'p')`，所以按字母对账就够。
   * 这条测试拦的是「加了键位忘了写实现」和「删了实现忘了删键位」两种漂移。
   */
  it('「随处可用」的键位与 App.vue 的 window 监听对得上', () => {
    const app = read('./App.vue')
    const handled = new Set([...app.matchAll(/key === '(\w+)'/g)].map((match) => match[1]))
    expect(handled.size).toBeGreaterThan(3)

    for (const item of shortcutsIn('global')) {
      // `Ctrl+Shift+Enter` 与 `Ctrl+Enter` 是同一个分支里看 shiftKey，所以只取最后一段
      const last = item.keys.split('+').pop()!.toLowerCase()
      expect(handled.has(last), `${item.keys} 表里有，App.vue 里没处理`).toBe(true)
    }
  })

  /** 同样对账编辑器那一组：它们写在 `ContentEditor.vue` 的 `onKeydown` 里。 */
  it('「在编辑器里」的键位与 ContentEditor 的监听对得上', () => {
    const editor = read('./components/ContentEditor.vue')
    const start = editor.indexOf('function onKeydown')
    const body = editor.slice(start, editor.indexOf('\n}', start))
    expect(body).toContain('ctrlKey')

    for (const item of shortcutsIn('editor')) {
      const last = item.keys.split('+').pop()!.toLowerCase()
      expect(body.includes(`${last}:`) || body.includes(`'${last}'`), item.keys).toBe(true)
    }
  })
})
