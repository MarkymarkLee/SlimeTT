//! Unit tests for `src/macos.rs` (compiled as that module's `tests` child, so private items are reachable).
use super::*;

#[test]
fn clipboard_writes_are_flagged_temporary() {
    let pb = objc2_app_kit::NSPasteboard::pasteboardWithUniqueName(); // not the user's clipboard
    let n = write_transient(&pb, "hello ");
    assert_eq!(pb.changeCount(), n);
    assert_eq!(
        pb.stringForType(unsafe { objc2_app_kit::NSPasteboardTypeString })
            .unwrap()
            .to_string(),
        "hello "
    );
    let types: Vec<String> = pb.types().unwrap().iter().map(|t| t.to_string()).collect();
    assert!(types.iter().any(|t| t == "org.nspasteboard.TransientType"), "{types:?}");
}
#[test]
fn hotkeys_parse() {
    assert_eq!(
        parse_hotkey("RightOption"),
        Some(Hotkey::ModifierTap {
            keycode: 61,
            flag: NSEventModifierFlags::Option
        })
    );
    assert_eq!(
        parse_hotkey("Ctrl+Alt+Space"),
        Some(Hotkey::Chord {
            mods: NSEventModifierFlags::Control | NSEventModifierFlags::Option,
            keycode: 49
        })
    );
    assert_eq!(
        parse_hotkey("Cmd+Shift+d"),
        Some(Hotkey::Chord {
            mods: NSEventModifierFlags::Command | NSEventModifierFlags::Shift,
            keycode: 2
        })
    );
    assert_eq!(parse_hotkey("nonsense"), None);
    assert_eq!(
        parse_hotkey("LeftCommand"),
        Some(Hotkey::ModifierTap {
            keycode: 55,
            flag: NSEventModifierFlags::Command
        })
    );
}
#[test]
fn recorded_names_round_trip() {
    let ca = NSEventModifierFlags::Control | NSEventModifierFlags::Option;
    assert_eq!(chord_name(49, ca).as_deref(), Some("Ctrl+Alt+Space"));
    assert_eq!(chord_name(2, NSEventModifierFlags::Command).as_deref(), Some("Cmd+D"));
    assert_eq!(chord_name(111, NSEventModifierFlags::empty()).as_deref(), Some("F12"));
    assert_eq!(chord_name(64, NSEventModifierFlags::empty()).as_deref(), Some("F17"));
    assert_eq!(chord_name(90, NSEventModifierFlags::empty()).as_deref(), Some("F20"));
    assert_eq!(
        chord_name(2, NSEventModifierFlags::empty()),
        None,
        "bare letter rejected"
    );
    for (code, mods) in [
        (49, ca),
        (2, NSEventModifierFlags::Command),
        (111, NSEventModifierFlags::empty()),
        (41, NSEventModifierFlags::Shift),
    ] {
        assert_eq!(
            parse_hotkey(&chord_name(code, mods).unwrap()),
            Some(Hotkey::Chord { mods, keycode: code })
        );
    }
}
