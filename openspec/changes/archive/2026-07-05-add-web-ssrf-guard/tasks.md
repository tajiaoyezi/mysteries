# Tasks — add-web-ssrf-guard

红灯纪律:纯函数先测后码、红灯独立成步(新签名允许桩:返错误值令断言红,非 `todo!()`/panic)。执行 agent MUST NOT:git 写、勾第 4 节真机、全仓 `cargo fmt`(只碰 `web.rs`)、kill 进程、**加新依赖**(`std::net` + 现成 `reqwest`/`tokio`,`Cargo.toml` 不动;**尤其不得为 `url::Host` 加 `url` 直接依赖**——见 2.1)。

## 1. IP 范围分类(纯,强制 TDD)
- [x] 1.1 红→绿:`is_blocked_ip(&IpAddr) -> bool`。测试 **blocked**:`127.0.0.1`/`::1`/`10.0.0.1`/`172.16.0.1`/`172.31.255.255`/`192.168.1.1`/`169.254.169.254`/`100.64.0.1`(CGNAT)/`224.0.0.1`(multicast)/`0.0.0.0`/**`0.1.2.3`(整 `0/8`)**/**`240.0.0.1` 与 `255.255.255.255`(整 `240/4`)**/`fe80::1`/`fc00::1`/`fd00::1`/**`64:ff9b::a9fe:a9fe`(NAT64→169.254.169.254)**/`::ffff:127.0.0.1`(v4-mapped);**allowed**:`1.1.1.1`/`8.8.8.8`/`172.15.0.1`/`172.32.0.1`/公网 v6 `2606:4700::1`。

## 2. URL 前置检查 + 已解析裁决(纯,强制 TDD)
- [x] 2.1 红→绿:`precheck_url(&reqwest::Url) -> Result<(), WebError>`。**host 只从 `url.host_str()` 取(v6 先剥 `[]`)+ `parse::<IpAddr>()`,禁用 `url::Host` enum(会逼加 `url` 依赖)/ 禁从原始字符串切**。测试:`ftp://x`/`file:///x` → Err;`http://127.0.0.1`/`https://169.254.169.254`/`http://[::1]/`(**验剥括号**)→ Err;**`http://2130706433/`、`http://0177.0.0.1/`(八/十进制编码 → parse 时归一化为 127.0.0.1)→ Err**(锁死「靠 parse 归一化」这条隐依赖);`http://evil@127.0.0.1/` → Err(host 正确取 127.0.0.1);`https://example.com`(主机名)/`https://1.1.1.1` → Ok。
- [x] 2.2 红→绿:`check_resolved(addrs: &[IpAddr]) -> Result<(), WebError>`。测试:含任一内网 IP → Err;全公网 → Ok;**空集 `&[]` → Err(fail-closed)**。

## 3. 接入 ReqwestFetcher(网络壳,真机验、无单测)
- [x] 3.1 `ReqwestFetcher::new`:client builder 装 **`redirect::Policy::none()`**(不自动跟转)。`fetch` 改**手写重定向循环(深度上限 3)**:每跳对当前 URL 过 `assert_target_allowed`——`precheck_url`?;若主机名,`spawn_blocking` + `(host_str_去括号, port_or_known_default) .to_socket_addrs()` 解析 → **解析 `Err`/空 → `WebError`(fail-closed)** → `check_resolved(&addrs)`?;通过再 GET(带 UA);响应 3xx → 取 `LOCATION`(缺 → Err)、`base.join(loc)?` 续循环;非 3xx → 出循环走既有 status/content-type/字节封顶/`html_to_text`;超 3 跳 → `WebError("too many redirects")`。（真网、不写单测。**落地时顺手 smoke:`host_str()` 的 v6 是否带 `[]`、`(&str,u16): ToSocketAddrs`**。)

## 4. 门禁 + 真机(真机主 agent/用户;执行 agent 勿勾)
- [x] 4.1 `cargo test --lib` 全绿;`cargo clippy --all-targets -- -D warnings` 零警告;`cargo build`(exe 占用报 os error 5 即报告、别 kill、可隔离 `CARGO_TARGET_DIR`)
- [x] 4.2 `openspec validate add-web-ssrf-guard --strict`;`git diff Cargo.toml` **确认无新依赖**(尤其无 `url`)
- [x] 4.3 真机:重跑 §5.4 —— `web_fetch http://127.0.0.1:8765/` 现应 **`is_error`(blocked)、不再读到 marker**;`http://169.254.169.254/…` 亦 blocked;公网文档页照常抓到正文、无回归;**重定向层**:找/搭一个公网 URL `302 → http://127.0.0.1:...`(字面量)与 `302 → 解析到内网的主机名`,两者都应被拦(验每跳同门)。
