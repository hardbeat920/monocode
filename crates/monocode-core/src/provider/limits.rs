//! Keep Claude's reset time through its terminal error, even when its synthetic
//! assistant frame ends the transport before the result frame can arrive.
use super::*;
use crate::usage::Limit;

impl Protocol {
    pub(super) fn claude_error(&mut self, error: &str, frame: &Value) -> String {
        let info = frame
            .get("rate_limit_info")
            .or_else(|| frame.get("quotaLimits"))
            .or_else(|| frame["apiErrorParams"].get("rate_limit_info"));
        if let Some(limit) = info.and_then(Limit::from_claude_info) {
            self.claude_usage_limit = Some(limit);
        }
        if self.claude_usage_limit.is_none() {
            self.claude_usage_limit = Limit::from_error(error).or_else(|| {
                frame["errors"]
                    .as_array()?
                    .iter()
                    .filter_map(Value::as_str)
                    .find_map(Limit::from_error)
            });
        }
        if self.claude_usage_limit.is_some() {
            "Claude Code reached its usage limit. Choose another available model or retry when the limit resets.".into()
        } else {
            error.into()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn protocol() -> Protocol {
        Protocol::new(Request {
            kind: Kind::Claude,
            binary: "unused".into(),
            cwd: "/tmp".into(),
            prompt: "Hello".into(),
            provider_id: None,
            model: None,
            settings: Default::default(),
        })
    }

    #[test]
    fn claude_limit_assistant_keeps_the_rejected_windows_reset_time() {
        let mut p = protocol();
        let mut out = Vec::new();
        p.frame(
            json!({"type":"rate_limit_event","rate_limit_info":{
                "status":"rejected","resetsAt":1792600200,"rateLimitType":"five_hour"
            }}),
            &mut out,
        )
        .unwrap();
        let error = p
            .frame(
                json!({"type":"assistant","error":"rate_limit",
                    "message":{"content":[{"type":"text","text":"You've hit your session limit"}]}
                }),
                &mut out,
            )
            .unwrap_err();
        assert!(error.contains("usage limit"));
        assert_eq!(
            p.claude_usage_limit.as_ref().unwrap().resets_at,
            Some(1_792_600_200_000)
        );
        assert!(p.claude_usage_limit.unwrap().resume_at_reset);
    }

    #[test]
    fn claude_limit_result_and_embedded_error_info_work_without_a_rate_event() {
        for frame in [
            json!({"type":"result","is_error":true,"result":"rate_limit"}),
            json!({"type":"result","is_error":true,"errors":["You've hit your usage limit."]}),
            json!({"type":"assistant","error":"rate_limit","apiErrorParams":{
                "rate_limit_info":{"status":"rejected","resetsAt":"1792600200000"}
            }}),
        ] {
            let mut p = protocol();
            assert!(p
                .frame(frame, &mut Vec::new())
                .unwrap_err()
                .contains("usage limit"));
            assert!(p.claude_usage_limit.is_some());
        }
    }

    #[test]
    fn claude_recovered_overage_and_child_limits_do_not_block_the_parent() {
        let mut p = protocol();
        let mut out = Vec::new();
        for info in [
            json!({"status":"allowed_warning","resetsAt":1792600200}),
            json!({"status":"rejected","isUsingOverage":true}),
            json!({"status":"rejected","rateLimitType":"overage"}),
        ] {
            p.claude_usage_limit = Some(Limit::default());
            p.frame(
                json!({"type":"rate_limit_event","rate_limit_info":info}),
                &mut out,
            )
            .unwrap();
            assert!(p.claude_usage_limit.is_none());
        }
        p.frame(
            json!({"type":"rate_limit_event","parent_tool_use_id":"child",
                "rate_limit_info":{"status":"rejected","resetsAt":1792600200}
            }),
            &mut out,
        )
        .unwrap();
        assert!(p.claude_usage_limit.is_none());
        assert_eq!(
            p.frame(
                json!({"type":"assistant","error":"authentication_failed"}),
                &mut out
            )
            .unwrap_err(),
            "authentication_failed"
        );
        assert!(p.claude_usage_limit.is_none());
    }

    #[cfg(unix)]
    #[test]
    fn claude_transport_delivers_the_limit_before_returning_its_terminal_error() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let binary = dir.path().join("claude-fixture");
        std::fs::write(&binary, r#"#!/bin/sh
read -r initialize
printf '%s\n' '{"type":"rate_limit_event","rate_limit_info":{"status":"rejected","resetsAt":1792600200}}' '{"type":"assistant","error":"rate_limit"}'
"#).unwrap();
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();
        let mut request = protocol().request;
        request.binary = binary;
        request.cwd = dir.path().into();
        let mut limited = Vec::new();
        let result = run(request, Arc::new(AtomicBool::new(false)), |events| {
            limited.extend(events.into_iter().filter_map(|event| match event {
                Event::UsageLimited(limit) => Some(limit),
                _ => None,
            }));
            Ok(())
        });
        assert!(result.unwrap_err().contains("usage limit"));
        assert_eq!(
            limited,
            vec![Limit {
                resets_at: Some(1_792_600_200_000),
                resume_at_reset: true
            }]
        );
    }
}
