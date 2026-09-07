<script setup lang="ts">
/**
 * SEO 体检面板：内容运营的日常入口。
 *
 * 只呈现「还差什么」与「点哪能改」，不做评分玄学：分数只用于今天比昨天好没好。
 * 结论来自 Rust 侧的 `audit_seo`，与 MCP 暴露给 AI Agent 的是同一份规则，
 * 所以「让 Agent 批量补描述」之后回到这里刷新，看到的是同一套判定。
 */
import { computed, onMounted, ref } from 'vue'

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

// ---------------------------------------------------------------- 媒体资源

/**
 * 媒体体检要扫盘，打开面板时跑一次就够，之后由用户手动刷新。
 */
onMounted(() => {
  if (!store.media) void actions.auditMedia()
})

const media = computed(() => store.media)
const selected = ref<Set<string>>(new Set())
const confirming = ref(false)

function toggle(path: string, on: boolean) {
  const next = new Set(selected.value)
  if (on) next.add(path)
  else next.delete(path)
  selected.value = next
}

function selectAll() {
  selected.value = new Set((media.value?.unused ?? []).map((f) => f.path))
}

async function removeSelected() {
  confirming.value = false
  const paths = [...selected.value]
  selected.value = new Set()
  await actions.removeMedia(paths)
}

function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`
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

    <div class="build__panel">
      <div class="build__head">
        <h3>媒体资源</h3>
        <button type="button" :disabled="store.busy" @click="actions.auditMedia()">重新扫描</button>
      </div>

      <template v-if="media">
        <p class="build__muted">
          共 {{ media.total }} 个文件 / {{ formatSize(media.total_size) }}，
          其中 {{ media.unused.length }} 个没人引用（可回收 {{ formatSize(media.reclaimable) }}）。
          引用范围含内容、模板与主题。
        </p>

        <template v-if="media.unused.length">
          <h4>未被引用（{{ media.unused.length }}）</h4>
          <ul class="seo__media">
            <li v-for="file in media.unused" :key="file.path">
              <label>
                <input
                  type="checkbox"
                  :checked="selected.has(file.path)"
                  @change="toggle(file.path, ($event.target as HTMLInputElement).checked)"
                />
                <code>{{ file.url }}</code>
                <span class="build__muted">{{ formatSize(file.size) }}</span>
              </label>
            </li>
          </ul>
          <div class="build__actions">
            <button type="button" @click="selectAll">全选</button>
            <template v-if="confirming">
              <button type="button" class="btn--danger" :disabled="store.busy" @click="removeSelected">
                确认删除 {{ selected.size }} 个文件（不可撤销）
              </button>
              <button type="button" @click="confirming = false">取消</button>
            </template>
            <button
              v-else
              type="button"
              :disabled="!selected.size || store.busy"
              @click="confirming = true"
            >
              删除所选（{{ selected.size }}）
            </button>
          </div>
        </template>

        <template v-if="media.missing.length">
          <h4>引用了但文件不存在（{{ media.missing.length }}）</h4>
          <ul class="seo__media">
            <li v-for="ref in media.missing" :key="ref.url">
              <span>
                <code>{{ ref.url }}</code>
                <span class="build__muted">← {{ ref.referenced_by.join('、') }}</span>
              </span>
            </li>
          </ul>
        </template>

        <p v-if="!media.unused.length && !media.missing.length" class="build__muted">
          没有未引用文件，也没有破图。
        </p>
      </template>
      <p v-else class="build__muted">正在扫描资源目录…</p>
    </div>
  </section>
</template>

