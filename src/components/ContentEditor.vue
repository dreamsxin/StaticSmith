<script setup lang="ts">
/**
 * 内容编辑区。
 *
 * 编辑的是 Markdown 源文（含 `+++` front matter），保存后由 Rust 侧
 * 重新解析并给出增量构建计划。
 *
 * 三个交互决定：
 * - 工具条与快捷键直接操作选区，不引入富文本模型——源文始终是唯一真相
 * - 有未保存改动时切换文章会被拦下来问一句，而不是静默丢弃
 * - 粘贴或拖入图片先落盘到站点资源目录（内容寻址命名），再把地址插到光标处
 */
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'

import { actions, isDirty, store } from '../store'
import { goTo, saveLayout, ui, type EditorCommands } from '../ui'
import { parseList } from '../text'
import { highlight } from '../source-highlight'

const textarea = ref<HTMLTextAreaElement | null>(null)
const mirror = ref<HTMLElement | null>(null)
const dragging = ref(false)
const filePicker = ref<HTMLInputElement | null>(null)

/**
 * 语法着色。
 *
 * `textarea` 的文字设为透明、只留光标，背后垫一层同字体同行高的高亮镜像。
 * 这样既看得清结构，改的又还是纯文本——不引入富文本模型，也就不存在
 * 「界面里的样式与源文不一致」这类问题。
 */
/**
 * 高亮镜像。着色规则跟着站点的正文格式走：HTML 站点里满屏标签，
 * 按 Markdown 着色等于全部当普通文字。格式没读到时按 Markdown 兜底（它是默认值）。
 */
const highlighted = computed(() =>
  highlight(store.currentRaw, store.project?.config.build.source_format ?? 'markdown'),
)

/** 镜像不参与滚动，只能跟着 textarea 走。 */
function syncScroll() {
  const el = textarea.value
  const box = mirror.value
  if (!el || !box) return
  box.scrollTop = el.scrollTop
  box.scrollLeft = el.scrollLeft
}


const affected = computed(() => store.plan?.pages.length ?? 0)
const total = computed(() => store.plan?.total_pages ?? 0)

// ---------------------------------------------------------------- 属性面板

/**
 * front matter 表单。
 *
 * 源文仍是唯一真相：表单的每次提交都换算成新的源文（换算在 Rust 侧，
 * 保留正文、注释与未知键），而不是维护一份平行的字段状态。
 * 因此手改源文与用表单改不会互相打架。
 */
/**
 * 属性面板的显隐在 `ui`（`src/ui.ts`）而不是这里。
 *
 * 界面模式要一次决定整套布局，「视图」菜单也要能勾它——状态放在组件里，
 * 那两处就都碰不到它。
 */
function toggleProps() {
  ui.showProps = !ui.showProps
  saveLayout()
  if (ui.showProps) void actions.loadFrontMatter()
}

const fm = computed(() => store.frontMatter)

/** 标签在源文里是数组，表单里用逗号分隔——中文逗号也认。 */
const tagText = computed(() => (fm.value?.tags ?? []).join(', '))

/** 关键词同理。它与标签分开：标签会生成标签页，关键词只进 meta。 */
const keywordText = computed(() => (fm.value?.keywords ?? []).join(', '))

/** 旧地址同样是数组。改过 slug 的文章靠它把老链接接回来。 */
const aliasText = computed(() => (fm.value?.aliases ?? []).join(', '))



function parseTags(text: string): string[] {
  return parseList(text)
}


/** 全站已用过的标签，给输入框做候选，避免同义标签越写越多。 */
const knownTags = computed(() => {
  const set = new Set<string>()
  for (const page of store.project?.pages ?? []) for (const tag of page.tags) set.add(tag)
  return [...set].sort()
})

/** 关键词候选沿用标签集合：站内既有的词就是最该复用的词。 */
const knownKeywords = knownTags


function fieldValue(event: Event): string {
  return (event.target as HTMLInputElement).value
}


// ---------------------------------------------------------------- 选区编辑

/** 用新文本替换选区，并把光标落在 `cursor` 指定的绝对位置。 */
function replaceSelection(text: string, cursorStart: number, cursorEnd: number) {
  const el = textarea.value
  if (!el) return
  const raw = store.currentRaw
  actions.setRaw(raw.slice(0, el.selectionStart) + text + raw.slice(el.selectionEnd))
  requestAnimationFrame(() => {
    el.setSelectionRange(cursorStart, cursorEnd)
    el.focus()
  })
}

/** 包裹选区。没有选区时插入标记并把光标放中间，方便直接输入。 */
function wrap(before: string, after = before) {
  const el = textarea.value
  if (!el) return
  const selected = store.currentRaw.slice(el.selectionStart, el.selectionEnd)
  const start = el.selectionStart + before.length
  replaceSelection(`${before}${selected}${after}`, start, start + selected.length)
}

/** 给选中的每一行加前缀（标题、引用、列表）。 */
function prefixLines(prefix: string) {
  const el = textarea.value
  if (!el) return
  const raw = store.currentRaw
  const lineStart = raw.lastIndexOf('\n', el.selectionStart - 1) + 1
  const lineEnd = raw.indexOf('\n', el.selectionEnd)
  const end = lineEnd === -1 ? raw.length : lineEnd
  const block = raw.slice(lineStart, end)
  const replaced = block
    .split('\n')
    .map((line) => (line.startsWith(prefix) ? line.slice(prefix.length) : prefix + line))
    .join('\n')

  actions.setRaw(raw.slice(0, lineStart) + replaced + raw.slice(end))
  requestAnimationFrame(() => {
    el.setSelectionRange(lineStart, lineStart + replaced.length)
    el.focus()
  })
}

function insertLink() {
  const el = textarea.value
  if (!el) return
  const selected = store.currentRaw.slice(el.selectionStart, el.selectionEnd) || '链接文字'
  const snippet = `[${selected}](https://)`
  // 光标停在 URL 位置，接着就能粘地址
  const urlStart = el.selectionStart + selected.length + 3
  replaceSelection(snippet, urlStart, urlStart + 8)
}

// ---------------------------------------------------------------- 资源插入

function markdownFor(file: File, url: string): string {
  const alt = file.name.replace(/\.[^.]+$/, '') || '图片'
  return file.type.startsWith('image/') ? `![${alt}](${url})` : `[${file.name}](${url})`
}

async function insertFiles(files: File[]) {
  for (const file of files) {
    const url = await actions.saveAsset(file)
    if (!url) continue
    const el = textarea.value
    const snippet = markdownFor(file, url)
    if (el) {
      const caret = el.selectionStart + snippet.length
      replaceSelection(snippet, caret, caret)
    } else {
      actions.setRaw(store.currentRaw + snippet)
    }
  }
}

async function onPaste(event: ClipboardEvent) {
  const files = [...(event.clipboardData?.items ?? [])]
    .filter((item) => item.kind === 'file')
    .map((item) => item.getAsFile())
    .filter((file): file is File => file !== null)

  if (files.length === 0) return // 纯文本粘贴走浏览器默认行为
  event.preventDefault()
  await insertFiles(files)
}

async function onDrop(event: DragEvent) {
  dragging.value = false
  const files = [...(event.dataTransfer?.files ?? [])]
  if (files.length === 0) return
  event.preventDefault()
  await insertFiles(files)
}

async function onPickFiles(event: Event) {
  const input = event.target as HTMLInputElement
  await insertFiles([...(input.files ?? [])])
  input.value = ''
}

// ---------------------------------------------------------------- 媒体库

/**
 * 复用已上传的资源。
 *
 * 资源是内容寻址的，同一张图再粘一次也只会命中去重，但用户得先找到那张图——
 * 之前 `list_assets` 只有后端有，界面里没有任何入口。
 */
const showAssets = ref(false)

async function toggleAssets() {
  showAssets.value = !showAssets.value
  if (showAssets.value) await actions.loadAssets()
}

const IMAGE_EXT = /\.(png|jpe?g|gif|webp|avif|svg|bmp|ico)$/i

/** 缩略图要走 HTTP：iframe 之外的 WebView 同样读不到磁盘文件。 */
const assetBase = computed(() => store.previewServer)

function insertAsset(url: string) {
  const name = url.split('/').pop() ?? '资源'
  const snippet = IMAGE_EXT.test(url) ? `![${name}](${url})` : `[${name}](${url})`
  const el = textarea.value
  if (el) {
    const caret = el.selectionStart + snippet.length
    replaceSelection(snippet, caret, caret)
  } else {
    actions.setRaw(store.currentRaw + snippet)
  }
}


/**
 * 首次上手的三步。
 *
 * 「从零到线上」这条路径以前只写在文档里，而新站点打开后中间是一句「从左侧选择一篇内容」
 * ——它说的是操作方式，没说下一步该干什么。这里把那条路径做成可点的三步，
 * 每一步的完成态取真实状态（有文章、有过生成、有过发布），不额外记标记：
 * 记标记就会出现「明明没做却显示做完了」。
 *
 * 只在前两步没走完时出现：写过也生成过之后它就是噪音，
 * 而「从没发布过」是很多人的常态（只想本地看），不该一直催。
 */
const steps = computed(() => [
  {
    id: 'write',
    label: '写第一篇',
    hint: '也可以用菜单「编辑 → 新建文章…」',
    done: (store.project?.pages.length ?? 0) > 0,
    run: () => {
      ui.requestNewContent = true
    },
  },
  {
    id: 'build',
    label: '生成一次',
    hint: 'Ctrl+Enter：把内容与模板渲染成 dist/ 里的静态文件',
    done: (store.project?.recent_builds.length ?? 0) > 0,
    run: () => void actions.build('incremental'),
  },
  {
    id: 'deploy',
    label: '发布出去',
    hint: '在「发布」页配置 Git 或 FTP，凭据存系统凭据管理器',
    done: store.lastDeploy !== null,
    run: () => goTo('deploy'),
  },
])

const showGuide = computed(() => steps.value.slice(0, 2).some((step) => !step.done))

// ---------------------------------------------------------------- 快捷键


/** 正文字数（不含 front matter）。中文按字算，西文按词算。 */
const wordCount = computed(() => {
  const raw = store.currentRaw
  let body = raw
  if (raw.startsWith('+++')) {
    const end = raw.indexOf('+++', 3)
    if (end !== -1) body = raw.slice(end + 3)
  }
  const cjk = (body.match(/[\u3400-\u9fff\u3040-\u30ff]/g) ?? []).length
  const words = (body.match(/[A-Za-z0-9_'-]+/g) ?? []).length
  return cjk + words
})

// ---------------------------------------------------------------- 单篇内查找替换

/**
 * 这一篇里的查找与替换。
 *
 * 侧栏的 `Ctrl+F` 回答「哪一篇里有这个词」，这里回答「这一篇里的第几处」，
 * 两件事不同（见 docs/ui-design.md 第九节）。所以编辑器内的 `Ctrl+F` 归正文，
 * 找文章让给 `Ctrl+Shift+F`——这是早就写进文档的约定，现在才补上实现。
 *
 * 规则与跨文件替换刻意保持一致：纯文本、不做正则、不重叠计数。
 * 一个界面里两种「查找」语义会让人以为其中一个坏了。
 */
const finding = ref(false)
const findQuery = ref('')
const findReplace = ref('')
const findIgnoreCase = ref(false)
const findBox = ref<HTMLInputElement | null>(null)
/** 当前停在第几处（0 起）。-1 表示还没定位过。 */
const findIndex = ref(-1)

/**
 * 所有命中的起点。
 *
 * 忽略大小写时在小写副本上找、按原文偏移切割；小写化改变了字节长度
 * （如土耳其语 `İ`）就退回区分大小写——偏移对不上时给出错位的选区，
 * 比「这个开关暂时不生效」糟得多。
 */
const findMatches = computed<number[]>(() => {
  const raw = store.currentRaw
  const lowered = findIgnoreCase.value && raw.toLowerCase().length === raw.length
  const hay = lowered ? raw.toLowerCase() : raw
  const needle = lowered ? findQuery.value.toLowerCase() : findQuery.value
  if (!needle) return []
  const out: number[] = []
  let at = 0
  for (;;) {
    const found = hay.indexOf(needle, at)
    if (found === -1) break
    out.push(found)
    at = found + needle.length
  }
  return out
})

/**
 * 计数文案。
 *
 * 定位之前只报总数（「12 处」），定位之后才报位置（「3 / 12」）——一上来就显示
 * 「1 / 12」是在骗人：光标并没有停在第一处。
 * 显示位置时对总数取小：边打字边找时命中表会变短，旧序号会指向不存在的位置。
 */
const findLabel = computed(() => {
  const total = findMatches.value.length
  if (!findQuery.value) return ''
  if (!total) return '没有命中'
  if (findIndex.value < 0) return `${total} 处`
  return `${Math.min(findIndex.value + 1, total)} / ${total}`
})

// 换文章后命中表整个变了，旧序号必须作废，否则会显示成「3 / 1」这种不存在的位置
watch(
  () => store.currentSource,
  () => {
    findIndex.value = -1
  },
)

function openFind() {
  finding.value = true
  const el = textarea.value
  // 选中一段再按 Ctrl+F 就拿它当查找词（编辑器的通行做法）；跨行的选区不算
  const selected = el ? store.currentRaw.slice(el.selectionStart, el.selectionEnd) : ''
  if (selected && !selected.includes('\n')) findQuery.value = selected
  requestAnimationFrame(() => findBox.value?.select())
}

function closeFind() {
  finding.value = false
  findIndex.value = -1
  // 关掉之后焦点回到正文：留在一个已经隐藏的输入框上等于焦点丢了
  textarea.value?.focus()
}

/** 跳到第 n 处（可越界，自动首尾相接）。 */
function selectMatch(n: number) {
  const matches = findMatches.value
  const el = textarea.value
  if (!matches.length || !el) return
  const index = ((n % matches.length) + matches.length) % matches.length
  findIndex.value = index
  const start = matches[index]
  el.focus()
  el.setSelectionRange(start, start + findQuery.value.length)
}

/** 替换当前这一处。还没定位就先跳到第一处——直接替换会改在用户没看见的地方。 */
function replaceCurrent() {
  const at = findMatches.value[findIndex.value]
  if (at === undefined) {
    selectMatch(0)
    return
  }
  const raw = store.currentRaw
  actions.setRaw(raw.slice(0, at) + findReplace.value + raw.slice(at + findQuery.value.length))
  // 替换后原地还在同一个序号上，下一处自然接上；越界时回到第一处
  requestAnimationFrame(() => selectMatch(findIndex.value))
}

/**
 * 全部替换。
 *
 * 按命中表一次拼出结果，不做「循环 indexOf 替换」：替换词里含查找词时
 * （把 `a` 换成 `aa`）那种写法会自己吃自己，永远停不下来。
 */
function replaceAllInFile() {
  const matches = findMatches.value
  if (!matches.length) return
  const raw = store.currentRaw
  let out = ''
  let at = 0
  for (const start of matches) {
    out += raw.slice(at, start) + findReplace.value
    at = start + findQuery.value.length
  }
  actions.setRaw(out + raw.slice(at))
  findIndex.value = -1
  actions.notify('success', `这一篇里替换了 ${matches.length} 处（还没保存）`)
}

/**
 * 编辑器内的快捷键：只留依赖选区的那几个。
 *
 * Ctrl+S 不在这里——它挂在 App 的全局监听上，焦点落在属性面板的输入框里也要能存。
 * 两处都写会保存两次（textarea 的事件会继续冒泡到 window）。
 */
function onKeydown(event: KeyboardEvent) {
  if (!(event.ctrlKey || event.metaKey)) return
  const key = event.key.toLowerCase()
  const handlers: Record<string, () => void> = {
    b: editorCommands.bold,
    i: editorCommands.italic,
    k: editorCommands.link,
    f: editorCommands.find,
  }
  // Ctrl+Shift+F 是「找文章」，让它照常冒泡到 window
  const handler = event.shiftKey && key === 'f' ? undefined : handlers[key]
  if (!handler) return
  event.preventDefault()
  // 编辑器内的 Ctrl+F 归正文查找：不拦住冒泡，window 上那个「找文章」会把焦点抢去侧栏
  event.stopPropagation()
  handler()
}


/**
 * 选区类命令只定义一份。
 *
 * 工具条按钮、快捷键、菜单栏三处都调这个对象：以前工具条直接写 `wrap('**')`、
 * 菜单栏另写一份，加一个「删除线」只会加在一处，两边迟早不一致。
 */
const editorCommands: EditorCommands = {
  bold: () => wrap('**'),
  italic: () => wrap('*'),
  code: () => wrap('`'),
  link: () => insertLink(),
  heading: () => prefixLines('## '),
  quote: () => prefixLines('> '),
  bullet: () => prefixLines('- '),
  pickFile: () => filePicker.value?.click(),
  assets: () => void toggleAssets(),
  find: () => openFind(),
}

/**
 * 把选区类命令注册给菜单栏。
 *
 * 加粗、插链接依赖 textarea 的选区，只有这个组件知道；菜单栏要能调就得有个注册点。
 * 注销同样重要：切到别的标签页后编辑器卸载，菜单里那几项必须跟着置灰，
 * 否则点了会作用在一个已经不存在的输入框上。
 */
onMounted(() => {
  ui.editor = editorCommands
})

onBeforeUnmount(() => {
  ui.editor = null
})
</script>

<template>
  <section v-if="store.currentSource" class="editor">
    <header class="editor__bar">
      <strong>{{ store.currentSource }}</strong>
      <span v-if="isDirty" class="editor__dirty" title="有未保存改动">●</span>
      <span class="editor__spacer" />
      <span class="editor__hint">保存后将重新生成 {{ affected }} / {{ total }} 个页面</span>
      <button type="button" :class="{ active: ui.showProps }" title="编辑标题、日期、标签等属性" @click="toggleProps">
        属性
      </button>
      <button type="button" :disabled="store.busy || !isDirty" @click="actions.saveContent()">
        保存 <kbd>Ctrl+S</kbd>
      </button>
      <button type="button" :disabled="store.busy" @click="actions.refreshPreview()">
        刷新预览
      </button>
    </header>

    <div v-if="ui.showProps" class="editor__props">
      <template v-if="fm">
        <label class="editor__prop editor__prop--wide">
          标题
          <input
            type="text"
            :value="fm.title"
            placeholder="留空则取正文第一个 # 标题"
            @change="actions.patchFrontMatter({ title: fieldValue($event) })"
          />
        </label>
        <label class="editor__prop">
          日期
          <input
            type="text"
            :value="fm.date ?? ''"
            placeholder="2026-09-07"
            @change="actions.patchFrontMatter({ date: fieldValue($event) })"
          />
        </label>
        <label class="editor__prop editor__prop--wide">
          描述
          <input
            type="text"
            :value="fm.description"
            placeholder="用于列表摘要与订阅源"
            @change="actions.patchFrontMatter({ description: fieldValue($event) })"
          />
        </label>
        <label class="editor__prop editor__prop--wide">
          标签
          <input
            type="text"
            list="known-tags"
            :value="tagText"
            placeholder="逗号分隔，如：模板, 增量构建"
            @change="actions.patchFrontMatter({ tags: parseTags(fieldValue($event)) })"
          />
        </label>
        <label class="editor__prop editor__prop--wide">
          关键词（SEO）
          <input
            type="text"
            list="known-keywords"
            :value="keywordText"
            placeholder="留空则用标签"
            @change="actions.patchFrontMatter({ keywords: parseTags(fieldValue($event)) })"
          />
        </label>
        <label class="editor__prop editor__prop--wide">
          旧地址（重定向）
          <input
            type="text"
            :value="aliasText"
            placeholder="改过地址时填老地址，如 /posts/old-slug/"
            @change="actions.patchFrontMatter({ aliases: parseTags(fieldValue($event)) })"
          />
        </label>
        <datalist id="known-tags">
          <option v-for="tag in knownTags" :key="tag" :value="tag" />
        </datalist>
        <datalist id="known-keywords">
          <option v-for="word in knownKeywords" :key="word" :value="word" />
        </datalist>

        <label class="editor__prop editor__prop--check">
          <input
            type="checkbox"
            :checked="fm.draft"
            @change="
              actions.patchFrontMatter({ draft: ($event.target as HTMLInputElement).checked })
            "
          />
          草稿（不进产物）
        </label>
      </template>
      <p v-else class="build__muted">
        front matter 暂时读不出来（可能正手改到一半）。修好 `+++` 之间的内容后即可用表单编辑。
      </p>
    </div>


    <p v-if="store.pendingPage" class="editor__pending">
      <span>当前文章有未保存改动，切换到「{{ store.pendingPage.title }}」前要怎么处理？</span>
      <button type="button" @click="actions.resolvePending('save')">保存并切换</button>
      <button type="button" @click="actions.resolvePending('discard')">放弃改动</button>
      <button type="button" @click="actions.resolvePending('cancel')">留在本页</button>
    </p>

    <!-- 提示文案与「编辑 / 插入」菜单里的那几项逐字一致：同一个动作在两处叫不同名字，
         用户会以为是两个功能（见 ui-design.md 6.5） -->
    <!-- 工具条可以整条收掉（源码模式默认如此）：按钮走的动作与快捷键、菜单同一份
         `editorCommands`，收起来只是少一排按钮，能力一件不少 -->
    <div v-if="ui.showToolbar" class="editor__toolbar">
      <button
        type="button"
        title="标题（二级）"
        aria-label="标题（二级）"
        @click="editorCommands.heading()"
      >
        H2
      </button>
      <button type="button" title="加粗 Ctrl+B" aria-label="加粗" @click="editorCommands.bold()">
        <b>B</b>
      </button>
      <button type="button" title="斜体 Ctrl+I" aria-label="斜体" @click="editorCommands.italic()">
        <i>I</i>
      </button>
      <button type="button" title="行内代码" aria-label="行内代码" @click="editorCommands.code()">
        &lt;/&gt;
      </button>
      <button type="button" title="引用" aria-label="引用" @click="editorCommands.quote()">❝</button>
      <button type="button" title="无序列表" aria-label="无序列表" @click="editorCommands.bullet()">
        •
      </button>
      <button type="button" title="链接 Ctrl+K" aria-label="插入链接" @click="editorCommands.link()">
        🔗
      </button>
      <button
        type="button"
        title="图片或附件…（也可直接粘贴或拖入）"
        aria-label="插入图片或附件"
        @click="editorCommands.pickFile()"
      >
        图片
      </button>
      <button
        type="button"
        :class="{ active: showAssets }"
        title="媒体库：复用已上传的资源"
        aria-label="媒体库"
        @click="editorCommands.assets()"
      >
        媒体库
      </button>
      <input
        ref="filePicker"
        type="file"
        accept="image/*"
        multiple
        hidden
        @change="onPickFiles"
      />
    </div>

    <div v-if="showAssets" class="editor__assets">
      <p v-if="!store.assets.length" class="build__muted">
        还没有资源。粘贴、拖入或用「图片」按钮上传后会出现在这里。
      </p>
      <button
        v-for="asset in store.assets"
        :key="asset.content_hash"
        type="button"
        class="editor__asset"
        :title="`${asset.url} · ${(asset.size / 1024).toFixed(1)} KB`"
        @click="insertAsset(asset.url)"
      >
        <img
          v-if="assetBase && IMAGE_EXT.test(asset.url)"
          :src="`${assetBase}${asset.url}`"
          :alt="asset.url"
        />
        <span v-else class="editor__asset-name">{{ asset.url.split('/').pop() }}</span>
      </button>
      <p v-if="store.assets.length && !assetBase" class="build__muted">
        启动本地预览服务器并生成一次后，这里会显示缩略图。
      </p>
    </div>


    <!-- 单篇内查找替换：一行，不做浮层——它要和正文同时看见 -->
    <div v-if="finding" class="editor__find" role="search">
      <input
        ref="findBox"
        v-model="findQuery"
        type="text"
        placeholder="在这一篇里查找"
        aria-label="在这一篇里查找"
        @keydown.enter.prevent="selectMatch(findIndex + 1)"
        @keydown.esc.prevent="closeFind"
      />
      <span class="editor__find-count">{{ findLabel }}</span>
      <button
        type="button"
        :disabled="!findMatches.length"
        title="上一处"
        @click="selectMatch(findIndex - 1)"
      >
        ↑
      </button>
      <button
        type="button"
        :disabled="!findMatches.length"
        title="下一处（回车）"
        @click="selectMatch(findIndex + 1)"
      >
        ↓
      </button>
      <input
        v-model="findReplace"
        type="text"
        placeholder="替换为（留空即删掉）"
        aria-label="替换为"
        @keydown.esc.prevent="closeFind"
      />
      <button type="button" :disabled="!findMatches.length" @click="replaceCurrent">替换这处</button>
      <button type="button" :disabled="!findMatches.length" @click="replaceAllInFile">
        全部替换
      </button>
      <label class="editor__find-case">
        <input v-model="findIgnoreCase" type="checkbox" />
        忽略大小写
      </label>
      <span class="app__spacer" />
      <button type="button" title="关闭（Esc）" @click="closeFind">×</button>
    </div>

    <div class="editor__code">
      <pre ref="mirror" class="editor__mirror" aria-hidden="true"><code v-html="highlighted" /></pre>
      <textarea
        ref="textarea"
        class="editor__area"
        :class="{ 'editor__area--dragging': dragging }"
        spellcheck="false"
        :value="store.currentRaw"
        aria-label="内容源文"
        @input="actions.setRaw(($event.target as HTMLTextAreaElement).value)"
        @scroll="syncScroll"
        @keydown="onKeydown"
        @paste="onPaste"
        @dragover.prevent="dragging = true"
        @dragleave="dragging = false"
        @drop="onDrop"
      />
    </div>


    <footer class="editor__foot">
      {{ wordCount }} 字 · <kbd>Ctrl+S</kbd> 保存 · <kbd>Ctrl+B</kbd> 加粗 · <kbd>Ctrl+I</kbd> 斜体 ·
      <kbd>Ctrl+K</kbd> 链接 · <kbd>Ctrl+Enter</kbd> 增量生成 · 粘贴或拖入图片即插入
    </footer>
  </section>

  <section v-else class="editor editor--empty">
    <p>从左侧选择一篇内容开始编辑，或点「新建」。</p>

    <!-- 首次上手：把「从零到线上」那条路径做成可点的三步。完成的一步留在原地打勾，
         不隐藏——隐藏会让人怀疑刚才那一步是不是做错了（同 6.4「置灰不隐藏」） -->
    <ol v-if="showGuide" class="editor__guide">
      <li v-for="step in steps" :key="step.id" :class="{ done: step.done }">
        <span class="editor__guide-mark" aria-hidden="true">{{ step.done ? '✓' : '·' }}</span>
        <button type="button" :disabled="store.busy" :title="step.hint" @click="step.run()">
          {{ step.label }}
        </button>
        <span class="editor__guide-hint">{{ step.hint }}</span>
      </li>
    </ol>
  </section>
</template>
