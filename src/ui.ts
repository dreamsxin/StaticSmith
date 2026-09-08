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
}

export const ui = reactive({
  /** 当前视图。标签页与菜单的「视图」组都改这一个值。 */
  tab: 'content' as Tab,
  paletteOpen: false,
  /** 内容页左侧列表栏是否显示（视图菜单可勾掉，类比 Word 的导航窗格）。 */
  showList: true,
  /** 内容页右侧预览栏是否显示。 */
  showPreview: true,
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
      { id: 'deploy', label: '发布', hint: '把 dist/ 送到 Git 或 FTP/SFTP；凭据存系统凭据管理器' },
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
