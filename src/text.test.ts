import { describe, expect, it } from 'vitest'

import { parseList } from './text'

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
