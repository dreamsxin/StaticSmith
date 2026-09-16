// @vitest-environment jsdom
import { mount } from '@vue/test-utils'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { reactive } from 'vue'

/**
 * 发布前确认的行为测试。
 *
 * 发布是唯一影响**线上**的动作：别的动作改错了还能在本地改回来，这个改错了是
 * 别人看到的页面变了。所以只测一条主线——**没确认就不许写远端**，
 * 以及确认那一屏要说清「传到哪、传几个、多少字节、哪些不会做」。
 */
const plan = {
  target: 'ftp://example.test/www',
  upload: ['index.html', 'posts/index.html'],
  skipped: 3,
  bytes: 2048,
  warnings: ['已逐个查询远端文件的大小与时间（只读），没有写入任何内容', '远端多余的文件不会被删除'],
}

const state = reactive({
  project: { config: { deploy: { type: 'ftp', ftp: { host: 'example.test', overwrite: 'size_or_newer' } } } },
  busy: false,
  deployPhase: null as 'plan' | 'upload' | null,
  deployStopping: false,
  lastDeploy: null as unknown,
})

const actions = {
  planDeploy: vi.fn(async () => plan as typeof plan | undefined),
  deploy: vi.fn(async () => {}),
  stopDeploy: vi.fn(async () => {}),
  checkDeploy: vi.fn(async () => {}),
  deployAccount: vi.fn(async () => 'ftp:user@example.test'),
  hasSecret: vi.fn(async () => true),
  saveSecret: vi.fn(async () => {}),
  deleteSecret: vi.fn(async () => {}),
}

vi.mock('../store', () => ({ store: state, actions }))

const DeployPanel = (await import('./DeployPanel.vue')).default

/** 「一键发布…」那颗按钮。 */
function trigger(wrapper: ReturnType<typeof mount>) {
  return wrapper
    .findAll('button')
    .find((button) => button.text().includes('一键发布'))
}

async function ask(wrapper: ReturnType<typeof mount>) {
  await trigger(wrapper)!.trigger('click')
  await new Promise((resolve) => setTimeout(resolve))
}

describe('DeployPanel 的发布确认', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    actions.planDeploy.mockResolvedValue(plan)
    state.deployPhase = null
    state.deployStopping = false
    state.lastDeploy = null
  })

  it('点发布只干跑，不写远端', async () => {
    const wrapper = mount(DeployPanel)
    await ask(wrapper)

    expect(actions.planDeploy).toHaveBeenCalled()
    expect(actions.deploy).not.toHaveBeenCalled()
  })

  it('确认那一屏说清目标、数量、字节与「不会做什么」', async () => {
    const wrapper = mount(DeployPanel)
    await ask(wrapper)

    const text = wrapper.get('.deploy__confirm').text()
    expect(text).toContain('ftp://example.test/www')
    expect(text).toContain('2 个文件')
    expect(text).toContain('2.0 KB')
    expect(text).toContain('跳过（未变化）：3')
    expect(text).toContain('远端多余的文件不会被删除')
    // 会传哪些也要给出来，不然「2 个文件」无法判断对不对
    expect(text).toContain('index.html')
  })

  it('确认之后才真的发布，确认区随即收起', async () => {
    const wrapper = mount(DeployPanel)
    await ask(wrapper)

    const confirm = wrapper
      .findAll('.deploy__confirm button')
      .find((button) => button.text().includes('确认发布'))!
    await confirm.trigger('click')

    expect(actions.deploy).toHaveBeenCalledTimes(1)
    expect(wrapper.find('.deploy__confirm').exists()).toBe(false)
  })

  it('取消什么也不做', async () => {
    const wrapper = mount(DeployPanel)
    await ask(wrapper)

    const cancel = wrapper
      .findAll('.deploy__confirm button')
      .find((button) => button.text() === '取消')!
    await cancel.trigger('click')

    expect(actions.deploy).not.toHaveBeenCalled()
    expect(wrapper.find('.deploy__confirm').exists()).toBe(false)
  })

  it('没有要传的文件时不让确认：那一次发布什么也不会发生', async () => {
    actions.planDeploy.mockResolvedValue({ ...plan, upload: [], bytes: 0 })
    const wrapper = mount(DeployPanel)
    await ask(wrapper)

    const confirm = wrapper
      .findAll('.deploy__confirm button')
      .find((button) => button.text().includes('确认发布'))!
    expect(confirm.attributes('disabled')).toBeDefined()
  })

  it('干跑失败（连不上、没配置）时不进确认态', async () => {
    actions.planDeploy.mockResolvedValue(undefined)
    const wrapper = mount(DeployPanel)
    await ask(wrapper)

    expect(wrapper.find('.deploy__confirm').exists()).toBe(false)
    expect(actions.deploy).not.toHaveBeenCalled()
  })
})

/**
 * 停止发布。
 *
 * 发布是三个长操作里唯一天然有停止点的：上传逐文件进行，文件之间的边界本来就不是
 * 原子的。界面这一侧要做到两件事——发布期间**能**停，以及停完之后别把
 * 「传了 12 个然后停了」说成「发布完成，传了 12 个」。
 */
describe('DeployPanel 的停止发布', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    state.deployPhase = null
    state.deployStopping = false
    state.lastDeploy = null
  })

  function stopButton(wrapper: ReturnType<typeof mount>) {
    return wrapper.findAll('button').find((button) => button.text().includes('停止'))
  }

  it('发布期间才出现「停止」', async () => {
    const wrapper = mount(DeployPanel)
    expect(stopButton(wrapper)).toBeUndefined()

    state.deployPhase = 'upload'
    await wrapper.vm.$nextTick()
    expect(stopButton(wrapper)).toBeDefined()
    // 发布中不该还摆着「一键发布…」：并行发两次没有任何好结果
    expect(trigger(wrapper)).toBeUndefined()
  })

  it('点一下就请求停止，并说清停在哪儿', async () => {
    state.deployPhase = 'upload'
    const wrapper = mount(DeployPanel)

    await stopButton(wrapper)!.trigger('click')
    expect(actions.stopDeploy).toHaveBeenCalledTimes(1)
    // 「会不会留下半个文件」是用户此刻唯一关心的事
    expect(wrapper.text()).toContain('不会留下半个文件')
  })

  it('干跑期间也能停，但话不一样：那一步不写远端', async () => {
    state.deployPhase = 'plan'
    const wrapper = mount(DeployPanel)

    expect(stopButton(wrapper)).toBeDefined()
    expect(wrapper.text()).toContain('停下来什么也不会改')
    // 干跑不写远端，「半个文件」那句话在这里是错的
    expect(wrapper.text()).not.toContain('不会留下半个文件')
  })

  it('已经在停的时候按钮不可再点', async () => {
    state.deployPhase = 'upload'
    state.deployStopping = true
    const wrapper = mount(DeployPanel)

    const button = stopButton(wrapper)!
    expect(button.text()).toContain('正在停止')
    expect(button.attributes('disabled')).toBeDefined()
  })

  it('被停止的发布不能显示成「发完了」', async () => {
    state.lastDeploy = {
      target: 'ftp://example.test/www',
      uploaded: ['index.html'],
      deleted: [],
      skipped: 0,
      duration_ms: 12,
      commit: null,
      warnings: ['已按要求停止：这一批 3 个文件里传完了 1 个。'],
      cancelled: true,
    }
    const wrapper = mount(DeployPanel)

    expect(wrapper.text()).toContain('是被停止的')
    expect(wrapper.text()).toContain('传完了 1 个')
  })
})
