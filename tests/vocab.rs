//! Unit tests for `src/vocab.rs` (compiled as that module's `tests` child, so private items are reachable).
use super::*;

#[test]
fn learns_word_swaps() {
    assert_eq!(
        word_fixes("I met Mark League today.", "I met Mark Lee today."),
        vec![("League".into(), "Lee".into())]
    );
    assert_eq!(
        word_fixes("我跟李力凱開會", "我跟李立凱開會"),
        vec![("李力凱".into(), "李立凱".into())]
    );
    assert_eq!(word_fixes("ok，好", "ok。好"), vec![]); // punctuation only
    assert_eq!(word_fixes("a b c", "a b c d e"), vec![]); // pure insertion (dictated text)
    assert!(
        word_fixes(
            "one two three four five six seven",
            "totally different words that replace everything here"
        )
        .is_empty()
    );
}
#[test]
fn applies_whole_words_case_insensitive() {
    let v = Vocab {
        entries: vec![
            Entry {
                heard: "league".into(),
                write: "Lee".into(),
                ..Default::default()
            },
            Entry {
                heard: "李力凱".into(),
                write: "李立凱".into(),
                ..Default::default()
            },
        ],
    };
    assert_eq!(
        v.apply("Mark League and the leagues, 李力凱說"),
        "Mark Lee and the leagues, 李立凱說"
    );
    let mut v = v;
    v.learn("Lee", "league"); // reverting removes the old pair instead of looping
    assert_eq!(
        v.entries
            .iter()
            .filter(|e| e.heard == "league" || e.heard == "Lee")
            .count(),
        1
    );
    assert!(v.prompt().contains("李立凱 (not 李力凱)"));
}
#[test]
fn learned_swaps_wait_for_confirmation() {
    let mut v = Vocab::default();
    assert_eq!(v.learn("go to it", "go do it")[0].seen, 1);
    assert_eq!(
        v.learn("to me", "do me")[0].seen,
        2,
        "same edit is counted, not duplicated"
    );
    assert_eq!(v.apply("to"), "to", "pending swaps are not applied");
    assert_eq!(v.prompt(), "", "nor given to the LLM");
    v.confirm("to", "do");
    assert_eq!(v.apply("to"), "do");
    assert!(
        v.learn("to me", "do me").is_empty(),
        "already confirmed: nothing to decide"
    );
}
