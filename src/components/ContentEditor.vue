<script setup lang="ts">
/**
 * 内容编辑区。
 *
 * 编辑的是 Markdown 源文（含 `+++` front matter），保存后由 Rust 侧
 * 重新解析并给出增量构建计划。
 *
 * 粘贴或拖入图片时先落盘到站点资源目录（内容寻址命名），再把地址插到光标处。
 */
import { computed, ref } from 'vue'

import { actions, store } from '../store'

const textarea = ref<HTMLTextAreaElement | null>(null)
const dragging = ref(false)

const affected = computed(() => store.plan?.pages.length ?? 0)
const total = computed(() => store.plan?.total_pages ?? 0)

/** 在光标处插入文本，并把光标移到插入内容之后。 */
function insertAtCursor(snippet: string) {
  const el = textarea.value
  const raw = store.currentRaw
  if (!el) {
    actions.setRaw(raw + snippet)
    return
  }
  const start = el.selectionStart ?? raw.length
  const end = el.selectionEnd ?? start
  actions.setRaw(raw.slice(0, start) + snippet + raw.slice(end))
  // DOM 的值下一帧才更新，光标位置也要等到那时再设置。
  requestAnimationFrame(() => {
    const caret = start + snippet.length
    el.setSelectionRange(caret, caret)
    el.focus()
  })
}

function markdownFor(file: File, url: string): string {
  const alt = file.name.replace(/\.[^.]+$/, '') || '图片'
  return file.type.startsWith('image/') ? `![${alt}](${url})` : `[${file.name}](${url})`
}

async function insertFiles(files: File[]) {
  for (const file of files) {
    const url = await actions.saveAsset(file)
    if (url) insertAtCursor(markdownFor(file, url))
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
</script>

<template>
  <section v-if="store.currentSource" class="editor">
    <header class="editor__bar">
      <strong>{{ store.currentSource }}</strong>
      <span class="editor__spacer" />
      <span class="editor__hint">保存后将重新生成 {{ affected }} / {{ total }} 个页面</span>
      <button type="button" :disabled="store.busy" @click="actions.saveContent()">保存</button>
      <button type="button" :disabled="store.busy" @click="actions.refreshPreview()">刷新预览</button>
    </header>

    <textarea
      ref="textarea"
      class="editor__area"
      :class="{ 'editor__area--dragging': dragging }"
      spellcheck="false"
      :value="store.currentRaw"
      aria-label="内容源文"
      @input="actions.setRaw(($event.target as HTMLTextAreaElement).value)"
      @paste="onPaste"
      @dragover.prevent="dragging = true"
      @dragleave="dragging = false"
      @drop="onDrop"
    />

    <footer class="editor__foot">粘贴或拖入图片即可插入，文件按内容哈希存入站点资源目录</footer>
  </section>

  <section v-else class="editor editor--empty">
    <p>从左侧选择一篇内容开始编辑。</p>
  </section>
</template>
