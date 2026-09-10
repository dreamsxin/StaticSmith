<script setup lang="ts">
/**
 * 「新建站点」对话框。
 *
 * 菜单栏原先把每套版式摊成一条（「新建站点：极简文档站…」「…博客园风格博客…」），
 * 「站点」菜单顶上立刻多出 N 行，加一套版式就再多一行；而且菜单里没地方填站点名称，
 * 只能拿目录名凑。改成弹一次表单：菜单回到一条，字段与起始页那张卡片共用一个组件。
 *
 * 浮层的做法照 `CommandPalette`：同一套背景遮罩、点空白关闭、Esc 关闭、
 * 关闭后把焦点还给打开它的元素。这个界面里的浮层就该只有一种行为。
 */
import { nextTick, ref, watch } from 'vue'

import NewSiteForm from './NewSiteForm.vue'
import { store } from '../store'

const props = defineProps<{ open: boolean }>()
const emit = defineEmits<{
  close: []
  submit: [title: string, preset: string]
}>()

const form = ref<InstanceType<typeof NewSiteForm> | null>(null)

/** 打开前记住焦点在哪，关闭后还回去——理由同 `CommandPalette`。 */
let restoreFocus: HTMLElement | null = null

watch(
  () => props.open,
  async (open) => {
    if (!open) {
      restoreFocus?.focus()
      restoreFocus = null
      return
    }
    const before = document.activeElement
    restoreFocus = before instanceof HTMLElement ? before : null
    await nextTick()
    form.value?.focus()
  },
)

function submit(title: string, preset: string) {
  emit('close')
  emit('submit', title, preset)
}
</script>

<template>
  <div v-if="props.open" class="palette" @pointerdown.self="emit('close')">
    <div
      class="palette__box dialog"
      role="dialog"
      aria-modal="true"
      aria-labelledby="new-site-title"
      @keydown.esc.prevent="emit('close')"
    >
      <h2 id="new-site-title" class="dialog__title">新建站点</h2>
      <p class="dialog__desc">
        下一步会让你选一个<strong>空目录</strong>，然后在里面写入
        <code>templates/</code>、<code>themes/default/</code>、<code>content/</code> 与
        <code>staticsmith.toml</code>。已存在的文件不会被覆盖。
      </p>

      <NewSiteForm ref="form" submit-label="选择空目录并创建…" @submit="submit" />

      <button type="button" class="dialog__cancel" :disabled="store.busy" @click="emit('close')">
        取消
      </button>
    </div>
  </div>
</template>
