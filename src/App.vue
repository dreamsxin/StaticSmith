<script setup lang="ts">
/** 应用外框：顶栏 + 工作区标签 + 状态栏。 */
import { ref } from 'vue'

import BuildPanel from './components/BuildPanel.vue'
import ContentEditor from './components/ContentEditor.vue'
import DeployPanel from './components/DeployPanel.vue'
import LayoutManager from './components/LayoutManager.vue'
import PageList from './components/PageList.vue'
import PreviewPane from './components/PreviewPane.vue'
import SettingsPanel from './components/SettingsPanel.vue'
import WelcomeScreen from './components/WelcomeScreen.vue'
import { actions, store } from './store'

type Tab = 'content' | 'layouts' | 'build' | 'deploy' | 'settings'

const tab = ref<Tab>('content')

const tabs: Array<{ id: Tab; label: string }> = [
  { id: 'content', label: '内容' },
  { id: 'layouts', label: '布局管理器' },
  { id: 'build', label: '生成' },
  { id: 'deploy', label: '发布' },
  { id: 'settings', label: '设置' },
]
</script>

<template>
  <WelcomeScreen v-if="!store.project" />

  <div v-else class="app">
    <header class="app__header">
      <strong>{{ store.project.config.site.title }}</strong>
      <code class="app__root">{{ store.project.root }}</code>
      <nav class="app__tabs">
        <button
          v-for="item in tabs"
          :key="item.id"
          type="button"
          :class="{ active: tab === item.id }"
          @click="tab = item.id"
        >
          {{ item.label }}
        </button>
      </nav>
      <span class="app__spacer" />
      <button type="button" @click="actions.closeProject()">关闭项目</button>
    </header>

    <p v-if="store.externalChange" class="app__banner">
      检测到磁盘上的模板或内容被外部修改。
      <button type="button" @click="actions.refresh()">刷新组件树</button>
    </p>

    <p v-if="store.error" class="app__error">
      {{ store.error }}
      <button type="button" @click="actions.dismissError()">知道了</button>
    </p>

    <main class="app__body">
      <template v-if="tab === 'content'">
        <PageList />
        <ContentEditor />
        <PreviewPane />
      </template>
      <LayoutManager v-else-if="tab === 'layouts'" />
      <BuildPanel v-else-if="tab === 'build'" />
      <DeployPanel v-else-if="tab === 'deploy'" />
      <SettingsPanel v-else />
    </main>

    <footer class="app__status">
      <span v-if="store.busy">处理中…</span>
      <span v-else-if="store.progress">{{ store.progress }}</span>
      <span v-else>就绪</span>
      <span class="app__spacer" />
      <span v-if="store.plan">
        待生成 {{ store.plan.pages.length }} / {{ store.plan.total_pages }}
      </span>
    </footer>
  </div>
</template>
