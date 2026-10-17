# ADR-0001: Rust 全栈，Cargo workspace 按 Native / WASM 隔离

- 状态：已接受
- 日期：2026-10-17

## 背景
Agent 运行在 Linux VPS（native），后端运行在 Cloudflare Workers（wasm32-unknown-unknown）。两者都想用 Rust，但运行时依赖不兼容（tokio/sysinfo vs worker/wasm-bindgen）。

## 决策
- 单仓库 Cargo workspace：
  - `crates/protocol`：纯 `serde` 消息模型，无运行时依赖，两端共用。
  - `crates/agent`：native，tokio + tokio-tungstenite + rustls。
  - `crates/worker`：wasm，`worker` crate（workers-rs 0.8.x），`crate-type = ["cdylib"]`。
- 后续纯逻辑（告警判定、聚合）放 `crates/core`，同样保持无运行时依赖。
- 前端 React + TypeScript（`web/`），构建产物由 Worker Static Assets 托管。

## 后果
- `cargo build` 在 workspace 根目录默认只构建 native 成员；worker 由 `worker-build` 单独构建（`default-members` 排除 worker）。
- 协议变更一处修改，两端编译期检查。

## 备选方案
- TypeScript 后端：生态更成熟，但放弃协议共享与「全 Rust」目标。
