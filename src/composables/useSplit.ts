/**
 * 可拖拽分栏宽度，记忆在 localStorage。
 *
 * 不引入拖拽库：两条分隔条的需求只是「按下、跟随、松开」，
 * 用 pointer 事件二十行就够，省掉一个依赖与它的样式约定。
 */
import { onBeforeUnmount, ref, watch } from 'vue'

export interface SplitOptions {
  /** localStorage 键名 */
  key: string
  /** 左侧列表初始宽度（px） */
  list: number
  /** 右侧预览初始宽度（px） */
  preview: number
}

const MIN_LIST = 180
const MAX_LIST = 480
const MIN_PREVIEW = 260

/**
 * 键盘一次调多少像素。
 *
 * 拖拽是连续的，键盘只能是离散的：步长太小（1px）要按几百次，太大（50px）
 * 又没法微调。16px 一档、按住 Shift 走 64px，与「一次一行 / 一次一屏」同一种手感。
 */
const STEP = 16
const BIG_STEP = 64


export function useSplit(options: SplitOptions) {
  const listWidth = ref(options.list)
  const previewWidth = ref(options.preview)

  restore()

  /**
   * 写盘防抖。
   *
   * 拖一次分隔条会连续触发几十上百次 `pointermove`，每一次都 `JSON.stringify` +
   * `localStorage.setItem` 是同步写：拖动手感会变涩，而中间那些宽度没人关心。
   * 只有停手之后那个值值得记住。
   */
  let saveTimer: ReturnType<typeof setTimeout> | null = null

  watch([listWidth, previewWidth], () => {
    if (saveTimer !== null) clearTimeout(saveTimer)
    saveTimer = setTimeout(() => {
      saveTimer = null
      localStorage.setItem(
        options.key,
        JSON.stringify({ list: listWidth.value, preview: previewWidth.value }),
      )
    }, 200)
  })

  /**
   * 窗口变窄时重新夹取。
   *
   * `clampPreview` 的上限依赖 `window.innerWidth`，所以只在拖动时夹一次是不够的：
   * 把窗口从 1920 拖到 1280，预览栏会保持原来的宽度，把编辑器挤到只剩一条缝。
   */
  function reclamp() {
    listWidth.value = clampList(listWidth.value)
    previewWidth.value = clampPreview(previewWidth.value)
  }

  window.addEventListener('resize', reclamp)

  function restore() {
    try {
      const raw = localStorage.getItem(options.key)
      if (!raw) return
      const parsed = JSON.parse(raw) as { list?: number; preview?: number }
      if (typeof parsed.list === 'number') listWidth.value = clampList(parsed.list)
      if (typeof parsed.preview === 'number') previewWidth.value = clampPreview(parsed.preview)
    } catch {
      // 坏数据就用默认值，不值得打扰用户
    }
  }

  function clampList(value: number) {
    return Math.min(MAX_LIST, Math.max(MIN_LIST, Math.round(value)))
  }

  function clampPreview(value: number) {
    const max = Math.max(MIN_PREVIEW, window.innerWidth - listWidth.value - 360)
    return Math.min(max, Math.max(MIN_PREVIEW, Math.round(value)))
  }

  let stop: (() => void) | null = null

  /** 开始拖动。`side` 决定改的是哪一栏，以及位移的符号。 */
  function startDrag(side: 'list' | 'preview', event: PointerEvent) {
    event.preventDefault()
    const startX = event.clientX
    const startList = listWidth.value
    const startPreview = previewWidth.value

    const onMove = (move: PointerEvent) => {
      const dx = move.clientX - startX
      if (side === 'list') listWidth.value = clampList(startList + dx)
      // 右侧分隔条往左拖 = 预览变宽，符号相反
      else previewWidth.value = clampPreview(startPreview - dx)
    }
    const onUp = () => stop?.()

    stop = () => {
      window.removeEventListener('pointermove', onMove)
      window.removeEventListener('pointerup', onUp)
      document.body.style.cursor = ''
      stop = null
    }
    window.addEventListener('pointermove', onMove)
    window.addEventListener('pointerup', onUp)
    document.body.style.cursor = 'col-resize'
  }

  /**
   * 键盘调宽。
   *
   * 分隔条以前只有鼠标能拖：键盘用户既改不了分栏，也完全感知不到它存在
   * （见 docs/ui-review.md 第 5 条）。方向键走 16px、按住 Shift 走 64px，
   * `Home` / `End` 直接到两端——夹取仍走 `clampList` / `clampPreview` 那一份，
   * 键盘不该能拖出鼠标拖不到的宽度。
   */
  function nudge(side: 'list' | 'preview', direction: -1 | 1, big = false) {
    const delta = (big ? BIG_STEP : STEP) * direction
    if (side === 'list') listWidth.value = clampList(listWidth.value + delta)
    // 右侧分隔条往左 = 预览变宽，与拖拽的符号一致
    else previewWidth.value = clampPreview(previewWidth.value - delta)
  }

  /** 跳到某一栏的最窄 / 最宽。`Home` / `End` 用。 */
  function jump(side: 'list' | 'preview', to: 'min' | 'max') {
    const large = to === 'max' ? Number.MAX_SAFE_INTEGER : 0
    if (side === 'list') listWidth.value = clampList(large)
    else previewWidth.value = clampPreview(large)
  }

  /** 当前区间，供 `role="separator"` 的 `aria-valuemin/max` 用。 */
  function bounds(side: 'list' | 'preview') {
    if (side === 'list') return { min: MIN_LIST, max: MAX_LIST }
    return { min: MIN_PREVIEW, max: clampPreview(Number.MAX_SAFE_INTEGER) }
  }

  onBeforeUnmount(() => {
    stop?.()
    window.removeEventListener('resize', reclamp)
    // 卸载时把还没落盘的宽度补写一次，否则拖完立刻关站点会丢掉这次调整
    if (saveTimer !== null) {
      clearTimeout(saveTimer)
      localStorage.setItem(
        options.key,
        JSON.stringify({ list: listWidth.value, preview: previewWidth.value }),
      )
    }
  })

  return { listWidth, previewWidth, startDrag, nudge, jump, bounds }
}
