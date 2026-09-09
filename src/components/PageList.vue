<script setup lang="ts">
/**
 * 内容侧栏：按栏目分组列出内容页，支持搜索、新建、删除，并标出待重新生成的页面。
 *
 * 布局按「这是什么 → 找什么 → 有什么」三段组织：
 * 标题行说明这一栏是内容并给出新建入口，搜索行只负责过滤，剩下才是列表。
 * 之前搜索框与「＋」并排且没有任何标识，很容易被当成「新建内容的名称输入框」。
 */
import { computed, reactive, ref, watch } from 'vue'

import { openContextMenu, type MenuEntry } from '../commands'
import { actions, isDirty, store } from '../store'
import { parseList } from '../text'
import { ui } from '../ui'
import { searchContent } from '../api'
import { outputKindLabel } from '../labels'
import type { BatchPreview, PageSummary, ReplaceResult, SearchHit, SeoSeverity } from '../api'

const keyword = ref('')
const searchBox = ref<HTMLInputElement | null>(null)

/**
 * 运营视角的筛选。
 *
 * 体检面板能告诉你「有 12 篇缺描述」，但补的时候还是要回到列表里一篇篇找。
 * 这一排筛选把体检结论接回工作列表：选「待补 SEO」就只剩要动的那些。
 */
type Filter = 'all' | 'draft' | 'dirty' | 'seo' | 'scheduled'
const filter = ref<Filter>('all')

const normalized = computed(() => keyword.value.trim().toLowerCase())

/** 每篇文章最严重的那条 SEO 问题。站点级问题没有 source，自然被排除。 */
const seoBySource = computed(() => {
  const worst = new Map<string, { severity: SeoSeverity; messages: string[] }>()
  const rank: Record<SeoSeverity, number> = { error: 0, warn: 1, hint: 2 }
  for (const issue of store.seo?.issues ?? []) {
    if (!issue.source) continue
    const current = worst.get(issue.source)
    if (!current) {
      worst.set(issue.source, { severity: issue.severity, messages: [issue.message] })
      continue
    }
    current.messages.push(issue.message)
    if (rank[issue.severity] < rank[current.severity]) current.severity = issue.severity
  }
  return worst
})

const dirtyPages = computed(() => new Set(store.plan?.pages ?? []))

function matchesFilter(page: PageSummary): boolean {
  switch (filter.value) {
    case 'draft':
      return page.draft
    case 'dirty':
      return dirtyPages.value.has(page.source)
    case 'seo':
      return seoBySource.value.has(page.source)
    case 'scheduled':
      return page.scheduled
    default:
      return true
  }
}

const groups = computed(() => {
  const map = new Map<string, PageSummary[]>()
  // 先把已知栏目摆上：空栏目也要看得见，否则新建完就「消失」了。
  // 搜索或筛选时不补空栏目——那时用户要的是命中项，不是完整结构。
  if (!normalized.value && filter.value === 'all') {
    for (const section of store.sections) map.set(section.path, [])
  }
  for (const page of store.project?.pages ?? []) {
    if (normalized.value && !`${page.title}\n${page.source}`.toLowerCase().includes(normalized.value))
      continue
    if (!matchesFilter(page as PageSummary)) continue
    const list = map.get(page.section) ?? []
    list.push(page as PageSummary)
    map.set(page.section, list)
  }
  // 栏目顺序跟着索引页的 weight 走，与站点上列出的顺序一致；
  // 没排过序的（weight 0）按路径，免得顺序看起来随机
  return [...map.entries()].sort(
    ([a], [b]) => weightOf(a) - weightOf(b) || a.localeCompare(b),
  )
})

function weightOf(path: string): number {
  return sectionOf.value.get(path)?.weight ?? 0
}

/** 栏目元信息（有没有索引页、直属篇数）按路径取用。 */
const sectionOf = computed(() => new Map(store.sections.map((s) => [s.path, s])))

function sectionLabel(path: string): string {
  return path === '' ? '根目录' : path
}


const total = computed(() => store.project?.pages.length ?? 0)
const matched = computed(() => groups.value.reduce((sum, [, pages]) => sum + pages.length, 0))

/**
 * 筛选项。
 *
 * 标签把作用写全（「草稿（还没发布）」而不是「草稿」）：两个字的标签省地方，
 * 但没人猜得出「待生成」是什么意思。计数跟在后面，为空的那类置灰——
 * 摆在那里让人点一下才发现没有，等于白点一次。
 */
const filters = computed<Array<{ id: Filter; label: string; count: number }>>(() => {
  const pages = store.project?.pages ?? []
  return [
    { id: 'all', label: '全部内容', count: pages.length },
    { id: 'draft', label: '草稿（还没发布）', count: pages.filter((p) => p.draft).length },
    {
      id: 'dirty',
      label: '待生成（改过还没生成）',
      count: pages.filter((p) => dirtyPages.value.has(p.source)).length,
    },
    {
      id: 'seo',
      label: '待补 SEO（缺描述等）',
      count: pages.filter((p) => seoBySource.value.has(p.source)).length,
    },
    {
      id: 'scheduled',
      label: '定时（日期未到，暂不上线）',
      count: pages.filter((p) => p.scheduled).length,
    },
  ]
})


/**
 * 生成出来但没有源文件的页面：标签列表、标签页、分页页。
 *
 * 它们只存在于产物目录里，内容树按源文件组织，因此需要单独一组，点击走服务器预览。
 */
const sitePages = computed(() =>
  store.outputs.filter(
    (o) =>
      (o.kind === 'taxonomy' || o.kind === 'pagination') &&
      (!normalized.value || o.url.toLowerCase().includes(normalized.value)),
  ),
)

// ---------------------------------------------------------------- 搜索

// Ctrl+F 的监听在 App 那一层（`commands.ts` 的 `focusSearch`）：挂在这里的话，
// 停在别的标签页、或把列表栏收起来时按下去就没有反应。
// 这里只负责被叫到时聚焦，见下面 `ui.requestFocusSearch` 的 watch。

/**
 * 正文命中：同一个输入框，两档结果。
 *
 * 上面那份分组列表是「标题或路径里有这个词」，在本地已加载的清单上即时过滤；
 * 这一份是「正文里提到过这个词」，要问 Rust 侧（正文是渲染后的 HTML，
 * 前端手里根本没有）。以前只有前者，于是「上次写过某个词的那篇」在界面里搜不出来，
 * 只有 AI Agent 能搜——同一个站点两种能力，说不通。
 *
 * 不加第二个输入框：搜索只该有一个入口，结果分档展示（同 6.9 的分档思路）。
 * 已经在上面出现过的文章不再重复列，否则一个词会出现两次。
 */
const bodyHits = ref<SearchHit[]>([])
const searching = ref(false)

/** 至少两个字符才去搜：单字符命中太多，等于把整站列一遍。 */
const MIN_QUERY = 2
let searchTimer: ReturnType<typeof setTimeout> | null = null

watch(normalized, (query) => {
  if (searchTimer !== null) clearTimeout(searchTimer)
  if (query.length < MIN_QUERY) {
    bodyHits.value = []
    searching.value = false
    return
  }
  // 防抖：打字过程中每个字符都发一次 IPC，站点大了会把界面拖住
  searching.value = true
  searchTimer = setTimeout(async () => {
    searchTimer = null
    try {
      bodyHits.value = await searchContent(query, 30)
    } catch {
      // 搜索失败不弹通知：它是辅助结果，主列表还在，报错反而打断打字
      bodyHits.value = []
    } finally {
      searching.value = false
    }
  }, 250)
})

/** 只留正文命中，且排除标题列表里已经出现过的那些。 */
const extraHits = computed(() => {
  const shown = new Set(groups.value.flatMap(([, pages]) => pages.map((p) => p.source)))
  return bodyHits.value.filter((hit) => hit.field === 'body' && !shown.has(hit.source))
})

/** 从一条命中回到那篇文章。带未保存改动时由 store 拦一道，这里不重复判断。 */
async function openHit(hit: SearchHit) {
  const page = (store.project?.pages ?? []).find((p) => p.source === hit.source)
  if (page) await actions.requestOpenContent(page as PageSummary)
}


// ---------------------------------------------------------------- 新建

const creating = ref(false)
const newTitle = ref('')
const newSection = ref('posts')
const titleBox = ref<HTMLInputElement | null>(null)

/** 已有栏目做候选，避免同一个栏目写出 post / posts 两种。 */
const sections = computed(() => {
  const set = new Set<string>()
  for (const page of store.project?.pages ?? []) if (page.section) set.add(page.section)
  return [...set].sort()
})

function openCreate() {
  creating.value = true
  replacing.value = false
  // 展开即聚焦到标题，少一次点击
  requestAnimationFrame(() => titleBox.value?.focus())
}

async function create() {
  if (!newTitle.value.trim()) return
  await actions.createContent(newTitle.value.trim(), newSection.value.trim())
  newTitle.value = ''
  creating.value = false
}

/**
 * 响应菜单栏的请求。
 *
 * 菜单里的「新建文章…」「查找内容」只能放个信号：点的时候侧栏可能还没挂载
 * （停在别的标签页）。这里消费完立刻清零，免得下次挂载时又弹一遍。
 */
watch(
  () => ui.requestNewContent,
  (asked) => {
    if (!asked) return
    ui.requestNewContent = false
    openCreate()
  },
  { immediate: true },
)

watch(
  () => ui.requestFocusSearch,
  (asked) => {
    if (!asked) return
    ui.requestFocusSearch = false
    requestAnimationFrame(() => {
      searchBox.value?.focus()
      // 选中已有关键词：再按一次 Ctrl+F 通常是想换个词搜，而不是接着上次的往后打
      searchBox.value?.select()
    })
  },
  { immediate: true },
)

// 「新建栏目」的同类 watch 放在 openCreateSection 定义之后：
// immediate 的回调在 setup 阶段就会跑，引用后面才声明的 ref 会踩 TDZ

// ---------------------------------------------------------------- 多选与批量

/**
 * 批量动作。
 *
 * 「把这十二篇都补上标签」「这一批放出去」是运营里最费手的操作，逐篇点开必然出错。
 * 选择态是显式的：不开「多选」就不会有勾选框，避免误点把批量动作作用到看不见的条目上。
 * 没有撤销栈——理由写在 store 的 afterBatch 上。
 */
const selecting = ref(false)
const selected = ref(new Set<string>())

function toggleSelecting() {
  selecting.value = !selecting.value
  if (!selecting.value) selected.value = new Set()
}

function toggleOne(source: string) {
  const next = new Set(selected.value)
  if (next.has(source)) next.delete(source)
  else next.add(source)
  selected.value = next
}

/** 只选当前可见（已按搜索与筛选过滤）的条目：所见即所选。 */
function selectVisible() {
  const next = new Set<string>()
  for (const [, pages] of groups.value) for (const page of pages) next.add(page.source)
  selected.value = next
}

const selectedList = computed(() => [...selected.value])

/** 批量加标签时给出已有标签，避免同一个概念写出三种写法。 */
const knownTags = computed(() =>
  [...new Set((store.project?.pages ?? []).flatMap((p) => p.tags))].sort((a, b) =>
    a.localeCompare(b),
  ),
)


const batchTags = ref('')
const batchSection = ref('')
const batchKeepAliases = ref(true)

/**
 * 待确认的动作及其干跑结果。
 *
 * 搬动会改地址、删除不可逆，这两件事先看一眼「哪几篇会怎么变」再落盘；
 * 加标签、切草稿反手就能改回来，不值得多一次点击。
 */
const pending = ref<{ kind: 'move' | 'delete'; preview: BatchPreview } | null>(null)


/** 每次批量动作后清空选择：文件可能已经改名、搬走或删掉，旧的选中集没有意义。 */
function clearSelection() {
  selected.value = new Set()
  pending.value = null
}

async function applyTags(mode: 'add' | 'remove') {
  const tags = parseList(batchTags.value)
  if (!tags.length) return
  const [add, remove] = mode === 'add' ? [tags, []] : [[], tags]
  await actions.batchEditTags(selectedList.value, add, remove)
  batchTags.value = ''
  clearSelection()
}

async function setDraft(draft: boolean) {
  await actions.batchSetDraft(selectedList.value, draft)
  clearSelection()
}

/** 先干跑：让用户看清「哪几篇会搬到哪、旧地址是什么」再决定。 */
async function previewMove() {
  const preview = await actions.batchPreview(selectedList.value, {
    kind: 'move',
    to_section: batchSection.value.trim(),
  })
  if (preview) pending.value = { kind: 'move', preview }
}

async function previewDelete() {
  const preview = await actions.batchPreview(selectedList.value, { kind: 'delete' })
  if (preview) pending.value = { kind: 'delete', preview }
}

async function confirmPending() {
  const kind = pending.value?.kind
  pending.value = null
  if (kind === 'move') {
    await actions.batchMove(selectedList.value, batchSection.value.trim(), batchKeepAliases.value)
    batchSection.value = ''
  } else if (kind === 'delete') {
    await actions.batchDelete(selectedList.value)
  }
  clearSelection()
}


// ---------------------------------------------------------------- 栏目管理


/**
 * 栏目就是 `content/` 下的一层目录，此前只能去文件管理器里建/改/删，
 * 而改名之后老链接会全部 404。这里把三件事收进界面，并默认保留旧地址。
 */
const creatingSection = ref(false)
const newSectionPath = ref('')
const newSectionTitle = ref('')
const sectionBox = ref<HTMLInputElement | null>(null)

function openCreateSection() {
  creatingSection.value = true
  creating.value = false
  replacing.value = false
  requestAnimationFrame(() => sectionBox.value?.focus())
}

async function createSection() {
  const path = newSectionPath.value.trim()
  if (!path) return
  await actions.createSection(path, newSectionTitle.value.trim())
  newSectionPath.value = ''
  newSectionTitle.value = ''
  creatingSection.value = false
}

watch(
  () => ui.requestNewSection,
  (asked) => {
    if (!asked) return
    ui.requestNewSection = false
    openCreateSection()
  },
  { immediate: true },
)

// ---------------------------------------------------------------- 跨文件替换

/**
 * 改一个称呼、统一一个术语：以前只能逐篇点开改。
 *
 * 与批量动作放在一起而不是做成浮层：它跟「多选」共用一个「范围」概念——
 * 开着多选并选了几篇时，这里能只改那几篇。
 *
 * 只改正文，front matter 不在范围内（理由见 staticsmith_core::replace），
 * 界面上必须说出来，否则用户会以为标题里的词也一起换了。
 */
const replacing = ref(false)
const findText = ref('')
const replaceText = ref('')
const ignoreCase = ref(false)
const onlySelected = ref(false)
const findBox = ref<HTMLInputElement | null>(null)
/** 干跑结果。为 null 表示还没预览过——没预览过不给按「替换」。 */
const replacePreview = ref<ReplaceResult | null>(null)

function openReplace() {
  replacing.value = true
  creating.value = false
  creatingSection.value = false
  replacePreview.value = null
  requestAnimationFrame(() => findBox.value?.focus())
}

function closeReplace() {
  replacing.value = false
  replacePreview.value = null
}

/** 改了任一条件，之前那份干跑结果就不再对应当前输入，作废掉。 */
watch([findText, replaceText, ignoreCase, onlySelected, selectedList], () => {
  replacePreview.value = null
})

const replaceScope = computed(() => (onlySelected.value ? selectedList.value : []))

async function runReplacePreview() {
  const result = await actions.previewReplace({
    find: findText.value,
    replace: replaceText.value,
    ignore_case: ignoreCase.value,
    sources: replaceScope.value,
  })
  if (result) replacePreview.value = result
}

async function confirmReplace() {
  await actions.applyReplace({
    find: findText.value,
    replace: replaceText.value,
    ignore_case: ignoreCase.value,
    sources: replaceScope.value,
  })
  closeReplace()
}

watch(
  () => ui.requestReplace,
  (asked) => {
    if (!asked) return
    ui.requestReplace = false
    openReplace()
  },
  { immediate: true },
)


const renamingSection = ref<string | null>(null)
const renameTo = ref('')
const keepAliases = ref(true)

/**
 * 就地表单与确认态出现后，把焦点放到最该操作的那个控件。
 *
 * 键盘用户点开「改名…」之后，焦点还留在刚刚消失的菜单项上——等于回到 body，
 * 要按好几次 Tab 才走到新出现的输入框。确认态聚焦的是**取消**：
 * 危险动作不该是默认落点，误按空格就删了。
 *
 * 用选择器而不是模板 ref：这几个表单在 v-for 里，ref 会收集成数组，
 * 而同一时刻只可能展开一个（互斥由各自的 start 函数保证），选择器反而更直白。
 */
function focusInside(selector: string) {
  requestAnimationFrame(() => document.querySelector<HTMLElement>(selector)?.focus())
}

function startRename(path: string) {
  renamingSection.value = path
  renameTo.value = path
  editingMeta.value = null
  confirmingSectionDelete.value = null
  focusInside('.page-list__rename input')
}

async function submitRename() {
  const from = renamingSection.value
  const to = renameTo.value.trim()
  renamingSection.value = null
  if (!from || !to || from === to) return
  await actions.renameSection(from, to, keepAliases.value)
}

/** 删空栏目也是不可逆的，沿用列表里的就地确认，不用原生弹窗。 */
const confirmingSectionDelete = ref<string | null>(null)

async function removeSection(path: string) {
  confirmingSectionDelete.value = null
  await actions.removeSection(path)
}

/**
 * 栏目元信息：标题、简介、排序权重。
 *
 * 这三项都存在索引页的 front matter 里——栏目就是目录，它的介绍该在那张列表页上，
 * 而不是另开一个栏目配置文件。缺索引页的栏目保存时会顺手补一张。
 */
const editingMeta = ref<string | null>(null)
const metaForm = reactive({ title: '', description: '', weight: 0 })

function startMeta(path: string) {
  const section = sectionOf.value.get(path)
  metaForm.title = section?.title ?? sectionLabel(path)
  metaForm.description = section?.description ?? ''
  metaForm.weight = section?.weight ?? 0
  editingMeta.value = path
  renamingSection.value = null
  confirmingSectionDelete.value = null
  focusInside('.page-list__rename input')
}

async function submitMeta() {
  const path = editingMeta.value
  if (path === null || !metaForm.title.trim()) return
  editingMeta.value = null
  await actions.saveSectionMeta(path, {
    title: metaForm.title.trim(),
    description: metaForm.description.trim(),
    weight: metaForm.weight,
  })
}

// ---------------------------------------------------------------- 删除文章

/**
 * 待确认删除的源路径。
 *
 * 不用 `window.confirm`：Tauri 的 WebView 里原生弹窗会抢焦点且样式与应用割裂，
 * 就地把按钮换成「删除 / 取消」更轻，也不会挡住列表。
 */
const confirmingDelete = ref<string | null>(null)

async function remove(page: PageSummary) {
  confirmingDelete.value = null
  await actions.deleteContent(page)
}

/**
 * 右键菜单：只放针对这一篇的动作。
 *
 * 行上原先挂着一个「×」，删除以外的动作全靠批量条。现在「×」换成「⋯」，
 * 同一份菜单既由右键触发也由「⋯」触发——右键是看不见的入口，必须有个可见的孪生入口。
 * 删除仍走就地确认，菜单只负责把那一行切进确认态：菜单里直接删等于绕过拦阻。
 */
function pageMenu(page: PageSummary): MenuEntry[] {
  return [
    { id: 'page.open', label: '打开编辑', run: () => actions.requestOpenContent(page) },
    {
      id: 'page.preview',
      label: '在预览里打开',
      hint: page.url,
      run: () => actions.previewOutput(page.url),
    },
    { separator: true },
    {
      id: 'page.draft',
      label: page.draft ? '发布（取消草稿）' : '设为草稿',
      run: () => actions.batchSetDraft([page.source], !page.draft),
    },
    {
      id: 'page.copy',
      label: '复制源文件路径',
      hint: page.source,
      run: () => copyText(page.source),
    },
    { separator: true },
    {
      id: 'page.delete',
      label: '删除…',
      hint: '还要再确认一次',
      danger: true,
      run: () => {
        confirmingDelete.value = page.source
        focusInside('.page-list__confirm-cancel')
      },
    },
  ]
}

/** 分组头的右键菜单：栏目级动作。根目录不能改名也不能删，那两项直接不出现。 */
function sectionMenu(path: string): MenuEntry[] {
  const section = sectionOf.value.get(path)
  const items: MenuEntry[] = [
    {
      id: 'section.new',
      label: '在此栏目新建文章…',
      run: () => {
        newSection.value = path
        openCreate()
      },
    },
    { separator: true },
    {
      id: 'section.meta',
      label: '栏目信息（标题 / 简介 / 排序）…',
      run: () => startMeta(path),
    },
  ]
  if (path === '') return items
  items.push(
    { id: 'section.rename', label: '栏目改名…', hint: '默认保留旧地址', run: () => startRename(path) },
    { separator: true },
    {
      id: 'section.remove',
      label: '删除空栏目…',
      hint: (section?.pages ?? 0) > 0 ? '里面还有文章' : '还要再确认一次',
      danger: true,
      disabled: (section?.pages ?? 0) > 0,
      run: () => {
        confirmingSectionDelete.value = path
        focusInside('.page-list__confirm-cancel')
      },
    },
  )
  return items
}

/** 复制到剪贴板。写不进去（无权限、旧 WebView）就说清楚，别假装成功。 */
async function copyText(text: string) {
  try {
    await navigator.clipboard.writeText(text)
    actions.notify('success', `已复制 ${text}`)
  } catch {
    actions.notify('error', '复制失败：这个环境不允许写剪贴板')
  }
}
</script>

<template>
  <nav class="page-list" aria-label="内容">
    <header class="page-list__head">
      <h2>内容</h2>
      <span class="page-list__count">
        {{ normalized || filter !== 'all' ? `${matched} / ${total}` : total }}
      </span>
      <button
        type="button"
        :class="{ active: selecting }"
        :disabled="store.busy"
        title="多选后批量改标签、发布、移动或删除"
        @click="toggleSelecting"
      >
        多选
      </button>
      <button type="button" :disabled="store.busy" title="新建栏目（content/ 下的一层目录）" @click="openCreateSection">
        栏目
      </button>
      <button type="button" class="btn--primary" :disabled="store.busy" @click="openCreate">
        新建
      </button>
    </header>

    <div class="page-list__search">
      <span class="page-list__search-icon" aria-hidden="true">🔍</span>
      <input
        ref="searchBox"
        v-model="keyword"
        type="text"
        placeholder="搜索标题或路径"
        aria-label="搜索内容"
        @keydown.esc.prevent="keyword = ''"
      />
      <button
        v-if="keyword"
        type="button"
        class="page-list__search-clear"
        title="清空搜索（Esc）"
        @click="keyword = ''"
      >
        ×
      </button>
    </div>

    <label class="page-list__filter">
      <span class="page-list__filter-label">只看</span>
      <select
        v-model="filter"
        :class="{ active: filter !== 'all' }"
        aria-label="筛选内容"
        title="缩小列表范围：草稿=还没发布，待生成=改过还没重新生成，待补 SEO=体检提示的那些，定时=日期未到"
      >
        <option
          v-for="item in filters"
          :key="item.id"
          :value="item.id"
          :disabled="item.count === 0 && item.id !== 'all'"
        >
          {{ item.label }}（{{ item.count }}）
        </option>
      </select>
    </label>

    <!-- 栏目候选在多处要用（新建内容、新建栏目、批量移动），放在外层只声明一次 -->
    <datalist id="known-sections">
      <option v-for="section in sections" :key="section" :value="section" />
    </datalist>
    <datalist id="batch-known-tags">
      <option v-for="tag in knownTags" :key="tag" :value="tag" />
    </datalist>

    <div v-if="selecting" class="page-list__batch">
      <p class="page-list__batch-head">
        已选 {{ selected.size }} 篇
        <span class="page-list__spacer" />
        <button type="button" class="page-list__icon" @click="selectVisible">全选当前</button>
        <button type="button" class="page-list__icon" @click="clearSelection">清空</button>
      </p>

      <template v-if="selected.size">
        <div class="page-list__batch-row">
          <input
            v-model="batchTags"
            type="text"
            list="batch-known-tags"
            placeholder="标签，逗号分隔"
            aria-label="批量标签"
          />
          <button
            type="button"
            :disabled="store.busy || !batchTags.trim()"
            title="加到每篇（原有标签保留）"
            @click="applyTags('add')"
          >
            加
          </button>
          <button
            type="button"
            :disabled="store.busy || !batchTags.trim()"
            title="从每篇去掉"
            @click="applyTags('remove')"
          >
            去
          </button>
        </div>

        <div class="page-list__batch-row">
          <input
            v-model="batchSection"
            type="text"
            list="known-sections"
            placeholder="移动到栏目（留空为根目录）"
            aria-label="目标栏目"
          />
          <button type="button" :disabled="store.busy" @click="previewMove">移动…</button>
        </div>
        <label class="page-list__keep">
          <input v-model="batchKeepAliases" type="checkbox" />
          移动后保留旧地址（生成重定向页）
        </label>

        <div class="page-list__batch-row">
          <button type="button" :disabled="store.busy" @click="setDraft(false)">发布</button>
          <button type="button" :disabled="store.busy" @click="setDraft(true)">设为草稿</button>
          <span class="page-list__spacer" />
          <button
            type="button"
            class="page-list__icon"
            title="删除选中的内容，不可撤销"
            @click="previewDelete"
          >
            删除…
          </button>
        </div>

        <!-- 干跑结果：搬动与删除先看清「哪几篇会怎么变」再落盘 -->
        <div v-if="pending" class="page-list__dry">
          <p class="page-list__batch-head">
            {{ pending.kind === 'move' ? '将搬动' : '将删除' }}
            {{ pending.preview.affected }} / {{ pending.preview.changes.length }} 篇
          </p>
          <ul class="page-list__dry-list">
            <li
              v-for="change in pending.preview.changes"
              :key="change.source"
              :class="{ skip: !change.changes }"
            >
              <code>{{ change.source }}</code>
              <span>{{ change.effect }}</span>
            </li>
          </ul>
          <div class="page-list__batch-row">
            <button
              type="button"
              :class="pending.kind === 'delete' ? 'page-list__danger' : 'btn--primary'"
              :disabled="store.busy || pending.preview.affected === 0"
              @click="confirmPending"
            >
              {{ pending.kind === 'move' ? '确认移动' : '确认删除' }}
              {{ pending.preview.affected }} 篇
            </button>
            <button type="button" @click="pending = null">取消</button>
          </div>
        </div>
      </template>
      <p v-else class="page-list__hint">勾选左侧条目，或点「全选当前」。</p>
    </div>

    <!-- 跨文件替换：只改正文，先干跑再落盘 -->
    <form v-if="replacing" class="page-list__new" @submit.prevent="runReplacePreview">
      <h3>跨文件替换</h3>
      <label>
        查找
        <input ref="findBox" v-model="findText" type="text" placeholder="要被换掉的文字" />
      </label>
      <label>
        替换为
        <input v-model="replaceText" type="text" placeholder="留空即删掉这个词" />
      </label>
      <label class="page-list__keep">
        <input v-model="ignoreCase" type="checkbox" />
        忽略大小写
      </label>
      <label class="page-list__keep">
        <input v-model="onlySelected" type="checkbox" :disabled="!selected.size" />
        只改选中的 {{ selected.size }} 篇（不勾就是全站）
      </label>
      <p class="page-list__hint">
        只改正文。标题、标签这些 front matter 字段不会动——那些用「多选」里的批量动作或
        属性面板改。不支持正则。
      </p>

      <div class="page-list__batch-row">
        <button type="submit" :disabled="store.busy || !findText">预览…</button>
        <button type="button" @click="closeReplace">取消</button>
      </div>

      <!-- 干跑结果：正文替换没有撤销，先看清「哪几篇、哪几行」 -->
      <div v-if="replacePreview" class="page-list__dry">
        <p class="page-list__batch-head">
          {{ replacePreview.files.length }} 篇、共 {{ replacePreview.hits }} 处
        </p>
        <p v-if="!replacePreview.hits" class="page-list__hint">没有找到这段文字。</p>
        <ul class="page-list__dry-list">
          <li v-for="file in replacePreview.files" :key="file.source">
            <code>{{ file.source }}</code>
            <span>{{ file.hits }} 处</span>
            <ul class="page-list__dry-lines">
              <li v-for="line in file.lines" :key="line.line">
                <span class="page-list__dry-no">第 {{ line.line }} 行</span>
                <del>{{ line.before }}</del>
                <ins>{{ line.after }}</ins>
              </li>
              <li v-if="file.hits > file.lines.length" class="skip">
                另有 {{ file.hits - file.lines.length }} 处未列出
              </li>
            </ul>
          </li>
          <li v-for="item in replacePreview.skipped" :key="item.source" class="skip">
            <code>{{ item.source }}</code>
            <span>{{ item.reason }}</span>
          </li>
        </ul>
        <div class="page-list__batch-row">
          <button
            type="button"
            class="btn--primary"
            :disabled="store.busy || !replacePreview.hits"
            @click="confirmReplace"
          >
            替换这 {{ replacePreview.hits }} 处
          </button>
        </div>
      </div>
    </form>


    <form v-if="creatingSection" class="page-list__new" @submit.prevent="createSection">
      <h3>新建栏目</h3>
      <label>
        目录名
        <input
          ref="sectionBox"
          v-model="newSectionPath"
          type="text"
          list="known-sections"
          placeholder="notes 或 posts/2026"
        />
      </label>
      <label>
        栏目标题
        <input v-model="newSectionTitle" type="text" placeholder="留空则用目录名" />
      </label>
      <p class="page-list__hint">会同时生成索引页（index.md）——没有它，栏目列表页打不开。</p>
      <div class="page-list__new-actions">
        <button type="submit" class="btn--primary" :disabled="store.busy || !newSectionPath.trim()">
          创建栏目
        </button>
        <button type="button" @click="creatingSection = false">取消</button>
      </div>
    </form>

    <form v-if="creating" class="page-list__new" @submit.prevent="create">
      <h3>新建内容</h3>
      <label>
        标题
        <input ref="titleBox" v-model="newTitle" type="text" placeholder="文章标题" />
      </label>
      <label>
        栏目
        <input
          v-model="newSection"
          type="text"
          list="known-sections"
          placeholder="posts（留空为根目录）"
        />
      </label>
      <div class="page-list__new-actions">
        <button type="submit" class="btn--primary" :disabled="store.busy || !newTitle.trim()">
          创建草稿
        </button>
        <button type="button" @click="creating = false">取消</button>
      </div>
    </form>

    <div v-for="[section, pages] in groups" :key="section" class="page-list__group">
      <h3 @contextmenu="openContextMenu($event, sectionMenu(section))">
        <span class="page-list__section-name">{{ sectionLabel(section) }}</span>
        <span class="page-list__count">{{ pages.length }}</span>
        <span
          v-if="section !== '' && sectionOf.get(section) && !sectionOf.get(section)!.index_source"
          class="badge badge--seo-warn"
          title="没有索引页，栏目地址打不开列表页。新建一篇 slug 为 index 的内容即可"
          >缺列表页</span
        >
        <span class="page-list__spacer" />
        <button
          type="button"
          class="page-list__icon"
          title="栏目信息：标题、简介、排序（存在索引页的 front matter 里）"
          @click="startMeta(section)"
        >
          信息
        </button>
        <template v-if="confirmingSectionDelete === section">
          <button
            type="button"
            class="page-list__danger"
            :disabled="store.busy"
            title="只删空栏目：里面还有文章时会报错"
            @click="removeSection(section)"
          >
            删除栏目
          </button>
          <button
            type="button"
            class="page-list__icon page-list__confirm-cancel"
            @click="confirmingSectionDelete = null"
          >
            取消
          </button>
        </template>
        <button
          v-else
          type="button"
          class="page-list__icon"
          title="更多动作（也可在这一行上右键）"
          aria-label="更多栏目动作"
          @click="openContextMenu($event, sectionMenu(section))"
        >
          ⋯
        </button>
      </h3>

      <form
        v-if="editingMeta === section"
        class="page-list__rename"
        @submit.prevent="submitMeta"
      >
        <label>
          栏目标题
          <input v-model="metaForm.title" type="text" />
        </label>
        <label>
          简介（列表页与 SEO 描述用）
          <input v-model="metaForm.description" type="text" />
        </label>
        <label>
          排序（小的在前，0 表示不排）
          <input v-model.number="metaForm.weight" type="number" />
        </label>
        <p v-if="!sectionOf.get(section)?.index_source" class="page-list__hint">
          这个栏目还没有列表页，保存时会顺手建一张 index.md。
        </p>
        <div class="page-list__new-actions">
          <button type="submit" class="btn--primary" :disabled="store.busy || !metaForm.title.trim()">
            保存
          </button>
          <button type="button" @click="editingMeta = null">取消</button>
        </div>
      </form>

      <form
        v-if="renamingSection === section"
        class="page-list__rename"
        @submit.prevent="submitRename"
      >
        <input v-model="renameTo" type="text" aria-label="新栏目名" />
        <label class="page-list__keep">
          <input v-model="keepAliases" type="checkbox" />
          保留旧地址（生成重定向页）
        </label>
        <div class="page-list__new-actions">
          <button type="submit" class="btn--primary" :disabled="store.busy || !renameTo.trim()">
            改名
          </button>
          <button type="button" @click="renamingSection = null">取消</button>
        </div>
      </form>

      <p v-if="!pages.length" class="page-list__hint">这个栏目还没有文章。</p>
      <ul>
        <li
          v-for="page in pages"
          :key="page.source"
          @contextmenu="openContextMenu($event, pageMenu(page as PageSummary))"
        >
          <input
            v-if="selecting"
            type="checkbox"
            class="page-list__pick"
            :checked="selected.has(page.source)"
            :aria-label="`选择 ${page.title}`"
            @change="toggleOne(page.source)"
          />
          <button
            type="button"
            :class="{ active: store.currentSource === page.source }"
            :title="page.source"
            @click="actions.requestOpenContent(page)"
          >
            <span class="page-list__title">{{ page.title }}</span>
            <span
              v-if="store.currentSource === page.source && isDirty"
              class="badge badge--unsaved"
              title="有未保存改动"
              >●</span
            >
            <span v-else-if="page.draft" class="badge badge--draft">草稿</span>
            <span
              v-else-if="page.scheduled"
              class="badge badge--draft"
              :title="`${page.date ?? ''} 到点后才进产物（站点开了定时发布）`"
              >定时</span
            >
            <!-- 栏目页要标出来：它在这份列表里长得跟文章一样，但日历不收它、
                 「没写日期」清单也不列它。不标的话，用户只会得出「界面漏了一篇」 -->
            <span
              v-if="page.is_index"
              class="badge badge--section"
              title="栏目列表页：这个栏目的门面，不算「发出去的一篇」，不参与发布节奏与排期"
              >栏目页</span
            >
            <span
              v-if="seoBySource.get(page.source)"
              class="badge"
              :class="`badge--seo-${seoBySource.get(page.source)!.severity}`"
              :title="seoBySource.get(page.source)!.messages.join('\n')"
              >SEO</span
            >
            <span
              v-if="dirtyPages.has(page.source)"
              class="badge badge--dirty"
              title="待重新生成"
              >●</span
            >
          </button>
          <template v-if="confirmingDelete === page.source">
            <button
              type="button"
              class="page-list__danger"
              :disabled="store.busy"
              title="删除源文件，产物在下次生成时清理"
              @click="remove(page)"
            >
              删除
            </button>
            <button
              type="button"
              class="page-list__icon page-list__confirm-cancel"
              @click="confirmingDelete = null"
            >
              取消
            </button>
          </template>
          <button
            v-else
            type="button"
            class="page-list__icon"
            title="更多动作（也可在这一行上右键）"
            aria-label="更多动作"
            @click="openContextMenu($event, pageMenu(page as PageSummary))"
          >
            ⋯
          </button>
        </li>
      </ul>
    </div>

    <!-- 没有产物时不整组隐藏（见 ui-design.md 6.9）：隐藏让人以为没有这个功能。
         但全新空站点（还没有任何文章）不提这一段，那时该先写第一篇 -->
    <div
      v-if="filter === 'all' && (sitePages.length > 0 || (!normalized && total > 0))"
      class="page-list__group"
    >
      <h3>
        站点页面（生成）
        <span v-if="sitePages.length" class="page-list__count">{{ sitePages.length }}</span>
      </h3>
      <ul v-if="sitePages.length">
        <li v-for="item in sitePages" :key="item.path">
          <button
            type="button"
            :class="{ active: store.previewTarget === item.url }"
            :title="`${item.path} · 点击用本地服务器预览`"
            @click="actions.previewOutput(item.url)"
          >
            <span class="page-list__title">{{ item.url }}</span>
            <span class="badge badge--kind">{{ outputKindLabel(item.kind) }}</span>
          </button>
        </li>
      </ul>
      <p v-else class="empty-hint">
        还没生成过，所以标签页与分页页还不存在。按 <kbd>Ctrl+Enter</kbd> 生成一次，
        它们会出现在这里，点一下用本地服务器预览。
      </p>
    </div>

    <!-- 正文命中：同一个搜索框的第二档结果，标题里没这个词但正文里有。
         已经在上面列出过的文章不重复出现 -->
    <div v-if="normalized.length >= 2 && (extraHits.length > 0 || searching)" class="page-list__group">
      <h3>
        正文里提到
        <span v-if="extraHits.length" class="page-list__count">{{ extraHits.length }}</span>
      </h3>
      <p v-if="searching" class="empty-hint">正在搜正文…</p>
      <ul v-else>
        <li v-for="hit in extraHits" :key="hit.source">
          <button
            type="button"
            :class="{ active: store.currentSource === hit.source }"
            :title="hit.source"
            @click="openHit(hit)"
          >
            <span class="page-list__title">{{ hit.title || hit.source }}</span>
          </button>
          <p class="page-list__snippet">{{ hit.snippet }}</p>
        </li>
      </ul>
    </div>

    <p
      v-if="normalized && matched === 0 && extraHits.length === 0 && !searching && !sitePages.length"
      class="page-list__empty"
    >
      没有匹配「{{ keyword }}」的内容
    </p>
    <p v-else-if="!normalized && filter !== 'all' && matched === 0" class="page-list__empty">
      这一类现在是空的。
    </p>
    <p v-else-if="!total" class="page-list__empty">还没有内容，点右上角「新建」写第一篇。</p>
  </nav>
</template>
