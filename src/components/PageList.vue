<script setup lang="ts">
/**
 * 内容侧栏：按栏目分组列出内容页，支持搜索、新建、删除，并标出待重新生成的页面。
 *
 * 布局按「这是什么 → 找什么 → 有什么」三段组织：
 * 标题行说明这一栏是内容并给出新建入口，搜索行只负责过滤，剩下才是列表。
 * 之前搜索框与「＋」并排且没有任何标识，很容易被当成「新建内容的名称输入框」。
 */
import { computed, ref, watch } from 'vue'

import SectionHeader from './SectionHeader.vue'
import ReplacePanel from './ReplacePanel.vue'
import CreatePanel from './CreatePanel.vue'
import BrokenList from './BrokenList.vue'
import PageRow from './PageRow.vue'
import BatchBar from './BatchBar.vue'



import { openContextMenu, type MenuEntry } from '../commands'
import { focusSelector } from '../focus'
import { actions, store } from '../store'

import { ui } from '../ui'
import { searchContent } from '../api'
import { outputKindLabel } from '../labels'
import { parseOutline, peerIndex } from '../manuscript'
import {
  canDropBeside,
  canDropIntoSection,
  groupBySection,
  moved,
  movedSection,
  siblingsOf,
  worstSeoBySource,
} from '../grouping'
import type { Criteria, Filter, MovablePage } from '../grouping'
import type { PageSummary, SearchHit, BatchPreview } from '../api'


const keyword = ref('')
const searchBox = ref<HTMLInputElement | null>(null)

/**
 * 运营视角的筛选。
 *
 * 体检面板能告诉你「有 12 篇缺描述」，但补的时候还是要回到列表里一篇篇找。
 * 这一排筛选把体检结论接回工作列表：选「待补 SEO」就只剩要动的那些。
 *
 * 筛选、分组、排序那套判断在 `src/grouping.ts` 里（纯逻辑，单独测）：
 * 一篇文章因为条件写错而不出现在列表里，界面不会报任何错，用户只会以为它丢了。
 */
const filter = ref<Filter>('all')

const normalized = computed(() => keyword.value.trim().toLowerCase())

/** 每篇文章最严重的那条 SEO 问题。站点级问题没有 source，自然被排除。 */
const seoBySource = computed(() => worstSeoBySource(store.seo?.issues ?? []))

const dirtyPages = computed(() => new Set(store.plan?.pages ?? []))

const criteria = computed<Criteria>(() => ({
  keyword: keyword.value,
  filter: filter.value,
  dirty: dirtyPages.value,
  seo: new Set(seoBySource.value.keys()),
}))

const groups = computed(() =>
  groupBySection(store.project?.pages ?? [], store.sections, criteria.value),
)

/** 栏目元信息（有没有索引页、直属篇数）按路径取用。 */
const sectionOf = computed(() => new Map(store.sections.map((s) => [s.path, s])))




const total = computed(() => store.project?.pages.length ?? 0)
const matched = computed(() => groups.value.reduce((sum, group) => sum + group.pages.length, 0))

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
  const shown = new Set(groups.value.flatMap((group) => group.pages.map((p) => p.source)))
  return bodyHits.value.filter((hit) => hit.field === 'body' && !shown.has(hit.source))
})

/** 从一条命中回到那篇文章。带未保存改动时由 store 拦一道，这里不重复判断。 */
async function openHit(hit: SearchHit) {
  const page = (store.project?.pages ?? []).find((p) => p.source === hit.source)
  if (page) await actions.requestOpenContent(page as PageSummary)
}


// ---------------------------------------------------------------- 新建

/**
 * 三块可展开的表单（新建内容、新建栏目、跨文件替换）共用侧栏顶部这一块位置，
 * 所以共用一个状态：互斥成了结构上的事实。
 *
 * 之前是三个布尔量各自 `false` 来 `false` 去，而「点『新建』时收起『新建栏目』」那一笔
 * 恰好漏了——两张表单会叠在一起，还同时绑同一份 datalist。
 */
type Panel = 'content' | 'section' | 'replace'
const panel = ref<Panel | null>(null)
const createPanel = ref<{ focusFirst: () => Promise<void> } | null>(null)

/** 新建内容时预填的栏目：栏目头的「在此栏目新建文章…」把意图递到这里。 */
const newContentSection = ref('posts')

function openPanel(next: Panel) {
  // 已经展开着又被叫一次（菜单里的「新建文章…」、Ctrl+N）：至少把光标送回第一格，
  // 否则按下去界面毫无反应，看起来像坏了
  if (panel.value === next) void createPanel.value?.focusFirst()
  panel.value = next
}

/**
 * 栏目头的「在此栏目新建文章…」：新建表单在 `CreatePanel` 里，栏目组件只把意图递过来。
 *
 * 把栏目那一块抽成组件之后，这是两者之间唯一的一条线——其余（改名、元信息、删除）
 * 组件自己调 actions 就够了。
 */
function startNewContentIn(section: string) {
  newContentSection.value = section
  openPanel('content')
}



/** 已有栏目做候选，避免同一个栏目写出 post / posts 两种。 */
const sections = computed(() => {
  const set = new Set<string>()
  for (const page of store.project?.pages ?? []) if (page.section) set.add(page.section)
  return [...set].sort()
})

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
    openPanel('content')
  },
  { immediate: true },
)

watch(
  () => ui.requestNewSection,
  (asked) => {
    if (!asked) return
    ui.requestNewSection = false
    openPanel('section')
  },
  { immediate: true },
)

watch(
  () => ui.requestReplace,
  (asked) => {
    if (!asked) return
    ui.requestReplace = false
    openPanel('replace')
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
  for (const group of groups.value) for (const page of group.pages) next.add(page.source)
  selected.value = next
}

const selectedList = computed(() => [...selected.value])

/** 批量加标签时给出已有标签，避免同一个概念写出三种写法。 */
const knownTags = computed(() =>
  [...new Set((store.project?.pages ?? []).flatMap((p) => p.tags))].sort((a, b) =>
    a.localeCompare(b),
  ),
)

/** 每次批量动作后清空选择：文件可能已经改名、搬走或删掉，旧的选中集没有意义。 */
function clearSelection() {
  selected.value = new Set()
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
      id: 'page.up',
      label: '上移一位',
      hint: '这一栏的顺序，与网站列表页一致',
      disabled: !canMove(page, -1),
      run: () => actions.movePage(page.source, -1),
    },
    {
      id: 'page.down',
      label: '下移一位',
      hint: '这一栏的顺序，与网站列表页一致',
      disabled: !canMove(page, 1),
      run: () => actions.movePage(page.source, 1),
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
        focusSelector('.page-list__confirm-cancel')
      },
    },
  ]
}

/**
 * 这一篇在它那一栏里还能不能往那个方向挪。
 *
 * 判断与真正的挪动共用 `moved`：两处各算一遍，迟早出现「菜单能点、点了没反应」。
 * 注意比的是**这一栏的全部文章**，不是列表上看到的那些——筛选或搜索时列表只剩几行，
 * 而顺序是整栏的事。
 */
function canMove(page: PageSummary, delta: -1 | 1): boolean {
  const siblings = (store.project?.pages ?? []).filter(
    (p) => p.section === page.section && !p.is_index,
  )
  return moved(siblings, page.source, delta) !== null
}

/**
 * 这个栏目在它那一层里还能不能往那个方向挪。
 *
 * 与文章那条路同构（判断与执行共用一份逻辑）。比的是**整层的栏目**，
 * 而不是列表上看到的那几组：筛选或搜索会把没命中的栏目整组去掉，而顺序是整层的事。
 */
function canMoveSection(section: string, delta: -1 | 1): boolean {
  return movedSection(siblingsOf(store.sections, section), section, delta) !== null
}

// ---------------------------------------------------------------- 拖拽排序

/**
 * 正在编辑的那一篇的篇内标题。
 *
 * 侧栏因此同时是「整本书的目录」与「这一章的细目」——Word 的导航窗格把整份文档的标题
 * 摊成一列，而这里一本书是许多文件，全摊开要读遍整个 `content/`。折中是只展开手上这一篇：
 * 它的正文已经在内存里（`store.currentRaw`），所以**边写边长**，连保存都不用等。
 *
 * 不新增一栏也是有意的：编辑器已经三栏，再挤一栏正文就没宽度了（同 `docs/ui.md`
 * 里「大纲做成浮层而不是常驻侧栏」的理由）。
 */
const currentOutline = computed(() => {
  const headings = parseOutline(store.currentRaw)
  // 能不能挪只看标题表（`peerIndex`），不去试算整篇重排：这一段每敲一个字都要重算。
  return headings.map((heading, at) => ({
    ...heading,
    canUp: peerIndex(headings, at, -1) >= 0,
    canDown: peerIndex(headings, at, 1) >= 0,
  }))
})


/**
 * 正被拖着的那一篇，以及落点。
 *
 * 两种落点，两件不同的事：
 * - 落在**一行**上（`dropTarget`）= 同栏目内排序，直接落盘（顺序是可逆的）
 * - 落在**栏目**上（`dropSection`）= 改归属，先干跑再确认（要补旧地址、
 *   还要改写别人文章里指向它的链接，背着人改文件比不改更糟）
 *
 * 跨栏目**不**接「插到某一行前后」：那一次拖拽同时改归属和顺序，
 * 一句话说不清会发生什么，而这个工具没有跨文件撤销栈。
 */
const dragging = ref<string | null>(null)
const dropTarget = ref<{ source: string; side: 'before' | 'after' } | null>(null)
const dropSection = ref<string | null>(null)
/** 拖到栏目上之后的干跑结果，确认前不落盘。 */
const pendingMove = ref<{
  source: string
  section: string
  preview: BatchPreview
} | null>(null)

function draggedPage(): MovablePage | undefined {
  return store.project?.pages.find((p) => p.source === dragging.value)
}

function hoverRow(page: PageSummary, side: 'before' | 'after') {
  const from = draggedPage()
  dropTarget.value = canDropBeside(from, page) ? { source: page.source, side } : null
  // 行在栏目里面，`dragover` 会冒泡到栏目上：进了某一行就别再高亮整个栏目，
  // 否则「插到这一行」和「搬进这个栏目」两个提示会同时亮着
  if (dropTarget.value) dropSection.value = null
}

async function dropOnRow(page: PageSummary) {
  const target = dropTarget.value
  const source = dragging.value
  endDrag()
  // 落点与松手的那一行必须是同一行：中途拖到别的栏目上时 `dropTarget` 已经清空
  if (!source || target?.source !== page.source) return
  await actions.movePageTo(source, target.source, target.side)
}

/** 拖过某个栏目：只有真能搬进去时才给落点态，不然不给任何提示。 */
function hoverSection(section: string) {
  dropSection.value = canDropIntoSection(draggedPage(), section) ? section : null
}

/**
 * 松手在栏目上：**不落盘**，先问 core「会发生什么」，把清单摆出来等确认。
 *
 * 干跑与执行都走批量那条路（`batchPreview` / `batchMove`，单元素数组）：
 * 一处定义，也就没有「拖拽搬动和批量搬动行为不一致」的可能。
 */
async function dropOnSection(section: string) {
  const source = dragging.value
  const target = dropSection.value
  endDrag()
  if (!source || target !== section) return
  const preview = await actions.batchPreview([source], { kind: 'move', to_section: section })
  if (!preview) return
  pendingMove.value = { source, section, preview }
}

/** 确认后才写盘。保留旧地址（生成重定向页）——搬走一篇不该让别人的链接变死链。 */
async function confirmMove() {
  const pending = pendingMove.value
  if (!pending) return
  pendingMove.value = null
  await actions.batchMove([pending.source], pending.section, true)
}

function endDrag() {
  dragging.value = null
  dropTarget.value = null
  dropSection.value = null
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
      <button type="button" :disabled="store.busy" title="新建栏目（content/ 下的一层目录）" @click="openPanel('section')">
        栏目
      </button>
      <button type="button" class="btn--primary" :disabled="store.busy" @click="openPanel('content')">
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


    <BatchBar
      v-if="selecting"
      :selected="selectedList"
      :known-tags="knownTags"
      @select-visible="selectVisible"
      @clear="clearSelection"
    />


    <!-- 跨文件替换：只改正文，先干跑再落盘（表单在 ReplacePanel.vue 里） -->
    <ReplacePanel :open="panel === 'replace'" :selected="selectedList" @close="panel = null" />

    <!-- 新建内容 / 新建栏目：两张表单在 CreatePanel.vue 里，互斥由 panel 一个状态表达 -->
    <CreatePanel
      ref="createPanel"
      :mode="panel === 'content' || panel === 'section' ? panel : null"
      :default-section="newContentSection"
      @close="panel = null"
    />


    <!-- 读不出来的源文件摆在最前：生成会因为它被拦下，是要先处理的事 -->
    <BrokenList :items="store.project?.broken_sources ?? []" />

    <!-- 栏目按树排（子栏目紧跟父栏目），缩进用 --depth 表达：层数不定，
         写成 --depth-1/2/3 这类固定类名迟早不够用。
         整个分组是拖放目标：把一篇拖到别的栏目上 = 改归属，松手先弹干跑 -->
    <div
      v-for="group in groups"
      :key="group.section"
      class="page-list__group"
      :class="{
        'page-list__group--nested': group.depth > 0,
        'page-list__group--drop': dropSection === group.section,
      }"
      :style="{ '--depth': group.depth }"
      @dragover.prevent="hoverSection(group.section)"
      @dragleave="dropSection = null"
      @drop.prevent="dropOnSection(group.section)"
    >

      <!-- 干跑结果：改归属要补旧地址、还要改写别人文章里指向它的链接，先说清再落盘 -->
      <div v-if="pendingMove?.section === group.section" class="page-list__dry">
        <p class="page-list__batch-head">
          将把 <code>{{ pendingMove.source }}</code> 搬进
          <code>{{ pendingMove.section }}</code>
        </p>
        <ul class="page-list__dry-list">
          <li
            v-for="change in pendingMove.preview.changes"
            :key="change.source"
            :class="{ skip: !change.changes }"
          >
            <code>{{ change.source }}</code>
            <span>{{ change.effect }}</span>
          </li>
        </ul>
        <p v-if="pendingMove.preview.refs.length" class="page-list__batch-note">
          另会把 {{ pendingMove.preview.refs.length }} 篇里的
          {{ pendingMove.preview.refs.reduce((sum, item) => sum + item.hits, 0) }}
          处站内链接改到新地址：{{
            pendingMove.preview.refs.map((item) => item.source).join('、')
          }}
        </p>
        <p v-if="pendingMove.preview.refs_manual.length" class="page-list__batch-note">
          另有 {{ pendingMove.preview.refs_manual.length }} 篇里的
          {{ pendingMove.preview.refs_manual.reduce((sum, item) => sum + item.hits, 0) }}
          处<strong>相对链接</strong>（<code>../a/</code> 这类）指向它，改写不到，需要手工改：{{
            pendingMove.preview.refs_manual.map((item) => item.source).join('、')
          }}
        </p>
        <div class="page-list__batch-row">
          <button
            type="button"
            class="btn--primary"
            :disabled="store.busy || pendingMove.preview.affected === 0"
            @click="confirmMove"
          >
            确认搬动
          </button>
          <button type="button" @click="pendingMove = null">取消</button>
        </div>
      </div>

      <SectionHeader
        :section="group.section"
        :count="group.pages.length"
        :nested="group.parentShown"
        :can-up="canMoveSection(group.section, -1)"
        :can-down="canMoveSection(group.section, 1)"
        :meta="sectionOf.get(group.section)"
        @new-content="startNewContentIn"
      />

      <p v-if="!group.pages.length" class="page-list__hint">这个栏目还没有文章。</p>
      <ul>
        <!-- 行内的样子在 PageRow.vue 里；哪一行处于确认态、菜单里放什么留在这里 -->
        <PageRow
          v-for="page in group.pages"
          :key="page.source"
          :page="page"
          :selecting="selecting"
          :checked="selected.has(page.source)"
          :dirty="dirtyPages.has(page.source)"
          :seo="seoBySource.get(page.source) ?? null"
          :confirming="confirmingDelete === page.source"
          :dragging="dragging === page.source"
          :drop-at="dropTarget?.source === page.source ? dropTarget.side : null"
          :outline="page.source === store.currentSource ? currentOutline : undefined"
          @open="actions.requestOpenContent(page)"
          @toggle="toggleOne(page.source)"
          @menu="openContextMenu($event, pageMenu(page as PageSummary))"
          @remove="remove(page as PageSummary)"
          @cancel="confirmingDelete = null"
          @dragstart="dragging = page.source"
          @dragover="(side) => hoverRow(page as PageSummary, side)"
          @drop="dropOnRow(page as PageSummary)"
          @dragend="endDrag"
          @jump="(offset) => ui.editor?.jumpTo(offset)"
          @move-heading="(offset, delta) => ui.editor?.moveHeading(offset, delta)"
        />
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
