/**
 * 批量动作的「跳过汇总」。
 *
 * 批量改的是磁盘上的源文，而这个项目**不做撤销栈**（理由见 `store.afterBatch`），
 * 所以「跳过了哪几篇、为什么」是唯一的补偿。它必须同时满足两件互相拉扯的要求：
 * 一行之内说清全局（气泡里只有一行），以及事后查得到每一篇（消息中心里的详情）。
 *
 * 逻辑单独放在这里而不是塞进 store：这样它不依赖 Tauri 也不依赖 reactive 状态，
 * 能被直接测到（同 `notices.ts`）。
 */

/** 与 `api.Skipped` 结构一致，但不引它：这里只需要这两个字段。 */
interface Skip {
  source: string
  reason: string
}


/** 一行里最多列几种原因。再多就成了没人读的一长串，剩下的给个数、详情里有全部。 */
const MAX_REASONS = 3

export interface SkipSummary {
  /** 一行汇总，进气泡。没有跳过时是空串。 */
  message: string
  /** 每一篇一行（`源文件：原因`），进通知历史的详情。 */
  details: string[]
}

export function summarizeSkips(skipped: readonly Skip[]): SkipSummary {
  if (skipped.length === 0) return { message: '', details: [] }

  const details = skipped.map((item) => `${item.source}：${item.reason}`)

  // 一篇的时候报出是哪一篇：一篇是找得到的，不必只给数量
  if (skipped.length === 1) {
    return { message: `跳过 1 篇：${skipped[0].source}「${skipped[0].reason}」`, details }
  }

  const byReason = new Map<string, number>()
  for (const item of skipped) byReason.set(item.reason, (byReason.get(item.reason) ?? 0) + 1)

  // 都是同一个原因时说出来：只报「跳过 3 篇」会让人以为还有别的情况没露面
  if (byReason.size === 1) {
    return { message: `跳过 ${skipped.length} 篇，都是「${skipped[0].reason}」`, details }
  }

  const sorted = [...byReason.entries()].sort(([, a], [, b]) => b - a)
  const listed = sorted.slice(0, MAX_REASONS).map(([reason, count]) => `${count} 篇「${reason}」`)
  const rest = sorted.length > MAX_REASONS ? `，另有 ${sorted.length - MAX_REASONS} 种原因` : ''
  return { message: `跳过 ${skipped.length} 篇：${listed.join('、')}${rest}`, details }
}
