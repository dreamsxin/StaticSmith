<script setup lang="ts">
/**
 * 文章列表里的一行。
 *
 * 从 `PageList.vue` 抽出来的第五块。抽它的第一目的是**能测**：它长在 `v-for` 里，
 * 与选择态、筛选态、右键菜单缠在一起，此前是整个界面里点得最多、却唯一没有测试的地方。
 *
 * 分工：
 *
 * - 行内的**样子**（徽标优先级、确认态换按钮）在这里
 * - 行外的**决定**（菜单里放哪些动作、哪一行处于确认态、选了哪几篇）留在 `PageList`
 *   ——菜单项要调 `actions` 也要读筛选态，搬进来只是把耦合换个地方藏
 *
 * 全局状态（在编辑哪一篇、有没有未保存改动、忙不忙）直接读 store，与
 * `BrokenList` 一致：这三件事每一行都要用，逐行经 props 传等于把同一个值抄 N 遍。
 */
import { isDirty, store } from '../store'

/** 一行要用到的字段。声明成只读结构：store 导出的状态是深只读的。 */
interface RowPage {
  readonly source: string
  readonly title: string
  readonly url: string
  readonly draft: boolean
  readonly scheduled?: boolean
  readonly is_index: boolean
  readonly date?: string | null
}

/** 这一篇的体检结果，没有问题时为 null。 */
interface RowSeo {
  readonly severity: string
  readonly messages: readonly string[]
}

const props = defineProps<{
  page: RowPage
  /** 多选态：勾选框只在这时出现 */
  selecting: boolean
  checked: boolean
  /** 待重新生成（在构建计划里） */
  dirty: boolean
  seo: RowSeo | null
  /** 处于删除确认态。哪一行在确认态由上层决定，一次只有一行 */
  confirming: boolean
}>()

const emit = defineEmits<{
  open: []
  toggle: []
  /** 右键或「⋯」：上层据此弹菜单（菜单项的构造留在上层） */
  menu: [event: MouseEvent]
  /** 确认态里按下「删除」 */
  remove: []
  /** 确认态里按下「取消」 */
  cancel: []
}>()
</script>

<template>
  <!-- 右键挂在整行上：行内任何位置都能唤出菜单，而「⋯」是它的可见孪生入口 -->
  <li @contextmenu="emit('menu', $event)">
    <input
      v-if="props.selecting"
      type="checkbox"
      class="page-list__pick"
      :checked="props.checked"
      :aria-label="`选择 ${props.page.title}`"
      @change="emit('toggle')"
    />
    <button
      type="button"
      :class="{ active: store.currentSource === props.page.source }"
      :title="props.page.source"
      @click="emit('open')"
    >
      <span class="page-list__title">{{ props.page.title }}</span>
      <!-- 徽标共用一个位置，所以有优先级：未保存 > 草稿 / 定时。
           未保存是唯一会丢东西的状态，被草稿盖住的代价最大 -->
      <span
        v-if="store.currentSource === props.page.source && isDirty"
        class="badge badge--unsaved"
        title="有未保存改动"
        >●</span
      >
      <span v-else-if="props.page.draft" class="badge badge--draft">草稿</span>
      <span
        v-else-if="props.page.scheduled"
        class="badge badge--draft"
        :title="`${props.page.date ?? ''} 到点后才进产物（站点开了定时发布）`"
        >定时</span
      >
      <!-- 栏目页要标出来：它在这份列表里长得跟文章一样，但日历不收它、
           「没写日期」清单也不列它。不标的话，用户只会得出「界面漏了一篇」 -->
      <span
        v-if="props.page.is_index"
        class="badge badge--section"
        title="栏目列表页：这个栏目的门面，不算「发出去的一篇」，不参与发布节奏与排期"
        >栏目页</span
      >
      <span
        v-if="props.seo"
        class="badge"
        :class="`badge--seo-${props.seo.severity}`"
        :title="props.seo.messages.join('\n')"
        >SEO</span
      >
      <span v-if="props.dirty" class="badge badge--dirty" title="待重新生成">●</span>
    </button>

    <!-- 确认态里「⋯」让位给这两颗按钮：三颗挤在一行按不准，而这时要按的只有这两颗。
         取消不受忙态影响（见 ui.md「忙态」）——退出确认永远得能退 -->
    <template v-if="props.confirming">
      <button
        type="button"
        class="page-list__danger"
        :disabled="store.busy"
        title="删除源文件，产物在下次生成时清理"
        @click="emit('remove')"
      >
        删除
      </button>
      <button
        type="button"
        class="page-list__icon page-list__confirm-cancel"
        @click="emit('cancel')"
      >
        取消
      </button>
    </template>
    <button
      v-else
      type="button"
      class="page-list__icon"
      title="更多动作（也可在这一行上右键）"
      aria-label="更多动作"
      @click="emit('menu', $event)"
    >
      ⋯
    </button>
  </li>
</template>
