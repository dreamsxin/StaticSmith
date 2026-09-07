<script setup lang="ts">
/** 内容树：按栏目分组列出内容页，标出待重新生成的页面。 */
import { computed } from 'vue'

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
</script>

<template>
  <nav class="page-list">
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
