import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'

import { describe, expect, it } from 'vitest'

/**
 * 样式表的几条硬约定。
 *
 * 为什么要给 CSS 写测试：这一批毛病**任何别的测试都抓不到**。
 * 组件测试挂载在 jsdom 里，根本不加载 `styles.css`；类型检查管不到样式；
 * 而「按钮永久不可见但仍能点」在 diff 里长得和一条正常的样式规则一模一样。
 * 这类规则是靠人眼跑一遍界面才发现的——跑一遍要几分钟，读一遍文件要几毫秒。
 *
 * 只钉**有过真实事故**的规则，不做泛泛的风格检查（那是 linter 的事）。
 *
 * 读原文而不是 `import './styles.css?raw'`：Vitest 会把所有 `.css` 请求换成空串，
 * 那样断言全对着空字符串通过，护栏是假的（`node:fs` 的类型声明见 `vite-env.d.ts`）。
 */
const css = readFileSync(fileURLToPath(new URL('./styles.css', import.meta.url)), 'utf8')
describe('styles.css 的硬约定', () => {
  /**
   * 事故：批量条的「全选当前」「清空」「删除…」三颗按钮永久不可见。
   *
   * 起因是 `.page-list button.page-list__icon { opacity: 0 }` 配一条
   * `li:hover` 的恢复规则——而那三颗按钮不在 `li` 里，于是没有任何东西能把它们点亮。
   * 它们仍然可点、Tab 仍然会停在上面，而同组件的空态文案还写着「或点『全选当前』」。
   *
   * 规矩：次要动作**压暗**（`--icon-idle`），不隐形。隐形的按钮等于不存在的功能。
   */
  it('不许再有 opacity: 0 的控件', () => {
    const offenders = css
      .split('\n')
      .map((line, index) => [index + 1, line.trim()] as const)
      .filter(([, line]) => /^opacity:\s*0(\.0+)?;$/.test(line))

    expect(offenders).toEqual([])
  })

  /** 压暗那一档只能有一个来源，否则「有点淡」会长出好几种淡法。 */
  it('--icon-idle 是压暗的唯一来源，浅色与深色各给一次', () => {
    expect(css.match(/--icon-idle:/g)).toHaveLength(2)
    expect(css.match(/opacity: var\(--icon-idle\)/g)!.length).toBeGreaterThanOrEqual(3)
  })

  /**
   * 事故：`.btn--danger` 只写了底色，没写 hover，于是继承 `button:hover` 的
   * `border-color: var(--accent)`——一颗红按钮悬停时描一圈橙边。
   *
   * 实心按钮都要自己把边框色写回来。
   */
  it('实心按钮的 hover 要把边框色写回来', () => {
    for (const [selector, token] of [
      ['.btn--primary:hover:not(:disabled)', '--accent'],
      ['.btn--danger:hover:not(:disabled)', '--danger'],
    ]) {
      const block = css.slice(css.indexOf(selector))
      const body = block.slice(block.indexOf('{'), block.indexOf('}'))
      expect(body, selector).toContain(`border-color: var(${token})`)
    }
  })

  /** 动效要给关掉动效的人留一条静态的路：扫动的进度条对前庭敏感的人是负担。 */
  it('有动画就要有 prefers-reduced-motion 的退路', () => {
    expect(css).toContain('@keyframes app-progress')
    expect(css).toContain('@media (prefers-reduced-motion: reduce)')
  })

  /**
   * 事故：写了 `border: none; background: none` 的按钮没有悬停反馈。
   *
   * 基座那条 `button:hover { border-color: var(--accent) }` 对它们是**空转**——
   * 没有边框，改边框色什么也看不见。命令面板整列因此只有键盘高亮，鼠标划过去毫无反应。
   * 规范（ui-design.md 6.1）要的是「动作：悬停出现面」。
   */
  it('无面按钮要有统一的悬停 / 聚焦「面」', () => {
    // 用正则定位而不是 indexOf：这个仓库的工作副本是 CRLF，写死 '\n' 的锚点找不到
    const rule = /:is\(\s*\.palette__item[\s\S]*?\}/.exec(css)?.[0] ?? ''
    const selector = rule.slice(0, rule.indexOf('{'))
    const body = rule.slice(rule.indexOf('{'))

    // 这两个是当初被点出来的重灾区，掉出这条规则就等于回到老样子
    expect(selector).toContain('.palette__item')
    expect(selector).toContain('.settings__nav-item')
    expect(selector).toContain(':hover')
    expect(selector).toContain(':focus-visible')
    expect(body).toContain('background: var(--bg-subtle)')
    // 选中态自己有底色，悬停不该盖掉它
    expect(selector).toContain('.active')
  })

  /**
   * 事故：压暗（`--icon-idle` ≈ 0.55）与基座的禁用态（0.5）几乎一样，
   * 而压暗那条特异性更高——目录里到头的「↑」看着和能点的一模一样，点下去没反应。
   *
   * 「次要」与「现在不能用」必须分得开；悬停也不该把禁用的按钮点亮。
   */
  it('幽灵按钮的禁用态要比压暗更暗，且悬停不点亮', () => {
    expect(css).toMatch(/\.page-list button\.page-list__icon:disabled[^}]*opacity: 0\.25/)

    const recovery = css.slice(css.indexOf('.page-list li:hover .page-list__icon'))
    const selector = recovery.slice(0, recovery.indexOf('{'))
    expect(selector.match(/:not\(:disabled\)/g)).toHaveLength(6)
  })

  /**
   * 事故：`.page-list__danger` 的红色写在 `.page-list button.page-list__danger` 里，
   * 带着祖先选择器。同一个类在设置页与消息中心里因此是**灰**的——
   * 同一件事在两个地方长得不一样，用户会以为其中一处不危险。
   */
  it('危险动作的红色不依赖祖先', () => {
    const rule = css.slice(css.indexOf('\n.page-list__danger {'))
    expect(rule.slice(0, rule.indexOf('}'))).toContain('color: var(--danger)')
  })
})
