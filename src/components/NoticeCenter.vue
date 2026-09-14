<script setup lang="ts">
/**
 * 消息中心：翻看刚才那些通知。
 *
 * 通知本身是瞬时的（错误 8 秒、其余 3.5 秒），这在「刚点了一下」的那几秒够用，
 * 之后就彻底没了。而最需要事后再看一眼的恰恰是错误：手改 `staticsmith.toml`
 * 存盘失败时，Rust 侧那条带行列号的 TOML 报错飘走之后无处可查，
 * 用户只知道「保存没成功」。
 *
 * 浮层行为照 `docs/ui.md`「所有浮层的共同约定」：Tab 圈在里面、Esc 关闭、
 * 点空白（click 而非 pointerdown）关闭、关闭后把焦点还给打开它的元素、
 * 关闭按钮不跟随忙态。
 */
import { nextTick, ref, watch } from 'vue'

import { trapTab } from '../focus'
import { formatNoticeTime, type NoticeLevel } from '../notices'
import { actions, store } from '../store'

const props = defineProps<{ open: boolean }>()
const emit = defineEmits<{ close: [] }>()

/** 浮层根元素，用来把 Tab 圈在里面。 */
const box = ref<HTMLElement | null>(null)
const closeButton = ref<HTMLButtonElement | null>(null)

let restoreFocus: HTMLElement | null = null

/** 三种程度各自的中文名。徽章只有颜色的话，读屏器那边什么都听不到。 */
const LEVEL_LABEL: Record<NoticeLevel, string> = {
  success: '成功',
  info: '提示',
  error: '错误',
}

watch(
  () => props.open,
  async (open) => {
    if (!open) {
      restoreFocus?.focus()
      restoreFocus = null
      return
    }
    restoreFocus = document.activeElement as HTMLElement | null
    // 打开即算看过：面板一打开，未读数就该归零，否则它会一直挂着数字。
    actions.markNoticesSeen()
    await nextTick()
    closeButton.value?.focus()
  },
)
</script>

<template>
  <!-- 点空白关闭用 click 而不是 pointerdown：按下即关会在从遮罩起手拖选文字时误关 -->
  <div v-if="props.open" class="palette" @click.self="emit('close')">
    <div
      ref="box"
      class="palette__box dialog"
      role="dialog"
      aria-modal="true"
      aria-labelledby="notices-title"
      @keydown.esc.prevent="emit('close')"
      @keydown.tab="trapTab(box, $event)"
    >
      <h2 id="notices-title" class="dialog__title">消息</h2>
      <p class="dialog__desc">
        最近 {{ store.notices.length }} 条通知，新的在上面。通知气泡几秒后会消失，
        这里留着让你事后还能读一遍——尤其是带行列号的报错。
      </p>

      <p v-if="store.notices.length === 0" class="dialog__desc">还没有消息。</p>
      <ul v-else class="notices" aria-labelledby="notices-title">
        <li v-for="notice in store.notices" :key="notice.id" class="notice">
          <span class="notice__time">{{ formatNoticeTime(notice.at) }}</span>
          <span class="badge" :class="`badge--${notice.level}`">
            {{ LEVEL_LABEL[notice.level] }}
          </span>
          <span class="notice__text">{{ notice.message }}</span>
        </li>
      </ul>

      <div class="dialog__actions">
        <!-- 关闭与清空都不改站点内容，不该跟着忙态变灰（见 ui.md） -->
        <button
          ref="closeButton"
          type="button"
          class="dialog__cancel"
          @click="emit('close')"
        >
          关闭
        </button>
        <button
          v-if="store.notices.length"
          type="button"
          class="page-list__danger"
          @click="actions.clearNotices()"
        >
          清空
        </button>
      </div>
    </div>
  </div>
</template>
