<script setup lang="ts">
/** 生成面板：展示增量影响范围、触发构建、查看历史。 */
import { computed } from 'vue'
import { revealItemInDir } from '@tauri-apps/plugin-opener'

import { outputDir } from '../api'
import { actions, store } from '../store'

const plan = computed(() => store.plan)
const report = computed(() => store.lastBuild)

async function openOutput() {
  const dir = await outputDir()
  await revealItemInDir(dir)
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
        <button type="button" @click="openOutput">打开输出目录</button>
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
    </div>
  </section>
</template>
