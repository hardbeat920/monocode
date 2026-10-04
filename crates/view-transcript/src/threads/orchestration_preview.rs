//! Port of src/features/orchestration/ui/OrchestrationPreview.tsx: the
//! orchestrator's assignment card inside the transcript. The lead proposes
//! tasks and worker models; the user can change each task's model and
//! effort, edit titles and instructions, pick how many workers run at once,
//! and start the run with Confirm & start.
//!
//! The card reaches the engine through [`OrchestrationActions`] (edits,
//! confirm, retry, View agents) and [`OrchestrationRuns`] (the live run and
//! hydration).

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    Animation, AnimationExt as _, AnyElement, App, AppContext as _, Context, Entity,
    InteractiveElement as _, IntoElement, MouseDownEvent, ParentElement as _, Render, SharedString,
    StatefulInteractiveElement as _, Styled as _, Subscription, Task, Transformation, WeakEntity,
    Window, div, percentage,
};
use gpui_base::input::{InputBaseState, InputEditorStyle, InputModeKind};
use gpui_component::input::{
    Enter, InputEvent, InputState, MoveDown, MoveLeft, MoveRight, MoveUp, TextareaState,
};
use monocode_core::block::{Block, ModelSettings};
use monocode_core::models::{
    AgentModel, ModelCatalog, ModelSetting, model_effort_label, model_effort_setting,
};
use monocode_core::orchestration::{
    OrchestrationChoice, OrchestrationProposal, OrchestrationProposalStatus, ProposedTask,
};
use monocode_ui::widgets::tooltip;
use monocode_ui::{IconName, Theme, UiStyled as _, icon, u};
use monocode_view_composer::pickers::anchor::{BoundsCell, popover_surface};

use super::actions::{
    OrchestrationActions, OrchestrationRunView, OrchestrationRuns, OrchestrationWorkerDetail,
};
use super::parts::{Placement, anchored_popover, eid, flip, harness_icon};

/// Tasks shown before "Show N more tasks".
const VISIBLE_TASKS: usize = 3;

/// The open assignment model picker (`AssignmentModel`).
struct AssignmentPicker {
    task_id: String,
    query: String,
    active: usize,
    effort_active: usize,
    in_effort: bool,
}

/// One task's editable fields.
struct TaskFields {
    title: Entity<InputState>,
    prompt: Entity<TextareaState>,
    _subscriptions: [Subscription; 2],
}

/// The card.
pub struct OrchestrationPreview {
    block_id: String,
    proposal: OrchestrationProposal,
    busy: bool,
    runs: Rc<dyn OrchestrationRuns>,
    actions: Option<Rc<dyn OrchestrationActions>>,
    catalog: Arc<ModelCatalog>,
    pending: bool,
    error: Option<String>,
    expanded: Vec<String>,
    show_all: bool,
    picker: Option<AssignmentPicker>,
    search: Entity<InputState>,
    fields: HashMap<String, TaskFields>,
    help_hovered: bool,
    help_open: bool,
    picker_bounds: BoundsCell,
    effort_bounds: BoundsCell,
    trigger_bounds: BoundsCell,
    row_bounds: BoundsCell,
    animate: bool,
    operation: Option<Task<()>>,
    hydration: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl OrchestrationPreview {
    /// A card for `block`, which must carry an orchestration proposal.
    pub fn new(
        block: &Block,
        runs: Rc<dyn OrchestrationRuns>,
        actions: Option<Rc<dyn OrchestrationActions>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let proposal = block.orchestration.clone().expect("an orchestration block");
        let search =
            cx.new(|cx| InputState::new(window, cx).placeholder("Search models or harnesses…"));
        let weak = cx.entity().downgrade();
        let runs_subscription = runs.observe(
            Box::new(move |cx| {
                weak.update(cx, |_, cx| cx.notify()).ok();
            }),
            cx,
        );
        let search_subscription = cx.subscribe_in(
            &search,
            window,
            |this, search, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::Change) {
                    let query = search.read(cx).value().to_string();
                    if let Some(picker) = &mut this.picker {
                        picker.query = query;
                        picker.active = 0;
                        picker.in_effort = false;
                    }
                    this.sync_effort_active();
                    cx.notify();
                }
            },
        );
        let mut this = Self {
            block_id: block.id.clone(),
            proposal,
            busy: false,
            runs,
            actions,
            catalog: Arc::new(ModelCatalog::new()),
            pending: false,
            error: None,
            expanded: Vec::new(),
            show_all: false,
            picker: None,
            search,
            fields: HashMap::new(),
            help_hovered: false,
            help_open: false,
            picker_bounds: BoundsCell::default(),
            effort_bounds: BoundsCell::default(),
            trigger_bounds: BoundsCell::default(),
            row_bounds: BoundsCell::default(),
            animate: true,
            operation: None,
            hydration: None,
            _subscriptions: vec![runs_subscription, search_subscription],
        };
        this.hydrate(cx);
        this
    }

    /// A new snapshot of the block, such as an edit the lead saved.
    pub fn set_block(&mut self, block: &Block, window: &mut Window, cx: &mut Context<Self>) {
        let Some(proposal) = block.orchestration.clone() else {
            return;
        };
        let lead_changed = proposal.lead_id != self.proposal.lead_id;
        self.block_id = block.id.clone();
        self.proposal = proposal;
        self.sync_fields(window, cx);
        if lead_changed {
            self.hydrate(cx);
        }
        cx.notify();
    }

    pub fn set_busy(&mut self, busy: bool, cx: &mut Context<Self>) {
        self.busy = busy;
        cx.notify();
    }

    pub fn set_actions(
        &mut self,
        actions: Option<Rc<dyn OrchestrationActions>>,
        cx: &mut Context<Self>,
    ) {
        let had = self.actions.is_some();
        self.actions = actions;
        if !had && self.actions.is_some() {
            self.hydrate(cx);
        }
        cx.notify();
    }

    /// The catalog that names models and their effort settings.
    pub fn set_catalog(&mut self, catalog: Arc<ModelCatalog>, cx: &mut Context<Self>) {
        self.catalog = catalog;
        cx.notify();
    }

    /// Turns popover animations off, for screenshots.
    pub fn set_animate(&mut self, animate: bool) {
        self.animate = animate;
    }

    pub fn proposal(&self) -> &OrchestrationProposal {
        &self.proposal
    }

    /// `orchestrator.hydrate(leadId)` whenever the card has actions.
    fn hydrate(&mut self, cx: &mut Context<Self>) {
        if self.actions.is_none() {
            return;
        }
        let task = self.runs.hydrate(&self.proposal.lead_id, cx);
        self.hydration = Some(cx.spawn(async move |this: WeakEntity<Self>, cx| {
            if let Err(error) = task.await {
                this.update(cx, |this, cx| {
                    this.error = Some(error);
                    cx.notify();
                })
                .ok();
            }
        }));
    }

    // Derived state.

    /// The live run this card started, if any.
    pub fn run(&self, cx: &App) -> Option<OrchestrationRunView> {
        self.runs.runs(cx).into_iter().find(|run| {
            run.lead_id == self.proposal.lead_id
                && run.proposal_id.as_deref() == Some(self.block_id.as_str())
        })
    }

    /// The card can still be edited and started.
    pub fn editable(&self, cx: &App) -> bool {
        self.proposal.status == OrchestrationProposalStatus::Ready
            && self.run(cx).is_none()
            && !self.pending
            && !self.busy
            && self.actions.is_some()
    }

    fn planning(&self) -> bool {
        self.proposal.status == OrchestrationProposalStatus::Planning
    }

    fn starting(&self) -> bool {
        self.pending || self.proposal.status == OrchestrationProposalStatus::Starting
    }

    /// The card's title line.
    pub fn title(&self) -> String {
        if self.planning() {
            "Planning assignments…".into()
        } else {
            self.proposal.title.clone()
        }
    }

    /// The header's action: Try again, Confirm & start (or Starting…), or
    /// View agents.
    pub fn primary_action(&self, cx: &App) -> Option<&'static str> {
        if self.run(cx).is_some() {
            return Some("View agents");
        }
        match self.proposal.status {
            OrchestrationProposalStatus::Invalid => Some("Try again"),
            OrchestrationProposalStatus::Ready | OrchestrationProposalStatus::Starting => {
                Some(if self.starting() {
                    "Starting…"
                } else {
                    "Confirm & start"
                })
            }
            _ => None,
        }
    }

    /// The tasks on screen.
    pub fn visible_tasks(&self) -> &[ProposedTask] {
        if self.show_all {
            &self.proposal.tasks
        } else {
            &self.proposal.tasks[..self.proposal.tasks.len().min(VISIBLE_TASKS)]
        }
    }

    /// The "Show N more tasks" button's label, when there are more than three.
    pub fn show_more_label(&self) -> Option<String> {
        let count = self.proposal.tasks.len();
        (count > VISIBLE_TASKS).then(|| {
            if self.show_all {
                "Show fewer tasks".into()
            } else {
                format!(
                    "Show {} more {}",
                    count - VISIBLE_TASKS,
                    if count == 4 { "task" } else { "tasks" }
                )
            }
        })
    }

    fn choice_for(&self, task: &ProposedTask) -> Option<&OrchestrationChoice> {
        self.proposal
            .settings
            .choices
            .iter()
            .find(|choice| choice.harness == task.harness && choice.model == task.model)
    }

    fn catalog_model(&self, harness: monocode_core::HarnessId, id: &str) -> Option<AgentModel> {
        self.catalog
            .find_model(id)
            .filter(|model| model.harness == harness)
            .cloned()
    }

    /// A task's model line: the choice's name (or the raw id) and its effort.
    pub fn model_line(&self, task: &ProposedTask) -> String {
        let name = self
            .choice_for(task)
            .map(|choice| choice.name.clone())
            .unwrap_or_else(|| task.model.clone());
        let effort = self
            .catalog_model(task.harness, &task.model)
            .and_then(|model| model_effort_label(&model, task.model_settings.as_ref()));
        match effort {
            Some(effort) => format!("{name} · {effort}"),
            None => name,
        }
    }

    /// The footer's status line.
    pub fn status_line(&self, cx: &App) -> String {
        let status = match self.run(cx) {
            Some(run) => run.status.as_str().to_string(),
            None => match self.proposal.status {
                OrchestrationProposalStatus::Approved => "Approved".into(),
                OrchestrationProposalStatus::Ready => "Awaiting confirmation".into(),
                _ => String::new(),
            },
        };
        format!("{status} · Shared project folder")
    }

    /// The error under the tasks: this card's, else the proposal's.
    pub fn error(&self) -> Option<String> {
        self.error.clone().or_else(|| self.proposal.error.clone())
    }

    // Edits.

    fn change(&mut self, id: &str, patch: impl FnOnce(&mut ProposedTask), cx: &mut Context<Self>) {
        self.error = None;
        let mut next = self.proposal.clone();
        if let Some(task) = next.tasks.iter_mut().find(|task| task.id == id) {
            patch(task);
        }
        if let Some(actions) = &self.actions {
            actions.update(&next.lead_id.clone(), &self.block_id, next, cx);
        }
        cx.notify();
    }

    pub fn change_title(&mut self, task_id: &str, title: &str, cx: &mut Context<Self>) {
        let title = title.to_string();
        self.change(task_id, move |task| task.title = title, cx);
    }

    pub fn change_prompt(&mut self, task_id: &str, prompt: &str, cx: &mut Context<Self>) {
        let prompt = prompt.to_string();
        self.change(task_id, move |task| task.prompt = prompt, cx);
    }

    pub fn set_max_workers(&mut self, workers: i64, cx: &mut Context<Self>) {
        let mut next = self.proposal.clone();
        next.settings.max_workers = workers;
        if let Some(actions) = &self.actions {
            actions.update(&next.lead_id.clone(), &self.block_id, next, cx);
        }
        cx.notify();
    }

    pub fn toggle_details(&mut self, task_id: &str, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(index) = self.expanded.iter().position(|id| id == task_id) {
            self.expanded.remove(index);
        } else {
            self.expanded.push(task_id.to_string());
        }
        self.sync_fields(window, cx);
        cx.notify();
    }

    pub fn is_expanded(&self, task_id: &str) -> bool {
        self.expanded.iter().any(|id| id == task_id)
    }

    pub fn toggle_show_all(&mut self, cx: &mut Context<Self>) {
        self.show_all = !self.show_all;
        cx.notify();
    }

    /// Confirm & start.
    pub fn confirm(&mut self, cx: &mut Context<Self>) {
        if !self.editable(cx) {
            return;
        }
        let Some(actions) = self.actions.clone() else {
            return;
        };
        self.pending = true;
        self.error = None;
        let task = actions.confirm(&self.proposal.lead_id, &self.block_id, cx);
        self.operation = Some(cx.spawn(async move |this: WeakEntity<Self>, cx| {
            let result = task.await;
            this.update(cx, |this, cx| {
                this.pending = false;
                if let Err(error) = result {
                    this.error = Some(error);
                }
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }

    /// Try again, on an invalid card.
    pub fn retry(&mut self, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        if let Some(actions) = &self.actions {
            actions.retry(&self.proposal.lead_id, &self.block_id, cx);
        }
    }

    /// View agents: every worker of the run as tabs beside the lead.
    pub fn view_agents(&mut self, cx: &mut Context<Self>) {
        let (Some(run), Some(actions)) = (self.run(cx), self.actions.clone()) else {
            return;
        };
        if !actions.can_open_agents() {
            return;
        }
        let workers = run
            .tasks
            .iter()
            .map(|task| OrchestrationWorkerDetail {
                session_id: task.session_id.clone(),
                lead_id: run.lead_id.clone(),
                title: task.title.clone(),
                harness: task.harness,
            })
            .collect();
        actions.open_agents(workers, cx);
    }

    /// Keeps each expanded task's inputs in step with the proposal.
    fn sync_fields(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let editable = self.editable(cx);
        let tasks = self.proposal.tasks.clone();
        self.fields
            .retain(|id, _| editable && tasks.iter().any(|task| &task.id == id));
        if !editable {
            return;
        }
        for task in &tasks {
            if !self.is_expanded(&task.id) {
                continue;
            }
            match self.fields.get(&task.id) {
                Some(fields) => {
                    if fields.title.read(cx).value() != task.title.as_str() {
                        let title = task.title.clone();
                        fields
                            .title
                            .update(cx, |input, cx| input.set_value(title, window, cx));
                    }
                    if fields.prompt.read(cx).value() != task.prompt.as_str() {
                        let prompt = task.prompt.clone();
                        fields
                            .prompt
                            .update(cx, |input, cx| input.set_value(prompt, window, cx));
                    }
                }
                None => {
                    let fields = self.task_fields(task, window, cx);
                    self.fields.insert(task.id.clone(), fields);
                }
            }
        }
    }

    fn task_fields(
        &mut self,
        task: &ProposedTask,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> TaskFields {
        let title_value = task.title.clone();
        let title = cx.new(|cx| {
            let mut input = InputState::new(window, cx);
            input.set_value(title_value, window, cx);
            input
        });
        let prompt_value = task.prompt.clone();
        // The same growth the composer uses: fit the text, stop at
        // `max-h-40` (eight 20px lines) and scroll from there.
        let prompt = cx.new(|cx| {
            let mut input = TextareaState::new(window, cx).auto_grow(1, 8);
            input.set_value(prompt_value, window, cx);
            input
        });
        let id = task.id.clone();
        let title_events = cx.subscribe_in(&title, window, move |this, input, event, _, cx| {
            if matches!(event, InputEvent::Change) {
                let value = input.read(cx).value().to_string();
                this.change_title(&id, &value, cx);
            }
        });
        let id = task.id.clone();
        let prompt_events = cx.subscribe_in(&prompt, window, move |this, input, event, _, cx| {
            if matches!(event, InputEvent::Change) {
                let value = input.read(cx).value().to_string();
                this.change_prompt(&id, &value, cx);
            }
        });
        TaskFields {
            title,
            prompt,
            _subscriptions: [title_events, prompt_events],
        }
    }

    // The assignment model picker.

    fn picker_task(&self) -> Option<&ProposedTask> {
        let picker = self.picker.as_ref()?;
        self.proposal
            .tasks
            .iter()
            .find(|task| task.id == picker.task_id)
    }

    /// The choices matching the picker's query.
    pub fn assignment_matches(&self) -> Vec<OrchestrationChoice> {
        let query = self
            .picker
            .as_ref()
            .map(|picker| monocode_core::js::trim(&picker.query).to_lowercase())
            .unwrap_or_default();
        self.proposal
            .settings
            .choices
            .iter()
            .filter(|choice| {
                format!(
                    "{} {} {}",
                    choice.name,
                    choice.model,
                    choice.harness.title()
                )
                .to_lowercase()
                .contains(&query)
            })
            .cloned()
            .collect()
    }

    fn effort_for_choice(&self, choice: &OrchestrationChoice) -> Option<ModelSetting> {
        let model = self.catalog_model(choice.harness, &choice.model)?;
        model_effort_setting(&model)
            .filter(|setting| !setting.options.is_empty())
            .cloned()
    }

    fn is_selected(task: &ProposedTask, choice: &OrchestrationChoice) -> bool {
        choice.harness == task.harness && choice.model == task.model
    }

    fn picker_active_choice(&self) -> Option<OrchestrationChoice> {
        let picker = self.picker.as_ref()?;
        self.assignment_matches().get(picker.active).cloned()
    }

    fn selected_effort_value(
        &self,
        task: &ProposedTask,
        choice: &OrchestrationChoice,
        effort: &ModelSetting,
    ) -> String {
        if Self::is_selected(task, choice) {
            task.model_settings
                .as_ref()
                .and_then(|settings| settings.get(&effort.id))
                .cloned()
                .unwrap_or_else(|| effort.value.clone())
        } else {
            effort.value.clone()
        }
    }

    /// `[effort, selectedEffortValue]`: highlight the selected effort.
    fn sync_effort_active(&mut self) {
        let index = (|| {
            let task = self.picker_task()?;
            let choice = self.picker_active_choice()?;
            let effort = self.effort_for_choice(&choice)?;
            let selected = self.selected_effort_value(task, &choice, &effort);
            effort
                .options
                .iter()
                .position(|option| option.value == selected)
        })()
        .unwrap_or(0);
        if let Some(picker) = &mut self.picker {
            picker.effort_active = index;
        }
    }

    /// Opens or closes the model picker of `task_id` (the trigger's click).
    pub fn toggle_assignment(
        &mut self,
        task_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self
            .picker
            .as_ref()
            .is_some_and(|picker| picker.task_id == task_id)
        {
            self.picker = None;
            cx.notify();
            return;
        }
        let Some(task) = self.proposal.tasks.iter().find(|task| task.id == task_id) else {
            return;
        };
        let active = self
            .proposal
            .settings
            .choices
            .iter()
            .position(|choice| Self::is_selected(task, choice))
            .unwrap_or(0);
        self.picker = Some(AssignmentPicker {
            task_id: task_id.to_string(),
            query: String::new(),
            active,
            effort_active: 0,
            in_effort: false,
        });
        self.search
            .update(cx, |search, cx| search.set_value("", window, cx));
        self.sync_effort_active();
        // The search field takes focus so arrows and typing reach it.
        self.search
            .update(cx, |search, cx| search.focus(window, cx));
        cx.notify();
    }

    pub fn close_assignment(&mut self, cx: &mut Context<Self>) {
        self.picker = None;
        cx.notify();
    }

    /// Types into the picker's search field.
    pub fn set_assignment_query(
        &mut self,
        query: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let query = query.to_string();
        self.search
            .update(cx, |search, cx| search.set_value(query.clone(), window, cx));
        if let Some(picker) = &mut self.picker {
            picker.query = query;
            picker.active = 0;
            picker.in_effort = false;
        }
        self.sync_effort_active();
        cx.notify();
    }

    /// The open picker's effort menu: the choice and its effort setting.
    pub fn assignment_effort_menu(&self) -> Option<(OrchestrationChoice, ModelSetting)> {
        let picker = self.picker.as_ref()?;
        if !picker.in_effort {
            return None;
        }
        let choice = self.picker_active_choice()?;
        let effort = self.effort_for_choice(&choice)?;
        Some((choice, effort))
    }

    fn settings_for(
        &self,
        task: &ProposedTask,
        choice: &OrchestrationChoice,
        effort_value: Option<&str>,
    ) -> ModelSettings {
        let Some(model) = self.catalog_model(choice.harness, &choice.model) else {
            return ModelSettings::new();
        };
        let mut current = ModelSettings::new();
        if Self::is_selected(task, choice)
            && let Some(settings) = &task.model_settings
        {
            current.extend(settings.clone());
        }
        if let (Some(setting), Some(value)) = (model_effort_setting(&model), effort_value) {
            current.insert(setting.id.clone(), value.to_string());
        }
        self.catalog.merge_model_settings(&model, Some(&current))
    }

    fn pick_assignment(
        &mut self,
        choice: OrchestrationChoice,
        effort_value: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(task) = self.picker_task().cloned() else {
            return;
        };
        let model_settings = self.settings_for(&task, &choice, effort_value.as_deref());
        self.picker = None;
        self.change(
            &task.id,
            move |task| {
                task.harness = choice.harness;
                task.model = choice.model;
                task.model_settings = Some(model_settings);
            },
            cx,
        );
        let _ = window;
    }

    fn open_effort_or_pick(
        &mut self,
        choice: OrchestrationChoice,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.effort_for_choice(&choice).is_some() {
            if let Some(picker) = &mut self.picker {
                picker.in_effort = true;
            }
            self.sync_effort_active();
            cx.notify();
            return;
        }
        self.pick_assignment(choice, None, window, cx);
    }

    /// The pointer entered option `index`.
    pub fn hover_assignment(&mut self, index: usize, cx: &mut Context<Self>) {
        let has_effort = self
            .assignment_matches()
            .get(index)
            .is_some_and(|choice| self.effort_for_choice(choice).is_some());
        if let Some(picker) = &mut self.picker {
            picker.active = index;
            picker.in_effort = has_effort;
        }
        self.sync_effort_active();
        cx.notify();
    }

    /// A click on option `index`.
    pub fn click_assignment(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(picker) = &mut self.picker {
            picker.active = index;
        }
        self.sync_effort_active();
        if let Some(choice) = self.assignment_matches().get(index).cloned() {
            self.open_effort_or_pick(choice, window, cx);
        }
    }

    pub fn hover_assignment_effort(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some(picker) = &mut self.picker {
            picker.effort_active = index;
        }
        cx.notify();
    }

    /// Picks effort option `index` for the highlighted choice.
    pub fn pick_assignment_effort(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((choice, effort)) = self.assignment_effort_menu() else {
            return;
        };
        if let Some(option) = effort.options.get(index) {
            let value = option.value.clone();
            self.pick_assignment(choice, Some(value), window, cx);
        }
    }

    /// `onSearchKey`: arrows walk the list, Right and Left move between the
    /// list and the effort menu, Enter opens or picks.
    pub fn assignment_key(
        &mut self,
        key: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(picker) = &self.picker else {
            return false;
        };
        let in_effort = picker.in_effort;
        let effort_active = picker.effort_active;
        let effort = self
            .picker_active_choice()
            .and_then(|choice| self.effort_for_choice(&choice));
        match key {
            "down" | "up" => {
                let step: isize = if key == "down" { 1 } else { -1 };
                if in_effort && let Some(effort) = &effort {
                    let len = effort.options.len() as isize;
                    if let Some(picker) = &mut self.picker {
                        picker.effort_active =
                            ((picker.effort_active as isize + step + len) % len) as usize;
                    }
                } else {
                    let len = self.assignment_matches().len();
                    if len == 0 {
                        return true;
                    }
                    if let Some(picker) = &mut self.picker {
                        picker.active =
                            (picker.active as isize + step).clamp(0, len as isize - 1) as usize;
                    }
                    self.sync_effort_active();
                }
            }
            "right" => {
                if effort.is_some()
                    && let Some(picker) = &mut self.picker
                {
                    picker.in_effort = true;
                }
            }
            "left" => {
                if !in_effort {
                    return false;
                }
                if let Some(picker) = &mut self.picker {
                    picker.in_effort = false;
                }
            }
            "enter" => {
                if in_effort {
                    self.pick_assignment_effort(effort_active, window, cx);
                    return true;
                }
                if let Some(choice) = self.picker_active_choice() {
                    self.open_effort_or_pick(choice, window, cx);
                }
            }
            _ => return false,
        }
        cx.notify();
        true
    }

    fn press_is_inside(&self, event: &MouseDownEvent) -> bool {
        self.trigger_bounds.contains(event.position) || self.effort_bounds.contains(event.position)
    }

    // Drawing.

    fn render_assignment(
        &self,
        task: &ProposedTask,
        theme: &Theme,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let open = self
            .picker
            .as_ref()
            .is_some_and(|picker| picker.task_id == task.id);
        let hover = theme.content(0.10);
        let ink = theme.colors.content;
        let task_id = task.id.clone();
        let mut trigger =
            div()
                .id(eid(&task.id, "model"))
                .flex()
                .h(u(28.))
                .max_w_full()
                .items_center()
                .gap(u(6.))
                .rounded(u(theme.radius.md))
                .bg(theme.content(0.05))
                .px(u(8.))
                .text_px(11.)
                .text_color(theme.content(0.60))
                .hover(move |style| style.bg(hover).text_color(ink))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.toggle_assignment(&task_id, window, cx)
                }))
                .child(harness_icon(task.harness, 14.))
                .child(div().min_w_0().truncate().child(format!(
                    "{} · {}",
                    self.model_line(task),
                    task.harness.title()
                )))
                .child(
                    icon(IconName::ChevronDown)
                        .size(u(12.))
                        .text_color(theme.content(0.60)),
                );
        if open {
            trigger = trigger.child(self.trigger_bounds.probe());
        }
        let mut root = div().relative().min_w_0().child(trigger);
        if open {
            let side = flip(
                Placement::BottomStart,
                self.trigger_bounds.get(),
                self.picker_bounds
                    .get()
                    .map_or(gpui::Size::default(), |bounds| bounds.size),
                monocode_ui::widgets::POPOVER_GAP,
                window,
            );
            root = root.child(anchored_popover(
                side,
                monocode_ui::widgets::POPOVER_GAP,
                theme.layer.popover,
                window,
                self.render_picker(task, theme, window, cx),
            ));
        }
        root.into_any_element()
    }

    fn render_picker(
        &self,
        task: &ProposedTask,
        theme: &Theme,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(picker) = &self.picker else {
            return div().into_any_element();
        };
        let matches = self.assignment_matches();
        let effort_menu = self.assignment_effort_menu();
        let search_style = InputEditorStyle {
            foreground: theme.colors.content,
            muted_foreground: theme.content(0.40),
            background: gpui::transparent_black(),
            border: gpui::transparent_black(),
            selection: theme.accent(0.35),
            caret: theme.colors.content,
            ..Default::default()
        };
        let search = styled_input(&self.search, search_style.clone(), cx);
        let search_row = div()
            .flex()
            .flex_none()
            .items_center()
            .gap(u(8.))
            .border_b_1()
            .border_color(theme.colors.stroke)
            .px(u(12.))
            .py(u(10.))
            .child(
                icon(IconName::Search)
                    .size(u(14.))
                    .text_color(theme.content(0.50)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h(u(18.))
                    .flex()
                    .items_center()
                    .text_px(13.)
                    .text_color(theme.colors.content)
                    .capture_action(cx.listener(|this, _: &MoveDown, window, cx| {
                        if this.assignment_key("down", window, cx) {
                            cx.stop_propagation();
                        }
                    }))
                    .capture_action(cx.listener(|this, _: &MoveUp, window, cx| {
                        if this.assignment_key("up", window, cx) {
                            cx.stop_propagation();
                        }
                    }))
                    .capture_action(cx.listener(|this, _: &MoveRight, window, cx| {
                        if this.assignment_key("right", window, cx) {
                            cx.stop_propagation();
                        }
                    }))
                    .capture_action(cx.listener(|this, _: &MoveLeft, window, cx| {
                        if this.assignment_key("left", window, cx) {
                            cx.stop_propagation();
                        }
                    }))
                    .capture_action(cx.listener(|this, _: &Enter, window, cx| {
                        if this.assignment_key("enter", window, cx) {
                            cx.stop_propagation();
                        }
                    }))
                    .child(search),
            );
        let mut list = div()
            .id("assignment-models")
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .p(u(4.));
        for (index, choice) in matches.iter().enumerate() {
            let highlighted = index == picker.active;
            let selected = Self::is_selected(task, choice);
            let has_effort = self.effort_for_choice(choice).is_some();
            let row =
                div()
                    .id(("assignment-option", index))
                    .flex()
                    .w_full()
                    .items_center()
                    .gap(u(8.))
                    .rounded(u(theme.radius.md))
                    .px(u(8.))
                    .py(u(6.))
                    .when(highlighted, |el| el.bg(theme.colors.selection))
                    .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                        if *hovered {
                            this.hover_assignment(index, cx);
                        }
                    }))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.click_assignment(index, window, cx)
                    }))
                    .child(harness_icon(choice.harness, 16.))
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .child(
                                div()
                                    .truncate()
                                    .text_px(13.)
                                    .leading(theme.leading.tight)
                                    .text_color(theme.colors.content)
                                    .child(choice.name.clone()),
                            )
                            .child(
                                div()
                                    .mt(u(2.))
                                    .truncate()
                                    .text_px(11.)
                                    .leading(theme.leading.tight)
                                    .text_color(theme.content(0.45))
                                    .child(choice.harness.title()),
                            ),
                    )
                    .when(selected, |row| {
                        row.child(
                            icon(IconName::Check)
                                .size(u(14.))
                                .text_color(theme.content(0.55)),
                        )
                    })
                    .when(has_effort, |row| {
                        row.child(
                            icon(IconName::ChevronRight)
                                .size(u(14.))
                                .text_color(theme.content(0.40)),
                        )
                    });
            let mut cell = div().relative().child(row);
            if highlighted {
                cell = cell.child(self.row_bounds.probe());
            }
            if highlighted && let Some((choice, effort)) = &effort_menu {
                let side = flip(
                    Placement::RightStart,
                    self.row_bounds.get(),
                    gpui::size(u(200.).to_pixels(window.rem_size()), gpui::px(0.)),
                    -4.,
                    window,
                );
                cell = cell.child(anchored_popover(
                    side,
                    -4.,
                    theme.layer.submenu,
                    window,
                    self.render_effort(task, choice, effort, theme, cx),
                ));
            }
            list = list.child(cell);
        }
        if matches.is_empty() {
            list = list.child(
                div()
                    .px(u(8.))
                    .py(u(12.))
                    .text_px(12.)
                    .text_color(theme.content(0.45))
                    .child("No matching models"),
            );
        }
        let outside = cx.listener(|this, event: &MouseDownEvent, _, cx| {
            if !this.press_is_inside(event) {
                this.close_assignment(cx);
            }
        });
        popover_surface(
            "assignment-picker",
            Some(260.),
            Some(320.),
            outside,
            div()
                .relative()
                .flex()
                .flex_col()
                .max_h(u(320.))
                .font_family(theme.fonts.sans.clone())
                .child(search_row)
                .child(list)
                .child(self.picker_bounds.probe()),
        )
        .into_any_element()
    }

    fn render_effort(
        &self,
        task: &ProposedTask,
        choice: &OrchestrationChoice,
        effort: &ModelSetting,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let selected = self.selected_effort_value(task, choice, effort);
        let effort_active = self
            .picker
            .as_ref()
            .map_or(0, |picker| picker.effort_active);
        let mut list = div()
            .relative()
            .flex()
            .flex_col()
            .p(u(4.))
            .font_family(theme.fonts.sans.clone());
        for (index, option) in effort.options.iter().enumerate() {
            let highlighted = index == effort_active;
            let hover = theme.content(0.05);
            list = list.child(
                div()
                    .id(("assignment-effort", index))
                    .flex()
                    .h(u(32.))
                    .w_full()
                    .items_center()
                    .gap(u(8.))
                    .rounded(u(theme.radius.lg))
                    .px(u(8.))
                    .text_px(13.)
                    .text_color(theme.colors.content)
                    .when(highlighted, |el| el.bg(theme.colors.selection))
                    .when(!highlighted, |el| el.hover(move |style| style.bg(hover)))
                    .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                        if *hovered {
                            this.hover_assignment_effort(index, cx);
                        }
                    }))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.pick_assignment_effort(index, window, cx)
                    }))
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .truncate()
                            .child(option.label.clone()),
                    )
                    .when(option.value == selected, |row| {
                        row.child(
                            icon(IconName::Check)
                                .size(u(14.))
                                .text_color(theme.content(0.50)),
                        )
                    }),
            );
        }
        popover_surface(
            "assignment-effort",
            Some(200.),
            None,
            |_, _, _| {},
            list.child(self.effort_bounds.probe()),
        )
        .into_any_element()
    }

    fn render_worker_help(
        &self,
        theme: &Theme,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let ink = theme.content(0.70);
        let mut root = div().relative().child(
            div()
                .id("worker-help")
                .group("worker-help")
                .size(u(16.))
                .flex()
                .items_center()
                .justify_center()
                .rounded_full()
                .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                    this.help_hovered = *hovered;
                    cx.notify();
                }))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.help_open = !this.help_open;
                    cx.notify();
                }))
                .child(
                    icon(IconName::CircleHelp)
                        .size(u(14.))
                        .text_color(theme.content(0.35))
                        .group_hover("worker-help", move |style| style.text_color(ink)),
                ),
        );
        if self.help_hovered || self.help_open {
            let content = div()
                .px(u(10.))
                .py(u(8.))
                .child(
                    div()
                        .text_px(12.)
                        .line_height(u(16.))
                        .text_color(theme.colors.content)
                        .child("How many workers run at once"),
                )
                .child(
                    div()
                        .mt(u(4.))
                        .text_px(11.)
                        .line_height(u(16.))
                        .text_color(theme.content(0.50))
                        .child(
                            "The rest of the tasks wait their turn, and a task that depends on \
                             another waits for it either way. Every worker edits this same \
                             project folder, so a lower number means fewer changes landing in \
                             it at the same time.",
                        ),
                );
            let outside = cx.listener(|this, _: &MouseDownEvent, _, cx| {
                if this.help_open {
                    this.help_open = false;
                    cx.notify();
                }
            });
            root = root.child(anchored_popover(
                Placement::TopStart,
                monocode_ui::widgets::POPOVER_GAP,
                theme.layer.popover,
                window,
                popover_surface("worker-help-popover", Some(250.), None, outside, content),
            ));
        }
        root.into_any_element()
    }

    fn render_header(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let proposal = &self.proposal;
        let spinning = self.planning() || proposal.status == OrchestrationProposalStatus::Starting;
        let mark = if spinning {
            icon(IconName::CircleDashed)
                .size(u(16.))
                .text_color(theme.content(0.55))
                .with_animation(
                    "orchestration-spin",
                    Animation::new(Duration::from_secs(1)).repeat(),
                    |svg, t| svg.with_transformation(Transformation::rotate(percentage(t))),
                )
                .into_any_element()
        } else {
            icon(IconName::MessageMultiple)
                .size(u(16.))
                .text_color(theme.content(0.55))
                .into_any_element()
        };
        let tasks = proposal.tasks.len();
        let secondary = |id: &'static str, label: &'static str, disabled: bool| {
            let hover = theme.content(0.08);
            let ink = theme.colors.content;
            div()
                .id(id)
                .flex()
                .h(u(28.))
                .items_center()
                .gap(u(6.))
                .rounded(u(theme.radius.md))
                .px(u(10.))
                .text_px(11.)
                .text_color(theme.content(0.50))
                .when(disabled, |el| el.opacity(0.35))
                .when(!disabled, |el| {
                    el.hover(move |style| style.bg(hover).text_color(ink))
                })
                .child(label)
        };
        let mut actions = div().ml_auto().flex().flex_none().items_center().gap(u(2.));
        match self.primary_action(cx) {
            Some("Try again") => {
                let disabled = self.busy || self.actions.is_none();
                actions = actions.child(
                    secondary("orchestration-retry", "Try again", disabled)
                        .on_click(cx.listener(|this, _, _, cx| this.retry(cx))),
                );
            }
            Some("View agents") => {
                actions = actions.child(
                    secondary("orchestration-agents", "View agents", false)
                        .on_click(cx.listener(|this, _, _, cx| this.view_agents(cx))),
                );
            }
            Some(label) => {
                let disabled = !self.editable(cx);
                let fill = theme.colors.content;
                let hover = theme.content(0.80);
                actions = actions.child(
                    div()
                        .id("orchestration-confirm")
                        .flex()
                        .h(u(28.))
                        .items_center()
                        .gap(u(6.))
                        .rounded(u(theme.radius.md))
                        .bg(fill)
                        .px(u(10.))
                        .text_px(11.)
                        .medium()
                        .text_color(theme.colors.background_base)
                        .when(disabled, |el| el.opacity(0.4))
                        .when(!disabled, |el| el.hover(move |style| style.bg(hover)))
                        .on_click(cx.listener(|this, _, _, cx| this.confirm(cx)))
                        .child(
                            icon(IconName::Play)
                                .size(u(12.))
                                .text_color(theme.colors.background_base),
                        )
                        .child(label),
                );
            }
            None => {}
        }
        div()
            .flex()
            .min_w_0()
            .flex_wrap()
            .items_center()
            .gap(u(10.))
            .px(u(12.))
            .py(u(10.))
            .child(
                div()
                    .flex()
                    .flex_none()
                    .size(u(32.))
                    .items_center()
                    .justify_center()
                    .rounded(u(theme.radius.lg))
                    .bg(theme.content(0.08))
                    .child(mark),
            )
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .child(
                        div()
                            .truncate()
                            .text_px(13.)
                            .medium()
                            .leading(theme.leading.tight)
                            .text_color(theme.content(0.90))
                            .child(self.title()),
                    )
                    .child(
                        div()
                            .id("orchestration-lead")
                            .mt(u(4.))
                            .flex()
                            .items_center()
                            .gap(u(6.))
                            .text_px(11.)
                            .leading(theme.leading.tight)
                            .text_color(theme.content(0.45))
                            .tooltip(tooltip(proposal.author.harness.title()))
                            .child(harness_icon(proposal.author.harness, 12.))
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .child(format!("Lead · {}", proposal.author.name)),
                            )
                            .when(tasks > 0, |el| {
                                el.child(div().flex_none().child(format!(
                                    "· {tasks} {}",
                                    if tasks == 1 { "task" } else { "tasks" }
                                )))
                            }),
                    ),
            )
            .child(actions)
            .into_any_element()
    }

    fn render_task(
        &self,
        task: &ProposedTask,
        editable: bool,
        theme: &Theme,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let open = self.is_expanded(&task.id);
        let row_hover = theme.content(0.05);
        let ink = theme.colors.content;
        let task_id = task.id.clone();
        let details = div()
            .id(eid(&task.id, "details"))
            .flex()
            .min_w_0()
            .flex_1()
            .items_center()
            .gap(u(8.))
            .py(u(4.))
            .text_color(theme.content(0.65))
            .hover(move |style| style.text_color(ink))
            .on_click(
                cx.listener(move |this, _, window, cx| this.toggle_details(&task_id, window, cx)),
            )
            .child(
                icon(if open {
                    IconName::ChevronDown
                } else {
                    IconName::ChevronRight
                })
                .size(u(14.))
                .text_color(theme.content(0.65)),
            )
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_px(12.)
                    .child(task.title.clone()),
            );
        let model = if editable {
            self.render_assignment(task, theme, window, cx)
        } else {
            div()
                .id(eid(&task.id, "model-label"))
                .flex()
                .min_w_0()
                .items_center()
                .gap(u(6.))
                .text_px(11.)
                .text_color(theme.content(0.50))
                .tooltip(tooltip(task.harness.title()))
                .child(harness_icon(task.harness, 12.))
                .child(div().min_w_0().truncate().child(self.model_line(task)))
                .into_any_element()
        };
        let row = div()
            .flex()
            .min_h(u(36.))
            .min_w_0()
            .flex_wrap()
            .items_center()
            .gap_x(u(8.))
            .gap_y(u(4.))
            .px(u(12.))
            .py(u(4.))
            .hover(move |style| style.bg(row_hover))
            .child(details)
            .child(div().max_w(gpui::relative(0.6)).min_w_0().child(model));
        let mut item = div().child(row);
        if open {
            let label = |text: &'static str| {
                div()
                    .mb(u(4.))
                    .text_px(11.)
                    .leading(theme.leading.tight)
                    .text_color(theme.content(0.45))
                    .child(text)
            };
            let field = |content: AnyElement| {
                div()
                    .w_full()
                    .rounded(u(theme.radius.md))
                    .border_1()
                    .border_color(theme.content(0.12))
                    .bg(monocode_ui::color::with_alpha(
                        theme.colors.background_base,
                        0.4,
                    ))
                    .px(u(8.))
                    .py(u(6.))
                    .text_px(12.)
                    .line_height(u(20.))
                    .text_color(theme.colors.content)
                    .child(content)
            };
            let mut body = div()
                .flex()
                .flex_col()
                .gap(u(10.))
                .px(u(12.))
                .pb(u(12.))
                .pl(u(32.))
                .text_px(11.)
                .line_height(u(16.))
                .text_color(theme.content(0.45));
            match self.fields.get(&task.id).filter(|_| editable) {
                Some(fields) => {
                    let style = InputEditorStyle {
                        foreground: theme.colors.content,
                        muted_foreground: theme.content(0.35),
                        background: gpui::transparent_black(),
                        border: gpui::transparent_black(),
                        selection: theme.accent(0.35),
                        caret: theme.colors.content,
                        ..Default::default()
                    };
                    let title = styled_input(&fields.title, style.clone(), cx);
                    let prompt = styled_input(&fields.prompt, style, cx);
                    body = body
                        .child(div().child(label("Task")).child(field(title)))
                        .child(div().child(label("Instructions")).child(field(prompt)));
                }
                None => {
                    body = body.child(
                        div()
                            .text_px(12.)
                            .line_height(u(20.))
                            .text_color(theme.content(0.60))
                            .child(task.prompt.clone()),
                    );
                }
            }
            if !task.depends_on.is_empty() {
                let after = task
                    .depends_on
                    .iter()
                    .map(|id| {
                        self.proposal
                            .tasks
                            .iter()
                            .find(|entry| &entry.id == id)
                            .map(|entry| entry.title.clone())
                            .unwrap_or_else(|| id.clone())
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                body = body.child(div().child(format!("After · {after}")));
            }
            item = item.child(body);
        }
        item.into_any_element()
    }

    fn render_footer(&self, theme: &Theme, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let editable = self.editable(cx);
        let mut left = div().flex().items_center().gap(u(6.));
        if editable {
            let mut group = div()
                .flex()
                .items_center()
                .gap(u(2.))
                .rounded(u(theme.radius.md))
                .bg(theme.content(0.05))
                .p(u(2.));
            for number in 1..=4i64 {
                let selected = self.proposal.settings.max_workers == number;
                let hover = theme.content(0.08);
                let ink = theme.colors.content;
                group = group.child(
                    div()
                        .id(("parallel-workers", number as usize))
                        .size(u(20.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(u(5.))
                        .text_px(11.)
                        .leading(theme.leading.none)
                        .tabular()
                        .when(selected, |el| {
                            el.bg(theme.colors.selection_hover)
                                .medium()
                                .text_color(theme.colors.content)
                        })
                        .when(!selected, |el| {
                            el.text_color(theme.content(0.45))
                                .hover(move |style| style.bg(hover).text_color(ink))
                        })
                        .on_click(
                            cx.listener(move |this, _, _, cx| this.set_max_workers(number, cx)),
                        )
                        .child(number.to_string()),
                );
            }
            left = left.child("Parallel workers").child(group);
        } else {
            left = left.child(format!("{} parallel", self.proposal.settings.max_workers));
        }
        left = left.child(self.render_worker_help(theme, window, cx));
        div()
            .flex()
            .flex_wrap()
            .items_center()
            .justify_between()
            .gap(u(8.))
            .border_t_1()
            .border_color(theme.colors.stroke)
            .px(u(12.))
            .py(u(8.))
            .text_px(11.)
            .text_color(theme.content(0.45))
            .child(left)
            .child(div().child(self.status_line(cx)))
            .into_any_element()
    }
}

/// gpui-base's unframed input in the card's colors.
fn styled_input<M: InputModeKind + 'static>(
    state: &Entity<InputBaseState<M>>,
    style: InputEditorStyle,
    cx: &mut App,
) -> AnyElement {
    state.update(cx, |state, _| state.set_editor_style(style));
    state.clone().into_any_element()
}

impl Render for OrchestrationPreview {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.picker.is_none() {
            self.trigger_bounds.clear();
            self.effort_bounds.clear();
        } else if self.assignment_effort_menu().is_none() {
            self.effort_bounds.clear();
        }
        let theme = Theme::of(cx).clone();
        let editable = self.editable(cx);
        let planning = self.planning();
        let mut card = div()
            .mb(u(8.))
            .overflow_hidden()
            .rounded(u(theme.radius.xl))
            .border_1()
            .border_color(theme.content(0.10))
            .bg(theme.content(0.03))
            .font_family(theme.fonts.sans.clone())
            .child(self.render_header(&theme, cx));
        if planning {
            card = card.child(
                div()
                    .px(u(12.))
                    .pb(u(10.))
                    .text_px(12.)
                    .line_height(u(20.))
                    .text_color(theme.content(0.50))
                    .child(if self.proposal.settings.choices.is_empty() {
                        "Checking available harnesses and models…"
                    } else {
                        "Your lead is choosing tasks and worker models. Review the assignments here before starting."
                    }),
            );
        }
        if !self.proposal.tasks.is_empty() {
            let mut list = div()
                .border_t_1()
                .border_color(theme.colors.stroke)
                .py(u(4.));
            for task in self.visible_tasks().to_vec() {
                list = list.child(self.render_task(&task, editable, &theme, window, cx));
            }
            card = card.child(list);
        }
        if let Some(label) = self.show_more_label() {
            let hover = theme.content(0.05);
            let ink = theme.content(0.70);
            card = card.child(
                div()
                    .id("orchestration-show-all")
                    .flex()
                    .h(u(32.))
                    .w_full()
                    .items_center()
                    .gap(u(6.))
                    .border_t_1()
                    .border_color(theme.colors.stroke)
                    .px(u(12.))
                    .text_px(11.)
                    .text_color(theme.content(0.45))
                    .hover(move |style| style.bg(hover).text_color(ink))
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_show_all(cx)))
                    .child(
                        icon(if self.show_all {
                            IconName::ChevronDown
                        } else {
                            IconName::ChevronRight
                        })
                        .size(u(14.))
                        .text_color(theme.content(0.45)),
                    )
                    .child(label),
            );
        }
        if let Some(error) = self.error() {
            card = card.child(
                div()
                    .px(u(12.))
                    .py(u(8.))
                    .text_px(12.)
                    .text_color(theme.colors.danger)
                    .child(SharedString::from(error)),
            );
        }
        if !planning {
            card = card.child(self.render_footer(&theme, window, cx));
        }
        card
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::threads::actions::{
        OrchestrationRunStatus, OrchestrationTaskStatus, OrchestrationTaskView,
    };
    use gpui::{TestAppContext, VisualTestContext};
    use monocode_core::models::{ModelSettingChoice, ModelSettingKind};
    use monocode_core::{BlockRole, Extra, HarnessId};
    use std::cell::RefCell;

    #[derive(Default)]
    struct Fake {
        runs: RefCell<Vec<OrchestrationRunView>>,
        updates: RefCell<Vec<OrchestrationProposal>>,
        confirmed: RefCell<Vec<OrchestrationProposal>>,
        opened: RefCell<Vec<String>>,
        agents: RefCell<Vec<Vec<OrchestrationWorkerDetail>>>,
    }

    impl OrchestrationRuns for Fake {
        fn runs(&self, _: &App) -> Vec<OrchestrationRunView> {
            self.runs.borrow().clone()
        }
        fn observe(&self, _: Box<dyn Fn(&mut App)>, _: &mut App) -> Subscription {
            Subscription::new(|| {})
        }
        fn cancel_task(&self, _: &str, _: &str, _: &mut App) -> Task<Result<(), String>> {
            Task::ready(Ok(()))
        }
        fn start(&self, _: &str, _: &[HarnessId], _: i64, _: &mut App) -> Task<Result<(), String>> {
            Task::ready(Ok(()))
        }
    }

    impl OrchestrationActions for Fake {
        fn update(&self, _: &str, _: &str, proposal: OrchestrationProposal, _: &mut App) {
            self.updates.borrow_mut().push(proposal);
        }
        fn confirm(&self, _: &str, _: &str, _: &mut App) -> Task<Result<(), String>> {
            let latest = self.updates.borrow().last().cloned();
            self.confirmed.borrow_mut().extend(latest);
            Task::ready(Ok(()))
        }
        fn retry(&self, _: &str, _: &str, _: &mut App) {}
        fn open(&self, session_id: &str, _: &mut App) {
            self.opened.borrow_mut().push(session_id.to_string());
        }
        fn can_open_agents(&self) -> bool {
            true
        }
        fn open_agents(&self, workers: Vec<OrchestrationWorkerDetail>, _: &mut App) {
            self.agents.borrow_mut().push(workers);
        }
    }

    fn effort(id: &str, label: &str) -> ModelSetting {
        ModelSetting {
            id: id.into(),
            label: label.into(),
            kind: ModelSettingKind::Select,
            value: "high".into(),
            options: [("xhigh", "Extra High"), ("high", "High")]
                .iter()
                .map(|(value, label)| ModelSettingChoice {
                    value: (*value).into(),
                    label: (*label).into(),
                })
                .collect(),
            description: None,
        }
    }

    fn catalog() -> Arc<ModelCatalog> {
        let mut catalog = ModelCatalog::new();
        let mut one = AgentModel::new("codex:one", HarnessId::Codex, "Worker One");
        one.settings = Some(vec![effort("reasoningEffort", "Reasoning")]);
        catalog.set_harness_models(
            HarnessId::Codex,
            vec![
                one,
                AgentModel::new("codex:two", HarnessId::Codex, "Worker Two"),
            ],
        );
        let mut two = AgentModel::new("claude:two", HarnessId::Claude, "Worker Two");
        two.settings = Some(vec![effort("effort", "Effort")]);
        catalog.set_harness_models(HarnessId::Claude, vec![two]);
        Arc::new(catalog)
    }

    fn choice(harness: HarnessId, model: &str, name: &str) -> OrchestrationChoice {
        OrchestrationChoice {
            harness,
            model: model.into(),
            name: name.into(),
            extra: Extra::new(),
        }
    }

    fn task(id: &str, title: &str, harness: HarnessId, model: &str) -> ProposedTask {
        ProposedTask {
            id: id.into(),
            title: title.into(),
            prompt: format!("Do {id}"),
            harness,
            model: model.into(),
            model_settings: None,
            files: Vec::new(),
            depends_on: Vec::new(),
            extra: Extra::new(),
        }
    }

    fn proposal(
        status: OrchestrationProposalStatus,
        tasks: Vec<ProposedTask>,
    ) -> OrchestrationProposal {
        let choices = vec![
            choice(HarnessId::Codex, "codex:one", "Worker One"),
            choice(HarnessId::Claude, "claude:two", "Worker Two"),
        ];
        OrchestrationProposal {
            version: 1,
            lead_id: "lead".into(),
            cwd: "/repo".into(),
            checkout_cwd: None,
            request: "Build".into(),
            author: choices[0].clone(),
            settings: monocode_core::orchestration::OrchestrationSettings {
                choices,
                max_workers: 2,
                extra: Extra::new(),
            },
            status,
            title: "Build settings".into(),
            summary: "Split UI and persistence".into(),
            tasks,
            error: None,
            response: None,
            extra: Extra::new(),
        }
    }

    fn block(proposal: OrchestrationProposal) -> Block {
        Block {
            orchestration: Some(proposal),
            ..Block::new("card", BlockRole::Plan, "Readable plan")
        }
    }

    fn mount(
        cx: &mut TestAppContext,
        fake: Rc<Fake>,
        proposal: OrchestrationProposal,
    ) -> (Entity<OrchestrationPreview>, &mut VisualTestContext) {
        cx.update(|cx| {
            gpui_component::init(cx);
            monocode_ui::init(monocode_ui::AppearanceSettings::default(), cx);
        });
        let block = block(proposal);
        let (card, cx) = cx.add_window_view(|window, cx| {
            let mut card =
                OrchestrationPreview::new(&block, fake.clone(), Some(fake.clone()), window, cx);
            card.set_catalog(catalog(), cx);
            card
        });
        draw(cx);
        (card, cx)
    }

    fn draw(cx: &mut VisualTestContext) {
        for _ in 0..2 {
            cx.update(|window, cx| {
                window.draw(cx).clear();
            });
            cx.run_until_parked();
        }
    }

    /// The lead saved the user's edit: show it, as the transcript would.
    fn apply_updates(card: &Entity<OrchestrationPreview>, fake: &Fake, cx: &mut VisualTestContext) {
        let latest = fake.updates.borrow().last().cloned();
        if let Some(proposal) = latest {
            let block = block(proposal);
            cx.update(|window, cx| card.update(cx, |card, cx| card.set_block(&block, window, cx)));
        }
        draw(cx);
    }

    #[gpui::test]
    fn lets_the_user_change_an_assignments_model_and_waits_for_explicit_confirmation(
        cx: &mut TestAppContext,
    ) {
        let fake = Rc::new(Fake::default());
        let initial = proposal(
            OrchestrationProposalStatus::Ready,
            vec![ProposedTask {
                prompt: "Build the form".into(),
                files: vec!["src/settings".into()],
                ..task("ui", "Settings UI", HarnessId::Codex, "codex:one")
            }],
        );
        let (card, cx) = mount(cx, fake.clone(), initial);
        cx.update(|_, cx| {
            let card = card.read(cx);
            assert_eq!(card.visible_tasks()[0].title, "Settings UI");
            assert_eq!(card.title(), "Build settings");
            assert_eq!(card.primary_action(cx), Some("Confirm & start"));
        });
        assert!(fake.confirmed.borrow().is_empty());

        // Typing filters by name, model id, and harness title.
        cx.update(|window, cx| {
            card.update(cx, |card, cx| {
                card.toggle_assignment("ui", window, cx);
                card.set_assignment_query("Claude", window, cx);
                let names: Vec<String> = card
                    .assignment_matches()
                    .into_iter()
                    .map(|c| c.name)
                    .collect();
                assert_eq!(names, ["Worker Two"]);
                // Arrows walk the list, then Enter opens and chooses its effort.
                card.set_assignment_query("Worker", window, cx);
                card.assignment_key("down", window, cx);
                card.assignment_key("enter", window, cx);
                let (choice, _) = card.assignment_effort_menu().expect("effort menu");
                assert_eq!(format!("{} effort", choice.name), "Worker Two effort");
                card.assignment_key("up", window, cx);
                card.assignment_key("enter", window, cx);
            })
        });
        apply_updates(&card, &fake, cx);
        cx.update(|_, cx| {
            let card = card.read(cx);
            assert_eq!(
                card.model_line(&card.proposal().tasks[0]),
                "Worker Two · Extra High"
            );
        });

        // The pointer reaches the same rows.
        cx.update(|window, cx| {
            card.update(cx, |card, cx| {
                card.toggle_assignment("ui", window, cx);
                let two = card
                    .assignment_matches()
                    .iter()
                    .position(|choice| choice.name == "Worker Two")
                    .unwrap();
                card.hover_assignment(two, cx);
                let (_, effort) = card.assignment_effort_menu().expect("effort menu");
                let high = effort
                    .options
                    .iter()
                    .position(|option| option.label == "High")
                    .unwrap();
                card.pick_assignment_effort(high, window, cx);
            })
        });
        apply_updates(&card, &fake, cx);
        cx.update(|_, cx| {
            let card = card.read(cx);
            assert_eq!(
                card.model_line(&card.proposal().tasks[0]),
                "Worker Two · High"
            );
        });

        cx.update(|window, cx| card.update(cx, |card, cx| card.toggle_details("ui", window, cx)));
        draw(cx);
        cx.update(|_, cx| {
            card.update(cx, |card, cx| {
                card.change_prompt(
                    "ui",
                    "Build the accessible form and check keyboard navigation",
                    cx,
                )
            })
        });
        apply_updates(&card, &fake, cx);
        cx.update(|_, cx| card.update(cx, |card, cx| card.set_max_workers(1, cx)));
        apply_updates(&card, &fake, cx);
        assert!(fake.confirmed.borrow().is_empty());

        cx.update(|_, cx| card.update(cx, |card, cx| card.confirm(cx)));
        let confirmed = fake.confirmed.borrow();
        let confirmed = confirmed.last().expect("confirmed");
        assert_eq!(confirmed.settings.max_workers, 1);
        let task = &confirmed.tasks[0];
        assert_eq!(task.harness, HarnessId::Claude);
        assert_eq!(task.model, "claude:two");
        assert_eq!(
            task.model_settings
                .as_ref()
                .and_then(|settings| settings.get("effort"))
                .map(String::as_str),
            Some("high")
        );
        assert_eq!(
            task.prompt,
            "Build the accessible form and check keyboard navigation"
        );
    }

    #[gpui::test]
    fn never_offers_confirmation_for_a_proposal_that_is_still_being_generated(
        cx: &mut TestAppContext,
    ) {
        let fake = Rc::new(Fake::default());
        let mut planning = proposal(OrchestrationProposalStatus::Planning, Vec::new());
        planning.settings.choices.clear();
        let (card, cx) = mount(cx, fake, planning);
        cx.update(|_, cx| {
            let card = card.read(cx);
            assert_eq!(card.title(), "Planning assignments…");
            assert_eq!(card.primary_action(cx), None);
            assert!(!card.editable(cx));
        });
    }

    #[gpui::test]
    fn opens_worker_panes_from_view_agents_instead_of_the_sidebar(cx: &mut TestAppContext) {
        let fake = Rc::new(Fake::default());
        let worker =
            |id: &str, session: &str, title: &str, harness, status| OrchestrationTaskView {
                id: id.into(),
                session_id: session.into(),
                title: title.into(),
                harness,
                status,
                error: None,
            };
        fake.runs.borrow_mut().push(OrchestrationRunView {
            lead_id: "lead".into(),
            proposal_id: Some("card".into()),
            status: OrchestrationRunStatus::Stopped,
            allowed_harnesses: vec![HarnessId::Codex, HarnessId::Cursor],
            max_workers: 2,
            error: None,
            tasks: vec![
                worker(
                    "engine",
                    "worker-a",
                    "Audit engine",
                    HarnessId::Codex,
                    OrchestrationTaskStatus::Cancelled,
                ),
                worker(
                    "ui",
                    "worker-b",
                    "Audit UI",
                    HarnessId::Cursor,
                    OrchestrationTaskStatus::Completed,
                ),
            ],
        });
        let approved = proposal(
            OrchestrationProposalStatus::Approved,
            vec![
                task("engine", "Audit engine", HarnessId::Codex, "codex:two"),
                task("ui", "Audit UI", HarnessId::Cursor, "cursor:composer-2.5"),
            ],
        );
        let (card, cx) = mount(cx, fake.clone(), approved);
        cx.update(|_, cx| {
            card.update(cx, |card, cx| {
                assert_eq!(card.primary_action(cx), Some("View agents"));
                assert_eq!(card.status_line(cx), "stopped · Shared project folder");
                card.view_agents(cx);
            })
        });
        assert!(fake.opened.borrow().is_empty());
        assert_eq!(
            *fake.agents.borrow(),
            [vec![
                OrchestrationWorkerDetail {
                    session_id: "worker-a".into(),
                    lead_id: "lead".into(),
                    title: "Audit engine".into(),
                    harness: HarnessId::Codex,
                },
                OrchestrationWorkerDetail {
                    session_id: "worker-b".into(),
                    lead_id: "lead".into(),
                    title: "Audit UI".into(),
                    harness: HarnessId::Cursor,
                },
            ]]
        );
    }

    #[gpui::test]
    fn caps_the_list_at_three_tasks_until_shown(cx: &mut TestAppContext) {
        let fake = Rc::new(Fake::default());
        let tasks = (1..=5)
            .map(|n| {
                task(
                    &format!("t{n}"),
                    &format!("Task {n}"),
                    HarnessId::Codex,
                    "codex:one",
                )
            })
            .collect();
        let (card, cx) = mount(
            cx,
            fake,
            proposal(OrchestrationProposalStatus::Approved, tasks),
        );
        cx.update(|_, cx| {
            card.update(cx, |card, cx| {
                assert_eq!(card.visible_tasks().len(), 3);
                assert_eq!(card.show_more_label().as_deref(), Some("Show 2 more tasks"));
                assert_eq!(card.status_line(cx), "Approved · Shared project folder");
                card.toggle_show_all(cx);
                assert_eq!(card.visible_tasks().len(), 5);
                assert_eq!(card.show_more_label().as_deref(), Some("Show fewer tasks"));
            })
        });
        draw(cx);
    }
}
