/// <reference types="vite/client" />

/**
 * `node:fs` 里我们真正用到的那一个函数。
 *
 * 只有测试用得到它（`src/styles.test.ts` 要读样式表的原文）。前端这边刻意没装
 * `@types/node`——为一个测试拉进一整套 node 类型不值得，而 `?raw` 那条路走不通：
 * Vitest 会把所有 `.css` 请求换成空串，断言就变成对着空字符串通过，护栏是假的。
 *
 * 所以这里只声明用到的签名，真实实现由跑测试的 node 提供。
 */
declare module 'node:fs' {
  export function readFileSync(path: string, encoding: 'utf8'): string
}

declare module 'node:url' {
  /** 用它把 `import.meta.url` 转成路径：手撕 `file://` 前缀在 Windows 上会多一个斜杠。 */
  export function fileURLToPath(url: string | URL): string
}
