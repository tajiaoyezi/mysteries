# 2026-06-27 · 01 · UI 原型设计契约落地与 D1/D2 定案

## 决策

- **用 web 原型驱动 TUI 实现的机制** | 选:把原型蒸馏成 text 设计契约(`设计规范/`),由 `openspec/config.yaml` 的 `rules` 强制每个 UI change 引用,再用 insta 快照 + `theme.rs` token 单测兜漂移,首帧快照设一个人类对眼关卡 | 弃:直接拿原型 HTML 当 spec(bundled blob 读不动 / diff 不了 / 不在权威链)、纯靠 Agent 自觉照着画(无约束力) | 主导:讨论收敛(用户选「现在落地」+「独立 `设计规范/` 目录」) | 依据:spec(原型) + CLAUDE.md 权威次序
  - 保真标准 = **语义保真**非像素一致(web→TUI 降维);元素强制 port / adapt / drop 三分类。
  - 视觉权威次序:`theme.rs` / insta(code+tests) > 设计契约(text) > 原型 HTML(人工参考) > Agent 推断;嵌入 CLAUDE.md 既有 `code > spec > Agent`。

- **D1 顶栏 header** | 选:§8 纳入顶栏,但**只放品牌**(`✦ mysteries agent · v1.0`),provider/model 归状态行、不重复 | 弃:去掉顶栏(原型有、层级需要)、顶栏照搬原型含 provider/model(与状态行重复) | 主导:用户(「按推荐执行」) | 依据:原型(Midnight) + 技术方案 §8
  - 冲突来源:原型有常驻顶栏,§8 原布局(transcript / 状态行 / 输入框)未列。

- **D2 状态 / meta 位置** | 选:**以 Midnight 为准 = 底部状态行** | 弃:Daylight 的「顶栏右侧承载实时状态 + meta」(与用户指定参考稿不符,且偏离 §8「状态行」) | 主导:讨论收敛(读 Daylight 渲染图时发现两套原型不一致) | 依据:原型(Midnight 参考稿) + 技术方案 §8
  - 这是**跨原型(Midnight vs Daylight)的视觉冲突**,在蒸馏 / 截图过程中暴露——印证「先蒸馏成可引用 text」能在开发前抓出原型自身的不一致。Daylight 顶栏布局视为浅色变体差异,实现统一到 Midnight。

## 变更

- 新建 `设计规范/`:`README`(权威边界 / 三分类 / 流程用法)、`01-设计令牌`(Midnight + Daylight 双主题语义色板 + 终端映射)、`02-布局与交互`(布局 map / 状态机 / 键位 / 与 §8 对账,含 D1·D2)、`03-组件清单`(11 组件 × 状态 × 驱动 `AgentEvent` × 渲染 × 三分类);`原型截图/` 存 Midnight + Daylight 各 3 态(欢迎 / 权限 / 致命错误)。
- 改 `openspec/config.yaml`:加 `context`(产物 = TUI 非 web、视觉权威 = `设计规范/`、原型 HTML 禁作 diff、语义保真)+ `rules`(proposal 必引契约条目;UI task 验收须含契约对照点 + insta 快照 + token 单测;首帧快照人工对截图)。
- 改 `技术方案 §8`:布局补「顶栏(1 行,品牌)」,注明 provider/model 归状态行(D1)。

## 待决

- 状态行 `CallingModel` / `ExecutingTool` 的 glyph + label 为推断,待首帧 insta 快照确认。
- 次级色值未钉:Midnight diff `add` 行底色、各 notice 级别底色、选中态高亮 —— `theme.rs` 落地时对 HTML 逐一固定。
- diff `del` 行原型 demo 未演示,实现需补全(契约 C6 已标注)。
- 256 / 16 色降级表归路线图 1.4,1.0 默认 truecolor。
- 尚无 OpenSpec change:第一个 UI change(TUI 骨架)待 propose。

## 引用

- 契约:`设计规范/README.md`、`01-设计令牌.md`、`02-布局与交互.md`(D1/D2 对账)、`03-组件清单.md`、`原型截图/`
- 设计源:`技术方案/mysteries-agent技术方案.md` §8;`UI设计/Mysteries Agent - Midnight (standalone).html`(参考稿)、`UI设计/Mysteries Agent - Daylight (standalone).html`
- 绑定:`openspec/config.yaml`
- OpenSpec change / spec id:无(`openspec/changes/`、`openspec/specs/` 当前为空)
- session log:本条为本仓库首条 `.ai_history` 记录,无跨 session 引用
