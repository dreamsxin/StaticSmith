<script setup lang="ts">
/**
 * 跨文件替换：改一个称呼、统一一个术语。以前只能逐篇点开改。
 *
 * 从 `PageList.vue` 抽出来的第二块（第一块是栏目头）。抽的理由同上一次：
 * 它有一个「先干跑再落盘」的确认态，而确认态的出口是最容易漏的地方——
 * 而且这个动作**没有撤销栈**，改错一个词不会报错，只会安静地把内容改坏。
 *
 * 与「多选」共用一个「范围」概念：开着多选并选了几篇时能只改那几篇，
 * 所以选中集合由上层传进来（`selected`），而不是这里自己去读。
 *
 * 只改正文，front matter 不在范围内（理由见 `staticsmith_core::replace`），
 * 界面上必须说出来，否则用户会以为标题里的词也一起换了。
 */
import { computed, nextTick, ref, watch } from 'vue'

import type { ReplaceResult } from '../api'
import { actions, store } from '../store'

const props = defineProps<{
  /** 是否展开。互斥（与新建内容、新建栏目）由上层保证。 */
  open: boolean
  /** 多选里勾中的源路径。空数组表示没勾——那时只能全站替换。 */
  selected: readonly string[]
}>()

const emit = defineEmits<{ close: [] }>()

const findText = ref('')
const replaceText = ref('')
const ignoreCase = ref(false)
const onlySelected = ref(false)
const findBox = ref<HTMLInputElement | null>(null)

/** 干跑结果。为 null 表示还没预览过——没预览过不给按「替换」。 */
const preview = ref<ReplaceResult | null>(null)

/** 实际范围：不勾「只改选中的」就是全站（空数组）。 */
const scope = computed(() => (onlySelected.value ? [...props.selected] : []))

watch(
  () => props.open,
  async (open) => {
    if (!open) {
      preview.value = null
      return
    }
    // 展开即聚焦到「查找」，少一次点击。
    // 用 `nextTick` 而不是 `requestAnimationFrame`：等的是「这次渲染提交完」，
    // 而这正是 nextTick 的语义；rAF 只是碰巧排在它之后，且在测试里要多等一帧。
    await nextTick()
    findBox.value?.focus()
  },
)

/**
 * 改了任一条件，之前那份干跑结果就不再对应当前输入，作废掉。
 *
 * 少了这一条最难看：把「查找」改成别的词之后，屏幕上还留着上一次的清单，
 * 而「替换这 N 处」按的是新条件——用户以为自己确认的是看到的那份。
 */
watch([findText, replaceText, ignoreCase, onlySelected, () => props.selected], () => {
  preview.value = null
})

async function runPreview() {
  const result = await actions.previewReplace({
    find: findText.value,
    replace: replaceText.value,
    ignore_case: ignoreCase.value,
    sources: scope.value,
  })
  if (result) preview.value = result
}

async function confirm() {
  await actions.applyReplace({
    find: findText.value,
    replace: replaceText.value,
    ignore_case: ignoreCase.value,
    sources: scope.value,
  })
  emit('close')
}
</script>

<template>
  <form v-if="props.open" class="page-list__new" @submit.prevent="runPreview">
    <h3>跨文件替换</h3>
    <label>
      查找
      <input ref="findBox" v-model="findText" type="text" placeholder="要被换掉的文字" />
    </label>
    <label>
      替换为
      <input v-model="replaceText" type="text" placeholder="留空即删掉这个词" />
    </label>
    <label class="page-list__keep">
      <input v-model="ignoreCase" type="checkbox" />
      忽略大小写
    </label>
    <label class="page-list__keep">
      <input v-model="onlySelected" type="checkbox" :disabled="!props.selected.length" />
      只改选中的 {{ props.selected.length }} 篇（不勾就是全站）
    </label>
    <p class="page-list__hint">
      只改正文。标题、标签这些 front matter 字段不会动——那些用「多选」里的批量动作或
      属性面板改。不支持正则。
    </p>

    <div class="page-list__batch-row">
      <button type="submit" :disabled="store.busy || !findText">预览…</button>
      <button type="button" @click="emit('close')">取消</button>
    </div>

    <!-- 干跑结果：正文替换没有撤销，先看清「哪几篇、哪几行」 -->
    <div v-if="preview" class="page-list__dry">
      <p class="page-list__batch-head">{{ preview.files.length }} 篇、共 {{ preview.hits }} 处</p>
      <p v-if="!preview.hits" class="page-list__hint">没有找到这段文字。</p>
      <ul class="page-list__dry-list">
        <li v-for="file in preview.files" :key="file.source">
          <code>{{ file.source }}</code>
          <span>{{ file.hits }} 处</span>
          <ul class="page-list__dry-lines">
            <li v-for="line in file.lines" :key="line.line">
              <span class="page-list__dry-no">第 {{ line.line }} 行</span>
              <del>{{ line.before }}</del>
              <ins>{{ line.after }}</ins>
            </li>
            <li v-if="file.hits > file.lines.length" class="skip">
              另有 {{ file.hits - file.lines.length }} 处未列出
            </li>
          </ul>
        </li>
        <li v-for="item in preview.skipped" :key="item.source" class="skip">
          <code>{{ item.source }}</code>
          <span>{{ item.reason }}</span>
        </li>
      </ul>
      <div class="page-list__batch-row">
        <button
          type="button"
          class="btn--primary"
          :disabled="store.busy || !preview.hits"
          @click="confirm"
        >
          替换这 {{ preview.hits }} 处
        </button>
      </div>
    </div>
  </form>
</template>
