use super::*;
use serde_json::json;

fn runtime(source: &str) -> VisualRuntime {
    VisualRuntime::new(source).unwrap()
}
fn button(node: &Node) -> &str {
    match node {
        Node::Button { handler, .. } => handler,
        Node::Column { children, .. } => children
            .iter()
            .find_map(|child| match child {
                Node::Button { handler, .. } => Some(handler.as_str()),
                _ => None,
            })
            .unwrap(),
        _ => panic!("Expected button"),
    }
}

#[test]
fn native_state_and_handlers_survive_rerenders() {
    let mut vm = runtime("const s=visual.state({count:0}); return () => visual.column([visual.text(s.count),visual.button('Add',()=>s.count++)]);");
    let first = vm.render().unwrap();
    let updated = vm.dispatch(button(&first), json!(null)).unwrap();
    let Node::Column { children, .. } = &updated else {
        panic!()
    };
    assert!(matches!(&children[0], Node::Text { text, .. } if text == "1"));
    let updated = vm.dispatch(button(&updated), json!(null)).unwrap();
    let Node::Column { children, .. } = updated else {
        panic!()
    };
    assert!(matches!(&children[0], Node::Text { text, .. } if text == "2"));
}

#[test]
fn source_seeded_randomness_repeats_setup_and_advances_with_state() {
    let source = "const s=visual.state({initial:Math.random(),count:0});return () => visual.column([visual.text(s.initial),visual.text(Math.random()),visual.text(s.count),visual.button('Step',()=>s.count++)]);";
    let mut left = runtime(source);
    let mut right = runtime(source);
    let first = left.render().unwrap();
    let repeated = right.render().unwrap();
    assert_eq!(
        serde_json::to_value(&first).unwrap(),
        serde_json::to_value(repeated).unwrap()
    );
    let advanced = left.dispatch(button(&first), json!(null)).unwrap();
    let first = serde_json::to_value(first).unwrap();
    let advanced = serde_json::to_value(advanced).unwrap();
    assert_eq!(
        first["children"][0]["text"],
        advanced["children"][0]["text"]
    );
    assert_ne!(
        first["children"][1]["text"],
        advanced["children"][1]["text"]
    );
    assert_eq!(advanced["children"][2]["text"], "1");
    assert_eq!(serde_json::to_value(check(source).unwrap()).unwrap(), first);
}

#[test]
fn stale_and_unregistered_handlers_are_rejected() {
    let mut vm = runtime("return () => visual.button('Press',()=>{});");
    let first = vm.render().unwrap();
    vm.dispatch(button(&first), json!(null)).unwrap();
    assert!(vm
        .dispatch(button(&first), json!(null))
        .unwrap_err()
        .contains("stale"));
    // A queued old native event must not stop the currently active controls.
    let fresh = vm.render().unwrap();
    assert!(vm.dispatch(button(&fresh), json!(null)).is_ok());
    assert!(
        check("return () => ({kind:'button',label:'Forged',handler:'1:0',disabled:false});")
            .unwrap_err()
            .contains("stale")
    );
}

#[test]
fn control_payloads_are_checked_before_callbacks() {
    for (source, value) in [
        (
            "return () => visual.toggle('Enabled',false,()=>{});",
            json!("yes"),
        ),
        (
            "return () => visual.slider('Amount',5,0,10,()=>{});",
            json!(11),
        ),
        ("return () => visual.tabs(['A','B'],0,()=>{});", json!(2)),
        (
            "return () => visual.select('Kind',['A','B'],0,()=>{});",
            json!(-1),
        ),
        ("return () => visual.input('Name','',()=>{});", json!(false)),
        (
            "return () => visual.button('Press',()=>{});",
            json!({"anything":true}),
        ),
    ] {
        let mut vm = runtime(source);
        let node = vm.render().unwrap();
        let id = serde_json::to_value(node).unwrap()["handler"]
            .as_str()
            .unwrap()
            .to_owned();
        assert!(vm
            .dispatch(&id, value)
            .unwrap_err()
            .contains("Invalid value"));
    }
}

#[test]
fn input_select_and_slider_update_state() {
    let mut vm = runtime("const s={value:'old'};return () => visual.column([visual.text(s.value),visual.input('Name',s.value,v=>s.value=v)]);");
    let node = vm.render().unwrap();
    let id = serde_json::to_value(&node).unwrap()["children"][1]["handler"]
        .as_str()
        .unwrap()
        .to_owned();
    let updated = serde_json::to_value(vm.dispatch(&id, json!("new")).unwrap()).unwrap();
    assert_eq!(updated["children"][0]["text"], "new");
    let mut vm = runtime(
        "const s={value:0};return () => visual.select('Pick',['a','b'],s.value,v=>s.value=v);",
    );
    let first = serde_json::to_value(vm.render().unwrap()).unwrap();
    let updated = vm
        .dispatch(first["handler"].as_str().unwrap(), json!(1))
        .unwrap();
    assert!(matches!(updated, Node::Select { selected: 1, .. }));
    let mut vm = runtime("const s={value:2};return () => visual.slider('Value',s.value,0,10,v=>s.value=v,{step:0.5});");
    let first = serde_json::to_value(vm.render().unwrap()).unwrap();
    let updated = vm
        .dispatch(first["handler"].as_str().unwrap(), json!(3.5))
        .unwrap();
    assert!(matches!(updated, Node::Slider { value, .. } if value == 3.5));
}

#[test]
fn app_owns_styles_and_rejects_arbitrary_colors() {
    for source in [
        "return () => visual.text('Bad',{color:'#ff0000'});",
        "return () => visual.column([],{background:'red'});",
        "return () => visual.text('Bad',{fontSize:42});",
        "return () => ({kind:'text',text:'Bad',size:'body',color:'ink',css:'position:fixed'});",
        "return () => visual.scene([visual.object({kind:'box'},{color:'#ff0000'})]);",
    ] {
        assert!(check(source).is_err(), "accepted {source}");
    }
    assert!(check("return () => visual.text('Native',{size:'title',color:'accent'});").is_ok());
}

#[test]
fn no_filesystem_network_process_or_dom_globals() {
    let node = check("return () => visual.text([typeof fetch,typeof XMLHttpRequest,typeof require,typeof process,typeof document,typeof console,typeof setTimeout].join(','));").unwrap();
    assert!(
        matches!(node, Node::Text {text,..} if text == "undefined,undefined,undefined,undefined,undefined,undefined,undefined")
    );
}

#[test]
fn native_blocking_primitives_are_unavailable() {
    let node =
        check("return () => visual.text([typeof Atomics,typeof SharedArrayBuffer].join(','));")
            .unwrap();
    assert!(matches!(node, Node::Text {text,..} if text == "undefined,undefined"));
    assert!(check("globalThis.Atomics={wait:()=>{}}; return () => visual.text('No');").is_err());
}

#[test]
fn exception_message_accessors_share_the_execution_deadline() {
    let started = Instant::now();
    let error = check(
        "const e=new Error();Object.defineProperty(e,'message',{get(){while(true){}}});throw e;",
    )
    .unwrap_err();
    assert!(error.contains("time budget"));
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[test]
fn runaway_initialization_render_and_handler_are_interrupted() {
    let started = Instant::now();
    assert!(
        VisualRuntime::new("while(true){} return () => visual.text('No');")
            .err()
            .unwrap()
            .contains("time budget")
    );
    assert!(check("return () => {while(true){} };")
        .unwrap_err()
        .contains("time budget"));
    assert!(
        check("return () => visual.button('Loop',()=>{while(true){}});")
            .unwrap_err()
            .contains("time budget")
    );
    assert!(started.elapsed() < Duration::from_secs(3));
}

#[test]
fn script_errors_keep_actionable_messages() {
    assert!(check("throw new Error('missing dataset');")
        .unwrap_err()
        .contains("missing dataset"));
    assert!(check("return () => { throw new Error('wrong chart'); };")
        .unwrap_err()
        .contains("wrong chart"));
    assert!(
        check("return () => visual.button('Bad',()=>{throw new Error('bad update')});")
            .unwrap_err()
            .contains("bad update")
    );
}

#[test]
fn check_probes_events_without_changing_the_initial_view() {
    let node = check("const s={n:0};return () => visual.column([visual.text(s.n),visual.button('Add',()=>s.n++)]);").unwrap();
    let Node::Column { children, .. } = node else {
        panic!()
    };
    assert!(matches!(&children[0],Node::Text {text,..} if text == "0"));
    assert!(check("return () => visual.button('Disabled',()=>{throw new Error('must not execute')},{disabled:true});").is_ok());
}

#[test]
fn invalid_numeric_data_is_never_painted() {
    for source in [
        "return () => visual.chart('line',[{label:'Bad',values:[NaN]}]);",
        "return () => visual.canvas([visual.circle(0,0,Infinity)]);",
        "return () => visual.canvas([visual.rect(0,0,-1,10)]);",
        "return () => visual.slider('Bad',0,0,0,()=>{});",
        "return () => visual.scene([],{camera:{eye:[0,0,0],target:[0,0,0]}});",
        "return () => visual.scene([visual.object({kind:'sphere'},{scale:[-1,1,1]})]);",
        "return () => visual.scene([],{width:1e20});",
    ] {
        assert!(check(source).is_err(), "accepted {source}");
    }
}

#[test]
fn structural_and_resource_limits_reject_unbounded_views() {
    for source in [
        "return () => visual.column(Array.from({length:512},()=>visual.text('many')));",
        "return () => {let n=visual.text('deep');for(let i=0;i<18;i++)n=visual.column([n]);return n};",
        "return () => visual.text('x'.repeat(16385));",
        "return () => visual.canvas(Array.from({length:513},()=>visual.circle(0,0,1)));",
        "return () => visual.table(['a','b'],[['one']]);",
        "return () => visual.chart('line',[{label:'a',values:[1]},{label:'b',values:[1,2]}]);",
        "return () => visual.scene([visual.object({kind:'mesh',vertices:[[0,0,0],[1,0,0],[0,1,0]],indices:[[0,1,3]]})]);",
        "return () => visual.scene(Array.from({length:24},()=>visual.object({kind:'sphere'})));",
        "return () => visual.input('Too long','x'.repeat(4097),()=>{});",
    ] { assert!(check(source).is_err(), "accepted {source}"); }
    assert!(VisualRuntime::new(&" ".repeat(MAX_SOURCE_BYTES + 1)).is_err());
    assert!(check("return () => visual.text('x'.repeat(64*1024*1024));").is_err());
}

#[test]
fn cycles_async_and_invalid_return_values_are_rejected() {
    for source in [
        "return async () => visual.text('Async');",
        "return () => {const x={kind:'column',gap:12};x.children=[x];return x};",
        "return () => null;",
        "return visual.text('Forgot render');",
        "return () => visual.button('Async',async()=>{});",
    ] {
        assert!(check(source).is_err(), "accepted {source}");
    }
}

#[test]
fn document_metadata_and_primitive_mesh_are_validated() {
    let mut doc = VisualDocument {
        id: "chart-1".into(),
        title: "Chart".into(),
        source: "return () => visual.text('ok');".into(),
    };
    assert!(doc.validate().is_ok());
    doc.id = "../escape".into();
    assert!(doc.validate().is_err());
    doc.id = "okay".into();
    doc.title = " ".into();
    assert!(doc.validate().is_err());
    assert!(check("return () => visual.scene([visual.object({kind:'mesh',vertices:[[0,0,0],[1,0,0],[0,1,0]],indices:[[0,1,2]]})]);").is_ok());
    assert!(check("return () => visual.canvas([visual.rect(0,0,10,10),visual.circle(20,20,3),visual.line(0,0,20,20),visual.polyline([[0,0],[2,2]]),visual.label(2,3,'hello')]);").is_ok());
}

#[test]
fn dashboard_components_render_and_reject_invalid_data() {
    let tree = check(
        "return () => visual.card([
            visual.stat('MRR', '$48K', {delta: '+12%', trend: [1, 3, 2, 5]}),
            visual.gauge('CPU', 68, {unit: '%'}),
            visual.donut([{label: 'API', value: 3}, {label: 'Web', value: 1}], {label: 'Requests'}),
            visual.heatmap([[0, 1, 2], [3, 4, 5]], {rows: ['Mon', 'Tue']}),
            visual.progress('Build', 28),
        ]);",
    )
    .unwrap();
    let Node::Card { children, .. } = tree else {
        panic!("Expected card")
    };
    assert!(
        matches!(&children[0], Node::Stat { delta: Some(d), trend, .. } if d == "+12%" && trend.len() == 4)
    );
    assert!(matches!(&children[1], Node::Gauge { max, .. } if *max == 100.));
    assert!(
        matches!(&children[2], Node::Donut { slices, .. } if slices[1].color == ThemeColor::Series2)
    );
    for source in [
        "return () => visual.gauge('CPU', 1, {min: 5, max: 5});",
        "return () => visual.donut([{label: 'none', value: 0}]);",
        "return () => visual.donut([{label: 'neg', value: -1}]);",
        "return () => visual.heatmap([[1, 2], [3]]);",
        "return () => visual.heatmap([[1, 2]], {rows: ['a', 'b']});",
        "return () => visual.progress('Over', 120);",
        "return () => visual.stat('Bad', '1', {trend: [NaN]});",
        "return () => visual.card([visual.button('Stale', () => {})], {columns: 2});",
    ] {
        assert!(check(source).is_err(), "accepted {source}");
    }
}
