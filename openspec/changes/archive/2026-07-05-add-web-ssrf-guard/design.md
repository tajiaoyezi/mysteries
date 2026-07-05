# Design — add-web-ssrf-guard

## 背景
提交后复审:`web_fetch` CRITICAL SSRF + HIGH 重定向绕过。§5.4 真机实证 `web_fetch` 无阻抓 `127.0.0.1:8765` / `169.254.169.254`。用户选方案 A:push 前补护栏,权限级别另随 plan 模式。

**本设计经两路对抗审查修订**(安全效力 + 可实现性):① 重定向主机名不做 DNS = 一跳 302 平凡绕过 → 改为**每跳同一把门**;② host 取值走 `host_str()`(零依赖)而非 `url.host()` enum(要加 `url` 依赖);③ 补 NAT64 / `0.0.0.0/8` / `240/4`;④ DNS 失败 fail-closed;⑤ 安全关键逻辑抽纯函数上单测。

## 决策

### D1 IP 范围分类抽纯函数(headless 强制 TDD)
`fn is_blocked_ip(ip: &IpAddr) -> bool`:
- **v4**:`is_loopback()`(127/8)、`is_private()`(10/8 · 172.16/12 · 192.168/16)、`is_link_local()`(169.254/16)、`is_multicast()`、`is_broadcast()`;**`octets()[0] == 0`(整 `0.0.0.0/8`,非仅 `is_unspecified()` 的单个 0.0.0.0——审查 LOW)**;**`octets()[0] >= 240`(整 `240/4` reserved,含 broadcast)**;CGNAT `100.64/10` 手判(`oct[0]==100 && (64..=127).contains(&oct[1])`;std `is_shared()` 仍 unstable)。
- **v6**:`is_loopback()`(::1)、`is_unspecified()`(::)、`is_multicast()`;unique-local `fc00::/7`(首字节 `& 0xFE == 0xFC`)、link-local `fe80::/10`(`seg[0] & 0xFFC0 == 0xFE80`)手判(std 相应方法 unstable);**NAT64 `64:ff9b::/96`(前 12 字节 == `[0,0x64,0xff,0x9b,0,…]`——审查 MED,内嵌 v4 经网关达内网)**;**v4-mapped `::ffff:a.b.c.d`:`to_ipv4_mapped()`(1.63 stable)降 v4 再查**。
- **公网**(`1.1.1.1` / `8.8.8.8` / 公网 v6)→ 放行。
- **进一步纵深(LOW,本次不做、记残留)**:6to4 `2002::/16`、Teredo `2001::/32`、v4-compat `::/96`、site-local `fec0::/10`(多为现代栈已弃)——需要更严时补。

### D2 host 取值:`host_str()`(零依赖)+ 只从已 parse 的 Url 取
- **审查冲突裁决**:`url.host()` 返回 `url::Host` enum,要 match 就得把 `url` 加为**直接依赖**、破坏「Cargo.toml 不动」(reqwest 不重导出 `Host`)。而 `reqwest::Url`(=url 2.5)在 `parse` 时**已把八进制/十进制/十六进制 IPv4 归一化**,`host_str()` 返回的就是归一化后的字面量(`http://2130706433/` → `host_str()=="127.0.0.1"`;userinfo `http://evil@127.0.0.1/` → host 正确为 `127.0.0.1`)。→ **走 `host_str()`,零依赖且归一化白拿。**
- **纪律(写进 spec/注释,防绕过复活)**:host **只从已 `parse` 的 `reqwest::Url` 的 `host_str()` 取,绝不从原始 URL 字符串按 `@`/`/` 手工切**——否则归一化丢失、数字编码绕过复活。
- `fn precheck_url(url: &reqwest::Url) -> Result<(), WebError>`(纯,TDD):scheme ∉ {http,https} → Err;`host_str()` 为 `None` → **Err(deny,fail-closed)**;取 `host_str()`(v6 **先剥 `[]`**)`parse::<IpAddr>()` 成功 → `is_blocked_ip` 命中 Err;parse 失败(主机名)→ Ok(留给 DNS 层)。

### D3 每跳同一把门 + 手写重定向循环(取代 literal-only 闭包)
- **审查 HIGH**:`redirect::Policy::custom` 闭包同步、无法 async DNS,只能查字面量 → 攻击者一跳 `302 → 主机名解析到内网`即绕过整个护栏。放弃字面量闭包。
- **改**:client 装 **`redirect::Policy::none()`**(不自动跟);`ReqwestFetcher::fetch` 内**手写重定向循环(深度上限 3)**,每一跳(初始 URL + 每个 `Location`)都过 **`assert_target_allowed(url)`**:`precheck_url`(scheme + 字面量 IP)→ 若为主机名,`(host, port).to_socket_addrs()`(`std::net`,经 `tokio::task::spawn_blocking`)解析 → `check_resolved(&addrs)`。命中/失败即 `WebError`、**不发该跳请求**。
- 相对 `Location` 用 `base_url.join(loc)?` 归一;循环超 3 跳 → `WebError("too many redirects")`;非 3xx 响应 → 出循环、走既有 status/content-type/字节封顶/`html_to_text` 处理。
- `fn check_resolved(addrs: &[IpAddr]) -> Result<(), WebError>`(纯,TDD):任一 `is_blocked_ip` → Err;**空集 → Err(fail-closed,承接 D4)**。

### D4 DNS 解析失败 fail-closed(审查 e)
- `to_socket_addrs()` 返 `Err` 或**空结果** → 编码 `is_error`、不发请求。`check_resolved` 对空集判 Err、解析 `Err` 用 `?` 上抛为 `WebError`。**不得** fall-through 放行(`any()` 对空集=false=fail-open,须显式堵)。

### D5 web_search 不分叉
- `web_search` URL 恒为 `ddg_search_url()` = `https://html.duckduckgo.com/…`(公网固定),过每跳门照常放行(DDG 无内网跳);护栏在 `ReqwestFetcher` 层对二者一致生效、无需分叉。

### D6 权限级别不在本 change
- 复审 finding 3(`ReadOnly` 静默放行、exfil 面)另随 L1 plan 模式引 `Network` 权限级(过 `PermissionDecider` / per-host allowlist)。本 change 保持 `ReadOnly`。

## 残留(记入 spec / 已知,T0 可接受)
- **对抗性 DNS rebinding**(主残留):护栏的 `to_socket_addrs` 解析通过后,`reqwest`/hyper 连接时**独立再解析** → 攻击者以 fast-TTL(TTL=0)在这两次解析间翻转记录、赢毫秒级 race,可致「检查那次见公网、连接那次连内网」。**注:`check_resolved` 检查 `to_socket_addrs` 返回的全部 A 记录、任一内网即拒,故稳定的多记录/round-robin 混合域(如 [公网, 内网])已被拦、非残留**——残留仅限时间维度的主动 rebinding。升级 = 自定义 `reqwest::dns::Resolve`(`ClientBuilder::dns_resolver`,内置非 feature-gated):解析一次、检查后把**同一批已核 IP** 交连接,合并「检查==连接」、一举消 TOCTOU 与重定向层。本次为控 T0 复杂度不做(评审 HIGH,用户拍板本地 T0 接受)。
- **v6 内嵌 v4 纵深**(6to4/Teredo/v4-compat/site-local)本次未拦(见 D1)。
- 二者如实记 spec;升级路径明确。

## 接缝
- `src/tool/web.rs`:纯函数 `is_blocked_ip` / `precheck_url` / `check_resolved`;`ReqwestFetcher::new` 装 `Policy::none()`;`fetch` 改手写重定向循环 + 每跳 `assert_target_allowed`(precheck + spawn_blocking DNS + check_resolved)。

## 风险 / 权衡
- 每跳一次 DNS 解析延迟 —— 可接受(抓取本就网络 IO)。
- 合法但落私网的目标(如真想抓本机 dev server)会被拒 —— 与安全取舍一致;需要时走未来 per-host allowlist(类比 `allowed_commands`)。
- 手写重定向循环比默认跟转多几行,但**每跳同门**是关掉 HIGH 的唯一途径。
