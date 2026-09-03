# Nexus Search Demo

这是一个独立的视觉与交互演示页，不连接 Tauri 核心，也不会读取或上传本地文件。

打开 `Nexus Search Demo.html` 即可查看；如果浏览器限制了 `file://` 页面中的资源，可以在仓库根目录运行：

```powershell
python -m http.server 4173 --directory demo
```

然后访问 <http://127.0.0.1:4173/Nexus%20Search%20Demo.html>。

## 简单风格演示

要先看基于 Nexus 图标的新视觉方向，打开 `Nexus Visual Direction.html`。它只保留 Hero、图标视觉和一个静态搜索/来源预览，不连接 Tauri 核心。

使用本地服务器时访问：<http://127.0.0.1:4173/Nexus%20Visual%20Direction.html>

当前参考页对齐版本：<http://127.0.0.1:4173/Nexus%20Search%20Demo.html?showcase=reference-fidelity-v3>。

演示内容：

- 首页状态带会随当前查询、命中数量、文件类型过滤和来源模式更新
- 视觉节奏参考 SaaS Analytics Dashboard：圆角白色顶栏、蓝紫数据氛围、分栏 Hero、图表预览、行动/指标行和三项能力卡
- 搜索输入、示例查询、文件类型过滤和空结果状态
- `Ctrl K` / `/` 聚焦搜索，`Esc` 清除搜索或关闭调整面板
- Paper / Night / Hard contrast 三种调色变体
- Soft feedback / Reduced motion 两种动效状态
- 可见键盘焦点、跳过链接、语义按钮、原生 `dialog` 调整面板

首页聚焦层采用 Aceternity UI 官方 Spotlight 组件的视觉模式，并针对这个无依赖演示页做了 CSS 适配：只执行一次入场动画，不引入 React/Tailwind 运行时，也不把内容改造成卡片网格。

页面使用 Google Fonts 作为可选增强；网络不可用时会回退到 Windows 本地字体，不影响功能演示。
