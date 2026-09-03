# Nexus design system

先读 `MASTER.md`，再按页面读取 `pages/` 下的 override。

- `MASTER.md`：Nexus 全局产品风格规范与不可偏离的 UX 规则。
- `pages/showcase.md`：当前简版展示页的范围和交互预算。
- `legacy-style-migration-prompt.md`：把旧版产品界面迁移到当前风格的可复制提示词。

当前桌面界面已经确认为最终视觉与交互基线：生产实现以
`apps/desktop/src/index.css`、`apps/desktop/src/App.tsx` 和
`apps/desktop/src/SearchView.tsx` 为准；`demo/Nexus Visual Direction.html`
用于补充说明视觉方向。若实现、参考页与规范冲突，以 `MASTER.md` 的
语义规则为判断依据并修正偏差，不得自行建立另一套风格。替换基线必须
先取得明确的产品决策，再同步更新规范、参考页与生产实现。
