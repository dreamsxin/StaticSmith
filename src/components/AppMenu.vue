<script setup lang="ts">
/**
 * 菜单栏：命令的家。
 *
 * 对标 Word / WPS 的菜单——**想做什么就来菜单里找**，找不到才怀疑功能不存在。
 * 所以这里只放动词（保存、生成、发布、加粗、插链接），名词（内容、外观、体检）
 * 留给下面那排标签页。命令表在 `src/commands.ts`，与命令面板共用一份。
 *
 * 交互沿用桌面软件的老规矩，用户不必学：点一下展开，展开后鼠标划过就切换菜单，
 * Esc 或点空白处关闭，方向键在项目间移动。不可用的项**置灰而不是隐藏**——
 * 隐藏会让人以为这个功能没有。
 */
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'

import { isSeparator, menus, type Command } from '../commands'

const openIndex = ref<number | null>(null)
const root = ref<HTMLElement | null>(null)

/** 每次展开都重算：禁用态、勾选态、最近站点都跟着当前状态变。 */
const list = computed(() => menus())

function toggle(index: number) {
  openIndex.value = openIndex.value === index ? null : index
}

/** 展开状态下划过标题就切换，这是桌面菜单的既有习惯；没展开时不响应悬停。 */
function hover(index: number) {
  if (openIndex.value !== null) openIndex.value = index
}

function close() {
  openIndex.value = null
}

async function run(command: Command) {
  if (command.disabled) return
  close()
  await command.run()
}

function onPointerDown(event: PointerEvent) {
  if (!root.value?.contains(event.target as Node)) close()
}

function onKeydown(event: KeyboardEvent) {
  if (event.key === 'Escape') close()
}

/** 下拉里的方向键：只在可用项之间循环，置灰项跳过。 */
function onMenuKeydown(event: KeyboardEvent) {
  if (event.key !== 'ArrowDown' && event.key !== 'ArrowUp') return
  const container = event.currentTarget as HTMLElement
  const items = [...container.querySelectorAll<HTMLButtonElement>('button:not(:disabled)')]
  if (items.length === 0) return
  event.preventDefault()
  const at = items.indexOf(document.activeElement as HTMLButtonElement)
  const next = event.key === 'ArrowDown' ? at + 1 : at - 1 + items.length
  items[((next % items.length) + items.length) % items.length]?.focus()
}

onMounted(() => {
  window.addEventListener('pointerdown', onPointerDown)
  window.addEventListener('keydown', onKeydown)
})

onBeforeUnmount(() => {
  window.removeEventListener('pointerdown', onPointerDown)
  window.removeEventListener('keydown', onKeydown)
})
</script>

<template>
  <nav ref="root" class="app__menu" aria-label="主菜单">
    <div v-for="(menu, index) in list" :key="menu.label" class="app__menu-item">
      <button
        type="button"
        class="app__menu-title"
        :class="{ open: openIndex === index }"
        :aria-expanded="openIndex === index"
        @click="toggle(index)"
        @mouseenter="hover(index)"
      >
        {{ menu.label }}
      </button>

      <div v-if="openIndex === index" class="app__menu-drop" @keydown="onMenuKeydown">
        <template v-for="(entry, at) in menu.items">
          <hr v-if="isSeparator(entry)" :key="`sep-${at}`" class="app__menu-sep" />
          <button
            v-else
            :key="entry.id"
            type="button"
            class="app__menu-command"
            :class="{ danger: entry.danger }"
            :disabled="entry.disabled"
            :title="entry.hint"
            @click="run(entry)"
          >
            <span class="app__menu-check" aria-hidden="true">{{ entry.checked ? '✓' : '' }}</span>
            <span class="app__menu-label">{{ entry.label }}</span>
            <span v-if="entry.hint" class="app__menu-hint">{{ entry.hint }}</span>
          </button>
        </template>
      </div>
    </div>
  </nav>
</template>
