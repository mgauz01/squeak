use squeak::hotkeys::gestures::{GestureFsm, GestureOutput};

#[test]
fn ptt_hold_release_sequence() {
    let mut fsm = GestureFsm::default();
    let out = fsm.simulate_hold(0, 350);
    assert_eq!(
        out,
        vec![
            GestureOutput::StartPushToTalk,
            GestureOutput::StopPushToTalk
        ]
    );
}

#[test]
fn double_tap_hands_free_toggle() {
    let mut fsm = GestureFsm::default();
    let _ = fsm.simulate_tap(0);
    let start = fsm.simulate_tap(100);
    assert!(start.contains(&GestureOutput::StartHandsFree));

    let _ = fsm.simulate_tap(200);
    let stop = fsm.simulate_tap(300);
    assert!(stop.contains(&GestureOutput::StopHandsFree));
}
