// @vitest-environment jsdom
import { mount } from '@vue/test-utils'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { computed, reactive } from 'vue'

/**
 * 侧栏的拖放：同栏目内排序 vs 拖进别的栏目。
 *
 * 这两件事看着像一个动作，实际是两条完全不同的路：排序直接落盘（可逆），
 * 改归属要先干跑再确认（它会写到用户没点名的文件上——那些引用这一篇的文章）。
 * 而判断「这一次是哪件事」全靠 `dragover` 时算出来的落点状态，
 * 那是**最容易悄悄坏掉**的一类逻辑：坏了不会报错，只是松手之后什么也没发生。
 *
 * 纯函数那层（`canDropBeside` / `canDropIntoSection`）已经在 grouping.test.ts 里钉住，
 * 这里钉的是接线：事件挂对了没有、冒泡有没有让两个落点提示同时亮、
 * 松手到底调了哪个 action。
 */
function page(source: string, section: string, extra: Record<string, unknown> = {}) {
  return {
    source,
    title: source,
    url: `/${source.replace(/\.md$/, '')}/`,
    template: 'page.html',
    section,
    is_index: false,
    draft: false,
    date: null,
    scheduled: false,
    tags: [] as string[],
    weight: 0,
    ...extra,
  }
}

function sectionMeta(path: string) {
  return { path, title: path, description: '', weight: 0, index_source: null, pages: 1 }
}

const preview = {
  changes: [{ source: 'posts/a.md', changes: true, effect: '搬到 notes/a.md，旧地址 /posts/a/' }],
  affected: 1,
  refs: [{ source: 'posts/b.md', hits: 2 }],
  refs_manual: [] as Array<{ source: string; hits: number }>,
}

const state = reactive({
  busy: false,
  currentRaw: '',
  currentSource: null as string | null,
  outputs: [] as unknown[],
  plan: null,
  previewTarget: null,
  sections: [sectionMeta('posts'), sectionMeta('notes')],
  seo: null,
  project: {
    pages: [
      page('posts/_index.md', 'posts', { is_index: true }),
      page('posts/a.md', 'posts'),
      page('posts/b.md', 'posts'),
      page('notes/c.md', 'notes'),
    ],
    broken_sources: [] as Array<{ source: string; reason: string }>,
  },
})

const actions = {
  batchPreview: vi.fn(async () => preview),
  batchMove: vi.fn(async () => {}),
  batchSetDraft: vi.fn(async () => {}),
  deleteContent: vi.fn(async () => {}),
  movePage: vi.fn(async () => {}),
  movePageTo: vi.fn(async () => {}),
  notify: vi.fn(),
  previewOutput: vi.fn(async () => {}),
  requestOpenContent: vi.fn(async () => {}),
}

vi.mock('../store', () => ({
  store: state,
  actions,
  isDirty: computed(() => false),
  brokenReason: computed(() => null),
}))

vi.mock('../api', () => ({ searchContent: vi.fn(async () => []) }))

const PageList = (await import('./PageList.vue')).default
const SectionHeader = (await import('./SectionHeader.vue')).default
const PageRow = (await import('./PageRow.vue')).default

function open() {
  return mount(PageList, {
    global: {
      stubs: {
        SectionHeader: true,
        ReplacePanel: true,
        CreatePanel: true,
        BrokenList: true,
        BatchBar: true,
      },
    },
  })
}

type Wrapper = ReturnType<typeof open>

/** 按栏目名找到那一组的容器（分组顺序由排序决定，不该在测试里写死下标）。 */
function groupOf(wrapper: Wrapper, section: string) {
  const found = wrapper
    .findAll('.page-list__group')
    .find((group) => group.findComponent(SectionHeader).props('section') === section)
  expect(found, section).toBeTruthy()
  return found!
}

/** 开始拖某一行（`PageList` 只从 `dragstart` 里记下 source）。 */
function startDragging(wrapper: Wrapper, source: string) {
  const row = wrapper
    .findAllComponents(PageRow)
    .find((candidate) => (candidate.props('page') as { source: string }).source === source)
  expect(row, source).toBeTruthy()
  return row!
}

describe('PageList 的跨栏目拖动', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    actions.batchPreview.mockResolvedValue(preview)
  })

  it('拖到别的栏目上：整组亮起落点，松手只发起干跑，不落盘', async () => {
    const wrapper = open()
    startDragging(wrapper, 'posts/a.md').vm.$emit('dragstart')

    const notes = groupOf(wrapper, 'notes')
    await notes.trigger('dragover')
    expect(notes.classes()).toContain('page-list__group--drop')

    await notes.trigger('drop')
    expect(actions.batchPreview).toHaveBeenCalledWith(['posts/a.md'], {
      kind: 'move',
      to_section: 'notes',
    })
    // 关键：干跑不是落盘
    expect(actions.batchMove).not.toHaveBeenCalled()

    await new Promise((resolve) => setTimeout(resolve))
    const dry = groupOf(wrapper, 'notes').get('.page-list__dry')
    expect(dry.text()).toContain('posts/a.md')
    // 会写到用户没点名的文件上，这一句必须在同一屏里
    expect(dry.text()).toContain('站内链接改到新地址')
  })

  it('确认之后才写盘，并且保留旧地址', async () => {
    const wrapper = open()
    startDragging(wrapper, 'posts/a.md').vm.$emit('dragstart')
    const notes = groupOf(wrapper, 'notes')
    await notes.trigger('dragover')
    await notes.trigger('drop')
    await new Promise((resolve) => setTimeout(resolve))

    await groupOf(wrapper, 'notes').get('.page-list__dry .btn--primary').trigger('click')
    expect(actions.batchMove).toHaveBeenCalledWith(['posts/a.md'], 'notes', true)
    // 确认完就收起，留着会让人以为还没执行
    expect(groupOf(wrapper, 'notes').find('.page-list__dry').exists()).toBe(false)
  })

  it('拖回自己所在的栏目不接：等于没动，不该弹一个「什么都不会变」的确认', async () => {
    const wrapper = open()
    startDragging(wrapper, 'posts/a.md').vm.$emit('dragstart')

    const posts = groupOf(wrapper, 'posts')
    await posts.trigger('dragover')
    expect(posts.classes()).not.toContain('page-list__group--drop')

    await posts.trigger('drop')
    expect(actions.batchPreview).not.toHaveBeenCalled()
  })

  it('栏目索引页不接：它的地址就是栏目名，搬它要走「栏目改名」', async () => {
    const wrapper = open()
    startDragging(wrapper, 'posts/_index.md').vm.$emit('dragstart')

    const notes = groupOf(wrapper, 'notes')
    await notes.trigger('dragover')
    expect(notes.classes()).not.toContain('page-list__group--drop')

    await notes.trigger('drop')
    expect(actions.batchPreview).not.toHaveBeenCalled()
  })

  it('进了某一行就撤掉栏目的落点：两个提示同时亮着等于没说清会发生什么', async () => {
    const wrapper = open()
    startDragging(wrapper, 'posts/a.md').vm.$emit('dragstart')

    const notes = groupOf(wrapper, 'notes')
    await notes.trigger('dragover')
    expect(notes.classes()).toContain('page-list__group--drop')

    // 行的 dragover 会冒泡到栏目上，所以顺序是「先进栏目、再进行」
    startDragging(wrapper, 'posts/b.md').vm.$emit('dragover', 'before')
    await wrapper.vm.$nextTick()
    expect(groupOf(wrapper, 'notes').classes()).not.toContain('page-list__group--drop')
  })

  it('同栏目内松手走排序那条路，不碰搬动', async () => {
    const wrapper = open()
    startDragging(wrapper, 'posts/a.md').vm.$emit('dragstart')

    const target = startDragging(wrapper, 'posts/b.md')
    target.vm.$emit('dragover', 'after')
    await wrapper.vm.$nextTick()
    target.vm.$emit('drop')
    await new Promise((resolve) => setTimeout(resolve))

    expect(actions.movePageTo).toHaveBeenCalledWith('posts/a.md', 'posts/b.md', 'after')
    expect(actions.batchPreview).not.toHaveBeenCalled()
  })

  it('拖过一个栏目、松手在另一个栏目上：不按松手那个算，什么都不做', async () => {
    const wrapper = open()
    startDragging(wrapper, 'posts/a.md').vm.$emit('dragstart')

    // 高亮的是 notes，松手却落在 posts 上（鼠标快速划过时真会这样）
    await groupOf(wrapper, 'notes').trigger('dragover')
    await groupOf(wrapper, 'posts').trigger('drop')

    expect(actions.batchPreview).not.toHaveBeenCalled()
    expect(actions.batchMove).not.toHaveBeenCalled()
  })

  it('拖进分组里的子元素不算「拖出去」：否则落点态会一闪一闪', async () => {
    const wrapper = open()
    startDragging(wrapper, 'posts/a.md').vm.$emit('dragstart')

    const notes = groupOf(wrapper, 'notes')
    await notes.trigger('dragover')
    expect(notes.classes()).toContain('page-list__group--drop')

    // 浏览器在拖进子元素时也会给父元素派发一次 dragleave，带着子元素当 relatedTarget
    const child = notes.element.querySelector('h3, ul, p')
    expect(child).toBeTruthy()
    await notes.trigger('dragleave', { relatedTarget: child })
    expect(groupOf(wrapper, 'notes').classes()).toContain('page-list__group--drop')

    // 真的拖到分组外面（relatedTarget 在别处）才撤掉
    await notes.trigger('dragleave', { relatedTarget: document.body })
    expect(groupOf(wrapper, 'notes').classes()).not.toContain('page-list__group--drop')
  })
})
