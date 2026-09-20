// @vitest-environment jsdom
import { mount } from '@vue/test-utils'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { computed, reactive } from 'vue'

/**
 * 正文的写入路径：改动必须记进 textarea 的**原生撤销栈**。
 *
 * 起因是一个真 bug：工具条、查找替换、「整节挪上挪下」都走 `actions.setRaw()` 直接
 * 换掉整篇字符串，而那样写 `Ctrl+Z` 撤不回去——撤销栈里没有这一步，按下去撤掉的是
 * 用户上一次**打字**。在一个写作工具里，「挪错了一节撤不回来」比挪不动更糟。
 *
 * 唯一能进原生撤销栈的 API 是 `document.execCommand('insertText')`（已废弃，但
 * `setRangeText` 与直接赋值 `value` 都会把栈清掉）。jsdom 里它不存在，所以这里
 * 自己装一个假的，断言「组件是否走了这条路」——这也正是真实环境里唯一可验的点。
 */
const state = reactive({
  currentSource: 'posts/a.md' as string | null,
  currentRaw: '一二三四五',
  savedRaw: '一二三四五',
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
        weight: 1,
      },
      // 第二篇：页脚的「下一篇」要有地方可去
      {
        source: 'posts/b.md',
        title: '乙',
        url: '/posts/b/',
        section: 'posts',
        is_index: false,
        draft: false,
        tags: [] as string[],
        weight: 2,
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
  // 页脚的「上一篇 / 下一篇」按整本书的顺序算，那要栏目表
  sections: [
    { path: 'posts', title: 'posts', description: '', weight: 0, index_source: null, pages: 2 },
  ],
  pendingPage: null,
  assets: [] as unknown[],
  previewServer: null,
  lastDeploy: null,
  autoBuild: false,
})

const actions = {
  previewSlug: vi.fn(async () => undefined),
  changeSlug: vi.fn(async () => {}),
  loadFrontMatter: vi.fn(async () => {}),
  patchFrontMatter: vi.fn(async () => {}),
  setRaw: vi.fn(),
  saveContent: vi.fn(async () => {}),
  refreshPreview: vi.fn(async () => {}),
  loadAssets: vi.fn(async () => {}),
  saveAsset: vi.fn(async () => ''),
  notify: vi.fn(),
  requestOpenContent: vi.fn(async () => {}),
  build: vi.fn(async () => {}),
  resolvePending: vi.fn(async () => {}),
}

vi.mock('../store', () => ({
  store: state,
  actions,
  isDirty: computed(() => state.currentRaw !== state.savedRaw),
  brokenReason: computed(() => null),
}))

const ContentEditor = (await import('./ContentEditor.vue')).default
const { ui } = await import('../ui')

/** 装一个假的 execCommand，返回它记下的调用。 */
function fakeExecCommand(result = true) {
  const spy = vi.fn(() => result)
  // jsdom 没有这个 API，直接挂上去
  ;(document as unknown as { execCommand: () => boolean }).execCommand = spy
  return spy
}

/** 挂载编辑器，把选区设在 `[start, end)`，并返回命令注册点。 */
function open(start: number, end: number) {
  const wrapper = mount(ContentEditor, { attachTo: document.body })
  const area = wrapper.get('.editor__area').element as HTMLTextAreaElement
  area.setSelectionRange(start, end)
  return { wrapper, area, editor: ui.editor! }
}

describe('ContentEditor 写正文的通道', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    state.currentRaw = '一二三四五'
    state.savedRaw = '一二三四五'
  })

  it('加粗走原生插入，而不是直接换掉整篇字符串', () => {
    const exec = fakeExecCommand()
    const { editor } = open(1, 3)

    editor.bold()

    expect(exec).toHaveBeenCalledWith('insertText', false, '**二三**')
    // 直接 setRaw 会清掉撤销栈，那正是这个 bug 的根因
    expect(actions.setRaw).not.toHaveBeenCalled()
  })

  it('加标题也走同一条路（整行块替换）', () => {
    const exec = fakeExecCommand()
    const { editor } = open(0, 0)

    editor.heading()

    expect(exec).toHaveBeenCalledWith('insertText', false, '## 一二三四五')
    expect(actions.setRaw).not.toHaveBeenCalled()
  })

  it('整节挪动前先选中被换掉的那一段：撤销一步就该回到挪动之前', () => {
    state.currentRaw = '## 甲\n\n甲的正文\n\n## 乙\n\n乙的正文\n'
    state.savedRaw = state.currentRaw
    const exec = fakeExecCommand()
    const { area, editor } = open(0, 0)

    // 第二个标题的偏移，把它往上挪
    editor.moveHeading(state.currentRaw.indexOf('## 乙'), -1)

    expect(exec).toHaveBeenCalledOnce()
    expect(area.selectionStart).toBe(0)
    expect(area.selectionEnd).toBe(state.currentRaw.length)
    expect(actions.setRaw).not.toHaveBeenCalled()
  })

  it('没有 execCommand 的环境退回直接写：撤销没了，但至少还能编辑', () => {
    fakeExecCommand(false)
    const { editor } = open(1, 3)

    editor.bold()

    expect(actions.setRaw).toHaveBeenCalledWith('一**二三**四五')
  })

  /**
   * 顺着目录往下写：这是「像编辑一本书」里最常用的一步。
   * 打开走的是 `requestOpenContent`——与侧栏点一行同一条路，未保存的改动照样会先问一句。
   */
  it('页脚能跳到下一篇；第一篇的「上一篇」置灰而不是藏起来', async () => {
    const { wrapper } = open(0, 0)
    const around = wrapper.findAll('.editor__around button')
    expect(around).toHaveLength(2)

    // 当前打开的是第一篇：往前没有了，按钮置灰但仍看得见（隐藏会让人以为没这功能）
    expect(around[0].attributes('disabled')).toBeDefined()
    expect(around[1].attributes('disabled')).toBeUndefined()

    await around[1].trigger('click')
    expect(actions.requestOpenContent).toHaveBeenCalledWith(
      expect.objectContaining({ source: 'posts/b.md' }),
    )
  })
})
