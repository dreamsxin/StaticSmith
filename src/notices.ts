/**
 * 通知历史。
 *
 * 通知本身是瞬时的（错误 8 秒、其余 3.5 秒后自动消失），这在「刚点了一下」的那几秒里
 * 够用，之后就彻底没了：手改配置存盘失败时，Rust 侧那条带行列号的 TOML 报错飘走之后
 * 无处可查，用户只知道「保存没成功」。
 *
 * 所以留一份历史。逻辑单独放在这里而不是塞进 store：这样它能不依赖 Tauri 直接被测到，
 * 而 store 里那套 reactive 状态在测试环境里跑不起来。
 */

/**
 * 通知的严重程度，与 toast 的三种样式同名（`success` / `info` / `error`）。
 *
 * 刻意与 `ToastKind` 用同一套取值而不是另起一套：两者说的是同一件事，
 * 分成两套就得有个翻译层，而翻译层是迟早会漏一种情况的地方。
 */
export type NoticeLevel = 'success' | 'info' | 'error'

export interface Notice {
  id: number
  level: NoticeLevel
  message: string
  /** 毫秒时间戳，显示时才格式化 */
  at: number
  /**
   * 一行装不下的细节，每项一行（批量动作里被跳过的每一篇、一长串警告）。
   *
   * 气泡里只显示 `message`：它几秒后就消失，塞十几行进去没人读得完。
   * 详情只在消息中心展开——「跳过 12 篇」那一刻用户需要的是数量与原因，
   * 「到底是哪几篇」是事后才去查的。
   *
   * 声明成 `readonly`：store 导出的状态是深只读的，可变数组类型会在传回这里时被拒。
   */
  details?: readonly string[]

}


/**
 * 历史最多留多少条。
 *
 * 100 条足够回溯「刚才那一串操作」，又不至于让面板变成一份需要翻页的日志——
 * 真要查更早的东西该去看日志文件，那不是这个面板的活。
 */
export const NOTICE_LIMIT = 100

/**
 * 追加一条通知，返回新数组（**新的在前**）。
 *
 * 返回新数组而不是原地 push：调用方是 reactive 状态，整体替换比原地增删更难出错，
 * 而这个列表最多 100 条，复制的代价可以忽略。
 */
export function appendNotice(
  list: readonly Notice[],
  notice: Notice,
  limit = NOTICE_LIMIT,
): Notice[] {
  return [notice, ...list].slice(0, Math.max(1, limit))
}

/**
 * 有多少条是上次看过之后新来的。
 *
 * 用「比 `lastSeenId` 大」而不是存一个布尔的已读标记：通知随时可能进来，
 * 布尔标记在「面板开着又来了新消息」时会算错。
 */
export function unseenCount(list: readonly Notice[], lastSeenId: number): number {
  return list.filter((notice) => notice.id > lastSeenId).length
}

/**
 * 时间戳格式化成 `HH:mm:ss`。
 *
 * 不带日期：历史只留 100 条，跨天的情况下日期意义不大，而时分秒才是
 * 「这条是刚才那次操作的吗」要对的东西。
 */
export function formatNoticeTime(at: number): string {
  const date = new Date(at)
  const pad = (value: number) => String(value).padStart(2, '0')
  return `${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(date.getSeconds())}`
}
