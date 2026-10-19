# ADR-0007: 前端技术栈

- 状态：已接受；「不引入组件库」一条被 ADR-0008 取代
- 日期：2026-10-17

## 决策
- React 19 + TypeScript + Vite 8 + Tailwind CSS v4（`@tailwindcss/vite`），图表使用 uPlot（约 50 KB，时间序列性能好）。
- ~~不引入组件库~~（见 ADR-0008）；不引入状态管理库：使用 `useSyncExternalStore` + 一个模块级 store 承载 WS 实时状态；路由使用 hash 路由（简单的自实现），因为静态资源不需要配置 SPA fallback。
- API 类型由 Rust `protocol` crate 通过 ts-rs 生成，前端不手写 DTO。
- 产物 `web/dist` 由 Worker Static Assets 托管，请求免费。
