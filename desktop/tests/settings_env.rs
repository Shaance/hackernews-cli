//! The settings dialog's round trip, against a temp `XDG_CONFIG_HOME`.
//!
//! One test in its own binary: `std::env::set_var` is process-global, so a second test in the
//! same process could see the temp root, or race the write. `Config::path()` reads
//! `XDG_CONFIG_HOME` first, which is what makes the isolation possible without touching the
//! reader's own `~/.config/hn/summary.json`.

use hn_desktop::settings::Settings;
use hn_lib::reader::{self, Config, Profile, Provider};

#[test]
fn apply_writes_the_config_the_terminal_app_reads_back() {
    let root = std::env::temp_dir().join(format!("hn-desktop-settings-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    // SAFETY: this binary holds exactly one test, so no other thread reads the environment.
    unsafe {
        std::env::set_var("XDG_CONFIG_HOME", &root);
        // An environment key would be the fallback for an empty saved one, which would make
        // the assertions below depend on the machine.
        std::env::remove_var("OPENROUTER_API_KEY");
        std::env::remove_var("ANTHROPIC_API_KEY");
    }

    let path = Config::path().expect("XDG_CONFIG_HOME is set");
    assert_eq!(path, root.join("hn/summary.json"), "the reader's own path");

    // The dialog opens on the reader's config and writes only on Apply.
    let mut settings = Settings::open(&Config::default(), Some(path.clone()), false);
    assert!(!path.exists(), "opening the dialog writes nothing");
    settings.set_key("sk-or-v1-roundtrip");
    settings.set_profile(Profile::Thorough);
    settings.toggle_automatic();
    let applied = settings.apply().expect("the temp directory is writable");
    assert_eq!(applied.provider, Provider::OpenRouter);
    assert_eq!(applied.profile, Profile::Thorough);
    assert!(!applied.automatic, "the switch was turned off");

    // The file is the reader's, and the terminal app reads it back unchanged.
    let reloaded = reader::load_config(&path).expect("the file it just wrote");
    assert_eq!(reloaded.api_key, "sk-or-v1-roundtrip");
    assert_eq!(reloaded.profile, Profile::Thorough);
    assert!(!reloaded.automatic);
    assert_eq!(reloaded.key(), "sk-or-v1-roundtrip");
    assert_eq!(reloaded.provider, Provider::OpenRouter);

    // A second dialog opens on what was saved, and switching provider swaps the field: each
    // provider has its own key, and the Anthropic one is still empty.
    let mut again = Settings::open(&reloaded, Some(path.clone()), false);
    assert_eq!(again.provider(), Provider::OpenRouter);
    assert_eq!(again.profile().model(again.provider()), "anthropic/claude-sonnet-5.5");
    again.set_provider(Provider::Anthropic);
    assert_eq!(again.profile().model(again.provider()), "claude-sonnet-5-5");
    again.set_key("sk-ant-roundtrip");
    again.apply().expect("the temp directory is writable");
    let both = reader::load_config(&path).expect("the file it just wrote");
    assert_eq!(both.api_key, "sk-or-v1-roundtrip", "the other key is kept");
    assert_eq!(both.anthropic_api_key, "sk-ant-roundtrip");
    assert_eq!(both.provider, Provider::Anthropic);

    // The auth path: the dialog says so, and the field is what the reader came to change.
    let reenter = Settings::open(&both, Some(path.clone()), true);
    assert!(reenter.reenter());

    // A dialog with nowhere to write reports it instead of pretending to save.
    let mut nowhere = Settings::open(&both, None, false);
    nowhere.set_key("sk-or-v1-nowhere");
    assert!(nowhere.apply().is_err(), "no path is a failure, not a silent success");

    let _ = std::fs::remove_dir_all(&root);
}
