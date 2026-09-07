<script setup lang="ts">
/**
 * 内容编辑区。
 *
 * 编辑的是 Markdown 源文（含 `+++` front matter），保存后由 Rust 侧
 * 重新解析并给出增量构建计划。
 *
 * 三个交互决定：
 * - 工具条与快捷键直接操作选区，不引入富文本模型——源文始终是唯一真相
 * - 有未保存改动时切换文章会被拦下来问一句，而不是静默丢弃
 * - 粘贴或拖入图片先落盘到站点资源目录（内容寻址命名），再把地址插到光标处
 */
import { computed, ref } from 'vue'

import { actions, isDirty, store } from '../store'

const textarea = ref<HTMLTextAreaElement | null>(null)
const dragging = ref(false)
const filePicker = ref<HTMLInputElement | null>(null)

const affected = computed(() => store.plan?.pages.length ?? 0)
const total = computed(() => store.plan?.total_pages ?? 0)

// ---------------------------------------------------------------- 选区编辑

/** 用新文本替换选区，并把光标落在 `cursor` 指定的绝对位置。 */
function replaceSelection(text: string, cursorStart: number, cursorEnd: number) {
  const el = textarea.value
  if (!el) return
  const raw = store.currentRaw
  actions.setRaw(raw.slice(0, el.selectionStart) + text + raw.slice(el.selectionEnd))
  requestAnimationFrame(() => {
    el.setSelectionRange(cursorStart, cursorEnd)
    el.focus()
  })
}

/** 包裹选区。没有选区时插入标记并把光标放中间，方便直接输入。 */
function wrap(before: string, after = before) {
  const el = textarea.value
  if (!el) return
  const selected = store.currentRaw.slice(el.selectionStart, el.selectionEnd)
  const start = el.selectionStart + before.length
  replaceSelection(`${before}${selected}${after}`, start, start + selected.length)
}

/** 给选中的每一行加前缀（标题、引用、列表）。 */
function prefixLines(prefix: string) {
  const el = textarea.value
  if (!el) return
  const raw = store.currentRaw
  const lineStart = raw.lastIndexOf('\n', el.selectionStart - 1) + 1
  const lineEnd = raw.indexOf('\n', el.selectionEnd)
  const end = lineEnd === -1 ? raw.length : lineEnd
  const block = raw.slice(lineStart, end)
  const replaced = block
    .split('\n')
    .map((line) => (line.startsWith(prefix) ? line.slice(prefix.length) : prefix + line))
    .join('\n')

  actions.setRaw(raw.slice(0, lineStart) + replaced + raw.slice(end))
  requestAnimationFrame(() => {
    el.setSelectionRange(lineStart, lineStart + replaced.length)
    el.focus()
  })
}

function insertLink() {
  const el = textarea.value
  if (!el) return
  const selected = store.currentRaw.slice(el.selectionStart, el.selectionEnd) || '链接文字'
  const snippet = `[${selected}](https://)`
  // 光标停在 URL 位置，接着就能粘地址
  const urlStart = el.selectionStart + selected.length + 3
  replaceSelection(snippet, urlStart, urlStart + 8)
}

// ---------------------------------------------------------------- 资源插入

function markdownFor(file: File, url: string): string {
  const alt = file.name.replace(/\.[^.]+$/, '') || '图片'
  return file.type.startsWith('image/') ? `![${alt}](${url})` : `[${file.name}](${url})`
}

async function insertFiles(files: File[]) {
  for (const file of files) {
    const url = await actions.saveAsset(file)
    if (!url) continue
    const el = textarea.value
    const snippet = markdownFor(file, url)
    if (el) {
      const caret = el.selectionStart + snippet.length
      replaceSelection(snippet, caret, caret)
    } else {
      actions.setRaw(store.currentRaw + snippet)
    }
  }
}

async function onPaste(event: ClipboardEvent) {
  const files = [...(event.clipboardData?.items ?? [])]
    .filter((item) => item.kind === 'file')
    .map((item) => item.getAsFile())
    .filter((file): file is File => file !== null)

  if (files.length === 0) return // 纯文本粘贴走浏览器默认行为
  event.preventDefault()
  await insertFiles(files)
}

async function onDrop(event: DragEvent) {
  dragging.value = false
  const files = [...(event.dataTransfer?.files ?? [])]
  if (files.length === 0) return
  event.preventDefault()
  await insertFiles(files)
}

async function onPickFiles(event: Event) {
  const input = event.target as HTMLInputElement
  await insertFiles([...(input.files ?? [])])
  input.value = ''
}

// ---------------------------------------------------------------- 快捷键

function onKeydown(event: KeyboardEvent) {
  if (!(event.ctrlKey || event.metaKey)) return
  const key = event.key.toLowerCase()
  const handlers: Record<string, () => void> = {
    s: () => void actions.saveContent(),
    b: () => wrap('**'),
    i: () => wrap('*'),
    k: insertLink,
  }
  const handler = handlers[key]
  if (!handler) return
  event.preventDefault()
  handler()
}
</script>

<template>
  <section v-if="store.currentSource" class="editor">
    <header class="editor__bar">
      <strong>{{ store.currentSource }}</strong>
      <span v-if="isDirty" class="editor__dirty" title="有未保存改动">●</span>
      <span class="editor__spacer" />
      <span class="editor__hint">保存后将重新生成 {{ affected }} / {{ total }} 个页面</span>
      <button type="button" :disabled="store.busy || !isDirty" @click="actions.saveContent()">
        保存 <kbd>Ctrl+S</kbd>
      </button>
      <button type="button" :disabled="store.busy" @click="actions.refreshPreview()">
        刷新预览
      </button>
    </header>

    <p v-if="store.pendingPage" class="editor__pending">
      <span>当前文章有未保存改动，切换到「{{ store.pendingPage.title }}」前要怎么处理？</span>
      <button type="button" @click="actions.resolvePending('save')">保存并切换</button>
      <button type="button" @click="actions.resolvePending('discard')">放弃改动</button>
      <button type="button" @click="actions.resolvePending('cancel')">留在本页</button>
    </p>

    <div class="editor__toolbar">
      <button type="button" title="标题（H2）" @click="prefixLines('## ')">H2</button>
      <button type="button" title="加粗 Ctrl+B" @click="wrap('**')"><b>B</b></button>
      <button type="button" title="斜体 Ctrl+I" @click="wrap('*')"><i>I</i></button>
      <button type="button" title="行内代码" @click="wrap('`')">&lt;/&gt;</button>
      <button type="button" title="引用" @click="prefixLines('> ')">❝</button>
      <button type="button" title="无序列表" @click="prefixLines('- ')">•</button>
      <button type="button" title="链接 Ctrl+K" @click="insertLink">🔗</button>
      <button type="button" title="插入图片（也可直接粘贴或拖入）" @click="filePicker?.click()">
        图片
      </button>
      <input
        ref="filePicker"
        type="file"
        accept="image/*"
        multiple
        hidden
        @change="onPickFiles"
      />
    </div>

    <textarea
      ref="textarea"
      class="editor__area"
      :class="{ 'editor__area--dragging': dragging }"
      spellcheck="false"
      :value="store.currentRaw"
      aria-label="内容源文"
      @input="actions.setRaw(($event.target as HTMLTextAreaElement).value)"
      @keydown="onKeydown"
      @paste="onPaste"
      @dragover.prevent="dragging = true"
      @dragleave="dragging = false"
      @drop="onDrop"
    />

    <footer class="editor__foot">
      <kbd>Ctrl+S</kbd> 保存 · <kbd>Ctrl+B</kbd> 加粗 · <kbd>Ctrl+I</kbd> 斜体 ·
      <kbd>Ctrl+K</kbd> 链接 · <kbd>Ctrl+Enter</kbd> 增量生成 · 粘贴或拖入图片即插入
    </footer>
  </section>

  <section v-else class="editor editor--empty">
    <p>从左侧选择一篇内容开始编辑，或点「新建内容」。</p>
  </section>
</template>
