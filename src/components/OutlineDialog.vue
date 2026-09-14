<script setup lang="ts">
/**
 * 大纲：这一篇的目录，点一下跳过去。
 *
 * 写长文时最缺的其实是纸质书那两样东西——目录和页码。这里补的是目录：
 * 从源文本直接抽标题（`parseOutline`），不经过渲染，所以边写边有，
 * 而且代码块里的 `# 注释` 不会混进来。
 *
 * 做成浮层而不是常驻侧栏：编辑器已经是三栏（列表 / 正文 / 预览），再挤一栏会让正文变窄；
 * 而目录是「用一下就走」的东西，跟命令面板同一类。行为照 `docs/ui.md`
 * 「所有浮层的共同约定」：Tab 圈在里面、Esc 关闭、点空白（click）关闭、
 * 关闭后焦点还给打开它的元素。
 *
 * 上下键与回车挂在**列表**上而不是外框上，与 `SnapshotDialog` 一致——挂外框的话，
 * 焦点在按钮上时方向键会被外框先吃掉。
 */
import { computed, nextTick, ref, watch } from 'vue'

import { trapTab } from '../focus'
import { parseOutline } from '../manuscript'

const props = defineProps<{ open: boolean; source: string }>()
const emit = defineEmits<{ close: []; jump: [offset: number] }>()

const box = ref<HTMLElement | null>(null)
const list = ref<HTMLElement | null>(null)
const closeButton = ref<HTMLButtonElement | null>(null)
/** 键盘高亮到第几条（不是选中，选中要按回车）。 */
const active = ref(0)

let restoreFocus: HTMLElement | null = null

const headings = computed(() => parseOutline(props.source))

/** 当前高亮项的 DOM id，给 `aria-activedescendant`：读屏器据此念出停在哪一条。 */
const activeId = computed(() =>
  headings.value[active.value] ? `outline-${active.value}` : undefined,
)

watch(
  () => props.open,
  async (open) => {
    if (!open) {
      restoreFocus?.focus()
      restoreFocus = null
      return
    }
    restoreFocus = document.activeElement as HTMLElement | null
    active.value = 0
    await nextTick()
    // 有目录就把焦点放列表上（方向键立刻可用）；没有就放关闭按钮，别把焦点丢在浮层外
    ;(list.value ?? closeButton.value)?.focus()
  },
)

function move(step: number) {
  const total = headings.value.length
  if (!total) return
  active.value = (active.value + step + total) % total
}

function jump(index: number) {
  const heading = headings.value[index]
  if (!heading) return
  emit('jump', heading.offset)
  emit('close')
}
</script>

<template>
  <!-- 点空白关闭用 click 而不是 pointerdown：按下即关会在从遮罩起手拖选文字时误关 -->
  <div v-if="props.open" class="palette" @click.self="emit('close')">
    <div
      ref="box"
      class="palette__box dialog"
      role="dialog"
      aria-modal="true"
      aria-labelledby="outline-title"
      @keydown.esc.prevent="emit('close')"
      @keydown.tab="trapTab(box, $event)"
    >
      <h2 id="outline-title" class="dialog__title">大纲</h2>
      <p class="dialog__desc">
        这一篇的标题结构，点一下跳到那一节。上下键移动、回车跳转。
      </p>

      <p v-if="headings.length === 0" class="dialog__desc">
        这一篇还没有标题。用 <code>##</code> 起一节，目录会自己长出来。
      </p>
      <ul
        v-else
        ref="list"
        class="outline"
        role="listbox"
        tabindex="0"
        aria-labelledby="outline-title"
        :aria-activedescendant="activeId"
        @keydown.down.prevent="move(1)"
        @keydown.up.prevent="move(-1)"
        @keydown.enter.prevent="jump(active)"
      >
        <li v-for="(heading, index) in headings" :key="heading.offset" role="presentation">
          <button
            :id="`outline-${index}`"
            type="button"
            class="outline__item"
            :class="[`outline__item--h${heading.level}`, { active: active === index }]"
            role="option"
            tabindex="-1"
            :aria-selected="active === index"
            @pointerenter="active = index"
            @click="jump(index)"
          >
            <span class="outline__level">H{{ heading.level }}</span>
            <span class="outline__text">{{ heading.text }}</span>
          </button>
        </li>
      </ul>

      <div class="dialog__actions">
        <!-- 关闭不改任何东西，不跟随忙态（见 ui.md） -->
        <button ref="closeButton" type="button" class="dialog__cancel" @click="emit('close')">
          关闭
        </button>
      </div>
    </div>
  </div>
</template>
