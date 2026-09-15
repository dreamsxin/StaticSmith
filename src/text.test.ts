import { describe, expect, it } from 'vitest'

import { formatBytes, parseList, snapshotLabel } from './text'



describe('parseList', () => {
  /**
   * 全角逗号是这个函数存在的理由：中文输入法下打出来的是 `，`。
   * 属性面板与批量动作共用它——一处认全角另一处不认，症状会是
   * 「有时候多一个空标签」，很难描述也很难查。
   */
  it('半角与全角逗号都认', () => {
    expect(parseList('a,b，c')).toEqual(['a', 'b', 'c'])
  })

  it('去空白、丢空项', () => {
    expect(parseList(' a , ,b ,，, c ')).toEqual(['a', 'b', 'c'])
  })

  it('空输入得到空数组，而不是一个空字符串', () => {
    expect(parseList('')).toEqual([])
    expect(parseList('  ')).toEqual([])
    expect(parseList(',，')).toEqual([])
  })

  /** 标签里的空格是有意义的，不该被当成分隔符。 */
  it('不按空格切分', () => {
    expect(parseList('hello world, 前端 工程')).toEqual(['hello world', '前端 工程'])
  })
})

/**
 * 字节数的说法只该有一份。
 *
 * 这段原先在 `BuildPanel`、`DeployPanel`、`SeoPanel` 里各写了一遍，收进 `text.ts`
 * 之后调用方变成四处（还多了快照占用），措辞改错一个字没人拦得住——所以钉住换挡点。
 */
describe('formatBytes', () => {
  it('1 KB 以下给字节：小文件报「0.1 KB」等于没说', () => {
    expect(formatBytes(0)).toBe('0 B')
    expect(formatBytes(1023)).toBe('1023 B')
  })

  it('KB 与 MB 各留一位小数', () => {
    expect(formatBytes(1024)).toBe('1.0 KB')
    expect(formatBytes(1536)).toBe('1.5 KB')
    expect(formatBytes(3 * 1024 * 1024)).toBe('3.0 MB')
  })

  it('换挡点在 1024 而不是 1000', () => {
    expect(formatBytes(1000)).toBe('1000 B')
    expect(formatBytes(1024 * 1024 - 1)).toContain('KB')
    expect(formatBytes(1024 * 1024)).toBe('1.0 MB')
  })
})

describe('snapshotLabel', () => {

  it('界面与 Agent 的操作各有说法', () => {
    expect(snapshotLabel('batch_delete')).toBe('批量删除之前')
    expect(snapshotLabel('write_content')).toBe('Agent 改文章之前')
  })

  /**
   * 认不出来的原样显示。
   *
   * 两种情况都会走到这里：MCP 那侧加了新写工具而前端还没跟上，
   * 以及回滚自己留下的那两笔（本来就是中文）。二者都不能变成空白——
   * 快照列表里一行没有说明，等于让人在一堆哈希里盲选。
   */
  it('不认识的 message 原样显示', () => {
    expect(snapshotLabel('some_new_tool')).toBe('some_new_tool')
    expect(snapshotLabel('回滚到 a1b2c3d4e5')).toBe('回滚到 a1b2c3d4e5')
  })
})

