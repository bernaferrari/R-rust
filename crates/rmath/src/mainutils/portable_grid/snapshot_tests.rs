use super::*;
#[test]
fn malformed_grid_snapshots_are_rejected_without_panicking() {
    let mut state = GridState::default();
    state.ensure((200, 100));
    let valid = serde_json::to_value(&state).unwrap();
    assert!(GridState::decode_snapshot(&serde_json::to_vec(&valid).unwrap()).is_ok());
    for (path, value) in [
        ("/nodes/0/parent", serde_json::json!(999999)),
        ("/nodes/0/children", serde_json::json!([0])),
        ("/active/0", serde_json::json!(100)),
        ("/frames/0/width", serde_json::json!(300)),
        ("/nodes/0/frame/gp/col", serde_json::json!([])),
        ("/nodes/0/frame/scale", serde_json::json!([[0, 0], [0, 1]])),
        ("/nodes/0/frame/gp/lineheight", serde_json::json!(-1)),
    ] {
        let mut bad = valid.clone();
        *bad.pointer_mut(path).unwrap() = value;
        assert!(
            GridState::decode_snapshot(&serde_json::to_vec(&bad).unwrap()).is_err(),
            "{path}"
        );
    }
    let mut bad = valid.clone();
    let duplicate = bad["nodes"][0].clone();
    bad["nodes"].as_array_mut().unwrap().push(duplicate);
    assert!(GridState::decode_snapshot(&serde_json::to_vec(&bad).unwrap()).is_err());
    assert!(
        GridState::decode_snapshot(&GridState::default().encode_snapshot().unwrap())
            .unwrap()
            .is_empty()
    );
}
#[test]
fn replay_scales_viewport_inches_once() {
    let mut root_only = GridState::default();
    root_only.ensure((192, 96));
    let before_root = current_inches(&root_only);
    root_only.scale_snapshot((192, 96), (384, 192)).unwrap();
    let after_root = current_inches(&root_only);
    for i in 0..4 {
        assert!(
            (after_root[i] - before_root[i] * 2.).abs() < 1e-9,
            "root {i}: {before_root:?} -> {after_root:?}"
        );
    }
    let mut state = GridState::default();
    state.ensure((192, 96));
    let parent = state.frames[0].clone();
    let anchor_x = 96.;
    let anchor_y = 48.;
    let mut child = parent.clone();
    child.width = 96.;
    child.height = 24.;
    child.anchor_x = anchor_x;
    child.anchor_y = anchor_y;
    child.matrix = compose(parent.matrix, [1., 0., 0., 1., anchor_x, anchor_y]);
    state.frames.push(child);
    let before = current_inches(&state);
    assert!(
        (before[0] - 1.).abs() < 1e-9 && (before[1] - 0.5).abs() < 1e-9,
        "{before:?}"
    );
    assert!(
        (before[2] - 1.).abs() < 1e-9 && (before[3] - 0.25).abs() < 1e-9,
        "{before:?}"
    );
    state.scale_snapshot((192, 96), (384, 192)).unwrap();
    let after = current_inches(&state);
    for (i, expected) in [2., 1., 2., 0.5].into_iter().enumerate() {
        assert!((after[i] - expected).abs() < 1e-9, "child {i}: {after:?}");
    }
}
