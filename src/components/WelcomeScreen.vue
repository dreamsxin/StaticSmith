<script setup lang="ts">
/** 未打开项目时的起始页：新建或打开一个站点目录。 */
import { ref } from 'vue'
import { open } from '@tauri-apps/plugin-dialog'

import { actions } from '../store'
import { isProject } from '../api'

const title = ref('我的静态站')
const hint = ref('')

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

    <p v-if="hint" class="welcome__hint">{{ hint }}</p>
    <p class="welcome__note">
      新建时会写入 <code>templates/</code>、<code>themes/default/</code>、
      <code>content/</code> 与 <code>staticsmith.toml</code>，已存在的文件不会被覆盖。
    </p>
  </section>
</template>
