/**
 * 浮层的焦点圈定（focus trap）。
 *
 * `aria-modal="true"` 是一句承诺：背景对辅助技术不可达。三个浮层都写了这个属性，
 * 但只有命令面板碰巧守住了（它把 Tab 改成「移动高亮」，焦点从没离开输入框）；
 * 新建站点与回退内容对话框里 Tab 一路走出去就落到背后的菜单栏与编辑器上，
 * 于是属性说的和实际行为不一致——读屏用户被带到一个「不存在」的地方。
 *
 * 不用 `inert` 或给背景加 `aria-hidden`：浮层就渲染在主结构同一棵树里，
 * 把外层整片标记掉会连浮层自己一起标掉。在浮层内部圈住 Tab 是更小的改动。
 */

/**
 * 能接 Tab 的元素。
 *
 * 排除 `tabindex="-1"`（列表项那种「用方向键走」的候选项）与禁用控件——
 * 后者本来就不接 Tab，算进来会让「最后一个」算错，Tab 到尽头时不回卷。
 */
const FOCUSABLE = [
  'a[href]',
  'button:not([disabled])',
  'input:not([disabled])',
  'select:not([disabled])',
  'textarea:not([disabled])',
  '[tabindex]:not([tabindex="-1"])',
].join(', ')

/**
 * Tab 应该跳到第几个可聚焦元素；返回 `null` 表示交给浏览器默认行为。
 *
 * 单独拆成纯函数是为了能测：跳转本身要 DOM，但「跳到哪」的判断不需要，
 * 而边界（焦点在外面、正/反向到尽头）恰恰是最容易写错的部分。
 *
 * @param count 容器内可聚焦元素个数
 * @param current 当前焦点的下标，焦点不在容器内时传 -1
 */
export function nextFocusIndex(count: number, current: number, shiftKey: boolean): number | null {
  if (count === 0) return null
  // 焦点在浮层外（比如刚点了背景遮罩）：正向收回第一个，反向收回最后一个。
  if (current < 0) return shiftKey ? count - 1 : 0
  if (shiftKey) return current === 0 ? count - 1 : null
  return current === count - 1 ? 0 : null
}

/**
 * 把 Tab 圈在 `container` 内。绑在浮层根元素的 `keydown.tab` 上。
 */
export function trapTab(container: HTMLElement | null, event: KeyboardEvent): void {
  if (!container) return
  const items = Array.from(container.querySelectorAll<HTMLElement>(FOCUSABLE))
  const current = items.indexOf(document.activeElement as HTMLElement)
  const next = nextFocusIndex(items.length, current, event.shiftKey)
  if (next === null) return
  event.preventDefault()
  items[next].focus()
}
