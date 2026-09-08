<script setup lang="ts">
/**
 * 内容侧栏：按栏目分组列出内容页，支持搜索、新建、删除，并标出待重新生成的页面。
 *
 * 布局按「这是什么 → 找什么 → 有什么」三段组织：
 * 标题行说明这一栏是内容并给出新建入口，搜索行只负责过滤，剩下才是列表。
 * 之前搜索框与「＋」并排且没有任何标识，很容易被当成「新建内容的名称输入框」。
 */
import { computed, onBeforeUnmount, onMounted, reactive, ref, watch } from 'vue'

import { actions, isDirty, store } from '../store'
import { parseList } from '../text'
import { ui } from '../ui'
import type { BatchPreview, PageSummary, SeoSeverity } from '../api'

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

/** 筛选项自带计数：为空的筛选还摆在那里只会让人点一下才发现没有。 */
const filters = computed<Array<{ id: Filter; label: string; count: number }>>(() => {
  const pages = store.project?.pages ?? []
  return [
    { id: 'all', label: '全部', count: pages.length },
    { id: 'draft', label: '草稿', count: pages.filter((p) => p.draft).length },
    {
      id: 'dirty',
      label: '待生成',
      count: pages.filter((p) => dirtyPages.value.has(p.source)).length,
    },
    {
      id: 'seo',
      label: '待补 SEO',
      count: pages.filter((p) => seoBySource.value.has(p.source)).length,
    },
    { id: 'scheduled', label: '定时', count: pages.filter((p) => p.scheduled).length },
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

/** Ctrl/Cmd+F 聚焦搜索框，Esc 清空——两者都是列表界面的通用预期。 */
function onKeydown(event: KeyboardEvent) {
  if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'f') {
    event.preventDefault()
    searchBox.value?.focus()
    searchBox.value?.select()
  }
}

onMounted(() => window.addEventListener('keydown', onKeydown))
onBeforeUnmount(() => window.removeEventListener('keydown', onKeydown))

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
    requestAnimationFrame(() => searchBox.value?.focus())
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

const renamingSection = ref<string | null>(null)
const renameTo = ref('')
const keepAliases = ref(true)

function startRename(path: string) {
  renamingSection.value = path
  renameTo.value = path
  confirmingSectionDelete.value = null
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

    <div class="page-list__filters">
      <button
        v-for="item in filters"
        :key="item.id"
        type="button"
        class="page-list__chip"
        :class="{ active: filter === item.id }"
        :disabled="item.count === 0 && item.id !== 'all'"
        @click="filter = item.id"
      >
        {{ item.label }} <span class="page-list__count">{{ item.count }}</span>
      </button>
    </div>

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
      <h3>
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
        <template v-if="section !== ''">
          <button
            type="button"
            class="page-list__icon"
            title="栏目改名（默认保留旧地址）"
            @click="startRename(section)"
          >
            改名
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
            <button type="button" class="page-list__icon" @click="confirmingSectionDelete = null">
              取消
            </button>
          </template>
          <button
            v-else
            type="button"
            class="page-list__icon"
            title="删除空栏目"
            @click="confirmingSectionDelete = section"
          >
            ×
          </button>
        </template>
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
        <li v-for="page in pages" :key="page.source">
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
            <button type="button" class="page-list__icon" @click="confirmingDelete = null">
              取消
            </button>
          </template>
          <button
            v-else
            type="button"
            class="page-list__icon"
            title="删除这篇内容"
            @click="confirmingDelete = page.source"
          >
            ×
          </button>
        </li>
      </ul>
    </div>

    <div v-if="filter === 'all' && sitePages.length" class="page-list__group">
      <h3>站点页面（生成）<span class="page-list__count">{{ sitePages.length }}</span></h3>
      <ul>
        <li v-for="item in sitePages" :key="item.path">
          <button
            type="button"
            :class="{ active: store.previewTarget === item.url }"
            :title="`${item.path} · 点击用本地服务器预览`"
            @click="actions.previewOutput(item.url)"
          >
            <span class="page-list__title">{{ item.url }}</span>
            <span class="badge badge--draft">{{
              item.kind === 'taxonomy' ? '标签' : '分页'
            }}</span>
          </button>
        </li>
      </ul>
    </div>

    <p v-if="normalized && matched === 0 && !sitePages.length" class="page-list__empty">
      没有匹配「{{ keyword }}」的内容
    </p>
    <p v-else-if="!normalized && filter !== 'all' && matched === 0" class="page-list__empty">
      这一类现在是空的。
    </p>
    <p v-else-if="!total" class="page-list__empty">还没有内容，点右上角「新建」写第一篇。</p>
  </nav>
</template>
