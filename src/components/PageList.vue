<script setup lang="ts">
/** 内容树：按栏目分组列出内容页，支持过滤、新建，并标出待重新生成的页面。 */
import { computed, ref } from 'vue'

import { actions, isDirty, store } from '../store'
import type { PageSummary } from '../api'

const filter = ref('')

const groups = computed(() => {
  const keyword = filter.value.trim().toLowerCase()
  const map = new Map<string, PageSummary[]>()
  for (const page of store.project?.pages ?? []) {
    if (keyword && !`${page.title}\n${page.source}`.toLowerCase().includes(keyword)) continue
    const key = page.section || '根目录'
    const list = map.get(key) ?? []
    list.push(page as PageSummary)
    map.set(key, list)
  }
  return [...map.entries()].sort(([a], [b]) => a.localeCompare(b))
})

const matched = computed(() => groups.value.reduce((sum, [, pages]) => sum + pages.length, 0))
const dirtyPages = computed(() => new Set(store.plan?.pages ?? []))

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
      <input v-model="filter" type="search" placeholder="过滤标题或路径" aria-label="过滤内容" />
      <button type="button" :title="creating ? '取消' : '新建内容'" @click="creating = !creating">
        {{ creating ? '×' : '＋' }}
      </button>
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
            @click="actions.requestOpenContent(page)"
          >
            <span class="page-list__title">{{ page.title }}</span>
            <span
              v-if="store.currentSource === page.source && isDirty"
              class="badge badge--unsaved"
              title="有未保存改动"
              >●</span
            >
            <span v-else-if="page.draft" class="badge badge--draft">草稿</span>
            <span
              v-if="dirtyPages.has(page.source)"
              class="badge badge--dirty"
              title="待重新生成"
              >●</span
            >
          </button>
        </li>
      </ul>
    </div>

    <p v-if="filter && matched === 0" class="page-list__empty">没有匹配的内容</p>
  </nav>
</template>
