import { describe, expect, it } from 'vitest'

import { nextFocusIndex } from './focus'

describe('nextFocusIndex', () => {
  it('正向到最后一个就回卷到第一个', () => {
    expect(nextFocusIndex(3, 2, false)).toBe(0)
  })

  it('反向到第一个就回卷到最后一个', () => {
    expect(nextFocusIndex(3, 0, true)).toBe(2)
  })

  it('中间位置交给浏览器默认行为', () => {
    expect(nextFocusIndex(3, 1, false)).toBeNull()
    expect(nextFocusIndex(3, 1, true)).toBeNull()
  })

  it('焦点在浮层外时收回来：正向第一个、反向最后一个', () => {
    expect(nextFocusIndex(3, -1, false)).toBe(0)
    expect(nextFocusIndex(3, -1, true)).toBe(2)
  })

  it('只有一个可聚焦元素时 Tab 停在原地', () => {
    expect(nextFocusIndex(1, 0, false)).toBe(0)
    expect(nextFocusIndex(1, 0, true)).toBe(0)
  })

  it('没有可聚焦元素时不拦 Tab', () => {
    expect(nextFocusIndex(0, -1, false)).toBeNull()
  })
})
