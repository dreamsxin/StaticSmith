// @vitest-environment jsdom
import { mount } from '@vue/test-utils'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { computed, reactive } from 'vue'

/**
 * 「回退内容」对话框的行为测试。
 *
 * 回退是这套东西里**动得最多的写操作**：整个内容目录、模板、配置一起换。
 * 它此前既没有测试也没有干跑——只有一句「这之后的改动会被撤销」。所以要钉的是：
 * 选中先干跑、清单要说清哪几类各几个、确认之后才落盘、干跑失败不进确认态。
 */
const state = reactive({
  snapshots: [
    { id: 'aaaaaaaaaa', message: 'batch_delete', at: '2026-09-15T02:00:00Z' },
    { id: 'bbbbbbbbbb', message: 'replace_text', at: '2026-09-14T02:00:00Z' },
  ],
  busy: false,
  currentRaw: '一样',
  savedRaw: '一样',
})

const preview = {
  restored_to: 'aaaaaaaaaa',
  message: 'batch_delete',
  at: '2026-09-15T02:00:00Z',
  changes: [
    { path: 'content/posts/a.md', kind: 'overwrite' as const },
    { path: 'content/posts/new.md', kind: 'delete' as const },
    { path: 'content/posts/gone.md', kind: 'recover' as const },
  ],
}

const actions = {
  loadSnapshots: vi.fn(async () => {}),
  previewRestore: vi.fn(async () => preview as typeof preview | undefined),
  restoreSnapshot: vi.fn(async () => {}),
}

vi.mock('../store', () => ({
  store: state,
  actions,
  isDirty: computed(() => state.currentRaw !== state.savedRaw),
  isTemplateDirty: computed(() => false),
}))

const SnapshotDialog = (await import('./SnapshotDialog.vue')).default

function mountDialog() {
  return mount(SnapshotDialog, { props: { open: false }, attachTo: document.body })
}

async function tick() {
  await new Promise((resolve) => setTimeout(resolve))
}

/** 打开对话框并选中第一条快照，走到「等确认」那一步。 */
async function pickFirst(wrapper: ReturnType<typeof mountDialog>) {
  await wrapper.setProps({ open: true })
  await tick()
  await wrapper.findAll('.snapshot')[0].trigger('click')
  await tick()
}

function button(wrapper: ReturnType<typeof mountDialog>, text: string) {
  return wrapper.findAll('button').find((b) => b.text().includes(text))!
}

describe('SnapshotDialog', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    actions.previewRestore.mockResolvedValue(preview)
    state.busy = false
    state.currentRaw = '一样'
    state.savedRaw = '一样'
  })

  it('关着的时候什么也不渲染', () => {
    const wrapper = mountDialog()
    expect(wrapper.find('.palette').exists()).toBe(false)
  })

  it('打开就读一次清单，并列出每一条（做了什么、什么时候、哪个 id）', async () => {
    const wrapper = mountDialog()
    await wrapper.setProps({ open: true })
    await tick()

    expect(actions.loadSnapshots).toHaveBeenCalled()
    const rows = wrapper.findAll('.snapshot')
    expect(rows).toHaveLength(2)
    expect(rows[0].text()).toContain('aaaaaaaaaa')
    // 操作标识翻成人话，而不是原样显示 batch_delete
    expect(rows[0].get('.snapshot__what').text()).not.toBe('batch_delete')
  })

  it('选中一条只干跑，不落盘', async () => {
    const wrapper = mountDialog()
    await pickFirst(wrapper)

    expect(actions.previewRestore).toHaveBeenCalledWith('aaaaaaaaaa')
    expect(actions.restoreSnapshot).not.toHaveBeenCalled()
  })

  it('干跑清单说清三类各几个，并列出具体路径', async () => {
    const wrapper = mountDialog()
    await pickFirst(wrapper)

    const text = wrapper.text()
    expect(text).toContain('会动 3 个文件')
    expect(text).toContain('换回旧版 1 个')
    expect(text).toContain('删掉 1 个')
    expect(text).toContain('找回 1 个')
    expect(text).toContain('content/posts/a.md')
    expect(text).toContain('content/posts/new.md')
    expect(text).toContain('content/posts/gone.md')
  })

  it('确认之后才落盘，并把对话框收起来', async () => {
    const wrapper = mountDialog()
    await pickFirst(wrapper)
    await button(wrapper, '确认回退').trigger('click')
    await tick()

    expect(actions.restoreSnapshot).toHaveBeenCalledWith('aaaaaaaaaa')
    expect(wrapper.emitted('close')).toHaveLength(1)
  })

  it('「换一份」回到列表，什么也不做', async () => {
    const wrapper = mountDialog()
    await pickFirst(wrapper)
    await button(wrapper, '换一份').trigger('click')
    await tick()

    expect(actions.restoreSnapshot).not.toHaveBeenCalled()
    expect(wrapper.findAll('.snapshot')).toHaveLength(2)
  })

  it('这次回退什么都不会改时置灰确认：那一下点了也没有任何结果', async () => {
    actions.previewRestore.mockResolvedValue({ ...preview, changes: [] })
    const wrapper = mountDialog()
    await pickFirst(wrapper)

    expect(wrapper.text()).toContain('不会改动任何文件')
    expect(button(wrapper, '确认回退').attributes('disabled')).toBeDefined()
  })

  it('干跑失败就不进确认态：没算出来的东西不能拿去让人确认', async () => {
    actions.previewRestore.mockResolvedValue(undefined)
    const wrapper = mountDialog()
    await pickFirst(wrapper)

    expect(wrapper.findAll('.snapshot')).toHaveLength(2)
    expect(wrapper.findAll('button').some((b) => b.text().includes('确认回退'))).toBe(false)
  })

  it('编辑器里有未保存改动时说出来：这次回退会让它作废', async () => {
    state.currentRaw = '改了一句'
    const wrapper = mountDialog()
    await pickFirst(wrapper)

    expect(wrapper.get('.snapshot__warn').text()).toContain('文章')
  })

  it('忙的时候不让确认，但「换一份」照样能按', async () => {
    const wrapper = mountDialog()
    await pickFirst(wrapper)
    state.busy = true
    await wrapper.vm.$nextTick()

    expect(button(wrapper, '确认回退').attributes('disabled')).toBeDefined()
    expect(button(wrapper, '换一份').attributes('disabled')).toBeUndefined()
  })

  it('Esc 关闭', async () => {
    const wrapper = mountDialog()
    await wrapper.setProps({ open: true })
    await tick()
    await wrapper.get('[role="dialog"]').trigger('keydown.esc')
    expect(wrapper.emitted('close')).toHaveLength(1)
  })
})
