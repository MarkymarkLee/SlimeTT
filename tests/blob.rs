//! Unit tests for `src/blob.rs` (compiled as that module's `tests` child, so private items are reachable).

#[test]
fn svg_renders_with_eye_cut_out() {
    let img = super::render(100);
    let alpha = |x: usize, y: usize| img[(y * 100 + x) * 4 + 3];
    assert_eq!(alpha(40, 56), 0, "eye is a hole");
    assert_eq!(alpha(50, 75), 255, "body is solid");
    assert_eq!(alpha(50, 21), 255, "headband drawn");
    assert_eq!(alpha(0, 0), 0, "background transparent");
}
#[test]
fn tray_icon_builds() {
    assert!(super::tray_icon().is_some());
}
