<script setup lang="ts">
/**
 * 「读不出来」那一组：front matter 手改坏了的源文件。
 *
 * 这些文件不在页面清单里（解析不了就没有标题、地址、栏目），可它们**必须在界面里点得到**：
 * 只弹一条「有 2 篇读不出来」的通知，等于告诉用户「出事了，自己去文件管理器里找」。
 * 点开就是纯文本编辑，修好保存后 `refresh()` 会把这一组清空，那一篇回到自己的栏目里。
 *
 * 摆在栏目分组之前：它是要先处理的事——生成会因为它被拦下。
 */
import { actions, store } from '../store'

/** 与 `api.Skipped` 同形。store 是深只读的，所以这里也声明成只读。 */
interface BrokenFile {
  readonly source: string
  readonly reason: string
}

const props = defineProps<{ items: readonly BrokenFile[] }>()
</script>

<template>
  <div v-if="props.items.length" class="page-list__group page-list__group--broken">
    <h3>
      读不出来
      <span class="page-list__count">{{ props.items.length }}</span>
    </h3>
    <p class="page-list__hint">
      这几篇没有进内容清单，生成也会被拦下（产物里静默少一页更难查）。点一下当纯文本改，
      修好保存就回到各自的栏目里。
    </p>
    <ul>
      <li v-for="item in props.items" :key="item.source">
        <button
          type="button"
          :class="{ active: store.currentSource === item.source }"
          :title="`${item.source} —— ${item.reason}`"
          @click="actions.openBroken(item.source)"
        >
          <span class="page-list__title">{{ item.source }}</span>
          <span class="badge badge--seo-error">读不出</span>
        </button>
        <p class="page-list__snippet">{{ item.reason }}</p>
      </li>
    </ul>
  </div>
</template>
