<script setup lang="ts">
/**
 * 应用外框：菜单栏 + 标识行 + 工作区标签 + 可拖拽分栏 + 状态栏。
 *
 * 五层各管一件事，参考 Word / WPS 的信息层级：
 * 菜单栏＝能做什么（命令），标识行＝在哪个站点 + 主动作，标签＝在看什么（视图），
 * 工作区＝干活，状态栏＝当前状态。界面状态在 `src/ui.ts`，命令表在 `src/commands.ts`。
 */
import { onMounted, onBeforeUnmount, computed } from 'vue'

import { focusSearch, saveCurrent } from './commands'
import AppMenu from './components/AppMenu.vue'
import BuildPanel from './components/BuildPanel.vue'
import CalendarPanel from './components/CalendarPanel.vue'
import CommandPalette from './components/CommandPalette.vue'
import ContentEditor from './components/ContentEditor.vue'
import ContextMenu from './components/ContextMenu.vue'
import DeployPanel from './components/DeployPanel.vue'
import LayoutManager from './components/LayoutManager.vue'
import PageList from './components/PageList.vue'
import PreviewPane from './components/PreviewPane.vue'
import SeoPanel from './components/SeoPanel.vue'
import SettingsPanel from './components/SettingsPanel.vue'
import ToastStack from './components/ToastStack.vue'
import WelcomeScreen from './components/WelcomeScreen.vue'
import { useSplit } from './composables/useSplit'
import { actions, isDirty, isTemplateDirty, store } from './store'
import { applyResponsive, tabGroups, tabHint, ui } from './ui'

const { listWidth, previewWidth, startDrag } = useSplit({
  key: 'staticsmith.split',
  list: 260,
  preview: 460,
})

/** 当前页的一句话说明：标签只有两个字，关系与用途放在这里说清。 */
const hint = computed(() => tabHint(ui.tab))

/**
 * 标识行的「正在编辑」：显示 Ctrl+S 这一下会存谁。
 *
 * 外观页看着模板时是模板，其余情况是文章——与 `saveTarget()` 的分派同一条判断，
 * 否则会出现「上面写着正在编辑文章、按下去存了模板」。
 */
const editing = computed(() => {
  if (ui.tab === 'layouts' && store.currentTemplate) {
    return { name: store.currentTemplate, dirty: isTemplateDirty.value }
  }
  if (store.currentSource) return { name: store.currentSource, dirty: isDirty.value }
  return null
})

/** 状态栏的「未保存」取并集：文章与模板可以同时挂着改动，藏掉任何一个都是说谎。 */
const anyDirty = computed(() => isDirty.value || isTemplateDirty.value)

/**
 * 内容页是三栏可调，其余面板占满整行。
 *
 * 「视图」菜单关掉的窗格直接从网格里去掉，而不是把宽度设成 0——`useSplit` 会把宽度
 * 持久化并在下次启动时夹回最小值，用宽度表达显隐迟早会打架。
 */
const bodyStyle = computed(() => {
  if (ui.tab !== 'content') return { gridTemplateColumns: 'minmax(0, 1fr)' }
  const columns = [
    ui.showList ? `${listWidth.value}px 6px` : '',
    'minmax(0, 1fr)',
    ui.showPreview ? `6px ${previewWidth.value}px` : '',
  ]
  return { gridTemplateColumns: columns.filter(Boolean).join(' ') }
})

/**
 * 全局快捷键。
 *
 * 全部挂在 window 上，而不是分散在各组件里：Ctrl+F 以前挂在列表栏组件上，
 * 停在别的标签页或把列表栏收起来时就按不出反应；Ctrl+S 以前只在 textarea 里生效，
 * 焦点落在属性面板的输入框上就存不了。快捷键的落点统一走 `commands.ts` 里的
 * `saveCurrent` / `focusSearch`，与菜单项、工具条按钮同一份判断。
 */
function onKeydown(event: KeyboardEvent) {
  if (!store.project || !(event.ctrlKey || event.metaKey)) return
  const key = event.key.toLowerCase()

  // Ctrl+P 而不是 Ctrl+K：后者在编辑器里是「插入链接」，抢走会更糟
  if (key === 'p') {
    event.preventDefault()
    ui.paletteOpen = !ui.paletteOpen
    return
  }
  if (key === 'enter') {
    event.preventDefault()
    void actions.build(event.shiftKey ? 'full' : 'incremental')
    return
  }
  if (key === 's') {
    // 没有改动时也吞掉：否则 WebView 会弹「保存网页」
    event.preventDefault()
    void saveCurrent()
    return
  }
  if (key === 'f') {
    event.preventDefault()
    focusSearch()
  }
}

function onResize() {
  applyResponsive(window.innerWidth)
}

onMounted(() => {
  window.addEventListener('keydown', onKeydown)
  window.addEventListener('resize', onResize)
  onResize()
})

onBeforeUnmount(() => {
  window.removeEventListener('keydown', onKeydown)
  window.removeEventListener('resize', onResize)
})
</script>

<template>
  <WelcomeScreen v-if="!store.project" />

  <div v-else class="app">
    <AppMenu />

    <header class="app__bar">
      <span class="app__mark" aria-hidden="true">◆</span>
      <div class="app__identity">
        <strong class="app__title">{{ store.project.config.site.title }}</strong>
        <code class="app__root" :title="store.project.root">{{ store.project.root }}</code>
      </div>
      <!-- 正在编辑哪一份要一直看得见：切到别的标签页后编辑器不在，光看状态栏的「● 未保存」
           不知道是谁没保存 -->
      <span v-if="editing" class="app__editing" :title="editing.name">
        正在编辑 <code>{{ editing.name }}</code>
        <span v-if="editing.dirty" class="app__status-dirty">●</span>
      </span>
      <span class="app__spacer" />
      <button type="button" @click="ui.paletteOpen = true">命令面板 <kbd>Ctrl+P</kbd></button>
      <button
        type="button"
        class="btn--primary"
        :disabled="store.busy"
        @click="actions.build('incremental')"
      >
        生成 <kbd>Ctrl+Enter</kbd>
      </button>
      <button type="button" @click="actions.closeProject()">关闭站点</button>
    </header>

    <nav class="app__tabs" aria-label="工作区">
      <div
        v-for="group in tabGroups"
        :key="group.label"
        class="app__tabs-cluster"
        role="group"
        :aria-label="group.label"
      >
        <span class="app__tabs-group" aria-hidden="true">{{ group.label }}</span>
        <button
          v-for="item in group.tabs"
          :key="item.id"
          type="button"
          class="app__tab"
          :class="{ active: ui.tab === item.id }"
          :aria-current="ui.tab === item.id ? 'page' : undefined"
          :title="item.hint"
          @click="ui.tab = item.id"
        >
          {{ item.label }}
        </button>
      </div>
      <span class="app__spacer" />
      <!-- 当前页说明并入标签栏：单独占一行会再吃掉 26px，而工作区已经很挤 -->
      <span class="app__tabs-hint">{{ hint }}</span>
    </nav>



    <p v-if="store.externalChange" class="app__banner">
      检测到磁盘上的模板或内容被外部修改。
      <button type="button" @click="actions.refresh()">刷新组件树</button>
    </p>

    <main class="app__body" :style="bodyStyle">
      <template v-if="ui.tab === 'content'">
        <PageList v-if="ui.showList" />
        <div
          v-if="ui.showList"
          class="splitter"
          title="拖动调整列表宽度"
          @pointerdown="startDrag('list', $event)"
        />
        <ContentEditor />
        <div
          v-if="ui.showPreview"
          class="splitter"
          title="拖动调整预览宽度"
          @pointerdown="startDrag('preview', $event)"
        />
        <PreviewPane v-if="ui.showPreview" />
      </template>
      <LayoutManager v-else-if="ui.tab === 'layouts'" />
      <CalendarPanel v-else-if="ui.tab === 'calendar'" @open="ui.tab = 'content'" />
      <BuildPanel v-else-if="ui.tab === 'build'" @preview="ui.tab = 'content'" />
      <SeoPanel v-else-if="ui.tab === 'audit'" @open="ui.tab = 'content'" />
      <DeployPanel v-else-if="ui.tab === 'deploy'" />
      <SettingsPanel v-else />
    </main>

    <footer class="app__status">
      <span v-if="store.busy">处理中…</span>
      <span v-else-if="store.progress">{{ store.progress }}</span>
      <span v-else>就绪</span>
      <!-- 「待生成」是级联更新的唯一实时体现，排在左侧视线起点；
           预览地址这类环境信息才靠右 -->
      <span v-if="store.plan" class="app__status-plan">
        待生成 {{ store.plan.pages.length }} / {{ store.plan.total_pages }}
      </span>
      <span v-if="anyDirty" class="app__status-dirty">● 未保存</span>
      <span class="app__spacer" />
      <span v-if="store.previewServer">预览 {{ store.previewServer }}</span>
    </footer>
  </div>

  <CommandPalette :open="ui.paletteOpen" @close="ui.paletteOpen = false" />
  <ContextMenu />
  <ToastStack />
</template>
