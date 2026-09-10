import { describe, expect, it } from 'vitest'

import { parseList, snapshotLabel } from './text'


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

