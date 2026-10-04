# README 预览与截图维护

README 优先展示产品定位、总览截图和下载路径。功能细节、其他页面截图与开发命令按需展开，避免 GitHub 首页被长段说明或过小的并排截图占满。

## 截图来源

- 图片由当前 React 前端直接渲染，保留侧栏、筛选、状态和页脚，未拼接或修改界面样式。
- 使用 `scripts/capture-readme.cjs` 中固定的演示数据，时间为 2026-10-04；不读取用户日志、账号信息或本地数据库。
- 模拟 IPC 只注入独立 Playwright 浏览器，生产应用没有演示数据入口。未实现的原生操作会报错，不会转发到本机后端。
- 总览、模型和项目的 Token、成本与会话汇总一致；工具趋势、类别与 Top 工具计数一致。价格来自项目内置快照，不代表截图生成时的在线价格。
- 页脚包含 `DEMO`，README 中也明确标注演示数据。截图用于展示界面，不是实测用量、性能或真实用户结果。

## 重新截图

先在项目根目录运行开发服务器：

```powershell
npm run dev -- --host 127.0.0.1
```

在另一个终端使用可用的 Playwright CLI 创建独立浏览器会话：

```powershell
playwright-cli -s=chronolume-readme open http://127.0.0.1:1420/
playwright-cli -s=chronolume-readme run-code --filename=scripts/capture-readme.cjs
playwright-cli -s=chronolume-readme close
```

脚本从 1440 × 900 CSS 像素视口开始，按实际内容高度调整视口后截图，固定时钟并关闭动画，等待图表和字体就绪。截图路径固定为 `docs/images/chronolume-*.png`，会覆盖对应的 README 图片；应在执行前检查工作区，避免覆盖其他人尚未保存的截图改动。

维护时同步检查：

1. 数据结构与当前 `ui/src/types.ts` / IPC 实现匹配；版本与价格快照更新时同步调整演示夹具。
2. 七张图没有加载骨架、错误提示、光标悬浮提示或意外裁切；表格包含操作列，图表正常显示。
3. README 的图片路径、跳转与 `<details>` 都有效；在窄屏上避免多列图片布局。
4. 总览 `<picture>` 的深浅图片内容相同，使用不同主题，`<img>` 提供浅色回退与明确的替代文本。

## GitHub 展示

使用 GitHub 支持的原生 Markdown 和 HTML，不依赖自定义 CSS 或脚本。总览通过 [`<picture>` 与 `prefers-color-scheme`](https://docs.github.com/en/get-started/writing-on-github/getting-started-with-writing-and-formatting-on-github/quickstart-for-writing-on-github) 选择深浅主题图片；额外截图与命令采用 [`<details>` / `<summary>`](https://docs.github.com/en/get-started/writing-on-github/working-with-advanced-formatting/organizing-information-with-collapsed-sections) 折叠展示。

本地视觉预览仅用于排版检查。GitHub 的最终页面需要提交并推送 README 与图片后才会更新；截图不能证明原生 WebView、IPC 或桌面分发链路已经验收。

## 本次验证记录

- 七张图片重新生成，检查了主内容、表格操作列、图表、主题与 DEMO 标识；截图过程中无页面运行错误。
- 使用 GitHub Markdown API 渲染 README，在真实仓库页面的本地 DOM 预览中替换文章内容，并以本地文件响应新图片请求；没有修改或发布远端仓库内容。
- 检查 1440 / 390 像素宽度 × 浅色 / 暗色四组组合：无页面横向溢出，无失效图片或目录跳转；八个折叠区均可使用键盘打开与关闭，展开后也无页面横向溢出。
- 验证所有本地图片与文档引用存在，截图脚本语法与 `git diff --check` 通过。README 展示优化阶段仅调整文档、图片和截图工具；生产前端的改进及验证另见 [前端质量记录](frontend-quality.md)。
