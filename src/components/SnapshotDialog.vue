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
 */
import { computed, nextTick, ref, watch } from 'vue'

import * as api from '../api'
import { actions, isDirty, isTemplateDirty, store } from '../store'
import { snapshotLabel } from '../text'

const props = defineProps<{ open: boolean }>()
const emit = defineEmits<{ close: [] }>()

/** 选中待确认的那一条。null 表示还在浏览列表。 */
const picked = ref<string | null>(null)
const box = ref<HTMLElement | null>(null)

let restoreFocus: HTMLElement | null = null

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
    // 每次打开都重新拉：期间可能又做过几次破坏性操作，也可能 Agent 动过。
    await actions.loadSnapshots()
    await nextTick()
    box.value?.focus()
  },
)

/**
 * 两步之间要把焦点收回外框。
 *
 * 点一条快照会让整个列表连着那个按钮一起卸载，焦点于是掉到 `<body>` 上——
 * 那已经在对话框外面，Esc 的 keydown 再也到不了这里，浮层就关不掉了。
 * 「换一份」回到列表时同理。
 */
watch(picked, async () => {
  if (!props.open) return
  await nextTick()
  box.value?.focus()
})

const target = computed(() => store.snapshots.find((item) => item.id === picked.value) ?? null)

/** 清单被上限截断了：更早的快照还在仓库里，但这里看不到。 */
const truncated = computed(() => store.snapshots.length >= api.SNAPSHOT_LIMIT)

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
      ref="box"
      class="palette__box dialog"
      role="dialog"
      aria-modal="true"
      aria-labelledby="snapshots-title"
      tabindex="-1"
      @keydown.esc.prevent="emit('close')"
    >
      <h2 id="snapshots-title" class="dialog__title">回退内容</h2>
      <p class="dialog__desc">
        每次删除、批量修改、跨文件替换与 Agent 写操作之前都会自动留一份内容快照。
        回退只动<strong>源文件</strong>（内容、模板、主题、静态资源与
        <code>staticsmith.toml</code>），产物 <code>dist/</code> 不动。
      </p>

      <template v-if="!target">
        <p v-if="store.snapshots.length === 0" class="dialog__desc">
          还没有快照。第一次做破坏性操作时会自动留下第一份。
        </p>
        <ul v-else class="snapshots">
          <li v-for="item in store.snapshots" :key="item.id">
            <button type="button" class="snapshot" @click="picked = item.id">
              <span class="snapshot__what">{{ snapshotLabel(item.message) }}</span>
              <span class="snapshot__when">{{ when(item.at) }}</span>
              <code class="snapshot__id">{{ item.id }}</code>
            </button>
          </li>
        </ul>
        <p v-if="truncated" class="dialog__desc">
          只列出最近 {{ api.SNAPSHOT_LIMIT }} 份。更早的快照仍在
          <code>.staticsmith/history.git</code> 里，可以用 git 取。
        </p>

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



        <button type="button" :disabled="store.busy" @click="confirm">确认回退</button>
        <button type="button" class="dialog__cancel" :disabled="store.busy" @click="picked = null">
          换一份
        </button>
      </template>

      <button
        v-if="!target"
        type="button"
        class="dialog__cancel"
        :disabled="store.busy"
        @click="emit('close')"
      >
        关闭
      </button>
    </div>
  </div>
</template>
