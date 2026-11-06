//! What the settings say of a notice and of the small preview: the corner of the screen they appear
//! in, the sound they come with, and how long a notice stays. The windows that show them are not
//! here; this is the part that is chosen and written down.

/// A corner of the primary monitor's work area: where a notice appears, and where the small preview
/// does. In the same corner, the notice keeps clear of the preview while there is one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Corner {
    #[default]
    BottomRight,
    BottomLeft,
    TopRight,
    TopLeft,
}

impl Corner {
    pub const ALL: [Corner; 4] = [
        Corner::BottomRight,
        Corner::BottomLeft,
        Corner::TopRight,
        Corner::TopLeft,
    ];

    /// The name used in settings, language files and between the windows and the application.
    pub const fn name(self) -> &'static str {
        match self {
            Corner::BottomRight => "bottomRight",
            Corner::BottomLeft => "bottomLeft",
            Corner::TopRight => "topRight",
            Corner::TopLeft => "topLeft",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|corner| corner.name() == name)
    }

    pub const fn is_right(self) -> bool {
        matches!(self, Corner::BottomRight | Corner::TopRight)
    }

    pub const fn is_bottom(self) -> bool {
        matches!(self, Corner::BottomRight | Corner::BottomLeft)
    }
}

/// The sound a notice comes with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Sound {
    None,
    /// One of the sounds the system comes with, by its name.
    System(String),
    /// A file of the user's own (which one is a setting of its own: it is remembered while
    /// another sound is chosen).
    Custom,
}

/// The system's sound for a notification, on the Windows of today.
const DEFAULT_SOUND: &str = "Windows Notify System Generic";

impl Default for Sound {
    fn default() -> Self {
        Sound::System(DEFAULT_SOUND.to_owned())
    }
}

impl Sound {
    /// As the window is told it: a sound of the system's that this system does not have (or that
    /// is not one to choose from) is no sound, which is also what is heard of it.
    pub fn shown(&self) -> String {
        match self {
            Sound::System(name) if crate::platform::system_sound(name).is_none() => {
                Sound::None.name()
            }
            sound => sound.name(),
        }
    }

    /// The name used in settings and between the windows and the application: `none`, `custom`,
    /// or `system:` and the name of one of the system's sounds.
    pub fn name(&self) -> String {
        match self {
            Sound::None => "none".to_owned(),
            Sound::Custom => "custom".to_owned(),
            Sound::System(name) => format!("system:{name}"),
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "none" => Some(Sound::None),
            "custom" => Some(Sound::Custom),
            _ => name
                .strip_prefix("system:")
                .filter(|name| !name.is_empty())
                .map(|name| Sound::System(name.to_owned())),
        }
    }
}

/// How long a notice that says how something ended is on screen, in seconds, until the user
/// chooses otherwise; and the longest they can choose. (With 0, it stays until it is clicked.)
pub const DEFAULT_SECONDS: f64 = 7.0;
pub const MOST_SECONDS: f64 = 3600.0;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sound_is_named_the_way_the_settings_write_it() {
        let ding = Sound::System("ding".into());
        for sound in [Sound::None, Sound::Custom, ding.clone(), Sound::default()] {
            assert_eq!(Sound::from_name(&sound.name()), Some(sound));
        }
        assert_eq!(ding.name(), "system:ding");
        assert_eq!(Sound::from_name("system:"), None);
        assert_eq!(Sound::from_name("loud"), None);
        // A sound the system does not have is shown as none.
        assert_eq!(Sound::System("no such sound".into()).shown(), "none");
        assert_eq!(Sound::Custom.shown(), "custom");
    }

    #[test]
    fn a_corner_is_named_the_way_the_settings_write_it() {
        for corner in Corner::ALL {
            assert_eq!(Corner::from_name(corner.name()), Some(corner));
        }
        assert_eq!(Corner::from_name("middle"), None);
        assert_eq!(Corner::default(), Corner::BottomRight);
        assert!(Corner::TopRight.is_right() && !Corner::TopRight.is_bottom());
        assert!(Corner::BottomLeft.is_bottom() && !Corner::BottomLeft.is_right());
    }
}
