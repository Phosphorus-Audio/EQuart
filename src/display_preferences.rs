use crate::analyzer::{
    AnalyzerOption, DEFAULT_HOLD, DEFAULT_RESOLUTION, DEFAULT_SPECTRUM_SMOOTHING,
    DEFAULT_SPECTRUM_SPEED, DEFAULT_TILT,
};
use serde::{Deserialize, Serialize};
use std::{
    env, fs,
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct DisplayPreferences {
    pub(crate) spectrum: SpectrumPreferences,
    pub(crate) layout: LayoutPreferences,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct SpectrumPreferences {
    pub(crate) resolution: u32,
    pub(crate) speed: u32,
    pub(crate) smoothing: u32,
    pub(crate) tilt: u32,
    pub(crate) peak_hold: u32,
}

impl Default for SpectrumPreferences {
    fn default() -> Self {
        Self {
            resolution: DEFAULT_RESOLUTION,
            speed: DEFAULT_SPECTRUM_SPEED,
            smoothing: DEFAULT_SPECTRUM_SMOOTHING,
            tilt: DEFAULT_TILT,
            peak_hold: DEFAULT_HOLD,
        }
    }
}

impl SpectrumPreferences {
    pub(crate) fn index(self, option: AnalyzerOption) -> u32 {
        option.index(match option {
            AnalyzerOption::Resolution => self.resolution,
            AnalyzerOption::Speed => self.speed,
            AnalyzerOption::Smoothing => self.smoothing,
            AnalyzerOption::Tilt => self.tilt,
            AnalyzerOption::Hold => self.peak_hold,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct LayoutPreferences {
    pub(crate) meter_visible: bool,
    pub(crate) meter_fade: bool,
    pub(crate) piano_keys_visible: bool,
    pub(crate) piano_key_fade: bool,
    pub(crate) phase_curve: bool,
    pub(crate) phase_unwrapped: bool,
}

impl Default for LayoutPreferences {
    fn default() -> Self {
        Self {
            meter_visible: true,
            meter_fade: true,
            piano_keys_visible: false,
            piano_key_fade: true,
            phase_curve: false,
            phase_unwrapped: false,
        }
    }
}

#[derive(Default)]
pub(crate) struct DisplayStore {
    path: Option<PathBuf>,
    values: Mutex<DisplayPreferences>,
}

impl DisplayStore {
    pub(crate) fn shared() -> Arc<Self> {
        static STORE: OnceLock<Arc<DisplayStore>> = OnceLock::new();
        let store = STORE.get_or_init(|| {
            let path = user_config_dir()
                .map(|root| root.join(crate::VENDOR).join("EQuart").join("display.json"));
            if path.is_none() {
                nice_plug::nice_warn!("EQuart could not locate the user configuration directory");
            }

            Arc::new(Self {
                path,
                ..Self::default()
            })
        });

        if let Err(error) = store.reload() {
            nice_plug::nice_warn!("EQuart could not load display preferences: {error}");
        }

        Arc::clone(store)
    }

    #[cfg(test)]
    pub(crate) fn with_values(values: DisplayPreferences) -> Self {
        Self {
            path: None,
            values: Mutex::new(values),
        }
    }

    pub(crate) fn read(&self) -> DisplayPreferences {
        *self
            .values
            .lock()
            .expect("display preferences lock poisoned")
    }

    fn reload(&self) -> io::Result<()> {
        if let Some(path) = &self.path {
            let mut values = self
                .values
                .lock()
                .expect("display preferences lock poisoned");
            *values = read_file(path)?;
        }

        Ok(())
    }

    pub(crate) fn update(&self, change: impl FnOnce(&mut DisplayPreferences)) -> io::Result<()> {
        let mut values = self
            .values
            .lock()
            .expect("display preferences lock poisoned");
        let path = self.path.as_ref().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "user configuration directory is unavailable",
            )
        })?;
        let parent = path
            .parent()
            .expect("display preferences have a parent directory");

        // merge with the last save from any other host process
        fs::create_dir_all(parent)?;
        let lock = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(path.with_extension("lock"))?;
        lock.lock()?;

        let current = read_file(path)?;
        let mut next = current;
        change(&mut next);

        if next == current {
            *values = current;
            return Ok(());
        }

        let bytes = serde_json::to_vec_pretty(&next).map_err(io::Error::other)?;

        let temp = path.with_extension("tmp");
        let mut file = fs::File::create(&temp)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(temp, path)?;

        *values = next;
        Ok(())
    }
}

fn read_file(path: &Path) -> io::Result<DisplayPreferences> {
    match fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(&text).map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{}: {error}", path.display()),
            )
        }),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(DisplayPreferences::default()),
        Err(error) => Err(error),
    }
}

fn user_config_dir() -> Option<PathBuf> {
    if cfg!(target_os = "windows") {
        env::var_os("APPDATA").map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        env::home_dir().map(|home| home.join("Library").join("Application Support"))
    } else {
        env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .or_else(|| env::home_dir().map(|home| home.join(".config")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nice_plug::prelude::Params;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn display_preferences_survive_reload_and_merge_independent_saves() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = env::temp_dir().join(format!("equart-display-{}-{nonce}", std::process::id()));
        let path = dir.join("display.json");

        let first = DisplayStore {
            path: Some(path.clone()),
            ..DisplayStore::default()
        };
        let second = DisplayStore {
            path: Some(path.clone()),
            ..DisplayStore::default()
        };
        let expected = DisplayPreferences {
            spectrum: SpectrumPreferences {
                resolution: 0,
                speed: 0,
                smoothing: 0,
                tilt: 0,
                peak_hold: 0,
            },
            layout: LayoutPreferences {
                meter_visible: false,
                meter_fade: false,
                piano_keys_visible: true,
                piano_key_fade: false,
                phase_curve: true,
                phase_unwrapped: true,
            },
        };

        first.reload().unwrap();
        assert_eq!(first.read(), DisplayPreferences::default());

        std::thread::scope(|scope| {
            scope.spawn(|| {
                first
                    .update(|prefs| prefs.spectrum = expected.spectrum)
                    .unwrap()
            });
            scope.spawn(|| {
                second
                    .update(|prefs| prefs.layout = expected.layout)
                    .unwrap()
            });
        });

        first.reload().unwrap();
        second.reload().unwrap();
        assert_eq!(first.read(), expected);
        assert_eq!(second.read(), expected);
        assert_eq!(read_file(&path).unwrap(), expected);

        let state = crate::params::EquartParams::default().serialize_fields();
        for id in [
            "analyzer_resolution",
            "analyzer_speed",
            "analyzer_smoothing",
            "analyzer_tilt",
            "analyzer_hold",
            "meter_visible",
            "meter_fade",
            "piano_keys_visible",
            "piano_key_fade",
            "phase_curve",
            "phase_unwrapped",
        ] {
            assert!(
                !state.contains_key(id),
                "{id} belongs to the user preferences"
            );
        }

        fs::remove_file(&path).unwrap();
        fs::remove_file(path.with_extension("lock")).unwrap();
        fs::remove_dir(dir).unwrap();
    }
}
