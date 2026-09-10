/**
 * 逗号分隔的输入 → 字符串数组。
 *
 * 标签、关键词、旧地址在界面上都是「用逗号隔开」的一行输入，中文输入法下打出的
 * 是全角逗号，两种都得认。抽出来是因为属性面板与批量动作要用同一套解析——
 * 一处认全角、另一处不认，会变成很难描述的「有时候多一个空标签」。
 */
export function parseList(text: string): string[] {
  return text
    .split(/[,，]/)
    .map((item) => item.trim())
    .filter(Boolean)
}

/**
 * 快照的 `message` → 一句人话。
 *
 * 快照是「操作之前自动留一份」，所以 message 存的是**操作的标识符**
 * （`batch_delete`、`replace_text`…）：界面与 MCP 共用一条历史，标识符是两边
 * 唯一都能给出的东西，让 Rust 侧去拼中文文案就得为每个调用点各写一份。
 *
 * 翻译放在这里，认不出来的**原样显示**：MCP 那侧加写工具时不必同步改前端，
 * 最坏情况是列表里出现一个工具名，而不是一片空白。回滚自己留下的那两笔
 * （「回滚到 …」）本来就是中文，也走这条原样通道。
 *
 * 只列真会出现的名字：界面这边是 `with_writing_session` 的那批操作，
 * Agent 那边是 MCP 的写工具名。

 */
const OPERATIONS: Record<string, string> = {
  // 界面上的破坏性操作
  delete_content: '删除文章之前',
  batch_edit_tags: '批量改标签之前',
  batch_set_draft: '批量改草稿状态之前',
  batch_move: '批量搬动之前',
  batch_delete: '批量删除之前',
  apply_replace: '跨文件替换之前',
  rename_section: '栏目改名之前',
  remove_section: '删除栏目之前',
  import_content: '导入内容之前',
  import_theme: '装入主题包之前',
  save_config: '保存站点设置之前',
  save_config_source: '手改配置原文之前',
  remove_media: '删除媒体之前',
  // Agent（MCP 写工具）
  create_content: 'Agent 新建文章之前',
  write_content: 'Agent 改文章之前',
  patch_front_matter: 'Agent 改 front matter 之前',
  replace_text: 'Agent 跨文件替换之前',
  move_content: 'Agent 搬动文章之前',
  create_section: 'Agent 新建栏目之前',
  write_template: 'Agent 改模板之前',
}



export function snapshotLabel(message: string): string {
  return OPERATIONS[message] ?? message
}

