# 设计规范 — Mysteries Agent 原型设计契约

> 本目录把 web 原型(`UI设计/*.html`)蒸馏为**可引用、可验证的 text 设计契约**,作为 TUI 实现的视觉权威。
> 状态:**草案,待审**。来源见末尾。

## 先读这段:为什么需要它

原型是 **web**(bundled HTML),产物是 **ratatui TUI**(终端)。两者不在同一介质——终端没有圆角、阴影、任意字体、补间动画。所以本契约的目标是**语义保真**,不是像素一致。

原型 HTML 本身**不能**直接当 spec:① 是 bundled blob,读不动、diff 不了、引不了行号;② 介质降维(web→TUI);③ 不在 CLAUDE.md 的权威链上。本目录就是给原型造的「可引用落点」。

## 视觉权威次序(嵌入 CLAUDE.md 既有次序)

```
theme.rs / insta 快照(code + tests)  >  本设计契约(text)  >  原型 HTML(人工参考稿)  >  Agent 推断
```

- 行为/逻辑权威仍依 CLAUDE.md:`code / 编译器 / 测试 > spec > Agent 推断`。
- 视觉/版式以本契约为准;**契约与原型冲突时,改契约并记录,不偷偷改实现**。
- 原型 HTML 仅供人工肉眼参考,**禁止作为 diff 或引用依据**。

## 保真标准:每个原型元素三分类

- ✅ **port 直接搬**:配色语义、布局分区、信息层级、diff / 工具卡结构、glyph。
- ⚠️ **adapt 降级适配**:圆角→box-drawing 边框或留白;hover/transition→选中态 `REVERSED`/`BOLD`;truecolor→256/16 色降级;按钮→`[y·允许]` 文本。
- ❌ **drop 丢弃**:阴影、渐变、鼠标 hover、补间动画。**写明理由,不假装能还原。**

## 文件

| 文件 | 内容 | 落点 |
|---|---|---|
| `01-设计令牌.md` | 配色 token(Midnight + Daylight)+ 终端映射 | → `tui/theme.rs` + token 单测 |
| `02-布局与交互.md` | 布局 map、键位、状态机、**与技术方案 §8 对账(含待拍板冲突)** | → `tui/app.rs` `render.rs` |
| `03-组件清单.md` | 组件 × 状态 × 驱动事件 × 终端渲染 × 三分类 | → `tui/widgets/` + insta 快照 |
| `原型截图/` | 关键态渲染图(human gate 对眼用) | 审快照时比对 |

> `原型截图/` 里的 active 态(权限、致命错误)**只有注入交互才看得到**,直接打开 HTML 看不到——故存档为审查锚点。

## 在 OpenSpec 流程里怎么用

1. 任何触及 UI 的 change,proposal 必须**引用本契约的具体条目/组件项**(由 `openspec/config.yaml` 的 `rules` 强制)。
2. UI task 验收 = ① 与契约对照点 ② `insta` 快照(配色另加 `theme.rs` token 单测)。
3. 首次 `cargo insta review` = **唯一人类「对眼」关卡**:拿 ASCII 渲染对 `原型截图/`,approve 后锁定;此后漂移由快照 diff 自动拦截。
4. archive 时决策记录写明:实现了原型哪些区域、哪些 adapt / drop。

## 来源与方法

- **截图**:Chrome headless 渲染 `UI设计/Mysteries Agent - Midnight (standalone).html`;active 态(权限/错误)经注入脚本点击 demo 行触发后截取。
- **token 与组件数据**:从 bundle 源码内嵌的 React demo 脚本提取(精确文本 > 肉眼估算)。
- Midnight 与 Daylight **均有渲染图**(欢迎 / 权限 / 致命错误 3 态),见 `原型截图/`。
