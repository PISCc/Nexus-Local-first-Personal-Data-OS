# Prompt: migrate the legacy Nexus UI to the current visual style

下面这段提示词可以直接交给 Codex 或其他开发代理，用于把旧版 Nexus 产品界面迁移到当前展示页的风格。

```text
你正在修改 Nexus，一个 Local-first Personal Data OS。请先阅读并遵守：

1. C:/Users/wzlSX1756/Documents/ChatGPT/Nexus/AGENTS.md
2. C:/Users/wzlSX1756/Documents/ChatGPT/Nexus/design-system/nexus/MASTER.md
3. C:/Users/wzlSX1756/Documents/ChatGPT/Nexus/design-system/nexus/pages/showcase.md
4. C:/Users/wzlSX1756/Documents/ChatGPT/Nexus/demo/Nexus Visual Direction.html

任务：把当前旧版产品界面迁移为“Organic Memory Field / 有机记忆场”风格。当前展示页是视觉参考，不是让你复制文案或静态布局；请将这套视觉语言适配到现有产品的真实界面。

视觉方向：
- 背景使用温暖的奶油薄荷纸面：#F7FBF6 / #EAF6EE。
- 正文使用深墨蓝：#0B2D35；辅助文字使用 #4D6468。
- 主操作、搜索、查询节点使用钴蓝：#155FE8；hover/pressed 使用 #0F48BE。
- 本地状态、来源追溯、深色面板使用深青：#07584F / #082F3B。
- Coral #E86D4E 只用于有意义的命中、焦点或节点信号，不要大面积使用。
- 标题/数字使用 Poppins（中文使用系统回退）；正文/控件使用 DM Sans；路径、文件类型、状态和索引标签使用 IBM Plex Mono。
- 面板使用 23–28px 圆角，顶栏使用 24px；使用细边框与低强度蓝青阴影，不使用厚黑阴影。
- 使用真实产品图标：C:/Users/wzlSX1756/Documents/ChatGPT/Nexus/demo/assets/nexus-brand/nexus-product-icon.png。不要用 CSS 重画图标，不要套滤镜改色，不要发明第二套 Logo。

必须保留：
- 现有 Tauri 命令、Rust/TypeScript 数据结构、搜索/索引业务逻辑和错误处理。
- 现有路由、锚点、导航语义、表单字段名/顺序、测试选择器和可访问性语义，除非先明确说明需要变更。
- “本地优先”“搜索优先于 AI”“来源可追溯”的产品原则。
- 浏览器预览没有 Tauri 核心时的降级状态，不要伪造已经连接成功。

实现要求：
- 先检查现有代码与测试，区分业务逻辑和 UI 层；只修改视觉和必要的交互反馈，不做无关重构。
- 让搜索成为视觉和交互主轴：输入框、结果、路径、来源轨迹要有清晰层级。
- 状态不仅用颜色表达，必须有文字或结构信号；补齐 loading、empty、error、disabled、focus 状态。
- 所有可点击元素最小 44×44px，使用明显的 :focus-visible；表单要有 label；结果更新使用 aria-live（如果适用）。
- 动效保持低频：只允许一次 Hero 入场和一次必要的状态反馈；只动画 transform/opacity；支持 prefers-reduced-motion。
- 响应式检查 375px、768px、1024px、1440px，不允许横向滚动或内容被 sticky 导航遮挡。
- 不新增 React/Tailwind/动画库或其他依赖，除非先说明理由并获得批准；优先用 CSS、内联 SVG 和现有资源。

禁止：
- 紫粉蓝 AI 渐变、赛博霓虹、过度毛玻璃、整页卡片堆叠。
- Emoji 作为图标、虚构客户/数据/评价、未实现的云处理承诺、凭空增加价格/注册/账号入口。
- 用 placeholder-only 的搜索输入、hover-only 操作、无结果空白屏、不可见焦点、颜色单独表达状态。

交付顺序：
1. 先给出简短的改造计划和受保护契约。
2. 修改最少的相关 UI 文件。
3. 添加或更新必要的 UI 回归测试，不修改与本次风格迁移无关的测试。
4. 运行相关的 typecheck、lint、test、build，并只报告实际执行过的检查。
5. 最后复查 diff，列出改动文件、保留的行为、测试结果和未解决风险。
```
