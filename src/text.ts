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
