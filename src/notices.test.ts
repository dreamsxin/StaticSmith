import { describe, expect, it } from 'vitest'

import { appendNotice, formatNoticeTime, type Notice, unseenCount } from './notices'

function notice(id: number, level: Notice['level'] = 'success'): Notice {
  return { id, level, message: `第 ${id} 条`, at: id }
}

describe('通知历史', () => {
  it('新的排在前面', () => {
    const list = appendNotice(appendNotice([], notice(1)), notice(2))
    expect(list.map((item) => item.id)).toEqual([2, 1])
  })

  it('超出上限丢掉最旧的那些', () => {
    let list: Notice[] = []
    for (let id = 1; id <= 5; id += 1) {
      list = appendNotice(list, notice(id), 3)
    }
    expect(list.map((item) => item.id)).toEqual([5, 4, 3])
  })

  it('不改动传进来的数组', () => {
    const original = [notice(1)]
    appendNotice(original, notice(2))
    expect(original.map((item) => item.id)).toEqual([1])
  })

  it('未读数按 id 比较，面板开着又来新消息也算得对', () => {
    const list = [notice(3), notice(2), notice(1)]
    expect(unseenCount(list, 0)).toBe(3)
    expect(unseenCount(list, 2)).toBe(1)
    expect(unseenCount(list, 3)).toBe(0)
  })

  it('时间只显示到秒，用本地时区', () => {
    const at = new Date(2026, 0, 2, 3, 4, 5).getTime()
    expect(formatNoticeTime(at)).toBe('03:04:05')
  })
})
