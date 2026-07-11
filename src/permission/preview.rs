use crate::tool::NetworkPermissionPreview;

pub fn format_network_permission_preview(preview: &NetworkPermissionPreview) -> String {
    let args = terminal_safe(
        &serde_json::to_string(&preview.full_args).unwrap_or_else(|_| "null".to_string()),
    );

    if !preview.authorizable {
        return format!(
            "reject-only\nargs: {args}\nreason: {}",
            terminal_safe(
                preview
                    .denial_reason
                    .as_deref()
                    .unwrap_or("network preview is not authorizable")
            )
        );
    }

    let target = terminal_safe(
        preview
            .canonical_initial_target
            .as_deref()
            .unwrap_or("missing"),
    );
    let scope = preview.scope.as_ref();
    match scope {
        Some(scope) => {
            let redirect_kind = if scope.may_cross_origin {
                "可能跨站"
            } else {
                "仅同站"
            };
            let ssrf_policy = if scope.ssrf_each_hop {
                "每跳仍过 SSRF"
            } else {
                "未声明逐跳 SSRF"
            };
            format!(
                "args: {args}\ntarget: {target}\nscope: max_redirects={}, may_cross_origin={}, ssrf_each_hop={}\n授权:仅本次调用；最多 {} 次、{}的公网重定向；{}",
                scope.max_redirects,
                scope.may_cross_origin,
                scope.ssrf_each_hop,
                scope.max_redirects,
                redirect_kind,
                ssrf_policy
            )
        }
        None => format!("args: {args}\ntarget: {target}\nscope: missing"),
    }
}

fn terminal_safe(value: &str) -> String {
    value
        .chars()
        .flat_map(|character| {
            if requires_escape(character) {
                format!("\\u{{{:X}}}", character as u32)
                    .chars()
                    .collect::<Vec<_>>()
            } else {
                vec![character]
            }
        })
        .collect()
}

fn requires_escape(character: char) -> bool {
    character.is_control()
        || matches!(
            character as u32,
            0x0300..=0x036F
                | 0x061C
                | 0x180B..=0x180F
                | 0x200B..=0x200F
                | 0x202A..=0x202E
                | 0x2060..=0x206F
                | 0xFE00..=0xFE0F
                | 0xFEFF
        )
}

#[cfg(test)]
mod tests {
    use super::format_network_permission_preview;
    use crate::tool::{NetworkPermissionPreview, NetworkPermissionScope};
    use serde_json::json;

    fn authorizable_preview(args: serde_json::Value) -> NetworkPermissionPreview {
        NetworkPermissionPreview {
            authorizable: true,
            full_args: args,
            canonical_initial_target: Some("https://example.com/path".to_string()),
            scope: Some(NetworkPermissionScope {
                max_redirects: 3,
                may_cross_origin: true,
                ssrf_each_hop: true,
            }),
            denial_reason: None,
        }
    }

    #[test]
    fn formatter_includes_lossless_args_target_and_scope() {
        let output = format_network_permission_preview(&authorizable_preview(json!({
            "query": "Rust 所有权 & 借用",
            "literal": r"\u{202E}",
        })));

        assert!(output.contains(r#""query":"Rust 所有权 & 借用""#));
        assert!(output.contains(r#""literal":"\\u{202E}""#));
        assert!(output.contains("https://example.com/path"));
        assert!(output.contains("max_redirects=3"));
        assert!(output.contains("may_cross_origin=true"));
        assert!(output.contains("ssrf_each_hop=true"));
    }

    #[test]
    fn formatter_escapes_terminal_unsafe_unicode_reversibly() {
        let output = format_network_permission_preview(&authorizable_preview(json!({
            "text": "a\u{009b}b\u{202e}c\u{2066}d\u{2069}e\u{200b}f\u{0301}g\u{fe0f}",
        })));

        for escaped in [
            r"\u{9B}",
            r"\u{202E}",
            r"\u{2066}",
            r"\u{2069}",
            r"\u{200B}",
            r"\u{301}",
            r"\u{FE0F}",
        ] {
            assert!(output.contains(escaped), "missing {escaped}: {output:?}");
        }
    }

    #[test]
    fn formatter_marks_reject_only_preview_with_generic_args_and_reason() {
        let preview = NetworkPermissionPreview {
            authorizable: false,
            full_args: json!({ "url": "bad\u{202e}target" }),
            canonical_initial_target: None,
            scope: None,
            denial_reason: Some("invalid URL".to_string()),
        };

        let output = format_network_permission_preview(&preview);
        assert!(output.contains("reject-only"));
        assert!(output.contains("invalid URL"));
        assert!(output.contains(r"\u{202E}"));
        assert!(!output.contains("max_redirects="));
    }
}
