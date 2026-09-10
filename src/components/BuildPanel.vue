<script setup lang="ts">
/** 生成面板：展示增量影响范围、触发构建、浏览产物、查看历史。 */
import { computed } from 'vue'

import { actions, store } from '../store'
import { outputKindLabel } from '../labels'
import type { OutputFile, OutputKind } from '../api'

const plan = computed(() => store.plan)
const report = computed(() => store.lastBuild)

/**
 * 分阶段耗时的一行文字，只列非零项。
 *
 * 全列的话一屏都是 `0 ms`，真正的大头反而看不出来——增量空跑的九成时间在标签页上，
 * 这种事只有把非零项摆出来才看得见。
 */
const phaseBreakdown = computed(() => {
  const phases = report.value?.phases
  if (!phases) return ''
  const parts = [
    ['计划', phases.plan_ms],
    ['渲染', phases.render_ms],
    ['写盘', phases.write_ms],
    ['站点文件', phases.site_files_ms],
    ['标签页', phases.taxonomy_ms],
    ['静态资源', phases.assets_ms],
    ['索引', phases.index_ms],
  ] as const
  const shown = parts.filter(([, ms]) => ms > 0).map(([label, ms]) => `${label} ${ms} ms`)
  return shown.length ? `时间去处：${shown.join(' / ')}` : '时间去处：都在 1 ms 以内'
})

/** 预览面板只在「内容」标签页里，点产物后请求外框切回去，否则点了看不到。 */
const emit = defineEmits<{ preview: [] }>()



/** 按类型分组的产物清单。顺序沿用后端的排序，不再二次打乱。 */
const outputGroups = computed(() => {
  const map = new Map<OutputKind, OutputFile[]>()
  for (const file of store.outputs) {
    const list = map.get(file.kind) ?? []
    list.push(file as OutputFile)
    map.set(file.kind, list)
  }
  return [...map.entries()]
})

function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`
}

async function openOutput(url: string) {
  await actions.previewOutput(url)
  if (store.previewServer) await actions.openInBrowser(`${store.previewServer}${url}`)
}

/** 在内嵌预览里打开产物。 */
async function showOutput(url: string) {
  await actions.previewOutput(url)
  if (store.previewTarget === url) emit('preview')
}

</script>


<template>
  <section class="build">
    <div class="build__panel">
      <h3>待生成范围</h3>
      <template v-if="plan">
        <p>
          受影响页面 <strong>{{ plan.pages.length }}</strong> / 全站 {{ plan.total_pages }}
        </p>
        <p v-if="plan.changed_templates.length">
          变更模板：<code v-for="t in plan.changed_templates" :key="t">{{ t }}</code>
        </p>
        <p v-if="plan.affected_templates.length" class="build__muted">
          级联影响模板：{{ plan.affected_templates.join('、') }}
        </p>
        <p v-if="plan.orphaned_pages.length" class="build__muted">
          将清理已删除内容的产物：{{ plan.orphaned_pages.join('、') }}
        </p>
        <p v-if="plan.pages.length === 0 && plan.orphaned_pages.length === 0">
          没有需要重新生成的内容。
        </p>
      </template>

      <div class="build__actions">
        <button type="button" :disabled="store.busy" @click="actions.build('incremental')">
          增量生成
        </button>
        <button type="button" :disabled="store.busy" @click="actions.build('full')">
          生成全站
        </button>
        <button type="button" :disabled="store.busy" @click="actions.recomputePlan()">
          重新计算范围
        </button>
        <button type="button" @click="actions.revealOutput()">在文件管理器中定位产物</button>
      </div>
    </div>

    <div class="build__panel">
      <div class="build__head">
        <h3>产物清单</h3>
        <button type="button" :disabled="store.busy" @click="actions.loadOutputs()">刷新</button>
      </div>
      <p v-if="!store.outputs.length" class="build__muted">
        还没有产物。先点「生成全站」，标签页、分页页、订阅源都会出现在这里。
      </p>
      <div v-for="[kind, files] in outputGroups" :key="kind" class="build__outputs">
        <h4>{{ outputKindLabel(kind) }}（{{ files.length }}）</h4>
        <ul>
          <li v-for="file in files" :key="file.path">
            <button
              type="button"
              class="build__output"
              :class="{ active: store.previewTarget === file.url }"
              :title="`在预览面板中打开 ${file.path}`"
              @click="showOutput(file.url)"
            >
              <span class="build__output-url">{{ file.url }}</span>
              <span class="build__muted">{{ formatSize(file.size) }}</span>
            </button>
            <button
              type="button"
              class="build__output-open"
              title="在浏览器打开"
              @click="openOutput(file.url)"
            >
              ↗
            </button>
          </li>
        </ul>
      </div>
    </div>


    <div class="build__panel">
      <h3>最近一次结果</h3>
      <template v-if="report">
        <ul class="build__stats">
          <li>模式：{{ report.mode === 'full' ? '全量' : '增量' }}</li>
          <li>渲染页面：{{ report.pages_rendered }}</li>
          <li>写入文件：{{ report.files_written }}（含分页）</li>
          <li>复制静态资源：{{ report.assets_copied }}</li>
          <li>耗时：{{ report.duration_ms }} ms</li>
        </ul>
        <!-- 分阶段耗时：回答「为什么这次这么久」。只列非零项，否则一屏全是 0 ms，
             真正的大头反而看不出来。各项之和略小于总耗时（零碎不单独计时）。 -->
        <p class="build__muted">{{ phaseBreakdown }}</p>
        <p v-if="report.removed_files.length" class="build__muted">
          已删除：{{ report.removed_files.join('、') }}
        </p>
        <p v-for="warning in report.warnings" :key="warning" class="warn">{{ warning }}</p>
      </template>
      <p v-else>尚未在本次会话中生成。</p>

      <h3>构建历史</h3>
      <ul class="build__stats">
        <li v-for="record in store.project?.recent_builds ?? []" :key="record.id">
          {{ record.finished_at }} · {{ record.mode }} · {{ record.pages_written }} 页 ·
          {{ record.duration_ms }} ms
        </li>
      </ul>
      <!-- 空表看起来像坏了：说清生成之后这里会出现什么（见 ui-design.md 6.9） -->
      <p v-if="(store.project?.recent_builds ?? []).length === 0" class="empty-hint">
        这个站点还没有生成记录。每次生成都会在这里留下时间、模式、页数与耗时，
        用来对比「这次为什么比上次慢」。
      </p>
    </div>
  </section>
</template>
