<script setup lang="ts">
/**
 * 「回退内容」对话框。
 *
 * 安全网原先只对 Agent 完整：MCP 有 `list_snapshots` / `restore_snapshot`，
 * 而界面上删了一批文章之后，人只能去翻 `.staticsmith/history.git`。
 * 快照本来就是**为界面上的不可逆操作**留的，入口不该只有 Agent 拿得到。
 *
 * 两步确认在对话框内部完成：选中一条 → 出现这条会做什么的说明 → 确认。
 * 不用系统 confirm：一是它在 Tauri 里样式与应用无关，二是回退需要解释
 * 「产物不动」「未保存的改动会作废」，一行提示框放不下。
 *
 * 浮层行为照 `CommandPalette` / `NewSiteDialog`：背景遮罩、点空白关闭、Esc 关闭、
 * 关闭后把焦点还给打开它的元素。
 *
 * 键盘处理的位置是有讲究的，见 `move` 上面那段注释——上下键与回车挂在
 * **列表**上而不是外框上。
 */
import { computed, nextTick, ref, watch } from 'vue'

import * as api from '../api'
import { actions, isDirty, isTemplateDirty, store } from '../store'
import { snapshotLabel } from '../text'

const props = defineProps<{ open: boolean }>()
const emit = defineEmits<{ close: [] }>()

/** 选中待确认的那一条。null 表示还在浏览列表。 */
const picked = ref<string | null>(null)
/** 键盘高亮到第几条（不是选中，选中要按回车）。 */
const active = ref(0)

const list = ref<HTMLElement | null>(null)
const confirmButton = ref<HTMLButtonElement | null>(null)

let restoreFocus: HTMLElement | null = null

const target = computed(() => store.snapshots.find((item) => item.id === picked.value) ?? null)

/** 清单被上限截断了：更早的快照还在仓库里，但这里看不到。 */
const truncated = computed(() => store.snapshots.length >= api.SNAPSHOT_LIMIT)

/** 当前高亮项的 DOM id，给 `aria-activedescendant` 用：读屏器据此念出选中的那一条。 */
const activeId = computed(() => {
  const item = store.snapshots[active.value]
  return item ? `snapshot-${item.id}` : undefined
})

/**
 * 未保存的改动会被回退作废，要说清是哪一种。
 *
 * 模板也在跟踪范围里，所以这里不能只看文章：模板的脏改动一旦被覆盖，
 * 下一次保存会把回退掉的那份写回去。
 */
const unsaved = computed(() => {
  if (isDirty.value && isTemplateDirty.value) return '文章与模板'
  if (isDirty.value) return '文章'
  if (isTemplateDirty.value) return '模板'
  return ''
})

watch(
  () => props.open,
  async (open) => {
    if (!open) {
      picked.value = null
      restoreFocus?.focus()
      restoreFocus = null
      return
    }
    const before = document.activeElement
    restoreFocus = before instanceof HTMLElement ? before : null
    active.value = 0
    // 每次打开都重新拉：期间可能又做过几次破坏性操作，也可能 Agent 动过。
    await actions.loadSnapshots()
    await focusStep()
  },
)

// 清单换了一批（重新拉过）之后高亮回到第一条，否则下标可能指到列表外面
watch(
  () => store.snapshots,
  () => {
    active.value = 0
  },
)

/**
 * 两步之间要重新安排焦点。
 *
 * 点一条快照会让整个列表连着那个按钮一起卸载，焦点于是掉到 `<body>` 上——
 * 那已经在对话框外面，Esc 的 keydown 再也到不了这里，浮层就关不掉了。
 * 「换一份」回到列表时同理。
 */
watch(picked, async () => {
  if (props.open) await focusStep()
})

async function focusStep() {
  await nextTick()
  if (target.value) confirmButton.value?.focus()
  else list.value?.focus()
}

/**
 * 上下键与回车挂在**列表**上，不挂在外框上。
 *
 * 挂在外框上试过，是错的：`@keydown.enter.prevent` 编译出来的 `withModifiers`
 * 会**先**无条件 `preventDefault()` 再求值那个表达式，所以 `!target && …` 这种
 * 守卫拦不住 `preventDefault`。而回车激活按钮正是被它取消掉的那个默认行为——
 * 结果是焦点在「关闭」上按回车不但没关闭，反而弹出了第一条快照的确认页。
 *
 * 挂在列表上就没有这个问题：列表里的选项是 `tabindex="-1"`，回车对它们没有默认
 * 行为可以取消，而对话框里真正的按钮都在列表外面，各自的回车照常工作。
 * 列表因此需要 `tabindex="0"` ——`aria-activedescendant` 必须待在**拿着焦点**的
 * 那个元素上，读屏器才会去念它指向的选项。
 *
 * 每一条都能被 Tab 走到也是一种做法，但快照可能有两百条，
 * 那意味着按两百次 Tab 才能离开列表。
 */
function move(delta: number) {
  const total = store.snapshots.length
  if (!total) return
  active.value = (active.value + delta + total) % total
  // 高亮走到视口外面就跟着滚动，否则键盘用户看不到自己选中了什么
  void nextTick(() => {
    document.getElementById(activeId.value ?? '')?.scrollIntoView({ block: 'nearest' })
  })
}

function pickActive() {
  const item = store.snapshots[active.value]
  if (item) picked.value = item.id
}

/** RFC3339 → 本地时间。坏时间戳原样显示，不该因为格式化失败而空掉一行。 */
function when(at: string): string {
  const date = new Date(at)
  return Number.isNaN(date.getTime()) ? at : date.toLocaleString()
}

async function confirm() {
  const id = picked.value
  if (!id) return
  picked.value = null
  emit('close')
  await actions.restoreSnapshot(id)
}
</script>

<template>
  <div v-if="props.open" class="palette" @pointerdown.self="emit('close')">
    <div
      class="palette__box dialog"
      role="dialog"
      aria-modal="true"
      aria-labelledby="snapshots-title"
      @keydown.esc.prevent="emit('close')"
    >
      <h2 id="snapshots-title" class="dialog__title">回退内容</h2>
      <p class="dialog__desc">
        每次删除、批量修改、跨文件替换、导入与 Agent 写操作之前都会自动留一份内容快照。
        回退只动<strong>源文件</strong>（内容、模板、主题、静态资源与
        <code>staticsmith.toml</code>），产物 <code>dist/</code> 不动。
      </p>

      <template v-if="!target">
        <p v-if="store.snapshots.length === 0" class="dialog__desc">
          还没有快照。第一次做破坏性操作时会自动留下第一份。
        </p>
        <ul
          v-else
          ref="list"
          class="snapshots"
          role="listbox"
          tabindex="0"
          aria-labelledby="snapshots-title"
          :aria-activedescendant="activeId"
          @keydown.down.prevent="move(1)"
          @keydown.up.prevent="move(-1)"
          @keydown.enter.prevent="pickActive"
        >
          <li v-for="(item, index) in store.snapshots" :key="item.id" role="presentation">
            <button
              :id="`snapshot-${item.id}`"
              type="button"
              class="snapshot"
              role="option"
              tabindex="-1"
              :aria-selected="active === index"
              :class="{ active: active === index }"
              @pointerenter="active = index"
              @click="picked = item.id"
            >
              <span class="snapshot__what">{{ snapshotLabel(item.message) }}</span>
              <span class="snapshot__when">{{ when(item.at) }}</span>
              <code class="snapshot__id">{{ item.id }}</code>
            </button>
          </li>
        </ul>
        <p v-if="store.snapshots.length > 0" class="dialog__desc">
          上下键选，回车看这一条会撤销什么。
          <template v-if="truncated">
            只列出最近 {{ api.SNAPSHOT_LIMIT }} 份，更早的仍在
            <code>.staticsmith/history.git</code> 里，可以用 git 取。
          </template>
        </p>

        <!-- 关闭不受忙态影响：后台在生成时也得关得掉这层浮层（见 ui.md「忙态」） -->
        <button type="button" class="dialog__cancel" @click="emit('close')">关闭</button>
      </template>

      <template v-else>
        <p class="dialog__desc">
          回退到 <strong>{{ snapshotLabel(target.message) }}</strong>
          （{{ when(target.at) }}，<code>{{ target.id }}</code>）。
        </p>
        <p class="dialog__desc">
          这之后的内容改动会被撤销。撤销掉的状态本身也会先存成一份快照，
          所以回退错了还能再回退回来。
        </p>
        <p v-if="unsaved" class="dialog__desc snapshot__warn">
          编辑器里有未保存的{{ unsaved }}改动，回退会让它作废。
        </p>

        <button ref="confirmButton" type="button" :disabled="store.busy" @click="confirm">
          确认回退
        </button>
        <button type="button" class="dialog__cancel" @click="picked = null">换一份</button>
      </template>
    </div>
  </div>
</template>
