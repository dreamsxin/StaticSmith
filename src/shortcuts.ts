/**
 * 快捷键的唯一定义。
 *
 * 起因：帮助菜单里那条「快捷键与全部命令」点下去**只是打开命令面板**——面板里能搜到命令，
 * 但键位以灰字散落在各条上，而且置灰的命令连带它的键位一起从那个入口消失。
 * 界面上唯一的「一览」是编辑器页脚那六个，且只在打开文章后可见。
 * 于是十几个键位里，用户能发现的不到一半。
 *
 * 这份表**只描述**键位，不绑定行为：真正的处理仍在各自的 `keydown` 里
 * （window 级在 `App.vue`，编辑器级在 `ContentEditor.vue`，控件级在各组件内）。
 * 把行为也搬过来会变成一个「什么都管」的中心——键位的作用域恰恰是它最重要的属性，
 * 而作用域是由「谁监听」决定的，搬走就丢了。
 *
 * 所以这里是**文档的单一来源**：一览浮层与编辑器页脚都从它渲染，
 * 两处各写一遍的下场是「页脚说 Ctrl+K 是链接，浮层忘了列」。
 * 新增快捷键时：先在对应组件里实现，再回来补一条——漏了它就等于没人知道。
 */

/** 键位的作用域。它决定「按下去有没有用」，是这张表里最重要的一列。 */
export type Scope = 'global' | 'editor' | 'widget'

export interface Shortcut {
  /** 键位写法与界面一致（`Ctrl+Shift+O`）。macOS 上 Ctrl 与 Cmd 都认，见下面的注释。 */
  readonly keys: string
  /** 做什么。用动词开头，与菜单项的措辞一致。 */
  readonly what: string
  readonly scope: Scope
  /** 需要解释的取舍：为什么是这个键、什么时候不生效。 */
  readonly note?: string
}

/**
 * 作用域的中文名与一句话说明。
 *
 * 「在哪儿按才有用」是用户最常猜错的一件事：`Ctrl+B` 在列表里按下去毫无反应，
 * 不是坏了，是它属于编辑器。
 */
export const SCOPES: ReadonlyArray<{ id: Scope; label: string; hint: string }> = [
  { id: 'global', label: '随处可用', hint: '打开了站点就生效，焦点在哪都行' },
  { id: 'editor', label: '在编辑器里', hint: '光标在正文里时生效' },
  { id: 'widget', label: '在浮层与列表里', hint: '方向键那一类，跟着当前那块走' },
]

/**
 * 全部键位。顺序就是浮层里的顺序：先全局、再编辑器、最后控件级。
 *
 * macOS 上 `Ctrl` 与 `Cmd` 都认（`event.ctrlKey || event.metaKey`），表里只写 `Ctrl`：
 * 两个都列会让每一行长一倍，而 Mac 用户按 Cmd 是肌肉记忆，不需要被提醒。
 */
export const SHORTCUTS: readonly Shortcut[] = [
  { keys: 'Ctrl+P', what: '命令面板', scope: 'global', note: '所有命令都在里面，能搜' },
  { keys: 'Ctrl+S', what: '保存当前（文章或模板）', scope: 'global' },
  { keys: 'Ctrl+Enter', what: '增量生成', scope: 'global', note: '只重做受影响的页面' },
  { keys: 'Ctrl+Shift+Enter', what: '完整重建', scope: 'global', note: '清空产物后全量生成' },
  {
    keys: 'Ctrl+Shift+F',
    what: '找文章（跨全站）',
    scope: 'global',
    note: '在编辑器外，不带 Shift 的 Ctrl+F 也是它',
  },

  { keys: 'Ctrl+B', what: '加粗', scope: 'editor' },
  { keys: 'Ctrl+I', what: '斜体', scope: 'editor' },
  { keys: 'Ctrl+K', what: '插入链接', scope: 'editor' },
  {
    keys: 'Ctrl+Shift+K',
    what: '插入站内链接',
    scope: 'editor',
    note: '挑一篇，地址由站点给出，不手打',
  },
  {
    keys: 'Ctrl+F',
    what: '在这一篇里查找 / 替换',
    scope: 'editor',
    note: '选中一段再按，就用它当查找词',
  },
  { keys: 'Ctrl+Shift+O', what: '大纲：按标题跳转', scope: 'editor' },

  { keys: 'Esc', what: '关闭浮层、退出就地确认', scope: 'widget' },
  { keys: '↑ ↓', what: '在列表与浮层里移动', scope: 'widget' },
  { keys: 'Enter', what: '选中当前那一条', scope: 'widget' },
  {
    keys: '← →',
    what: '切换标签页、拖动分栏',
    scope: 'widget',
    note: '焦点在标签栏或分隔条上时；分隔条按住 Shift 走得更快',
  },
  {
    keys: 'PageUp / PageDown',
    what: '日历翻月',
    scope: 'widget',
    note: '↑↓ 是 ±7 天，←→ 是 ±1 天',
  },
]

/**
 * 编辑器页脚那一行显示哪几个。
 *
 * 挑「写作时手不离键盘就会用到」的：保存、三个行内格式、大纲、生成。
 * 页脚只有一行宽，不是一览——完整的在 `Ctrl+P` → 「快捷键一览」里。
 * 从同一份表里挑，而不是在模板里再抄一遍：抄一遍就会与浮层说的不一样。
 */
const FOOTER_KEYS = ['Ctrl+S', 'Ctrl+B', 'Ctrl+I', 'Ctrl+K', 'Ctrl+Shift+O', 'Ctrl+Enter']

export function footerShortcuts(): readonly Shortcut[] {
  return FOOTER_KEYS.map((keys) => {
    const found = SHORTCUTS.find((item) => item.keys === keys)
    if (!found) throw new Error(`页脚点名了一个不存在的键位：${keys}`)
    return found
  })
}

/** 某个作用域下的键位，浮层按作用域分组渲染。 */
export function shortcutsIn(scope: Scope): readonly Shortcut[] {
  return SHORTCUTS.filter((item) => item.scope === scope)
}
