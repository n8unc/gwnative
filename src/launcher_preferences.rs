//! Immutable, nonsecret launch settings resolved before queue admission.
//!
//! Credentials deliberately cannot appear in these types.  A queued child owns
//! one `ResolvedLaunchOptions` value, so later Account edits cannot affect it.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WindowMode {
    #[default]
    Windowed,
    Fullscreen,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FrameRateLimit {
    Default,
    Limit(u16),
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchFrame {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// Stored with Account metadata.  Optional scalar fields distinguish an
/// inherited value from a deliberate false/default value.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountLaunchPreferences {
    #[serde(default)]
    pub muted: Option<bool>,
    #[serde(default)]
    pub frame_rate_limit: Option<FrameRateLimit>,
    #[serde(default)]
    pub window_mode: Option<WindowMode>,
    #[serde(default)]
    pub preferred_character: Option<String>,
    #[serde(default)]
    pub texture_pack_ids: Vec<String>,
}

/// Per-request choices. Nested options allow a caller to explicitly clear
/// optional Account values without treating absence as a clear request.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct InvocationOverrides {
    pub muted: Option<bool>,
    pub frame_rate_limit: Option<FrameRateLimit>,
    pub window_mode: Option<WindowMode>,
    pub window_frame: Option<Option<LaunchFrame>>,
    pub preferred_character: Option<Option<String>>,
    pub texture_pack_ids: Option<Vec<String>>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LaunchDefaults {
    pub muted: bool,
    pub frame_rate_limit: FrameRateLimit,
    pub window_mode: WindowMode,
    pub window_frame: Option<LaunchFrame>,
}

impl Default for LaunchDefaults {
    fn default() -> Self {
        Self {
            muted: false,
            frame_rate_limit: FrameRateLimit::Default,
            window_mode: WindowMode::Windowed,
            window_frame: None,
        }
    }
}

/// Fully resolved child snapshot. Serializable only for process environment
/// transport; it contains no email, password, profile secret, or credential.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedLaunchOptions {
    pub muted: bool,
    pub frame_rate_limit: FrameRateLimit,
    pub window_mode: WindowMode,
    pub window_frame: Option<LaunchFrame>,
    pub preferred_character: Option<String>,
    pub texture_pack_ids: Vec<String>,
}

pub fn resolve_launch_options(
    defaults: &LaunchDefaults,
    preferences: &AccountLaunchPreferences,
    overrides: &InvocationOverrides,
) -> ResolvedLaunchOptions {
    ResolvedLaunchOptions {
        muted: overrides
            .muted
            .or(preferences.muted)
            .unwrap_or(defaults.muted),
        frame_rate_limit: overrides
            .frame_rate_limit
            .or(preferences.frame_rate_limit)
            .unwrap_or(defaults.frame_rate_limit),
        window_mode: overrides
            .window_mode
            .or(preferences.window_mode)
            .unwrap_or(defaults.window_mode),
        window_frame: overrides.window_frame.unwrap_or(defaults.window_frame),
        preferred_character: overrides
            .preferred_character
            .clone()
            .unwrap_or_else(|| preferences.preferred_character.clone()),
        texture_pack_ids: overrides
            .texture_pack_ids
            .clone()
            .unwrap_or_else(|| preferences.texture_pack_ids.clone()),
    }
}

/// Validate player-provided settings before any Account or Keychain mutation.
pub fn validate_account_launch_preferences(
    preferences: &AccountLaunchPreferences,
) -> Result<(), String> {
    if let Some(FrameRateLimit::Limit(limit)) = preferences.frame_rate_limit
        && !(1..=1000).contains(&limit)
    {
        return Err("frame-rate limit must be between 1 and 1000".into());
    }
    if preferences
        .preferred_character
        .as_deref()
        .is_some_and(|value| {
            value.is_empty() || value.len() > 128 || value.chars().any(char::is_control)
        })
    {
        return Err("preferred character must be 1-128 non-control characters".into());
    }
    if preferences.texture_pack_ids.len() > 64 {
        return Err("an Account can enable at most 64 texture packs".into());
    }
    if preferences
        .texture_pack_ids
        .iter()
        .any(|id| id.is_empty() || id.len() > 128 || id.chars().any(char::is_control))
    {
        return Err("texture pack IDs must be 1-128 non-control characters".into());
    }
    if preferences
        .texture_pack_ids
        .iter()
        .enumerate()
        .any(|(index, id)| {
            preferences.texture_pack_ids[..index]
                .iter()
                .any(|earlier| earlier == id)
        })
    {
        return Err("texture pack IDs must not repeat".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_false_and_default_win_over_account_values() {
        let preferences = AccountLaunchPreferences {
            muted: Some(true),
            frame_rate_limit: Some(FrameRateLimit::Limit(90)),
            ..Default::default()
        };
        let resolved = resolve_launch_options(
            &LaunchDefaults::default(),
            &preferences,
            &InvocationOverrides {
                muted: Some(false),
                frame_rate_limit: Some(FrameRateLimit::Default),
                ..Default::default()
            },
        );
        assert!(!resolved.muted);
        assert_eq!(resolved.frame_rate_limit, FrameRateLimit::Default);
    }

    #[test]
    fn explicit_character_clear_differs_from_absent_override() {
        let preferences = AccountLaunchPreferences {
            preferred_character: Some("Koss".into()),
            ..Default::default()
        };
        assert_eq!(
            resolve_launch_options(
                &LaunchDefaults::default(),
                &preferences,
                &Default::default()
            )
            .preferred_character,
            Some("Koss".into())
        );
        assert_eq!(
            resolve_launch_options(
                &LaunchDefaults::default(),
                &preferences,
                &InvocationOverrides {
                    preferred_character: Some(None),
                    ..Default::default()
                },
            )
            .preferred_character,
            None
        );
    }

    #[test]
    fn rejects_unusable_player_values_before_persistence() {
        assert!(
            validate_account_launch_preferences(&AccountLaunchPreferences {
                frame_rate_limit: Some(FrameRateLimit::Limit(0)),
                ..Default::default()
            })
            .is_err()
        );
        assert!(
            validate_account_launch_preferences(&AccountLaunchPreferences {
                texture_pack_ids: vec!["same".into(), "same".into()],
                ..Default::default()
            })
            .is_err()
        );
    }
}
