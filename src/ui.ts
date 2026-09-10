/**
 * 界面级状态：当前在哪个视图、哪些窗格显示、编辑器能被外部调用的那几个命令。
 *
 * 单独抽出来是因为菜单栏要和标签页、编辑器共享同一份状态：Word 里「视图」菜单能
 * 勾掉导航窗格，「开始」菜单的加粗作用于当前文档——两者都要拿到别人的状态。
 * 若继续把这些状态藏在各组件的 `ref` 里，菜单就只能靠事件层层转发。
 *
 * 这里只放**界面状态**。站点数据与所有写盘动作仍在 `store.ts`，两者不混。
 */
import { reactive } from 'vue'

export type Tab = 'content' | 'calendar' | 'layouts' | 'audit' | 'build' | 'deploy' | 'settings'

/**
 * 界面模式：一次点出一整套布局。
 *
 * 定位已经明确——**正文是源码**，目标用户是愿意看 `+++` 与 Markdown 的人。
 * 但「愿意看源码」不等于「每一刻都要看见全部面板」：写长文时属性面板与预览是干扰，
 * 调模板时预览是主角，改一批 front matter 时属性面板才是主角。
 * 与其为每个人猜一套默认，不如像 VS Code 那样把布局做成能一键切换的预设。
 *
 * 三条自我约束：
 *
 * 1. **模式是预设，不是约束**。切过去之后每个窗格仍可单独开关，模式不会把它扳回来。
 *    「模式锁死布局」会让人以为界面坏了。
 * 2. **只改布局，不改能力**。任何模式下菜单、快捷键、命令面板都是全的——
 *    藏功能等于让人以为这个模式「不能做那件事」。
 * 3. **标准模式把窗格交给窗口宽度**（`applyResponsive`），另两种模式是用户的明确选择，
 *    因此接管显隐、不再自动收放。
 */
export type Mode = 'standard' | 'writing' | 'source'

export interface ModeSpec {
  id: Mode
  label: string
  /** 一句话说明「这套布局适合谁、为什么」，菜单里作为提示显示。 */
  hint: string
  layout: {
    showList: boolean
    showPreview: boolean
    showProps: boolean
    showToolbar: boolean
  }
}

export const modes: ModeSpec[] = [
  {
    id: 'standard',
    label: '标准',
    hint: '列表 + 编辑器 + 预览，属性面板与工具条都在；窗格随窗口宽度自动收放',
    layout: { showList: true, showPreview: true, showProps: true, showToolbar: true },
  },
  {
    id: 'writing',
    label: '专注写作',
    hint: '只留编辑器与工具条：收起列表、预览与属性面板，宽度全给正文',
    layout: { showList: false, showPreview: false, showProps: false, showToolbar: true },
  },
  {
    id: 'source',
    label: '源码',
    hint: '列表 + 编辑器，收起预览与属性面板：front matter 直接在 +++ 里改，格式用快捷键',
    layout: { showList: true, showPreview: false, showProps: false, showToolbar: false },
  },
]

export function modeSpec(mode: Mode): ModeSpec {
  return modes.find((item) => item.id === mode) ?? modes[0]
}


/**
 * 编辑器把选区类命令注册进来。
 *
 * 加粗、插链接这些动作依赖 textarea 的选区，只有编辑器组件自己知道；菜单栏要能调，
 * 就得有一个注册点。没打开文章时是 `null`，菜单据此把这些项置灰（而不是藏起来——
 * 藏起来会让人以为功能不存在，Word 的做法也是灰掉）。
 */
export interface EditorCommands {
  bold(): void
  italic(): void
  code(): void
  link(): void
  heading(): void
  quote(): void
  bullet(): void
  /** 打开文件选择器插入图片或附件 */
  pickFile(): void
  /** 开合媒体库：复用已上传过的资源 */
  assets(): void
  /** 展开「在这一篇里查找 / 替换」那一行 */
  find(): void
}

export const ui = reactive({
  /** 当前视图。标签页与菜单的「视图」组都改这一个值。 */
  tab: 'content' as Tab,
  paletteOpen: false,
  /** 「新建站点」对话框。菜单只放一条入口，字段在弹出的表单里填。 */
  newSiteOpen: false,
  /** 「回退内容」对话框：浏览内容快照并回到其中一份。 */
  snapshotsOpen: false,


  /** 当前界面模式（布局预设），见 [`modes`]。 */
  mode: 'standard' as Mode,
  /** 内容页左侧列表栏是否显示（视图菜单可勾掉，类比 Word 的导航窗格）。 */
  showList: true,
  /** 内容页右侧预览栏是否显示。 */
  showPreview: true,
  /** 编辑器右侧的属性面板（front matter 表单）是否显示。 */
  showProps: true,
  /** 编辑器上方的格式工具条是否显示。 */
  showToolbar: true,
  /**
   * 用户是否在「视图」菜单里手动开关过窗格。
   *
   * 手动动过之后就不再自动收放：窗口一变化就把人的选择改掉，比不响应式更烦人。
   */
  listOverride: false,
  previewOverride: false,
  /** 编辑器注册的选区命令；没打开文章时为 null。 */
  editor: null as EditorCommands | null,
  /**
   * 一次性请求：菜单点「新建内容」时置为 true，内容侧栏看到就展开新建表单并清零。
   *
   * 用信箱而不是直接调组件方法：侧栏可能还没挂载（正停在别的标签页），
   * 而信箱天然能等到它挂载之后再消费。
   */
  requestNewContent: false,
  /** 一次性请求：聚焦内容搜索框（菜单的「查找内容」与 Ctrl+F 同一个入口）。 */
  requestFocusSearch: false,
  /** 一次性请求：展开侧栏的「新建栏目」表单。 */
  requestNewSection: false,
  /** 一次性请求：展开侧栏的「跨文件替换」表单。 */
  requestReplace: false,
})

/**
 * 标签页按「做一件事的先后」分四组。
 *
 * 组名小而淡，只负责说明「这几个是一伙的」；`hint` 是选中后显示在标签栏下面的
 * 那一句，也是悬停提示——标签只有两个字，用途与关系必须写在别处。
 */
export const tabGroups: Array<{
  label: string
  tabs: Array<{ id: Tab; label: string; hint: string }>
}> = [
  {
    label: '写',
    tabs: [
      { id: 'content', label: '内容', hint: '写文章、管栏目：左边找、中间写、右边看最终效果' },
      {
        id: 'calendar',
        label: '日历',
        hint: '发布节奏：这个月发了几篇、下周排了什么、哪些还没写日期',
      },
    ],
  },
  {
    label: '外观',
    tabs: [
      {
        id: 'layouts',
        label: '外观',
        hint: '布局与组件的继承关系、改一处影响哪些页面；主题包的打包与装入也在这里',
      },
    ],
  },
  {
    label: '上线',
    tabs: [
      { id: 'audit', label: '体检', hint: 'SEO 字段、站内死链、媒体资源——上线前该修的都在这一页' },
      { id: 'build', label: '生成', hint: '把内容与模板渲染成 dist/ 里的静态文件（Ctrl+Enter 也可）' },
      { id: 'deploy', label: '发布', hint: '把 dist/ 送到 Git 或 FTP；凭据存系统凭据管理器' },
    ],
  },
  {
    label: '站点',
    tabs: [
      {
        id: 'settings',
        label: '设置',
        hint: 'staticsmith.toml 的可视化表单：站点信息、构建、媒体、分类、导航、发布，以及一次性的内容导入',
      },
    ],
  },
]

export const allTabs = tabGroups.flatMap((group) => group.tabs)

export function tabHint(tab: Tab): string {
  return allTabs.find((item) => item.id === tab)?.hint ?? ''
}

export function goTo(tab: Tab) {
  ui.tab = tab
}

/**
 * 按窗口宽度自动收放窗格。
 *
 * 三栏是 260 + 460 = 720px 的固定占用：1366 宽只剩约 600px 给编辑器，
 * 1024 宽就只剩约 250px——基本没法写。所以窄窗口先收预览，再收列表。
 *
 * 手动动过的那一栏不再自动收放（`listOverride` / `previewOverride`）：
 * 窗口一变就把人的选择改掉，比不响应式更烦人。
 */
export const PREVIEW_MIN_WIDTH = 1200
export const LIST_MIN_WIDTH = 900

export function applyResponsive(width: number) {
  if (!ui.previewOverride) ui.showPreview = width >= PREVIEW_MIN_WIDTH
  if (!ui.listOverride) ui.showList = width >= LIST_MIN_WIDTH
}

// ---------------------------------------------------------------- 界面模式

/** 记在本机：布局是「这台机器上这个人怎么用」，不属于站点。 */
const LAYOUT_KEY = 'staticsmith.layout'

/**
 * 切到某个界面模式。
 *
 * 标准模式把窗格交回窗口宽度（清掉「手动动过」的标记）；另两种是用户的明确选择，
 * 因此接管显隐并停掉自动收放——否则把窗口拉宽，刚收起的预览栏又自己冒出来。
 */
export function applyMode(mode: Mode) {
  const spec = modeSpec(mode)
  ui.mode = mode
  ui.showProps = spec.layout.showProps
  ui.showToolbar = spec.layout.showToolbar
  if (mode === 'standard') {
    ui.listOverride = false
    ui.previewOverride = false
    applyResponsive(window.innerWidth)
  } else {
    ui.showList = spec.layout.showList
    ui.showPreview = spec.layout.showPreview
    ui.listOverride = true
    ui.previewOverride = true
  }
  saveLayout()
}

/**
 * 单独开关某个窗格之后调用。
 *
 * 模式是预设不是约束：用户在模式之外的微调要记住，下次启动仍是他离开时的样子。
 */
export function saveLayout() {
  localStorage.setItem(
    LAYOUT_KEY,
    JSON.stringify({ mode: ui.mode, showProps: ui.showProps, showToolbar: ui.showToolbar }),
  )
}

/** 启动时恢复布局。模式先应用，随后盖上用户单独动过的那两个开关。 */
export function restoreLayout() {
  try {
    const raw = localStorage.getItem(LAYOUT_KEY)
    if (!raw) return
    const saved = JSON.parse(raw) as {
      mode?: Mode
      showProps?: boolean
      showToolbar?: boolean
    }
    if (saved.mode && modes.some((item) => item.id === saved.mode)) applyMode(saved.mode)
    if (typeof saved.showProps === 'boolean') ui.showProps = saved.showProps
    if (typeof saved.showToolbar === 'boolean') ui.showToolbar = saved.showToolbar
  } catch {
    // 坏数据就用默认布局，不值得打扰用户
  }
}
