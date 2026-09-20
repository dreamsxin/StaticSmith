// @vitest-environment jsdom
import { mount } from '@vue/test-utils'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { computed, reactive } from 'vue'

/**
 * 编辑器里「改地址」那条路的行为测试。
 *
 * 只测这一条：它是唯一会**写到用户没有点名的文件**上的编辑器动作（那些引用这一篇的
 * 文章），因此必须先就地问一句；而「问过之后还剩什么状态」正是最容易漏的地方。
 *
 * `../store` 整个被替换掉：真 store 一路连到 Tauri 的 `invoke`，而这里要验的是组件
 * 在给定回答下怎么走，不是 IPC 有没有接通（后者由 Rust 侧的测试与 docs/ipc.md 管）。
 */
const slugPreview = {
  source: 'posts/a.md',
  from_url: '/posts/a/',
  to_url: '/posts/新名字/',
  refs: [{ source: 'posts/b.md', hits: 2 }],
  refs_manual: [{ source: 'posts/c.md', hits: 1 }],
}

const state = reactive({
  currentSource: 'posts/a.md' as string | null,
  currentRaw: '+++\ntitle = "甲"\n+++\n\n正文\n',
  savedRaw: '+++\ntitle = "甲"\n+++\n\n正文\n',
  project: {
    config: { build: { source_format: 'markdown' } },
    pages: [
      {
        source: 'posts/a.md',
        title: '甲',
        url: '/posts/a/',
        section: 'posts',
        is_index: false,
        draft: false,
        tags: [] as string[],
      },
    ],
    recent_builds: [] as unknown[],
    broken_sources: [] as Array<{ source: string; reason: string }>,
  },

  frontMatter: {
    title: '甲',
    date: null,
    template: null,
    slug: null as string | null,
    description: '',
    tags: [] as string[],
    keywords: [] as string[],
    aliases: [] as string[],
    draft: false,
    weight: 0,
    extra: {},
  },
  plan: null,
  busy: false,
  // 编辑器页脚的「上一篇 / 下一篇」要按整本书的顺序算，那要栏目表
  sections: [{ path: 'posts', title: 'posts', description: '', weight: 0, index_source: null, pages: 1 }],
  pendingPage: null,
  assets: [] as unknown[],
  previewServer: null,
  lastDeploy: null,
  autoBuild: false,
})

const actions = {
  previewSlug: vi.fn(async () => slugPreview as typeof slugPreview | undefined),
  changeSlug: vi.fn(async () => {}),
  loadFrontMatter: vi.fn(async () => {}),
  patchFrontMatter: vi.fn(async () => {}),
  setRaw: vi.fn(),
  saveContent: vi.fn(async () => {}),
  refreshPreview: vi.fn(async () => {}),
  loadAssets: vi.fn(async () => {}),
  saveAsset: vi.fn(async () => ''),
  notify: vi.fn(),
  build: vi.fn(async () => {}),
  resolvePending: vi.fn(async () => {}),
}

vi.mock('../store', () => ({
  store: state,
  actions,
  isDirty: computed(() => state.currentRaw !== state.savedRaw),
  brokenReason: computed(
    () =>
      state.project?.broken_sources?.find(
        (item: { source: string }) => item.source === state.currentSource,
      )?.reason ?? null,
  ),
}))


const ContentEditor = (await import('./ContentEditor.vue')).default

/** 属性面板里的「地址（slug）」输入框。 */
function slugBox(wrapper: ReturnType<typeof mount>) {
  return wrapper.findAll('.editor__prop input')[2]
}

async function editSlug(wrapper: ReturnType<typeof mount>, next = '新名字') {
  const box = slugBox(wrapper)
  await box.setValue(next)
  await box.trigger('change')
  await new Promise((resolve) => setTimeout(resolve))
}

describe('ContentEditor 的改地址', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    actions.previewSlug.mockResolvedValue(slugPreview)
    state.currentSource = 'posts/a.md'
    state.frontMatter.slug = null
  })

  it('地址栏默认显示当前地址的末段，而不是空白', () => {
    const wrapper = mount(ContentEditor)
    expect((slugBox(wrapper).element as HTMLInputElement).value).toBe('a')
  })

  it('改完先干跑再问一句，把「还会改哪几篇」说清楚', async () => {
    const wrapper = mount(ContentEditor)
    await editSlug(wrapper)

    expect(actions.previewSlug).toHaveBeenCalledWith('posts/a.md', '新名字')
    expect(actions.changeSlug).not.toHaveBeenCalled()
    const prompt = wrapper.get('.editor__pending').text()
    expect(prompt).toContain('/posts/a/')
    expect(prompt).toContain('/posts/新名字/')
    expect(prompt).toContain('1 篇里的 2 处')
    // 相对链接改写不到，这一句得在同一屏里说清楚
    expect(prompt).toContain('相对链接')
  })

  it('确认之后才落盘', async () => {
    const wrapper = mount(ContentEditor)
    await editSlug(wrapper)
    await wrapper.get('.editor__pending button').trigger('click')

    expect(actions.changeSlug).toHaveBeenCalledWith('posts/a.md', '新名字')
    expect(wrapper.find('.editor__pending').exists()).toBe(false)
  })

  it('取消不落盘，并把输入框读回磁盘上的值', async () => {
    const wrapper = mount(ContentEditor)
    await editSlug(wrapper)
    await wrapper.findAll('.editor__pending button')[1].trigger('click')

    expect(actions.changeSlug).not.toHaveBeenCalled()
    expect(actions.loadFrontMatter).toHaveBeenCalled()
    expect(wrapper.find('.editor__pending').exists()).toBe(false)
  })

  it('干跑被拦下时不出现待确认行（错误由 store 那层弹通知）', async () => {
    actions.previewSlug.mockResolvedValue(undefined)
    const wrapper = mount(ContentEditor)
    await editSlug(wrapper)

    expect(wrapper.find('.editor__pending').exists()).toBe(false)
    expect(actions.loadFrontMatter).toHaveBeenCalled()
  })

  it('读不出来的那一篇：把原因与「修好会怎样」摆在编辑区里', async () => {
    state.project.broken_sources = [
      { source: 'posts/a.md', reason: 'front matter 缺少结束的 `+++`' },
    ]
    const wrapper = mount(ContentEditor)
    await wrapper.vm.$nextTick()

    const banner = wrapper.get('.editor__broken').text()
    expect(banner).toContain('front matter 缺少结束的 `+++`')
    // 属性面板此时是空的，不说明的话看起来像编辑器坏了
    expect(banner).toContain('没有进内容清单')
    expect(banner).toContain('保存')

    state.project.broken_sources = []
    await wrapper.vm.$nextTick()
    expect(wrapper.find('.editor__broken').exists()).toBe(false)
  })

  it('换到另一篇时待确认行必须作废：确认它会把改动落到刚才那一篇上', async () => {

    const wrapper = mount(ContentEditor)
    await editSlug(wrapper)
    expect(wrapper.find('.editor__pending').exists()).toBe(true)

    state.currentSource = 'posts/b.md'
    await wrapper.vm.$nextTick()
    expect(wrapper.find('.editor__pending').exists()).toBe(false)
  })
})
