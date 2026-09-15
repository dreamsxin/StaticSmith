import { describe, expect, it } from 'vitest'

import { summarizeSkips } from './skips'

/**
 * 批量动作的「跳过汇总」。
 *
 * 批量动作**没有撤销栈**，所以「跳过了哪几篇、为什么」是唯一的补偿。原先只报第一条原因
 * 加一句「另有 N 篇被跳过」：五篇里两种原因时，第二种原因根本没露过面，
 * 而用户会按看到的那一条去排查——查错方向比不给信息更费时间。
 */
describe('summarizeSkips', () => {
  const skip = (source: string, reason: string) => ({ source, reason })

  it('没有跳过就没有话说', () => {
    expect(summarizeSkips([])).toEqual({ message: '', details: [] })
  })

  it('只跳过一篇时报出是哪一篇：一篇是找得到的，不必只给数量', () => {
    const result = summarizeSkips([skip('posts/a.md', 'front matter 读不出来')])
    expect(result.message).toBe('跳过 1 篇：posts/a.md「front matter 读不出来」')
  })

  it('多篇同一个原因：说清是同一个原因，别让人以为还有别的情况', () => {
    const result = summarizeSkips([
      skip('posts/a.md', 'front matter 读不出来'),
      skip('posts/b.md', 'front matter 读不出来'),
      skip('posts/c.md', 'front matter 读不出来'),
    ])
    expect(result.message).toBe('跳过 3 篇，都是「front matter 读不出来」')
  })

  it('多种原因按篇数从多到少列出，每种都有数量', () => {
    const result = summarizeSkips([
      skip('posts/a.md', '目标已存在'),
      skip('posts/b.md', 'front matter 读不出来'),
      skip('posts/c.md', 'front matter 读不出来'),
    ])
    expect(result.message).toBe(
      '跳过 3 篇：2 篇「front matter 读不出来」、1 篇「目标已存在」',
    )
  })

  it('原因种类太多时只列前三种，剩下的给个数：一行里塞不下六种', () => {
    const result = summarizeSkips([
      skip('a.md', '甲'),
      skip('b.md', '甲'),
      skip('c.md', '乙'),
      skip('d.md', '丙'),
      skip('e.md', '丁'),
      skip('f.md', '戊'),
    ])
    expect(result.message).toBe('跳过 6 篇：2 篇「甲」、1 篇「乙」、1 篇「丙」，另有 2 种原因')
  })

  it('每一篇都进详情：汇总只有一行，而「到底是哪几篇」得能查', () => {
    const result = summarizeSkips([
      skip('posts/a.md', '目标已存在'),
      skip('posts/b.md', 'front matter 读不出来'),
    ])
    expect(result.details).toEqual([
      'posts/a.md：目标已存在',
      'posts/b.md：front matter 读不出来',
    ])
  })
})
