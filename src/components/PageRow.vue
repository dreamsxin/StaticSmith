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
import { dropSide } from '../grouping'
import type { Heading } from '../manuscript'
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

/** 目录里的一条：标题本身，加上「还能不能挪」。 */
interface OutlineEntry extends Heading {
  readonly canUp?: boolean
  readonly canDown?: boolean
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
  /** 正被拖着的就是这一行 */
  dragging?: boolean
  /** 落点指示线画在这一行的上边还是下边；`null` 表示这一行不是落点 */
  dropAt?: 'before' | 'after' | null
  /**
   * 这一篇的篇内标题，接在行下面。
   *
   * 只有**正在编辑的那一篇**会拿到它（见 `PageList`）：其余的要读盘才知道，
   * 一本两百章的书为了画目录去读两百个文件不划算。所以侧栏是
   * 「整本书的目录 + 这一章的细目」，而不是把全书标题都摊开。
   *
   * `canUp` / `canDown` 由上层算（它握着正文）：这一节还能不能挪。
   */
  outline?: readonly OutlineEntry[]
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
  /** 开始拖这一行 */
  dragstart: []
  /**
   * 拖着别人经过这一行，并告诉上层落点在上边还是下边。
   *
   * 中线判断留在这里：那是**这一行自己的几何**。上层只需要知道结论。
   */
  dragover: [side: 'before' | 'after']
  /** 在这一行上松手 */
  drop: []
  /** 拖动结束（松手或按 Esc 取消都会来） */
  dragend: []
  /** 点了篇内某个标题：把光标送到源文的这个位置 */
  jump: [offset: number]
  /** 把某一节整块上移 / 下移（`-1` / `1`） */
  moveHeading: [offset: number, delta: -1 | 1]
}>()

/**
 * 拖到这一行上时算前面还是后面。
 *
 * `preventDefault` 必须调：不调的话浏览器不认为这里能落，`drop` 事件根本不会来。
 */
function over(event: DragEvent) {
  event.preventDefault()
  const box = (event.currentTarget as HTMLElement).getBoundingClientRect()
  emit('dragover', dropSide(event.clientY, box.top, box.height))
}

</script>

<template>
  <!-- 右键挂在整行上：行内任何位置都能唤出菜单，而「⋯」是它的可见孪生入口。
       整行可拖：顺序是这一行的属性，抓哪儿都该能拖，不另设一个小抓手 -->
  <li
    draggable="true"
    :class="{
      'page-list__row--dragging': props.dragging,
      'page-list__row--drop-before': props.dropAt === 'before',
      'page-list__row--drop-after': props.dropAt === 'after',
    }"
    @contextmenu="emit('menu', $event)"
    @dragstart="emit('dragstart')"
    @dragover="over"
    @drop.prevent="emit('drop')"
    @dragend="emit('dragend')"
  >
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

    <!-- 篇内标题：正在编辑的那一篇把自己的目录摊在这里，点一条跳到那一节。
         `draggable=false`：拖标题不该挪动整篇文章的位次（那是行本身的动作）。
         层级用缩进表达，另标一个 H3 之类的小字给缩进兜底（同大纲浮层） -->
    <ul v-if="props.outline?.length" class="page-list__outline" draggable="false">
      <li v-for="(heading, index) in props.outline" :key="`${heading.offset}-${index}`">
        <button
          type="button"
          :class="`page-list__outline-item page-list__outline-item--h${heading.level}`"
          :title="`跳到「${heading.text}」`"
          @click="emit('jump', heading.offset)"
        >
          <span class="page-list__outline-level">H{{ heading.level }}</span>
          <span class="page-list__title">{{ heading.text }}</span>
        </button>
        <!-- 整节上移 / 下移：标题连同它下属的内容一起走。
             与行上的「⋯」一样悬停或聚焦才显形，免得每条标题后面都挂两个按钮 -->
        <button
          type="button"
          class="page-list__icon"
          :disabled="!heading.canUp"
          title="整节上移（与同级的上一节互换，含子标题与正文）"
          :aria-label="`把「${heading.text}」整节上移`"
          @click="emit('moveHeading', heading.offset, -1)"
        >
          ↑
        </button>
        <button
          type="button"
          class="page-list__icon"
          :disabled="!heading.canDown"
          title="整节下移（与同级的下一节互换，含子标题与正文）"
          :aria-label="`把「${heading.text}」整节下移`"
          @click="emit('moveHeading', heading.offset, 1)"
        >
          ↓
        </button>
      </li>
    </ul>
  </li>
</template>
