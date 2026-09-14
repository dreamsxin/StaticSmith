<script setup lang="ts">
/**
 * 侧栏里一个栏目的头部：名字、篇数、以及栏目级的三件事（信息 / 改名 / 删空栏目）。
 *
 * 从 `PageList.vue` 抽出来的。那个文件曾经把搜索、筛选、多选批量、跨文件替换、
 * 新建、栏目管理、右键菜单挤在一处，改一处要通读全文；而栏目这一块与其余部分
 * **完全解耦**（不碰筛选、选中集合、搜索结果），是最干净的一刀。抽出来的直接好处是
 * 它现在测得动：改名的确认态有三个出口（确认 / 取消 / 干跑被拒），
 * 上一次我在同构的地方漏掉一个就写出了真 bug。
 *
 * 分组容器 `.page-list__group` 与下面的文章列表**留在** `PageList` 里：
 * 全局样式里有 `.page-list__group h3 .page-list__icon { opacity: 0 }` 这类后代选择器
 * （hover 才显形），根节点跑出那层容器，栏目按钮就会一直亮着。
 */
import { computed, reactive, ref } from 'vue'

import type { SectionRenamePreview } from '../api'
import { openContextMenu, type MenuEntry } from '../commands'
import { focusSelector } from '../focus'
import { actions, store } from '../store'

/**
 * 栏目元信息里这块界面用得上的部分。
 *
 * 不直接用 `Section`：`store.sections` 是深只读的（`reactive` + `readonly`），
 * 传 `Section` 会在传参处被类型拒掉（`children: readonly string[]` 对不上 `string[]`）。
 * 与 `crossref.ts` 的 `LinkTarget` 同一套做法——只声明用得上的字段，且都是 `readonly`。
 */
interface SectionMetaView {
  readonly title: string
  readonly description: string
  readonly weight: number
  readonly index_source: string | null
  readonly pages: number
}

const props = defineProps<{
  /** 栏目路径，根目录是空串。 */
  section: string
  /** 这个栏目直属的文章数，显示在名字后面。 */
  count: number
  /** 栏目元信息。没有索引页的栏目取不到，界面要能容忍它缺席。 */
  meta?: SectionMetaView
}>()

const emit = defineEmits<{
  /** 「在此栏目新建文章…」：新建表单在 PageList 那边，这里只把意图递出去。 */
  newContent: [section: string]
}>()

const label = computed(() => (props.section === '' ? '根目录' : props.section))

// ---------------------------------------------------------------- 改名

const renaming = ref(false)
const renameTo = ref('')
const keepAliases = ref(true)

/** 待确认的改名：用户填的新名字 + 干跑结果。 */
const pendingRename = ref<{ to: string; preview: SectionRenamePreview } | null>(null)

/** 会被改写的引用总处数，确认那一句要用。 */
const pendingHits = computed(() =>
  (pendingRename.value?.preview.refs ?? []).reduce((sum, item) => sum + item.hits, 0),
)

function startRename() {
  renaming.value = true
  renameTo.value = props.section
  editingMeta.value = false
  confirmingDelete.value = false
  focusSelector('.page-list__rename input')
}

/**
 * 改名先干跑再问一句。
 *
 * 栏目改名一次动整棵子树的地址：搬文件、补旧地址、改写站内引用，其中最后一件
 * 会写到用户没有点名的文件上。干跑被拦下时（同名、目标已存在、栏目不存在）
 * 错误已由 `run` 弹出，这里只收起表单，不进确认态。
 */
async function submitRename() {
  const to = renameTo.value.trim()
  if (!to || to === props.section) {
    renaming.value = false
    return
  }
  const preview = await actions.previewRenameSection(props.section, to, keepAliases.value)
  if (!preview) {
    renaming.value = false
    return
  }
  pendingRename.value = { to, preview }
}

async function confirmRename() {
  const pending = pendingRename.value
  pendingRename.value = null
  renaming.value = false
  if (!pending) return
  await actions.renameSection(props.section, pending.to, keepAliases.value)
}

function cancelRename() {
  pendingRename.value = null
  renaming.value = false
}

// ---------------------------------------------------------------- 栏目信息

/**
 * 栏目元信息：标题、简介、排序权重。
 *
 * 这三项都存在索引页的 front matter 里——栏目就是目录，它的介绍该在那张列表页上，
 * 而不是另开一个栏目配置文件。缺索引页的栏目保存时会顺手补一张。
 */
const editingMeta = ref(false)
const metaForm = reactive({ title: '', description: '', weight: 0 })

function startMeta() {
  metaForm.title = props.meta?.title ?? label.value
  metaForm.description = props.meta?.description ?? ''
  metaForm.weight = props.meta?.weight ?? 0
  editingMeta.value = true
  renaming.value = false
  confirmingDelete.value = false
  focusSelector('.page-list__rename input')
}

async function submitMeta() {
  if (!metaForm.title.trim()) return
  editingMeta.value = false
  await actions.saveSectionMeta(props.section, {
    title: metaForm.title.trim(),
    description: metaForm.description.trim(),
    weight: metaForm.weight,
  })
}

// ---------------------------------------------------------------- 删除

/** 删空栏目也是不可逆的，沿用列表里的就地确认，不用原生弹窗。 */
const confirmingDelete = ref(false)

async function removeSection() {
  confirmingDelete.value = false
  await actions.removeSection(props.section)
}

/** 分组头的右键菜单：栏目级动作。根目录不能改名也不能删，那两项直接不出现。 */
function menu(): MenuEntry[] {
  const items: MenuEntry[] = [
    {
      id: 'section.new',
      label: '在此栏目新建文章…',
      run: () => emit('newContent', props.section),
    },
    { separator: true },
    {
      id: 'section.meta',
      label: '栏目信息（标题 / 简介 / 排序）…',
      run: () => startMeta(),
    },
  ]
  if (props.section === '') return items
  const pages = props.meta?.pages ?? 0
  items.push(
    {
      id: 'section.rename',
      label: '栏目改名…',
      hint: '默认保留旧地址',
      run: () => startRename(),
    },
    { separator: true },
    {
      id: 'section.remove',
      label: '删除空栏目…',
      hint: pages > 0 ? '里面还有文章' : '还要再确认一次',
      danger: true,
      disabled: pages > 0,
      run: () => {
        confirmingDelete.value = true
        focusSelector('.page-list__confirm-cancel')
      },
    },
  )
  return items
}
</script>

<template>
  <h3 @contextmenu="openContextMenu($event, menu())">
    <span class="page-list__section-name">{{ label }}</span>
    <span class="page-list__count">{{ props.count }}</span>
    <span
      v-if="props.section !== '' && props.meta && !props.meta.index_source"
      class="badge badge--seo-warn"
      title="没有索引页，栏目地址打不开列表页。新建一篇 slug 为 index 的内容即可"
      >缺列表页</span
    >
    <span class="page-list__spacer" />
    <button
      type="button"
      class="page-list__icon"
      title="栏目信息：标题、简介、排序（存在索引页的 front matter 里）"
      @click="startMeta"
    >
      信息
    </button>
    <template v-if="confirmingDelete">
      <button
        type="button"
        class="page-list__danger"
        :disabled="store.busy"
        title="只删空栏目：里面还有文章时会报错"
        @click="removeSection"
      >
        删除栏目
      </button>
      <button
        type="button"
        class="page-list__icon page-list__confirm-cancel"
        @click="confirmingDelete = false"
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
      @click="openContextMenu($event, menu())"
    >
      ⋯
    </button>
  </h3>

  <form v-if="editingMeta" class="page-list__rename" @submit.prevent="submitMeta">
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
    <p v-if="!props.meta?.index_source" class="page-list__hint">
      这个栏目还没有列表页，保存时会顺手建一张 index.md。
    </p>
    <div class="page-list__new-actions">
      <button type="submit" class="btn--primary" :disabled="store.busy || !metaForm.title.trim()">
        保存
      </button>
      <button type="button" @click="editingMeta = false">取消</button>
    </div>
  </form>

  <form v-if="renaming" class="page-list__rename" @submit.prevent="submitRename">
    <input v-model="renameTo" type="text" aria-label="新栏目名" />
    <label class="page-list__keep">
      <input v-model="keepAliases" type="checkbox" />
      保留旧地址（生成重定向页）
    </label>
    <!-- 干跑结果：改名会写到用户没有点名的文件上（那些引用这个栏目的文章） -->
    <div v-if="pendingRename" class="page-list__dry">
      <p class="page-list__batch-head">
        将把 <code>{{ props.section }}</code> 改名为 <code>{{ pendingRename.to }}</code>
      </p>
      <ul class="page-list__dry-list">
        <li>
          <span>搬动 {{ pendingRename.preview.files }} 个文件</span>
        </li>
        <li v-if="pendingRename.preview.aliases">
          <span>给 {{ pendingRename.preview.aliases }} 篇补旧地址（构建后是重定向页）</span>
        </li>
      </ul>
      <p v-if="pendingHits" class="page-list__batch-note">
        另会把 {{ pendingRename.preview.refs.length }} 篇里的 {{ pendingHits }}
        处站内链接改到新地址：{{ pendingRename.preview.refs.map((r) => r.source).join('、') }}
      </p>
      <div class="page-list__batch-row">
        <button type="button" class="btn--primary" :disabled="store.busy" @click="confirmRename">
          确认改名
        </button>
        <button type="button" @click="cancelRename">取消</button>
      </div>
    </div>
    <div v-else class="page-list__new-actions">
      <button type="submit" class="btn--primary" :disabled="store.busy || !renameTo.trim()">
        改名…
      </button>
      <button type="button" @click="cancelRename">取消</button>
    </div>
  </form>
</template>
