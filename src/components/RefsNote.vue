<script setup lang="ts">
/**
 * 「这次改动会顺手改写别人文章里的链接」这两句话，只在这里写一遍。
 *
 * 改地址、搬动、栏目改名都会写到**用户没有点名的文件**上（那些引用它的文章），
 * 所以每条干跑都要报这两件事。它们原先在四个地方各写了一遍，措辞已经开始分叉：
 * 有的列文件名、有的不列，「改写不到」与「需要手工改」的顺序也不一样。
 * 同一件事在两个地方长得不一样，用户会以为是两件事。
 *
 * 两句分开而不是合成一句，因为它们对用户的要求不同：
 * 上面那句是**知会**（工具会替你改好），下面那句是**待办**（只有你能改）。
 *
 * 编辑器里改地址那一处**刻意没用这个组件**：它是嵌在一句话里的行内文字，
 * 不是独立段落，套过来会把紧凑的一行拆成两段。但措辞必须逐字一致——
 * `RefsNote.test.ts` 拿两边的源码对账钉着这一点。
 */
import type { RefUpdate } from '../api'

const props = defineProps<{
  /** 能自动改写的引用 */
  refs: readonly RefUpdate[]
  /** 改写不到、要人工处理的相对链接 */
  manual: readonly RefUpdate[]
  /**
   * 只报数，不列文件名。
   *
   * 「改了哪几篇」是用户判断「这次要不要按下去」的依据，所以默认列出来；
   * 地方窄的确认块可以打开这个开关。写成**反义**（`compact` 而不是 `names`）是因为
   * Vue 会把没传的布尔 prop 当成 `false`——正向写法的默认值会变成「不列名」，
   * 与「默认列」相反。
   */
  compact?: boolean
}>()

function hits(list: readonly RefUpdate[]): number {
  return list.reduce((sum, item) => sum + item.hits, 0)
}

function sources(list: readonly RefUpdate[]): string {
  return list.map((item) => item.source).join('、')
}
</script>

<template>
  <p v-if="props.refs.length" class="refs-note">
    另会把 {{ props.refs.length }} 篇里的 {{ hits(props.refs) }} 处站内链接改到新地址<template
      v-if="!props.compact"
      >：{{ sources(props.refs) }}</template
    >
  </p>
  <!-- 相对链接（`../a/`）按引用方所在目录解析，而改的是被引用方，所以改写改不到。
       以前这些只会在「死链体检」里出现——那是改完之后 -->
  <p v-if="props.manual.length" class="refs-note">
    另有 {{ props.manual.length }} 篇里的 {{ hits(props.manual) }}
    处<strong>相对链接</strong>（<code>../a/</code> 这类）指向它，改写不到，需要手工改<template
      v-if="!props.compact"
      >：{{ sources(props.manual) }}</template
    >
  </p>
</template>
