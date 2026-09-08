<script setup lang="ts">
/**
 * 外观页：查看布局 → 组件的继承关系、直接编辑全局共享组件，以及整套外观的打包与装入。
 *
 * 保存组件后立即展示级联影响范围，对应产品文档 4.2 的「触发更新」提示。
 * 主题包放在这一页而不是设置页：它换的就是这一页列出的那些模板与主题资源，
 * 而设置页管的是 `staticsmith.toml`，两件事不该混在一起。
 */
import { computed, reactive, ref, watch } from 'vue'
import { open, save as saveDialog } from '@tauri-apps/plugin-dialog'

import { templateTree } from '../api'
import type { TemplateInfo, TemplateNode, ThemePreview } from '../api'
import { actions, store } from '../store'
import { highlight } from '../template-highlight'

const tree = ref<TemplateNode | null>(null)
const selectedLayout = ref<string>('')
const editor = ref<HTMLTextAreaElement | null>(null)
const mirror = ref<HTMLElement | null>(null)

/** 模板着色与内容编辑器同一套机制，只是规则换成 HTML + Tera。 */
const highlighted = computed(() => highlight(store.currentTemplateSource))

function syncScroll() {
  const el = editor.value
  const box = mirror.value
  if (!el || !box) return
  box.scrollTop = el.scrollTop
  box.scrollLeft = el.scrollLeft
}


const layouts = computed(() => store.project?.layouts ?? [])
const components = computed(() => store.project?.components ?? [])
const pageTemplates = computed(
  () => store.project?.templates.filter((t) => t.kind === 'page') ?? [],
)

watch(
  layouts,
  (list) => {
    if (!selectedLayout.value && list.length > 0) selectedLayout.value = list[0].name
  },
  { immediate: true },
)

watch(selectedLayout, async (name) => {
  tree.value = name ? await templateTree(name) : null
})

function flatten(node: TemplateNode, depth = 0): Array<{ node: TemplateNode; depth: number }> {
  return [{ node, depth }, ...node.children.flatMap((child) => flatten(child, depth + 1))]
}

const flatTree = computed(() => (tree.value ? flatten(tree.value) : []))

// ---------------------------------------------------------------- 主题包

/**
 * 主题包：把这一页管的全部外观打成 zip，或装一个别人做好的。
 *
 * 与命令行 `staticsmith theme export` / `import` 同一套逻辑，界面「先扫后装」：
 * 装一次会重写十几个模板，看清哪些是覆盖再落盘。包里只有外观，
 * `content/` 与 `static/` 不进包也不会被写。
 */
const themeMeta = reactive({ name: '', version: '', author: '', description: '' })
const themeArchive = ref('')
const themePreview = ref<ThemePreview | null>(null)
const themeOverwrite = ref(false)

/** 装入会写的文件数：不覆盖时排掉冲突的那些。 */
const themeWillWrite = computed(() => {
  const preview = themePreview.value
  if (!preview) return 0
  return themeOverwrite.value
    ? preview.files.length
    : preview.files.length - preview.conflicts.length
})

async function exportTheme() {
  const name = themeMeta.name.trim() || (store.project?.config.site.title ?? '').trim()
  const target = await saveDialog({
    defaultPath: `${name || 'theme'}.zip`,
    filters: [{ name: '主题包', extensions: ['zip'] }],
  })
  if (!target) return
  await actions.exportTheme(target, {
    name,
    version: themeMeta.version.trim(),
    author: themeMeta.author.trim(),
    description: themeMeta.description.trim(),
  })
}

async function pickTheme() {
  const selected = await open({
    multiple: false,
    filters: [{ name: '主题包', extensions: ['zip'] }],
  })
  if (typeof selected !== 'string') return
  themeArchive.value = selected
  themePreview.value = (await actions.scanTheme(selected)) ?? null
}

async function runThemeImport() {
  const report = await actions.importTheme(themeArchive.value, themeOverwrite.value)
  if (!report) return
  // 装完重扫：刚写进去的文件会变成「会覆盖」，剩下没装的一眼可见
  themePreview.value = (await actions.scanTheme(themeArchive.value)) ?? null
}
</script>

<template>
  <section class="layouts">
    <div class="layouts__column">
      <h3>主布局</h3>
      <ul class="layouts__list">
        <li v-for="layout in layouts" :key="layout.name">
          <label>
            <input v-model="selectedLayout" type="radio" :value="layout.name" />
            {{ layout.name }}
          </label>
          <button type="button" @click="actions.openTemplate(layout as TemplateInfo)">编辑</button>
        </li>
      </ul>

      <h3>全局共享组件</h3>
      <ul class="layouts__list">
        <li v-for="component in components" :key="component.name">
          <span>{{ component.name }}</span>
          <button type="button" @click="actions.openTemplate(component as TemplateInfo)">编辑</button>
        </li>
      </ul>

      <h3>页面模板</h3>
      <ul class="layouts__list">
        <li v-for="tpl in pageTemplates" :key="tpl.name">
          <span>{{ tpl.name }}</span>
          <button type="button" @click="actions.openTemplate(tpl as TemplateInfo)">编辑</button>
        </li>
      </ul>
    </div>

    <div class="layouts__column">
      <h3>继承结构</h3>
      <ul class="layouts__tree">
        <li v-for="entry in flatTree" :key="entry.node.name + entry.depth">
          <span :style="{ paddingLeft: `${entry.depth * 1.2}rem` }">
            {{ entry.node.name }}
            <em v-if="entry.node.cyclic" class="warn">（循环引用）</em>
          </span>
        </li>
      </ul>

      <details class="layouts__theme">
        <summary>主题包：打包当前外观 / 装入别人的</summary>
        <p class="build__muted">
          把左边这些模板与主题静态资源打成一个 zip 带走，或装一个别人做好的。
          <strong>包里不含 content/ 与 static/</strong>——文章与上传的图片是站点的，不是主题的。
        </p>
      <div class="settings__import-row">
        <input
          v-model="themeMeta.name"
          type="text"
          :placeholder="store.project?.config.site.title || '主题名'"
          aria-label="主题名"
        />
        <input v-model="themeMeta.version" type="text" placeholder="版本" aria-label="版本" />
      </div>
      <div class="settings__import-row">
        <input v-model="themeMeta.author" type="text" placeholder="作者" aria-label="作者" />
        <input
          v-model="themeMeta.description"
          type="text"
          placeholder="一句话说明"
          aria-label="说明"
        />
      </div>
      <button type="button" :disabled="store.busy" @click="exportTheme">打包当前外观…</button>
      <button type="button" :disabled="store.busy" @click="pickTheme">装入主题包…</button>

      <template v-if="themePreview">
        <p class="build__muted">
          {{ themePreview.manifest.name }}
          <template v-if="themePreview.manifest.version">
            {{ themePreview.manifest.version }}
          </template>
          <template v-if="themePreview.manifest.author">
            · {{ themePreview.manifest.author }}
          </template>
          ：{{ themePreview.files.length }} 个文件，其中
          {{ themePreview.conflicts.length }} 个会覆盖现有文件。
        </p>
        <p v-if="themePreview.manifest.description" class="build__muted">
          {{ themePreview.manifest.description }}
        </p>
        <ul class="settings__import-list">
          <li
            v-for="file in themePreview.files"
            :key="file"
            :class="{ skip: !themeOverwrite && themePreview.conflicts.includes(file) }"
          >
            <code>{{ file }}</code>
            <span v-if="themePreview.conflicts.includes(file)" class="badge badge--seo-warn">
              已存在
            </span>
          </li>
          <li v-for="row in themePreview.rejected" :key="row.entry" class="skip">
            <code>{{ row.entry }}</code>
            <span class="badge badge--seo-warn">已拒绝</span>
            <span class="build__muted">{{ row.reason }}</span>
          </li>
        </ul>
        <label class="settings__checkbox">
          <input v-model="themeOverwrite" type="checkbox" />
          覆盖已存在的文件（自己改过的模板会被替换）
        </label>
        <div class="settings__import-row">
          <button
            type="button"
            class="btn--primary"
            :disabled="store.busy || themeWillWrite === 0"
            @click="runThemeImport"
          >
            装入 {{ themeWillWrite }} 个文件
          </button>
          <button type="button" @click="themePreview = null">收起</button>
        </div>
        <p class="build__muted">装完记得整站重新生成：换外观等于所有页面的模板都变了。</p>
      </template>
      </details>
    </div>


    <div class="layouts__column layouts__column--wide">
      <h3>{{ store.currentTemplate ?? '模板源码' }}</h3>
      <template v-if="store.currentTemplate">
        <div class="editor__code layouts__code">
          <pre ref="mirror" class="editor__mirror" aria-hidden="true"><code v-html="highlighted" /></pre>
          <textarea
            ref="editor"
            class="layouts__editor"
            spellcheck="false"
            aria-label="模板源码"
            :value="store.currentTemplateSource"
            @input="actions.setTemplateSource(($event.target as HTMLTextAreaElement).value)"
            @scroll="syncScroll"
          />
        </div>
        <div class="layouts__actions">
          <button type="button" :disabled="store.busy" @click="actions.saveTemplate()">
            保存组件
          </button>
          <span v-if="store.plan" class="layouts__impact">
            检测到全局组件变更，影响 {{ store.plan.pages.length }} 个页面
          </span>
        </div>
      </template>
      <p v-else>选择左侧的布局或组件进行编辑。</p>
    </div>
  </section>
</template>
