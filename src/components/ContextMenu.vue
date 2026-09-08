<script setup lang="ts">
/**
 * 右键菜单：只放「针对眼前这个对象」的动作。
 *
 * 与菜单栏共用 `MenuEntry` 形状与外观（`src/commands.ts`），所以禁用置灰、勾选、
 * 危险项标红三条规则不必再实现一遍。位置跟着鼠标，靠近右/下边缘时向内翻，
 * 免得菜单跑到窗口外面。
 *
 * 右键是**看不见的入口**，所以这里出现的动作必须另有可见入口（分组头的「⋯」按钮、
 * 批量动作条、菜单栏）。否则等于把功能藏起来。
 */
import { computed, nextTick, onBeforeUnmount, onMounted, watch } from 'vue'

import { closeContextMenu, contextMenu, isSeparator, type Command } from '../commands'

/** 菜单尺寸的保守估计：只用来决定要不要翻向，不追求精确。 */
const WIDTH = 240
const ITEM = 30

const style = computed(() => {
  const height = contextMenu.items.length * ITEM + 12
  const flipX = contextMenu.x + WIDTH > window.innerWidth
  const flipY = contextMenu.y + height > window.innerHeight
  return {
    left: `${flipX ? Math.max(4, contextMenu.x - WIDTH) : contextMenu.x}px`,
    top: `${flipY ? Math.max(4, contextMenu.y - height) : contextMenu.y}px`,
  }
})

async function focusFirst() {
  await nextTick()
  document
    .querySelector<HTMLButtonElement>('.context-menu .app__menu-command:not(:disabled)')
    ?.focus()
}

watch(
  () => contextMenu.open,
  (open) => {
    if (open) void focusFirst()
  },
)

async function run(command: Command) {
  if (command.disabled) return
  closeContextMenu()
  await command.run()
}

/** 点空白、按 Esc、滚动都关掉：浮层不该跟着页面内容飘。 */
function onPointerDown(event: PointerEvent) {
  if (!(event.target as HTMLElement).closest('.context-menu')) closeContextMenu()
}

function onKeydown(event: KeyboardEvent) {
  if (event.key === 'Escape') closeContextMenu()
}

function onKeydownInside(event: KeyboardEvent) {
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
  window.addEventListener('wheel', closeContextMenu, { passive: true })
})

onBeforeUnmount(() => {
  window.removeEventListener('pointerdown', onPointerDown)
  window.removeEventListener('keydown', onKeydown)
  window.removeEventListener('wheel', closeContextMenu)
})
</script>

<template>
  <div
    v-if="contextMenu.open"
    class="context-menu app__menu-drop"
    role="menu"
    aria-label="右键菜单"
    :style="style"
    @keydown="onKeydownInside"
  >
    <template v-for="(entry, at) in contextMenu.items">
      <hr v-if="isSeparator(entry)" :key="`sep-${at}`" class="app__menu-sep" role="separator" />
      <button
        v-else
        :key="entry.id"
        type="button"
        class="app__menu-command"
        :class="{ danger: entry.danger }"
        role="menuitem"
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
</template>
