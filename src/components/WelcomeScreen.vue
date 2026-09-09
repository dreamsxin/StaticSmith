<script setup lang="ts">
/**
 * 未打开项目时的起始页。
 *
 * 布局按「先决定做什么，再决定用哪个站点」组织：左侧两张等宽卡片是两个入口，
 * 右侧是最近打开列表。之前入口是一个裸按钮加一组表单并排，高度不齐、
 * 对齐方式也和下方列表相反，看起来像临时拼的。
 */
import { onMounted, ref } from 'vue'
import { open } from '@tauri-apps/plugin-dialog'

import { isProject, listPresets, type PresetOption } from '../api'
import { actions, store } from '../store'

const title = ref('我的静态站')
const hint = ref('')

/**
 * 可选的模板预设。名字与说明来自 Rust 侧的 `Preset`，界面不抄一份——
 * 加一套预设只该改 core，不该同时改两处文案。
 */
const presets = ref<PresetOption[]>([])
const preset = ref('')

onMounted(async () => {
  await actions.loadRecent()
  presets.value = await listPresets()
  // 第一项即默认值，与 Rust 侧 `Preset::ALL[0]` 一致。
  preset.value = presets.value[0]?.slug ?? ''
})

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
  await actions.initProject(path, title.value, preset.value || undefined)
}

/** 只显示日期，起始页不需要精确到秒。 */
function shortDate(value: string): string {
  return value.slice(0, 10)
}
</script>

<template>
  <main class="welcome">
    <div class="welcome__sheet">
      <header class="welcome__brand">
        <h1>本地静站·工坊</h1>
        <p>可视化的自由，工业级的静态生成</p>
      </header>

      <div class="welcome__columns">
        <section class="welcome__col">
          <article class="card">
            <h2>打开已有站点</h2>
            <p class="card__desc">选择包含 <code>staticsmith.toml</code> 的目录。</p>
            <button type="button" class="btn--primary" :disabled="store.busy" @click="openExisting">
              选择目录…
            </button>
          </article>

          <article class="card">
            <h2>新建站点</h2>
            <p class="card__desc">
              在空目录里写入 <code>templates/</code>、<code>themes/default/</code>、
              <code>content/</code> 与 <code>staticsmith.toml</code>，已存在的文件不会被覆盖。
            </p>
            <label class="card__field">
              站点名称
              <input v-model="title" type="text" placeholder="我的静态站" />
            </label>

            <!-- 选一套版式。用单选而不是下拉：只有两三项，且每项都需要一句说明，
                 收进下拉之后要点开才看得到「长什么样」。 -->
            <fieldset v-if="presets.length > 1" class="welcome__presets">
              <legend>版式</legend>
              <label v-for="item in presets" :key="item.slug" class="welcome__preset">
                <input v-model="preset" type="radio" name="preset" :value="item.slug" />
                <span class="welcome__preset-title">{{ item.title }}</span>
                <span class="welcome__preset-desc">{{ item.description }}</span>
              </label>
            </fieldset>

            <button type="button" :disabled="store.busy || !title.trim()" @click="createNew">
              选择空目录…
            </button>
          </article>
        </section>

        <section class="welcome__col">
          <article class="card card--fill">
            <h2>最近打开</h2>
            <ul v-if="store.recent.length" class="welcome__recent">
              <li v-for="item in store.recent" :key="item.path">
                <button
                  type="button"
                  class="welcome__recent-open"
                  :disabled="store.busy"
                  @click="actions.openProject(item.path)"
                >
                  <span class="welcome__recent-title">{{ item.title || item.path }}</span>
                  <code>{{ item.path }}</code>
                </button>
                <time>{{ shortDate(item.opened_at) }}</time>
                <button
                  type="button"
                  class="welcome__recent-forget"
                  title="从列表移除（不删除磁盘上的站点）"
                  @click="actions.forgetRecent(item.path)"
                >
                  ×
                </button>
              </li>
            </ul>
            <p v-else class="card__desc">还没有记录。打开或新建一个站点后会出现在这里。</p>
          </article>
        </section>
      </div>

      <p v-if="hint || store.error" class="alert">{{ hint || store.error }}</p>
    </div>
  </main>
</template>
