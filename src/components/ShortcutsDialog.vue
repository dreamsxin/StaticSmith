<script setup lang="ts">
/**
 * 快捷键一览。
 *
 * 帮助菜单里那条「快捷键与全部命令」原先**只是打开命令面板**——面板里能搜到命令，
 * 但键位以灰字散落在各条上，还会被「还有 N 条」折起来；置灰的命令连带它的键位一起消失。
 * 界面上唯一的一览是编辑器页脚那六个，且只在打开文章后可见。名不副实的入口比没有更糟：
 * 用户点过一次，以为「就这些了」。
 *
 * 键位表是 `src/shortcuts.ts`，这里只负责显示。按**作用域**分组是这个浮层的全部价值——
 * 「在哪儿按才有用」是用户最常猜错的一件事：`Ctrl+B` 在列表里按下去毫无反应，
 * 不是坏了，是它属于编辑器。
 *
 * 行为照 `docs/ui.md`「所有浮层的共同约定」：Tab 圈在里面、Esc 关闭、
 * 点空白（click 而不是 pointerdown）关闭、关闭后焦点还给打开它的元素。
 * 没有列表交互，所以不接方向键——它是一张表，不是一个选择器。
 */
import { nextTick, ref, watch } from 'vue'

import { trapTab } from '../focus'
import { SCOPES, shortcutsIn } from '../shortcuts'

const props = defineProps<{ open: boolean }>()
const emit = defineEmits<{ close: [] }>()

const box = ref<HTMLElement | null>(null)
const closeButton = ref<HTMLButtonElement | null>(null)

let restoreFocus: HTMLElement | null = null

watch(
  () => props.open,
  async (open) => {
    if (!open) {
      restoreFocus?.focus()
      restoreFocus = null
      return
    }
    restoreFocus = document.activeElement as HTMLElement | null
    await nextTick()
    // 焦点给关闭按钮：这里没有可操作的列表，Tab 一圈就是「读完就走」
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
      aria-labelledby="shortcuts-title"
      @keydown.esc.prevent="emit('close')"
      @keydown.tab="trapTab(box, $event)"
    >
      <h2 id="shortcuts-title" class="dialog__title">快捷键一览</h2>
      <p class="dialog__desc">
        macOS 上 <kbd>Ctrl</kbd> 与 <kbd>Cmd</kbd> 都认。全部命令（含没有键位的）在命令面板里，
        <kbd>Ctrl+P</kbd>。
      </p>

      <div class="shortcuts">
        <section v-for="scope in SCOPES" :key="scope.id" class="shortcuts__group">
          <h3 class="shortcuts__scope">
            {{ scope.label }}
            <span class="shortcuts__scope-hint">{{ scope.hint }}</span>
          </h3>
          <ul class="shortcuts__list">
            <li v-for="item in shortcutsIn(scope.id)" :key="item.keys" class="shortcuts__row">
              <kbd class="shortcuts__keys">{{ item.keys }}</kbd>
              <span class="shortcuts__what">{{ item.what }}</span>
              <!-- 取舍与前提写在右边而不是塞进动作里：一行里两种信息要分得开 -->
              <span v-if="item.note" class="shortcuts__note">{{ item.note }}</span>
            </li>
          </ul>
        </section>
      </div>

      <div class="dialog__actions">
        <!-- 关闭不改任何东西，不跟随忙态（见 ui.md） -->
        <button ref="closeButton" type="button" class="dialog__cancel" @click="emit('close')">
          关闭
        </button>
      </div>
    </div>
  </div>
</template>
