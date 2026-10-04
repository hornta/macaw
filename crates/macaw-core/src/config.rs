//! User settings, stored as TOML. Every field has a default, so a partial file is fine.

use serde::Deserialize;

/// The commented settings file written on first run. Parsing it must give [`Config::default`].
pub const TEMPLATE: &str = include_str!("default_config.toml");

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub enabled: bool,
    pub apply_to: ApplyTo,
    pub keyboard_layout: PrintedLayout,
    pub fn_key: FnKey,
    pub mac_mode: MacMode,
    pub game_mode: GameMode,
    pub fixes: Fixes,
    pub brightness: Brightness,
    pub diagnostics: Diagnostics,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            enabled: true,
            apply_to: ApplyTo::Apple,
            keyboard_layout: PrintedLayout::Us,
            fn_key: FnKey::default(),
            mac_mode: MacMode::default(),
            game_mode: GameMode::default(),
            fixes: Fixes::default(),
            brightness: Brightness::default(),
            diagnostics: Diagnostics::default(),
        }
    }
}

impl Config {
    pub fn parse(text: &str) -> Result<Config, String> {
        let config: Config = toml::from_str(text).map_err(|e| e.to_string())?;
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<(), String> {
        if !(1..=100).contains(&self.brightness.step_percent) {
            return Err("brightness.step_percent must be between 1 and 100".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApplyTo {
    Apple,
    All,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrintedLayout {
    Us,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StandIn {
    CapsLock,
    RightCommand,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OptionKey {
    MacCharacters,
    #[serde(rename = "altgr")]
    AltGr,
    Alt,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FnKey {
    pub stand_in: StandIn,
    /// A tap of Caps Lock switches keyboard layout (like the globe key on a Mac); then a long
    /// press on its own toggles capitals instead.
    pub tap_switches_layout: bool,
    pub tap_toggles_caps_lock: bool,
    pub tap_timeout_ms: u64,
}

impl Default for FnKey {
    fn default() -> FnKey {
        FnKey {
            stand_in: StandIn::CapsLock,
            tap_switches_layout: false,
            tap_toggles_caps_lock: true,
            tap_timeout_ms: 400,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MacMode {
    pub enabled: bool,
    pub option_key: OptionKey,
    pub terminal_apps: Vec<String>,
    pub browser_apps: Vec<String>,
}

impl Default for MacMode {
    fn default() -> MacMode {
        MacMode {
            enabled: true,
            option_key: OptionKey::MacCharacters,
            terminal_apps: Vec::new(),
            browser_apps: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct GameMode {
    pub enabled: bool,
    pub always_off_in: Vec<String>,
    pub never_off_in: Vec<String>,
}

impl Default for GameMode {
    fn default() -> GameMode {
        GameMode {
            enabled: true,
            always_off_in: Vec::new(),
            never_off_in: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Fixes {
    pub stuck_keys: bool,
}

impl Default for Fixes {
    fn default() -> Fixes {
        Fixes { stuck_keys: true }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Brightness {
    pub step_percent: u8,
}

impl Default for Brightness {
    fn default() -> Brightness {
        Brightness { step_percent: 10 }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Diagnostics {
    pub log_key_events: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn template_matches_defaults() {
        assert_eq!(Config::parse(TEMPLATE).unwrap(), Config::default());
    }

    #[test]
    fn empty_file_gives_defaults() {
        assert_eq!(Config::parse("").unwrap(), Config::default());
    }

    #[test]
    fn partial_file_overrides_only_what_it_sets() {
        let c = Config::parse("apply_to = \"all\"\n[mac_mode]\noption_key = \"altgr\"\n").unwrap();
        assert_eq!(c.apply_to, ApplyTo::All);
        assert_eq!(c.mac_mode.option_key, OptionKey::AltGr);
        assert!(c.mac_mode.enabled);
        assert_eq!(c.fn_key, FnKey::default());
    }

    #[test]
    fn typos_are_reported() {
        let err = Config::parse("[mac_mode]\nenabeld = false\n").unwrap_err();
        assert!(err.contains("enabeld"), "{err}");
        assert!(Config::parse("apply_to = \"everything\"").is_err());
        assert!(Config::parse("[brightness]\nstep_percent = 0").is_err());
    }
}
