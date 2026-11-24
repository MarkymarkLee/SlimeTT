//! Unit tests for `src/text.rs` (compiled as that module's `tests` child, so private items are reachable).
use super::*;

#[test]
fn opencc_taiwan_vocab() {
    assert_eq!(finalize("这个软件的性能", "s2twp"), "這個軟體的效能");
    assert_eq!(finalize("hello", "s2twp"), "hello");
    assert_eq!(finalize("软件", "off"), "软件");
    assert_eq!(finalize("不太好， 太高了。 ok", "s2twp"), "不太好，太高了。ok");
}
#[test]
fn commands() {
    assert_eq!(instant_command("Undo."), Some(Instant::Undo));
    assert_eq!(instant_command("清除"), Some(Instant::Clear));
    assert_eq!(instant_command("please undo that"), None);
}

#[test]
fn inserts_with_spacing() {
    let mut b = "hello world".to_string();
    assert_eq!(insert_at(&mut b, 5, "big"), 9);
    assert_eq!(b, "hello big world");
    let mut c = "你好世界".to_string();
    assert_eq!(insert_at(&mut c, 2, "大"), 3);
    assert_eq!(c, "你好大世界");
    let mut d = "hello ".to_string(); // caret after the popup's trailing space
    assert_eq!(insert_at(&mut d, 6, "big"), 9);
    assert_eq!(d, "hello big");
    let mut e = String::new();
    assert_eq!(insert_at(&mut e, 0, "hi"), 2);
    assert_eq!(e, "hi");
}

#[test]
fn joins() {
    let v: Vec<String> = ["今天很好", "我們去 team meeting", "ok", "\n", "下一段"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    assert_eq!(join(&v), "今天很好我們去 team meeting ok\n下一段");
}
