<script setup lang="ts">
/** 未打开项目时的起始页：从最近列表打开，或新建 / 选择一个站点目录。 */
import { onMounted, ref } from 'vue'
import { open } from '@tauri-apps/plugin-dialog'

import { isProject } from '../api'
import { actions, store } from '../store'

const title = ref('我的静态站')
const hint = ref('')

onMounted(() => actions.loadRecent())

async function pickDirectory(): Promise<string | null> {
  const selected = await open({ directory: true, multiple: false })
  return typeof selected === 'string' ? selected : null
}

async function openExisting() {
  const path = await pickDirectory()
  if (!path) return
  if (!(await isProject(path))) {
    hint.value = '该目录下没有 staticsmith.toml，请改用「新建站点」。'
    return
  }
  hint.value = ''
  await actions.openProject(path)
}

async function createNew() {
  const path = await pickDirectory()
  if (!path) return
  hint.value = ''
  await actions.initProject(path, title.value)
}

/** 只显示日期，起始页不需要精确到秒。 */
function shortDate(value: string): string {
  return value.slice(0, 10)
}
</script>

<template>
  <section class="welcome">
    <h1>本地静站·工坊</h1>
    <p class="welcome__slogan">可视化的自由，工业级的静态生成</p>

    <div class="welcome__actions">
      <button type="button" @click="openExisting">打开已有站点</button>
      <div class="welcome__create">
        <label>
          站点名称
          <input v-model="title" type="text" />
        </label>
        <button type="button" @click="createNew">在空目录新建站点</button>
      </div>
    </div>

    <div v-if="store.recent.length" class="welcome__recent">
      <h2>最近打开</h2>
      <ul>
        <li v-for="item in store.recent" :key="item.path">
          <button
            type="button"
            class="welcome__recent-open"
            :disabled="store.busy"
            @click="actions.openProject(item.path)"
          >
            <strong>{{ item.title || item.path }}</strong>
            <code>{{ item.path }}</code>
          </button>
          <button
            type="button"
            class="welcome__recent-forget"
            title="从列表移除（不删除磁盘上的站点）"
            @click="actions.forgetRecent(item.path)"
          >
            移除
          </button>
          <time>{{ shortDate(item.opened_at) }}</time>
        </li>
      </ul>
    </div>

    <p v-if="hint" class="welcome__hint">{{ hint }}</p>
    <p v-if="store.error" class="welcome__hint">{{ store.error }}</p>
    <p class="welcome__note">
      新建时会写入 <code>templates/</code>、<code>themes/default/</code>、
      <code>content/</code> 与 <code>staticsmith.toml</code>，已存在的文件不会被覆盖。
    </p>
  </section>
</template>
