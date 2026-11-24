use super::*;
use crate::region::{REGION_COUNT, dynamics::Curve};
use nice_plug::{context::gui::GuiContextInner, params::internals::ParamPtr};
use std::sync::Mutex;

pub(super) const UNDO: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::Z);
pub(super) const REDO: egui::KeyboardShortcut = egui::KeyboardShortcut::new(
    egui::Modifiers::COMMAND.plus(egui::Modifiers::SHIFT),
    egui::Key::Z,
);

const HISTORY_LENGTH: usize = 100;

#[derive(Clone, Copy)]
pub(super) enum Direction {
    Undo,
    Redo,
}

#[derive(Default)]
pub(super) struct History {
    pending: Edit,
    undo: Vec<Change>,
    redo: Vec<Change>,
}

enum Change {
    Edit(Box<Edit>),
    Recall(Box<(Recall, Recall)>),
}

pub(super) struct Recall {
    pub state: PluginState,
    pub preset: Option<crate::presets::Entry>,
    pub comparison: presets::Comparison,
}

#[derive(Default)]
struct Edit {
    params: Vec<ParamEdit>,
    placed: (u32, u32),
    curves: [Option<(Curve, Curve)>; REGION_COUNT],
}

struct ParamEdit {
    param: ParamPtr,
    before: f32,
    after: f32,
}

pub(super) struct SoundFields {
    placed: u32,
    curves: [Option<Curve>; REGION_COUNT],
}

impl SoundFields {
    pub fn read(params: &EquartParams) -> Self {
        Self {
            placed: params.bands_placed.load(Ordering::Relaxed),
            curves: std::array::from_fn(|index| params.spectral_transfer_curves.read(index)),
        }
    }
}

impl History {
    pub fn capture_fields(&mut self, before: SoundFields, after: SoundFields) {
        let changed = before.placed ^ after.placed;
        let (first, last) = &mut self.pending.placed;
        let new = changed & !(*first ^ *last);
        *first = (*first & !new) | (before.placed & new);
        *last = (*last & !changed) | (after.placed & changed);

        for (index, (before, after)) in before.curves.into_iter().zip(after.curves).enumerate() {
            if let (Some(before), Some(after)) = (before, after)
                && before != after
            {
                let change = self.pending.curves[index].get_or_insert((before, before));
                change.1 = after;
            }
        }
    }

    pub fn finish_edit(&mut self) {
        let mut edit = std::mem::take(&mut self.pending);
        edit.params.retain(|change| change.before != change.after);

        if !edit.params.is_empty()
            || edit.placed.0 != edit.placed.1
            || edit.curves.iter().flatten().any(|(a, b)| a != b)
        {
            self.push(Change::Edit(Box::new(edit)));
        }
    }

    pub fn record_recall(&mut self, before: Recall, after: Recall) {
        self.finish_edit();
        self.push(Change::Recall(Box::new((before, after))));
    }

    fn push(&mut self, change: Change) {
        if self.undo.len() == HISTORY_LENGTH {
            self.undo.remove(0);
        }

        self.undo.push(change);
        self.redo.clear();
    }

    pub fn available(&self, direction: Direction) -> bool {
        match direction {
            Direction::Undo => !self.undo.is_empty(),
            Direction::Redo => !self.redo.is_empty(),
        }
    }

    fn take_step(&mut self, direction: Direction) -> Option<Change> {
        match direction {
            Direction::Undo => self.undo.pop(),
            Direction::Redo => self.redo.pop(),
        }
    }

    fn finish_step(&mut self, direction: Direction, change: Change) {
        match direction {
            Direction::Undo => self.redo.push(change),
            Direction::Redo => self.undo.push(change),
        }
    }
}

impl Direction {
    fn value<T>(self, before: T, after: T) -> T {
        match self {
            Self::Undo => before,
            Self::Redo => after,
        }
    }
}

pub(super) struct RecordingGui {
    pub host: GuiContext,
    pub history: Arc<Mutex<History>>,
    _params: Arc<EquartParams>,
}

impl GuiContextInner for RecordingGui {
    fn plugin_api(&self) -> PluginApi {
        self.host.plugin_api()
    }

    unsafe fn raw_begin_set_parameter(&self, param: ParamPtr) {
        unsafe { self.host.raw_begin_set_parameter(param) };
    }

    unsafe fn raw_set_parameter_normalized(&self, param: ParamPtr, normalized: f32) {
        {
            let mut history = self.history.lock().unwrap();
            let changes = &mut history.pending.params;

            if let Some(change) = changes.iter_mut().find(|change| change.param == param) {
                change.after = normalized;
            } else {
                // the editor supplies pointers into the retained parameter allocation
                let before = unsafe { param.unmodulated_normalized_value() };
                changes.push(ParamEdit {
                    param,
                    before,
                    after: normalized,
                });
            }
        }

        unsafe { self.host.raw_set_parameter_normalized(param, normalized) };
    }

    unsafe fn raw_end_set_parameter(&self, param: ParamPtr) {
        unsafe { self.host.raw_end_set_parameter(param) };
    }

    fn get_state(&self) -> PluginState {
        self.host.get_state()
    }

    fn set_state(&self, state: PluginState) {
        self.host.set_state(state);
    }

    fn request_restart(&self) {
        self.host.request_restart();
    }
}

impl EquartEditor {
    pub(super) fn attach_history(&mut self, host: GuiContext) {
        self.gui = Some(GuiContext::new(Arc::new(RecordingGui {
            host: host.clone(),
            history: Arc::clone(&self.history),
            _params: Arc::clone(&self.params),
        })));
        self.history_host = Some(host);
    }

    pub(super) fn history_shortcut(&mut self, ctx: &egui::Context) {
        let focused = ctx.memory(|memory| memory.focused());

        if self.about.is_shown()
            || self.conversion_dialog.is_some()
            || self.theme_preset_dialog.is_some()
            || focused.is_some_and(|id| egui::text_edit::TextEditState::load(ctx, id).is_some())
        {
            return;
        }

        let mac = ctx.os() == egui::os::OperatingSystem::Mac;
        self.history_action = ctx.input_mut(|input| {
            if input.consume_shortcut(&REDO)
                || (!mac && input.consume_key(egui::Modifiers::CTRL, egui::Key::Y))
            {
                Some(Direction::Redo)
            } else if input.consume_shortcut(&UNDO) {
                Some(Direction::Undo)
            } else {
                None
            }
        });
    }

    pub(super) fn step_history(&mut self, direction: Direction) {
        let Some(host) = self.history_host.clone() else {
            return;
        };
        let Some(change) = self.history.lock().unwrap().take_step(direction) else {
            return;
        };

        match &change {
            Change::Edit(edit) => {
                let mut changes = edit.params.iter();
                let mut removed_region = false;

                while let Some(&ParamEdit {
                    param,
                    before,
                    after,
                }) = match direction {
                    Direction::Undo => changes.next_back(),
                    Direction::Redo => changes.next(),
                } {
                    let value = direction.value(before, after);
                    removed_region |= value == 0.0
                        && self.selected_region.is_some_and(|index| {
                            self.params.regions[index].enabled.as_ptr() == param
                        });

                    // the history only holds pointers into this instance's live params
                    unsafe {
                        host.raw_begin_set_parameter(param);
                        host.raw_set_parameter_normalized(param, value);
                        host.raw_end_set_parameter(param);
                    }
                }

                let changed = edit.placed.0 ^ edit.placed.1;
                let placed = direction.value(edit.placed.0, edit.placed.1);

                if changed != 0 {
                    let _ = self.params.bands_placed.fetch_update(
                        Ordering::Relaxed,
                        Ordering::Relaxed,
                        |current| Some((current & !changed) | (placed & changed)),
                    );
                }

                for (index, curve) in edit.curves.iter().enumerate() {
                    if let Some((before, after)) = *curve
                        && before != after
                    {
                        self.params
                            .spectral_transfer_curves
                            .edit(index, direction.value(before, after));
                    }
                }

                let removed = changed & !placed;

                if removed_region
                    || self
                        .selected
                        .is_some_and(|index| removed & (1 << index) != 0)
                {
                    self.clear_selection();
                } else {
                    for (index, picked) in self.picked.iter_mut().enumerate() {
                        *picked &= removed & (1 << index) == 0;
                    }
                }
            }
            Change::Recall(recall) => {
                let recall = direction.value(&recall.0, &recall.1);
                self.restore_plugin_state(recall.state.clone());
                self.preset.set_current(recall.preset.clone());
                self.comparison = recall.comparison.clone();
            }
        }

        self.history.lock().unwrap().finish_step(direction, change);
    }
}
