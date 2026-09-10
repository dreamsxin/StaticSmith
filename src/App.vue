<script setup lang="ts">
/**
 * 应用外框：菜单栏 + 标识行 + 工作区标签 + 可拖拽分栏 + 状态栏。
 *
 * 五层各管一件事，参考 Word / WPS 的信息层级：
 * 菜单栏＝能做什么（命令），标识行＝在哪个站点 + 主动作，标签＝在看什么（视图），
 * 工作区＝干活，状态栏＝当前状态。界面状态在 `src/ui.ts`，命令表在 `src/commands.ts`。
 */
import { onMounted, onBeforeUnmount, computed } from 'vue'

import { createSiteWith, focusSearch, saveCurrent } from './commands'

import AppMenu from './components/AppMenu.vue'
import BuildPanel from './components/BuildPanel.vue'
import CalendarPanel from './components/CalendarPanel.vue'
import CommandPalette from './components/CommandPalette.vue'
import ContentEditor from './components/ContentEditor.vue'
import ContextMenu from './components/ContextMenu.vue'
import DeployPanel from './components/DeployPanel.vue'
import LayoutManager from './components/LayoutManager.vue'
import NewSiteDialog from './components/NewSiteDialog.vue'
import PageList from './components/PageList.vue'

import PreviewPane from './components/PreviewPane.vue'
import SeoPanel from './components/SeoPanel.vue'
import SettingsPanel from './components/SettingsPanel.vue'
import ToastStack from './components/ToastStack.vue'
import WelcomeScreen from './components/WelcomeScreen.vue'
import { useSplit } from './composables/useSplit'
import { actions, isDirty, isTemplateDirty, store } from './store'
import { applyResponsive, allTabs, modeSpec, restoreLayout, tabGroups, tabHint, ui } from './ui'

const { listWidth, previewWidth, startDrag, nudge, jump, bounds } = useSplit({
  key: 'staticsmith.split',
  list: 260,
  preview: 460,
})

/**
 * 分隔条的键盘操作。
 *
 * `role="separator"` + `tabindex=0` 之后它才是个能被 Tab 走到的控件；
 * 没有这段，键盘用户既改不了分栏，也完全感知不到它存在。
 * 方向键 16px、Shift 64px、`Home` / `End` 到两端，判断与拖拽共用 `useSplit`。
 */
function onSplitterKey(side: 'list' | 'preview', event: KeyboardEvent) {
  switch (event.key) {
    case 'ArrowLeft':
      nudge(side, -1, event.shiftKey)
      break
    case 'ArrowRight':
      nudge(side, 1, event.shiftKey)
      break
    case 'Home':
      jump(side, 'min')
      break
    case 'End':
      jump(side, 'max')
      break
    default:
      return
  }
  event.preventDefault()
}

/**
 * 标签栏的方向键。
 *
 * 标签栏现在是真正的 `tablist`：整条只占一个 Tab 停靠点（roving tabindex），
 * 页间移动用方向键。七个视图各占一个 Tab 停靠点的话，键盘用户每次想进工作区
 * 都要按七下。
 *
 * 采用「焦点即选中」：切视图是纯导航、代价很低，多按一次回车只是多一步。
 */
function onTabKey(event: KeyboardEvent) {
  const index = allTabs.findIndex((item) => item.id === ui.tab)
  const last = allTabs.length - 1
  let next: number
  switch (event.key) {
    case 'ArrowRight':
      next = index >= last ? 0 : index + 1
      break
    case 'ArrowLeft':
      next = index <= 0 ? last : index - 1
      break
    case 'Home':
      next = 0
      break
    case 'End':
      next = last
      break
    default:
      return
  }
  event.preventDefault()
  ui.tab = allTabs[next].id
  // 选中态换了，roving tabindex 也跟着换，焦点要手动搬到新的那颗，
  // 否则它留在一个 tabindex=-1 的按钮上，再按方向键就没反应了
  requestAnimationFrame(() => document.getElementById(`tab-${ui.tab}`)?.focus())
}


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

  // Ctrl+P 而不是 Ctrl+K：后者在编辑器里是「插入链接」，抢走会更糟。
  // 语义是「打开并聚焦」而不是开关：标识行的按钮、视图菜单也都是打开，
  // 三处一致；关闭统一用 Esc。开关式会让「面板已开着时点按钮没反应」看起来像按钮坏了。
  if (key === 'p') {
    event.preventDefault()
    ui.paletteOpen = true
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
  // 布局先恢复再算响应式：标准模式要按当前窗口宽度决定窗格，
  // 另两种模式自己接管显隐，restoreLayout 里已经写明
  restoreLayout()
  // 版式列表在这里取而不是在起始页取：菜单栏的「新建站点」在项目已打开时也要能用，
  // 那时起始页没挂载。版式是编译进程序的，取一次就够。
  void actions.loadPresets()
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

    <!-- 真正的 tablist：整条只占一个 Tab 停靠点，页间移动用方向键。
         分组容器只能是 presentation——ARIA 规定 tablist 的子节点除 tab 之外
         只能是无语义包装，组名（写／外观／上线／站点）本来就是给眼睛的装饰 -->
    <nav class="app__tabs" role="tablist" aria-label="工作区" @keydown="onTabKey">
      <div v-for="group in tabGroups" :key="group.label" class="app__tabs-cluster" role="presentation">
        <span class="app__tabs-group" aria-hidden="true">{{ group.label }}</span>
        <button
          v-for="item in group.tabs"
          :id="`tab-${item.id}`"
          :key="item.id"
          type="button"
          role="tab"
          class="app__tab"
          :class="{ active: ui.tab === item.id }"
          :aria-selected="ui.tab === item.id"
          :tabindex="ui.tab === item.id ? 0 : -1"
          aria-controls="app-workspace"
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

    <main
      id="app-workspace"
      class="app__body"
      role="tabpanel"
      :aria-labelledby="`tab-${ui.tab}`"
      :style="bodyStyle"
    >
      <template v-if="ui.tab === 'content'">
        <PageList v-if="ui.showList" />
        <!-- 分隔条是控件而不是装饰：能被 Tab 走到，方向键调宽，读屏念得出当前值 -->
        <div
          v-if="ui.showList"
          class="splitter"
          role="separator"
          tabindex="0"
          aria-orientation="vertical"
          aria-label="列表栏宽度（方向键调整，Shift 加速）"
          :aria-valuenow="listWidth"
          :aria-valuemin="bounds('list').min"
          :aria-valuemax="bounds('list').max"
          title="拖动或用方向键调整列表宽度"
          @pointerdown="startDrag('list', $event)"
          @keydown="onSplitterKey('list', $event)"
        />
        <ContentEditor />
        <div
          v-if="ui.showPreview"
          class="splitter"
          role="separator"
          tabindex="0"
          aria-orientation="vertical"
          aria-label="预览栏宽度（方向键调整，Shift 加速）"
          :aria-valuenow="previewWidth"
          :aria-valuemin="bounds('preview').min"
          :aria-valuemax="bounds('preview').max"
          title="拖动或用方向键调整预览宽度"
          @pointerdown="startDrag('preview', $event)"
          @keydown="onSplitterKey('preview', $event)"
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
      <!-- 当前模式属于环境信息，靠右；状态栏只报告状态，切换走「视图」菜单 -->
      <span class="app__status-mode" :title="modeSpec(ui.mode).hint">
        {{ modeSpec(ui.mode).label }}模式
      </span>
      <span v-if="store.previewServer">预览 {{ store.previewServer }}</span>
    </footer>
  </div>

  <CommandPalette :open="ui.paletteOpen" @close="ui.paletteOpen = false" />
  <NewSiteDialog
    :open="ui.newSiteOpen"
    @close="ui.newSiteOpen = false"
    @submit="createSiteWith"
  />

  <ContextMenu />
  <ToastStack />
</template>
