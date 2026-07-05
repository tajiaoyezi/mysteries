# add-web-ssrf-guard

## Why

提交后自动安全复审(针对**未 push** 的 `dde7a73`)判 `web_fetch` 存在 **CRITICAL SSRF**:模型 / prompt-injection 可诱导抓 `http://127.0.0.1:*`(本机服务)、`http://169.254.169.254/…`(云元数据);`reqwest` 默认还跟 302 重定向 → 公网 URL 可放大到内网(HIGH)。§5.4 真机已实证此行为。

v1 曾把 SSRF 列为「已知局限、只观察不修」;但复审判 CRITICAL、且该 commit **尚未 push**,趁早补一层**廉价护栏**关掉主链最划算(push 前叠一个 commit,无 force)。

本 change **只做 SSRF 地址护栏**。复审 finding 3(web 工具为 `ReadOnly` = 各模式静默自动放行、网络出站有 exfil 面)**另在 L1 plan 模式随 `PermissionMode` 改造引入 `Network` 级处理**,不在此。

## What Changes

1. **IP 范围分类纯函数** `is_blocked_ip(&IpAddr)`:loopback / 私网(RFC1918)/ link-local(含 `169.254.169.254`)/ unique-local / CGNAT(`100.64/10`)/ **NAT64(`64:ff9b::/96`)** / multicast / **`0.0.0.0/8`** / **`240/4`**,v4 + v6(含 v4-mapped)。headless 强制 TDD。
2. **每跳同一把门**:对**初始 URL 及每个重定向目标**——拒绝 scheme 非 `http`/`https`;host 从**已解析 URL** 取(数字编码 IP 已归一化),IP 字面量直接查 `is_blocked_ip`、主机名 DNS 解析后逐 IP 查(**解析失败/空 → fail-closed 拒**);命中即拒(`is_error`、**不发该请求**、不 panic)。抽纯函数 `precheck_url` / `check_resolved`。
3. **重定向不自动跟随**(`redirect::Policy::none`):`web_fetch` **手写重定向循环**(深度上限 3),每跳过第 2 条同门——关掉「一跳 302 到内网主机名」的平凡绕过(对抗审查 HIGH)。
4. **已知残留(如实记入 spec)**:对抗性 DNS rebinding(护栏与连接层独立再解析;稳定多记录混合域已被 `check_resolved` 全查拦下、非残留);v6 内嵌 v4 纵深(6to4/Teredo 等)未拦。升级 = 自定义 `dns::Resolve` pin 解析。

## Impact

- 修改 capability:`builtin-tools`(**MODIFY** `web_fetch` requirement:把「v1 SHALL NOT 防护 SSRF」改为「SHALL 拒内网/loopback/link-local 目标 + 重定向重检」;title 不变,纯改 body + 加 2 scenario)。无新 requirement。
- Affected code:`src/tool/web.rs`(`is_blocked_ip` + `precheck_url` 纯函数;`ReqwestFetcher::new` 装 redirect policy;`fetch` 加前置 + DNS 检查)。`web_search` 走同一 `ReqwestFetcher`,其 URL 恒为公网 `html.duckduckgo.com`、照常放行,不受影响、不分叉。
- **无新依赖**:`is_blocked_ip` 用 `std::net`;DNS 用 `std::net::ToSocketAddrs` + `tokio::task::spawn_blocking`(tokio `rt` 已在);redirect 用现成 `reqwest`。`Cargo.toml` 不动。
- 回退:护栏纯增,拒绝路径编码 `is_error`;不影响公网抓取。
- 权限级别(finding 3)不在本 change,随 plan 模式做。
