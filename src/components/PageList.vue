<script setup lang="ts">
/** 内容树：按栏目分组列出内容页，标出待重新生成的页面，并支持新建。 */
import { computed, ref } from 'vue'

import { actions, store } from '../store'
import type { PageSummary } from '../api'

const groups = computed(() => {
  const map = new Map<string, PageSummary[]>()
  for (const page of store.project?.pages ?? []) {
    const key = page.section || '根目录'
    const list = map.get(key) ?? []
    list.push(page as PageSummary)
    map.set(key, list)
  }
  return [...map.entries()].sort(([a], [b]) => a.localeCompare(b))
})

const dirty = computed(() => new Set(store.plan?.pages ?? []))

const creating = ref(false)
const newTitle = ref('')
const newSection = ref('posts')

async function create() {
  if (!newTitle.value.trim()) return
  await actions.createContent(newTitle.value.trim(), newSection.value.trim())
  newTitle.value = ''
  creating.value = false
}
</script>

<template>
  <nav class="page-list">
    <div class="page-list__toolbar">
      <button type="button" @click="creating = !creating">{{ creating ? '取消' : '新建内容' }}</button>
    </div>

    <form v-if="creating" class="page-list__new" @submit.prevent="create">
      <label>
        标题
        <input v-model="newTitle" type="text" placeholder="文章标题" />
      </label>
      <label>
        栏目
        <input v-model="newSection" type="text" placeholder="posts（留空为根目录）" />
      </label>
      <button type="submit" :disabled="store.busy || !newTitle.trim()">创建草稿</button>
    </form>

    <div v-for="[section, pages] in groups" :key="section" class="page-list__group">
      <h3>{{ section }}</h3>
      <ul>
        <li v-for="page in pages" :key="page.source">
          <button
            type="button"
            :class="{ active: store.currentSource === page.source }"
            @click="actions.openContent(page)"
          >
            <span class="page-list__title">{{ page.title }}</span>
            <span v-if="page.draft" class="badge badge--draft">草稿</span>
            <span v-if="dirty.has(page.source)" class="badge badge--dirty" title="待重新生成">●</span>
          </button>
        </li>
      </ul>
    </div>
  </nav>
</template>
