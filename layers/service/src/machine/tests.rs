use super::*;

#[test]
fn a_snapshot_names_the_platform_it_is_running_on() {
    // The multiplatform claim is checked here rather than asserted: whatever
    // this is built for is what it reports.
    let snapshot = snapshot(&[]);
    assert!(snapshot.contains(&format!(r#""os":"{}""#, std::env::consts::OS)));
    assert!(snapshot.contains(&format!(r#""arch":"{}""#, std::env::consts::ARCH)));
}

#[test]
fn a_snapshot_lists_the_adapters_this_process_can_serve() {
    // The fact a placement actually needs: whether a node can be created here.
    let snapshot = snapshot(&["llamacpp".into(), "vllm".into()]);
    assert!(
        snapshot.contains(r#""adapters":["llamacpp","vllm"]"#),
        "{snapshot}"
    );
}

#[test]
fn an_agent_with_no_backend_says_so_rather_than_omitting_the_field() {
    let snapshot = snapshot(&[]);
    assert!(snapshot.contains(r#""adapters":[]"#), "{snapshot}");
}

#[test]
fn a_name_that_would_break_the_snapshot_is_escaped() {
    let snapshot = snapshot(&[r#"od"d"#.into()]);
    assert!(snapshot.contains(r#""od\"d""#), "{snapshot}");
}
