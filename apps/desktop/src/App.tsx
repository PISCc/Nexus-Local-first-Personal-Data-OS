const principles = [
  {
    index: "01",
    title: "默认本地运行",
    description: "除非你主动选择，源数据始终保留在这台设备上。",
  },
  {
    index: "02",
    title: "先搜索，再用人工智能",
    description: "确定性的检索能力是基础，而不是事后补上的功能。",
  },
  {
    index: "03",
    title: "来源始终可追溯",
    description: "未来的每个回答，都应能回到它所依据的原始文档。",
  },
];

function App() {
  return (
    <div className="app-shell">
      <aside className="side-rail" aria-label="Nexus 导航">
        <div>
          <div className="brand-lockup">
            <span className="brand-index">N / 00</span>
            <span className="brand-name">Nexus</span>
          </div>
          <p className="rail-caption">个人数据操作系统</p>
        </div>

        <nav className="rail-nav" aria-label="里程碑">
          <div className="rail-item active" aria-current="page">
            <span className="rail-item-index">00</span>
            <span className="rail-item-label">工程基础</span>
            <span className="rail-item-state">当前</span>
          </div>
          <div className="rail-item quiet">
            <span className="rail-item-index">01</span>
            <span className="rail-item-label">文件索引</span>
            <span className="rail-item-state">下一步</span>
          </div>
          <div className="rail-item quiet">
            <span className="rail-item-index">03</span>
            <span className="rail-item-label">全文搜索</span>
            <span className="rail-item-state">后续</span>
          </div>
        </nav>

        <div className="rail-footer">
          <span className="rail-footer-label">存储模式</span>
          <div className="local-badge">
            <span className="status-dot" aria-hidden="true" />
            仅本地
          </div>
          <p>让重要数据始终留在身边。</p>
        </div>
      </aside>

      <main className="main-surface">
        <header className="topbar">
          <div className="breadcrumb">
            <span>工作区</span>
            <span className="breadcrumb-separator" aria-hidden="true">
              /
            </span>
            <strong>工程基础</strong>
          </div>
          <div className="topbar-meta">
            <span className="version-pill">v0.1.0</span>
            <span className="topbar-note">离线优先</span>
          </div>
        </header>

        <div className="content-wrap">
          <section className="hero" aria-labelledby="hero-title">
            <div className="hero-copy">
              <p className="eyebrow">Nexus / M0 工程基础</p>
              <h1 id="hero-title">
                个人数据，
                <span>留在身边。</span>
              </h1>
              <p className="hero-lede">
                为文件、笔记、代码与想法搭建一处安静可靠的本地基础。
              </p>
              <div className="hero-meta">
                <span>当前界面</span>
                <strong>桌面基础</strong>
              </div>
            </div>

            <section className="status-card" aria-labelledby="status-title">
              <div className="status-card-header">
                <span className="eyebrow">运行状态</span>
                <span className="status-state">
                  <span className="status-dot" aria-hidden="true" />
                  本地 / 就绪
                </span>
              </div>
              <div className="status-card-body">
                <span className="status-card-index" aria-hidden="true">
                  01
                </span>
                <div>
                  <h2 id="status-title">基础界面已启动。</h2>
                  <p>第一版界面保持克制，为本地核心留出可靠的生长空间。</p>
                </div>
              </div>
              <div className="status-card-footer">
                <span>核心连接</span>
                <span className="footer-rule" aria-hidden="true" />
                <strong>尚未连接</strong>
              </div>
            </section>
          </section>

          <section className="principles" aria-labelledby="principles-title">
            <div className="section-heading">
              <p className="eyebrow">运行原则</p>
              <h2 id="principles-title">先建立信任。</h2>
            </div>
            <div className="principles-grid">
              {principles.map((principle) => (
                <article className="principle" key={principle.index}>
                  <span className="principle-index">{principle.index}</span>
                  <h3>{principle.title}</h3>
                  <p>{principle.description}</p>
                </article>
              ))}
            </div>
          </section>

          <section className="next-step" aria-label="下一里程碑">
            <div>
              <span className="eyebrow">下一步</span>
              <strong>M1 / 本地文件扫描</strong>
            </div>
            <span className="next-step-copy">先记录元数据，再理解内容。</span>
            <span className="next-step-arrow" aria-hidden="true">
              →
            </span>
          </section>

          <footer className="page-footer">
            <span>Nexus / 本地优先个人数据操作系统</span>
            <span>工程基础 · 2026</span>
          </footer>
        </div>
      </main>
    </div>
  );
}

export default App;
