<script setup lang="ts">
/**
 * 布局管理器：查看布局 → 组件的继承关系，并直接编辑全局共享组件。
 *
 * 保存组件后立即展示级联影响范围，对应产品文档 4.2 的「触发更新」提示。
 */
import { computed, ref, watch } from 'vue'

import { templateTree } from '../api'
import type { TemplateInfo, TemplateNode } from '../api'
import { actions, store } from '../store'

const tree = ref<TemplateNode | null>(null)
const selectedLayout = ref<string>('')

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
    </div>

    <div class="layouts__column layouts__column--wide">
      <h3>{{ store.currentTemplate ?? '模板源码' }}</h3>
      <template v-if="store.currentTemplate">
        <textarea
          class="layouts__editor"
          spellcheck="false"
          aria-label="模板源码"
          :value="store.currentTemplateSource"
          @input="actions.setTemplateSource(($event.target as HTMLTextAreaElement).value)"
        />
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
