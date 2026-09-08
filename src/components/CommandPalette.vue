<script setup lang="ts">
/**
 * 命令面板（Ctrl+P）。
 *
 * 站点大了以后，「跳到某篇文章」「跑一次体检」「切到发布」这些动作都要先想
 * 「它在哪个标签页里」。命令面板把导航与动作合并成一个输入框：记得名字就够了，
 * 不必记得界面结构。这也是同类编辑器（VS Code、Obsidian）里最省时间的一个入口。
 *
 * 刻意不做模糊匹配打分：中文标题里子串匹配已经足够准，
 * 而一个排序玄学的列表会让人怀疑「是不是漏了」。
 */
import { computed, nextTick, ref, watch } from 'vue'

import { flatCommands } from '../commands'
import { actions, store } from '../store'
import { goTo, allTabs } from '../ui'
import type { PageSummary, TemplateInfo } from '../api'

interface Item {
  id: string
  group: string
  label: string
  /** 右侧灰字：路径、地址或快捷键 */
  hint?: string
  run: () => void | Promise<void>
}

const props = defineProps<{ open: boolean }>()
const emit = defineEmits<{ close: [] }>()

const keyword = ref('')
const active = ref(0)
const input = ref<HTMLInputElement | null>(null)

/** 每组在空关键词下的展示上限：面板是入口，不是完整列表。 */
const PREVIEW_LIMIT = 6

/**
 * 命令来自菜单栏的同一份表（`src/commands.ts`）。
 *
 * 两边各写一遍迟早分裂成「面板里有、菜单里没有」，而那正是「不知道能干什么」的来源。
 * 置灰的项不进面板：搜出来又点不动更让人困惑。
 */
const commands = computed<Item[]>(() =>
  flatCommands().map((command) => ({
    id: command.id,
    group: '命令',
    // 带上所属菜单：搜到之后也知道下次能在哪个菜单里找到它
    label: `${command.group} · ${command.label}`,
    hint: command.hint,
    run: command.run,
  })),
)

/**
 * 页面跳转只留在这里，不进菜单。
 *
 * 「视图」菜单曾经也列过七个「切换到…」，与标签栏完全重复——Word 的视图菜单同样
 * 不重复选项卡。跳转的正规入口是标签栏，搜索式跳转归命令面板。
 */
const tabs = computed<Item[]>(() =>
  allTabs.map((item) => ({
    id: `go.${item.id}`,
    group: '页面',
    label: `切换到 ${item.label}`,
    hint: item.hint,
    run: () => goTo(item.id),
  })),
)

const pages = computed<Item[]>(() =>
  (store.project?.pages ?? []).map((page) => ({
    id: `page:${page.source}`,
    group: '文章',
    label: page.title || page.source,
    hint: page.source,
    run: async () => {
      goTo('content')
      await actions.requestOpenContent(page as PageSummary)
    },
  })),
)

const templates = computed<Item[]>(() =>
  (store.project?.templates ?? []).map((template) => ({
    id: `template:${template.name}`,
    group: '模板',
    label: template.name,
    hint: template.kind,
    run: async () => {
      goTo('layouts')
      await actions.openTemplate(template as TemplateInfo)
    },
  })),
)

const outputs = computed<Item[]>(() =>
  store.outputs
    .filter((file) => file.path.endsWith('.html'))
    .map((file) => ({
      id: `output:${file.path}`,
      group: '产物页面',
      label: file.url,
      hint: file.path,
      run: async () => {
        goTo('content')
        await actions.previewOutput(file.url)
      },
    })),
)

/** 命令在前：输入框空着时先给「能做什么」，而不是一屏文件名。 */
const groups = computed(() => {
  const query = keyword.value.trim().toLowerCase()
  const all = [commands.value, tabs.value, pages.value, templates.value, outputs.value]

  return all
    .map((items) => {
      const matched = query
        ? items.filter(
            (item) =>
              item.label.toLowerCase().includes(query) ||
              (item.hint ?? '').toLowerCase().includes(query),
          )
        : items.slice(0, PREVIEW_LIMIT)
      return { name: items[0]?.group ?? '', items: matched, hidden: items.length - matched.length }
    })
    .filter((group) => group.items.length > 0)
})

/** 上下键要在整个列表里走，所以按显示顺序摊平一份。 */
const flat = computed<Item[]>(() => groups.value.flatMap((group) => group.items))

watch([keyword, () => props.open], () => {
  active.value = 0
})

/**
 * 打开前记住焦点在哪，关闭后还回去。
 *
 * 面板是这个界面里唯一的浮层，用完必须把焦点交回原处：键盘用户在编辑器里按 Ctrl+P、
 * 关掉之后如果焦点落在 body 上，下一次按 Tab 会从页面最开头重新走一遍。
 */
let restoreFocus: HTMLElement | null = null

watch(
  () => props.open,
  async (open) => {
    if (!open) {
      restoreFocus?.focus()
      restoreFocus = null
      return
    }
    const before = document.activeElement
    restoreFocus = before instanceof HTMLElement ? before : null
    keyword.value = ''
    await nextTick()
    input.value?.focus()
  },
)

function move(delta: number) {
  const total = flat.value.length
  if (!total) return
  active.value = (active.value + delta + total) % total
}

async function accept() {
  const item = flat.value[active.value]
  if (!item) return
  emit('close')
  await item.run()
}

/** 当前高亮项的 DOM id，给 `aria-activedescendant` 用：读屏器据此念出选中的那一条。 */
const activeId = computed(() => {
  const item = flat.value[active.value]
  return item ? `palette-item-${item.id}` : undefined
})

</script>

<template>
  <div v-if="props.open" class="palette" @pointerdown.self="emit('close')">
    <div class="palette__box" role="dialog" aria-modal="true" aria-label="命令面板">
      <input
        ref="input"
        v-model="keyword"
        class="palette__input"
        type="text"
        role="combobox"
        aria-controls="palette-list"
        aria-expanded="true"
        :aria-activedescendant="activeId"
        placeholder="搜索文章、模板、产物，或执行命令…"
        @keydown.down.prevent="move(1)"
        @keydown.up.prevent="move(-1)"
        @keydown.tab.exact.prevent="move(1)"
        @keydown.tab.shift.prevent="move(-1)"
        @keydown.enter.prevent="accept"
        @keydown.esc.prevent="emit('close')"
      />

      <div v-if="flat.length" id="palette-list" class="palette__list" role="listbox">
        <template v-for="group in groups" :key="group.name">
          <p class="palette__group">
            {{ group.name }}
            <span v-if="group.hidden > 0" class="palette__more">还有 {{ group.hidden }} 条，输入可筛选</span>
          </p>
          <button
            v-for="item in group.items"
            :id="`palette-item-${item.id}`"
            :key="item.id"
            type="button"
            class="palette__item"
            role="option"
            tabindex="-1"
            :aria-selected="flat[active]?.id === item.id"
            :class="{ active: flat[active]?.id === item.id }"
            @pointerenter="active = flat.findIndex((i) => i.id === item.id)"
            @click="accept"
          >
            <span class="palette__label">{{ item.label }}</span>
            <code v-if="item.hint" class="palette__hint">{{ item.hint }}</code>
          </button>
        </template>
      </div>
      <p v-else class="palette__empty">没有匹配项。</p>

      <p class="palette__foot">
        <kbd>↑</kbd><kbd>↓</kbd> 或 <kbd>Tab</kbd> 选择 · <kbd>Enter</kbd> 执行 ·
        <kbd>Esc</kbd> 关闭
      </p>
    </div>
  </div>
</template>
