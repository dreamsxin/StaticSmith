<script setup lang="ts">
/**
 * 发布节奏：把 `date` 摊在月历上。
 *
 * 定时发布（`publish_future = false`）能让文章排队上线，但「这个月发了几篇、
 * 下周排了什么」此前只能自己数日期。日期的判断不在这里做：
 * 「会不会进这次产物」由 Rust 侧算好后随 `PageSummary.scheduled` 一起送来，
 * 界面只按日期分格；能改的也只有 `date` 这一个字段，走的是属性面板那条同一条路。
 */
import { computed, nextTick, ref } from 'vue'

import { actions, isDirty, store } from '../store'
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

/** 按周切行。`role="row"` 需要真实的行容器，布局仍由 `.calendar__grid` 的七列负责。 */
const weeks = computed(() => {
  const out: Array<typeof cells.value> = []
  for (let i = 0; i < cells.value.length; i += 7) out.push(cells.value.slice(i, i + 7))
  return out
})

const monthLabel = computed(
  () => `${cursor.value.getFullYear()} 年 ${cursor.value.getMonth() + 1} 月`,
)

// ---------------------------------------------------------------- 月历的键盘操作

/**
 * 月历是二维结构，键盘要能走进去。
 *
 * 以前这 42 个格子里只有「有文章的那几颗按钮」能被 Tab 走到：空格子不存在于
 * 键盘世界里，也没法「走到 9 月 12 日看看那天有什么」（见 docs/ui-review.md 第 5 条）。
 *
 * 采用 grid 那套：整张表只占一个 Tab 停靠点（roving tabindex），方向键走格子，
 * `Home` / `End` 到本周两端，`PageUp` / `PageDown` 翻月，回车进入格子里的条目、
 * `Esc` 退回格子。42 个格子各占一个停靠点的话，想跳过日历要按四十多下 Tab。
 */
const grid = ref<HTMLElement | null>(null)
const activeDay = ref(todayKey)

/** 当前能被聚焦的那一格。落点不在这个月里时退回本月第一天，避免整张表都不可聚焦。 */
const focusDay = computed(() => {
  if (cells.value.some((cell) => cell.key === activeDay.value)) return activeDay.value
  return cells.value.find((cell) => !cell.outside)?.key ?? cells.value[0]?.key
})

function dateFromKey(key: string): Date {
  const [year, month, day] = key.split('-').map(Number)
  return new Date(year, month - 1, day)
}

/** 把焦点搬到某一天；那天不在当前视图里就先翻月。 */
async function goToDay(key: string) {
  activeDay.value = key
  if (!cells.value.some((cell) => cell.key === key)) {
    cursor.value = startOfMonth(dateFromKey(key))
    await nextTick()
  }
  grid.value?.querySelector<HTMLElement>(`[data-day="${key}"]`)?.focus()
}

function moveDays(key: string, delta: number) {
  const date = dateFromKey(key)
  date.setDate(date.getDate() + delta)
  void goToDay(keyOf(date))
}

function onCellKey(key: string, event: KeyboardEvent) {
  const weekday = (dateFromKey(key).getDay() + 6) % 7 // 以周一开头
  switch (event.key) {
    case 'ArrowLeft':
      moveDays(key, -1)
      break
    case 'ArrowRight':
      moveDays(key, 1)
      break
    case 'ArrowUp':
      moveDays(key, -7)
      break
    case 'ArrowDown':
      moveDays(key, 7)
      break
    case 'Home':
      moveDays(key, -weekday)
      break
    case 'End':
      moveDays(key, 6 - weekday)
      break
    case 'PageUp':
      moveDays(key, -daysInMonth(dateFromKey(key), -1))
      break
    case 'PageDown':
      moveDays(key, daysInMonth(dateFromKey(key), 0))
      break
    case 'Enter':
    case ' ':
      // 进入格子：条目自己不占 Tab 停靠点，靠这一下把焦点交给它们
      grid.value
        ?.querySelector<HTMLElement>(`[data-day="${key}"] .calendar__entry`)
        ?.focus()
      break
    default:
      return
  }
  event.preventDefault()
}

/**
 * 翻月要落在「同一天」而不是「同样往前 30 天」。
 *
 * `offset = -1` 给出上一个月的天数（往前翻用），`0` 给出当月天数（往后翻用）。
 * 目标日超出对月长度时 `Date` 自己会溢出到下个月，所以调用方拿到的是
 * 「尽量同一天」——这与系统日历的行为一致。
 */
function daysInMonth(date: Date, offset: number): number {
  return new Date(date.getFullYear(), date.getMonth() + offset + 1, 0).getDate()
}

/** 格子里的条目：上下键在同一天的几篇之间走，`Esc` 退回格子。 */
function onEntryKey(key: string, event: KeyboardEvent) {
  const entries = [
    ...(grid.value?.querySelectorAll<HTMLElement>(`[data-day="${key}"] .calendar__entry`) ?? []),
  ]
  const index = entries.indexOf(event.target as HTMLElement)
  switch (event.key) {
    case 'ArrowDown':
      entries[Math.min(entries.length - 1, index + 1)]?.focus()
      break
    case 'ArrowUp':
      if (index <= 0) grid.value?.querySelector<HTMLElement>(`[data-day="${key}"]`)?.focus()
      else entries[index - 1]?.focus()
      break
    case 'Escape':
      grid.value?.querySelector<HTMLElement>(`[data-day="${key}"]`)?.focus()
      break
    default:
      return
  }
  event.preventDefault()
}

/** 读屏要能只听一句就知道这一格是哪天、有几篇。 */
function cellLabel(cell: { key: string; pages: PageSummary[] }): string {
  const date = dateFromKey(cell.key)
  const when = `${date.getMonth() + 1} 月 ${date.getDate()} 日`
  return cell.pages.length ? `${when}，${cell.pages.length} 篇` : `${when}，没有内容`
}


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
    // 「已经发布」只认 Rust 的结论：`scheduled` 是它用 `Utc::now()` 算好的
    // 「日期没到、这次不进产物」。这里**不再自己比一遍日期**——原先多了一道
    // `day <= todayKey`，用的是浏览器本地时区，跨日边界上会和构建计划给出相反答案，
    // 而这个文件顶部恰好写着「会不会进产物由 Rust 算好」。
    .filter((page) => !page.is_index && !page.draft && !page.scheduled)
    .map((page) => dayOf(page as PageSummary))
    .filter((day): day is string => day !== null)
    .sort()
  const last = days.at(-1)
  if (!last) return null
  // 站点开着「立即发布未来日期的文章」时，已发布的稿子日期可以在今天之后，
  // 差值会是负数。那种情况按 0 天算——它确实已经在线上了。
  const diff = Math.max(
    0,
    Math.round((Date.parse(`${todayKey}T00:00:00`) - Date.parse(`${last}T00:00:00`)) / 86_400_000),
  )
  return { last, diff }
})

/** 还没上线、排在后面的稿子，按日期升序——「下周要发什么」看这里。 */
const upcoming = computed(() =>
  (store.project?.pages ?? [])
    .filter((page) => !page.is_index)
    .map((page) => ({ page: page as PageSummary, day: dayOf(page as PageSummary) }))
    // 「还没上线」分两种，只有第二种才需要看日期：
    // - `scheduled`：Rust 已经判定「日期没到，这次不进产物」，直接用它的结论；
    // - 草稿：无论日期到没到都不进产物，它的日期只是计划，本地比一下不会与
    //   构建计划冲突。
    .filter(
      (row): row is { page: PageSummary; day: string } =>
        !!row.day && (row.page.scheduled || (row.page.draft && row.day > todayKey)),
    )
    .sort((a, b) => a.day.localeCompare(b.day))
    .slice(0, 8),
)

async function open(page: PageSummary) {
  await actions.requestOpenContent(page)
  emit('open')
}

/**
 * 改发布日期：从「看节奏」到「排节奏」。
 *
 * 以前这一页是纯呈现，看得见「下周排了什么」，想把某篇挪到下周三却得回内容页、
 * 打开属性面板、改日期、保存四步。这里合成一步。
 *
 * 走的仍是现成那条路（打开 → 改 front matter → 保存），没有新增后端动作：
 * 排期改的就是文章的 `date` 字段，与属性面板改的是同一处，不该有第二种写法。
 * 副作用是这篇会被打开——这是好事，用户能看见到底改了哪一篇。
 */
const editingDate = ref<string | null>(null)
const dateDraft = ref('')

function startDate(page: PageSummary) {
  editingDate.value = page.source
  dateDraft.value = dayOf(page) ?? keyOf(new Date())
}

async function submitDate(page: PageSummary) {
  const day = dateDraft.value
  editingDate.value = null
  if (!day) return
  // 有未保存改动时不动手：requestOpenContent 会弹拦截，排期这一步会卡在半路，
  // 用户只看到「点了没反应」。说清楚比替他决定要好
  if (isDirty.value) {
    actions.notify('error', '当前文章有未保存改动，先保存或放弃再改排期')
    return
  }
  await actions.requestOpenContent(page)
  if (store.currentSource !== page.source) return
  // 只写日期部分，时间沿用 front matter 的写法（缺时间就按当天零点算）
  await actions.patchFrontMatter({ date: day })
  await actions.saveContent()
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

      <!-- 真正的 grid：整张表一个 Tab 停靠点，方向键走格子。
           行容器用 `display: contents`，语义补上了而七列布局不动 -->
      <div ref="grid" class="calendar__grid" role="grid" aria-label="月历">
        <div class="calendar__row" role="row">
          <span v-for="name in WEEKDAYS" :key="name" class="calendar__weekday" role="columnheader">
            {{ name }}
          </span>
        </div>
        <div v-for="(week, index) in weeks" :key="index" class="calendar__row" role="row">
          <div
            v-for="cell in week"
            :key="cell.key"
            class="calendar__cell"
            :class="{ outside: cell.outside, today: cell.key === todayKey }"
            role="gridcell"
            :data-day="cell.key"
            :tabindex="cell.key === focusDay ? 0 : -1"
            :aria-label="cellLabel(cell)"
            @keydown="onCellKey(cell.key, $event)"
          >
            <span class="calendar__day">{{ cell.day }}</span>
            <button
              v-for="page in cell.pages"
              :key="page.source"
              type="button"
              class="calendar__entry"
              :class="{ draft: page.draft, scheduled: page.scheduled }"
              tabindex="-1"
              :title="`${page.source}${page.draft ? '（草稿）' : page.scheduled ? '（定时，未进产物）' : ''}`"
              @click="open(page)"
              @keydown="onEntryKey(cell.key, $event)"
            >
              {{ page.title }}
            </button>
          </div>
        </div>
      </div>
    </div>

    <div class="build__panel">
      <h3>接下来</h3>
      <p v-if="!upcoming.length" class="build__muted">
        没有排队中的稿子。给文章的日期填一个将来的日子，它就会出现在这里。
      </p>
      <ul v-else class="calendar__list">
        <li v-for="row in upcoming" :key="row.page.source">
          <span class="calendar__date">{{ row.day }}</span>
          <button type="button" class="calendar__link" @click="open(row.page)">
            {{ row.page.title }}
          </button>
          <span v-if="row.page.draft" class="badge badge--draft">草稿</span>
          <span v-else-if="row.page.scheduled" class="badge badge--draft">定时</span>
          <!-- 排期在列表里改，不在七列的格子里改：格子太窄，放不下一个日期输入框 -->
          <template v-if="editingDate === row.page.source">
            <input v-model="dateDraft" type="date" aria-label="发布日期" />
            <button type="button" :disabled="store.busy" @click="submitDate(row.page)">改期</button>
            <button type="button" @click="editingDate = null">取消</button>
          </template>
          <button v-else type="button" @click="startDate(row.page)">改期…</button>
        </li>
      </ul>
    </div>

    <div class="build__panel">
      <h3>没写日期（{{ undated.length }}）</h3>
      <p class="build__muted">
        这些文章不在日历里，而且<strong>一律被当成已到时间</strong>——把「忘了写日期」
        当成「永不发布」只会让人莫名少一篇。要排队上线就先补上日期。
      </p>
      <p class="build__muted">
        栏目列表页（内容列表里带「栏目页」标的那些，例如
        <code>posts/index.md</code>）既不在日历里，也不在这份清单里：它是那个栏目的门面，
        不是某天发出去的一篇。改它的介绍文字去内容页对应的那一行。
      </p>
      <p v-if="!undated.length" class="build__muted">都写了日期。</p>
      <ul v-else class="calendar__list">
        <li v-for="page in undated" :key="page.source">
          <button type="button" class="calendar__link" @click="open(page)">{{ page.title }}</button>
          <span v-if="page.draft" class="badge badge--draft">草稿</span>
          <code class="build__muted">{{ page.source }}</code>
          <template v-if="editingDate === page.source">
            <input v-model="dateDraft" type="date" aria-label="发布日期" />
            <button type="button" :disabled="store.busy" @click="submitDate(page)">写入日期</button>
            <button type="button" @click="editingDate = null">取消</button>
          </template>
          <button v-else type="button" @click="startDate(page)">补日期…</button>
        </li>
      </ul>
    </div>
  </section>
</template>
