<script setup lang="ts">
/**
 * SEO 体检面板：内容运营的日常入口。
 *
 * 只呈现「还差什么」与「点哪能改」，不做评分玄学：分数只用于今天比昨天好没好。
 * 结论来自 Rust 侧的 `audit_seo`，与 MCP 暴露给 AI Agent 的是同一份规则，
 * 所以「让 Agent 批量补描述」之后回到这里刷新，看到的是同一套判定。
 */
import { computed } from 'vue'

import { actions, store } from '../store'
import type { PageSummary, SeoIssue, SeoSeverity } from '../api'

/** 点问题条目要跳到对应文章，预览面板只在内容标签页里。 */
const emit = defineEmits<{ open: [] }>()

const report = computed(() => store.seo)

const SEVERITY_LABEL: Record<SeoSeverity, string> = {
  error: '必须修',
  warn: '建议修',
  hint: '可优化',
}

const groups = computed(() => {
  const order: SeoSeverity[] = ['error', 'warn', 'hint']
  return order
    .map((severity) => ({
      severity,
      issues: (report.value?.issues ?? []).filter((i) => i.severity === severity),
    }))
    .filter((g) => g.issues.length > 0)
})

/** 缺描述的篇数——运营最常一次性处理的就是这一类。 */
const missingDescription = computed(
  () => (report.value?.issues ?? []).filter((i) => i.code === 'description.missing').length,
)

async function openIssue(issue: SeoIssue) {
  if (!issue.source) return // 站点级问题去「设置」改，不属于某一篇
  const page = store.project?.pages.find((p) => p.source === issue.source)
  if (!page) return
  await actions.requestOpenContent(page as PageSummary)
  emit('open')
}
</script>

<template>
  <section class="seo">
    <div class="build__panel">
      <div class="build__head">
        <h3>SEO 体检</h3>
        <button type="button" :disabled="store.busy" @click="actions.auditSeo()">重新检查</button>
      </div>

      <template v-if="report">
        <p class="seo__score">
          <strong>{{ report.score }}</strong>
          <span class="build__muted">/ 100 · 已检查 {{ report.checked }} 个页面（草稿不算）</span>
        </p>
        <ul class="build__stats">
          <li>必须修：{{ report.errors }}</li>
          <li>建议修：{{ report.warnings }}</li>
          <li>可优化：{{ report.hints }}</li>
        </ul>
        <p v-if="missingDescription" class="build__muted">
          有 {{ missingDescription }} 篇缺描述。可以让 AI Agent 通过 MCP 的
          <code>audit_seo</code> + <code>patch_front_matter</code> 批量补齐，见
          <code>docs/seo.md</code>。
        </p>
        <p v-if="!report.issues.length" class="build__muted">没有发现问题。</p>
      </template>
      <p v-else class="build__muted">还没有体检结果，点「重新检查」。</p>
    </div>

    <div v-for="group in groups" :key="group.severity" class="build__panel">
      <h3>{{ SEVERITY_LABEL[group.severity] }}（{{ group.issues.length }}）</h3>
      <ul class="seo__issues">
        <li v-for="(issue, index) in group.issues" :key="`${issue.code}-${issue.source}-${index}`">
          <button
            type="button"
            class="seo__issue"
            :class="{ 'seo__issue--site': !issue.source }"
            :title="issue.source || '站点级问题，去「设置」修改'"
            @click="openIssue(issue)"
          >
            <span class="seo__issue-title">{{ issue.title || issue.source || '站点' }}</span>
            <span class="seo__issue-message">{{ issue.message }}</span>
            <code class="seo__issue-code">{{ issue.code }}</code>
          </button>
        </li>
      </ul>
    </div>
  </section>
</template>
