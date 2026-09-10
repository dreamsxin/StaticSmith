<script setup lang="ts">
/**
 * 「新建站点」的表单本体：站点名称 + 版式。
 *
 * 单独抽出来是因为有两个宿主——起始页的卡片（内嵌）和菜单栏弹出的对话框。
 * 各写一份的话，加一个字段就得改两处，而漏改的那一处不会报错、只会静静地
 * 少一个输入框。上一轮「菜单入口没有版式选择」就是这么来的。
 *
 * 目录不在这里选：那一步要调系统对话框，由宿主决定何时弹（起始页是点按钮，
 * 对话框是提交表单）。这个组件只负责收集填写内容。
 */
import { onMounted, ref } from 'vue'

import { actions, store } from '../store'

const props = defineProps<{
  /** 提交按钮上的字。两处措辞不同：起始页强调「选目录」，对话框强调「创建」。 */
  submitLabel: string
}>()

const emit = defineEmits<{ submit: [title: string, preset: string] }>()

const title = ref('我的静态站')
const preset = ref('')
const titleBox = ref<HTMLInputElement | null>(null)

onMounted(async () => {
  await actions.loadPresets()
  // 第一项即默认值，与 Rust 侧 `Preset::ALL[0]` 一致。
  preset.value = store.presets[0]?.slug ?? ''
})

/** 宿主打开对话框后要把焦点放进来。 */
defineExpose({ focus: () => titleBox.value?.focus() })

function submit() {
  const name = title.value.trim()
  if (!name) return
  emit('submit', name, preset.value)
}
</script>

<template>
  <!-- 用 form 而不是一堆裸 input：回车即提交，键盘用户不必找按钮 -->
  <form class="new-site" @submit.prevent="submit">
    <label class="card__field">
      站点名称
      <input ref="titleBox" v-model="title" type="text" placeholder="我的静态站" />
    </label>

    <!-- 选一套版式。用单选而不是下拉：只有两三项，且每项都需要一句说明，
         收进下拉之后要点开才看得到「长什么样」。 -->
    <fieldset v-if="store.presets.length > 1" class="presets">
      <legend>版式</legend>
      <label v-for="item in store.presets" :key="item.slug" class="preset">
        <input v-model="preset" type="radio" name="preset" :value="item.slug" />
        <span class="preset__title">{{ item.title }}</span>
        <span class="preset__desc">{{ item.description }}</span>
      </label>
    </fieldset>

    <button type="submit" class="btn--primary" :disabled="store.busy || !title.trim()">
      {{ props.submitLabel }}
    </button>
  </form>
</template>
