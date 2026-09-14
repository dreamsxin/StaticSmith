<script setup lang="ts">
/**
 * 链到站内的哪一篇。
 *
 * 交叉引用（「见第三章」）是写书时最常做的动作之一。以前 `Ctrl+K` 插出来是
 * `[文字](https://)`，链到站内就得自己记地址或者切去列表看一眼——手打的地址
 * 正是死链的来源，而死链要等「体检」页才发现。这里改成挑：候选来自已经加载的
 * 站点页面列表，插进去的地址一定存在。
 *
 * 焦点始终留在输入框里，`↑` `↓` 只移动高亮——与命令面板一致，
 * 边打字边换候选不该要求手离开键盘中央。筛选规则也刻意与命令面板相同：
 * 只做子串、不做打分（`src/crossref.ts`）。
 *
 * 草稿会标出来：草稿不进产物，链过去就是一条死链。标而不禁——写作顺序常常是
 * 先把互链写好、再把那一篇写完，禁掉等于逼人手打地址。
 */
import { computed, nextTick, ref, watch } from 'vue'

import { LINK_LIMIT, matchTargets, type LinkTarget } from '../crossref'
import { trapTab } from '../focus'

const props = defineProps<{
  open: boolean
  pages: readonly LinkTarget[]
  /** 当前正在编辑的那一篇：链到自己几乎总是手滑，从候选里去掉。 */
  exclude?: string
}>()
const emit = defineEmits<{ close: []; pick: [page: LinkTarget] }>()

const box = ref<HTMLElement | null>(null)
const input = ref<HTMLInputElement | null>(null)
const query = ref('')
const active = ref(0)

let restoreFocus: HTMLElement | null = null

const targets = computed(() => matchTargets(props.pages, query.value, { exclude: props.exclude }))

/** 命中数已经顶到上限时说一声，否则「怎么找不到那一篇」会归咎于搜索坏了。 */
const capped = computed(() => targets.value.length === LINK_LIMIT)

const activeId = computed(() => (targets.value[active.value] ? `link-${active.value}` : undefined))

watch(
  () => props.open,
  async (open) => {
    if (!open) {
      restoreFocus?.focus()
      restoreFocus = null
      return
    }
    restoreFocus = document.activeElement as HTMLElement | null
    // 每次重开都从空关键词起：上一次找的那一篇与这一次要链的几乎没关系
    query.value = ''
    active.value = 0
    await nextTick()
    input.value?.focus()
  },
)

// 关键词变了，旧序号会指向另一篇——回到第一条
watch(query, () => {
  active.value = 0
})

function move(step: number) {
  const total = targets.value.length
  if (!total) return
  active.value = (active.value + step + total) % total
}

function pick(index: number) {
  const page = targets.value[index]
  if (!page) return
  emit('pick', page)
  emit('close')
}
</script>

<template>
  <div v-if="props.open" class="palette" @click.self="emit('close')">
    <div
      ref="box"
      class="palette__box dialog"
      role="dialog"
      aria-modal="true"
      aria-labelledby="link-title"
      @keydown.esc.prevent="emit('close')"
      @keydown.tab="trapTab(box, $event)"
    >
      <h2 id="link-title" class="dialog__title">链到站内的一篇</h2>
      <p class="dialog__desc">
        地址由站点数据给出，不用手打——手打的地址正是死链的来源。有选中的文字就拿它当链接文字，
        否则用对方的标题。
      </p>

      <input
        ref="input"
        v-model="query"
        type="text"
        class="palette__input"
        placeholder="按标题、地址或文件路径找"
        aria-label="按标题、地址或文件路径找要链接的内容"
        role="combobox"
        aria-expanded="true"
        aria-controls="link-list"
        :aria-activedescendant="activeId"
        @keydown.down.prevent="move(1)"
        @keydown.up.prevent="move(-1)"
        @keydown.enter.prevent="pick(active)"
      />

      <p v-if="targets.length === 0" class="dialog__desc">
        没有匹配的内容。清空关键词看全部，或先去写那一篇。
      </p>
      <ul
        v-else
        id="link-list"
        class="outline"
        role="listbox"
        aria-labelledby="link-title"
        :aria-activedescendant="activeId"
      >
        <li v-for="(page, index) in targets" :key="page.source" role="presentation">
          <button
            :id="`link-${index}`"
            type="button"
            class="outline__item"
            :class="{ active: active === index }"
            role="option"
            tabindex="-1"
            :aria-selected="active === index"
            :title="page.source"
            @pointerenter="active = index"
            @click="pick(index)"
          >
            <span class="outline__text">{{ page.title || page.source }}</span>
            <span v-if="page.draft" class="link__draft" title="草稿不进产物，链过去会是一条死链">
              草稿
            </span>
            <span class="link__url">{{ page.url }}</span>
          </button>
        </li>
      </ul>
      <p v-if="capped" class="dialog__desc">
        候选只显示前 {{ LINK_LIMIT }} 条，继续输入以缩小范围。
      </p>

      <div class="dialog__actions">
        <!-- 关闭什么也不改，不跟随忙态（见 docs/ui.md） -->
        <button type="button" class="dialog__cancel" @click="emit('close')">取消</button>
      </div>
    </div>
  </div>
</template>
