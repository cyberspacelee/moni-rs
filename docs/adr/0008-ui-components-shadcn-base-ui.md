# ADR-0008: 界面组件改用 shadcn/ui（Base UI）+ Animate UI

- 状态：已接受
- 日期：2026-10-19
- 取代：ADR-0007 中「不引入组件库」一条；其余决策不变

## 背景

v1 前端的组件全部手写在一个文件里：原生 `<dialog>`、原生 `<select>`、原生复选框，以及 `confirm()` / `alert()`。主题系统管不到这些原生控件的弹出层，在手机上 `title` 提示也看不到。键盘操作、焦点管理和读屏支持只能逐个补，迁移前已经出现过对话框缺少可访问名称这类问题。

## 决策

- 组件来自 shadcn/ui（`base-nova` 风格），底层 primitive 用 Base UI（`@base-ui/react`）。组件源码由 CLI 生成到 `web/src/components/ui/`，归项目所有，可以直接修改。`components.json` 记录了生成配置。
- 下列组件经 Animate UI CLI（`@animate-ui/cli animate <组件>`）处理，把 primitive 换成 `@animate-ui/react` 中由 motion 驱动的版本：Dialog、AlertDialog、Select、Switch、Tabs、ToggleGroup、Toggle、Tooltip、RadioGroup、Checkbox、DropdownMenu。Drawer 和 Toast 保留 Base UI 的 CSS 过渡，因为这两个组件的滑动手势和堆叠动画依赖 CSS 变量。Progress 不使用 motion，因为仪表每帧都会更新。`AnimateUIProvider` 默认遵循系统的「减少动态效果」设置。
- 主题仍由 `web/src/themes/*.css` 定义调色板（`--bg`、`--surface`、`--ink`、`--brand`……）。`index.css` 把 shadcn 的语义角色（`--background`、`--card`、`--primary`、`--muted-foreground`……）映射到这套调色板，并在每个 `[data-theme]` 元素上重新声明，所以作用域内的主题预览也能正确着色。圆角沿用各主题覆盖的 `--radius-md/lg/xl/2xl`；`--radius-sm` 和 `--radius-3xl/4xl` 由这几个值推导，因此直角主题会在所有组件上保持直角。
- 图标直接使用 lucide-react，与 shadcn 一致：尺寸由组件的 `[&_svg]` 规则控制，按钮内的图标标注 `data-icon="inline-start"`。只有品牌标识（moni logo、GitHub）保留手绘 SVG。
- 图表仍用 uPlot。shadcn Chart 基于 Recharts，体积更大，也不适合实时追加数据。
- 包管理器统一为 pnpm workspace（根目录放 wrangler，`web/` 是 workspace 成员），只保留一份 `pnpm-lock.yaml`。

## 后果

- 首页需要加载的 JS 从约 122 KB 增加到约 232 KB（gzip），其中 Base UI 和 motion 大约各占一半增量。登录、服务器详情（含 uPlot）、管理页和外观选择器都改为按需加载。静态资源不计入 Workers 免费额度，主要代价是首屏下载量。
- 确认操作改用页内的 AlertDialog，错误提示改用 Toast。浏览器冒烟测试改为与这些角色交互，出现原生对话框即视为回归。
- 升级组件的方式：`pnpm dlx shadcn@latest add <组件> -o`，然后对需要动画的组件重新运行 Animate UI CLI，并检查本地对卡片、对话框、徽章所做的主题化修改（`card`/`shadow-card`、`rounded-2xl`、`rounded-pill`、中文文案）是否被覆盖。

## 备选方案

- 继续手写组件：体积最小，但可访问性和交互细节都要自己维护，原生控件的弹出层也无法主题化。
- shadcn + Radix：功能等价。shadcn 已把 Base UI 作为默认底层，Base UI 由同一团队持续维护，而且 Drawer、Toast、Meter 等组件不需要额外依赖。
- 所有组件都接入 Animate UI：Drawer 和 Toast 的手势动画会退化，Progress 每帧都会触发 motion 计算。
