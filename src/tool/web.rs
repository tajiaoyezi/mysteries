use crate::tool::fs::truncate_utf8;
use crate::tool::{
    NetworkPermissionPreview, NetworkPermissionScope, PermissionLevel, Tool, ToolContext,
    ToolOutcome,
};
use async_trait::async_trait;
use futures_util::StreamExt;
use regex::Regex;
use reqwest::header::{HeaderValue, LOCATION, USER_AGENT};
use serde_json::{json, Value};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, ToSocketAddrs};
use std::time::Duration;
use thiserror::Error;

pub const MAX_SEARCH_RESULTS: usize = 8;
const WEB_TIMEOUT: Duration = Duration::from_secs(15);
const WEB_MAX_BYTES: usize = 2 * 1024 * 1024;
const MAX_REDIRECTS: u32 = 3;
const BROWSER_USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:128.0) Gecko/20100101 Firefox/128.0";

pub fn redirect_allowed(redirects_followed: u32) -> bool {
    redirects_followed < MAX_REDIRECTS
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchResult {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

pub fn ddg_search_url(query: &str) -> String {
    reqwest::Url::parse_with_params("https://html.duckduckgo.com/html/", &[("q", query)])
        .expect("valid ddg search base url")
        .to_string()
}

pub fn decode_uddg(href: &str) -> Option<String> {
    // HTML 属性里 & 常写作 &amp;，须在 URL 解析前还原（非 percent 解码）
    let normalized = href.replace("&amp;", "&");
    let url_str = if normalized.starts_with("//") {
        format!("https:{normalized}")
    } else {
        normalized
    };
    let url = reqwest::Url::parse(&url_str).ok()?;
    for (key, value) in url.query_pairs() {
        if key == "uddg" {
            return Some(value.into_owned());
        }
    }
    None
}

pub fn html_to_text(html: &str) -> String {
    let script_re = Regex::new(r"(?si)<script[^>]*>.*?</script>").expect("script strip regex");
    let style_re = Regex::new(r"(?si)<style[^>]*>.*?</style>").expect("style strip regex");
    let tag_re = Regex::new(r"<[^>]*>").expect("tag strip regex");
    let hex_entity_re = Regex::new(r"&#x([0-9a-fA-F]+);").expect("hex entity regex");
    let dec_entity_re = Regex::new(r"&#([0-9]+);").expect("dec entity regex");
    let whitespace_re = Regex::new(r"\s+").expect("whitespace regex");

    let mut text = script_re.replace_all(html, "").into_owned();
    text = style_re.replace_all(&text, "").into_owned();
    text = tag_re.replace_all(&text, "").into_owned();

    text = hex_entity_re
        .replace_all(&text, |caps: &regex::Captures| {
            u32::from_str_radix(&caps[1], 16)
                .ok()
                .and_then(char::from_u32)
                .map(|c| c.to_string())
                .unwrap_or_else(|| caps[0].to_string())
        })
        .into_owned();
    text = dec_entity_re
        .replace_all(&text, |caps: &regex::Captures| {
            caps[1]
                .parse::<u32>()
                .ok()
                .and_then(char::from_u32)
                .map(|c| c.to_string())
                .unwrap_or_else(|| caps[0].to_string())
        })
        .into_owned();

    text = text.replace("&lt;", "<");
    text = text.replace("&gt;", ">");
    text = text.replace("&quot;", "\"");
    text = text.replace("&nbsp;", " ");
    text = text.replace("&amp;", "&");

    whitespace_re.replace_all(text.trim(), " ").into_owned()
}

pub fn parse_ddg_results(html: &str) -> Vec<SearchResult> {
    let title_re = Regex::new(r#"(?s)class="result__a"[^>]*href="([^"]+)"[^>]*>(.*?)</a>"#)
        .expect("result__a regex");
    let snippet_re =
        Regex::new(r#"(?s)class="result__snippet"[^>]*>(.*?)</a>"#).expect("result__snippet regex");

    let titles: Vec<(String, String)> = title_re
        .captures_iter(html)
        .map(|caps| {
            let href = caps[1].to_string();
            let title = html_to_text(&caps[2]);
            (href, title)
        })
        .collect();

    let snippets: Vec<String> = snippet_re
        .captures_iter(html)
        .map(|caps| html_to_text(&caps[1]))
        .collect();

    titles
        .into_iter()
        .zip(snippets)
        .map(|((href, title), snippet)| SearchResult {
            url: decode_uddg(&href).unwrap_or(href),
            title,
            snippet,
        })
        .take(MAX_SEARCH_RESULTS)
        .collect()
}

/// loopback / 私网 / link-local / CGNAT / NAT64 / multicast / 0/8 / 240/4 等内网或保留范围。
pub fn is_blocked_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_blocked_ipv4(v4),
        IpAddr::V6(v6) => {
            if let Some(mapped) = v6.to_ipv4_mapped() {
                return is_blocked_ipv4(&mapped);
            }
            is_blocked_ipv6(v6)
        }
    }
}

fn is_blocked_ipv4(v4: &Ipv4Addr) -> bool {
    let oct = v4.octets();
    v4.is_loopback()
        || v4.is_private()
        || v4.is_link_local()
        || v4.is_multicast()
        || v4.is_broadcast()
        || oct[0] == 0
        || oct[0] >= 240
        || (oct[0] == 100 && (64..=127).contains(&oct[1]))
}

fn is_blocked_ipv6(v6: &Ipv6Addr) -> bool {
    if v6.is_loopback() || v6.is_unspecified() || v6.is_multicast() {
        return true;
    }
    let octets = v6.octets();
    let segs = v6.segments();
    // fc00::/7 — 首字节 & 0xFE == 0xFC
    if (octets[0] & 0xFE) == 0xFC {
        return true;
    }
    // fe80::/10
    if (segs[0] & 0xFFC0) == 0xFE80 {
        return true;
    }
    // NAT64 64:ff9b::/96 — 前 96 位 == 64:ff9b:0:0:0:0
    if segs[0] == 0x0064 && segs[1] == 0xff9b && segs[2..=5].iter().all(|&s| s == 0) {
        return true;
    }
    false
}

/// host 只从已 parse 的 `reqwest::Url::host_str()` 取(v6 先剥 `[]`),禁止从原始 URL 字符串切 host。
pub fn precheck_url(url: &reqwest::Url) -> Result<(), WebError> {
    match url.scheme() {
        "http" | "https" => {}
        scheme => {
            return Err(WebError::new(format!("blocked scheme: {scheme}")));
        }
    }
    let Some(host) = url.host_str() else {
        return Err(WebError::new("missing host"));
    };
    if host.is_empty() {
        return Err(WebError::new("missing host"));
    }
    let host = strip_ipv6_brackets(host);
    if let Ok(ip) = host.parse::<IpAddr>() {
        if is_blocked_ip(&ip) {
            return Err(WebError::new(format!("blocked IP: {ip}")));
        }
    }
    Ok(())
}

fn strip_ipv6_brackets(host: &str) -> &str {
    host.strip_prefix('[')
        .and_then(|inner| inner.strip_suffix(']'))
        .unwrap_or(host)
}

/// 已解析地址集裁决:任一内网 IP 拒;空集 fail-closed。
pub fn check_resolved(addrs: &[IpAddr]) -> Result<(), WebError> {
    if addrs.is_empty() {
        return Err(WebError::new("no resolved addresses"));
    }
    for addr in addrs {
        if is_blocked_ip(addr) {
            return Err(WebError::new(format!("blocked IP: {addr}")));
        }
    }
    Ok(())
}

async fn assert_target_allowed(url: &reqwest::Url) -> Result<(), WebError> {
    precheck_url(url)?;

    let Some(host) = url.host_str() else {
        return Err(WebError::new("missing host"));
    };
    let host = strip_ipv6_brackets(host).to_string();
    let port = url.port_or_known_default().unwrap_or(0);

    let addrs = tokio::task::spawn_blocking(move || (host.as_str(), port).to_socket_addrs())
        .await
        .map_err(|err| WebError::new(format!("dns task failed: {err}")))?
        .map_err(|err| WebError::new(format!("dns resolution failed: {err}")))?;

    let ips: Vec<IpAddr> = addrs.map(|addr| addr.ip()).collect();
    check_resolved(&ips)
}

fn format_search_results(results: &[SearchResult]) -> String {
    results
        .iter()
        .enumerate()
        .map(|(index, result)| {
            format!(
                "{}. {} — {}\n   {}",
                index + 1,
                result.title,
                result.url,
                result.snippet
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn is_readable_content_type(content_type: Option<&str>) -> bool {
    let Some(raw) = content_type else {
        return true;
    };
    let mime = raw
        .split(';')
        .next()
        .unwrap_or(raw)
        .trim()
        .to_ascii_lowercase();
    if mime.starts_with("text/") {
        return true;
    }
    !(mime.starts_with("image/") || mime == "application/octet-stream" || mime == "application/pdf")
}

async fn read_body_capped(
    response: reqwest::Response,
    max_bytes: usize,
) -> Result<Vec<u8>, WebError> {
    let mut body = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|err| WebError::new(format!("read body failed: {err}")))?;
        let remaining = max_bytes.saturating_sub(body.len());
        if remaining == 0 {
            break;
        }
        if chunk.len() <= remaining {
            body.extend_from_slice(&chunk);
        } else {
            body.extend_from_slice(&chunk[..remaining]);
            break;
        }
    }
    Ok(body)
}

fn bytes_to_text(body: Vec<u8>) -> String {
    match String::from_utf8(body) {
        Ok(text) => text,
        Err(err) => {
            let mut bytes = err.into_bytes();
            while !bytes.is_empty() && std::str::from_utf8(&bytes).is_err() {
                bytes.pop();
            }
            String::from_utf8_lossy(&bytes).into_owned()
        }
    }
}

fn string_arg<'a>(args: &'a Value, field: &str) -> Option<&'a str> {
    args.get(field).and_then(Value::as_str)
}

fn web_fetch_request(args: &Value) -> Result<String, WebError> {
    let url = string_arg(args, "url").ok_or_else(|| WebError::new("missing or invalid url"))?;
    reqwest::Url::parse(url)
        .map(|url| url.to_string())
        .map_err(|err| WebError::new(format!("invalid url: {err}")))
}

fn web_search_request(args: &Value) -> Result<String, WebError> {
    let query =
        string_arg(args, "query").ok_or_else(|| WebError::new("missing or invalid query"))?;
    Ok(ddg_search_url(query))
}

fn authorizable_preview(
    args: &Value,
    target: Result<String, WebError>,
    scope: NetworkPermissionScope,
) -> NetworkPermissionPreview {
    match target {
        Ok(canonical_initial_target) => NetworkPermissionPreview {
            authorizable: true,
            full_args: args.clone(),
            canonical_initial_target: Some(canonical_initial_target),
            scope: Some(scope),
            denial_reason: None,
        },
        Err(err) => NetworkPermissionPreview {
            authorizable: false,
            full_args: args.clone(),
            canonical_initial_target: None,
            scope: None,
            denial_reason: Some(err.to_string()),
        },
    }
}

fn error_outcome(content: impl Into<String>) -> ToolOutcome {
    ToolOutcome {
        content: content.into(),
        is_error: true,
        truncated: false,
        exit: None,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{message}")]
pub struct WebError {
    message: String,
}

impl WebError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

#[async_trait]
pub trait WebFetcher: Send + Sync {
    async fn fetch(&self, url: &str) -> Result<String, WebError>;

    fn permission_scope(&self) -> NetworkPermissionScope;
}

pub struct MockFetcher {
    response: Result<String, WebError>,
}

impl MockFetcher {
    pub fn ok(html: impl Into<String>) -> Self {
        Self {
            response: Ok(html.into()),
        }
    }

    pub fn err(message: impl Into<String>) -> Self {
        Self {
            response: Err(WebError::new(message)),
        }
    }
}

#[async_trait]
impl WebFetcher for MockFetcher {
    async fn fetch(&self, _url: &str) -> Result<String, WebError> {
        match &self.response {
            Ok(html) => Ok(html.clone()),
            Err(err) => Err(WebError::new(&err.message)),
        }
    }

    fn permission_scope(&self) -> NetworkPermissionScope {
        NetworkPermissionScope {
            max_redirects: 0,
            may_cross_origin: false,
            ssrf_each_hop: false,
        }
    }
}

pub struct ReqwestFetcher {
    client: reqwest::Client,
}

impl ReqwestFetcher {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(WEB_TIMEOUT)
                .build()
                .expect("reqwest client"),
        }
    }
}

impl Default for ReqwestFetcher {
    fn default() -> Self {
        Self::new()
    }
}

async fn process_http_response(response: reqwest::Response) -> Result<String, WebError> {
    if !response.status().is_success() {
        return Err(WebError::new(format!("HTTP {}", response.status())));
    }

    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok());
    if !is_readable_content_type(content_type) {
        return Err(WebError::new(format!(
            "unsupported content-type: {}",
            content_type.unwrap_or("unknown")
        )));
    }

    let body = read_body_capped(response, WEB_MAX_BYTES).await?;
    Ok(bytes_to_text(body))
}

#[async_trait]
impl WebFetcher for ReqwestFetcher {
    async fn fetch(&self, url: &str) -> Result<String, WebError> {
        let mut url =
            reqwest::Url::parse(url).map_err(|err| WebError::new(format!("invalid url: {err}")))?;
        let mut redirects = 0u32;

        let response = loop {
            assert_target_allowed(&url).await?;

            let response = self
                .client
                .get(url.clone())
                .header(USER_AGENT, HeaderValue::from_static(BROWSER_USER_AGENT))
                .send()
                .await
                .map_err(|err| WebError::new(format!("request failed: {err}")))?;

            if response.status().is_redirection() {
                if !redirect_allowed(redirects) {
                    return Err(WebError::new("too many redirects"));
                }
                let location = response
                    .headers()
                    .get(LOCATION)
                    .ok_or_else(|| WebError::new("redirect missing Location header"))?
                    .to_str()
                    .map_err(|err| WebError::new(format!("invalid Location header: {err}")))?;
                url = url
                    .join(location)
                    .map_err(|err| WebError::new(format!("invalid redirect url: {err}")))?;
                redirects += 1;
                continue;
            }

            break response;
        };

        process_http_response(response).await
    }

    fn permission_scope(&self) -> NetworkPermissionScope {
        NetworkPermissionScope {
            max_redirects: MAX_REDIRECTS,
            may_cross_origin: true,
            ssrf_each_hop: true,
        }
    }
}

pub struct WebFetchTool {
    fetcher: Box<dyn WebFetcher>,
}

pub struct WebSearchTool {
    fetcher: Box<dyn WebFetcher>,
}

impl WebFetchTool {
    pub fn new(fetcher: Box<dyn WebFetcher>) -> Self {
        Self { fetcher }
    }
}

impl WebSearchTool {
    pub fn new(fetcher: Box<dyn WebFetcher>) -> Self {
        Self { fetcher }
    }
}

#[async_trait]
impl Tool for WebFetchTool {
    fn name(&self) -> &str {
        "web_fetch"
    }

    fn description(&self) -> &str {
        "抓 URL 返可读文本,读文档/网页"
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "url": { "type": "string" }
            },
            "required": ["url"]
        })
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::Network
    }

    fn network_permission_preview(&self, args: &Value) -> NetworkPermissionPreview {
        authorizable_preview(
            args,
            web_fetch_request(args),
            self.fetcher.permission_scope(),
        )
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> ToolOutcome {
        let url = match web_fetch_request(&args) {
            Ok(url) => url,
            Err(err) => return error_outcome(err.to_string()),
        };

        let html = match self.fetcher.fetch(&url).await {
            Ok(html) => html,
            Err(err) => return error_outcome(err.to_string()),
        };

        let text = html_to_text(&html);
        let (content, truncated) = truncate_utf8(text, ctx.max_output_bytes);
        ToolOutcome {
            content,
            is_error: false,
            truncated,
            exit: None,
        }
    }
}

#[async_trait]
impl Tool for WebSearchTool {
    fn name(&self) -> &str {
        "web_search"
    }

    fn description(&self) -> &str {
        "联网搜索,返回标题/摘要/URL;拿 URL 用 web_fetch 深读"
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": { "type": "string" }
            },
            "required": ["query"]
        })
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::Network
    }

    fn network_permission_preview(&self, args: &Value) -> NetworkPermissionPreview {
        authorizable_preview(
            args,
            web_search_request(args),
            self.fetcher.permission_scope(),
        )
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> ToolOutcome {
        let search_url = match web_search_request(&args) {
            Ok(search_url) => search_url,
            Err(err) => return error_outcome(err.to_string()),
        };

        let html = match self.fetcher.fetch(&search_url).await {
            Ok(html) => html,
            Err(err) => return error_outcome(err.to_string()),
        };

        let results = parse_ddg_results(&html);
        if results.is_empty() {
            return error_outcome("无结果/疑似被限流");
        }

        let text = format_search_results(&results);
        let (content, truncated) = truncate_utf8(text, ctx.max_output_bytes);
        ToolOutcome {
            content,
            is_error: false,
            truncated,
            exit: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        bytes_to_text, check_resolved, ddg_search_url, decode_uddg, html_to_text, is_blocked_ip,
        is_readable_content_type, parse_ddg_results, precheck_url, process_http_response,
        MockFetcher, ReqwestFetcher, WebFetchTool, WebFetcher, WebSearchTool, MAX_SEARCH_RESULTS,
        WEB_MAX_BYTES,
    };
    use crate::tool::{NetworkPermissionScope, Tool, ToolContext};
    use serde_json::json;
    use std::net::IpAddr;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    // 真抓 DDG 片段(2026-07-05, query=rust ownership);含 &amp;rut= / &#x27; / <b> / result__a·result__snippet
    const DDG_FIXTURE: &str = r#"
                <div class="result results_links results_links_deep web-result ">
                  <div class="links_main links_deep result__body">
                      <h2 class="result__title">
                        <a rel="nofollow" class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fdoc.rust%2Dlang.org%2Fbook%2Fch04%2D01%2Dwhat%2Dis%2Downership.html&amp;rut=b560a441023d55ad730a26ec876c42ee7096abe323f838f08901ce78e8b4c68f">What is Ownership? - The Rust Programming Language</a>
                      </h2>
                        <a class="result__snippet" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fdoc.rust%2Dlang.org%2Fbook%2Fch04%2D01%2Dwhat%2Dis%2Downership.html&amp;rut=b560a441023d55ad730a26ec876c42ee7096abe323f838f08901ce78e8b4c68f">Learn how <b>Rust</b> manages memory through a system of <b>ownership</b> with a set of rules that the compiler checks. See how <b>ownership</b> affects the stack, the heap, and strings in <b>Rust</b>.</a>
                  </div>
                </div>
                <div class="result results_links results_links_deep web-result ">
                  <div class="links_main links_deep result__body">
                      <h2 class="result__title">
                        <a rel="nofollow" class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fdoc.rust%2Dlang.org%2Fbook%2Fch04%2D00%2Dunderstanding%2Downership.html&amp;rut=4a446c0cf310642290f10978495ceaaa71147590961f5d86ae9fa0b7450508f4">Understanding Ownership - The Rust Programming Language</a>
                      </h2>
                        <a class="result__snippet" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fdoc.rust%2Dlang.org%2Fbook%2Fch04%2D00%2Dunderstanding%2Downership.html&amp;rut=4a446c0cf310642290f10978495ceaaa71147590961f5d86ae9fa0b7450508f4">Understanding <b>Ownership</b> <b>Ownership</b> is <b>Rust&#x27;s</b> most unique feature and has deep implications for the rest of the language. It enables <b>Rust</b> to make memory safety guarantees without needing a garbage collector, so it&#x27;s important to understand how <b>ownership</b> works. In this chapter, we&#x27;ll talk about <b>ownership</b> as well as several related features: borrowing, slices, and how <b>Rust</b> lays data out in ...</a>
                  </div>
                </div>"#;

    fn ctx(max_output_bytes: usize) -> ToolContext {
        ToolContext {
            cwd: PathBuf::from("."),
            max_output_bytes,
        }
    }

    #[derive(Clone)]
    struct CaptureFetcher {
        urls: Arc<Mutex<Vec<String>>>,
        scope: NetworkPermissionScope,
    }

    impl CaptureFetcher {
        fn new(scope: NetworkPermissionScope) -> Self {
            Self {
                urls: Arc::new(Mutex::new(Vec::new())),
                scope,
            }
        }

        fn urls(&self) -> Vec<String> {
            self.urls.lock().unwrap().clone()
        }
    }

    #[async_trait::async_trait]
    impl WebFetcher for CaptureFetcher {
        async fn fetch(&self, url: &str) -> Result<String, super::WebError> {
            self.urls.lock().unwrap().push(url.to_string());
            Ok("<p>captured</p>".to_string())
        }

        fn permission_scope(&self) -> NetworkPermissionScope {
            self.scope.clone()
        }
    }

    // --- 1.1 ddg_search_url / decode_uddg ---

    #[test]
    fn ddg_search_url_encodes_spaces_as_plus() {
        let url = ddg_search_url("rust ownership");
        assert!(url.starts_with("https://html.duckduckgo.com/html/?"));
        assert!(url.contains("q=rust+ownership"));
    }

    #[test]
    fn redirect_budget_allows_only_the_first_three_redirects() {
        assert!(super::redirect_allowed(0));
        assert!(super::redirect_allowed(1));
        assert!(super::redirect_allowed(2));
        assert!(!super::redirect_allowed(3));
    }

    #[test]
    fn ddg_search_url_encodes_chinese_and_ampersand() {
        let url = ddg_search_url("Rust 所有权 & 借用");
        assert!(url.contains("q="));
        assert!(!url.contains(' '));
        assert!(url.contains("%26"));
        let parsed = reqwest::Url::parse(&url).unwrap();
        let q: String = parsed
            .query_pairs()
            .find(|(k, _)| k == "q")
            .map(|(_, v)| v.into_owned())
            .unwrap();
        assert_eq!(q, "Rust 所有权 & 借用");
    }

    #[test]
    fn decode_uddg_extracts_real_url_and_does_not_swallow_rut_tail() {
        let href = "//duckduckgo.com/l/?uddg=https%3A%2F%2Fdoc.rust-lang.org%2Fx.html&rut=b560a441023d55ad730a26ec876c42ee7096abe323f838f08901ce78e8b4c68f";
        assert_eq!(
            decode_uddg(href),
            Some("https://doc.rust-lang.org/x.html".to_string())
        );
    }

    #[test]
    fn decode_uddg_returns_none_for_ad_or_missing_uddg() {
        assert_eq!(decode_uddg("//duckduckgo.com/y.js?d=123"), None);
        assert_eq!(decode_uddg("//example.com/page"), None);
    }

    // --- 1.2 html_to_text ---

    #[test]
    fn html_to_text_strips_script_style_tags_and_decodes_entities() {
        let html = r#"<script>alert("x")</script><style>.x{color:red}</style><p>Hello <b>World</b></p><span>Rust&#x27;s&nbsp;guide</span>  extra   spaces"#;
        let text = html_to_text(html);
        assert!(!text.contains("alert"));
        assert!(!text.contains("color:red"));
        assert!(!text.contains('<'));
        assert!(!text.contains('>'));
        assert!(text.contains("Hello World"));
        assert!(text.contains("Rust's guide"));
        assert!(!text.contains('\u{00a0}'));
        assert_eq!(text.split_whitespace().count(), 5);
    }

    #[test]
    fn html_to_text_handles_malformed_and_empty_without_panic() {
        assert_eq!(html_to_text(""), "");
        assert_eq!(html_to_text("<p>unclosed"), "unclosed");
        assert_eq!(html_to_text("plain text"), "plain text");
    }

    #[test]
    fn html_to_text_decodes_hex_apostrophe_from_ddg() {
        assert_eq!(html_to_text("Rust&#x27;s ownership"), "Rust's ownership");
    }

    // --- 1.3 parse_ddg_results ---

    #[test]
    fn parse_ddg_results_extracts_title_url_snippet_from_real_fixture() {
        let results = parse_ddg_results(DDG_FIXTURE);
        assert_eq!(results.len(), 2);
        assert_eq!(
            results[0].title,
            "What is Ownership? - The Rust Programming Language"
        );
        assert_eq!(
            results[0].url,
            "https://doc.rust-lang.org/book/ch04-01-what-is-ownership.html"
        );
        assert!(results[0].snippet.contains("Rust"));
        assert!(results[0].snippet.contains("ownership"));
        assert!(!results[0].url.contains("duckduckgo.com/l/"));
        assert!(!results[0].url.contains("rut="));

        assert_eq!(
            results[1].title,
            "Understanding Ownership - The Rust Programming Language"
        );
        assert_eq!(
            results[1].url,
            "https://doc.rust-lang.org/book/ch04-00-understanding-ownership.html"
        );
        assert!(results[1].snippet.contains("Rust's"));
        assert!(!results[1].snippet.contains("&#x27;"));
    }

    #[test]
    fn parse_ddg_results_returns_empty_for_no_results() {
        assert!(parse_ddg_results("<html><body>no results</body></html>").is_empty());
    }

    #[test]
    fn parse_ddg_results_caps_at_max_search_results() {
        let mut html = String::new();
        for i in 0..10 {
            html.push_str(&format!(
                r#"<a class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.com%2F{i}&amp;rut=abc">Title {i}</a><a class="result__snippet">Snippet {i}</a>"#
            ));
        }
        let results = parse_ddg_results(&html);
        assert_eq!(results.len(), MAX_SEARCH_RESULTS);
    }

    // --- SSRF 护栏纯函数(§1 + §2) ---

    #[test]
    fn is_blocked_ip_blocks_private_and_reserved_ranges() {
        let blocked = [
            "127.0.0.1",
            "::1",
            "10.0.0.1",
            "172.16.0.1",
            "172.31.255.255",
            "192.168.1.1",
            "169.254.169.254",
            "100.64.0.1",
            "224.0.0.1",
            "0.0.0.0",
            "0.1.2.3",
            "240.0.0.1",
            "255.255.255.255",
            "fe80::1",
            "fc00::1",
            "fd00::1",
            "64:ff9b::a9fe:a9fe",
            "::ffff:127.0.0.1",
        ];
        for addr in blocked {
            assert!(
                is_blocked_ip(&addr.parse::<IpAddr>().unwrap()),
                "expected blocked: {addr}"
            );
        }
    }

    #[test]
    fn is_blocked_ip_allows_public_addresses() {
        let allowed = [
            "1.1.1.1",
            "8.8.8.8",
            "172.15.0.1",
            "172.32.0.1",
            "2606:4700::1",
        ];
        for addr in allowed {
            assert!(
                !is_blocked_ip(&addr.parse::<IpAddr>().unwrap()),
                "expected allowed: {addr}"
            );
        }
    }

    #[test]
    fn precheck_url_rejects_non_http_schemes_and_blocked_ips() {
        for url in ["ftp://x", "file:///x"] {
            assert!(precheck_url(&reqwest::Url::parse(url).unwrap()).is_err());
        }
        for url in [
            "http://127.0.0.1",
            "https://169.254.169.254",
            "http://[::1]/",
            "http://2130706433/",
            "http://0177.0.0.1/",
            "http://evil@127.0.0.1/",
        ] {
            assert!(
                precheck_url(&reqwest::Url::parse(url).unwrap()).is_err(),
                "expected blocked: {url}"
            );
        }
    }

    #[test]
    fn precheck_url_allows_hostnames_and_public_ips() {
        for url in ["https://example.com", "https://1.1.1.1"] {
            assert!(
                precheck_url(&reqwest::Url::parse(url).unwrap()).is_ok(),
                "expected allowed: {url}"
            );
        }
    }

    #[test]
    fn precheck_url_normalizes_encoded_ipv4_literals() {
        let decimal = reqwest::Url::parse("http://2130706433/").unwrap();
        assert_eq!(decimal.host_str(), Some("127.0.0.1"));
        assert!(precheck_url(&decimal).is_err());

        let octal = reqwest::Url::parse("http://0177.0.0.1/").unwrap();
        assert_eq!(octal.host_str(), Some("127.0.0.1"));
        assert!(precheck_url(&octal).is_err());
    }

    #[test]
    fn precheck_url_strips_ipv6_brackets_from_host_str() {
        let url = reqwest::Url::parse("http://[::1]/path").unwrap();
        assert!(precheck_url(&url).is_err());
    }

    #[test]
    fn check_resolved_rejects_blocked_and_empty() {
        assert!(check_resolved(&[]).is_err());
        assert!(
            check_resolved(&["10.0.0.1".parse().unwrap(), "1.1.1.1".parse().unwrap()]).is_err()
        );
    }

    #[test]
    fn check_resolved_allows_all_public() {
        assert!(check_resolved(&["1.1.1.1".parse().unwrap(), "8.8.8.8".parse().unwrap()]).is_ok());
        assert!(check_resolved(&["2606:4700::1".parse().unwrap()]).is_ok());
    }

    // --- WebFetcher + tools ---

    #[tokio::test]
    async fn web_fetch_returns_html_to_text_on_success() {
        let html = "<p>Hello <b>web</b></p>";
        let tool = WebFetchTool::new(Box::new(MockFetcher::ok(html)));
        let outcome = tool
            .execute(json!({ "url": "https://example.com" }), &ctx(4096))
            .await;
        assert!(!outcome.is_error);
        assert_eq!(outcome.content, html_to_text(html));
        assert!(!outcome.truncated);
    }

    #[test]
    fn web_tools_require_network_permission() {
        let fetch = WebFetchTool::new(Box::new(MockFetcher::ok("unused")));
        let search = WebSearchTool::new(Box::new(MockFetcher::ok("unused")));

        assert_eq!(
            fetch.permission_level(),
            crate::tool::PermissionLevel::Network
        );
        assert_eq!(
            search.permission_level(),
            crate::tool::PermissionLevel::Network
        );
    }

    #[tokio::test]
    async fn web_fetch_preview_uses_the_canonical_url_sent_to_its_fetcher() {
        let scope = NetworkPermissionScope {
            max_redirects: 7,
            may_cross_origin: true,
            ssrf_each_hop: true,
        };

        for raw_url in [
            "https://user:pass@EXAMPLE.com:443/path",
            "https://bücher.example/guide",
            "http://2130706433/numeric-ip",
            "https://[2001:4860:4860::8888]:443/ipv6",
        ] {
            let capture = CaptureFetcher::new(scope.clone());
            let tool = WebFetchTool::new(Box::new(capture.clone()));
            let args = json!({ "url": raw_url });
            let preview = tool.network_permission_preview(&args);
            let expected = reqwest::Url::parse(raw_url).unwrap().to_string();

            assert!(preview.authorizable, "{raw_url}");
            assert_eq!(
                preview.canonical_initial_target.as_deref(),
                Some(expected.as_str())
            );
            assert_eq!(preview.scope, Some(scope.clone()));

            let outcome = tool.execute(args, &ctx(4096)).await;
            assert!(!outcome.is_error, "{raw_url}");
            assert_eq!(capture.urls(), vec![expected]);
        }
    }

    #[tokio::test]
    async fn web_search_preview_uses_the_ddg_url_sent_to_its_fetcher() {
        let scope = NetworkPermissionScope {
            max_redirects: 2,
            may_cross_origin: false,
            ssrf_each_hop: true,
        };

        for query in ["rust ownership", "Rust 所有权 & 借用"] {
            let capture = CaptureFetcher::new(scope.clone());
            let tool = WebSearchTool::new(Box::new(capture.clone()));
            let args = json!({ "query": query });
            let preview = tool.network_permission_preview(&args);
            let expected = ddg_search_url(query);

            assert!(preview.authorizable, "{query}");
            assert_eq!(
                preview.canonical_initial_target.as_deref(),
                Some(expected.as_str())
            );
            assert_eq!(preview.scope, Some(scope.clone()));

            let _ = tool.execute(args, &ctx(4096)).await;
            assert_eq!(capture.urls(), vec![expected]);
        }
    }

    #[tokio::test]
    async fn invalid_web_fetch_arguments_are_unauthorizable_and_do_not_fetch() {
        for args in [
            json!({}),
            json!({ "url": 42 }),
            json!({ "url": "http://[invalid" }),
        ] {
            let capture = CaptureFetcher::new(NetworkPermissionScope {
                max_redirects: 3,
                may_cross_origin: true,
                ssrf_each_hop: true,
            });
            let tool = WebFetchTool::new(Box::new(capture.clone()));
            let preview = tool.network_permission_preview(&args);

            assert!(!preview.authorizable, "{args}");
            assert!(preview
                .denial_reason
                .as_deref()
                .is_some_and(|reason| !reason.is_empty()));

            let outcome = tool.execute(args, &ctx(4096)).await;
            assert!(outcome.is_error);
            assert!(capture.urls().is_empty());
        }
    }

    #[test]
    fn reqwest_fetcher_declares_its_redirect_and_ssrf_permission_scope() {
        assert_eq!(
            ReqwestFetcher::new().permission_scope(),
            NetworkPermissionScope {
                max_redirects: 3,
                may_cross_origin: true,
                ssrf_each_hop: true,
            }
        );
    }

    #[test]
    fn decode_uddg_handles_https_href_without_leading_slashes() {
        assert_eq!(
            decode_uddg("https://duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.com%2Fdoc"),
            Some("https://example.com/doc".to_string())
        );
    }

    #[test]
    fn html_to_text_preserves_invalid_decimal_entity_literal() {
        assert_eq!(html_to_text("bad&#99999999;token"), "bad&#99999999;token");
    }

    #[test]
    fn precheck_url_rejects_redirect_target_to_loopback() {
        let base = reqwest::Url::parse("https://example.com/start").unwrap();
        let target = base.join("http://127.0.0.1/secret").expect("redirect join");
        assert!(precheck_url(&target).is_err());
    }

    #[test]
    fn is_readable_content_type_allows_text_and_missing_header() {
        assert!(is_readable_content_type(None));
        assert!(is_readable_content_type(Some("text/html; charset=utf-8")));
        assert!(!is_readable_content_type(Some("application/pdf")));
        assert!(!is_readable_content_type(Some("image/png")));
    }

    #[test]
    fn bytes_to_text_recovers_valid_prefix_from_invalid_utf8_tail() {
        let mut bytes = b"hello \xFF world".to_vec();
        while !bytes.is_empty() && std::str::from_utf8(&bytes).is_err() {
            bytes.pop();
        }
        assert_eq!(bytes_to_text(b"hello \xFF world".to_vec()), "hello ");
    }

    #[tokio::test]
    async fn reqwest_fetcher_rejects_localhost_via_dns_resolution() {
        let fetcher = ReqwestFetcher::new();
        let err = fetcher.fetch("http://localhost/").await.unwrap_err();
        assert!(err.to_string().contains("blocked IP"));
    }

    async fn one_shot_http_server(status_line: &str, extra_headers: &str, body: &str) -> String {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let status_line = status_line.to_string();
        let extra_headers = extra_headers.to_string();
        let body = body.to_string();

        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = vec![0u8; 16_384];
            let _ = socket.read(&mut buf).await;
            let response = format!(
                "HTTP/1.1 {status_line}\r\n{extra_headers}Content-Length: {}\r\n\r\n{body}",
                body.len()
            );
            let _ = socket.write_all(response.as_bytes()).await;
        });

        format!("http://{addr}")
    }

    #[tokio::test]
    async fn process_http_response_rejects_redirect_target_to_internal() {
        let base_url =
            one_shot_http_server("302 Found", "Location: http://127.0.0.1/secret\r\n", "").await;
        let response = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .build()
            .unwrap()
            .get(&base_url)
            .send()
            .await
            .unwrap();
        assert!(response.status().is_redirection());
        let joined = response
            .url()
            .join(
                response
                    .headers()
                    .get(reqwest::header::LOCATION)
                    .unwrap()
                    .to_str()
                    .unwrap(),
            )
            .unwrap();
        assert!(precheck_url(&joined).is_err());
    }

    #[tokio::test]
    async fn process_http_response_rejects_unsupported_content_type() {
        let base_url =
            one_shot_http_server("200 OK", "Content-Type: application/pdf\r\n", "%PDF-1.4").await;
        let response = reqwest::Client::builder()
            .no_proxy()
            .build()
            .unwrap()
            .get(&base_url)
            .send()
            .await
            .unwrap();
        let err = process_http_response(response).await.unwrap_err();
        assert!(err.to_string().contains("unsupported content-type"));
    }

    #[tokio::test]
    async fn process_http_response_caps_response_body_bytes() {
        let body = "x".repeat(WEB_MAX_BYTES + 1024);
        let base_url = one_shot_http_server("200 OK", "Content-Type: text/plain\r\n", &body).await;
        let response = reqwest::Client::builder()
            .no_proxy()
            .build()
            .unwrap()
            .get(&base_url)
            .send()
            .await
            .unwrap();
        let text = process_http_response(response).await.unwrap();
        assert_eq!(text.len(), WEB_MAX_BYTES);
    }

    #[tokio::test]
    async fn process_http_response_rejects_non_success_http_status() {
        let base_url = one_shot_http_server("404 Not Found", "", "missing").await;
        let response = reqwest::Client::builder()
            .no_proxy()
            .build()
            .unwrap()
            .get(&base_url)
            .send()
            .await
            .unwrap();
        let err = process_http_response(response).await.unwrap_err();
        assert!(err.to_string().contains("HTTP 404"));
    }

    #[tokio::test]
    async fn web_fetch_returns_error_for_missing_url_argument() {
        let tool = WebFetchTool::new(Box::new(MockFetcher::ok("unused")));
        let outcome = tool.execute(json!({}), &ctx(4096)).await;
        assert!(outcome.is_error);
        assert_eq!(outcome.content, "missing or invalid url");
    }

    #[tokio::test]
    async fn web_search_returns_error_for_missing_query_argument() {
        let tool = WebSearchTool::new(Box::new(MockFetcher::ok(DDG_FIXTURE)));
        let outcome = tool.execute(json!({}), &ctx(4096)).await;
        assert!(outcome.is_error);
        assert_eq!(outcome.content, "missing or invalid query");
    }

    #[tokio::test]
    async fn web_fetch_returns_error_when_fetcher_fails() {
        let tool = WebFetchTool::new(Box::new(MockFetcher::err("network down")));
        let outcome = tool
            .execute(json!({ "url": "https://example.com" }), &ctx(4096))
            .await;
        assert!(outcome.is_error);
    }

    #[tokio::test]
    async fn web_fetch_truncates_when_exceeding_max_output_bytes() {
        let html = "é".repeat(20);
        let tool = WebFetchTool::new(Box::new(MockFetcher::ok(&html)));
        let outcome = tool
            .execute(json!({ "url": "https://example.com" }), &ctx(5))
            .await;
        assert!(!outcome.is_error);
        assert!(outcome.truncated);
        assert!(outcome.content.len() <= 5);
    }

    #[tokio::test]
    async fn web_search_returns_parsed_results_on_success() {
        let tool = WebSearchTool::new(Box::new(MockFetcher::ok(DDG_FIXTURE)));
        let outcome = tool
            .execute(json!({ "query": "rust ownership" }), &ctx(8192))
            .await;
        assert!(!outcome.is_error);
        assert!(outcome.content.contains("What is Ownership?"));
        assert!(outcome
            .content
            .contains("doc.rust-lang.org/book/ch04-01-what-is-ownership.html"));
        assert!(outcome.content.contains("Understanding Ownership"));
    }

    #[tokio::test]
    async fn web_search_returns_error_when_fetcher_fails() {
        let tool = WebSearchTool::new(Box::new(MockFetcher::err("ddg blocked")));
        let outcome = tool.execute(json!({ "query": "rust" }), &ctx(4096)).await;
        assert!(outcome.is_error);
    }

    #[tokio::test]
    async fn web_search_returns_error_when_no_results_parsed() {
        let tool = WebSearchTool::new(Box::new(MockFetcher::ok("<html></html>")));
        let outcome = tool.execute(json!({ "query": "rust" }), &ctx(4096)).await;
        assert!(outcome.is_error);
    }
}
