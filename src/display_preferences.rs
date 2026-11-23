use crate::analyzer::{
    AnalyzerOption, DEFAULT_HOLD, DEFAULT_RESOLUTION, DEFAULT_SPECTRUM_SMOOTHING,
    DEFAULT_SPECTRUM_SPEED, DEFAULT_TILT,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    env, fs,
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
};

pub(crate) mod theme;

pub(crate) const THEME_FOLDER: &str = "Themes";
pub(crate) const THEME_EXTENSION: &str = "equartheme";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct DisplayPreferences {
    pub(crate) spectrum: SpectrumPreferences,
    pub(crate) layout: LayoutPreferences,
    pub(crate) theme: theme::Theme,
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
    pub(crate) footer_border: bool,
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
            footer_border: false,
        }
    }
}

#[derive(Default)]
pub(crate) struct DisplayStore {
    path: Option<PathBuf>,
    values: Mutex<DisplayPreferences>,
    themes: Mutex<BTreeMap<String, theme::Theme>>,
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
            themes: Mutex::new(BTreeMap::new()),
        }
    }

    fn themes_dir(&self) -> Option<PathBuf> {
        self.path
            .as_ref()
            .and_then(|path| path.parent())
            .map(|parent| parent.join(THEME_FOLDER))
    }

    pub(crate) fn read(&self) -> DisplayPreferences {
        *self
            .values
            .lock()
            .expect("display preferences lock poisoned")
    }

    pub(crate) fn theme_presets(&self) -> BTreeMap<String, theme::Theme> {
        self.themes
            .lock()
            .expect("theme presets lock poisoned")
            .clone()
    }

    pub(crate) fn reload_theme_presets(&self) -> io::Result<()> {
        let Some(dir) = self.themes_dir() else {
            return Ok(());
        };

        let loaded = read_theme_dir(&dir)?;
        *self.themes.lock().expect("theme presets lock poisoned") = loaded;

        Ok(())
    }

    pub(crate) fn save_theme_preset(
        &self,
        name: &str,
        palette: theme::Theme,
        replace: bool,
    ) -> io::Result<()> {
        let name = name.trim();

        if !crate::presets::is_valid_name(name) || theme::presets::is_builtin_name(name) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Choose a valid custom preset name",
            ));
        }

        let dir = self.themes_dir().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "user configuration directory is unavailable",
            )
        })?;

        fs::create_dir_all(&dir)?;

        let path = theme_file_path(&dir, name);

        if !replace && path.exists() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "A theme preset already uses this name",
            ));
        }

        write_theme_file(&path, &palette)?;

        self.themes
            .lock()
            .expect("theme presets lock poisoned")
            .insert(name.to_owned(), palette);

        Ok(())
    }

    pub(crate) fn delete_theme_preset(&self, name: &str) -> io::Result<()> {
        let name = name.trim();

        if !crate::presets::is_valid_name(name) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Invalid theme preset name",
            ));
        }

        let dir = self.themes_dir().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "user configuration directory is unavailable",
            )
        })?;

        let path = theme_file_path(&dir, name);

        match fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }

        self.themes
            .lock()
            .expect("theme presets lock poisoned")
            .remove(name);

        Ok(())
    }

    pub(crate) fn reveal_theme_folder(&self) -> io::Result<()> {
        let dir = self.themes_dir().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "user configuration directory is unavailable",
            )
        })?;

        crate::presets::open_folder(&dir)
    }

    fn reload(&self) -> io::Result<()> {
        if let Some(path) = &self.path {
            let mut values = self
                .values
                .lock()
                .expect("display preferences lock poisoned");

            *values = match open_lock(path) {
                Ok(lock) => {
                    lock.lock_shared()?;
                    read_file(path)?
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    DisplayPreferences::default()
                }
                Err(error) => return Err(error),
            };
            drop(values);

            self.reload_theme_presets()?;
        }

        Ok(())
    }

    pub(crate) fn update(&self, change: impl FnOnce(&mut DisplayPreferences)) -> io::Result<()> {
        self.update_file(|prefs| {
            let previous = *prefs;
            change(prefs);
            Ok(previous != *prefs)
        })
    }

    fn update_file(
        &self,
        change: impl FnOnce(&mut DisplayPreferences) -> io::Result<bool>,
    ) -> io::Result<()> {
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
        let lock = open_lock(path)?;
        lock.lock()?;

        let mut next = read_file(path)?;
        let changed = change(&mut next);

        if matches!(changed, Ok(true)) {
            let bytes = serde_json::to_vec_pretty(&next).map_err(io::Error::other)?;
            let temp = path.with_extension("tmp");
            let mut file = fs::File::create(&temp)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
            drop(file);
            fs::rename(temp, path)?;
        }

        *values = next;
        changed.map(|_| ())
    }
}

fn theme_file_path(dir: &Path, name: &str) -> PathBuf {
    dir.join(format!("{name}.{THEME_EXTENSION}"))
}

fn write_theme_file(path: &Path, palette: &theme::Theme) -> io::Result<()> {
    static TEMP_SEQ: AtomicU64 = AtomicU64::new(0);

    let bytes = serde_json::to_vec_pretty(palette).map_err(io::Error::other)?;
    let seq = TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let stem = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("theme");
    let temp = path.with_file_name(format!("{stem}.{}.{seq}.tmp", std::process::id()));

    let mut file = fs::File::create(&temp)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    drop(file);

    if let Err(error) = fs::rename(&temp, path) {
        let _ = fs::remove_file(&temp);
        return Err(error);
    }

    Ok(())
}

fn read_theme_dir(dir: &Path) -> io::Result<BTreeMap<String, theme::Theme>> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
        Err(error) => return Err(error),
    };

    let mut presets = BTreeMap::new();

    for entry in entries.flatten() {
        let path = entry.path();

        if !path
            .extension()
            .is_some_and(|ext| ext == THEME_EXTENSION)
        {
            continue;
        }

        let Some(name) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };

        if theme::presets::is_builtin_name(name) {
            continue;
        }

        match fs::read_to_string(&path).and_then(|text| {
            serde_json::from_str::<theme::Theme>(&text).map_err(|error| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("{}: {error}", path.display()),
                )
            })
        }) {
            Ok(palette) => {
                presets.insert(name.to_owned(), palette);
            }
            Err(error) => {
                nice_plug::nice_warn!("EQuart could not load theme preset {name}: {error}");
            }
        }
    }

    Ok(presets)
}

fn open_lock(path: &Path) -> io::Result<fs::File> {
    fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path.with_extension("lock"))
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

pub(crate) fn user_config_dir() -> Option<PathBuf> {
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
    fn preferences_merge_concurrent_saves() {
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
        let mut theme = theme::Theme::default();
        theme.spectrum_fill.mode = theme::ColorMode::Solid;
        theme.spectrum_fill.solid = egui::Color32::LIGHT_BLUE;
        theme.band[31] = egui::Color32::GOLD;
        theme.region[5] = egui::Color32::LIGHT_RED;
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
                footer_border: true,
            },
            theme,
        };

        first.reload().unwrap();
        assert_eq!(first.read(), DisplayPreferences::default());
        assert!(!first.read().layout.footer_border);

        let partial: DisplayPreferences =
            serde_json::from_str(r#"{"layout":{"meter_visible":false}}"#).unwrap();
        assert_eq!(partial.theme, theme::Theme::default());
        assert!(!partial.layout.meter_visible);

        let original = first.read().theme;
        let mut fill_edit = original;
        fill_edit.spectrum_fill.mode = expected.theme.spectrum_fill.mode;
        let mut color_edit = expected.theme;
        color_edit.spectrum_fill.mode = original.spectrum_fill.mode;

        std::thread::scope(|scope| {
            scope.spawn(|| {
                first
                    .update(|prefs| {
                        prefs.spectrum = expected.spectrum;
                        prefs.theme.merge_edits(&original, &fill_edit);
                    })
                    .unwrap()
            });
            scope.spawn(|| {
                second
                    .update(|prefs| {
                        prefs.layout = expected.layout;
                        prefs.theme.merge_edits(&original, &color_edit);
                    })
                    .unwrap()
            });
            scope.spawn(|| first.save_theme_preset("测试配色", expected.theme, false).unwrap());
            scope.spawn(|| {
                let reader = DisplayStore {
                    path: Some(path.clone()),
                    ..DisplayStore::default()
                };

                for _ in 0..8 {
                    reader.reload().unwrap();
                }
            });
        });

        first.reload().unwrap();
        second.reload().unwrap();
        assert_eq!(first.read(), expected);
        assert_eq!(second.read(), expected);

        let saved = read_file(&path).unwrap();
        assert_eq!(saved, expected);

        let theme_file = dir.join(THEME_FOLDER).join(format!("测试配色.{THEME_EXTENSION}"));
        assert!(theme_file.exists());
        assert_eq!(second.theme_presets().get("测试配色"), Some(&expected.theme));

        let builtin = theme::presets::builtins()[1].1;
        assert_eq!(
            second.save_theme_preset("DEFAULT", builtin, true).unwrap_err().kind(),
            io::ErrorKind::InvalidInput,
        );
        assert_eq!(
            second.save_theme_preset("../escape", builtin, true).unwrap_err().kind(),
            io::ErrorKind::InvalidInput,
        );
        assert_eq!(
            second.save_theme_preset("测试配色", builtin, false).unwrap_err().kind(),
            io::ErrorKind::AlreadyExists,
        );

        second.save_theme_preset(" 测试配色 ", builtin, true).unwrap();
        first.reload().unwrap();
        assert_eq!(first.theme_presets()["测试配色"], builtin);
        assert_eq!(first.read(), expected);

        first.delete_theme_preset("测试配色").unwrap();
        assert!(!theme_file.exists());
        second.reload().unwrap();
        assert!(second.theme_presets().is_empty());
        assert_eq!(second.read().theme, expected.theme);

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
            "footer_border",
        ] {
            assert!(
                !state.contains_key(id),
                "{id} belongs to the user preferences"
            );
        }

        fs::remove_dir_all(dir).unwrap();
    }
}
