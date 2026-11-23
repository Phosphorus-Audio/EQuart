use crate::params::EquartParams;
use nice_plug::params::InternalParamMut;
use nice_plug::plugin::{ParamValue, PluginState};
use nice_plug::prelude::*;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap},
    fs, io,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
};

pub(crate) const EXTENSION: &str = "equart";
pub(crate) const FOLDER: &str = "Presets";
pub(crate) const FACTORY: &str = "Factory";
pub(crate) const USER: &str = "User";
pub(crate) const DEFAULT: &str = "Factory/Default";

const FORMAT: u32 = 2;
const INDEX: &str = "presets.json";

/// persisted fields that belong to sound rather than window state
const SOUND_FIELDS: [&str; 1] = ["bands_placed"];

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Preset {
    pub(crate) format: u32,
    pub(crate) name: String,
    #[serde(flatten)]
    pub(crate) state: PluginState,
    /// v1 normalized values converted on load
    #[serde(skip)]
    legacy: BTreeMap<String, f32>,
}

impl Preset {
    pub(crate) fn capture(state: PluginState, name: &str) -> Self {
        let mut state = state;
        state
            .fields
            .retain(|key, _| SOUND_FIELDS.contains(&key.as_str()));

        Self {
            format: FORMAT,
            name: name.to_owned(),
            state,
            legacy: BTreeMap::new(),
        }
    }

    fn read(path: &Path) -> io::Result<Self> {
        let text = fs::read_to_string(path)?;
        Self::from_json(&text).map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{}: {error}", path.display()),
            )
        })
    }

    fn from_json(text: &str) -> io::Result<Self> {
        let file: File = serde_json::from_str(text).map_err(io::Error::other)?;
        let format = file.format.ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "missing preset format version")
        })?;

        if !(1..=FORMAT).contains(&format) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unsupported preset format version: {format}"),
            ));
        }

        Ok(file.into_preset(format))
    }

    pub(crate) fn into_state(mut self, params: &EquartParams) -> PluginState {
        if !self.legacy.is_empty() {
            let known: HashMap<String, ParamPtr> = params
                .param_map()
                .into_iter()
                .map(|(id, ptr, _)| (id, ptr))
                .collect();

            for (id, normalized) in std::mem::take(&mut self.legacy) {
                let Some(ptr) = known.get(&id).copied() else {
                    continue;
                };

                self.state
                    .params
                    .insert(id, from_normalized(ptr, normalized));
            }
        }

        // migrate before filling defaults so or_insert migrations still run
        crate::Equart::filter_state(&mut self.state);

        for (id, ptr, _) in params.param_map() {
            self.state
                .params
                .entry(id)
                .or_insert_with(|| default_value(ptr));
        }

        self.state
            .fields
            .entry("bands_placed".to_owned())
            .or_insert_with(|| "0".to_owned());

        self.state
    }

    fn to_json(&self) -> io::Result<String> {
        serde_json::to_string_pretty(self).map_err(io::Error::other)
    }
}

#[derive(Deserialize)]
struct File {
    #[serde(default)]
    format: Option<u32>,
    #[serde(default)]
    name: String,
    #[serde(default)]
    version: String,
    #[serde(default)]
    params: BTreeMap<String, serde_json::Value>,
    #[serde(default)]
    fields: BTreeMap<String, String>,
    #[serde(default)]
    placed: Option<u32>,
}

impl File {
    fn into_preset(self, format: u32) -> Preset {
        let mut params = BTreeMap::new();
        let mut legacy = BTreeMap::new();

        for (id, value) in self.params {
            match serde_json::from_value::<ParamValue>(value.clone()) {
                Ok(value) => {
                    params.insert(id, value);
                }
                // bare numbers come from v1 normalized files
                Err(_) => {
                    if let Some(normalized) = value.as_f64() {
                        legacy.insert(id, normalized as f32);
                    }
                }
            }
        }

        let mut fields = self.fields;

        if let Some(placed) = self.placed
            && !fields.contains_key("bands_placed")
        {
            fields.insert("bands_placed".to_owned(), placed.to_string());
        }

        Preset {
            format,
            name: self.name,
            state: PluginState {
                version: self.version,
                params,
                fields,
            },
            legacy,
        }
    }
}

pub(crate) fn state_of(params: &EquartParams) -> PluginState {
    let values = params
        .param_map()
        .into_iter()
        .map(|(id, ptr, _)| (id, plain_value(ptr)))
        .collect();

    PluginState {
        version: String::new(),
        params: values,
        fields: params.serialize_fields(),
    }
}

pub(crate) fn restore(params: &EquartParams, state: &PluginState) {
    for (id, ptr, _) in params.param_map() {
        if let Some(value) = state.params.get(&id) {
            set_value(ptr, value);
        }
    }

    params.deserialize_fields(&state.fields);
}

pub(crate) fn seed_instance(params: &EquartParams, store: &Store) -> Option<Entry> {
    let (entry, preset) = store.default_preset()?;

    if store.pinned().is_some() {
        restore(params, &preset.into_state(params));
    }

    Some(entry)
}

fn plain_value(ptr: ParamPtr) -> ParamValue {
    // SAFETY: caller keeps the parameter alive
    unsafe {
        match ptr {
            ParamPtr::FloatParam(_) => ParamValue::F32(ptr.unmodulated_plain_value()),
            ParamPtr::IntParam(_) => ParamValue::I32(ptr.unmodulated_plain_value() as i32),
            ParamPtr::BoolParam(_) => ParamValue::Bool(ptr.unmodulated_normalized_value() >= 0.5),
            ParamPtr::EnumParam(inner) => match (*inner).unmodulated_plain_id() {
                Some(id) => ParamValue::String(id.to_owned()),
                None => ParamValue::I32((*inner).unmodulated_plain_value()),
            },
        }
    }
}

fn default_value(ptr: ParamPtr) -> ParamValue {
    // SAFETY: caller keeps the parameter alive
    unsafe {
        match ptr {
            ParamPtr::FloatParam(_) => ParamValue::F32(ptr.default_plain_value()),
            ParamPtr::IntParam(_) => ParamValue::I32(ptr.default_plain_value() as i32),
            ParamPtr::BoolParam(_) => ParamValue::Bool(ptr.default_normalized_value() >= 0.5),
            ParamPtr::EnumParam(inner) => ParamValue::I32((*inner).default_plain_value()),
        }
    }
}

fn from_normalized(ptr: ParamPtr, normalized: f32) -> ParamValue {
    // SAFETY: caller keeps the parameter alive
    let plain = unsafe { ptr.preview_plain(normalized) };

    match ptr {
        ParamPtr::FloatParam(_) => ParamValue::F32(plain),
        ParamPtr::IntParam(_) => ParamValue::I32(plain.round() as i32),
        ParamPtr::BoolParam(_) => ParamValue::Bool(plain >= 0.5),
        ParamPtr::EnumParam(_) => ParamValue::I32(plain.round() as i32),
    }
}

fn set_value(ptr: ParamPtr, value: &ParamValue) {
    // SAFETY: caller keeps the parameter alive
    unsafe {
        match (ptr, value) {
            (ParamPtr::FloatParam(param), ParamValue::F32(value)) => {
                (*param)._internal_set_plain_value(*value);
            }
            (ParamPtr::IntParam(param), ParamValue::I32(value)) => {
                (*param)._internal_set_plain_value(*value);
            }
            (ParamPtr::BoolParam(param), ParamValue::Bool(value)) => {
                (*param)._internal_set_plain_value(*value);
            }
            (ParamPtr::EnumParam(param), ParamValue::I32(value)) => {
                (*param)._internal_set_plain_value(*value);
            }
            (ParamPtr::EnumParam(param), ParamValue::String(id)) => {
                let param = &*param;

                if !param.set_from_id(id) {
                    param._internal_set_plain_value(param.default_plain_value());
                }
            }
            _ => (),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Entry {
    pub(crate) name: String,
    pub(crate) folder: String,
    pub(crate) path: PathBuf,
}

impl Entry {
    pub(crate) fn id(&self) -> String {
        format!("{}/{}", self.folder, self.name)
    }
}

#[derive(Default)]
pub(crate) struct Store {
    root: Option<PathBuf>,
    index: Option<PathBuf>,
    default: Mutex<Option<String>>,
}

impl Store {
    #[cfg(test)]
    pub(crate) fn at(dir: PathBuf) -> Self {
        let store = Self {
            root: Some(dir.join(FOLDER)),
            index: Some(dir.join(INDEX)),
            default: Mutex::new(None),
        };

        store.seed().expect("could seed the preset folders");
        store.load_index().expect("could read the preset index");

        store
    }

    pub(crate) fn shared() -> Arc<Self> {
        static STORE: OnceLock<Arc<Store>> = OnceLock::new();

        let store = STORE.get_or_init(|| {
            let base = crate::display_preferences::user_config_dir()
                .map(|root| root.join(crate::VENDOR).join("EQuart"));

            let store = Self {
                root: base.as_deref().map(preset_folder),
                index: base.as_ref().map(|base| base.join(INDEX)),
                default: Mutex::new(None),
            };

            if let Err(error) = store.seed() {
                nice_plug::nice_warn!("EQuart could not create the preset folders: {error}");
            }

            if let Err(error) = store.load_index() {
                nice_plug::nice_warn!("EQuart could not read the default preset: {error}");
            }

            Arc::new(store)
        });

        Arc::clone(store)
    }

    pub(crate) fn root(&self) -> Option<&Path> {
        self.root.as_deref()
    }

    pub(crate) fn folders(&self) -> Vec<String> {
        let mut names = Vec::new();

        if let Some(root) = &self.root
            && let Ok(entries) = fs::read_dir(root)
        {
            for entry in entries.flatten() {
                if entry.file_type().is_ok_and(|kind| kind.is_dir())
                    && let Ok(name) = entry.file_name().into_string()
                {
                    names.push(name);
                }
            }
        }

        names.sort_by_key(|name| (folder_rank(name), name.to_lowercase()));
        names
    }

    pub(crate) fn entries(&self) -> Vec<Entry> {
        let mut entries = Vec::new();

        for folder in self.folders() {
            entries.extend(self.entries_in(&folder));
        }

        entries
    }

    pub(crate) fn entries_in(&self, folder: &str) -> Vec<Entry> {
        let Some(root) = &self.root else {
            return Vec::new();
        };

        let Ok(dir) = fs::read_dir(root.join(folder)) else {
            return Vec::new();
        };

        let mut entries: Vec<Entry> = dir
            .flatten()
            .filter(|entry| entry.path().extension().is_some_and(|ext| ext == EXTENSION))
            .filter_map(|entry| {
                let path = entry.path();
                let name = path.file_stem()?.to_str()?.to_owned();
                Some(Entry {
                    name,
                    folder: folder.to_owned(),
                    path,
                })
            })
            .collect();

        entries.sort_by_cached_key(|entry| entry.name.to_lowercase());
        entries
    }

    pub(crate) fn load(&self, path: &Path) -> io::Result<Preset> {
        Preset::read(path)
    }

    pub(crate) fn save(&self, folder: &str, name: &str, preset: &Preset) -> io::Result<PathBuf> {
        let root = self.root().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "user configuration directory is unavailable",
            )
        })?;

        let folder = folder.trim();
        let name = name.trim();

        if !is_valid_name(folder) || !is_valid_name(name) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Use a name without slashes or reserved characters",
            ));
        }

        let dir = root.join(folder);
        fs::create_dir_all(&dir)?;

        let mut preset = preset.clone();
        preset.format = FORMAT;
        preset.name = name.to_owned();

        static TEMP_SEQ: AtomicU64 = AtomicU64::new(0);
        let seq = TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
        let path = dir.join(format!("{name}.{EXTENSION}"));
        let temp = dir.join(format!("{name}.{EXTENSION}.{}.{seq}.tmp", std::process::id()));

        fs::write(&temp, preset.to_json()?)?;

        if let Err(error) = fs::rename(&temp, &path) {
            let _ = fs::remove_file(&temp);
            return Err(error);
        }

        Ok(path)
    }

    pub(crate) fn pinned(&self) -> Option<String> {
        self.default
            .lock()
            .expect("preset index lock poisoned")
            .clone()
    }

    pub(crate) fn default_id(&self) -> String {
        self.pinned().unwrap_or_else(|| DEFAULT.to_owned())
    }

    pub(crate) fn set_default(&self, id: Option<&str>) -> io::Result<()> {
        *self.default.lock().expect("preset index lock poisoned") = id.map(str::to_owned);

        let Some(index) = &self.index else {
            return Ok(());
        };

        let text = serde_json::to_string_pretty(&IndexFile {
            default: id.map(str::to_owned),
        })
        .map_err(io::Error::other)?;

        if let Some(parent) = index.parent() {
            fs::create_dir_all(parent)?;
        }

        fs::write(index, text)
    }

    pub(crate) fn default_preset(&self) -> Option<(Entry, Preset)> {
        let id = self.default_id();

        self.load_by_id(&id).or_else(|| {
            if id != DEFAULT {
                self.load_by_id(DEFAULT)
            } else {
                None
            }
        })
    }

    fn load_by_id(&self, id: &str) -> Option<(Entry, Preset)> {
        let (folder, name) = id.split_once('/')?;

        if !is_valid_name(folder) || !is_valid_name(name) {
            return None;
        }

        let root = self.root()?;
        let path = root.join(folder).join(format!("{name}.{EXTENSION}"));
        let preset = self.load(&path).ok()?;

        Some((
            Entry {
                name: name.to_owned(),
                folder: folder.to_owned(),
                path,
            },
            preset,
        ))
    }

    pub(crate) fn reveal(&self) -> io::Result<()> {
        let root = self.root().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "user configuration directory is unavailable",
            )
        })?;

        fs::create_dir_all(root)?;

        #[cfg(target_os = "windows")]
        let mut command = {
            let mut command = std::process::Command::new("explorer");
            command.arg(root);
            command
        };
        #[cfg(target_os = "macos")]
        let mut command = {
            let mut command = std::process::Command::new("open");
            command.arg(root);
            command
        };
        #[cfg(all(unix, not(target_os = "macos")))]
        let mut command = {
            let mut command = std::process::Command::new("xdg-open");
            command.arg(root);
            command
        };

        // explorer returns non-zero even when it opens the window
        command.spawn().map(drop)
    }

    fn seed(&self) -> io::Result<()> {
        let Some(root) = &self.root else {
            return Ok(());
        };

        fs::create_dir_all(root.join(USER))?;

        let factory = root.join(FACTORY);
        fs::create_dir_all(&factory)?;

        for (name, preset) in factory_library() {
            let path = factory.join(format!("{name}.{EXTENSION}"));
            let current = Preset::read(&path).is_ok_and(|existing| existing.format >= FORMAT);

            if current {
                continue;
            }

            if let Err(error) = fs::write(&path, preset.to_json()?) {
                nice_plug::nice_warn!("EQuart could not write factory preset {name}: {error}");
            }
        }

        Ok(())
    }

    fn load_index(&self) -> io::Result<()> {
        let Some(index) = &self.index else {
            return Ok(());
        };

        let file: IndexFile = match fs::read_to_string(index) {
            Ok(text) => serde_json::from_str(&text).map_err(|error| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("{}: {error}", index.display()),
                )
            })?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error),
        };

        *self.default.lock().expect("preset index lock poisoned") = file.default;

        Ok(())
    }
}

/// rename legacy lowercase `presets` folder via a temp name on case-insensitive filesystems
fn preset_folder(base: &Path) -> PathBuf {
    let target = base.join(FOLDER);

    let Ok(entries) = fs::read_dir(base) else {
        return target;
    };

    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };

        if name == FOLDER || !name.eq_ignore_ascii_case(FOLDER) {
            continue;
        }

        let staging = base.join(format!("{FOLDER}.moving"));
        let moved = fs::rename(entry.path(), &staging).and_then(|()| fs::rename(&staging, &target));

        if let Err(error) = moved {
            nice_plug::nice_warn!("EQuart could not rename the preset folder: {error}");
        }

        break;
    }

    target
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct IndexFile {
    default: Option<String>,
}

fn folder_rank(name: &str) -> u8 {
    match name {
        FACTORY => 0,
        USER => 1,
        _ => 2,
    }
}

pub(crate) fn is_valid_name(name: &str) -> bool {
    if name.is_empty()
        || name.len() > 96
        || name.starts_with('.')
        || name.ends_with('.')
        || name
            .chars()
            .any(|c| c.is_control() || r#"<>:"/\|?*"#.contains(c))
    {
        return false;
    }

    // windows reserves device names regardless of case or extension
    let stem = name.split('.').next().unwrap_or(name);
    let upper = stem.to_ascii_uppercase();

    !matches!(
        upper.as_str(),
        "CON"
            | "PRN"
            | "AUX"
            | "NUL"
            | "COM1"
            | "COM2"
            | "COM3"
            | "COM4"
            | "COM5"
            | "COM6"
            | "COM7"
            | "COM8"
            | "COM9"
            | "LPT1"
            | "LPT2"
            | "LPT3"
            | "LPT4"
            | "LPT5"
            | "LPT6"
            | "LPT7"
            | "LPT8"
            | "LPT9"
    )
}

fn factory_library() -> [(&'static str, Preset); 3] {
    let mut library = [
        ("Default", factory_default()),
        ("200-LowCut", factory_low_cut(200.0, false)),
        ("Auto-LowCut", factory_low_cut(80.0, true)),
    ];

    for (name, preset) in &mut library {
        preset.name = (*name).to_owned();
    }

    library
}

fn factory_default() -> Preset {
    let params = EquartParams::default();

    Preset::capture(state_of(&params), "Default")
}

fn factory_low_cut(hz: f32, follow: bool) -> Preset {
    use crate::filter::{CutSlope, FilterKind};

    let params = EquartParams::default();
    let band = &params.bands[0];

    set(&band.enabled, true);
    set(&band.kind, FilterKind::Highpass);
    set(&band.frequency, hz);
    set(&band.slope, CutSlope::Db24);

    if follow {
        set(&band.pitch.enabled, true);
        set(&band.pitch.bypassed, false);
        set(&band.pitch.multiplier, 1.0);
        set(
            &band.pitch.detection.algorithm,
            crate::pitch::DetectionMode::SpectralMelody,
        );
        set(&band.pitch.detection.target, crate::pitch::TargetMode::Auto);
    }

    params.mark_placed(0);

    Preset::capture(state_of(&params), "")
}

fn set<P: Param>(param: &P, value: P::Plain) {
    let normalized = param.preview_normalized(value);

    // SAFETY: `param` is alive for the whole call
    unsafe { param.as_ptr()._internal_set_normalized_value(normalized) };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filter::{CutSlope, FilterKind};
    use std::sync::atomic::Ordering;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_store() -> (Store, PathBuf) {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("equart-presets-{}-{nonce}", std::process::id()));

        (
            Store {
                root: Some(dir.join(FOLDER)),
                index: Some(dir.join(INDEX)),
                default: Mutex::new(None),
            },
            dir,
        )
    }

    fn apply(params: &EquartParams, preset: Preset) {
        restore(params, &preset.into_state(params));
    }

    fn json(params: &BTreeMap<String, ParamValue>) -> String {
        serde_json::to_string(params).expect("parameters serialise")
    }

    #[test]
    fn factory_library_loads() {
        let params = EquartParams::default();
        let known: Vec<String> = params
            .param_map()
            .into_iter()
            .map(|(id, _, _)| id)
            .collect();

        for (name, preset) in factory_library() {
            assert_eq!(preset.format, FORMAT, "{name}");

            for id in preset.state.params.keys() {
                assert!(known.contains(id), "{name} stores unknown parameter {id}");
            }

            assert_eq!(preset.state.params.len(), known.len(), "{name}");
            assert!(preset.state.fields.contains_key("bands_placed"), "{name}");
        }

        let (_, default) = factory_library()[0].clone();
        let fresh = EquartParams::default();
        assert_eq!(
            json(&default.into_state(&fresh).params),
            json(&state_of(&fresh).params),
            "the factory Default is the factory default"
        );

        let (_, cut) = &factory_library()[1];
        let applied = EquartParams::default();
        apply(&applied, cut.clone());

        let band = applied.control(0);
        assert!(band.enabled);
        assert_eq!(band.kind, FilterKind::Highpass);
        assert!((band.frequency - 200.0).abs() < 0.5, "{}", band.frequency);
        assert_eq!(band.slope, CutSlope::Db24);
        assert!(!band.pitch.enabled);

        let (_, auto) = &factory_library()[2];
        let applied = EquartParams::default();
        apply(&applied, auto.clone());

        let band = applied.control(0);
        assert!(band.enabled);
        assert!(band.pitch.enabled);
        assert!((band.pitch.multiplier - 1.0).abs() < 0.001);
        assert!(applied.is_placed(0));
    }

    #[test]
    fn preset_preserves_all_params() {
        let params = EquartParams::default();
        let mut expected = BTreeMap::new();

        for (index, (id, ptr, _)) in params.param_map().into_iter().enumerate() {
            let value = 0.15 + (index % 53) as f32 * 0.013;
            // SAFETY: pointers come from `params`
            unsafe { ptr._internal_set_normalized_value(value) };
            let read = unsafe { ptr.unmodulated_normalized_value() };
            expected.insert(id, read);
        }

        let preset = Preset::capture(state_of(&params), "every");

        for id in [
            "region_enabled_3",
            "region_low_3",
            "region_gain_3",
            "res_depth_3",
            "harmonic_width_3",
        ] {
            assert!(preset.state.params.contains_key(id), "{id} is missing");
        }

        let loaded = EquartParams::default();
        restore(&loaded, &preset.into_state(&loaded));

        for (id, ptr, _) in loaded.param_map() {
            // SAFETY: pointers come from `loaded`
            let value = unsafe { ptr.unmodulated_normalized_value() };
            let want = expected[&id];
            assert!(
                (value - want).abs() < 1e-4,
                "{id}: saved {want}, loaded {value}"
            );
        }

        params.clear_placed(0);
        params.mark_placed(5);

        let preset = Preset::capture(state_of(&params), "placed");
        let loaded = EquartParams::default();
        restore(&loaded, &preset.into_state(&loaded));
        assert_eq!(loaded.bands_placed.load(Ordering::Relaxed), 1 << 5);
    }

    #[test]
    fn preset_disk_round_trip() {
        let (store, dir) = temp_store();
        store.seed().unwrap();
        assert_eq!(store.folders(), [FACTORY.to_owned(), USER.to_owned()]);

        let entries = store.entries_in(FACTORY);
        let names: Vec<&str> = entries.iter().map(|entry| entry.name.as_str()).collect();
        assert!(names.contains(&"Default"));
        assert!(names.contains(&"200-LowCut"));
        assert!(names.contains(&"Auto-LowCut"));

        let params = EquartParams::default();
        params.mark_placed(0);
        params.mark_placed(2);

        let preset = Preset::capture(state_of(&params), "Broad");
        let path = store.save("Mine", "Broad", &preset).unwrap();
        assert_eq!(path.extension().unwrap(), EXTENSION);
        assert!(path.exists());
        assert_eq!(
            store.folders(),
            [FACTORY.to_owned(), USER.to_owned(), "Mine".to_owned()]
        );

        let loaded = store.load(&path).unwrap();
        assert_eq!(loaded.name, "Broad");
        assert_eq!(loaded.format, FORMAT);

        let applied = EquartParams::default();
        restore(&applied, &loaded.into_state(&applied));
        assert_eq!(applied.bands_placed.load(Ordering::Relaxed), 0b101);

        assert_eq!(
            store.save("Mine", "../escape", &preset).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
        assert_eq!(
            store.save("Mine", "  ", &preset).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );

        assert_eq!(store.default_id(), DEFAULT);
        let (entry, default) = store.default_preset().unwrap();
        assert_eq!(entry.id(), "Factory/Default");
        assert_eq!(default.name, "Default");

        store.set_default(Some("Mine/Broad")).unwrap();
        assert_eq!(store.pinned().as_deref(), Some("Mine/Broad"));
        let (entry, default) = store.default_preset().unwrap();
        assert_eq!(entry.id(), "Mine/Broad");
        assert_eq!(
            default.into_state(&applied).fields.get("bands_placed"),
            Some(&0b101.to_string())
        );

        // deleted pinned default falls back to Factory/Default
        store.set_default(Some("User/DeletedPreset")).unwrap();
        let (entry, _) = store.default_preset().unwrap();
        assert_eq!(entry.id(), DEFAULT);

        store.set_default(None).unwrap();
        assert_eq!(store.default_id(), DEFAULT);

        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn v1_preset_loads() {
        let params = EquartParams::default();
        let frequency = &params.bands[0].frequency;
        let gain = &params.bands[0].gain;
        let frequency_value = frequency.unmodulated_normalized_value();
        let gain_value = gain.unmodulated_normalized_value();

        let text = serde_json::json!({
            "format": 1,
            "name": "Old",
            "placed": 1,
            "params": {
                "frequency_1": frequency_value,
                "gain_1": gain_value,
                "bogus": 0.5,
            }
        })
        .to_string();

        let preset = Preset::from_json(&text).unwrap();
        let applied = EquartParams::default();
        let state = preset.into_state(&applied);
        restore(&applied, &state);

        assert!((applied.control(0).frequency - frequency.value()).abs() < 1.0);
        assert_eq!(applied.bands_placed.load(Ordering::Relaxed), 1);
        assert_eq!(
            applied.control(1).frequency,
            EquartParams::default().control(1).frequency
        );
    }

    #[test]
    fn preset_migrates_legacy_params() {
        let text = serde_json::json!({
            "format": 2,
            "name": "LegacyRegion",
            "params": {
                "oversampling": { "string": "2x" },
                "region_depth_1": { "f32": 6.0 }
            }
        })
        .to_string();

        let preset = Preset::from_json(&text).unwrap();
        let applied = EquartParams::default();
        restore(&applied, &preset.into_state(&applied));

        assert_eq!(
            applied.global.processing_mode.value(),
            crate::processing_mode::ProcessingMode::NaturalPhase
        );
        assert!((applied.regions[0].gain.value() - (-6.0)).abs() < 1e-3);
    }

    #[test]
    fn invalid_presets_rejected() {
        assert!(Preset::from_json("{}").is_err());
        assert!(Preset::from_json(r#"{"format": 99, "params": {}}"#).is_err());

        for bad in ["foo.", "CON", "nul", "com1", "LPT9.eq", ".hidden", "a/b"] {
            assert!(!is_valid_name(bad), "{bad} should be rejected");
        }

        for good in ["Default", "200-LowCut", "v1.2 Vocal", "console"] {
            assert!(is_valid_name(good), "{good} should be accepted");
        }
    }
}
