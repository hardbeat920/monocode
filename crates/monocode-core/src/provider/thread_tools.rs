use super::*;
use std::io::Write;

pub(super) fn enabled(request: &Request) -> bool {
    !request.settings.text_only
        && request
            .settings
            .environment
            .get("MONOCODE_THREADS")
            .is_some_and(|v| v == "1")
}

pub(super) fn pi_extension(request: &Request) -> Result<Option<tempfile::NamedTempFile>, String> {
    if !request.kind.is_pi() || !enabled(request) {
        return Ok(None);
    }
    let mut extension = tempfile::Builder::new()
        .prefix("monocode-threads-")
        .suffix(".mjs")
        .tempfile()
        .map_err(|e| e.to_string())?;
    let definitions = serde_json::to_string(&crate::visual_bridge::thread_tool_definitions())
        .map_err(|e| e.to_string())?;
    let environment = &request.settings.environment;
    let guidance = crate::visual_bridge::instructions(environment, false)
        .unwrap_or_else(|| crate::visual_bridge::THREAD_INSTRUCTIONS.into());
    let json = |text: &str| serde_json::to_string(text).map_err(|e| e.to_string());
    let source = include_str!("thread_tools.mjs")
        .replace("/* TOOL_DEFINITIONS */ []", &definitions)
        .replace("/* GUIDANCE */ \"\"", &json(&guidance)?)
        .replace(
            "/* THREAD_GUIDANCE */ \"\"",
            &json(crate::visual_bridge::THREAD_INSTRUCTIONS)?,
        );
    extension
        .write_all(source.as_bytes())
        .map_err(|e| e.to_string())?;
    Ok(Some(extension))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn extensions_contain_no_credentials_and_children_get_no_extension() {
        let mut request = Request {
            kind: Kind::Pi,
            binary: "pi".into(),
            cwd: "/project".into(),
            prompt: "Review".into(),
            provider_id: None,
            model: None,
            settings: Settings::default(),
        };
        assert!(pi_extension(&request).unwrap().is_none());
        request
            .settings
            .environment
            .insert("MONOCODE_THREADS".into(), "1".into());
        request
            .settings
            .environment
            .insert("MONOCODE_APP_TOKEN".into(), "private-test-token".into());
        let extension = pi_extension(&request).unwrap().unwrap();
        let source = std::fs::read_to_string(extension.path()).unwrap();
        assert!(source.contains("thread_start"));
        assert!(source.contains("registerMcpServer"));
        // Pi's system prompt gets the subagent guidance too.
        assert!(source.contains("before_agent_start"));
        assert!(!source.contains("/* THREAD_GUIDANCE */ \"\""));
        assert!(source.contains("Use thread_start instead of your built-in subagent"));
        assert!(source.contains("\"name\":\"new_thread\""));
        assert!(source.contains("\"threads.new\""));
        assert!(!source.contains("private-test-token"));
        request
            .settings
            .environment
            .insert("MONOCODE_THREADS".into(), "0".into());
        assert!(pi_extension(&request).unwrap().is_none());
    }
    /// A fake Claude and Codex fail the turn unless MonoCode's subagent
    /// guidance reaches their own system prompt, and unless their own
    /// subagent tool is off exactly when the setting asks.
    #[cfg(unix)]
    #[test]
    fn claude_and_codex_system_prompts_carry_subagent_guidance() {
        use std::os::unix::fs::PermissionsExt;
        let directory = tempfile::tempdir().unwrap();
        let claude = directory.path().join("claude");
        std::fs::write(
            &claude,
            r#"#!/usr/bin/env python3
import json,os,sys
only=os.environ['MONOCODE_TEST_SUBAGENTS_ONLY']=='1'
assert (['--disallowedTools','Agent'] in [sys.argv[i:i+2] for i in range(len(sys.argv))])==only, sys.argv
prompt=sys.argv[sys.argv.index('--append-system-prompt')+1]
assert 'thread_start' in prompt and 'built-in subagent' in prompt, prompt
def send(value):
    print(json.dumps(value),flush=True)
for line in sys.stdin:
    frame=json.loads(line)
    if frame.get('type')=='control_request':
        send({'type':'control_response','response':{'subtype':'success','request_id':frame['request_id']}})
    elif frame.get('type')=='user':
        send({'type':'result','subtype':'success','is_error':False,'result':'ok','usage':{}})
"#,
        )
        .unwrap();
        let codex = directory.path().join("codex");
        std::fs::write(
            &codex,
            r#"#!/usr/bin/env python3
import json,os,sys
only=os.environ['MONOCODE_TEST_SUBAGENTS_ONLY']=='1'
assert ('features.multi_agent=false' in sys.argv)==only, sys.argv
def send(value):
    print(json.dumps(value),flush=True)
for line in sys.stdin:
    frame=json.loads(line)
    method=frame.get('method')
    if method=='initialize':
        send({'id':frame['id'],'result':{}})
    elif method=='thread/start':
        instructions=frame['params']['developerInstructions']
        assert instructions.startswith('You are the coding agent in Monocode.'), instructions
        assert 'thread_start' in instructions and 'built-in subagent' in instructions, instructions
        send({'id':frame['id'],'result':{'thread':{'id':'thread'}}})
    elif method=='turn/start':
        send({'id':frame['id'],'result':{'turn':{'id':'turn'}}})
        send({'method':'turn/completed','params':{'turn':{'status':'completed'}}})
"#,
        )
        .unwrap();
        let mut settings = Settings::default();
        for (key, value) in [
            ("MONOCODE_THREADS", "1"),
            ("MONOCODE_VISUAL_ENDPOINT", "127.0.0.1:1"),
            ("MONOCODE_VISUAL_TOKEN", "fixture"),
            ("MONOCODE_VISUAL_SESSION", "7"),
            ("MONOCODE_VISUAL_EXECUTABLE", "/fixture/app"),
        ] {
            settings.environment.insert(key.into(), value.into());
        }
        let fakes = [(Kind::Claude, claude), (Kind::Codex, codex)];
        for (_, binary) in &fakes {
            std::fs::set_permissions(binary, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        for ((kind, binary), only) in fakes.iter().flat_map(|fake| [(fake, false), (fake, true)]) {
            let mut settings = settings.clone();
            settings.monocode_subagents_only = only;
            settings.environment.insert(
                "MONOCODE_TEST_SUBAGENTS_ONLY".into(),
                if only { "1" } else { "0" }.into(),
            );
            let kind = *kind;
            let request = Request {
                kind,
                binary: binary.clone(),
                cwd: directory.path().into(),
                prompt: "Use a Haiku subagent".into(),
                provider_id: None,
                model: None,
                settings,
            };
            let mut events = Vec::new();
            run(request, Arc::new(AtomicBool::new(false)), |batch| {
                events.extend(batch);
                Ok(())
            })
            .unwrap_or_else(|error| panic!("{kind:?}: {error}"));
            assert!(events.contains(&Event::Done), "{kind:?}");
        }
    }
    #[cfg(unix)]
    #[test]
    fn opencode_thread_tools_use_acp_and_forward_the_session_capability() {
        use std::os::unix::fs::PermissionsExt;
        let directory = tempfile::tempdir().unwrap();
        let binary = directory.path().join("opencode");
        std::fs::write(
            &binary,
            r#"#!/usr/bin/env python3
import json,sys
assert sys.argv[1:] == ['acp'], sys.argv
for line in sys.stdin:
    request=json.loads(line)
    method=request.get('method')
    result={}
    if method == 'initialize':
        result={'protocolVersion':1,'agentCapabilities':{},'authMethods':[]}
    elif method == 'session/new':
        server=request['params']['mcpServers'][0]
        assert server['name']=='monocode'
        env={entry['name']:entry['value'] for entry in server['env']}
        assert env['MONOCODE_APP_SESSION']=='7'
        assert env['MONOCODE_THREADS']=='1'
        result={'sessionId':'fixture-session'}
    elif method == 'session/prompt':
        result={'stopReason':'end_turn'}
    print(json.dumps({'jsonrpc':'2.0','id':request['id'],'result':result}),flush=True)
"#,
        )
        .unwrap();
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
        let mut settings = Settings::default();
        for (key, value) in [
            ("MONOCODE_THREADS", "1"),
            ("MONOCODE_APP_SESSION", "7"),
            ("MONOCODE_VISUAL_ENDPOINT", "127.0.0.1:1"),
            ("MONOCODE_VISUAL_TOKEN", "fixture"),
            ("MONOCODE_VISUAL_SESSION", "7"),
            ("MONOCODE_VISUAL_EXECUTABLE", "/fixture/app"),
        ] {
            settings.environment.insert(key.into(), value.into());
        }
        let request = Request {
            kind: Kind::OpenCode,
            binary,
            cwd: directory.path().into(),
            prompt: "Review".into(),
            provider_id: None,
            model: None,
            settings,
        };
        let mut events = Vec::new();
        run(request, Arc::new(AtomicBool::new(false)), |batch| {
            events.extend(batch);
            Ok(())
        })
        .unwrap();
        assert!(events.contains(&Event::Done));
        forget_session(
            Kind::OpenCode,
            "fixture-session",
            crate::accounts::DEFAULT,
            directory.path(),
        );
    }
}
