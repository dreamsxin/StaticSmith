<script setup lang="ts">
/** 应用外框：顶栏 + 工作区标签 + 可拖拽分栏 + 状态栏。 */
import { onMounted, onBeforeUnmount, computed, ref } from 'vue'

import BuildPanel from './components/BuildPanel.vue'
import ContentEditor from './components/ContentEditor.vue'
import DeployPanel from './components/DeployPanel.vue'
import LayoutManager from './components/LayoutManager.vue'
import PageList from './components/PageList.vue'
import PreviewPane from './components/PreviewPane.vue'
import SeoPanel from './components/SeoPanel.vue'
import SettingsPanel from './components/SettingsPanel.vue'
import ToastStack from './components/ToastStack.vue'
import WelcomeScreen from './components/WelcomeScreen.vue'
import { useSplit } from './composables/useSplit'
import { actions, isDirty, store } from './store'

type Tab = 'content' | 'layouts' | 'build' | 'seo' | 'deploy' | 'settings'

const tab = ref<Tab>('content')

const tabs: Array<{ id: Tab; label: string }> = [
  { id: 'content', label: '内容' },
  { id: 'layouts', label: '布局管理器' },
  { id: 'build', label: '生成' },
  { id: 'seo', label: 'SEO' },
  { id: 'deploy', label: '发布' },
  { id: 'settings', label: '设置' },
]

const { listWidth, previewWidth, startDrag } = useSplit({
  key: 'staticsmith.split',
  list: 260,
  preview: 460,
})

/** 内容页是三栏可调；其余面板占满整行。 */
const bodyStyle = computed(() =>
  tab.value === 'content'
    ? {
        gridTemplateColumns: `${listWidth.value}px 6px minmax(0, 1fr) 6px ${previewWidth.value}px`,
      }
    : { gridTemplateColumns: 'minmax(0, 1fr)' },
)

/**
 * 全局快捷键。
 *
 * 编辑器内部的 Ctrl+S / Ctrl+B 由 textarea 自己处理（要操作选区），
 * 这里只管跟焦点无关的动作，并兜住焦点不在编辑器时的保存。
 */
function onKeydown(event: KeyboardEvent) {
  if (!store.project || !(event.ctrlKey || event.metaKey)) return
  const key = event.key.toLowerCase()

  if (key === 'enter') {
    event.preventDefault()
    void actions.build(event.shiftKey ? 'full' : 'incremental')
    return
  }
  if (key === 's' && !isDirty.value) {
    // 没有改动时按 Ctrl+S 也不该触发浏览器的保存页面
    event.preventDefault()
  }
}

onMounted(() => window.addEventListener('keydown', onKeydown))
onBeforeUnmount(() => window.removeEventListener('keydown', onKeydown))
</script>

<template>
  <WelcomeScreen v-if="!store.project" />

  <div v-else class="app">
    <header class="app__bar">
      <span class="app__mark" aria-hidden="true">◆</span>
      <div class="app__identity">
        <strong class="app__title">{{ store.project.config.site.title }}</strong>
        <code class="app__root" :title="store.project.root">{{ store.project.root }}</code>
      </div>
      <span class="app__spacer" />
      <button
        type="button"
        class="btn--primary"
        :disabled="store.busy"
        @click="actions.build('incremental')"
      >
        生成 <kbd>Ctrl+Enter</kbd>
      </button>
      <button type="button" @click="actions.closeProject()">关闭项目</button>
    </header>

    <nav class="app__tabs">
      <button
        v-for="item in tabs"
        :key="item.id"
        type="button"
        class="app__tab"
        :class="{ active: tab === item.id }"
        @click="tab = item.id"
      >
        {{ item.label }}
      </button>
    </nav>


    <p v-if="store.externalChange" class="app__banner">
      检测到磁盘上的模板或内容被外部修改。
      <button type="button" @click="actions.refresh()">刷新组件树</button>
    </p>

    <main class="app__body" :style="bodyStyle">
      <template v-if="tab === 'content'">
        <PageList />
        <div class="splitter" title="拖动调整列表宽度" @pointerdown="startDrag('list', $event)" />
        <ContentEditor />
        <div class="splitter" title="拖动调整预览宽度" @pointerdown="startDrag('preview', $event)" />
        <PreviewPane />
      </template>
      <LayoutManager v-else-if="tab === 'layouts'" />
      <BuildPanel v-else-if="tab === 'build'" @preview="tab = 'content'" />
      <SeoPanel v-else-if="tab === 'seo'" @open="tab = 'content'" />
      <DeployPanel v-else-if="tab === 'deploy'" />
      <SettingsPanel v-else />
    </main>

    <footer class="app__status">
      <span v-if="store.busy">处理中…</span>
      <span v-else-if="store.progress">{{ store.progress }}</span>
      <span v-else>就绪</span>
      <span v-if="isDirty" class="app__status-dirty">● 未保存</span>
      <span class="app__spacer" />
      <span v-if="store.previewServer">预览 {{ store.previewServer }}</span>
      <span v-if="store.plan">
        待生成 {{ store.plan.pages.length }} / {{ store.plan.total_pages }}
      </span>
    </footer>
  </div>

  <ToastStack />
</template>
