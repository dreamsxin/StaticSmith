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

import { actions, isDirty, store } from '../store'
import type { PageSummary, TemplateInfo } from '../api'

/** 与 App.vue 的标签页一致。面板只负责发意图，切页仍归外框。 */
export type PaletteTab =
  | 'content'
  | 'calendar'
  | 'layouts'
  | 'audit'
  | 'build'
  | 'deploy'
  | 'settings'

interface Item {
  id: string
  group: string
  label: string
  /** 右侧灰字：路径、地址或快捷键 */
  hint?: string
  run: () => void | Promise<void>
}

const props = defineProps<{ open: boolean }>()
const emit = defineEmits<{ close: []; navigate: [PaletteTab] }>()

const keyword = ref('')
const active = ref(0)
const input = ref<HTMLInputElement | null>(null)

/** 每组在空关键词下的展示上限：面板是入口，不是完整列表。 */
const PREVIEW_LIMIT = 6

function go(tab: PaletteTab) {
  emit('navigate', tab)
}

const commands = computed<Item[]>(() => {
  const list: Item[] = [
    { id: 'go.content', group: '命令', label: '切换到 内容（写文章）', run: () => go('content') },
    { id: 'go.calendar', group: '命令', label: '切换到 日历（发布节奏）', run: () => go('calendar') },
    { id: 'go.layouts', group: '命令', label: '切换到 外观（模板与主题包）', run: () => go('layouts') },
    { id: 'go.audit', group: '命令', label: '切换到 体检（SEO / 死链 / 媒体）', run: () => go('audit') },
    { id: 'go.build', group: '命令', label: '切换到 生成', run: () => go('build') },
    { id: 'go.deploy', group: '命令', label: '切换到 发布', run: () => go('deploy') },
    { id: 'go.settings', group: '命令', label: '切换到 设置', run: () => go('settings') },
    {
      id: 'build.incremental',
      group: '命令',
      label: '增量生成',
      hint: 'Ctrl+Enter',
      run: () => actions.build('incremental'),
    },
    {
      id: 'build.full',
      group: '命令',
      label: '完整重建',
      hint: 'Ctrl+Shift+Enter',
      run: () => actions.build('full'),
    },
    {
      id: 'audit.seo',
      group: '命令',
      label: 'SEO 体检',
      run: async () => {
        go('audit')
        await actions.auditSeo()
      },
    },
    {
      id: 'audit.media',
      group: '命令',
      label: '媒体资源体检',
      run: async () => {
        go('audit')
        await actions.auditMedia()
      },
    },
    {
      id: 'audit.links',
      group: '命令',
      label: '站内死链体检',
      run: async () => {
        go('audit')
        await actions.auditLinks()
      },
    },
    {
      id: 'preview.server',
      group: '命令',
      label: store.previewServer ? '关闭本地预览服务器' : '启动本地预览服务器',
      hint: store.previewServer ?? undefined,
      run: () => actions.togglePreviewServer(),
    },
    {
      id: 'autobuild',
      group: '命令',
      label: store.autoBuild ? '关闭「保存即生成」' : '打开「保存即生成」',
      run: () => actions.setAutoBuild(!store.autoBuild),
    },
    { id: 'reveal', group: '命令', label: '在文件管理器里打开产物目录', run: () => actions.revealOutput() },
    { id: 'refresh', group: '命令', label: '重新读取项目', run: () => actions.refresh() },
    { id: 'close', group: '命令', label: '关闭项目', run: () => actions.closeProject() },
  ]
  // 没改动时列出「保存」只会让人误以为有东西没存
  if (isDirty.value) {
    list.splice(6, 0, {
      id: 'save',
      group: '命令',
      label: '保存当前内容',
      hint: 'Ctrl+S',
      run: () => actions.saveContent(),
    })
  }
  return list
})

const pages = computed<Item[]>(() =>
  (store.project?.pages ?? []).map((page) => ({
    id: `page:${page.source}`,
    group: '文章',
    label: page.title || page.source,
    hint: page.source,
    run: async () => {
      go('content')
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
      go('layouts')
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
        go('content')
        await actions.previewOutput(file.url)
      },
    })),
)

/** 命令在前：输入框空着时先给「能做什么」，而不是一屏文件名。 */
const groups = computed(() => {
  const query = keyword.value.trim().toLowerCase()
  const all = [commands.value, pages.value, templates.value, outputs.value]

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

watch(
  () => props.open,
  async (open) => {
    if (!open) return
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
</script>

<template>
  <div v-if="props.open" class="palette" @pointerdown.self="emit('close')">
    <div class="palette__box" role="dialog" aria-label="命令面板">
      <input
        ref="input"
        v-model="keyword"
        class="palette__input"
        type="text"
        placeholder="搜索文章、模板、产物，或执行命令…"
        @keydown.down.prevent="move(1)"
        @keydown.up.prevent="move(-1)"
        @keydown.enter.prevent="accept"
        @keydown.esc.prevent="emit('close')"
      />

      <div v-if="flat.length" class="palette__list">
        <template v-for="group in groups" :key="group.name">
          <p class="palette__group">
            {{ group.name }}
            <span v-if="group.hidden > 0" class="palette__more">还有 {{ group.hidden }} 条，输入可筛选</span>
          </p>
          <button
            v-for="item in group.items"
            :key="item.id"
            type="button"
            class="palette__item"
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
        <kbd>↑</kbd><kbd>↓</kbd> 选择 · <kbd>Enter</kbd> 执行 · <kbd>Esc</kbd> 关闭
      </p>
    </div>
  </div>
</template>
