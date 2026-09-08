<script setup lang="ts">
/** 应用外框：顶栏 + 工作区标签 + 可拖拽分栏 + 状态栏。 */
import { onMounted, onBeforeUnmount, computed, ref } from 'vue'

import BuildPanel from './components/BuildPanel.vue'
import CalendarPanel from './components/CalendarPanel.vue'
import CommandPalette from './components/CommandPalette.vue'
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

type Tab = 'content' | 'calendar' | 'layouts' | 'audit' | 'build' | 'deploy' | 'settings'

const tab = ref<Tab>('content')
const paletteOpen = ref(false)

/**
 * 标签页按「一件事的先后」分四组，而不是平铺七个名词。
 *
 * 之前是一排看不出关系的标签（内容 / 布局管理器 / 日历 / 生成 / SEO / 发布 / 设置），
 * 新用户既不知道从哪开始，也不知道「生成」和「发布」是什么关系。现在分组读出来
 * 就是流程：**写什么 → 长什么样 → 上线前检查 → 送出去**，设置单独一组（一次性设定）。
 * 每组给一句话说明，选中哪个标签就显示对应那句，不必猜。
 */
const groups: Array<{ label: string; tabs: Array<{ id: Tab; label: string; hint: string }> }> = [
  {
    label: '写',
    tabs: [
      { id: 'content', label: '内容', hint: '写文章、管栏目：左边找、中间写、右边看最终效果' },
      { id: 'calendar', label: '日历', hint: '发布节奏：这个月发了几篇、下周排了什么、哪些还没写日期' },
    ],
  },
  {
    label: '外观',
    tabs: [
      {
        id: 'layouts',
        label: '外观',
        hint: '布局与组件的继承关系、改一处影响哪些页面；主题包的打包与装入也在这里',
      },
    ],
  },
  {
    label: '上线',
    tabs: [
      { id: 'audit', label: '体检', hint: 'SEO 字段、站内死链、媒体资源——上线前该修的都在这一页' },
      { id: 'build', label: '生成', hint: '把内容与模板渲染成 dist/ 里的静态文件（Ctrl+Enter 也可）' },
      { id: 'deploy', label: '发布', hint: '把 dist/ 送到 Git 或 FTP/SFTP；凭据存系统凭据管理器' },
    ],
  },
  {
    label: '站点',
    tabs: [
      {
        id: 'settings',
        label: '设置',
        hint: 'staticsmith.toml 的可视化表单：站点信息、构建、媒体、分类、导航、发布，以及一次性的内容导入',
      },
    ],
  },
]

const allTabs = computed(() => groups.flatMap((group) => group.tabs))

/** 当前页的一句话说明：标签只有两个字，关系与用途放在这里说清。 */
const hint = computed(() => allTabs.value.find((item) => item.id === tab.value)?.hint ?? '')

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

  // Ctrl+P 而不是 Ctrl+K：后者在编辑器里是「插入链接」，抢走会更糟
  if (key === 'p') {
    event.preventDefault()
    paletteOpen.value = !paletteOpen.value
    return
  }
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
      <button type="button" @click="paletteOpen = true">
        命令面板 <kbd>Ctrl+P</kbd>
      </button>
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

    <nav class="app__tabs" aria-label="工作区">
      <template v-for="(group, index) in groups" :key="group.label">
        <span v-if="index > 0" class="app__tabs-sep" aria-hidden="true" />
        <span class="app__tabs-group">{{ group.label }}</span>
        <button
          v-for="item in group.tabs"
          :key="item.id"
          type="button"
          class="app__tab"
          :class="{ active: tab === item.id }"
          :title="item.hint"
          @click="tab = item.id"
        >
          {{ item.label }}
        </button>
      </template>
    </nav>

    <p class="app__hint">{{ hint }}</p>


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
      <CalendarPanel v-else-if="tab === 'calendar'" @open="tab = 'content'" />
      <BuildPanel v-else-if="tab === 'build'" @preview="tab = 'content'" />
      <SeoPanel v-else-if="tab === 'audit'" @open="tab = 'content'" />
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

  <CommandPalette
    :open="paletteOpen"
    @close="paletteOpen = false"
    @navigate="(next) => (tab = next)"
  />
  <ToastStack />
</template>
