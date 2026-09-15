// @vitest-environment jsdom
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { watch } from 'vue'

/**
 * 忙态计数器的行为测试。
 *
 * 这是所有「按钮该不该置灰」的地基：`store.busy` 由一个**计数器**驱动，
 * 而不是布尔——`build`、`saveContent` 一次点击里会调好几次 `run`，布尔标志会在
 * 中途跳回 false，按钮闪烁且能被二次点击。这条性质此前只写在注释里，没有断言。
 *
 * `store.ts` 在模块作用域就注册了三个事件监听（`onBuildProgress` 等），所以
 * `./api` 必须整个换掉：用一个 Proxy 兜住所有导出，取到的每个名字都是同一个
 * 异步空函数，需要具体返回值的再单独指定。这样加新 api 时测试不会因为
 * 「mock 少了一项」而崩。
 */
const calls = new Map<string, ReturnType<typeof vi.fn>>()

/** 某个 api 这次要返回什么。默认 undefined（等于「失败」，调用方各有 guard）。 */
const results = new Map<string, unknown>()

function apiMock(name: string) {
  if (!calls.has(name)) {
    calls.set(
      name,
      vi.fn(async () => {
        const value = results.get(name)
        if (value instanceof Error) throw value
        return value
      }),
    )
  }
  return calls.get(name)!
}

/**
 * 把 `./api` 的每个函数导出换成同名的假实现。
 *
 * 按真模块的键生成，而不是手写一份清单：加新 api 时测试不会因为「mock 少了一项」
 * 而崩，也不会悄悄放过真实调用。非函数导出（常量）原样保留。
 */
vi.mock('./api', async () => {
  const actual = await vi.importActual<Record<string, unknown>>('./api')
  const mocked: Record<string, unknown> = {}
  for (const [key, value] of Object.entries(actual)) {
    mocked[key] = typeof value === 'function' ? apiMock(key) : value
  }
  return mocked
})

const { store, actions } = await import('./store')

/** 记录 `busy` 的每一次跳变，用来验「一次点击只亮一次」。 */
function trackBusy(): boolean[] {
  const seen: boolean[] = []
  watch(
    () => store.busy,
    (value) => seen.push(value),
    { flush: 'sync' },
  )
  return seen
}

describe('store 的忙态计数器', () => {
  beforeEach(() => {
    // 不清 `calls`：store 在导入时就抓住了那几个函数引用，换掉映射表里的值它也看不见。
    // 要改行为只能改 `results`（假实现每次调用都重新读它）。
    results.clear()
    vi.clearAllMocks()
  })

  it('单次调用：期间为忙，结束归闲', async () => {
    const seen = trackBusy()
    expect(store.busy).toBe(false)

    const pending = actions.checkDeploy()
    expect(store.busy).toBe(true)
    await pending

    expect(store.busy).toBe(false)
    expect(seen).toEqual([true, false])
  })

  it('复合动作中途不许跳回空闲：布尔标志会让按钮闪烁并允许二次点击', async () => {
    // build 会顺序调用 runBuild → recomputePlan → refresh → loadOutputs 等多次 run
    results.set('runBuild', { pages: 1, duration_ms: 5, phases: {}, warnings: [] })
    const seen = trackBusy()

    await actions.build('full')

    expect(seen).toEqual([true, false])
    expect(seen.filter((v) => v).length).toBe(1)
  })

  it('失败会记下错误、弹通知，并且照样归零', async () => {
    results.set('checkDeploy', new Error('连不上服务器'))
    const seen = trackBusy()

    await actions.checkDeploy()

    expect(store.busy).toBe(false)
    expect(store.error).toBe('连不上服务器')
    expect(store.toasts.at(-1)).toMatchObject({ kind: 'error', message: '连不上服务器' })
    expect(seen).toEqual([true, false])
  })

  it('成功之后清掉上一次的错误：错误横幅不该跨越两次操作', async () => {
    results.set('checkDeploy', new Error('连不上服务器'))
    await actions.checkDeploy()
    expect(store.error).toBe('连不上服务器')

    results.set('checkDeploy', undefined)
    await actions.checkDeploy()
    expect(store.error).toBe(null)
  })

  it('归零时清空进度文字：停在「render 42/42」上看起来像还在跑', async () => {
    results.set('runBuild', { pages: 1, duration_ms: 5, phases: {}, warnings: [] })
    await actions.build('full')
    expect(store.progress).toBe('')
  })

  it('两个动作并行时，先结束的那个不许解除忙态', async () => {
    let releaseFirst: (() => void) | undefined
    // 让 checkDeploy 悬着：假实现会 await 取到的值，所以塞一个未决的 Promise 就行
    results.set(
      'checkDeploy',
      new Promise<void>((resolve) => (releaseFirst = resolve)),
    )
    const seen = trackBusy()

    const slow = actions.checkDeploy()
    const quick = actions.deployAccount()
    await quick
    expect(store.busy).toBe(true)

    releaseFirst?.()
    await slow
    expect(store.busy).toBe(false)
    expect(seen).toEqual([true, false])
  })
})
