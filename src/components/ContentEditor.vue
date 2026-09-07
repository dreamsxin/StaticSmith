<script setup lang="ts">
/**
 * 内容编辑区。
 *
 * 编辑的是 Markdown 源文（含 `+++` front matter），保存后由 Rust 侧
 * 重新解析并给出增量构建计划。
 */
import { computed } from 'vue'

import { actions, store } from '../store'

const affected = computed(() => store.plan?.pages.length ?? 0)
const total = computed(() => store.plan?.total_pages ?? 0)
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
      class="editor__area"
      spellcheck="false"
      :value="store.currentRaw"
      aria-label="内容源文"
      @input="actions.setRaw(($event.target as HTMLTextAreaElement).value)"
    />
  </section>

  <section v-else class="editor editor--empty">
    <p>从左侧选择一篇内容开始编辑。</p>
  </section>
</template>
