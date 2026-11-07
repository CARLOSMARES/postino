//! The color of an environment: the dot in the title bar's environment pill and menu, the
//! Environments panel, the environment editor and the command palette (see
//! `docs/design-system.md`).
//!
//! Each environment is drawn in one of nine named colors. The user may choose one (#217), saved
//! by name in `.postino/project.toml`; otherwise it is derived from the name ([`auto_color`]).
//! "No environment" has no color: the views draw a hollow ring for it instead.

use std::collections::HashMap;

use postino_workspace::Workspace;

/// One of the colors an environment can be drawn in. Each has a light and a dark value in
/// `theme::palette` (`PostinoColors::env_color`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EnvColor {
    /// The `danger` token. Automatic for names with "prod".
    Red,
    /// A color of Postino's own.
    Orange,
    /// The `warning` token. Automatic for names with "stag", "test", "qa" or "uat".
    Amber,
    /// The `success` token. Automatic for every other name.
    Green,
    /// A color of Postino's own.
    Teal,
    /// The `info` token.
    Blue,
    /// The PATCH method's color.
    Violet,
    /// A color of Postino's own.
    Pink,
    /// A color of Postino's own.
    Grey,
}

impl EnvColor {
    /// Every color, in the order the color picker shows them.
    pub const ALL: [EnvColor; 9] = [
        EnvColor::Red,
        EnvColor::Orange,
        EnvColor::Amber,
        EnvColor::Green,
        EnvColor::Teal,
        EnvColor::Blue,
        EnvColor::Violet,
        EnvColor::Pink,
        EnvColor::Grey,
    ];

    /// The name written in `.postino/project.toml`.
    pub fn key(self) -> &'static str {
        match self {
            EnvColor::Red => "red",
            EnvColor::Orange => "orange",
            EnvColor::Amber => "amber",
            EnvColor::Green => "green",
            EnvColor::Teal => "teal",
            EnvColor::Blue => "blue",
            EnvColor::Violet => "violet",
            EnvColor::Pink => "pink",
            EnvColor::Grey => "grey",
        }
    }

    /// The color named `key` in the file, ignoring ASCII case. `None` for an unknown name, which
    /// leaves the environment on its automatic color.
    pub fn from_key(key: &str) -> Option<EnvColor> {
        EnvColor::ALL
            .into_iter()
            .find(|color| color.key().eq_ignore_ascii_case(key.trim()))
    }

    /// The translation key of the color's name, shown in the picker's tooltips.
    pub fn label_key(self) -> &'static str {
        match self {
            EnvColor::Red => "environments.color.red",
            EnvColor::Orange => "environments.color.orange",
            EnvColor::Amber => "environments.color.amber",
            EnvColor::Green => "environments.color.green",
            EnvColor::Teal => "environments.color.teal",
            EnvColor::Blue => "environments.color.blue",
            EnvColor::Violet => "environments.color.violet",
            EnvColor::Pink => "environments.color.pink",
            EnvColor::Grey => "environments.color.grey",
        }
    }
}

/// The name fragments, checked case-insensitively, that make an environment amber.
const WARNING_NEEDLES: [&str; 4] = ["stag", "test", "qa", "uat"];

/// The automatic color of an environment, from its name, case-insensitive: red for "prod",
/// amber for "stag", "test", "qa" or "uat", green for anything else.
pub fn auto_color(name: &str) -> EnvColor {
    let lower = name.to_ascii_lowercase();
    if lower.contains("prod") {
        EnvColor::Red
    } else if WARNING_NEEDLES.iter().any(|needle| lower.contains(needle)) {
        EnvColor::Amber
    } else {
        EnvColor::Green
    }
}

/// The colors the user chose, by environment name, as read from `.postino/project.toml`.
/// Environments without an entry use [`auto_color`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EnvColors {
    chosen: HashMap<String, EnvColor>,
}

impl EnvColors {
    /// Reads the chosen colors of `names` from the workspace configuration. A file that cannot
    /// be read, or a color name Postino does not know, leaves the automatic colors. File IO:
    /// call it when the environments are loaded, never from render.
    pub fn load(workspace: &Workspace, names: &[String]) -> Self {
        let config = match workspace.load_project() {
            Ok(config) => config,
            Err(error) => {
                log::warn!("ignoring the environment colors: {error}");
                return Self::default();
            }
        };
        let chosen = names
            .iter()
            .filter_map(|name| {
                let key = config.environment_color(name)?;
                match EnvColor::from_key(key) {
                    Some(color) => Some((name.clone(), color)),
                    None => {
                        log::warn!("unknown color {key:?} for environment {name:?}, ignoring it");
                        None
                    }
                }
            })
            .collect();
        EnvColors { chosen }
    }

    /// The color `name` is drawn in: the chosen one, or its automatic color.
    pub fn color(&self, name: &str) -> EnvColor {
        self.chosen(name).unwrap_or_else(|| auto_color(name))
    }

    /// The color the user chose for `name`, `None` when it is automatic.
    pub fn chosen(&self, name: &str) -> Option<EnvColor> {
        self.chosen.get(name).copied()
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use postino_workspace::ProjectWrite;
    use pretty_assertions::assert_eq;

    #[test]
    fn production_is_red() {
        assert_eq!(auto_color("production"), EnvColor::Red);
        assert_eq!(auto_color("PROD"), EnvColor::Red);
    }

    #[test]
    fn staging_test_qa_and_uat_are_amber() {
        assert_eq!(auto_color("staging"), EnvColor::Amber);
        assert_eq!(auto_color("Test"), EnvColor::Amber);
        assert_eq!(auto_color("qa-env"), EnvColor::Amber);
        assert_eq!(auto_color("UAT"), EnvColor::Amber);
    }

    #[test]
    fn anything_else_is_green() {
        assert_eq!(auto_color("local"), EnvColor::Green);
        assert_eq!(auto_color("dev"), EnvColor::Green);
    }

    #[test]
    fn prod_wins_over_warning_needles_when_both_are_present() {
        assert_eq!(auto_color("staging-prod-mirror"), EnvColor::Red);
    }

    #[test]
    fn keys_round_trip_and_ignore_case() {
        for color in EnvColor::ALL {
            assert_eq!(EnvColor::from_key(color.key()), Some(color));
        }
        assert_eq!(EnvColor::from_key(" Teal "), Some(EnvColor::Teal));
        assert_eq!(EnvColor::from_key("gray"), None);
    }

    #[test]
    fn chosen_colors_come_from_the_project_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let workspace = Workspace::open(dir.path()).expect("open");
        let names = vec!["dev".to_string(), "prod".to_string(), "qa".to_string()];

        let colors = EnvColors::load(&workspace, &names);
        assert_eq!(colors.color("prod"), EnvColor::Red);
        assert_eq!(colors.chosen("prod"), None);

        workspace
            .update_project(ProjectWrite::Create, |config| {
                config.set_environment_color("dev", Some("teal"));
                config.set_environment_color("qa", Some("ultraviolet"));
            })
            .expect("update");
        let colors = EnvColors::load(&workspace, &names);
        assert_eq!(colors.color("dev"), EnvColor::Teal);
        assert_eq!(colors.chosen("dev"), Some(EnvColor::Teal));
        // An unknown color name keeps the automatic one.
        assert_eq!(colors.color("qa"), EnvColor::Amber);
        assert_eq!(colors.chosen("qa"), None);
    }
}
