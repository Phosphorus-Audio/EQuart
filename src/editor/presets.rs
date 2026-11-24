use super::*;
use crate::presets::{self, Entry, Preset, Store};
use controls::{Fall, card_plate};
use modifiers::picker_button;

const BAR_W: f32 = 142.0;
const STEP_HIT: f32 = 16.0;
const STEP_GLYPH: f32 = 11.0;
pub(super) const COMPARE_W: f32 = 46.0;
const COMPARE_GAP: f32 = 10.0;
const COMPARE_BUTTON_W: f32 = 18.0;
const COMPARE_SLASH_GAP: f32 = COMPARE_W - 2.0 * COMPARE_BUTTON_W;

const CARD_W: f32 = 468.0;
const CARD_H: f32 = 300.0;
const CARD_MARGIN: f32 = 10.0;
const FOLDER_W: f32 = 118.0;
const COLUMN_GAP: f32 = 10.0;
const BAR_H: f32 = 26.0;
const SAVE_ROW: f32 = 26.0;
const BAR_GAP: f32 = 8.0;
const TOOL_SIZE: f32 = 22.0;

pub(super) struct Browser {
    pub(super) store: Arc<Store>,
    shared_current: Arc<std::sync::Mutex<Option<Entry>>>,
    life: controls::CardLife,
    menu: controls::CardLife,
    folder: Option<String>,
    pub(super) search: String,
    pub(super) current: Option<Entry>,
    pub(super) save: Option<SaveDraft>,
    cached_folders: Vec<String>,
    cached_entries: Vec<Entry>,
    cache_dirty: bool,
    keep_open: bool,
}

impl Browser {
    pub(super) fn new(
        store: Arc<Store>,
        shared_current: Arc<std::sync::Mutex<Option<Entry>>>,
    ) -> Self {
        let current = shared_current
            .lock()
            .expect("current preset lock poisoned")
            .clone();
        let folder = current.as_ref().map(|entry| entry.folder.clone());
        Self {
            store,
            shared_current,
            life: controls::CardLife::SHUT,
            menu: controls::CardLife::SHUT,
            folder,
            search: String::new(),
            current,
            save: None,
            cached_folders: Vec::new(),
            cached_entries: Vec::new(),
            cache_dirty: true,
            keep_open: false,
        }
    }

    pub(super) fn set_current(&mut self, entry: Option<Entry>) {
        self.current = entry.clone();
        *self
            .shared_current
            .lock()
            .expect("current preset lock poisoned") = entry;
    }

    pub(super) fn invalidate_cache(&mut self) {
        self.cache_dirty = true;
    }

    fn refresh_cache(&mut self) {
        if !self.cache_dirty {
            return;
        }

        self.cached_folders = self.store.folders();
        self.cached_entries = self.store.entries();
        self.cache_dirty = false;
    }

    pub(super) fn is_open(&self) -> bool {
        self.life.is_up()
    }

    pub(super) fn is_shown(&self) -> bool {
        self.life.is_shown() || self.menu.is_shown()
    }

    pub(super) fn close(&mut self) {
        self.life.shut();
        self.menu.shut();
        self.save = None;
        self.search.clear();
    }

    pub(super) fn release(&mut self, now: f64) {
        self.life.release(now);
        self.menu.release(now);
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum ComparisonSlot {
    #[default]
    A,
    B,
}

impl ComparisonSlot {
    const ALL: [Self; 2] = [Self::A, Self::B];

    fn index(self) -> usize {
        match self {
            Self::A => 0,
            Self::B => 1,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::A => "A",
            Self::B => "B",
        }
    }
}

#[derive(Default)]
pub(super) struct Comparison {
    active: ComparisonSlot,
    states: [Option<PluginState>; 2],
}

impl Comparison {
    fn reset(&mut self) {
        self.active = ComparisonSlot::A;
        self.states = [None, None];
    }
}

pub(super) struct SaveDraft {
    name: String,
    folder: String,
    focus_name: bool,
    error: Option<String>,
}

#[cfg(test)]
pub(super) fn save_draft(name: &str, folder: &str) -> SaveDraft {
    SaveDraft {
        name: name.to_owned(),
        folder: folder.to_owned(),
        focus_name: false,
        error: None,
    }
}

impl EquartEditor {
    pub(super) fn preset_bar(
        &mut self,
        ui: &mut egui::Ui,
        footer: Rect,
        now: f64,
        right: f32,
        divider_x: f32,
    ) {
        let mid = footer.center().y;
        let comparison_divider = right - COMPARE_W - COMPARE_GAP;
        let preset_right = comparison_divider - COMPARE_GAP;
        let bar = Rect::from_min_max(
            Pos2::new(preset_right - BAR_W, mid - 10.0),
            Pos2::new(preset_right, mid + 10.0),
        );

        let palette = theme::current(ui.ctx());

        ui.painter().vline(
            divider_x,
            (mid - 7.0)..=(mid + 7.0),
            Stroke::new(1.0, palette.panel.border),
        );

        let previous_rect = Rect::from_center_size(
            Pos2::new(bar.left() + STEP_HIT * 0.5, mid),
            Vec2::splat(STEP_HIT),
        );

        let next_rect = Rect::from_center_size(
            Pos2::new(bar.right() - STEP_HIT * 0.5, mid),
            Vec2::splat(STEP_HIT),
        );

        let label_rect = Rect::from_min_max(
            Pos2::new(previous_rect.right() + 2.0, bar.top()),
            Pos2::new(next_rect.left() - 2.0, bar.bottom()),
        );

        let previous = ui
            .interact(
                previous_rect,
                ui.make_persistent_id(("preset-step", 0)),
                Sense::click(),
            )
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text("Previous preset");

        let next = ui
            .interact(
                next_rect,
                ui.make_persistent_id(("preset-step", 1)),
                Sense::click(),
            )
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text("Next preset");

        let label = ui
            .interact(
                label_rect,
                ui.make_persistent_id(("preset-bar", 0)),
                Sense::click(),
            )
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text("Presets");

        self.preset.keep_open = false;

        if previous.clicked() {
            self.step_preset(false, now);
        }

        if next.clicked() {
            self.step_preset(true, now);
        }

        if label.clicked() {
            if self.preset.life.is_up() {
                self.preset.release(now);
            } else {
                self.settings.release(now);
                self.scale.release(now);
                self.layout.release(now);

                if self.theme_open {
                    self.theme_open = false;
                    self.flush_theme(Some(ui.ctx()));
                }

                self.preset.invalidate_cache();
                self.preset.life.open(now);
            }

            self.preset.keep_open = true;
        }

        let open = self.preset.is_open();
        let ink = |response: &Response| {
            let hover = hover_amount(response);
            let base = if open {
                palette.text.body.lerp_to_gamma(palette.text.primary, 0.6)
            } else {
                palette.text.muted.lerp_to_gamma(palette.text.body, 0.4)
            };
            base.lerp_to_gamma(palette.text.primary, hover)
        };

        for (response, glyph) in [
            (previous, icons::Glyph::CaretLeft),
            (next, icons::Glyph::CaretRight),
        ] {
            icons::draw(
                ui.painter(),
                ui.ctx(),
                glyph,
                Rect::from_center_size(response.rect.center(), Vec2::splat(STEP_GLYPH)),
                ink(&response),
            );
        }

        let caption = self
            .preset
            .current
            .as_ref()
            .map_or("Presets", |entry| entry.name.as_str());
        let font = FontId::proportional(11.0);

        let text = elide(
            ui.painter(),
            caption,
            font.clone(),
            label_rect.width() - 4.0,
        );

        ui.painter().text(
            label_rect.center(),
            egui::Align2::CENTER_CENTER,
            text,
            font,
            ink(&label),
        );

        let mut life = self.preset.life;
        let dismissed = Self::animated_card(
            browser_card(&label).width(CARD_W),
            &mut life,
            now,
            Some(label.rect.center_top()),
            |ui, progress, flash, fall| self.browser_page(ui, now, progress, flash, fall),
        );

        if dismissed && !label.clicked() && !self.preset.keep_open && !self.preset.menu.is_shown() {
            life.release(now);
            self.preset.menu.release(now);
        }

        if !life.is_shown() {
            self.preset.save = None;
            self.preset.menu.shut();
            self.preset.search.clear();
        }

        self.preset.life = life;
        self.comparison_bar(ui, footer, comparison_divider, right);
    }

    fn comparison_bar(&mut self, ui: &mut egui::Ui, footer: Rect, divider_x: f32, right: f32) {
        let mid = footer.center().y;
        let palette = theme::current(ui.ctx());

        ui.painter().vline(
            divider_x,
            (mid - 7.0)..=(mid + 7.0),
            Stroke::new(1.0, palette.panel.border),
        );
        ui.painter().text(
            Pos2::new(right - COMPARE_W * 0.5, mid),
            egui::Align2::CENTER_CENTER,
            "/",
            FontId::proportional(11.0),
            palette.text.muted,
        );

        let mut picked = None;

        for (index, slot) in ComparisonSlot::ALL.into_iter().enumerate() {
            let rect = Rect::from_min_size(
                Pos2::new(
                    right - COMPARE_W + index as f32 * (COMPARE_BUTTON_W + COMPARE_SLASH_GAP),
                    mid - 10.0,
                ),
                Vec2::new(COMPARE_BUTTON_W, 20.0),
            );
            let selected = self.comparison.active == slot;
            let tip = if slot == ComparisonSlot::A {
                "Switch to A"
            } else {
                "Switch to B"
            };
            let response = ui
                .interact(
                    rect,
                    ui.make_persistent_id(("comparison", index)),
                    Sense::click(),
                )
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text(tip);
            let hover = hover_amount(&response);
            let ink = if selected {
                palette.text.primary
            } else {
                palette.text.muted.gamma_multiply(0.55 + 0.25 * hover)
            };
            let font = if selected {
                FontId::new(11.0, egui::FontFamily::Name(GLOBAL_FONT.into()))
            } else {
                FontId::proportional(11.0)
            };

            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                slot.label(),
                font,
                ink,
            );

            if response.clicked() {
                picked = Some(slot);
            }
        }

        if let Some(slot) = picked {
            self.switch_comparison(slot);
        }
    }

    fn browser_page(
        &mut self,
        ui: &mut egui::Ui,
        now: f64,
        progress: f32,
        flash: f32,
        fall: Option<Fall>,
    ) -> bool {
        let accent = self.accent();
        let card = Rect::from_min_size(ui.cursor().min, Vec2::new(CARD_W, CARD_H));

        menu::LabelFlash::begin(ui, card, flash);
        ui.allocate_rect(card, Sense::hover());
        card_plate(ui, card, progress.clamp(0.0, 1.0), accent);

        let inner = card.shrink(CARD_MARGIN);

        if self.preset.save.is_some() {
            return self.save_page(ui, inner, now, fall);
        }

        let list_h = inner.height() - BAR_H - BAR_GAP;
        let folders_rect = Rect::from_min_size(inner.min, Vec2::new(FOLDER_W, list_h));
        let presets_rect = Rect::from_min_size(
            Pos2::new(folders_rect.right() + COLUMN_GAP, inner.top()),
            Vec2::new(inner.width() - FOLDER_W - COLUMN_GAP, list_h),
        );
        let bar_rect = Rect::from_min_size(
            Pos2::new(inner.left(), inner.bottom() - BAR_H),
            Vec2::new(inner.width(), BAR_H),
        );

        self.preset.refresh_cache();
        let folders = self.preset.cached_folders.clone();
        let folder = self
            .preset
            .folder
            .clone()
            .filter(|folder| folders.contains(folder))
            .or_else(|| folders.first().cloned());

        let needle = self.preset.search.trim().to_lowercase();
        let searching = !needle.is_empty();

        let entries: Vec<Entry> = if searching {
            self.preset
                .cached_entries
                .iter()
                .filter(|entry| entry.name.to_lowercase().contains(&needle))
                .cloned()
                .collect()
        } else {
            folder.as_deref().map_or_else(Vec::new, |folder| {
                self.preset
                    .cached_entries
                    .iter()
                    .filter(|entry| entry.folder == folder)
                    .cloned()
                    .collect()
            })
        };

        let current = self.preset.current.as_ref().map(|entry| entry.path.clone());
        let palette = theme::current(ui.ctx());
        let mut picked_folder = None;
        let mut picked_preset = None;

        let mut left = ui.new_child(egui::UiBuilder::new().max_rect(folders_rect));

        menu::body(&mut left);

        theme::scroll(
            &mut left,
            egui::ScrollArea::vertical()
                .id_salt("preset-folders")
                .auto_shrink([false, false]),
            palette.scrollbar,
            |ui| {
                menu::section(ui, "FOLDERS");

                if folders.is_empty() {
                    menu::note(ui, "No folders yet");
                }

                for name in &folders {
                    let selected = folder.as_deref() == Some(name.as_str());

                    if menu::row(ui, name, None, selected, accent) {
                        picked_folder = Some(name.clone());
                    }
                }
            },
        );

        let mut right = ui.new_child(egui::UiBuilder::new().max_rect(presets_rect));

        menu::body(&mut right);

        theme::scroll(
            &mut right,
            egui::ScrollArea::vertical()
                .id_salt("preset-files")
                .auto_shrink([false, false]),
            palette.scrollbar,
            |ui| {
                let heading = if searching { "RESULTS" } else { "PRESETS" };
                menu::section(ui, heading);

                if entries.is_empty() {
                    menu::note(
                        ui,
                        if searching {
                            "Nothing found"
                        } else {
                            "Empty folder"
                        },
                    );
                }

                for entry in &entries {
                    let ticked = current.as_ref().is_some_and(|path| *path == entry.path);
                    let value = searching.then_some(entry.folder.as_str());

                    if menu::row(ui, &entry.name, value, ticked, accent) {
                        picked_preset = Some(entry.clone());
                    }
                }
            },
        );

        if let Some(folder) = picked_folder {
            self.preset.folder = Some(folder);
            self.preset.search.clear();
        }

        if let Some(entry) = picked_preset {
            self.load_preset(&entry, now);
        }

        let rule = (bar_rect.top() - BAR_GAP * 0.5).round();
        ui.painter().hline(
            inner.x_range(),
            rule,
            Stroke::new(1.0, palette.panel.border),
        );

        let mut bar = ui.new_child(egui::UiBuilder::new().max_rect(bar_rect));
        let mut save = false;

        bar.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            let field_w = (ui.available_width() - 3.0 * TOOL_SIZE - 4.0 * 6.0 - 9.0).max(60.0);
            let field = ui.add_sized(
                [field_w, 20.0],
                egui::TextEdit::singleline(&mut self.preset.search)
                    .hint_text("Search")
                    .font(FontId::proportional(11.0))
                    .margin(Vec2::new(6.0, 2.0)),
            );

            if field.has_focus() && ui.input(|input| input.key_pressed(egui::Key::Escape)) {
                self.preset.search.clear();
                field.surrender_focus();
            }

            if search_button(ui).clicked() {
                field.request_focus();
            }

            let (_, rule) = ui.allocate_space(Vec2::new(9.0, 16.0));
            ui.painter().vline(
                rule.center().x,
                rule.y_range(),
                Stroke::new(1.0, palette.panel.border),
            );

            save = icon_plate(ui, ("preset-tool", 0), icons::Glyph::SavePreset)
                .on_hover_text("Save preset")
                .clicked();

            self.preset_menu(ui, now);
        });

        if save {
            self.preset.save = Some(SaveDraft {
                name: self
                    .preset
                    .current
                    .as_ref()
                    .map_or_else(String::new, |entry| entry.name.clone()),
                folder: folder
                    .or_else(|| {
                        self.preset
                            .current
                            .as_ref()
                            .map(|entry| entry.folder.clone())
                    })
                    .unwrap_or_else(|| presets::USER.to_owned()),
                focus_name: true,
                error: None,
            });
        }

        false
    }

    fn save_page(&mut self, ui: &mut egui::Ui, inner: Rect, now: f64, fall: Option<Fall>) -> bool {
        let Some(mut draft) = self.preset.save.take() else {
            return false;
        };
        let accent = self.accent();
        let mut page = ui.new_child(egui::UiBuilder::new().max_rect(inner));
        menu::body(&mut page);
        menu::section(&mut page, "SAVE PRESET");

        let mut commit = false;
        let mut cancel = false;

        page.label("Name");
        let name = page.add(
            egui::TextEdit::singleline(&mut draft.name)
                .hint_text("Preset name")
                .desired_width(f32::INFINITY),
        );

        if std::mem::take(&mut draft.focus_name) {
            name.request_focus();
        }

        if name.lost_focus() && page.input(|input| input.key_pressed(egui::Key::Enter)) {
            commit = true;
        }

        let name_ok = presets::is_valid_name(draft.name.trim());

        page.add_space(2.0);
        menu::section(&mut page, "SAVE TO");

        self.preset.refresh_cache();

        let folders = self.preset.cached_folders.clone();
        let mut picked = None;

        let room = page.max_rect().bottom() - 44.0 - page.cursor().top();
        let rows = ((room / SAVE_ROW).floor()).clamp(1.0, 6.0);
        let scrollbar = theme::current(page.ctx()).scrollbar;

        theme::scroll(
            &mut page,
            egui::ScrollArea::vertical()
                .id_salt("preset-save-folders")
                .max_height(rows * SAVE_ROW)
                .auto_shrink([false, true]),
            scrollbar,
            |ui| {
                for folder in &folders {
                    if menu::row(ui, folder, None, *folder == draft.folder, accent) {
                        picked = Some(folder.clone());
                    }
                }

                if !folders.contains(&draft.folder) {
                    menu::row(ui, &draft.folder, None, true, accent);
                }
            },
        );

        if let Some(folder) = picked {
            draft.folder = folder;
        }

        if let Some(error) = &draft.error {
            page.add_space(2.0);
            page.colored_label(RED, error.as_str());
        }

        let floor = page.max_rect().bottom() - 26.0;
        page.add_space((floor - page.cursor().top()).max(0.0));
        page.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            commit |= picker_button(ui, "Save", 46.0, Some(accent), name_ok);
            cancel = picker_button(ui, "Cancel", 54.0, None, true);
        });

        let _ = fall;

        if cancel {
            self.preset.save = None;
            return false;
        }

        if commit {
            let name = draft.name.trim().to_owned();
            let preset = Preset::capture(self.sound_state(), &name);

            match self.preset.store.save(&draft.folder, &name, &preset) {
                Ok(path) => {
                    self.preset.invalidate_cache();
                    self.preset.folder = Some(draft.folder.clone());
                    self.preset.search.clear();
                    self.notify(format!("Saved {name}"), now);
                    self.preset.set_current(Some(Entry {
                        name,
                        folder: draft.folder,
                        path,
                    }));
                    self.preset.save = None;
                    ui.ctx().request_repaint();
                    return false;
                }
                Err(error) => draft.error = Some(error.to_string()),
            }
        }

        self.preset.save = Some(draft);

        false
    }

    fn preset_menu(&mut self, ui: &mut egui::Ui, now: f64) {
        let button = icon_plate(ui, ("preset-tool", 1), icons::Glyph::ModifierMenu)
            .on_hover_text("Preset options");

        let mut life = self.preset.menu;
        if button.clicked() {
            if life.is_up() {
                life.release(now);
            } else {
                life.open(now);
            }
        }

        let accent = self.accent();
        let current = self.preset.current.clone();
        let default = self.preset.store.default_id();
        let mut reveal = false;
        let mut pin = false;

        let dismissed = if life.is_shown() {
            let popup = egui::Popup::menu(&button)
                .open(true)
                .align(egui::emath::RectAlign::TOP_END)
                .gap(5.0)
                .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                .width(196.0)
                .frame(menu::card(&button.ctx));

            Self::animated_card(
                popup,
                &mut life,
                now,
                Some(button.rect.right_top()),
                |ui, _, flash, fall| {
                    ui.set_width(182.0);
                    menu::LabelFlash::begin(
                        ui,
                        ui.max_rect(),
                        if fall.is_none() { flash } else { 1.0 },
                    );
                    menu::body(ui);
                    menu::section(ui, "PRESETS");
                    reveal = menu::row(ui, "Open preset folder", None, false, accent);
                    let ticked = current.as_ref().is_some_and(|entry| entry.id() == default);
                    let pinned = ui
                        .add_enabled_ui(current.is_some(), |ui| {
                            menu::row(ui, "Set as default preset", None, ticked, accent)
                        })
                        .inner;
                    pin = pinned && current.is_some();
                    reveal || pin
                },
            )
        } else {
            false
        };

        if dismissed && !button.clicked() {
            life.release(now);
        }
        self.preset.menu = life;

        if reveal {
            if let Err(error) = self.preset.store.reveal() {
                self.notify(format!("Could not open the preset folder: {error}"), now);
            }
            self.preset.invalidate_cache();
            self.preset.menu.release(now);
        }

        if pin && let Some(entry) = &self.preset.current {
            match self.preset.store.set_default(Some(&entry.id())) {
                Ok(()) => self.notify(format!("{} is the default preset", entry.name), now),
                Err(error) => {
                    self.notify(format!("Could not save the default preset: {error}"), now)
                }
            }
            self.preset.menu.release(now);
        }
    }

    fn step_preset(&mut self, forwards: bool, now: f64) {
        self.preset.invalidate_cache();
        self.preset.refresh_cache();

        let entries: Vec<Entry> = match &self.preset.folder {
            Some(folder) if self.preset.search.trim().is_empty() => {
                let in_folder: Vec<Entry> = self
                    .preset
                    .cached_entries
                    .iter()
                    .filter(|entry| entry.folder == *folder)
                    .cloned()
                    .collect();
                if in_folder.is_empty() {
                    self.preset.cached_entries.clone()
                } else {
                    in_folder
                }
            }
            _ => self.preset.cached_entries.clone(),
        };

        if entries.is_empty() {
            self.notify("No presets", now);
            return;
        }

        let current = self.preset.current.as_ref().map(|entry| entry.path.clone());
        let index = current.and_then(|path| entries.iter().position(|entry| entry.path == path));
        let next = match (index, forwards) {
            (Some(index), true) => (index + 1) % entries.len(),
            (Some(index), false) => (index + entries.len() - 1) % entries.len(),
            (None, true) => 0,
            (None, false) => entries.len() - 1,
        };

        let entry = entries[next].clone();
        self.load_preset(&entry, now);
        self.preset.keep_open = true;
    }

    fn load_preset(&mut self, entry: &Entry, now: f64) {
        match self.preset.store.load(&entry.path) {
            Ok(preset) => {
                self.apply_preset(preset);
                self.preset.folder = Some(entry.folder.clone());
                self.preset.set_current(Some(entry.clone()));
                self.notify(format!("Loaded {}", entry.name), now);
            }
            Err(error) => {
                nice_plug::nice_error!("EQuart could not load a preset: {error}");
                self.notify(format!("Could not load {}", entry.name), now);
            }
        }
    }

    fn apply_preset(&mut self, preset: Preset) {
        self.restore_plugin_state(preset.into_state(&self.params));
        self.comparison.reset();
    }

    fn sound_state(&self) -> PluginState {
        let state = match &self.gui {
            Some(gui) => gui.get_state(),
            None => crate::presets::state_of(&self.params),
        };

        crate::presets::sound_state(state)
    }

    fn restore_plugin_state(&mut self, state: PluginState) {
        match &self.gui {
            Some(gui) => gui.set_state(state),
            None => crate::presets::restore(&self.params, &state),
        }

        self.band_conversion_baselines.fill(None);
        self.region_conversion_baselines.fill(None);
        self.clear_selection();
        self.held_delta = None;
        self.held_audition = None;
        self.reveal_modifier = None;
        self.dial.shut();
    }

    fn switch_comparison(&mut self, slot: ComparisonSlot) {
        if self.comparison.active == slot {
            return;
        }

        let state = self.sound_state();
        let target = slot.index();

        if self.comparison.states[target].is_none() {
            self.comparison.states[target] = Some(state.clone());
        }

        self.comparison.states[self.comparison.active.index()] = Some(state);

        let state = self.comparison.states[target]
            .clone()
            .expect("comparison slot initialized before restore");

        self.restore_plugin_state(state);
        self.comparison.active = slot;
    }
}

fn browser_card<'a>(button: &'a Response) -> egui::Popup<'a> {
    egui::Popup::menu(button)
        .open(true)
        .align(egui::emath::RectAlign::TOP_END)
        .gap(5.0)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .frame(egui::Frame::NONE)
}

fn plate(
    ui: &mut egui::Ui,
    salt: impl std::hash::Hash + std::fmt::Debug,
) -> (Response, Rect, Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(TOOL_SIZE), Sense::hover());
    let response = ui
        .interact(rect, ui.make_persistent_id(salt), Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    let palette = theme::current(ui.ctx());
    let hover = hover_amount(&response);

    if hover > 0.0 {
        ui.painter().rect_filled(
            rect.shrink(1.0),
            5.0,
            menu::fade(palette.panel.hover, hover),
        );
    }

    let ink = palette
        .text
        .muted
        .lerp_to_gamma(palette.text.primary, 0.2 + 0.8 * hover);

    (response, rect, ink)
}

fn icon_plate(
    ui: &mut egui::Ui,
    salt: impl std::hash::Hash + std::fmt::Debug,
    glyph: icons::Glyph,
) -> Response {
    let (response, rect, ink) = plate(ui, salt);
    icons::draw(
        ui.painter(),
        ui.ctx(),
        glyph,
        Rect::from_center_size(rect.center(), Vec2::splat(13.0)),
        ink,
    );

    response
}

fn search_button(ui: &mut egui::Ui) -> Response {
    let (response, rect, ink) = plate(ui, ("preset-tool", 2));
    let p = ui.painter();
    let centre = rect.center() - Vec2::new(1.0, 1.0);
    let stroke = Stroke::new(1.4, ink);
    p.circle_stroke(centre, 4.0, stroke);
    p.line_segment(
        [centre + Vec2::new(2.9, 2.9), centre + Vec2::new(6.2, 6.2)],
        stroke,
    );

    response.on_hover_text("Search all folders")
}

fn elide(painter: &egui::Painter, text: &str, font: FontId, width: f32) -> String {
    let measure = |text: &str| {
        painter
            .layout_no_wrap(text.to_owned(), font.clone(), Color32::WHITE)
            .size()
            .x
    };

    if measure(text) <= width {
        return text.to_owned();
    }

    let mut keep: String = String::new();

    for ch in text.chars() {
        let mut candidate = keep.clone();
        candidate.push(ch);
        candidate.push('…');

        if measure(&candidate) > width {
            break;
        }

        keep.push(ch);
    }

    keep.push('…');

    keep
}
