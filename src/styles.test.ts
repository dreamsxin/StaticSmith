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

/**
 * 取出某个属性的所有取值（连行号），用来判断「它走没走令牌」。
 *
 * 不用「`^prop:\s*(?!var\()`」那种否定前瞻：`\s*` 会回退成零宽，
 * 于是 `box-shadow: var(...)` 也被判成裸值——第一版就是这么写的，
 * 三条断言全是绿的，护栏是假的。先取值、再判断，这种坑绕不开也看得见。
 */
const valuesOf = (prop: string) =>
  css
    .split('\n')
    .map((line, index) => [index + 1, line.trim()] as const)
    .map(([at, line]) => [at, new RegExp(`^${prop}:\\s*(.+);$`).exec(line)?.[1]] as const)
    .filter((entry): entry is readonly [number, string] => entry[1] !== undefined)

const bare = (prop: string, allowed: RegExp) =>
  valuesOf(prop).filter(([, value]) => !allowed.test(value))

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

  /**
   * 形状、阴影、层级都必须走令牌。
   *
   * 这三样原先是裸值：圆角散着 3 / 4 / 6 / 8 / 10 / 999px 六种（规范说该有一种），
   * 阴影三处各自硬编码黑色（深色模式下几乎看不见），`z-index` 五个数字散在五个地方
   * ——「谁压谁」只能靠通读全文去记，加一层新浮层时只能猜。
   *
   * 圆角允许 `0`：那不是一个档，是「明确不要圆角」（贴边的列表项、全宽输入）。
   */
  it('圆角、阴影、层级不许出现裸值', () => {
    // 先确认这份清单真的抓到了东西：空清单会让下面三条断言「全绿而无用」，
    // 而那正是第一版犯的错（值都取成了 undefined，filter 自然是空的）
    expect(valuesOf('border-radius').length).toBeGreaterThan(20)
    expect(valuesOf('z-index').length).toBeGreaterThan(4)
    expect(valuesOf('box-shadow').length).toBeGreaterThan(4)

    // 圆角允许 `0`：那不是一个档，是「明确不要圆角」（贴边的列表项、全宽输入）
    expect(bare('border-radius', /^(var\(--radius-|0$)/)).toEqual([])
    expect(bare('z-index', /^var\(--z-/)).toEqual([])
    // inset 的那几条是「线」不是「浮起」（落点指示、焦点圈），不在此列
    expect(bare('box-shadow', /^(var\(--shadow-|inset )/)).toEqual([])
  })

  /** 阴影在深色下要单独给：18% 的黑压在近黑底上等于没有。 */
  it('阴影令牌浅色与深色各给一套', () => {
    for (const token of ['--shadow-raised', '--shadow-dropdown', '--shadow-dialog']) {
      expect(css.match(new RegExp(`${token}:`, 'g')), token).toHaveLength(2)
    }
  })

  /** 层级要成表：每一档都有名字，中间留空档好插新层。 */
  it('层级令牌齐全且从下到上递增', () => {
    const order = ['--z-sticky', '--z-sticky-head', '--z-dropdown', '--z-context', '--z-toast']
    const values = order.map((token) => {
      const found = new RegExp(`${token}:\\s*(\\d+)`).exec(css)
      expect(found, token).not.toBeNull()
      return Number(found![1])
    })

    expect(values).toEqual([...values].sort((a, b) => a - b))
    expect(new Set(values).size).toBe(order.length)
  })

  /**
   * 字号必须走刻度。
   *
   * 盘点出的实情：11 个取值，其中 6 个（0.8 / 0.78 / 0.75 / 0.74 / 0.72 / 0.7rem）
   * 全挤在 11.2–12.8px 这 1.6px 的带宽里——肉眼分不出差别，却谁也不敢动，
   * 因为不知道哪一处是有意的。更糟的是 `body` 是 14px 而「小字」是 12.8px，
   * 于是**没写 font-size 的按钮反而比写了的标签大**：视觉层级是反的。
   *
   * `em` 不在此列：`kbd` 用 `0.85em` 是刻意的相对——它嵌在句子里，要跟着周围的字走。
   */
  it('字号不许出现裸值，一律走 --text-* 刻度', () => {
    // 自检：清单空了下面那条断言就「全绿而无用」
    expect(valuesOf('font-size').length).toBeGreaterThan(80)
    expect(bare('font-size', /^(var\(--text-|inherit$|[\d.]+em$)/)).toEqual([])
    // `font` 简写里也藏过一个 14px（body）：它是全局默认，最该是令牌
    expect(css).toMatch(/font:\s*var\(--text-base\)\//)
  })

  /** 刻度要成套且拉开距离：档与档之间看不出差别，就等于没有层级。 */
  it('字号刻度五档齐全、严格递增，且不随主题变', () => {
    const order = ['--text-xs', '--text-sm', '--text-base', '--text-lg', '--text-xl']
    const values = order.map((token) => {
      const found = new RegExp(`${token}:\\s*([\\d.]+)rem`).exec(css)
      expect(found, token).not.toBeNull()
      // 深色模式不该重定义字号：字号是结构，不是配色
      expect(css.match(new RegExp(`${token}:`, 'g')), token).toHaveLength(1)
      return Number(found![1])
    })

    expect(values).toEqual([...values].sort((a, b) => a - b))
    expect(new Set(values).size).toBe(order.length)
    // 相邻两档至少差 1px（16px 基准下 0.0625rem），否则用户分不出「主」和「次」
    for (let i = 1; i < values.length; i += 1) {
      expect(values[i] - values[i - 1], order[i]).toBeGreaterThanOrEqual(0.0625)
    }
  })
})
