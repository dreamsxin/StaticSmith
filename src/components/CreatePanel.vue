<script setup lang="ts">
/**
 * 「新建内容」「新建栏目」两张表单。
 *
 * 从 `PageList.vue` 抽出来的第四块（前三块：栏目头、跨文件替换、批量动作条）。
 * 两张表单合成一个组件而不是两个：它们本来就互斥（同一块位置、同一套样式），
 * 分成两个组件的话「谁开着谁要关」这件事又要在上层写一遍——而上层原先正是漏了这一笔
 * （点「新建」不会收起已展开的「新建栏目」，两张表单会叠在一起）。
 * 用一个 `mode` 表达状态，互斥就是结构上的事实，不再靠调用方记得清。
 *
 * 栏目候选 `#known-sections` 那份 datalist 留在 `PageList` 里：批量移动也要用它，
 * 一份 datalist 服务多处，按 id 引用即可。
 */
import { nextTick, ref, watch } from 'vue'

import { actions, store } from '../store'

const props = defineProps<{
  /** 展开哪一张。null 表示都不展开。互斥（与跨文件替换）由上层保证。 */
  mode: 'content' | 'section' | null
  /**
   * 新建内容时预填的栏目。
   *
   * 栏目头上的「在此栏目新建文章…」就靠它落到对的栏目里；平时是上一次用过的那个。
   */
  defaultSection: string
}>()

const emit = defineEmits<{ close: [] }>()

const newTitle = ref('')
const newSection = ref(props.defaultSection)
/**
 * 手写源文件路径。
 *
 * 留空走「栏目 + 标题」推导，够日常用；但归档结构（`posts/2026/09/hello.md`）
 * 推导不出来，只能让人直接写。写了就以它为准，栏目退到一边——两个都参与推导
 * 会出现「栏目填 posts、路径填 notes/x.md」这种自相矛盾的输入。
 */
const newPath = ref('')
const titleBox = ref<HTMLInputElement | null>(null)

const newSectionPath = ref('')
const newSectionTitle = ref('')
/** 栏目简介。当场填掉，否则新栏目一建出来体检面板就多一条「缺描述」。 */
const newSectionDescription = ref('')
const sectionBox = ref<HTMLInputElement | null>(null)

/**
 * 把光标送回第一格。
 *
 * 上层在「表单已经展开着又被要求新建一次」时叫它：那时 `mode` 没有变化，watch 不会响，
 * 而按了 Ctrl+N 却什么都没动等于没反应。
 */
async function focusFirst() {
  await nextTick()
  if (props.mode === 'content') titleBox.value?.focus()
  else if (props.mode === 'section') sectionBox.value?.focus()
}

defineExpose({ focusFirst })

/**
 * 展开即聚焦，少一次点击。
 *
 * 用 `nextTick` 而不是 `requestAnimationFrame`：等的是「这次渲染提交完」，
 * 而这正是 nextTick 的语义（同 `ReplacePanel`）。
 */
watch(() => props.mode, focusFirst)

/**
 * 栏目只在上层换了意图时才覆盖（栏目头的「在此栏目新建文章…」），平时保持不动：
 * 连着往同一个栏目里写几篇是常态，每次展开都重置成默认值等于每次都要重填。
 */
watch(
  () => props.defaultSection,
  (section) => {
    newSection.value = section
  },
)



/**
 * 取消不清输入：误点一下不该把刚写的标题丢掉，再展开还在。
 * 建成之后才清——那时这些字已经落到磁盘上了，留着只会让下一篇接着上一篇写。
 */
async function create() {
  const title = newTitle.value.trim()
  if (!title) return
  await actions.createContent(title, newSection.value.trim(), newPath.value.trim())
  newTitle.value = ''
  newPath.value = ''
  emit('close')
}

async function createSection() {
  const path = newSectionPath.value.trim()
  if (!path) return
  await actions.createSection(
    path,
    newSectionTitle.value.trim(),
    newSectionDescription.value.trim(),
  )
  newSectionPath.value = ''
  newSectionTitle.value = ''
  newSectionDescription.value = ''
  emit('close')
}
</script>

<template>
  <form
    v-if="props.mode === 'section'"
    class="page-list__new"
    @submit.prevent="createSection"
  >
    <h3>新建栏目</h3>
    <label>
      目录名
      <input
        ref="sectionBox"
        v-model="newSectionPath"
        type="text"
        list="known-sections"
        placeholder="notes 或 posts/2026"
      />
    </label>
    <label>
      栏目标题
      <input v-model="newSectionTitle" type="text" placeholder="留空则用目录名" />
    </label>
    <label>
      栏目简介
      <input v-model="newSectionDescription" type="text" placeholder="一句话说明这个栏目写什么" />
    </label>
    <p class="page-list__hint">
      会同时生成索引页（index.md）——没有它，栏目列表页打不开。简介留空的话，体检面板会立刻记一条「缺描述」。
    </p>
    <div class="page-list__new-actions">
      <button type="submit" class="btn--primary" :disabled="store.busy || !newSectionPath.trim()">
        创建栏目
      </button>
      <button type="button" @click="emit('close')">取消</button>
    </div>
  </form>

  <form v-else-if="props.mode === 'content'" class="page-list__new" @submit.prevent="create">
    <h3>新建内容</h3>
    <label>
      标题
      <input ref="titleBox" v-model="newTitle" type="text" placeholder="文章标题" />
    </label>
    <label>
      栏目
      <input
        v-model="newSection"
        type="text"
        list="known-sections"
        placeholder="posts（留空为根目录）"
        :disabled="newPath.trim() !== ''"
      />
    </label>
    <label>
      路径（可选）
      <input v-model="newPath" type="text" placeholder="posts/2026/hello.md" />
    </label>
    <p class="page-list__new-hint">
      留空则按「栏目 + 标题」生成文件名。填了就完全按它落盘，栏目由路径本身决定；省略
      <code>.md</code> 会自动补上。
    </p>

    <div class="page-list__new-actions">
      <button type="submit" class="btn--primary" :disabled="store.busy || !newTitle.trim()">
        创建草稿
      </button>
      <button type="button" @click="emit('close')">取消</button>
    </div>
  </form>
</template>
