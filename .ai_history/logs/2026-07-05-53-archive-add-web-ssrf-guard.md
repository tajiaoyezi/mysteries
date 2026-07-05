# 2026-07-05 · 53 · archive add-web-ssrf-guard

## 决策
- `web_fetch` SSRF 走 **T0 廉价护栏**(非全防)| 选:每跳同门(scheme + IP + DNS)| 弃:全 `dns_resolver` pin(闭 TOCTOU 但复杂、exotic API)、literal-only(平凡绕过)| 主导:用户拍板 A | 依据:两路对抗审查 + §4.3 真机
- **每跳同门**取代 literal-only redirect | 关掉「一跳 302 → 内网主机名」平凡绕过 | 依据:安全审查 HIGH —— literal-only 使初始 DNS 检查边际安全≈0(攻击者控 URL 时只需多一跳)
- **host 走 `host_str()`(零依赖)非 `url.host()` enum** | 裁决两审查冲突:`host()` 需加 `url` 直接依赖、破坏零 Cargo;`host_str()` 已归一化(`2130706433`→`127.0.0.1`)| 依据:compiler(`Cargo.toml` 空)+ 真机(§4.3 #3 编码 IP 被拦)
- **fail-closed**:DNS 解析失败 + `check_resolved` 空集均拒 | 依据:审查(`any()` 对空集 = fail-open)
- 权限级别 finding 3(`ReadOnly` 静默放行、exfil 面)**不在本 change** | → 随 L1 plan 引 `Network` 级
- **对抗性 DNS rebinding 残留**(评审 HIGH)本地 T0 接受 | 稳定多记录已被 `check_resolved` 全查拦、非残留;残留仅时间维度主动 rebinding;升级 = 自定义 `dns::Resolve` pin | 主导:用户 A

## 变更
- `src/tool/web.rs`:纯函数 `is_blocked_ip` / `precheck_url` / `check_resolved`(全范围含 NAT64 `64:ff9b::/96` / `0/8` / `240/4` / v4-mapped)+ `ReqwestFetcher` 改 `Policy::none` + 手写重定向循环、每跳 `assert_target_allowed`(precheck + `spawn_blocking` DNS + `check_resolved`)
- spec `builtin-tools`:MODIFY `web_fetch`(SSRF 护栏段 + 3 scenario);工具数不变、Purpose 未改
- 无新依赖(`std::net` + 现成 `reqwest`/`tokio`);8 新纯函数测试;`cargo test --lib` 676/0/2、clippy 净
- 真机 §4.3:`127.0.0.1`(活靶)/ `169.254.169.254` / 十进制编码 全在**连接前** blocked、公网无回归

## 待决
- 升级 = 自定义 `dns::Resolve` pin 解析(闭对抗 rebinding + 重定向层),web 后端换代或上云时做
- v6 内嵌 v4 纵深(6to4/Teredo 等)未拦
- README / 技术方案「7 个内置工具」→ 9 未同步(change 外收尾)

## 引用
- OpenSpec change:`add-web-ssrf-guard`(archived `2026-07-05-add-web-ssrf-guard`)
- 触发:提交后安全复审(`dde7a73` web 工具)finding 1/2 SSRF;finding 3 权限级别延后 plan
- 跨:add-web-tools(log 52,web 工具本体)
