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

/**
 * 未保存改动的确认流。
 *
 * 这是用户最容易丢东西的路径：切文章时缓冲区里还有没存的字。三种选择
 * （保存 / 放弃 / 取消）各有一条出口，其中「保存失败要留在原处」最容易漏——
 * 漏了就是「以为存上了，其实换了文章，改动没了」。
 */
describe('store 的未保存确认流', () => {
  const page = (source: string) => ({ source, title: source }) as never

  beforeEach(async () => {
    results.clear()
    vi.clearAllMocks()
    // 先干净地打开一篇：readContent 决定缓冲区内容
    results.set('readContent', '原始内容')
    await actions.openContent(page('posts/a.md'))
  })

  it('没有未保存改动时直接切过去', async () => {
    results.set('readContent', '乙的内容')
    await actions.requestOpenContent(page('posts/b.md'))

    expect(store.pendingPage).toBe(null)
    expect(store.currentSource).toBe('posts/b.md')
    expect(store.currentRaw).toBe('乙的内容')
  })

  it('有未保存改动时先挂起，不动缓冲区', async () => {
    actions.setRaw('改了一句')
    results.set('readContent', '乙的内容')

    await actions.requestOpenContent(page('posts/b.md'))

    expect(store.pendingPage).toMatchObject({ source: 'posts/b.md' })
    expect(store.currentSource).toBe('posts/a.md')
    expect(store.currentRaw).toBe('改了一句')
  })

  it('选「取消」留在原处，改动还在', async () => {
    actions.setRaw('改了一句')
    await actions.requestOpenContent(page('posts/b.md'))

    await actions.resolvePending('cancel')

    expect(store.pendingPage).toBe(null)
    expect(store.currentSource).toBe('posts/a.md')
    expect(store.currentRaw).toBe('改了一句')
  })

  it('选「放弃」才真的丢掉改动', async () => {
    actions.setRaw('改了一句')
    await actions.requestOpenContent(page('posts/b.md'))
    results.set('readContent', '乙的内容')

    await actions.resolvePending('discard')

    expect(store.currentSource).toBe('posts/b.md')
    expect(store.currentRaw).toBe('乙的内容')
    expect(calls.get('saveContent')).not.toHaveBeenCalled()
  })

  it('选「保存」先存再切', async () => {
    actions.setRaw('改了一句')
    await actions.requestOpenContent(page('posts/b.md'))
    results.set('saveContent', { pages: [], reason: '内容已改' })
    results.set('readContent', '乙的内容')

    await actions.resolvePending('save')

    expect(calls.get('saveContent')).toHaveBeenCalledWith('posts/a.md', '改了一句')
    expect(store.currentSource).toBe('posts/b.md')
  })

  it('保存失败就留在原处：切过去等于把没存上的改动丢掉', async () => {
    actions.setRaw('改了一句')
    await actions.requestOpenContent(page('posts/b.md'))
    results.set('saveContent', new Error('磁盘满了'))

    await actions.resolvePending('save')

    expect(store.currentSource).toBe('posts/a.md')
    expect(store.currentRaw).toBe('改了一句')
    expect(store.error).toBe('磁盘满了')
  })

  it('点已经打开的那一篇不触发确认', async () => {
    actions.setRaw('改了一句')
    await actions.requestOpenContent(page('posts/a.md'))
    expect(store.pendingPage).toBe(null)
  })
})

/**
 * 批量动作的收尾汇总。
 *
 * 批量动作**没有撤销栈**，所以这条汇总是唯一的补偿：它是用户判断「要不要去 Git 里回滚」
 * 的全部依据。三件事以前只写在注释里：一篇都没动时不许报成功（「已处理 0 篇」配一个绿勾
 * 等于说做完了）、跳过的原因要按种类说全、以及被跳过的每一篇都得能事后查到。
 */
describe('store 的批量收尾汇总', () => {
  const plan = { pages: [], orphaned_pages: [], reason: '内容已改' } as never

  beforeEach(() => {
    results.clear()
    vi.clearAllMocks()
    actions.clearNotices()
  })

  const last = () => store.notices[0]

  it('全都做成了：报成功，说清几篇', async () => {
    await actions.afterBatch(3, [], plan)
    expect(last()).toMatchObject({ level: 'success', message: '已处理 3 篇' })
  })

  it('一篇都没动也没跳过时不许报成功：绿勾配「已处理 0 篇」等于说做完了', async () => {
    await actions.afterBatch(0, [], plan)
    expect(last().level).toBe('info')
    expect(last().message).toContain('没有需要改的')
  })

  it('有跳过的：一条 info 里说清做成几篇、跳过几篇、都是什么原因', async () => {
    await actions.afterBatch(
      2,
      [
        { source: 'posts/a.md', reason: '目标已存在' },
        { source: 'posts/b.md', reason: 'front matter 读不出来' },
        { source: 'posts/c.md', reason: 'front matter 读不出来' },
      ],
      plan,
    )

    expect(last().level).toBe('info')
    expect(last().message).toBe(
      '已处理 2 篇；跳过 3 篇：2 篇「front matter 读不出来」、1 篇「目标已存在」',
    )
  })

  it('被跳过的每一篇都进通知历史的详情：汇总只有一行，而「是哪几篇」得查得到', async () => {
    await actions.afterBatch(
      1,
      [
        { source: 'posts/a.md', reason: '目标已存在' },
        { source: 'posts/b.md', reason: '目标已存在' },
      ],
      plan,
    )

    expect(last().details).toEqual(['posts/a.md：目标已存在', 'posts/b.md：目标已存在'])
  })

  it('一篇都没做成就是错误，不是提示', async () => {
    await actions.afterBatch(0, [{ source: 'posts/a.md', reason: '目标已存在' }], plan)
    expect(last().level).toBe('error')
  })

  it('调用方可以换掉「已处理 N 篇」那句：替换要说清共几处', async () => {
    await actions.afterBatch(1, [], plan, '替换了 1 篇里的 4 处')
    expect(last()).toMatchObject({ level: 'success', message: '替换了 1 篇里的 4 处' })
  })

  it('顺手把新计划写进状态：改完不重新生成的话，界面得标出「待生成」', async () => {
    await actions.afterBatch(1, [], plan)
    expect(store.plan).toMatchObject({ reason: '内容已改' })
  })
})

/**
 * 版式清单读不出来时不许只写进控制台。
 *
 * 原先这里是 `console.warn`：用户看到的是一个**空的版式列表**，而空列表看起来像
 * 「这个版本没有版式」，不像「读取失败了」。控制台在打包好的桌面应用里没人开着。
 */
describe('store 读版式清单', () => {
  beforeEach(() => {
    results.clear()
    vi.clearAllMocks()
    actions.clearNotices()
  })

  it('读得出来就填进状态', async () => {
    results.set('listPresets', [{ id: 'blog', title: '博客' }])
    await actions.loadPresets()
    expect(store.presets).toHaveLength(1)
  })

  it('读不出来要说一句，并且不把上一次的清单留在界面上', async () => {
    results.set('listPresets', [{ id: 'blog', title: '博客' }])
    await actions.loadPresets()

    results.set('listPresets', new Error('版式目录读不出来'))
    await actions.loadPresets()

    expect(store.presets).toHaveLength(0)
    const notice = store.notices.find((n) => n.message.includes('版式'))!
    expect(notice.message).toContain('默认版式')
    expect(notice.message).toContain('版式目录读不出来')
  })

  it('读不出版式不算致命错误：新建站点还能用默认版式建', async () => {
    results.set('listPresets', new Error('炸了'))
    await actions.loadPresets()
    expect(store.notices[0].level).not.toBe('error')
  })
})

/**
 * 读不出来的源文件。
 *
 * front matter 手改坏了的那一篇不会出现在页面清单里。以前这种站根本打不开，
 * 现在打得开——那就必须有人说出「少的那一篇去哪了」，否则用户只看到列表里凭空少一篇。
 */
describe('store 报出读不出来的文件', () => {
  const summary = (broken: Array<{ source: string; reason: string }>) => ({
    root: '/site',
    config: { site: { title: '测试站点' } },

    pages: [],
    layouts: [],
    components: [],
    templates: [],
    recent_builds: [],
    config_warnings: [],
    broken_sources: broken,
  })

  beforeEach(() => {
    results.clear()
    vi.clearAllMocks()
    actions.clearNotices()
  })

  it('打开项目时按原因汇总报出来，明细里有每一篇', async () => {
    results.set(
      'openProject',
      summary([
        { source: 'posts/a.md', reason: 'front matter 缺少结束的 `+++`' },
        { source: 'posts/b.md', reason: 'front matter 缺少结束的 `+++`' },
      ]),
    )

    await actions.openProject('/site')

    const notice = store.notices.find((n) => n.message.includes('读不出来'))!
    expect(notice.level).toBe('error')
    expect(notice.message).toContain('2 篇')
    expect(notice.details).toEqual([
      'posts/a.md：front matter 缺少结束的 `+++`',
      'posts/b.md：front matter 缺少结束的 `+++`',
    ])
  })

  it('同一批坏文件不反复报：每次刷新都弹一条会把通知区刷满', async () => {
    const broken = [{ source: 'posts/a.md', reason: 'front matter 缺少结束的 `+++`' }]
    results.set('openProject', summary(broken))
    results.set('projectSummary', summary(broken))

    await actions.openProject('/site')
    await actions.refresh()
    await actions.refresh()

    expect(store.notices.filter((n) => n.message.includes('读不出来'))).toHaveLength(1)
  })

  it('修好了再坏一次要重新报：那是新的问题', async () => {
    results.set('openProject', summary([{ source: 'posts/a.md', reason: '坏了' }]))
    await actions.openProject('/site')

    results.set('projectSummary', summary([]))
    await actions.refresh()

    results.set('projectSummary', summary([{ source: 'posts/a.md', reason: '坏了' }]))
    await actions.refresh()

    expect(store.notices.filter((n) => n.message.includes('读不出来'))).toHaveLength(2)
  })
})

/**
 * 一行装不下的消息。
 *
 * 气泡里只放得下一行，所以这些出口都做了截断（「N 条生成警告：第一条 等」）。

 * 截断本身没问题，问题是**被截掉的部分以前没有任何落点**：用户读到「另有 3 篇同样没改成」
 * 之后无处可查，只能自己去 grep 全站。现在整份清单进通知历史的详情。
 */
describe('store 把截断掉的那部分留进详情', () => {
  const plan = { pages: [], orphaned_pages: [], reason: '内容已改' } as never

  beforeEach(() => {
    results.clear()
    vi.clearAllMocks()
    actions.clearNotices()
  })

  const noticeWith = (part: string) => store.notices.find((n) => n.message.includes(part))!

  it('生成警告：气泡里只留一条，全部进详情', async () => {
    results.set('runBuild', {
      pages_rendered: 1,
      files_written: 1,
      duration_ms: 5,
      phases: {},
      warnings: ['posts/a.md: 缺描述', 'posts/b.md: 图片没有 alt', 'posts/c.md: 标题过长'],
    })

    await actions.build('full')

    const notice = noticeWith('3 条生成警告')
    expect(notice.level).toBe('info')
    expect(notice.details).toEqual([
      'posts/a.md: 缺描述',
      'posts/b.md: 图片没有 alt',
      'posts/c.md: 标题过长',
    ])
  })

  it('只有一条警告时不摆详情：那条已经整句显示了', async () => {
    results.set('runBuild', {
      pages_rendered: 1,
      files_written: 1,
      duration_ms: 5,
      phases: {},
      warnings: ['posts/a.md: 缺描述'],
    })

    await actions.build('full')
    expect(noticeWith('缺描述').details).toBeUndefined()
  })

  it('改地址时没改成的引用：每一篇都进详情，地址已经变了，这些链接还指着旧的', async () => {
    // 用一篇没打开的文章：改地址对**当前打开且有未保存改动**的那一篇会先拒绝，
    // 而那条规则有它自己的测试，这里要验的是失败清单
    results.set('changeSlug', {
      source: 'posts/z.md',
      from_url: '/posts/z/',
      to_url: '/posts/zz/',
      alias_added: true,
      refs_updated: [],
      refs_failed: [
        { source: 'posts/x.md', reason: 'front matter 读不出来' },
        { source: 'posts/y.md', reason: 'front matter 读不出来' },
      ],
      plan,
    })

    await actions.changeSlug('posts/z.md', 'zz')


    const notice = noticeWith('没能改写')
    expect(notice.level).toBe('error')
    expect(notice.details).toEqual([
      'posts/x.md：front matter 读不出来',
      'posts/y.md：front matter 读不出来',
    ])
  })

  it('导入内容：「N 处需要人看一下」得说得出是哪几处', async () => {
    results.set('importContent', {
      imported: ['posts/a.md'],
      skipped: [],
      warnings: ['old/a.md: 日期认不出来', 'old/b.md: 没有标题'],
    })

    await actions.importContent('/tmp/old', 'posts')

    const notice = noticeWith('需要人看一下')
    expect(notice.details).toEqual(['old/a.md: 日期认不出来', 'old/b.md: 没有标题'])
  })
})

/**
 * 停止发布。
 *
 * 发布是三个长操作里唯一天然有停止点的。这里钉住的是「停下来之后怎么说话」：
 * 停止**不是**失败，但也绝不能说成「发布完成」——两者的上传数可能都是 1，
 * 一个是全部、一个是一半。
 */
describe('store 的停止发布', () => {
  const stopped = {
    target: 'ftp://example.test/www',
    uploaded: ['index.html'],
    deleted: [],
    skipped: 0,
    duration_ms: 12,
    commit: null,
    warnings: ['已按要求停止：这一批 3 个文件里传完了 1 个。'],
    cancelled: true,
  }

  beforeEach(() => {
    results.clear()
    vi.clearAllMocks()
  })

  it('发布期间才允许请求停止，而且真的发出去', async () => {
    results.set('deploySite', { ...stopped, cancelled: false, warnings: [] })

    // 没有发布在跑时点停止是空操作：不该给后端发一条没人接的命令
    await actions.stopDeploy()
    expect(calls.get('cancelDeploy')).not.toHaveBeenCalled()

    const running = actions.deploy()
    expect(store.deployPhase).toBe('upload')
    await actions.stopDeploy()
    expect(calls.get('cancelDeploy')).toHaveBeenCalledTimes(1)
    // 连点第二次不再重复发：界面上按钮此刻已经置灰，状态得跟得上
    await actions.stopDeploy()
    expect(calls.get('cancelDeploy')).toHaveBeenCalledTimes(1)

    await running
    expect(store.deployPhase).toBeNull()
    expect(store.deployStopping).toBe(false)
  })

  /**
   * 干跑也算「发布在跑」：FTP 要逐个查远端状态，上千个文件时够久了。
   * 停止按钮得在这一步就能按，否则那段时间界面上只有一个转圈。
   */
  it('干跑期间也能停，而且干跑失败之后状态要收干净', async () => {
    results.set('planDeploy', new Error('已停止：发布预览已按要求停止'))

    const running = actions.planDeploy()
    expect(store.deployPhase).toBe('plan')
    await actions.stopDeploy()
    expect(calls.get('cancelDeploy')).toHaveBeenCalledTimes(1)

    expect(await running).toBeUndefined()
    expect(store.deployPhase).toBeNull()
    expect(store.deployStopping).toBe(false)
  })

  it('停下来的那次发布不许说成「发布完成」', async () => {
    results.set('deploySite', stopped)

    await actions.deploy()

    expect(store.lastDeploy?.cancelled).toBe(true)
    // 新的通知在最前面：这次发布留下的那条就是它
    const notice = store.notices[0]
    expect(notice.message).toContain('发布已停止')
    expect(notice.message).not.toContain('发布完成')
    // 自己点的停止不是错误
    expect(notice.level).toBe('info')
    // 「传到哪儿了、线上是什么状态」留在详情里，事后还能翻到
    expect(notice.details).toEqual(['已按要求停止：这一批 3 个文件里传完了 1 个。'])
  })
})

/**
 * 挪动文章在栏目里的位次。
 *
 * 侧栏要像一本书的目录，前提是它显示的顺序就是读者看到的顺序，并且**能改**。
 * 这里钉住三件事：交给后端的是整栏的清单（不是「挪到第几位」）、索引页不算一篇、
 * 挪不动的时候一个字节都不写。
 */
describe('store 挪动文章的位次', () => {
  const page = (source: string, extra: Record<string, unknown> = {}) => ({
    source,
    title: source,
    url: `/${source}`,
    template: 'pages/post.html',
    section: 'posts',
    is_index: false,
    draft: false,
    date: null,
    scheduled: false,
    tags: [],
    weight: 0,
    ...extra,
  })

  beforeEach(async () => {
    results.clear()
    vi.clearAllMocks()
    results.set('openProject', {
      root: '/site',
      config: { site: { title: '测试站点' } },
      // 日期倒序，所以顺序是 a、b、c
      pages: [
        page('posts/index.md', { is_index: true }),
        page('posts/a.md', { date: '2026-01-03T00:00:00Z' }),
        page('posts/b.md', { date: '2026-01-02T00:00:00Z' }),
        page('posts/c.md', { date: '2026-01-01T00:00:00Z' }),
      ],
      layouts: [],
      components: [],
      templates: [],
      recent_builds: [],
      config_warnings: [],
      broken_sources: [],
    })
    await actions.openProject('/site')
    vi.clearAllMocks()
  })

  it('上移一位：交给后端的是整栏的新顺序，索引页不算一篇', async () => {
    results.set('reorderSection', { changed: ['posts/b.md', 'posts/a.md'], total: 3 })

    await actions.movePage('posts/b.md', -1)

    expect(calls.get('reorderSection')).toHaveBeenCalledWith('posts', [
      'posts/b.md',
      'posts/a.md',
      'posts/c.md',
    ])
  })

  it('已经在最前 / 最后时一个字节都不写', async () => {
    await actions.movePage('posts/a.md', -1)
    await actions.movePage('posts/c.md', 1)
    expect(calls.get('reorderSection')).not.toHaveBeenCalled()
  })

  it('第一次挪动会固化整栏，通知里就得说清动了几篇', async () => {
    // 三篇的 weight 原本都是 0，所以第一次挪动会写三篇
    results.set('reorderSection', {
      changed: ['posts/b.md', 'posts/a.md', 'posts/c.md'],
      total: 3,
    })

    await actions.movePage('posts/b.md', -1)

    // 「我只挪了一下，怎么 3 个文件都变了」不能像个 bug
    expect(store.notices[0].message).toContain('固化')
    expect(store.notices[0].message).toContain('3 篇')
  })
})



