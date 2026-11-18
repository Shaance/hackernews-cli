//! Settings dialog behavior and configuration writes.

use super::super::*;
use super::super::scenes::scene_settings;
use super::super::scenes::scene_story;
use super::super::window::view;
use super::support::*;

#[test]
fn regenerate_is_only_available_with_an_open_story() {
    for has_story in [false, true] {
        let mut s = app(true, SavedView::in_memory());
        if has_story { scene_story(&mut s); }
        scene_settings(&mut s);
        let mut rt = rune::Runtime::new(s, view, (1180.0, 760.0), rune::Env::new(true, 1.0));
        rt.frame(0.0);
        assert_eq!(rt.elements().iter().any(|e| e.label.as_deref() == Some("Regenerate summary")), has_story);
    }
}

#[test]
fn quality_tiles_select_and_expose_radio_state() {
    let mut s = app(true, SavedView::in_memory());
    scene_settings(&mut s);
    let mut rt = rune::Runtime::new(s, view, (1180.0, 760.0), rune::Env::new(true, 1.0));
    rt.frame(0.0);
    let [x, y, w, h] = rt.elements().iter().find(|e| e.key.as_deref() == Some("thorough")).unwrap().rect;
    rt.pointer_down(x + w / 2.0, y + h / 2.0);
    rt.pointer_up(x + w / 2.0, y + h / 2.0);
    rt.frame(0.0);
    assert_eq!(rt.state.settings.as_ref().unwrap().profile(), reader::Profile::Thorough);
    let elements = rt.elements();
    assert!(elements.iter().any(|e| e.role == "radiogroup" && e.label.as_deref() == Some("Summary quality")));
    assert!(elements.iter().any(|e| e.key.as_deref() == Some("thorough") && e.role == "radio" && e.item_selected));
    assert!(elements.iter().any(|e| e.key.as_deref() == Some("fast") && e.role == "radio" && !e.item_selected));

    // Keep Thorough focused but unchecked, so each key must actually select it.
    for key in [rune::Key::Enter, rune::Key::Text(" ".into())] {
        rt.update(|s, _| s.settings_mut().unwrap().set_profile(reader::Profile::Fast));
        rt.frame(0.0);
        rt.key(rune::KeyEvent { key, mods: Default::default() });
        rt.frame(0.0);
        assert_eq!(rt.state.settings.as_ref().unwrap().profile(), reader::Profile::Thorough);
    }
}

#[test]
fn apply_settings_writes_the_config_and_closes_the_dialog() {
    let dir = temp_dir("apply");
    let path = dir.join("hn/summary.json");
    let mut s = app(true, SavedView::in_memory());
    let mut settings = Settings::open(&s.config, Some(path.clone()), false);
    settings.set_key("sk-or-v1-not-a-real-key");
    settings.set_profile(reader::Profile::Thorough);
    s.settings = Some(settings);

    s.apply_settings(false);

    assert!(s.settings.is_none(), "Apply closes the dialog");
    assert_eq!(s.config.api_key, "sk-or-v1-not-a-real-key");
    assert_eq!(s.config.profile, reader::Profile::Thorough);
    assert_eq!(
        s.toast.as_ref().map(|notice| notice.message.as_str()),
        Some("Settings saved"),
        "the save is confirmed in the toast region"
    );
    // The file is the one the terminal app reads: same writer, same path.
    let saved = reader::load_config(&path).expect("the file Apply wrote");
    assert_eq!(saved.api_key, "sk-or-v1-not-a-real-key");
    assert_eq!(saved.profile, reader::Profile::Thorough);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_cancelled_dialog_writes_nothing() {
    let dir = temp_dir("cancel");
    let path = dir.join("hn/summary.json");
    let mut s = app(true, SavedView::in_memory());
    let mut settings = Settings::open(&s.config, Some(path.clone()), false);
    settings.set_key("sk-or-v1-not-a-real-key");
    s.settings = Some(settings);

    s.close_settings();

    assert!(s.settings.is_none());
    assert!(s.config.api_key.is_empty(), "the draft was dropped");
    assert!(!path.exists(), "Cancel writes no file");
    let _ = std::fs::remove_dir_all(&dir);
}

