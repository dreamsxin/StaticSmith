<script setup lang="ts">
/**
 * 发布节奏：把 `date` 摊在月历上。
 *
 * 定时发布（`publish_future = false`）能让文章排队上线，但「这个月发了几篇、
 * 下周排了什么」此前只能自己数日期。这一页纯呈现，不新增任何判断：
 * 「会不会进这次产物」由 Rust 侧算好后随 `PageSummary.scheduled` 一起送来，
 * 界面只按日期分格。
 */
import { computed, ref } from 'vue'

import { actions, store } from '../store'
import type { PageSummary } from '../api'

/** 点条目要跳到对应文章，编辑器只在内容标签页里。 */
const emit = defineEmits<{ open: [] }>()

const WEEKDAYS = ['一', '二', '三', '四', '五', '六', '日']

/** 显示的月份，用「当地时间的年月」表示。 */
const cursor = ref(startOfMonth(new Date()))

function startOfMonth(date: Date): Date {
  return new Date(date.getFullYear(), date.getMonth(), 1)
}

function shiftMonth(delta: number) {
  cursor.value = new Date(cursor.value.getFullYear(), cursor.value.getMonth() + delta, 1)
}

function today() {
  cursor.value = startOfMonth(new Date())
}

/**
 * 取出作者写下的那一天（`YYYY-MM-DD`）。
 *
 * 刻意不做时区换算：`2026-03-05` 用 `new Date()` 解析是 UTC 零点，在东八区会
 * 显示成前一天——日历要显示的是 front matter 里写的日期，不是某个时刻的投影。
 */
function dayOf(page: PageSummary): string | null {
  const match = /^(\d{4}-\d{2}-\d{2})/.exec(page.date ?? '')
  return match ? match[1] : null
}

function keyOf(date: Date): string {
  const month = `${date.getMonth() + 1}`.padStart(2, '0')
  const day = `${date.getDate()}`.padStart(2, '0')
  return `${date.getFullYear()}-${month}-${day}`
}

const todayKey = keyOf(new Date())

/** 按日期分桶。没写日期的进不了日历，单独列在下面。 */
const byDay = computed(() => {
  const map = new Map<string, PageSummary[]>()
  for (const page of store.project?.pages ?? []) {
    if (page.is_index) continue // 栏目列表页不是「发布」出去的一篇
    const day = dayOf(page as PageSummary)
    if (!day) continue
    const list = map.get(day) ?? []
    list.push(page as PageSummary)
    map.set(day, list)
  }
  return map
})

const undated = computed(() =>
  (store.project?.pages ?? []).filter(
    (page) => !page.is_index && !dayOf(page as PageSummary),
  ) as PageSummary[],
)

/** 月历格子：补齐首尾使每周整行，跨月的格子压暗但仍可点。 */
const cells = computed(() => {
  const first = cursor.value
  // getDay() 周日是 0，这里以周一开头
  const lead = (first.getDay() + 6) % 7
  const start = new Date(first.getFullYear(), first.getMonth(), 1 - lead)
  const out: Array<{ key: string; day: number; outside: boolean; pages: PageSummary[] }> = []
  for (let i = 0; i < 42; i += 1) {
    const date = new Date(start.getFullYear(), start.getMonth(), start.getDate() + i)
    const key = keyOf(date)
    out.push({
      key,
      day: date.getDate(),
      outside: date.getMonth() !== first.getMonth(),
      pages: byDay.value.get(key) ?? [],
    })
    // 已经排满整周且越过本月就收尾，不白留一整行空格
    if (i >= 27 && (i + 1) % 7 === 0 && date.getMonth() !== first.getMonth()) break
  }
  return out
})

const monthLabel = computed(
  () => `${cursor.value.getFullYear()} 年 ${cursor.value.getMonth() + 1} 月`,
)

/** 本月统计：已发 / 草稿 / 定时待上线。 */
const stats = computed(() => {
  const inMonth = cells.value.filter((cell) => !cell.outside).flatMap((cell) => cell.pages)
  return {
    total: inMonth.length,
    drafts: inMonth.filter((page) => page.draft).length,
    scheduled: inMonth.filter((page) => page.scheduled).length,
    published: inMonth.filter((page) => !page.draft && !page.scheduled).length,
  }
})

/** 距上次发布多少天——比「本月几篇」更能说明节奏是不是断了。 */
const sinceLast = computed(() => {
  const days = (store.project?.pages ?? [])
    .filter((page) => !page.is_index && !page.draft && !page.scheduled)
    .map((page) => dayOf(page as PageSummary))
    .filter((day): day is string => day !== null && day <= todayKey)
    .sort()
  const last = days.at(-1)
  if (!last) return null
  const diff = Math.round(
    (Date.parse(`${todayKey}T00:00:00`) - Date.parse(`${last}T00:00:00`)) / 86_400_000,
  )
  return { last, diff }
})

/** 排在今天之后的稿子，按日期升序——「下周要发什么」看这里。 */
const upcoming = computed(() =>
  (store.project?.pages ?? [])
    .filter((page) => !page.is_index)
    .map((page) => ({ page: page as PageSummary, day: dayOf(page as PageSummary) }))
    .filter((row): row is { page: PageSummary; day: string } => !!row.day && row.day > todayKey)
    .sort((a, b) => a.day.localeCompare(b.day))
    .slice(0, 8),
)

async function open(page: PageSummary) {
  await actions.requestOpenContent(page)
  emit('open')
}
</script>

<template>
  <section class="calendar">
    <div class="build__panel">
      <h3>发布节奏</h3>
      <div class="calendar__toolbar">
        <button type="button" @click="shiftMonth(-1)">← 上月</button>
        <strong class="calendar__month">{{ monthLabel }}</strong>
        <button type="button" @click="shiftMonth(1)">下月 →</button>
        <button type="button" @click="today">回到本月</button>
        <span class="app__spacer" />
        <span class="build__muted">
          本月 {{ stats.total }} 篇：已发 {{ stats.published }}、草稿 {{ stats.drafts }}、定时
          {{ stats.scheduled }}
        </span>
      </div>

      <p v-if="sinceLast" class="build__muted">
        上次发布在 {{ sinceLast.last }}，距今
        <strong>{{ sinceLast.diff }}</strong> 天。
      </p>
      <p v-else class="build__muted">还没有已发布的文章。</p>

      <p v-if="store.project?.config.build.publish_future" class="build__muted">
        站点当前<strong>不排队</strong>：未来日期的文章一构建就上线。想让它们到点再上线，
        去「设置」取消勾选「立即发布未来日期的文章」。
      </p>

      <div class="calendar__grid">
        <span v-for="name in WEEKDAYS" :key="name" class="calendar__weekday">{{ name }}</span>
        <div
          v-for="cell in cells"
          :key="cell.key"
          class="calendar__cell"
          :class="{ outside: cell.outside, today: cell.key === todayKey }"
        >
          <span class="calendar__day">{{ cell.day }}</span>
          <button
            v-for="page in cell.pages"
            :key="page.source"
            type="button"
            class="calendar__entry"
            :class="{ draft: page.draft, scheduled: page.scheduled }"
            :title="`${page.source}${page.draft ? '（草稿）' : page.scheduled ? '（定时，未进产物）' : ''}`"
            @click="open(page)"
          >
            {{ page.title }}
          </button>
        </div>
      </div>
    </div>

    <div class="build__panel">
      <h3>接下来</h3>
      <p v-if="!upcoming.length" class="build__muted">今天之后没有排期的稿子。</p>
      <ul v-else class="calendar__list">
        <li v-for="row in upcoming" :key="row.page.source">
          <span class="calendar__date">{{ row.day }}</span>
          <button type="button" class="calendar__link" @click="open(row.page)">
            {{ row.page.title }}
          </button>
          <span v-if="row.page.draft" class="badge badge--draft">草稿</span>
          <span v-else-if="row.page.scheduled" class="badge badge--draft">定时</span>
        </li>
      </ul>
    </div>

    <div class="build__panel">
      <h3>没写日期（{{ undated.length }}）</h3>
      <p class="build__muted">
        这些文章不在日历里，而且<strong>一律被当成已到时间</strong>——把「忘了写日期」
        当成「永不发布」只会让人莫名少一篇。要排队上线就先补上日期。
      </p>
      <p v-if="!undated.length" class="build__muted">都写了日期。</p>
      <ul v-else class="calendar__list">
        <li v-for="page in undated" :key="page.source">
          <button type="button" class="calendar__link" @click="open(page)">{{ page.title }}</button>
          <span v-if="page.draft" class="badge badge--draft">草稿</span>
          <code class="build__muted">{{ page.source }}</code>
        </li>
      </ul>
    </div>
  </section>
</template>
