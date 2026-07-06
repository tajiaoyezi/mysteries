use crate::agent::message::Message;
use crate::agent::AgentStatus;
use crate::config::ProviderProfile;
use crate::permission::{cycle_permission_mode, PermissionMode, PermissionReply};
use crate::provider::model_meta::{anthropic_thinking_capability, AnthropicThinking};
use crate::provider::registry::models_for;
use crate::provider::{Depth, Usage};
use crate::session::SessionSummary;
use crate::tool::ask::Answer;
use crate::tool::plan::{Plan, PlanDecision, PlanProgressUpdate, StepStatus};
use crate::tui::channel::{
    AgentEvent, PermissionRequest, PlanApprovalRequest, QuestionRequest, UserInput,
};
use crate::tui::command::{command_metadata, parse_command, Command, CommandMetadata, ThinkArg};
use crate::tui::input_batch::{PasteTailMatcher, TailAction};
use crate::tui::input_buffer::{reduce_input_buffer, InputBufferAction, InputBufferState};
use crate::tui::jump_to_bottom::{bump_new_message_count, new_message_count_on_follow_bottom};
use crate::tui::selection::{reduce_selection, SelectionAction, SelectionState};
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, Mutex as AsyncMutex};

pub const SPINNER_FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
pub const ASCII_SPINNER_FRAMES: [&str; 4] = ["|", "/", "-", "\\"];
pub const THINK_DEPTH_OPTIONS: &str = "off, low, medium, high, xhigh";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Phase {
    Ready,
    Busy,
    CallingModel,
    ExecutingTool(String),
    WaitingForPermission,
    /// 手动 /compact 进行中:activity line 显示压缩动画,提交入队,CompactDone 收场。
    Compacting,
}

impl Phase {
    pub fn is_running(&self) -> bool {
        !matches!(self, Phase::Ready)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum TranscriptBlock {
    User(String),
    Assistant(String),
    Tool(ToolCard),
    Error(String),
    Help,
    Status(StatusSnapshot),
    Notice(String),
    Thinking(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionSnapshot {
    pub provider: String,
    pub model: String,
    pub max_iterations: u32,
    pub cwd: PathBuf,
    pub tools: usize,
}

impl Default for SessionSnapshot {
    fn default() -> Self {
        Self {
            provider: "mock".to_string(),
            model: "mock-model".to_string(),
            max_iterations: 4,
            cwd: PathBuf::from("."),
            tools: 7,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusSnapshot {
    pub provider: String,
    pub model: String,
    pub iteration: u32,
    pub max_iterations: u32,
    pub messages: usize,
    pub cwd: PathBuf,
    pub tools: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ToolCardStatus {
    Running,
    Done,
    Error,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ToolCard {
    pub id: String,
    pub name: String,
    pub args: Value,
    pub readonly: bool,
    pub status: ToolCardStatus,
    pub output: Option<String>,
    pub truncated: bool,
    pub exit: Option<i32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandCompletion {
    pub candidates: Vec<CommandMetadata>,
    pub selected: usize,
}

/// models picker 行:provider 标题(不可选)或缩进模型行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModelsPickerRowKind {
    ProviderHeader,
    Model,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelsPickerRow {
    pub provider_id: String,
    pub model: Option<String>,
    pub kind: ModelsPickerRowKind,
    pub is_current: bool,
}

/// `/models` 浮层纯逻辑状态机(与 ratatui 解耦,见 tui-shell spec)。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelsPicker {
    rows: Vec<ModelsPickerRow>,
    filter: String,
    visible_indices: Vec<usize>,
    highlight_visible_index: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionRow {
    pub id: String,
    pub label: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionPicker {
    pub rows: Vec<SessionRow>,
    pub highlighted: usize,
}

pub fn build_rows(
    profiles: &BTreeMap<String, ProviderProfile>,
    active: (&str, &str),
) -> Vec<ModelsPickerRow> {
    let mut rows = Vec::new();
    for (id, profile) in profiles {
        rows.push(ModelsPickerRow {
            provider_id: id.clone(),
            model: None,
            kind: ModelsPickerRowKind::ProviderHeader,
            is_current: false,
        });
        let models: Vec<String> = if let Some(catalog) = models_for(id) {
            catalog.iter().map(|model| (*model).to_string()).collect()
        } else {
            vec![profile.model.clone()]
        };
        for model in models {
            rows.push(ModelsPickerRow {
                provider_id: id.clone(),
                model: Some(model.clone()),
                kind: ModelsPickerRowKind::Model,
                is_current: id.as_str() == active.0 && model == active.1,
            });
        }
    }
    rows
}

/// 从 session 字段推断当前 active `(provider_id, model)`。
pub fn resolve_active_provider<'a>(
    session_provider: &'a str,
    session_model: &'a str,
    profiles: &'a BTreeMap<String, ProviderProfile>,
) -> (&'a str, &'a str) {
    if profiles.contains_key(session_provider) {
        return (session_provider, session_model);
    }

    let model_matches: Vec<&str> = profiles
        .iter()
        .filter(|(id, profile)| {
            if profile.model == session_model {
                return true;
            }
            models_for(id).is_some_and(|catalog| catalog.contains(&session_model))
        })
        .map(|(id, _)| id.as_str())
        .collect();

    if model_matches.len() == 1 {
        return (model_matches[0], session_model);
    }

    if let Some((id, _)) = profiles
        .iter()
        .find(|(_, profile)| default_provider_id_for_kind(&profile.kind) == session_provider)
    {
        return (id.as_str(), session_model);
    }

    profiles
        .iter()
        .next()
        .map(|(id, _)| (id.as_str(), session_model))
        .unwrap_or((session_provider, session_model))
}

fn default_provider_id_for_kind(kind: &crate::config::ProviderKind) -> &'static str {
    match kind {
        crate::config::ProviderKind::OpenAi => "openai",
        crate::config::ProviderKind::Anthropic => "anthropic",
        crate::config::ProviderKind::Mock => "mock",
    }
}

impl ModelsPicker {
    pub fn new(profiles: &BTreeMap<String, ProviderProfile>, active: (&str, &str)) -> Self {
        let rows = build_rows(profiles, active);
        let mut picker = Self {
            rows,
            filter: String::new(),
            visible_indices: Vec::new(),
            highlight_visible_index: 0,
        };
        picker.recompute_visible();
        picker.highlight_visible_index = picker.initial_highlight_model_index();
        picker
    }

    pub fn rows(&self) -> &[ModelsPickerRow] {
        &self.rows
    }

    pub fn filter_text(&self) -> &str {
        &self.filter
    }

    pub fn visible_rows(&self) -> Vec<&ModelsPickerRow> {
        self.visible_indices
            .iter()
            .map(|index| &self.rows[*index])
            .collect()
    }

    pub fn highlighted_row(&self) -> Option<&ModelsPickerRow> {
        let model_indices = self.visible_model_indices();
        let row_index = *model_indices.get(self.highlight_visible_index)?;
        Some(&self.rows[row_index])
    }

    pub fn push_filter_char(&mut self, ch: char) {
        self.filter.push(ch);
        self.recompute_visible();
    }

    pub fn pop_filter_char(&mut self) {
        self.filter.pop();
        self.recompute_visible();
    }

    pub fn move_highlight(&mut self, delta: isize) {
        let model_indices = self.visible_model_indices();
        if model_indices.is_empty() {
            return;
        }
        let len = model_indices.len() as isize;
        let current = self.highlight_visible_index as isize;
        self.highlight_visible_index = ((current + delta).rem_euclid(len)) as usize;
    }

    pub fn selected(&self) -> Option<(String, String)> {
        if self.shows_empty_hint() {
            return None;
        }
        let row = self.highlighted_row()?;
        Some((row.provider_id.clone(), row.model.clone()?))
    }

    pub fn shows_empty_hint(&self) -> bool {
        !self.filter.is_empty() && self.visible_model_indices().is_empty()
    }

    fn visible_model_indices(&self) -> Vec<usize> {
        self.visible_indices
            .iter()
            .copied()
            .filter(|index| self.rows[*index].kind == ModelsPickerRowKind::Model)
            .collect()
    }

    fn initial_highlight_model_index(&self) -> usize {
        let model_indices = self.visible_model_indices();
        if model_indices.is_empty() {
            return 0;
        }
        model_indices
            .iter()
            .position(|index| self.rows[*index].is_current)
            .unwrap_or(0)
    }

    fn recompute_visible(&mut self) {
        let needle = self.filter.to_lowercase();
        if needle.is_empty() {
            self.visible_indices = (0..self.rows.len()).collect();
            self.highlight_visible_index = self
                .initial_highlight_model_index()
                .min(self.visible_model_indices().len().saturating_sub(1));
            return;
        }

        let mut visible = Vec::new();
        let mut index = 0;
        while index < self.rows.len() {
            if self.rows[index].kind != ModelsPickerRowKind::ProviderHeader {
                index += 1;
                continue;
            }
            let header_index = index;
            let provider_id = self.rows[index].provider_id.clone();
            index += 1;
            let mut matching = Vec::new();
            while index < self.rows.len() && self.rows[index].kind == ModelsPickerRowKind::Model {
                let model = self.rows[index].model.as_deref().unwrap_or("");
                let haystack = format!("{provider_id}/{model}").to_lowercase();
                if haystack.contains(&needle) {
                    matching.push(index);
                }
                index += 1;
            }
            if !matching.is_empty() {
                visible.push(header_index);
                visible.extend(matching);
            }
        }
        self.visible_indices = visible;
        self.highlight_visible_index = 0;
    }
}

impl SessionPicker {
    pub fn new(summaries: Vec<SessionSummary>) -> Self {
        let rows = summaries
            .into_iter()
            .map(|summary| {
                let short_id = summary.id.chars().take(8).collect::<String>();
                let first_user = summary.first_user.as_deref().unwrap_or("(无输入)");
                SessionRow {
                    id: summary.id,
                    label: format!("{short_id} · {} · {first_user}", summary.created_at),
                }
            })
            .collect();
        Self {
            rows,
            highlighted: 0,
        }
    }

    pub fn move_highlight(&mut self, delta: isize) {
        if self.rows.is_empty() {
            self.highlighted = 0;
            return;
        }

        let max = self.rows.len().saturating_sub(1) as isize;
        self.highlighted = (self.highlighted as isize + delta).clamp(0, max) as usize;
    }

    pub fn selected(&self) -> Option<&str> {
        self.rows.get(self.highlighted).map(|row| row.id.as_str())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DiffKind {
    Add,
    Del,
    Ctx,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiffLine {
    pub kind: DiffKind,
    pub text: String,
}

pub fn compute_diff(tool_name: &str, args: &Value) -> Vec<DiffLine> {
    match tool_name {
        "write_file" => diff_lines(args.get("content"), DiffKind::Add),
        "edit_file" => {
            let mut lines = diff_lines(args.get("old_string"), DiffKind::Del);
            lines.extend(diff_lines(args.get("new_string"), DiffKind::Add));
            lines
        }
        _ => Vec::new(),
    }
}

fn diff_lines(value: Option<&Value>, kind: DiffKind) -> Vec<DiffLine> {
    value
        .and_then(Value::as_str)
        .map(|text| {
            text.lines()
                .map(|line| DiffLine {
                    kind: kind.clone(),
                    text: line.to_string(),
                })
                .collect()
        })
        .unwrap_or_default()
}

pub const ESTIMATED_CHARS_PER_TOKEN: u32 = 4;

pub fn estimate_tokens_from_chars(chars: u32) -> u32 {
    chars / ESTIMATED_CHARS_PER_TOKEN
}

pub fn estimate_streaming_rate_tps(chars: u32, elapsed: Duration) -> Option<f64> {
    if elapsed.is_zero() {
        return None;
    }
    Some(estimate_tokens_from_chars(chars) as f64 / elapsed.as_secs_f64())
}

/// 复制成功轻提示的存续时长(activity line 右侧;过期由渲染侧按 TTL 过滤,
/// 重绘由既有 120ms tick 驱动,不另设定时器)。
pub const COPY_HINT_TTL: Duration = Duration::from_secs(4);
pub const PASTE_TAIL_QUIET_FALLBACK: Duration = Duration::from_secs(2);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CopyHint {
    pub text: String,
    pub set_at: Instant,
}

#[derive(Clone, Debug)]
pub struct PasteTailState {
    pub matcher: PasteTailMatcher,
    pub last_key_at: Instant,
}

pub struct AppState {
    pub session: SessionSnapshot,
    pub agent_history: Arc<AsyncMutex<Vec<Message>>>,
    pub iteration: u32,
    pub transcript: Vec<TranscriptBlock>,
    pub tools_expanded: bool,
    pub command_completion: Option<CommandCompletion>,
    pub models_picker: Option<ModelsPicker>,
    pub session_picker: Option<SessionPicker>,
    pub provider_profiles: BTreeMap<String, ProviderProfile>,
    pub input_line: InputBufferState,
    pub selection: SelectionState,
    pub permission_mode: Arc<Mutex<PermissionMode>>,
    pub thinking_depth: Arc<Mutex<Depth>>,
    pub phase: Phase,
    pub pending_permission: Option<PermissionRequest>,
    pub pending_plan_approval: Option<PlanApprovalRequest>,
    pub pending_question: Option<PendingQuestion>,
    pub current_plan: Option<ActivePlan>,
    pub scroll_offset: usize,
    pub spinner_frame: usize,
    pub should_exit: bool,
    pub new_message_count: u32,
    follows_bottom: bool,
    turn_output_tokens: u32,
    last_rate_tps: Option<f64>,
    streaming_chars_this_call: u32,
    last_rate_is_approximate: bool,
    idle_output_tokens: u32,
    idle_rate_tps: Option<f64>,
    idle_rate_is_approximate: bool,
    /// running 时提交的 prompt 排队(FIFO);推进时 pop 队首进 transcript。
    pub pending_queue: Vec<String>,
    pending_session_switch: Option<String>,
    /// 最近一次「第 1 次取消键」时刻;Task 4 时间窗两级取消用。
    last_cancel_at: Option<Instant>,
    last_exit_intent_at: Option<Instant>,
    copy_hint: Option<CopyHint>,
    pub paste_tail: Option<PasteTailState>,
    paste_receiving_hint: bool,
}

pub struct PendingQuestion {
    pub request: QuestionRequest,
    pub cursor: usize,
    pub selected: Vec<String>,
    pub supplement: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActivePlan {
    pub title: String,
    pub steps: Vec<ActiveStep>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActiveStep {
    pub description: String,
    pub validation: String,
    pub status: StepStatus,
    pub validation_result: Option<String>,
}

impl ActivePlan {
    pub fn from_plan(plan: &Plan) -> Self {
        Self {
            title: plan.title.clone(),
            steps: plan
                .steps
                .iter()
                .map(|step| ActiveStep {
                    description: step.description.clone(),
                    validation: step.validation.clone(),
                    status: StepStatus::Pending,
                    validation_result: None,
                })
                .collect(),
        }
    }
}

impl AppState {
    pub fn new() -> Self {
        Self::with_session(SessionSnapshot::default())
    }

    pub fn with_session(session: SessionSnapshot) -> Self {
        Self::with_session_and_history(
            session,
            Arc::new(AsyncMutex::new(vec![Message::System(
                crate::agent::DEFAULT_SYSTEM_PROMPT.to_string(),
            )])),
        )
    }

    pub fn with_session_and_history(
        session: SessionSnapshot,
        agent_history: Arc<AsyncMutex<Vec<Message>>>,
    ) -> Self {
        Self {
            session,
            agent_history,
            iteration: 0,
            transcript: Vec::new(),
            tools_expanded: false,
            command_completion: None,
            models_picker: None,
            session_picker: None,
            provider_profiles: BTreeMap::new(),
            input_line: InputBufferState::default(),
            selection: SelectionState::default(),
            permission_mode: Arc::new(Mutex::new(PermissionMode::Normal)),
            thinking_depth: Arc::new(Mutex::new(Depth::Low)),
            phase: Phase::Ready,
            pending_permission: None,
            pending_plan_approval: None,
            pending_question: None,
            current_plan: None,
            scroll_offset: 0,
            spinner_frame: 0,
            should_exit: false,
            new_message_count: 0,
            follows_bottom: true,
            turn_output_tokens: 0,
            last_rate_tps: None,
            streaming_chars_this_call: 0,
            last_rate_is_approximate: false,
            idle_output_tokens: 0,
            idle_rate_tps: None,
            idle_rate_is_approximate: false,
            pending_queue: Vec::new(),
            pending_session_switch: None,
            last_cancel_at: None,
            last_exit_intent_at: None,
            copy_hint: None,
            paste_tail: None,
            paste_receiving_hint: false,
        }
    }

    /// 将 prompt 追加到排队队尾。
    pub fn enqueue_prompt(&mut self, s: String) {
        self.pending_queue.push(s);
    }

    /// 新一轮 user turn 开始时清空执行中计划面板(直发与 dequeue 共用)。
    fn begin_user_turn(&mut self) {
        self.current_plan = None;
    }

    fn apply_plan_progress(&mut self, update: PlanProgressUpdate) {
        let Some(plan) = self.current_plan.as_mut() else {
            return;
        };
        if update.step == 0 || update.step > plan.steps.len() {
            return;
        }
        let step = &mut plan.steps[update.step - 1];
        step.status = update.status;
        if update.validation_result.is_some() {
            step.validation_result = update.validation_result;
        }
    }

    /// pop 队首(FIFO);有值时 push transcript User、置 Busy、reset turn token。
    pub fn dequeue_next(&mut self) -> Option<String> {
        if self.pending_queue.is_empty() {
            return None;
        }
        let s = self.pending_queue.remove(0);
        self.begin_user_turn();
        self.transcript.push(TranscriptBlock::User(s.clone()));
        self.phase = Phase::Busy;
        self.reset_turn_token_usage();
        Some(s)
    }

    /// 清空所有排队消息。
    pub fn clear_queue(&mut self) {
        self.pending_queue.clear();
    }

    /// 排队区是否非空。
    pub fn has_queue(&self) -> bool {
        !self.pending_queue.is_empty()
    }

    pub fn open_session_picker(&mut self, summaries: Vec<SessionSummary>) {
        self.session_picker = Some(SessionPicker::new(summaries));
    }

    pub fn close_session_picker(&mut self) {
        self.session_picker = None;
    }

    pub fn take_pending_session_switch(&mut self) -> Option<String> {
        self.pending_session_switch.take()
    }

    pub(crate) fn last_cancel_at(&self) -> Option<Instant> {
        self.last_cancel_at
    }

    pub(crate) fn set_last_cancel_at(&mut self, at: Instant) {
        self.last_cancel_at = Some(at);
    }

    pub(crate) fn last_exit_intent_at(&self) -> Option<Instant> {
        self.last_exit_intent_at
    }

    pub(crate) fn set_last_exit_intent_at(&mut self, at: Instant) {
        self.last_exit_intent_at = Some(at);
    }

    /// 记录复制成功轻提示(现在开始计时;新复制覆盖旧 hint 并重新计时)。
    pub fn set_copy_hint(&mut self, text: String) {
        self.copy_hint = Some(CopyHint {
            text,
            set_at: Instant::now(),
        });
    }

    /// TTL 内返回 hint 文案,过期返回 `None`(渲染侧据此显示 / 隐藏)。
    pub fn active_copy_hint(&self, now: Instant) -> Option<&str> {
        self.copy_hint.as_ref().and_then(|hint| {
            (now.duration_since(hint.set_at) < COPY_HINT_TTL).then_some(hint.text.as_str())
        })
    }

    pub fn set_paste_tail(&mut self, matcher: PasteTailMatcher, now: Instant) {
        self.paste_tail = Some(PasteTailState {
            matcher,
            last_key_at: now,
        });
    }

    pub fn paste_tail_active(&self) -> bool {
        self.paste_tail.is_some()
    }

    pub fn set_paste_receiving_hint(&mut self, active: bool) {
        self.paste_receiving_hint = active;
    }

    pub fn paste_receiving_hint_active(&self) -> bool {
        self.paste_receiving_hint
    }

    pub fn clear_paste_tail(&mut self) {
        self.paste_tail = None;
    }

    pub fn record_paste_tail_action(&mut self, action: TailAction, now: Instant) {
        if action == TailAction::Drop {
            if let Some(tail) = self.paste_tail.as_mut() {
                tail.last_key_at = now;
            }
        }
    }

    pub fn expire_paste_tail(&mut self, now: Instant) -> bool {
        let Some(tail) = self.paste_tail.as_ref() else {
            return false;
        };
        if now.saturating_duration_since(tail.last_key_at) >= PASTE_TAIL_QUIET_FALLBACK {
            self.clear_paste_tail();
            return true;
        }
        false
    }

    pub fn follows_bottom(&self) -> bool {
        self.follows_bottom
    }

    fn clear_new_message_count_if_following(&mut self) {
        if self.follows_bottom {
            self.new_message_count = new_message_count_on_follow_bottom();
        }
    }

    pub fn input(&self) -> &str {
        self.input_line.text()
    }

    pub(crate) fn insert_paste_fold(&mut self, text: String) {
        self.apply_input_action(InputBufferAction::InsertPasteFold(text));
        self.refresh_command_completion();
    }

    fn input_has_fold(&self) -> bool {
        !self.input_line.pasted.is_empty()
    }

    pub fn apply_selection_action(&mut self, action: SelectionAction) {
        self.selection = reduce_selection(&self.selection, action);
    }

    pub fn clear_selection(&mut self) {
        self.apply_selection_action(SelectionAction::Clear);
    }

    pub fn has_selection(&self) -> bool {
        self.selection.selection.is_some()
    }

    pub fn current_permission_mode(&self) -> PermissionMode {
        *self
            .permission_mode
            .lock()
            .expect("permission mode mutex poisoned")
    }

    pub fn current_thinking_depth(&self) -> Depth {
        *self
            .thinking_depth
            .lock()
            .expect("thinking depth mutex poisoned")
    }

    pub fn thinking_cannot_disable_active(&self) -> bool {
        if self.current_thinking_depth() != Depth::Off {
            return false;
        }
        matches!(
            anthropic_thinking_capability(&self.session.model),
            AnthropicThinking::Adaptive {
                can_disable: false,
                ..
            }
        )
    }

    fn maybe_notice_thinking_cannot_disable(&mut self) {
        if self.thinking_cannot_disable_active() {
            self.transcript
                .push(TranscriptBlock::Notice("该模型思考无法关闭".to_string()));
        }
    }

    pub fn depth_label(depth: Depth) -> &'static str {
        match depth {
            Depth::Off => "off",
            Depth::Low => "low",
            Depth::Medium => "medium",
            Depth::High => "high",
            Depth::Xhigh => "xhigh",
        }
    }

    pub fn has_pending_dialog(&self) -> bool {
        self.pending_permission.is_some()
            || self.pending_plan_approval.is_some()
            || self.pending_question.is_some()
    }

    fn apply_input_action(&mut self, action: InputBufferAction) {
        self.input_line = reduce_input_buffer(&self.input_line, action);
    }

    pub fn set_input_text(&mut self, text: impl Into<String>) {
        self.apply_input_action(InputBufferAction::SetText(text.into()));
    }

    fn cycle_permission_mode(&mut self) {
        let mut guard = self
            .permission_mode
            .lock()
            .expect("permission mode mutex poisoned");
        *guard = cycle_permission_mode(*guard);
    }

    pub fn output_tokens_this_turn(&self) -> u32 {
        self.turn_output_tokens
    }

    pub fn last_rate_tps(&self) -> Option<f64> {
        self.last_rate_tps
    }

    pub fn record_usage(&mut self, usage: Usage, elapsed: Duration) {
        self.turn_output_tokens += usage.output_tokens;
        self.last_rate_tps = if elapsed.is_zero() {
            None
        } else {
            Some(usage.output_tokens as f64 / elapsed.as_secs_f64())
        };
        self.last_rate_is_approximate = false;
    }

    pub fn record_streaming_chars(&mut self, chars: u32, elapsed: Duration) {
        self.streaming_chars_this_call += chars;
        self.last_rate_tps = estimate_streaming_rate_tps(self.streaming_chars_this_call, elapsed);
        self.last_rate_is_approximate = true;
    }

    pub fn reset_streaming_chars_for_call(&mut self) {
        self.streaming_chars_this_call = 0;
    }

    pub fn idle_output_tokens(&self) -> u32 {
        self.idle_output_tokens
    }

    pub fn idle_rate_tps(&self) -> Option<f64> {
        self.idle_rate_tps
    }

    pub fn idle_rate_is_approximate(&self) -> bool {
        self.idle_rate_is_approximate
    }

    pub fn last_rate_is_approximate(&self) -> bool {
        self.last_rate_is_approximate
    }

    fn reset_turn_token_usage(&mut self) {
        self.turn_output_tokens = 0;
        self.last_rate_tps = None;
        self.last_rate_is_approximate = false;
        self.streaming_chars_this_call = 0;
    }

    fn save_idle_token_summary(&mut self) {
        self.idle_output_tokens = self.turn_output_tokens;
        self.idle_rate_tps = self.last_rate_tps;
        self.idle_rate_is_approximate = self.last_rate_is_approximate;
    }

    pub fn status_snapshot(&self) -> StatusSnapshot {
        StatusSnapshot {
            provider: self.session.provider.clone(),
            model: self.session.model.clone(),
            iteration: self.iteration,
            max_iterations: self.session.max_iterations,
            messages: self.dialog_message_count(),
            cwd: self.session.cwd.clone(),
            tools: self.session.tools,
        }
    }

    pub fn dialog_message_count(&self) -> usize {
        self.transcript
            .iter()
            .filter(|block| {
                matches!(
                    block,
                    TranscriptBlock::User(_) | TranscriptBlock::Assistant(_)
                )
            })
            .count()
    }

    pub fn visible_scroll_offset(&self, total_lines: usize, viewport_lines: usize) -> usize {
        if self.follows_bottom {
            bottom_offset(total_lines, viewport_lines)
        } else {
            self.scroll_offset
                .min(bottom_offset(total_lines, viewport_lines))
        }
    }

    pub fn page_up(&mut self, total_lines: usize, viewport_lines: usize) {
        let current = self.visible_scroll_offset(total_lines, viewport_lines);
        self.scroll_offset = current.saturating_sub(viewport_lines);
        self.follows_bottom = false;
    }

    pub fn page_down(&mut self, total_lines: usize, viewport_lines: usize) {
        let bottom = bottom_offset(total_lines, viewport_lines);
        let next = self
            .visible_scroll_offset(total_lines, viewport_lines)
            .saturating_add(viewport_lines)
            .min(bottom);
        self.scroll_offset = next;
        self.follows_bottom = next == bottom;
        self.clear_new_message_count_if_following();
    }

    pub fn scroll_to_top(&mut self, _total_lines: usize, _viewport_lines: usize) {
        self.scroll_offset = 0;
        self.follows_bottom = false;
    }

    pub fn scroll_to_bottom(&mut self, _total_lines: usize, _viewport_lines: usize) {
        self.follows_bottom = true;
        self.new_message_count = new_message_count_on_follow_bottom();
    }

    pub fn scroll_up(&mut self, total_lines: usize, viewport_lines: usize, lines: usize) {
        let current = self.visible_scroll_offset(total_lines, viewport_lines);
        self.scroll_offset = current.saturating_sub(lines);
        self.follows_bottom = false;
    }

    pub fn scroll_down(&mut self, total_lines: usize, viewport_lines: usize, lines: usize) {
        let bottom = bottom_offset(total_lines, viewport_lines);
        let next = self
            .visible_scroll_offset(total_lines, viewport_lines)
            .saturating_add(lines)
            .min(bottom);
        self.scroll_offset = next;
        self.follows_bottom = next == bottom;
        self.clear_new_message_count_if_following();
    }

    pub fn advance_spinner(&mut self) {
        self.spinner_frame = (self.spinner_frame + 1) % SPINNER_FRAMES.len();
    }

    pub fn spinner_glyph(&self) -> &'static str {
        spinner_glyph(self.spinner_frame, true)
    }

    pub fn toggle_tools_expanded(&mut self) {
        self.tools_expanded = !self.tools_expanded;
    }

    fn refresh_command_completion(&mut self) {
        let previous = self
            .command_completion
            .as_ref()
            .and_then(|completion| completion.candidates.get(completion.selected))
            .map(|command| command.name);

        let Some(mut candidates) = command_completion_candidates(self.input()) else {
            self.command_completion = None;
            return;
        };

        let selected = previous
            .and_then(|name| candidates.iter().position(|command| command.name == name))
            .unwrap_or(0);

        self.command_completion = Some(CommandCompletion {
            candidates: std::mem::take(&mut candidates),
            selected,
        });
    }

    fn insert_newline_and_refresh(&mut self) {
        self.apply_input_action(InputBufferAction::InsertNewline);
        self.refresh_command_completion();
    }

    fn close_command_completion(&mut self) {
        self.command_completion = None;
    }

    fn move_command_completion_selection(&mut self, delta: isize) {
        let Some(completion) = self.command_completion.as_mut() else {
            return;
        };
        if completion.candidates.is_empty() {
            completion.selected = 0;
            return;
        }

        let len = completion.candidates.len();
        completion.selected = if delta.is_negative() {
            completion.selected.checked_sub(1).unwrap_or(len - 1)
        } else {
            (completion.selected + 1) % len
        };
    }

    fn complete_selected_command(&mut self) {
        let Some(completion) = self.command_completion.as_ref() else {
            return;
        };
        let Some(command) = completion.candidates.get(completion.selected) else {
            return;
        };

        self.set_input_text(command.name.to_string());
        self.close_command_completion();
    }

    fn close_models_picker(&mut self) {
        self.models_picker = None;
    }

    fn open_models_picker(&mut self) {
        if self.provider_profiles.is_empty() {
            self.transcript
                .push(TranscriptBlock::Notice("无已配 provider".to_string()));
            return;
        }
        let active = resolve_active_provider(
            &self.session.provider,
            &self.session.model,
            &self.provider_profiles,
        );
        self.models_picker = Some(ModelsPicker::new(&self.provider_profiles, active));
    }

    fn handle_models_picker_key(
        &mut self,
        key: KeyEvent,
        input_tx: &mpsc::UnboundedSender<UserInput>,
    ) -> bool {
        if self.models_picker.is_none() {
            return false;
        }

        match key.code {
            KeyCode::Up => {
                if let Some(picker) = self.models_picker.as_mut() {
                    picker.move_highlight(-1);
                }
                true
            }
            KeyCode::Down => {
                if let Some(picker) = self.models_picker.as_mut() {
                    picker.move_highlight(1);
                }
                true
            }
            KeyCode::Enter => {
                if let Some(picker) = self.models_picker.as_ref() {
                    if let Some((id, model)) = picker.selected() {
                        let _ = input_tx.send(UserInput::SetProvider {
                            id: id.clone(),
                            model: model.clone(),
                        });
                        self.session.provider = id;
                        self.session.model = model;
                    }
                }
                self.close_models_picker();
                true
            }
            KeyCode::Esc => {
                self.close_models_picker();
                true
            }
            KeyCode::Char(ch) => {
                if let Some(picker) = self.models_picker.as_mut() {
                    picker.push_filter_char(ch);
                }
                true
            }
            KeyCode::Backspace => {
                if let Some(picker) = self.models_picker.as_mut() {
                    picker.pop_filter_char();
                }
                true
            }
            _ => true,
        }
    }

    pub fn handle_session_picker_key(&mut self, key: KeyEvent) -> bool {
        if self.session_picker.is_none() {
            return false;
        }

        match key.code {
            KeyCode::Up => {
                if let Some(picker) = self.session_picker.as_mut() {
                    picker.move_highlight(-1);
                }
                true
            }
            KeyCode::Down => {
                if let Some(picker) = self.session_picker.as_mut() {
                    picker.move_highlight(1);
                }
                true
            }
            KeyCode::Enter => {
                let selected = self
                    .session_picker
                    .as_ref()
                    .and_then(|picker| picker.selected().map(str::to_string));
                if let Some(id) = selected {
                    self.pending_session_switch = Some(id);
                    self.close_session_picker();
                }
                true
            }
            KeyCode::Esc => {
                self.close_session_picker();
                true
            }
            _ => true,
        }
    }

    fn handle_command_completion_key(&mut self, key: KeyEvent) -> bool {
        if self.command_completion.is_none() {
            return false;
        }

        match key.code {
            KeyCode::Up => {
                self.move_command_completion_selection(-1);
                true
            }
            KeyCode::Down => {
                self.move_command_completion_selection(1);
                true
            }
            KeyCode::Tab | KeyCode::Enter => {
                self.complete_selected_command();
                true
            }
            KeyCode::Esc => {
                self.close_command_completion();
                true
            }
            _ => false,
        }
    }

    pub fn apply(&mut self, event: AgentEvent) {
        match event {
            AgentEvent::TextDelta(text) => {
                if self.phase == Phase::Ready {
                    self.phase = Phase::Busy;
                }
                match self.transcript.last_mut() {
                    Some(TranscriptBlock::Assistant(current)) => current.push_str(&text),
                    _ => self.transcript.push(TranscriptBlock::Assistant(text)),
                }
            }
            AgentEvent::ThinkingDelta(text) => {
                if self.phase == Phase::Ready {
                    self.phase = Phase::Busy;
                }
                match self.transcript.last_mut() {
                    Some(TranscriptBlock::Thinking(current)) => current.push_str(&text),
                    _ => self.transcript.push(TranscriptBlock::Thinking(text)),
                }
            }
            AgentEvent::ToolCallStarted {
                id,
                name,
                args,
                readonly,
            } => {
                self.transcript.push(TranscriptBlock::Tool(ToolCard {
                    id,
                    name,
                    args,
                    readonly,
                    status: ToolCardStatus::Running,
                    output: None,
                    truncated: false,
                    exit: None,
                }));
            }
            AgentEvent::ToolCallFinished { id, outcome } => {
                let status = if outcome.is_error {
                    ToolCardStatus::Error
                } else {
                    ToolCardStatus::Done
                };

                if let Some(card) = self.transcript.iter_mut().find_map(|block| match block {
                    TranscriptBlock::Tool(card) if card.id == id => Some(card),
                    _ => None,
                }) {
                    card.status = status;
                    card.output = Some(outcome.content);
                    card.truncated = outcome.truncated;
                    card.exit = outcome.exit;
                }
            }
            AgentEvent::StatusChanged(status) => {
                if status == AgentStatus::CallingModel {
                    self.iteration += 1;
                }
                match status {
                    AgentStatus::Idle => { /* phase→Ready 仅由三终止事件驱动 */ }
                    AgentStatus::CallingModel => self.phase = Phase::CallingModel,
                    AgentStatus::ExecutingTool(name) => self.phase = Phase::ExecutingTool(name),
                    AgentStatus::WaitingForPermission => self.phase = Phase::WaitingForPermission,
                }
            }
            AgentEvent::PermissionRequired(request) => {
                self.pending_permission = Some(request);
                self.phase = Phase::WaitingForPermission;
            }
            AgentEvent::PlanApprovalRequired(request) => {
                self.pending_plan_approval = Some(request);
                self.phase = Phase::WaitingForPermission;
            }
            AgentEvent::UserQuestionRequired(request) => {
                self.pending_question = Some(PendingQuestion {
                    request,
                    cursor: 0,
                    selected: Vec::new(),
                    supplement: String::new(),
                });
                self.phase = Phase::WaitingForPermission;
            }
            AgentEvent::TurnComplete => {
                let was_busy = self.phase != Phase::Ready;
                self.pending_permission = None;
                self.pending_plan_approval = None;
                self.pending_question = None;
                self.iteration = 0;
                self.phase = Phase::Ready;
                self.save_idle_token_summary();
                self.reset_turn_token_usage();
                if was_busy {
                    self.new_message_count =
                        bump_new_message_count(self.follows_bottom, self.new_message_count);
                }
            }
            AgentEvent::Notice(message) => {
                self.transcript.push(TranscriptBlock::Notice(message));
            }
            AgentEvent::CompactDone => {
                self.phase = Phase::Ready;
            }
            AgentEvent::Interrupted => {
                let was_busy = self.phase != Phase::Ready;
                self.pending_permission = None;
                self.pending_plan_approval = None;
                self.pending_question = None;
                self.iteration = 0;
                self.phase = Phase::Ready;
                self.transcript
                    .push(TranscriptBlock::Notice("⊘ 已中断本轮".to_string()));
                if was_busy {
                    self.new_message_count =
                        bump_new_message_count(self.follows_bottom, self.new_message_count);
                }
            }
            AgentEvent::Error(message) => {
                let was_busy = self.phase != Phase::Ready;
                self.pending_permission = None;
                self.pending_plan_approval = None;
                self.pending_question = None;
                self.iteration = 0;
                self.phase = Phase::Ready;
                self.transcript.push(TranscriptBlock::Error(message));
                if was_busy {
                    self.new_message_count =
                        bump_new_message_count(self.follows_bottom, self.new_message_count);
                }
            }
            AgentEvent::Usage {
                input_tokens: _,
                output_tokens: _,
            } => {}
            AgentEvent::PlanProgress(update) => {
                self.apply_plan_progress(update);
            }
        }
    }

    pub fn on_key(&mut self, key: KeyEvent, input_tx: &mpsc::UnboundedSender<UserInput>) {
        self.on_key_inner(key, input_tx, None);
    }

    pub fn on_key_with_interrupt(
        &mut self,
        key: KeyEvent,
        input_tx: &mpsc::UnboundedSender<UserInput>,
        interrupt_tx: &mpsc::UnboundedSender<UserInput>,
    ) {
        self.on_key_inner(key, input_tx, Some(interrupt_tx));
    }

    fn on_key_inner(
        &mut self,
        key: KeyEvent,
        input_tx: &mpsc::UnboundedSender<UserInput>,
        interrupt_tx: Option<&mpsc::UnboundedSender<UserInput>>,
    ) {
        if key.kind != KeyEventKind::Press {
            return;
        }

        if key.code == KeyCode::Char('o') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.toggle_tools_expanded();
            return;
        }

        if key.code == KeyCode::BackTab {
            self.cycle_permission_mode();
            return;
        }

        if self.pending_permission.is_some() {
            match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
                    self.answer_pending_permission(PermissionReply::AllowOnce);
                }
                KeyCode::Char('a') | KeyCode::Char('A')
                    if self
                        .pending_permission
                        .as_ref()
                        .is_some_and(|request| request.allow_always_key.is_some()) =>
                {
                    self.answer_pending_permission(PermissionReply::AllowAlways);
                }
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                    self.answer_pending_permission(PermissionReply::Deny);
                }
                _ => {}
            }
            return;
        }

        if self.pending_plan_approval.is_some() {
            match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
                    self.answer_pending_plan_approval(PlanDecision::Approve);
                }
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                    self.answer_pending_plan_approval(PlanDecision::Reject("用户驳回".to_string()));
                }
                _ => {}
            }
            return;
        }

        if let Some(pending) = self.pending_question.as_mut() {
            let options_len = pending.request.question.options.len();
            let other_index = options_len;
            match key.code {
                KeyCode::Enter => {
                    self.submit_pending_question();
                    return;
                }
                KeyCode::Esc => {
                    self.cancel_pending_question();
                    return;
                }
                KeyCode::Up => {
                    pending.cursor = pending.cursor.saturating_sub(1);
                    return;
                }
                KeyCode::Down => {
                    pending.cursor = (pending.cursor + 1).min(other_index);
                    return;
                }
                KeyCode::Backspace => {
                    if pending.cursor == other_index {
                        pending.supplement.pop();
                    }
                    return;
                }
                KeyCode::Char(' ') => {
                    if pending.request.question.allow_multi && pending.cursor < options_len {
                        let label = pending.request.question.options[pending.cursor]
                            .label
                            .clone();
                        if let Some(index) = pending
                            .selected
                            .iter()
                            .position(|selected| selected == &label)
                        {
                            pending.selected.remove(index);
                        } else {
                            pending.selected.push(label);
                        }
                    }
                    return;
                }
                KeyCode::Char(ch @ '1'..='9') => {
                    let picked = (ch as u8 - b'1') as usize;
                    if picked <= other_index {
                        pending.cursor = picked;
                        if picked < options_len && pending.request.question.allow_multi {
                            let label = pending.request.question.options[picked].label.clone();
                            if let Some(selected_index) = pending
                                .selected
                                .iter()
                                .position(|selected| selected == &label)
                            {
                                pending.selected.remove(selected_index);
                            } else {
                                pending.selected.push(label);
                            }
                        }
                    } else if pending.cursor == other_index {
                        pending.supplement.push(ch);
                    }
                    return;
                }
                KeyCode::Char(ch) => {
                    let pure_control = key.modifiers.contains(KeyModifiers::CONTROL)
                        && !key.modifiers.contains(KeyModifiers::ALT);
                    if pure_control {
                        return;
                    }
                    if pending.cursor == other_index {
                        pending.supplement.push(ch);
                    }
                    return;
                }
                _ => return,
            }
        }

        if self.handle_models_picker_key(key, input_tx) {
            return;
        }

        if self.handle_command_completion_key(key) {
            return;
        }

        let interrupt_key = key.code == KeyCode::Esc
            || (key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL));
        if interrupt_key && self.phase.is_running() {
            if let Some(tx) = interrupt_tx {
                let _ = tx.send(UserInput::Interrupt);
            }
            return;
        }

        let pure_control = key.modifiers.contains(KeyModifiers::CONTROL)
            && !key.modifiers.contains(KeyModifiers::ALT);
        let newline_key = match key.code {
            KeyCode::Enter => {
                key.modifiers.contains(KeyModifiers::CONTROL)
                    || key.modifiers.contains(KeyModifiers::SHIFT)
            }
            KeyCode::Char(ch) => ch.eq_ignore_ascii_case(&'j') && pure_control,
            _ => false,
        };
        if newline_key {
            self.insert_newline_and_refresh();
            return;
        }

        match key.code {
            KeyCode::Up => {
                self.apply_input_action(InputBufferAction::Up);
            }
            KeyCode::Down => {
                self.apply_input_action(InputBufferAction::Down);
            }
            KeyCode::Left => {
                self.apply_input_action(InputBufferAction::MoveLeft);
            }
            KeyCode::Right => {
                self.apply_input_action(InputBufferAction::MoveRight);
            }
            KeyCode::Home => {
                self.apply_input_action(InputBufferAction::MoveLineStart);
            }
            KeyCode::End => {
                self.apply_input_action(InputBufferAction::MoveLineEnd);
            }
            KeyCode::Char(ch) => {
                if pure_control {
                    return;
                }
                self.apply_input_action(InputBufferAction::InsertChar(ch));
                self.refresh_command_completion();
            }
            KeyCode::Backspace => {
                self.apply_input_action(InputBufferAction::Backspace);
                self.refresh_command_completion();
            }
            KeyCode::Delete => {
                self.apply_input_action(InputBufferAction::Delete);
                self.refresh_command_completion();
            }
            KeyCode::Enter => {
                self.close_command_completion();
                let contains_newline = self.input().contains('\n') || self.input_has_fold();
                let prompt = self.input_line.expand_folds();
                let prompt = prompt.trim().to_string();
                if prompt.is_empty() {
                    return;
                }
                self.clear_selection();
                self.apply_input_action(InputBufferAction::PushSubmitted(prompt.clone()));
                if !contains_newline {
                    if let Some(command) = parse_command(&prompt) {
                        self.execute_command(command, input_tx);
                        return;
                    }
                }
                if self.phase == Phase::Ready && !self.has_queue() {
                    self.reset_turn_token_usage();
                    self.iteration = 0;
                    self.phase = Phase::Busy;
                    self.begin_user_turn();
                    self.transcript.push(TranscriptBlock::User(prompt.clone()));
                    let _ = input_tx.send(UserInput::Prompt(prompt));
                } else {
                    self.enqueue_prompt(prompt);
                }
            }
            _ => {}
        }
    }

    pub fn execute_command(
        &mut self,
        command: Command,
        _input_tx: &mpsc::UnboundedSender<UserInput>,
    ) {
        match command {
            Command::Help => self.transcript.push(TranscriptBlock::Help),
            Command::Clear => {
                self.transcript.clear();
                self.clear_selection();
            }
            Command::Status => self
                .transcript
                .push(TranscriptBlock::Status(self.status_snapshot())),
            Command::Exit => self.should_exit = true,
            Command::Unknown(name) => self
                .transcript
                .push(TranscriptBlock::Notice(format!("未知命令: /{name}"))),
            Command::Model(None) => {
                self.transcript.push(TranscriptBlock::Notice(format!(
                    "当前 model: {} — 输入 /model <name> 切换",
                    self.session.model
                )));
            }
            Command::Model(Some(model)) => {
                self.session.model = model.clone();
                let _ = _input_tx.send(UserInput::SetModel(model));
                self.maybe_notice_thinking_cannot_disable();
            }
            Command::Compact => {
                // 仅就绪且无排队时可发起:压缩期间 phase 非 Ready,后续提交自然入队;
                // 运行中/有排队时拒绝,避免 Compact 在 channel 里排到轮后延迟执行。
                if self.phase == Phase::Ready && !self.has_queue() {
                    self.phase = Phase::Compacting;
                    let _ = _input_tx.send(UserInput::Compact);
                } else {
                    self.transcript.push(TranscriptBlock::Notice(
                        "当前有任务进行中,/compact 请稍后再试".to_string(),
                    ));
                }
            }
            Command::Models => self.open_models_picker(),
            Command::Think(arg) => match arg {
                ThinkArg::Query => {
                    let current = self.current_thinking_depth();
                    self.transcript.push(TranscriptBlock::Notice(format!(
                        "当前思考档位: {} — 可选: {}",
                        Self::depth_label(current),
                        THINK_DEPTH_OPTIONS
                    )));
                }
                ThinkArg::Set(depth) => {
                    *self
                        .thinking_depth
                        .lock()
                        .expect("thinking depth mutex poisoned") = depth;
                    self.transcript.push(TranscriptBlock::Notice(format!(
                        "思考档位已设为 {}",
                        Self::depth_label(depth)
                    )));
                    self.maybe_notice_thinking_cannot_disable();
                }
                ThinkArg::Invalid(value) => {
                    self.transcript.push(TranscriptBlock::Notice(format!(
                        "无效档位 \"{value}\" — 可选: {THINK_DEPTH_OPTIONS}"
                    )));
                }
            },
        }
    }

    fn answer_pending_permission(&mut self, reply: PermissionReply) {
        if let Some(request) = self.pending_permission.take() {
            let _ = request.responder.send(reply);
            self.phase = Phase::Busy;
        }
    }

    fn answer_pending_plan_approval(&mut self, decision: PlanDecision) {
        if let Some(request) = self.pending_plan_approval.take() {
            if matches!(decision, PlanDecision::Approve) {
                self.current_plan = Some(ActivePlan::from_plan(&request.plan));
            }
            let _ = request.responder.send(decision);
            self.phase = Phase::Busy;
        }
    }

    fn submit_pending_question(&mut self) {
        if let Some(pending) = self.pending_question.take() {
            let question = &pending.request.question;
            let other_index = question.options.len();
            let (selected, supplement) = if pending.cursor == other_index {
                (
                    Vec::new(),
                    (!pending.supplement.is_empty()).then_some(pending.supplement),
                )
            } else if question.allow_multi {
                (pending.selected, None)
            } else if question.options.is_empty() {
                (Vec::new(), None)
            } else {
                (vec![question.options[pending.cursor].label.clone()], None)
            };
            let _ = pending.request.responder.send(Answer {
                selected,
                supplement,
            });
            self.phase = Phase::Busy;
        }
    }

    fn cancel_pending_question(&mut self) {
        if let Some(pending) = self.pending_question.take() {
            let _ = pending.request.responder.send(Answer {
                selected: Vec::new(),
                supplement: None,
            });
            self.phase = Phase::Busy;
        }
    }
}

pub(crate) enum ApplyBatchKeyResult {
    Continue,
    BreakBatch,
}

pub(crate) fn flush_merged_input_chars(state: &mut AppState, pending_str: &mut String) {
    if !pending_str.is_empty() {
        state.apply_input_action(InputBufferAction::InsertStr(std::mem::take(pending_str)));
        state.refresh_command_completion();
    }
}

fn is_bare_enter_key(key: KeyEvent) -> bool {
    key.code == KeyCode::Enter
        && !key.modifiers.contains(KeyModifiers::CONTROL)
        && !key.modifiers.contains(KeyModifiers::SHIFT)
}

fn is_insertable_char_key(key: KeyEvent) -> bool {
    match key.code {
        KeyCode::Char(_) => {
            let pure_control = key.modifiers.contains(KeyModifiers::CONTROL)
                && !key.modifiers.contains(KeyModifiers::ALT);
            !pure_control
        }
        _ => false,
    }
}

pub(crate) fn apply_batch_input_key(
    state: &mut AppState,
    key: KeyEvent,
    intent: crate::tui::input_batch::KeyIntent,
    modal_closed_in_batch: &mut bool,
    pending_str: &mut String,
    input_tx: &mpsc::UnboundedSender<UserInput>,
    interrupt_tx: &mpsc::UnboundedSender<UserInput>,
) -> ApplyBatchKeyResult {
    use crate::tui::input_batch::KeyIntent;

    if state.has_pending_dialog() {
        flush_merged_input_chars(state, pending_str);
        state.on_key_with_interrupt(key, input_tx, interrupt_tx);
        return if state.has_pending_dialog() {
            ApplyBatchKeyResult::Continue
        } else {
            ApplyBatchKeyResult::BreakBatch
        };
    }

    if state.models_picker.is_some() {
        flush_merged_input_chars(state, pending_str);
        state.on_key_with_interrupt(key, input_tx, interrupt_tx);
        if state.models_picker.is_none() {
            *modal_closed_in_batch = true;
        }
        return ApplyBatchKeyResult::Continue;
    }

    if *modal_closed_in_batch && is_bare_enter_key(key) {
        flush_merged_input_chars(state, pending_str);
        return ApplyBatchKeyResult::Continue;
    }

    if intent == KeyIntent::Newline {
        flush_merged_input_chars(state, pending_str);
        state.insert_newline_and_refresh();
        return ApplyBatchKeyResult::Continue;
    }

    if is_insertable_char_key(key) && intent == KeyIntent::Passthrough {
        if let KeyCode::Char(ch) = key.code {
            pending_str.push(ch);
        }
        return ApplyBatchKeyResult::Continue;
    }

    flush_merged_input_chars(state, pending_str);
    state.on_key_with_interrupt(key, input_tx, interrupt_tx);
    ApplyBatchKeyResult::Continue
}

fn command_completion_candidates(input: &str) -> Option<Vec<CommandMetadata>> {
    if !input.starts_with('/') {
        return None;
    }
    if input.chars().any(char::is_whitespace) {
        return None;
    }

    let candidates = command_metadata()
        .iter()
        .copied()
        .filter(|command| command.name.starts_with(input))
        .collect::<Vec<_>>();
    if candidates.is_empty() {
        None
    } else {
        Some(candidates)
    }
}

fn bottom_offset(total_lines: usize, viewport_lines: usize) -> usize {
    total_lines.saturating_sub(viewport_lines)
}

fn spinner_glyph(frame: usize, unicode: bool) -> &'static str {
    if unicode {
        SPINNER_FRAMES[frame % SPINNER_FRAMES.len()]
    } else {
        ASCII_SPINNER_FRAMES[frame % ASCII_SPINNER_FRAMES.len()]
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        build_rows, compute_diff, estimate_streaming_rate_tps, estimate_tokens_from_chars,
        AppState, DiffKind, DiffLine, ModelsPicker, ModelsPickerRowKind, Phase, SessionPicker,
        SessionRow, SessionSnapshot, StatusSnapshot, ToolCard, ToolCardStatus, TranscriptBlock,
        PASTE_TAIL_QUIET_FALLBACK,
    };
    use crate::agent::AgentStatus;
    use crate::config::{AuthType, ProviderKind, ProviderProfile};
    use crate::permission::{PermissionMode, PermissionReply};
    use crate::provider::Usage;
    use crate::session::SessionSummary;
    use crate::tool::ask::{Answer, Question, QuestionOption};
    use crate::tool::plan::{Plan, PlanDecision, PlanProgressUpdate, PlanStep, StepStatus};
    use crate::tool::ToolOutcome;
    use crate::tui::channel::{
        AgentEvent, PermissionRequest, PlanApprovalRequest, QuestionRequest, UserInput,
    };
    use crate::tui::command::Command;
    use crate::tui::input_batch::{PasteTailMatcher, TailAction};
    use crate::tui::input_buffer::InputBufferAction;
    use crate::tui::selection::{Point, SelectionAction};
    use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
    use serde::{de::DeserializeOwned, Serialize};
    use serde_json::json;
    use std::collections::BTreeMap;
    use std::fmt::Debug;
    use std::path::PathBuf;
    use std::time::{Duration, Instant};
    use tokio::sync::{mpsc, oneshot};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn key_with_kind(code: KeyCode, kind: KeyEventKind) -> KeyEvent {
        KeyEvent::new_with_kind(code, KeyModifiers::NONE, kind)
    }

    fn key_with_modifiers_and_kind(
        code: KeyCode,
        modifiers: KeyModifiers,
        kind: KeyEventKind,
    ) -> KeyEvent {
        KeyEvent::new_with_kind(code, modifiers, kind)
    }

    fn assert_input_state(state: &AppState, text: &str, cursor: usize) {
        assert_eq!(state.input_line.text(), text);
        assert_eq!(state.input_line.cursor, cursor);
    }

    fn selection_point(col: u16, row: u16) -> Point {
        Point { col, row }
    }

    fn create_selection(state: &mut AppState) {
        state.apply_selection_action(SelectionAction::Press(selection_point(2, 1)));
        state.apply_selection_action(SelectionAction::Drag(selection_point(6, 1)));
        state.apply_selection_action(SelectionAction::Release(selection_point(6, 1)));
    }

    fn diff_line(kind: DiffKind, text: &str) -> DiffLine {
        DiffLine {
            kind,
            text: text.to_string(),
        }
    }

    fn session() -> SessionSnapshot {
        SessionSnapshot {
            provider: "anthropic".to_string(),
            model: "claude-test".to_string(),
            max_iterations: 8,
            cwd: PathBuf::from("workspace"),
            tools: 7,
        }
    }

    fn test_profile(id: &str, model: &str, kind: ProviderKind) -> ProviderProfile {
        ProviderProfile {
            id: id.to_string(),
            kind,
            base_url: None,
            model: model.to_string(),
            auth_type: AuthType::ApiKey,
        }
    }

    fn assert_json_round_trip<T>(value: T)
    where
        T: Serialize + DeserializeOwned + PartialEq + Debug,
    {
        let encoded = serde_json::to_string(&value).expect("value should serialize");
        let decoded: T = serde_json::from_str(&encoded).expect("value should deserialize");
        assert_eq!(decoded, value);
    }

    fn status_snapshot() -> StatusSnapshot {
        StatusSnapshot {
            provider: "anthropic".to_string(),
            model: "claude-test".to_string(),
            iteration: 2,
            max_iterations: 8,
            messages: 5,
            cwd: PathBuf::from("workspace"),
            tools: 7,
        }
    }

    fn tool_card(status: ToolCardStatus, exit: Option<i32>) -> ToolCard {
        ToolCard {
            id: format!("call-{status:?}-{exit:?}"),
            name: "shell".to_string(),
            args: json!({
                "command": "echo hello",
                "metadata": {
                    "cwd": "workspace",
                    "attempt": 1
                }
            }),
            readonly: false,
            status,
            output: Some("hello\n".to_string()),
            truncated: exit.is_none(),
            exit,
        }
    }

    #[test]
    fn transcript_block_round_trips_all_variants() {
        let blocks = vec![
            TranscriptBlock::User("user input".to_string()),
            TranscriptBlock::Assistant("assistant answer".to_string()),
            TranscriptBlock::Tool(tool_card(ToolCardStatus::Done, Some(0))),
            TranscriptBlock::Error("model failed".to_string()),
            TranscriptBlock::Help,
            TranscriptBlock::Status(status_snapshot()),
            TranscriptBlock::Notice("session saved".to_string()),
            TranscriptBlock::Thinking("reasoning trace".to_string()),
        ];

        for block in blocks {
            assert_json_round_trip(block);
        }
    }

    #[test]
    fn tool_card_round_trips_args_exit_and_statuses() {
        let cards = [
            tool_card(ToolCardStatus::Running, None),
            tool_card(ToolCardStatus::Done, Some(0)),
            tool_card(ToolCardStatus::Error, Some(2)),
        ];

        for card in cards {
            assert_json_round_trip(card);
        }
    }

    #[test]
    fn tool_card_status_round_trips_all_variants() {
        for status in [
            ToolCardStatus::Running,
            ToolCardStatus::Done,
            ToolCardStatus::Error,
        ] {
            assert_json_round_trip(status);
        }
    }

    #[test]
    fn status_snapshot_round_trips() {
        assert_json_round_trip(status_snapshot());
    }

    fn wps_openai_profiles() -> BTreeMap<String, ProviderProfile> {
        BTreeMap::from([
            (
                "wps".to_string(),
                test_profile("wps", "zhipu/glm-5.2", ProviderKind::OpenAi),
            ),
            (
                "openai".to_string(),
                test_profile("openai", "gpt-5.5", ProviderKind::OpenAi),
            ),
        ])
    }

    // --- 消息排队 Task 1.1 (RED) ---

    #[test]
    fn enqueue_prompt_appends_to_queue_tail_in_fifo_order() {
        let mut state = AppState::new();
        state.enqueue_prompt("x".to_string());
        state.enqueue_prompt("y".to_string());
        assert_eq!(state.pending_queue, vec!["x".to_string(), "y".to_string()]);
        assert!(state.has_queue());
    }

    #[test]
    fn dequeue_next_on_empty_queue_returns_none_without_side_effects() {
        let mut state = AppState::new();
        state.phase = Phase::CallingModel;
        state
            .transcript
            .push(TranscriptBlock::User("existing".to_string()));

        assert_eq!(state.dequeue_next(), None);
        assert_eq!(state.phase, Phase::CallingModel);
        assert_eq!(
            state.transcript,
            vec![TranscriptBlock::User("existing".to_string())]
        );
    }

    #[test]
    fn dequeue_next_pops_front_sets_busy_and_appends_user_transcript() {
        let mut state = AppState::new();
        state.enqueue_prompt("x".to_string());
        state.enqueue_prompt("y".to_string());

        assert_eq!(state.dequeue_next(), Some("x".to_string()));
        assert_eq!(state.phase, Phase::Busy);
        assert_eq!(
            state.transcript.last(),
            Some(&TranscriptBlock::User("x".to_string()))
        );
        assert_eq!(state.pending_queue, vec!["y".to_string()]);
    }

    #[test]
    fn dequeue_next_resets_turn_token_usage() {
        let mut state = AppState::new();
        state.record_usage(
            Usage {
                input_tokens: 0,
                output_tokens: 40,
            },
            Duration::from_secs(1),
        );
        assert_eq!(state.output_tokens_this_turn(), 40);
        state.enqueue_prompt("next".to_string());

        let _ = state.dequeue_next();

        assert_eq!(state.output_tokens_this_turn(), 0);
        assert_eq!(state.last_rate_tps(), None);
    }

    #[test]
    fn clear_queue_empties_pending_and_has_queue_returns_false() {
        let mut state = AppState::new();
        state.enqueue_prompt("a".to_string());
        state.enqueue_prompt("b".to_string());
        assert!(state.has_queue());

        state.clear_queue();

        assert!(state.pending_queue.is_empty());
        assert!(!state.has_queue());
    }

    #[test]
    fn last_exit_intent_at_round_trips_through_app_state() {
        let mut state = AppState::new();
        let at = Instant::now();

        assert_eq!(state.last_exit_intent_at(), None);
        state.set_last_exit_intent_at(at);

        assert_eq!(state.last_exit_intent_at(), Some(at));
    }

    fn session_summary(id: &str, created_at: &str, first_user: Option<&str>) -> SessionSummary {
        SessionSummary {
            id: id.to_string(),
            created_at: created_at.to_string(),
            first_user: first_user.map(str::to_string),
        }
    }

    fn session_row(id: &str) -> SessionRow {
        SessionRow {
            id: id.to_string(),
            label: id.to_string(),
        }
    }

    #[test]
    fn session_picker_new_builds_labels_from_summary_fields() {
        let picker = SessionPicker::new(vec![session_summary(
            "1234567890abcdef",
            "2026-07-04T14:30:22Z",
            Some("first prompt"),
        )]);

        assert_eq!(picker.highlighted, 0);
        assert_eq!(picker.rows.len(), 1);
        let row = &picker.rows[0];
        assert_eq!(row.id, "1234567890abcdef");
        assert!(row.label.contains("12345678"));
        assert!(!row.label.contains("1234567890abcdef"));
        assert!(row.label.contains("2026-07-04T14:30:22Z"));
        assert!(row.label.contains("first prompt"));
    }

    #[test]
    fn session_picker_new_handles_missing_first_user() {
        let picker = SessionPicker::new(vec![session_summary(
            "abcdef1234567890",
            "2026-07-04T15:01:02Z",
            None,
        )]);

        assert_eq!(picker.rows.len(), 1);
        let row = &picker.rows[0];
        assert_eq!(row.id, "abcdef1234567890");
        assert!(row.label.contains("abcdef12"));
        assert!(row.label.contains("2026-07-04T15:01:02Z"));
    }

    #[test]
    fn session_picker_move_highlight_clamps_to_bounds() {
        let mut picker = SessionPicker {
            rows: vec![session_row("a"), session_row("b")],
            highlighted: 0,
        };

        picker.move_highlight(-1);
        assert_eq!(picker.highlighted, 0);

        picker.move_highlight(10);
        assert_eq!(picker.highlighted, 1);

        picker.move_highlight(-10);
        assert_eq!(picker.highlighted, 0);
    }

    #[test]
    fn session_picker_key_navigation_moves_highlight_with_clamping() {
        let mut state = AppState::new();
        state.open_session_picker(vec![
            session_summary("session-a", "created-a", Some("first a")),
            session_summary("session-b", "created-b", Some("first b")),
            session_summary("session-c", "created-c", Some("first c")),
        ]);

        assert!(state.handle_session_picker_key(key(KeyCode::Down)));
        assert_eq!(state.session_picker.as_ref().unwrap().highlighted, 1);

        assert!(state.handle_session_picker_key(key(KeyCode::Down)));
        assert_eq!(state.session_picker.as_ref().unwrap().highlighted, 2);

        assert!(state.handle_session_picker_key(key(KeyCode::Down)));
        assert_eq!(state.session_picker.as_ref().unwrap().highlighted, 2);

        assert!(state.handle_session_picker_key(key(KeyCode::Up)));
        assert_eq!(state.session_picker.as_ref().unwrap().highlighted, 1);

        assert!(state.handle_session_picker_key(key(KeyCode::Up)));
        assert_eq!(state.session_picker.as_ref().unwrap().highlighted, 0);

        assert!(state.handle_session_picker_key(key(KeyCode::Up)));
        assert_eq!(state.session_picker.as_ref().unwrap().highlighted, 0);
    }

    #[test]
    fn session_picker_enter_sets_pending_switch_and_closes_picker() {
        let mut state = AppState::new();
        state.open_session_picker(vec![
            session_summary("session-a", "created-a", Some("first a")),
            session_summary("session-b", "created-b", Some("first b")),
        ]);
        state.session_picker.as_mut().unwrap().highlighted = 1;

        assert!(state.handle_session_picker_key(key(KeyCode::Enter)));

        assert!(state.session_picker.is_none());
        assert_eq!(
            state.take_pending_session_switch(),
            Some("session-b".to_string())
        );
        assert_eq!(state.take_pending_session_switch(), None);
    }

    #[test]
    fn session_picker_escape_closes_without_pending_switch() {
        let mut state = AppState::new();
        state.open_session_picker(vec![session_summary(
            "session-a",
            "created-a",
            Some("first a"),
        )]);

        assert!(state.handle_session_picker_key(key(KeyCode::Esc)));

        assert!(state.session_picker.is_none());
        assert_eq!(state.take_pending_session_switch(), None);
    }

    #[test]
    fn session_picker_catch_all_consumes_character_without_input_or_switch() {
        let mut state = AppState::new();
        state.open_session_picker(vec![session_summary(
            "session-a",
            "created-a",
            Some("first a"),
        )]);
        state.set_input_text("keep");

        assert!(state.handle_session_picker_key(key(KeyCode::Char('x'))));

        assert!(state.session_picker.is_some());
        assert_eq!(state.input(), "keep");
        assert_eq!(state.take_pending_session_switch(), None);
    }

    // --- 消息排队 Task 2(提交分流) ---

    #[test]
    fn running_submit_enqueues_without_polluting_current_turn() {
        let (input_tx, mut input_rx) = mpsc::unbounded_channel();
        let mut state = AppState::new();
        state.phase = Phase::CallingModel;
        state.iteration = 3;
        state.record_usage(
            Usage {
                input_tokens: 0,
                output_tokens: 40,
            },
            Duration::from_secs(1),
        );
        let transcript_len_before = state.transcript.len();
        state.set_input_text("queued prompt");

        state.on_key(key(KeyCode::Enter), &input_tx);

        assert_eq!(state.pending_queue, vec!["queued prompt".to_string()]);
        assert_eq!(state.transcript.len(), transcript_len_before);
        assert_eq!(state.iteration, 3);
        assert_eq!(state.output_tokens_this_turn(), 40);
        assert_eq!(state.last_rate_tps(), Some(40.0));
        assert_eq!(state.phase, Phase::CallingModel);
        assert!(input_rx.try_recv().is_err());
    }

    #[test]
    fn ready_with_nonempty_queue_submits_enqueue_not_direct_send() {
        let (input_tx, mut input_rx) = mpsc::unbounded_channel();
        let mut state = AppState::new();
        state.enqueue_prompt("already queued".to_string());
        assert_eq!(state.phase, Phase::Ready);
        state.set_input_text("second prompt");

        state.on_key(key(KeyCode::Enter), &input_tx);

        assert_eq!(
            state.pending_queue,
            vec!["already queued".to_string(), "second prompt".to_string()]
        );
        assert!(state.transcript.is_empty());
        assert_eq!(state.phase, Phase::Ready);
        assert!(input_rx.try_recv().is_err());
    }

    #[test]
    fn ready_with_empty_queue_submits_directly() {
        let (input_tx, mut input_rx) = mpsc::unbounded_channel();
        let mut state = AppState::new();
        assert_eq!(state.phase, Phase::Ready);
        assert!(!state.has_queue());
        state.set_input_text("direct prompt");

        state.on_key(key(KeyCode::Enter), &input_tx);

        assert_eq!(
            input_rx.try_recv().unwrap(),
            UserInput::Prompt("direct prompt".to_string())
        );
        assert_eq!(
            state.transcript,
            vec![TranscriptBlock::User("direct prompt".to_string())]
        );
        assert_eq!(state.phase, Phase::Busy);
        assert!(state.pending_queue.is_empty());
    }

    // --- ModelsPicker §2.1 (卡点 A) ---

    #[test]
    fn app_state_selection_helpers_apply_and_clear_selection() {
        let mut state = AppState::new();
        assert!(!state.has_selection());

        create_selection(&mut state);

        assert!(state.has_selection());
        assert!(!state.selection.dragging);

        state.clear_selection();

        assert!(!state.has_selection());
        assert!(!state.selection.dragging);
    }

    #[test]
    fn enter_submission_clears_selection() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut state = AppState::new();
        create_selection(&mut state);
        assert!(state.has_selection());
        state.set_input_text("hello");

        state.on_key(key(KeyCode::Enter), &tx);

        assert!(!state.has_selection());
        assert_eq!(
            rx.try_recv().unwrap(),
            UserInput::Prompt("hello".to_string())
        );
    }

    #[test]
    fn clear_command_clears_selection() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut state = AppState::new();
        state
            .transcript
            .push(TranscriptBlock::User("old".to_string()));
        create_selection(&mut state);
        assert!(state.has_selection());
        state.set_input_text("/clear");

        state.on_key(key(KeyCode::Enter), &tx);

        assert!(state.transcript.is_empty());
        assert!(!state.has_selection());
        assert!(rx.try_recv().is_err());
    }
    #[test]
    fn build_rows_groups_providers_marks_current_and_headers_not_selectable() {
        let profiles = wps_openai_profiles();
        let rows = build_rows(&profiles, ("wps", "zhipu/glm-5.2"));

        let wps_header = rows
            .iter()
            .find(|row| row.provider_id == "wps" && row.kind == ModelsPickerRowKind::ProviderHeader)
            .expect("wps header");
        assert_eq!(wps_header.model, None);
        assert!(!wps_header.is_current);

        let wps_models: Vec<_> = rows
            .iter()
            .filter(|row| row.provider_id == "wps" && row.kind == ModelsPickerRowKind::Model)
            .collect();
        assert_eq!(wps_models.len(), 8, "wps catalog has 8 models");
        assert!(wps_models
            .iter()
            .any(|row| row.model.as_deref() == Some("zhipu/glm-5.2") && row.is_current));
        assert_eq!(wps_models.iter().filter(|row| row.is_current).count(), 1);

        let openai_header = rows
            .iter()
            .find(|row| {
                row.provider_id == "openai" && row.kind == ModelsPickerRowKind::ProviderHeader
            })
            .expect("openai header");
        assert_eq!(openai_header.model, None);

        let openai_models: Vec<_> = rows
            .iter()
            .filter(|row| row.provider_id == "openai" && row.kind == ModelsPickerRowKind::Model)
            .collect();
        assert_eq!(openai_models.len(), 1);
        assert_eq!(openai_models[0].model.as_deref(), Some("gpt-5.5"));
        assert!(!openai_models[0].is_current);
    }

    #[test]
    fn build_rows_custom_provider_lists_configured_model_only() {
        let profiles = BTreeMap::from([(
            "my-llm".to_string(),
            test_profile("my-llm", "x-1", ProviderKind::OpenAi),
        )]);
        let rows = build_rows(&profiles, ("my-llm", "x-1"));

        let headers: Vec<_> = rows
            .iter()
            .filter(|row| row.kind == ModelsPickerRowKind::ProviderHeader)
            .collect();
        assert_eq!(headers.len(), 1);
        assert_eq!(headers[0].provider_id, "my-llm");

        let models: Vec<_> = rows
            .iter()
            .filter(|row| row.kind == ModelsPickerRowKind::Model)
            .collect();
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].model.as_deref(), Some("x-1"));
    }

    #[test]
    fn move_highlight_skips_headers_wraps_on_model_rows_only() {
        let profiles = wps_openai_profiles();
        let mut picker = ModelsPicker::new(&profiles, ("wps", "zhipu/glm-5.2"));

        let first = picker.highlighted_row().expect("initial highlight").clone();
        assert_eq!(first.kind, ModelsPickerRowKind::Model);

        picker.move_highlight(1);
        assert_eq!(
            picker.highlighted_row().unwrap().kind,
            ModelsPickerRowKind::Model
        );
        assert_ne!(picker.highlighted_row().unwrap().model, first.model);

        for _ in 0..8 {
            picker.move_highlight(1);
            assert_eq!(
                picker.highlighted_row().unwrap().kind,
                ModelsPickerRowKind::Model
            );
        }

        let all_models: Vec<_> = build_rows(&profiles, ("wps", "zhipu/glm-5.2"))
            .into_iter()
            .filter(|row| row.kind == ModelsPickerRowKind::Model)
            .collect();
        let first_in_list = all_models.first().unwrap().model.clone();
        let last_in_list = all_models.last().unwrap().model.clone();

        while picker.highlighted_row().unwrap().model != last_in_list {
            picker.move_highlight(1);
        }
        assert_eq!(picker.highlighted_row().unwrap().model, last_in_list);

        picker.move_highlight(1);
        assert_eq!(picker.highlighted_row().unwrap().model, first_in_list);

        while picker.highlighted_row().unwrap().model != first_in_list {
            picker.move_highlight(-1);
        }
        picker.move_highlight(-1);
        assert_eq!(picker.highlighted_row().unwrap().model, last_in_list);
    }

    #[test]
    fn filter_substring_resets_highlight_to_first_visible_model() {
        let profiles = wps_openai_profiles();
        let mut picker = ModelsPicker::new(&profiles, ("wps", "zhipu/glm-5.2"));

        for c in "glm".chars() {
            picker.push_filter_char(c);
        }
        assert_eq!(picker.filter_text(), "glm");

        let visible = picker.visible_rows();
        assert!(
            visible
                .iter()
                .all(|row| row.kind != ModelsPickerRowKind::ProviderHeader
                    || row.provider_id == "wps"),
            "only wps group should remain when filtering glm"
        );
        assert!(visible
            .iter()
            .filter(|row| row.kind == ModelsPickerRowKind::Model)
            .all(|row| {
                let haystack =
                    format!("{}/{}", row.provider_id, row.model.as_deref().unwrap_or(""));
                haystack.to_lowercase().contains("glm")
            }));

        let highlighted = picker.highlighted_row().expect("highlight after filter");
        assert_eq!(highlighted.kind, ModelsPickerRowKind::Model);
        let first_model = visible
            .iter()
            .find(|row| row.kind == ModelsPickerRowKind::Model)
            .expect("first visible model");
        assert_eq!(highlighted.provider_id, first_model.provider_id);
        assert_eq!(highlighted.model, first_model.model);
    }

    #[test]
    fn filter_no_match_shows_empty_hint_and_enter_is_no_op() {
        let profiles = wps_openai_profiles();
        let mut picker = ModelsPicker::new(&profiles, ("wps", "zhipu/glm-5.2"));

        for c in "zzznomatch".chars() {
            picker.push_filter_char(c);
        }
        assert!(picker.shows_empty_hint());
        assert_eq!(picker.selected(), None);
    }

    #[test]
    fn selected_returns_highlighted_provider_and_model_on_enter() {
        let profiles = wps_openai_profiles();
        let mut picker = ModelsPicker::new(&profiles, ("wps", "zhipu/glm-5.2"));

        while picker.highlighted_row().is_none_or(|row| {
            row.provider_id != "wps" || row.model.as_deref() != Some("zhipu/glm-5")
        }) {
            picker.move_highlight(1);
        }

        assert_eq!(
            picker.selected(),
            Some(("wps".to_string(), "zhipu/glm-5".to_string()))
        );
    }

    #[test]
    fn compute_diff_derives_write_edit_and_shell_from_args_without_reading_files() {
        assert_eq!(
            compute_diff(
                "write_file",
                &json!({ "path": "does-not-need-to-exist.txt", "content": "one\ntwo" }),
            ),
            vec![
                diff_line(DiffKind::Add, "one"),
                diff_line(DiffKind::Add, "two")
            ]
        );

        assert_eq!(
            compute_diff(
                "edit_file",
                &json!({
                    "path": "does-not-need-to-exist.txt",
                    "old_string": "old a\nold b",
                    "new_string": "new a\nnew b"
                }),
            ),
            vec![
                diff_line(DiffKind::Del, "old a"),
                diff_line(DiffKind::Del, "old b"),
                diff_line(DiffKind::Add, "new a"),
                diff_line(DiffKind::Add, "new b"),
            ]
        );

        assert_eq!(
            compute_diff("run_shell", &json!({ "command": "cargo test" })),
            Vec::new()
        );
    }

    #[test]
    fn scroll_offset_follows_bottom_preserves_manual_position_and_clamps() {
        let mut state = AppState::new();

        assert_eq!(state.visible_scroll_offset(20, 5), 15);

        state.page_up(20, 5);
        assert_eq!(state.scroll_offset, 10);
        assert_eq!(state.visible_scroll_offset(30, 5), 10);

        state.page_up(30, 5);
        state.page_up(30, 5);
        state.page_up(30, 5);
        assert_eq!(state.visible_scroll_offset(30, 5), 0);

        state.page_down(30, 5);
        assert_eq!(state.visible_scroll_offset(30, 5), 5);

        for _ in 0..10 {
            state.page_down(30, 5);
        }
        assert_eq!(state.visible_scroll_offset(30, 5), 25);
        assert_eq!(state.visible_scroll_offset(40, 5), 35);
    }

    #[test]
    fn line_scroll_moves_in_small_steps_and_refollows_at_bottom() {
        let mut state = AppState::new();

        state.scroll_up(30, 5, 3);
        assert_eq!(state.visible_scroll_offset(30, 5), 22);
        assert_eq!(state.visible_scroll_offset(40, 5), 22);

        state.scroll_down(40, 5, 2);
        assert_eq!(state.visible_scroll_offset(40, 5), 24);

        state.scroll_down(40, 5, 20);
        assert_eq!(state.visible_scroll_offset(40, 5), 35);
        assert_eq!(state.visible_scroll_offset(50, 5), 45);
    }

    #[test]
    fn boundary_scroll_jumps_to_top_and_returns_to_followed_bottom() {
        let mut state = AppState::new();

        assert_eq!(state.visible_scroll_offset(40, 5), 35);
        assert!(state.follows_bottom);

        state.scroll_up(40, 5, 7);
        assert_eq!(state.scroll_offset, 28);
        assert_eq!(state.visible_scroll_offset(40, 5), 28);
        assert!(!state.follows_bottom);

        state.scroll_to_top(40, 5);
        assert_eq!(state.scroll_offset, 0);
        assert_eq!(state.visible_scroll_offset(40, 5), 0);
        assert!(!state.follows_bottom);

        state.scroll_to_bottom(40, 5);
        assert!(state.follows_bottom);
        assert_eq!(state.visible_scroll_offset(40, 5), 35);
    }

    #[test]
    fn advance_spinner_cycles_frame_index() {
        let mut state = AppState::new();

        for expected in 1..=9 {
            state.advance_spinner();
            assert_eq!(state.spinner_frame, expected);
        }

        state.advance_spinner();
        assert_eq!(state.spinner_frame, 0);
    }

    #[test]
    fn app_state_tracks_session_snapshot_and_iteration_counter() {
        let mut state = AppState::with_session(session());
        let (tx, _rx) = mpsc::unbounded_channel();

        assert_eq!(state.session.provider, "anthropic");
        assert_eq!(state.session.model, "claude-test");
        assert_eq!(state.session.max_iterations, 8);
        assert_eq!(state.session.cwd, PathBuf::from("workspace"));
        assert_eq!(state.session.tools, 7);

        state.apply(AgentEvent::StatusChanged(AgentStatus::CallingModel));
        assert_eq!(state.iteration, 1);
        state.apply(AgentEvent::StatusChanged(AgentStatus::CallingModel));
        assert_eq!(state.iteration, 2);

        state.apply(AgentEvent::TurnComplete);
        assert_eq!(state.iteration, 0);

        state.apply(AgentEvent::StatusChanged(AgentStatus::CallingModel));
        assert_eq!(state.iteration, 1);
        state.apply(AgentEvent::TurnComplete);
        assert_eq!(state.iteration, 0);
        state.set_input_text("next prompt");
        state.on_key(key(KeyCode::Enter), &tx);
        assert_eq!(state.iteration, 0);
    }

    #[test]
    fn slash_commands_clear_help_status_exit_and_do_not_submit_prompt() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut state = AppState::with_session(session());
        state
            .transcript
            .push(TranscriptBlock::User("old".to_string()));

        state.set_input_text("/clear");
        state.on_key(key(KeyCode::Enter), &tx);
        assert!(state.transcript.is_empty());
        assert!(rx.try_recv().is_err());

        state.set_input_text("/help");
        state.on_key(key(KeyCode::Enter), &tx);
        assert_eq!(state.transcript, vec![TranscriptBlock::Help]);

        state.iteration = 3;
        state.set_input_text("/status");
        state.on_key(key(KeyCode::Enter), &tx);
        assert_eq!(
            state.transcript.last(),
            Some(&TranscriptBlock::Status(StatusSnapshot {
                provider: "anthropic".to_string(),
                model: "claude-test".to_string(),
                iteration: 3,
                max_iterations: 8,
                messages: 0,
                cwd: PathBuf::from("workspace"),
                tools: 7,
            }))
        );

        state.set_input_text("/exit");
        state.on_key(key(KeyCode::Enter), &tx);
        assert!(state.should_exit);
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn placeholder_and_unknown_commands_append_notice_without_agent_input() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut state = AppState::with_session(session());

        for input in ["/login", "/logout", "/xyz"] {
            state.set_input_text(input);
            state.on_key(key(KeyCode::Enter), &tx);
        }

        assert!(matches!(
            &state.transcript[0],
            TranscriptBlock::Notice(text) if text.contains("未知命令") && text.contains("login")
        ));
        assert!(matches!(
            &state.transcript[1],
            TranscriptBlock::Notice(text) if text.contains("未知命令") && text.contains("logout")
        ));
        assert!(matches!(
            &state.transcript[2],
            TranscriptBlock::Notice(text) if text.contains("未知命令") && text.contains("xyz")
        ));
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn status_snapshot_counts_only_user_and_assistant_dialog_messages() {
        let mut state = AppState::with_session(session());
        state
            .transcript
            .push(TranscriptBlock::User("u1".to_string()));
        state.transcript.push(TranscriptBlock::Tool(ToolCard {
            id: "call-1".to_string(),
            name: "read_file".to_string(),
            args: json!({ "path": "note.txt" }),
            readonly: true,
            status: ToolCardStatus::Done,
            output: Some("tool output".to_string()),
            truncated: false,
            exit: None,
        }));
        state.transcript.push(TranscriptBlock::Help);
        state
            .transcript
            .push(TranscriptBlock::Assistant("a1".to_string()));
        state
            .transcript
            .push(TranscriptBlock::Notice("notice".to_string()));
        state
            .transcript
            .push(TranscriptBlock::Error("fatal".to_string()));

        assert_eq!(state.status_snapshot().messages, 2);
    }

    #[test]
    fn model_command_shows_current_model_or_sends_set_model() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut state = AppState::with_session(session());

        state.set_input_text("/model");
        state.on_key(key(KeyCode::Enter), &tx);
        assert!(matches!(
            state.transcript.last(),
            Some(TranscriptBlock::Notice(text))
                if text.contains("claude-test") && text.contains("model")
        ));

        state.set_input_text("/model claude-next");
        state.on_key(key(KeyCode::Enter), &tx);
        assert_eq!(state.session.model, "claude-next");
        assert_eq!(
            rx.try_recv().unwrap(),
            UserInput::SetModel("claude-next".to_string())
        );
    }

    #[test]
    fn apply_tool_call_started_adds_running_tool_block() {
        let mut state = AppState::new();

        state.apply(AgentEvent::ToolCallStarted {
            id: "call-1".to_string(),
            name: "read_file".to_string(),
            args: json!({ "path": "note.txt" }),
            readonly: true,
        });

        assert_eq!(
            state.transcript,
            vec![TranscriptBlock::Tool(ToolCard {
                id: "call-1".to_string(),
                name: "read_file".to_string(),
                args: json!({ "path": "note.txt" }),
                readonly: true,
                status: ToolCardStatus::Running,
                output: None,
                truncated: false,
                exit: None,
            })]
        );
    }

    #[test]
    fn tool_events_are_inserted_into_transcript_timeline() {
        let mut state = AppState::new();
        state
            .transcript
            .push(TranscriptBlock::User("inspect config".to_string()));

        state.apply(AgentEvent::ToolCallStarted {
            id: "call-1".to_string(),
            name: "read_file".to_string(),
            args: json!({ "path": "src/config.rs" }),
            readonly: true,
        });

        assert_eq!(state.transcript.len(), 2);
        assert!(matches!(
            &state.transcript[1],
            TranscriptBlock::Tool(card)
                if card.id == "call-1" && card.status == ToolCardStatus::Running
        ));

        state.apply(AgentEvent::ToolCallFinished {
            id: "call-1".to_string(),
            outcome: ToolOutcome {
                content: "config contents".to_string(),
                is_error: false,
                truncated: false,
                exit: None,
            },
        });

        assert_eq!(state.transcript.len(), 2);
        assert!(matches!(
            &state.transcript[1],
            TranscriptBlock::Tool(card)
                if card.status == ToolCardStatus::Done
                    && card.output.as_deref() == Some("config contents")
        ));
    }

    #[test]
    fn apply_tool_call_finished_updates_tool_block_to_done_or_error() {
        let mut state = AppState::new();
        state.apply(AgentEvent::ToolCallStarted {
            id: "call-1".to_string(),
            name: "write_file".to_string(),
            args: json!({ "path": "note.txt" }),
            readonly: false,
        });
        state.apply(AgentEvent::ToolCallFinished {
            id: "call-1".to_string(),
            outcome: ToolOutcome {
                content: "wrote note.txt".to_string(),
                is_error: false,
                truncated: false,
                exit: None,
            },
        });

        let TranscriptBlock::Tool(card) = &state.transcript[0] else {
            panic!("expected tool block");
        };
        assert_eq!(card.status, ToolCardStatus::Done);
        assert_eq!(card.output.as_deref(), Some("wrote note.txt"));

        state.apply(AgentEvent::ToolCallStarted {
            id: "call-2".to_string(),
            name: "run_shell".to_string(),
            args: json!({ "command": "false" }),
            readonly: false,
        });
        state.apply(AgentEvent::ToolCallFinished {
            id: "call-2".to_string(),
            outcome: ToolOutcome {
                content: "command failed: permission denied".to_string(),
                is_error: true,
                truncated: true,
                exit: None,
            },
        });

        let TranscriptBlock::Tool(card) = &state.transcript[1] else {
            panic!("expected tool block");
        };
        assert_eq!(card.status, ToolCardStatus::Error);
        assert_eq!(
            card.output.as_deref(),
            Some("command failed: permission denied")
        );
        assert!(card.truncated);
    }

    #[test]
    fn apply_tool_call_finished_without_matching_started_tool_is_ignored() {
        let mut state = AppState::new();
        state
            .transcript
            .push(TranscriptBlock::User("keep me".to_string()));

        state.apply(AgentEvent::ToolCallFinished {
            id: "missing".to_string(),
            outcome: ToolOutcome {
                content: "ignored".to_string(),
                is_error: false,
                truncated: false,
                exit: Some(0),
            },
        });

        assert_eq!(
            state.transcript,
            vec![TranscriptBlock::User("keep me".to_string())]
        );
    }

    #[test]
    fn apply_status_changed_idle_does_not_set_ready() {
        let mut state = AppState::new();
        state.phase = Phase::CallingModel;

        state.apply(AgentEvent::StatusChanged(AgentStatus::Idle));

        assert_eq!(state.phase, Phase::CallingModel);
    }

    #[test]
    fn apply_status_changed_updates_full_phase() {
        let mut state = AppState::new();

        state.apply(AgentEvent::StatusChanged(AgentStatus::ExecutingTool(
            "write_file".to_string(),
        )));

        assert_eq!(state.phase, Phase::ExecutingTool("write_file".to_string()));
    }

    #[test]
    fn apply_text_delta_accumulates_current_assistant_block() {
        let mut state = AppState::new();

        state.apply(AgentEvent::TextDelta("hello".to_string()));
        state.apply(AgentEvent::TextDelta(" world".to_string()));

        assert_eq!(
            state.transcript,
            vec![TranscriptBlock::Assistant("hello world".to_string())]
        );
        assert_eq!(state.phase, Phase::Busy);
    }

    #[test]
    fn apply_permission_request_sets_pending_and_waiting_phase() {
        let (tx, _rx) = oneshot::channel();
        let mut state = AppState::new();

        state.apply(AgentEvent::PermissionRequired(PermissionRequest {
            tool_name: "write_file".to_string(),
            args: json!({ "path": "note.txt" }),
            allow_always_key: None,
            responder: tx,
        }));

        assert_eq!(state.phase, Phase::WaitingForPermission);
        let pending = state.pending_permission.as_ref().unwrap();
        assert_eq!(pending.tool_name, "write_file");
        assert_eq!(pending.args, json!({ "path": "note.txt" }));
    }

    #[test]
    fn on_key_edits_text_and_enter_submits_prompt() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut state = AppState::new();

        state.on_key(key(KeyCode::Char('h')), &tx);
        state.on_key(key(KeyCode::Char('i')), &tx);
        state.on_key(key(KeyCode::Backspace), &tx);
        state.on_key(key(KeyCode::Char('!')), &tx);
        state.on_key(key(KeyCode::Enter), &tx);

        assert_eq!(state.input(), "");
        assert_eq!(
            state.transcript,
            vec![TranscriptBlock::User("h!".to_string())]
        );
        assert_eq!(state.phase, Phase::Busy);
        assert_eq!(rx.try_recv().unwrap(), UserInput::Prompt("h!".to_string()));
    }

    #[test]
    fn enter_submits_expanded_paste_fold_text() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut state = AppState::new();
        state.phase = Phase::Ready;
        state.insert_paste_fold("l1\nl2\nl3".to_string());

        state.on_key(key(KeyCode::Enter), &tx);

        assert_eq!(state.input(), "");
        assert!(state.input_line.pasted.is_empty());
        assert_eq!(
            state.transcript,
            vec![TranscriptBlock::User("l1\nl2\nl3".to_string())]
        );
        assert_eq!(state.phase, Phase::Busy);
        assert_eq!(
            rx.try_recv().unwrap(),
            UserInput::Prompt("l1\nl2\nl3".to_string())
        );
    }

    #[test]
    fn enter_submits_mixed_text_and_paste_fold_in_order() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut state = AppState::new();
        state.phase = Phase::Ready;

        state.on_key(key(KeyCode::Char('a')), &tx);
        state.insert_paste_fold("X\nY".to_string());
        state.on_key(key(KeyCode::Char('b')), &tx);
        state.on_key(key(KeyCode::Enter), &tx);

        assert_eq!(state.input(), "");
        assert_eq!(
            state.transcript,
            vec![TranscriptBlock::User("aX\nYb".to_string())]
        );
        assert_eq!(
            rx.try_recv().unwrap(),
            UserInput::Prompt("aX\nYb".to_string())
        );
    }

    #[test]
    fn newline_keys_insert_newline_without_submitting() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut state = AppState::new();

        for key_event in [
            KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL),
            KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT),
            KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL),
        ] {
            state.on_key(key_event, &tx);
        }

        assert_eq!(state.input(), "\n\n\n");
        assert!(state.transcript.is_empty());
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn multiline_slash_text_submits_as_prompt_not_command() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut state = AppState::new();
        state
            .transcript
            .push(TranscriptBlock::User("old".to_string()));
        state.set_input_text("/clear\nbody");

        state.on_key(key(KeyCode::Enter), &tx);

        assert_eq!(
            state.transcript,
            vec![
                TranscriptBlock::User("old".to_string()),
                TranscriptBlock::User("/clear\nbody".to_string())
            ]
        );
        assert_eq!(
            rx.try_recv().unwrap(),
            UserInput::Prompt("/clear\nbody".to_string())
        );
    }

    #[test]
    fn ctrl_chars_are_filtered_except_newline_and_altgr() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let mut state = AppState::new();

        state.on_key(
            KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL),
            &tx,
        );
        assert_eq!(state.input(), "");

        state.on_key(
            KeyEvent::new(KeyCode::Char('J'), KeyModifiers::CONTROL),
            &tx,
        );
        assert_eq!(state.input(), "\n");

        state.on_key(
            KeyEvent::new(
                KeyCode::Char('@'),
                KeyModifiers::CONTROL | KeyModifiers::ALT,
            ),
            &tx,
        );
        assert_eq!(state.input(), "\n@");
    }

    #[test]
    fn cursor_keys_edit_at_cursor_and_line_boundaries() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let mut state = AppState::new();

        state.set_input_text("a你b");
        state.on_key(key(KeyCode::Left), &tx);
        assert_eq!(state.input_line.cursor, "a你".len());
        state.on_key(key(KeyCode::Left), &tx);
        assert_eq!(state.input_line.cursor, "a".len());
        state.on_key(key(KeyCode::Right), &tx);
        assert_eq!(state.input_line.cursor, "a你".len());
        state.on_key(key(KeyCode::Left), &tx);
        state.on_key(key(KeyCode::Delete), &tx);
        assert_eq!(state.input(), "ab");
        assert_eq!(state.input_line.cursor, "a".len());

        state.set_input_text("ab\ncd你");
        state.on_key(key(KeyCode::Left), &tx);
        state.on_key(key(KeyCode::Left), &tx);
        assert_eq!(state.input_line.cursor, "ab\nc".len());
        state.on_key(key(KeyCode::Home), &tx);
        assert_eq!(state.input_line.cursor, "ab\n".len());
        state.on_key(key(KeyCode::End), &tx);
        assert_eq!(state.input_line.cursor, "ab\ncd你".len());
    }

    #[test]
    fn up_down_move_inside_multiline_before_history_in_app() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let mut state = AppState::new();
        state.set_input_text("old");
        state.on_key(key(KeyCode::Enter), &tx);
        state.set_input_text("aa\nbb");

        state.on_key(key(KeyCode::Up), &tx);
        assert_eq!(state.input(), "aa\nbb");
        assert_eq!(state.input_line.cursor, "aa".len());
        assert_eq!(state.input_line.history_cursor, None);

        state.on_key(key(KeyCode::Up), &tx);
        assert_eq!(state.input(), "old");
        assert_eq!(state.input_line.history_cursor, Some(0));

        state.on_key(key(KeyCode::Down), &tx);
        assert_eq!(state.input(), "aa\nbb");
        assert_eq!(state.input_line.cursor, "aa\nbb".len());
        assert_eq!(state.input_line.history_cursor, None);
    }

    #[test]
    fn pending_permission_blocks_newline_char_and_editing_keys() {
        let (input_tx, _input_rx) = mpsc::unbounded_channel();
        let keys = [
            KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL),
            key(KeyCode::Char('x')),
            key(KeyCode::Left),
            key(KeyCode::Home),
            key(KeyCode::Delete),
        ];

        for key_event in keys {
            let (permission_tx, _permission_rx) = oneshot::channel();
            let mut state = AppState::new();
            state.set_input_text("keep");
            state.input_line.cursor = "ke".len();
            state.apply(AgentEvent::PermissionRequired(PermissionRequest {
                tool_name: "write_file".to_string(),
                args: json!({}),
                allow_always_key: None,
                responder: permission_tx,
            }));

            state.on_key(key_event, &input_tx);

            assert_input_state(&state, "keep", "ke".len());
        }
    }

    #[test]
    fn models_picker_blocks_newline_cursor_and_editing_keys_from_input_buffer() {
        let (input_tx, _input_rx) = mpsc::unbounded_channel();
        let keys = [
            KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL),
            key(KeyCode::Char('z')),
            key(KeyCode::Left),
            key(KeyCode::Home),
            key(KeyCode::Delete),
        ];

        for key_event in keys {
            let mut state = AppState::new();
            state.provider_profiles = wps_openai_profiles();
            state.session.provider = "wps".to_string();
            state.session.model = "zhipu/glm-5.2".to_string();
            state.set_input_text("keep");
            state.input_line.cursor = "ke".len();
            state.execute_command(Command::Models, &input_tx);
            assert!(state.models_picker.is_some());

            state.on_key(key_event, &input_tx);

            assert_input_state(&state, "keep", "ke".len());
        }
    }

    #[test]
    fn on_key_ignores_non_press_events_to_avoid_duplicate_input() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let mut state = AppState::new();

        state.on_key(key_with_kind(KeyCode::Char('a'), KeyEventKind::Press), &tx);
        state.on_key(
            key_with_kind(KeyCode::Char('a'), KeyEventKind::Release),
            &tx,
        );
        state.on_key(key_with_kind(KeyCode::Char('a'), KeyEventKind::Repeat), &tx);

        assert_eq!(state.input(), "a");
    }

    #[test]
    fn ctrl_o_is_reserved_for_tool_expansion_and_never_enters_input() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut state = AppState::new();

        assert!(!state.tools_expanded);

        state.on_key(
            key_with_modifiers_and_kind(
                KeyCode::Char('o'),
                KeyModifiers::CONTROL,
                KeyEventKind::Press,
            ),
            &tx,
        );

        assert!(
            state.tools_expanded,
            "ctrl+o Press should expand folded tool cards"
        );
        assert_eq!(
            state.input(),
            "",
            "ctrl+o should toggle tool expansion instead of typing 'o'"
        );
        assert!(rx.try_recv().is_err());

        state.on_key(
            key_with_modifiers_and_kind(
                KeyCode::Char('o'),
                KeyModifiers::CONTROL,
                KeyEventKind::Press,
            ),
            &tx,
        );

        assert!(
            !state.tools_expanded,
            "second ctrl+o Press should fold tool cards again"
        );
        assert_eq!(state.input(), "");

        state.set_input_text("keep");
        state.on_key(
            key_with_modifiers_and_kind(
                KeyCode::Char('o'),
                KeyModifiers::CONTROL,
                KeyEventKind::Release,
            ),
            &tx,
        );
        state.on_key(
            key_with_modifiers_and_kind(
                KeyCode::Char('o'),
                KeyModifiers::CONTROL,
                KeyEventKind::Repeat,
            ),
            &tx,
        );

        assert!(!state.tools_expanded);
        assert_eq!(state.input(), "keep");
    }

    #[test]
    fn ctrl_o_toggles_during_pending_permission_without_answering_it() {
        let (input_tx, _input_rx) = mpsc::unbounded_channel();
        let (allow_tx, mut allow_rx) = oneshot::channel();
        let mut state = AppState::new();
        state.apply(AgentEvent::PermissionRequired(PermissionRequest {
            tool_name: "write_file".to_string(),
            args: json!({}),
            allow_always_key: None,
            responder: allow_tx,
        }));

        state.on_key(
            key_with_modifiers_and_kind(
                KeyCode::Char('o'),
                KeyModifiers::CONTROL,
                KeyEventKind::Press,
            ),
            &input_tx,
        );

        assert!(state.tools_expanded);
        assert!(state.pending_permission.is_some());
        assert_eq!(state.phase, Phase::WaitingForPermission);
        assert!(allow_rx.try_recv().is_err());

        state.on_key(key(KeyCode::Char('y')), &input_tx);

        assert_eq!(allow_rx.try_recv().unwrap(), PermissionReply::AllowOnce);
        assert!(state.pending_permission.is_none());
        assert_eq!(state.phase, Phase::Busy);
        assert!(state.tools_expanded);
    }

    #[test]
    fn ctrl_o_toggles_during_running_phase_without_interrupting_escape() {
        let (input_tx, mut input_rx) = mpsc::unbounded_channel();
        let (interrupt_tx, mut interrupt_rx) = mpsc::unbounded_channel();
        let mut state = AppState::new();
        state.phase = Phase::CallingModel;

        state.on_key_with_interrupt(
            key_with_modifiers_and_kind(
                KeyCode::Char('o'),
                KeyModifiers::CONTROL,
                KeyEventKind::Press,
            ),
            &input_tx,
            &interrupt_tx,
        );

        assert!(state.tools_expanded);
        assert_eq!(state.phase, Phase::CallingModel);
        assert!(input_rx.try_recv().is_err());
        assert!(interrupt_rx.try_recv().is_err());

        state.on_key_with_interrupt(key(KeyCode::Esc), &input_tx, &interrupt_tx);

        assert_eq!(interrupt_rx.try_recv().unwrap(), UserInput::Interrupt);
        assert_eq!(state.phase, Phase::CallingModel);
        assert!(state.tools_expanded);
    }

    #[test]
    fn running_ctrl_c_sends_interrupt_without_exiting() {
        let (input_tx, mut input_rx) = mpsc::unbounded_channel();
        let (interrupt_tx, mut interrupt_rx) = mpsc::unbounded_channel();
        let mut state = AppState::new();
        state.phase = Phase::CallingModel;

        state.on_key_with_interrupt(
            key_with_modifiers_and_kind(
                KeyCode::Char('c'),
                KeyModifiers::CONTROL,
                KeyEventKind::Press,
            ),
            &input_tx,
            &interrupt_tx,
        );

        assert_eq!(interrupt_rx.try_recv().unwrap(), UserInput::Interrupt);
        assert!(input_rx.try_recv().is_err());
        assert!(!state.should_exit);
        assert_eq!(state.phase, Phase::CallingModel);
    }

    fn completion_names(state: &AppState) -> Vec<&'static str> {
        state
            .command_completion
            .as_ref()
            .expect("completion should be visible")
            .candidates
            .iter()
            .map(|command| command.name)
            .collect()
    }

    fn selected_completion_name(state: &AppState) -> &'static str {
        let completion = state
            .command_completion
            .as_ref()
            .expect("completion should be visible");
        completion.candidates[completion.selected].name
    }

    #[test]
    fn slash_completion_filters_candidates_and_completes_selection() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut state = AppState::new();

        state.on_key(key(KeyCode::Char('/')), &tx);
        assert_eq!(
            completion_names(&state),
            vec!["/help", "/clear", "/model", "/models", "/status", "/exit", "/compact", "/think",]
        );
        assert_eq!(selected_completion_name(&state), "/help");

        state.on_key(key(KeyCode::Char('c')), &tx);
        assert_eq!(completion_names(&state), vec!["/clear", "/compact"]);
        assert_eq!(selected_completion_name(&state), "/clear");

        state.on_key(key(KeyCode::Down), &tx);
        assert_eq!(selected_completion_name(&state), "/compact");

        state.on_key(key(KeyCode::Up), &tx);
        assert_eq!(selected_completion_name(&state), "/clear");

        state.on_key(key(KeyCode::Tab), &tx);
        assert_eq!(state.input(), "/clear");
        assert!(state.command_completion.is_none());
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn command_completion_keeps_char_and_backspace_editing_soft() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let mut state = AppState::new();

        state.on_key(key(KeyCode::Char('/')), &tx);
        state.on_key(key(KeyCode::Char('c')), &tx);
        assert_eq!(state.input(), "/c");
        assert_eq!(completion_names(&state), vec!["/clear", "/compact"]);

        state.on_key(key(KeyCode::Char('o')), &tx);
        assert_eq!(state.input(), "/co");
        assert_eq!(completion_names(&state), vec!["/compact"]);

        state.on_key(key(KeyCode::Backspace), &tx);
        assert_eq!(state.input(), "/c");
        assert_eq!(completion_names(&state), vec!["/clear", "/compact"]);
    }

    #[test]
    fn models_command_opens_picker_and_enter_sends_set_provider() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut state = AppState::new();
        state.provider_profiles = wps_openai_profiles();
        state.session.provider = "wps".to_string();
        state.session.model = "zhipu/glm-5.2".to_string();

        state.set_input_text("/models");
        state.on_key(key(KeyCode::Enter), &tx);
        assert!(state.models_picker.is_some());

        state.on_key(key(KeyCode::Enter), &tx);

        assert!(state.models_picker.is_none());
        assert_eq!(state.session.provider, "wps");
        assert_eq!(state.session.model, "zhipu/glm-5.2");
        match rx.try_recv() {
            Ok(UserInput::SetProvider { id, model }) => {
                assert_eq!(id, "wps");
                assert_eq!(model, "zhipu/glm-5.2");
            }
            other => panic!("expected SetProvider, got {other:?}"),
        }
    }

    #[test]
    fn models_picker_escape_closes_without_set_provider() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut state = AppState::new();
        state.provider_profiles = wps_openai_profiles();
        state.session.provider = "wps".to_string();
        state.session.model = "zhipu/glm-5.2".to_string();
        state.execute_command(Command::Models, &tx);
        assert!(state.models_picker.is_some());

        state.on_key(key(KeyCode::Esc), &tx);

        assert!(state.models_picker.is_none());
        assert_eq!(state.session.model, "zhipu/glm-5.2");
        assert_eq!(state.session.provider, "wps");
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn slash_completion_hides_for_arguments_plain_prompt_and_escape() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let mut state = AppState::new();

        state.set_input_text("/model ");
        state.on_key(key(KeyCode::Char('g')), &tx);
        assert!(state.command_completion.is_none());

        state.set_input_text("hello");
        state.on_key(key(KeyCode::Char('!')), &tx);
        assert!(state.command_completion.is_none());

        state.set_input_text("");
        state.on_key(key(KeyCode::Char('/')), &tx);
        assert!(state.command_completion.is_some());
        state.on_key(key(KeyCode::Esc), &tx);
        assert_eq!(state.input(), "/");
        assert!(state.command_completion.is_none());
    }

    #[test]
    fn on_key_escape_interrupts_running_turn_on_dedicated_channel_only_for_press() {
        let (input_tx, mut input_rx) = mpsc::unbounded_channel();
        let (interrupt_tx, mut interrupt_rx) = mpsc::unbounded_channel();
        let mut state = AppState::new();
        state.phase = Phase::CallingModel;

        state.on_key_with_interrupt(key(KeyCode::Esc), &input_tx, &interrupt_tx);

        assert_eq!(interrupt_rx.try_recv().unwrap(), UserInput::Interrupt);
        assert!(input_rx.try_recv().is_err());
        assert_eq!(state.phase, Phase::CallingModel);

        state.on_key_with_interrupt(
            key_with_kind(KeyCode::Esc, KeyEventKind::Release),
            &input_tx,
            &interrupt_tx,
        );
        state.on_key_with_interrupt(
            key_with_kind(KeyCode::Esc, KeyEventKind::Repeat),
            &input_tx,
            &interrupt_tx,
        );

        assert!(interrupt_rx.try_recv().is_err());
    }

    #[test]
    fn on_key_answers_pending_permission_with_y_or_n() {
        let (input_tx, _input_rx) = mpsc::unbounded_channel();
        let (allow_tx, mut allow_rx) = oneshot::channel();
        let mut allow_state = AppState::new();
        allow_state.apply(AgentEvent::PermissionRequired(PermissionRequest {
            tool_name: "write_file".to_string(),
            args: json!({}),
            allow_always_key: None,
            responder: allow_tx,
        }));

        allow_state.on_key(key(KeyCode::Char('y')), &input_tx);

        assert_eq!(allow_rx.try_recv().unwrap(), PermissionReply::AllowOnce);
        assert!(allow_state.pending_permission.is_none());
        assert_eq!(allow_state.phase, Phase::Busy);

        let (deny_tx, mut deny_rx) = oneshot::channel();
        let mut deny_state = AppState::new();
        deny_state.apply(AgentEvent::PermissionRequired(PermissionRequest {
            tool_name: "write_file".to_string(),
            args: json!({}),
            allow_always_key: None,
            responder: deny_tx,
        }));

        deny_state.on_key(key(KeyCode::Char('n')), &input_tx);

        assert_eq!(deny_rx.try_recv().unwrap(), PermissionReply::Deny);
        assert!(deny_state.pending_permission.is_none());
        assert_eq!(deny_state.phase, Phase::Busy);
    }

    #[test]
    fn on_key_answers_pending_permission_with_enter_or_escape() {
        let (input_tx, _input_rx) = mpsc::unbounded_channel();
        let (allow_tx, mut allow_rx) = oneshot::channel();
        let mut allow_state = AppState::new();
        allow_state.apply(AgentEvent::PermissionRequired(PermissionRequest {
            tool_name: "write_file".to_string(),
            args: json!({}),
            allow_always_key: None,
            responder: allow_tx,
        }));

        allow_state.on_key(key(KeyCode::Enter), &input_tx);

        assert_eq!(allow_rx.try_recv().unwrap(), PermissionReply::AllowOnce);

        let (deny_tx, mut deny_rx) = oneshot::channel();
        let mut deny_state = AppState::new();
        deny_state.apply(AgentEvent::PermissionRequired(PermissionRequest {
            tool_name: "write_file".to_string(),
            args: json!({}),
            allow_always_key: None,
            responder: deny_tx,
        }));

        deny_state.on_key(key(KeyCode::Esc), &input_tx);

        assert_eq!(deny_rx.try_recv().unwrap(), PermissionReply::Deny);
    }

    #[test]
    fn on_key_answers_pending_permission_with_allow_always_only_when_key_exists() {
        let (input_tx, _input_rx) = mpsc::unbounded_channel();
        let (allow_tx, mut allow_rx) = oneshot::channel();
        let mut allow_state = AppState::new();
        allow_state.apply(AgentEvent::PermissionRequired(PermissionRequest {
            tool_name: "run_shell".to_string(),
            args: json!({ "command": "cargo test" }),
            allow_always_key: Some("cargo test".to_string()),
            responder: allow_tx,
        }));

        allow_state.on_key(key(KeyCode::Char('a')), &input_tx);

        assert_eq!(allow_rx.try_recv().unwrap(), PermissionReply::AllowAlways);
        assert!(allow_state.pending_permission.is_none());
        assert_eq!(allow_state.phase, Phase::Busy);

        let (ignored_tx, mut ignored_rx) = oneshot::channel();
        let mut ignored_state = AppState::new();
        ignored_state.apply(AgentEvent::PermissionRequired(PermissionRequest {
            tool_name: "edit_file".to_string(),
            args: json!({ "path": "src/lib.rs" }),
            allow_always_key: None,
            responder: ignored_tx,
        }));

        ignored_state.on_key(key(KeyCode::Char('a')), &input_tx);

        assert!(ignored_rx.try_recv().is_err());
        assert!(ignored_state.pending_permission.is_some());
        assert_eq!(ignored_state.phase, Phase::WaitingForPermission);
    }

    #[test]
    fn record_usage_accumulates_output_tokens_and_computes_rate() {
        let mut state = AppState::new();

        state.record_usage(
            Usage {
                input_tokens: 10,
                output_tokens: 120,
            },
            Duration::from_secs(2),
        );
        assert_eq!(state.output_tokens_this_turn(), 120);
        assert_eq!(state.last_rate_tps(), Some(60.0));

        state.record_usage(
            Usage {
                input_tokens: 5,
                output_tokens: 60,
            },
            Duration::from_secs(1),
        );
        assert_eq!(state.output_tokens_this_turn(), 180);
        assert_eq!(state.last_rate_tps(), Some(60.0));
    }

    #[test]
    fn record_usage_zero_elapsed_has_no_rate_without_panic() {
        let mut state = AppState::new();

        state.record_usage(
            Usage {
                input_tokens: 0,
                output_tokens: 50,
            },
            Duration::ZERO,
        );

        assert_eq!(state.output_tokens_this_turn(), 50);
        assert_eq!(state.last_rate_tps(), None);
    }

    #[test]
    fn turn_complete_resets_output_token_accumulation() {
        let mut state = AppState::new();
        state.record_usage(
            Usage {
                input_tokens: 0,
                output_tokens: 40,
            },
            Duration::from_secs(1),
        );
        assert_eq!(state.output_tokens_this_turn(), 40);
        assert_eq!(state.last_rate_tps(), Some(40.0));

        state.apply(AgentEvent::TurnComplete);

        assert_eq!(state.output_tokens_this_turn(), 0);
        assert_eq!(state.last_rate_tps(), None);
    }

    #[test]
    fn new_prompt_resets_output_token_accumulation() {
        let (input_tx, _input_rx) = mpsc::unbounded_channel();
        let mut state = AppState::new();
        state.record_usage(
            Usage {
                input_tokens: 0,
                output_tokens: 40,
            },
            Duration::from_secs(1),
        );
        assert_eq!(state.output_tokens_this_turn(), 40);
        state.set_input_text("hello");

        state.on_key(key(KeyCode::Enter), &input_tx);

        assert_eq!(state.output_tokens_this_turn(), 0);
        assert_eq!(state.last_rate_tps(), None);
    }

    #[test]
    fn assistant_completion_increments_new_message_count_when_not_following_bottom() {
        let mut state = AppState::new();
        state.page_up(100, 10);

        state.apply(AgentEvent::StatusChanged(AgentStatus::CallingModel));
        state.apply(AgentEvent::StatusChanged(AgentStatus::ExecutingTool(
            "read_file".to_string(),
        )));
        state.apply(AgentEvent::StatusChanged(AgentStatus::CallingModel));
        state.apply(AgentEvent::TurnComplete);

        assert_eq!(state.new_message_count, 1);
    }

    #[test]
    fn assistant_completion_does_not_increment_when_following_bottom() {
        let mut state = AppState::new();

        state.apply(AgentEvent::StatusChanged(AgentStatus::CallingModel));
        state.apply(AgentEvent::TurnComplete);

        assert_eq!(state.new_message_count, 0);
    }

    #[test]
    fn interrupted_bumps_new_message_count_when_not_following_bottom() {
        let mut state = AppState::new();
        state.page_up(100, 10);
        state.phase = Phase::CallingModel;

        state.apply(AgentEvent::Interrupted);

        assert_eq!(state.new_message_count, 1);
        assert_eq!(state.phase, Phase::Ready);
    }

    #[test]
    fn error_bumps_new_message_count_when_not_following_bottom() {
        let mut state = AppState::new();
        state.page_up(100, 10);
        state.phase = Phase::ExecutingTool("read_file".to_string());

        state.apply(AgentEvent::Error("provider failed".to_string()));

        assert_eq!(state.new_message_count, 1);
        assert_eq!(state.phase, Phase::Ready);
    }

    #[test]
    fn user_notice_and_tool_events_do_not_increment_new_message_count() {
        let mut state = AppState::new();
        state.page_up(100, 10);

        state
            .transcript
            .push(TranscriptBlock::User("hi".to_string()));
        state.apply(AgentEvent::Notice("saved".to_string()));
        state.apply(AgentEvent::ToolCallStarted {
            id: "call-1".to_string(),
            name: "read_file".to_string(),
            args: json!({ "path": "note.txt" }),
            readonly: true,
        });
        state.apply(AgentEvent::ToolCallFinished {
            id: "call-1".to_string(),
            outcome: ToolOutcome {
                content: "ok".to_string(),
                is_error: false,
                truncated: false,
                exit: None,
            },
        });

        assert_eq!(state.new_message_count, 0);
    }

    #[test]
    fn scroll_to_bottom_clears_new_message_count() {
        let mut state = AppState::new();
        state.page_up(100, 10);
        state.new_message_count = 3;

        state.scroll_to_bottom(100, 10);

        assert!(state.follows_bottom());
        assert_eq!(state.new_message_count, 0);
    }

    #[test]
    fn estimate_tokens_from_chars_uses_quarter_char_ratio() {
        assert_eq!(estimate_tokens_from_chars(400), 100);
    }

    #[test]
    fn estimate_streaming_rate_tps_uses_chars_per_token_ratio() {
        let rate = estimate_streaming_rate_tps(400, Duration::from_secs(2)).unwrap();
        assert!((rate - 50.0).abs() < f64::EPSILON);
    }

    #[test]
    fn estimate_streaming_rate_tps_zero_elapsed_returns_none() {
        assert_eq!(estimate_streaming_rate_tps(100, Duration::ZERO), None);
    }

    #[test]
    fn record_streaming_chars_sets_approximate_rate_before_real_usage_corrects() {
        let mut state = AppState::new();
        state.record_streaming_chars(400, Duration::from_secs(2));
        assert!(state.last_rate_is_approximate());
        assert_eq!(state.last_rate_tps(), Some(50.0));

        state.record_usage(
            Usage {
                input_tokens: 0,
                output_tokens: 120,
            },
            Duration::from_secs(2),
        );
        assert!(!state.last_rate_is_approximate());
        assert_eq!(state.last_rate_tps(), Some(60.0));
        assert_eq!(state.output_tokens_this_turn(), 120);
    }

    #[test]
    fn backtab_cycles_permission_mode_normal_accept_edits_yolo_plan() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let mut state = AppState::new();

        assert_eq!(state.current_permission_mode(), PermissionMode::Normal);

        state.on_key(key(KeyCode::BackTab), &tx);
        assert_eq!(state.current_permission_mode(), PermissionMode::AcceptEdits);

        state.on_key(key(KeyCode::BackTab), &tx);
        assert_eq!(state.current_permission_mode(), PermissionMode::Yolo);

        state.on_key(key(KeyCode::BackTab), &tx);
        assert_eq!(state.current_permission_mode(), PermissionMode::Plan);

        state.on_key(key(KeyCode::BackTab), &tx);
        assert_eq!(state.current_permission_mode(), PermissionMode::Normal);
    }

    #[test]
    fn backtab_cycles_during_pending_permission_without_answering() {
        let (input_tx, _input_rx) = mpsc::unbounded_channel();
        let (allow_tx, mut allow_rx) = oneshot::channel();
        let mut state = AppState::new();
        state.apply(AgentEvent::PermissionRequired(PermissionRequest {
            tool_name: "write_file".to_string(),
            args: json!({}),
            allow_always_key: None,
            responder: allow_tx,
        }));

        state.on_key(key(KeyCode::BackTab), &input_tx);

        assert_eq!(state.current_permission_mode(), PermissionMode::AcceptEdits);
        assert!(state.pending_permission.is_some());
        assert!(allow_rx.try_recv().is_err());
    }

    #[test]
    fn input_history_up_down_in_main_input_state() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut state = AppState::new();

        state.on_key(key(KeyCode::Char('a')), &tx);
        state.on_key(key(KeyCode::Enter), &tx);
        state.apply(AgentEvent::TurnComplete);
        state.on_key(key(KeyCode::Char('b')), &tx);
        state.on_key(key(KeyCode::Enter), &tx);
        assert!(rx.try_recv().is_ok());
        assert!(rx.try_recv().is_ok());

        state.on_key(key(KeyCode::Up), &tx);
        assert_eq!(state.input(), "b");
        state.on_key(key(KeyCode::Up), &tx);
        assert_eq!(state.input(), "a");
        state.on_key(key(KeyCode::Down), &tx);
        assert_eq!(state.input(), "b");
    }

    #[test]
    fn history_arrows_do_not_apply_when_command_completion_open() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let mut state = AppState::new();

        state.on_key(key(KeyCode::Char('h')), &tx);
        state.on_key(key(KeyCode::Enter), &tx);
        for ch in "/cl".chars() {
            state.on_key(key(KeyCode::Char(ch)), &tx);
        }
        assert!(state.command_completion.is_some());

        state.on_key(key(KeyCode::Up), &tx);
        assert_eq!(
            state.input(),
            "/cl",
            "↑ should move completion, not history"
        );
        assert_eq!(state.input_line.input_history.len(), 1);
    }

    #[test]
    fn history_arrows_do_not_apply_when_models_picker_open() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let mut state = AppState::new();
        state.provider_profiles.insert(
            "mock".to_string(),
            test_profile("mock", "mock-model", ProviderKind::Mock),
        );

        state.on_key(key(KeyCode::Char('x')), &tx);
        state.on_key(key(KeyCode::Enter), &tx);
        state.execute_command(Command::Models, &tx);
        assert!(state.models_picker.is_some());

        state.on_key(key(KeyCode::Up), &tx);
        assert_eq!(
            state.input(),
            "",
            "↑ should move picker highlight, not history"
        );
        assert_eq!(state.input_line.input_history.len(), 1);
    }

    #[test]
    fn history_arrows_do_not_apply_while_pending_permission() {
        let (input_tx, _input_rx) = mpsc::unbounded_channel();
        let (allow_tx, _allow_rx) = oneshot::channel();
        let mut state = AppState::new();

        state.on_key(key(KeyCode::Char('p')), &input_tx);
        state.on_key(key(KeyCode::Enter), &input_tx);
        assert_eq!(state.input_line.input_history.len(), 1);

        state.apply(AgentEvent::PermissionRequired(PermissionRequest {
            tool_name: "run_shell".to_string(),
            args: json!({ "command": "echo" }),
            allow_always_key: None,
            responder: allow_tx,
        }));

        state.on_key(key(KeyCode::Up), &input_tx);
        assert_eq!(state.input(), "");
        assert_eq!(state.input_line.input_history.len(), 1);
    }

    #[test]
    fn backspace_exits_history_in_on_key() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let mut state = AppState::new();

        state.on_key(key(KeyCode::Char('a')), &tx);
        state.on_key(key(KeyCode::Enter), &tx);
        state.on_key(key(KeyCode::Up), &tx);
        assert_eq!(state.input_line.history_cursor, Some(0));

        state.on_key(key(KeyCode::Backspace), &tx);
        assert_eq!(state.input_line.history_cursor, None);
        assert_eq!(state.input(), "");
    }

    fn apply_press_key_batch(
        state: &mut AppState,
        keys: &[KeyEvent],
        input_tx: &mpsc::UnboundedSender<UserInput>,
        interrupt_tx: &mpsc::UnboundedSender<UserInput>,
    ) {
        use crate::tui::app::{
            apply_batch_input_key, flush_merged_input_chars, ApplyBatchKeyResult,
        };
        use crate::tui::input_batch::classify_key_batch;

        let intents = classify_key_batch(keys);
        let mut pending_str = String::new();
        let mut modal_closed_in_batch = false;
        for (key, intent) in keys.iter().zip(intents.iter()) {
            match apply_batch_input_key(
                state,
                *key,
                *intent,
                &mut modal_closed_in_batch,
                &mut pending_str,
                input_tx,
                interrupt_tx,
            ) {
                ApplyBatchKeyResult::Continue => {}
                ApplyBatchKeyResult::BreakBatch => break,
            }
        }
        flush_merged_input_chars(state, &mut pending_str);
    }

    #[test]
    fn batch_char_burst_with_internal_newline_inserts_without_submit() {
        let (input_tx, mut input_rx) = mpsc::unbounded_channel();
        let (interrupt_tx, _interrupt_rx) = mpsc::unbounded_channel();
        let mut state = AppState::new();

        apply_press_key_batch(
            &mut state,
            &[
                key(KeyCode::Char('a')),
                key(KeyCode::Char('b')),
                key(KeyCode::Enter),
                key(KeyCode::Char('x')),
                key(KeyCode::Char('y')),
            ],
            &input_tx,
            &interrupt_tx,
        );

        assert_eq!(state.input(), "ab\nxy");
        assert!(input_rx.try_recv().is_err());
    }

    #[test]
    fn batch_newline_apply_closes_command_completion() {
        let (input_tx, _input_rx) = mpsc::unbounded_channel();
        let (interrupt_tx, _interrupt_rx) = mpsc::unbounded_channel();
        let mut state = AppState::new();
        state.set_input_text("/mod");
        state.refresh_command_completion();
        assert!(state.command_completion.is_some());

        // 批内纯 [Enter, Enter](n=2 → 两个 Newline):首 Enter 走 insert_newline_and_refresh,
        // pending_str 为空故 flush 不触发 refresh —— 关闭 completion 的必须是 Newline 分支自身的
        // refresh(而非 flush 顺带),这样退化掉 insert_newline_and_refresh 的 refresh 时本测试会红。
        apply_press_key_batch(
            &mut state,
            &[key(KeyCode::Enter), key(KeyCode::Enter)],
            &input_tx,
            &interrupt_tx,
        );

        assert!(state.command_completion.is_none());
    }

    #[test]
    fn batch_pending_permission_answers_once_and_drops_rest() {
        let (input_tx, mut input_rx) = mpsc::unbounded_channel();
        let (interrupt_tx, _interrupt_rx) = mpsc::unbounded_channel();
        let (permission_tx, mut permission_rx) = oneshot::channel();
        let mut state = AppState::new();
        state.apply(AgentEvent::PermissionRequired(PermissionRequest {
            tool_name: "write_file".to_string(),
            args: json!({}),
            allow_always_key: None,
            responder: permission_tx,
        }));

        apply_press_key_batch(
            &mut state,
            &[
                key(KeyCode::Enter),
                key(KeyCode::Char('x')),
                key(KeyCode::Char('y')),
            ],
            &input_tx,
            &interrupt_tx,
        );

        assert_eq!(
            permission_rx.try_recv().unwrap(),
            PermissionReply::AllowOnce
        );
        assert!(state.pending_permission.is_none());
        assert_eq!(state.input(), "");
        assert!(!state.input().contains('\n'));
        assert!(input_rx.try_recv().is_err());
    }

    fn pending_question_on_other_row(responder: oneshot::Sender<Answer>) -> AppState {
        let mut state = AppState::new();
        state.apply(AgentEvent::UserQuestionRequired(QuestionRequest {
            question: Question {
                question: "选一个".to_string(),
                options: vec![QuestionOption {
                    label: "选项".to_string(),
                    description: "说明".to_string(),
                }],
                allow_multi: false,
                allow_other: true,
            },
            responder,
        }));
        if let Some(pending) = state.pending_question.as_mut() {
            pending.cursor = 1;
        }
        state
    }

    #[test]
    fn batch_pending_question_other_row_keeps_all_chars_in_supplement() {
        let (input_tx, _input_rx) = mpsc::unbounded_channel();
        let (interrupt_tx, _interrupt_rx) = mpsc::unbounded_channel();
        let (question_tx, _question_rx) = oneshot::channel();
        let mut state = pending_question_on_other_row(question_tx);

        apply_press_key_batch(
            &mut state,
            &[
                key(KeyCode::Char('你')),
                key(KeyCode::Char('好')),
                key(KeyCode::Char('测')),
            ],
            &input_tx,
            &interrupt_tx,
        );

        let supplement = state
            .pending_question
            .as_ref()
            .expect("dialog still open")
            .supplement
            .clone();
        assert_eq!(supplement, "你好测");
        assert_eq!(state.input(), "");
    }

    #[test]
    fn batch_pending_question_submit_breaks_batch_and_drops_rest() {
        let (input_tx, _input_rx) = mpsc::unbounded_channel();
        let (interrupt_tx, _interrupt_rx) = mpsc::unbounded_channel();
        let (question_tx, mut question_rx) = oneshot::channel();
        let mut state = pending_question_on_other_row(question_tx);
        if let Some(pending) = state.pending_question.as_mut() {
            pending.supplement = "已有".to_string();
        }

        apply_press_key_batch(
            &mut state,
            &[
                key(KeyCode::Enter),
                key(KeyCode::Char('x')),
                key(KeyCode::Char('y')),
            ],
            &input_tx,
            &interrupt_tx,
        );

        assert_eq!(
            question_rx.try_recv().unwrap(),
            Answer {
                selected: vec![],
                supplement: Some("已有".to_string()),
            }
        );
        assert!(state.pending_question.is_none());
        assert_eq!(state.input(), "");
    }

    #[test]
    fn batch_models_picker_keeps_all_filter_chars() {
        let (input_tx, _input_rx) = mpsc::unbounded_channel();
        let (interrupt_tx, _interrupt_rx) = mpsc::unbounded_channel();
        let mut state = AppState::new();
        state.provider_profiles = wps_openai_profiles();
        state.execute_command(Command::Models, &input_tx);
        assert!(state.models_picker.is_some());

        apply_press_key_batch(
            &mut state,
            &[
                key(KeyCode::Char('g')),
                key(KeyCode::Char('p')),
                key(KeyCode::Char('t')),
            ],
            &input_tx,
            &interrupt_tx,
        );

        assert_eq!(
            state
                .models_picker
                .as_ref()
                .expect("picker stays open")
                .filter_text(),
            "gpt"
        );
    }

    #[test]
    fn batch_trailing_enter_after_picker_close_is_discarded() {
        let (input_tx, mut input_rx) = mpsc::unbounded_channel();
        let (interrupt_tx, _interrupt_rx) = mpsc::unbounded_channel();
        let mut state = AppState::new();
        state.provider_profiles = wps_openai_profiles();
        state.session.provider = "wps".to_string();
        state.session.model = "zhipu/glm-5.2".to_string();
        state.execute_command(Command::Models, &input_tx);
        assert!(state.models_picker.is_some());

        apply_press_key_batch(
            &mut state,
            &[key(KeyCode::Enter), key(KeyCode::Enter)],
            &input_tx,
            &interrupt_tx,
        );

        assert!(state.models_picker.is_none());
        // 尾随裸 Enter 必须被 modal_closed_in_batch 守卫丢弃:既不落缓冲、也不提交。
        // 断言 input 为空可杀死"删掉守卫"的实现(否则第 2 个 Enter 因 n==2 判 Newline 会插入 "\n")。
        assert_eq!(
            state.input(),
            "",
            "trailing bare Enter after picker close must be discarded, not inserted as newline"
        );
        match input_rx.try_recv() {
            Ok(UserInput::SetProvider { .. }) => {}
            other => panic!("expected SetProvider from picker Enter, got {other:?}"),
        }
        assert!(
            input_rx.try_recv().is_err(),
            "trailing bare Enter after picker close must not submit"
        );
    }

    #[test]
    fn batch_lone_enter_submits_prompt_via_batch_path() {
        let (input_tx, mut input_rx) = mpsc::unbounded_channel();
        let (interrupt_tx, _interrupt_rx) = mpsc::unbounded_channel();
        let mut state = AppState::new();
        state.set_input_text("hi");

        // 孤立 [Enter](n=1 → Submit):经 apply_batch_input_key 末路 flush + on_key 提交,
        // 锁住"burst 判定不误伤正常提交"这条 change 底线。
        apply_press_key_batch(&mut state, &[key(KeyCode::Enter)], &input_tx, &interrupt_tx);

        match input_rx.try_recv() {
            Ok(UserInput::Prompt(text)) => assert_eq!(text, "hi"),
            other => panic!("lone Enter should submit a prompt, got {other:?}"),
        }
        assert_eq!(state.input(), "");
    }

    #[test]
    fn compact_command_starts_compacting_phase_when_ready() {
        use crate::tui::command::Command;

        let (input_tx, mut input_rx) = mpsc::unbounded_channel();
        let mut state = AppState::new();

        state.execute_command(Command::Compact, &input_tx);

        assert_eq!(state.phase, Phase::Compacting);
        assert_eq!(input_rx.try_recv().unwrap(), UserInput::Compact);
    }

    #[test]
    fn compact_command_rejected_while_running_or_queued() {
        use crate::tui::command::Command;

        // 运行中:不发 Compact、phase 不动、回 notice。
        let (input_tx, mut input_rx) = mpsc::unbounded_channel();
        let mut state = AppState::new();
        state.phase = Phase::CallingModel;

        state.execute_command(Command::Compact, &input_tx);

        assert!(
            input_rx.try_recv().is_err(),
            "running: Compact must not be sent"
        );
        assert_eq!(state.phase, Phase::CallingModel);
        assert!(
            matches!(
                state.transcript.last(),
                Some(TranscriptBlock::Notice(text)) if text.contains("/compact")
            ),
            "rejection must leave an explanatory notice"
        );

        // Ready 但有排队:同样拒绝(避免 Compact 插到排队消息之前延迟执行)。
        let mut state = AppState::new();
        state.enqueue_prompt("queued".to_string());

        state.execute_command(Command::Compact, &input_tx);

        assert!(
            input_rx.try_recv().is_err(),
            "queued: Compact must not be sent"
        );
        assert_ne!(state.phase, Phase::Compacting);
    }

    #[test]
    fn apply_compact_done_returns_ready() {
        let mut state = AppState::new();
        state.phase = Phase::Compacting;

        state.apply(AgentEvent::CompactDone);

        assert_eq!(state.phase, Phase::Ready);
    }

    #[test]
    fn copy_hint_active_within_ttl_and_expires_after() {
        use super::COPY_HINT_TTL;
        use std::time::Instant;

        let mut state = AppState::new();
        assert_eq!(state.active_copy_hint(Instant::now()), None);

        state.set_copy_hint("已复制 5 字".to_string());
        let now = Instant::now();
        assert_eq!(state.active_copy_hint(now), Some("已复制 5 字"));
        assert_eq!(
            state.active_copy_hint(now + COPY_HINT_TTL + Duration::from_millis(1)),
            None,
            "hint must expire after COPY_HINT_TTL"
        );
    }

    #[test]
    fn copy_hint_overwrite_replaces_previous_text() {
        use std::time::Instant;

        let mut state = AppState::new();
        state.set_copy_hint("已复制 5 字".to_string());
        state.set_copy_hint("已复制 9 字".to_string());

        assert_eq!(
            state.active_copy_hint(Instant::now()),
            Some("已复制 9 字"),
            "a new copy must overwrite the previous hint"
        );
    }

    #[test]
    fn paste_tail_set_clear_and_active_query() {
        let now = Instant::now();
        let mut state = AppState::new();

        assert!(!state.paste_tail_active());
        state.set_paste_tail(PasteTailMatcher::new("abc".to_string()), now);

        assert!(state.paste_tail_active());
        assert!(state.paste_tail.is_some());
        state.clear_paste_tail();
        assert!(!state.paste_tail_active());
        assert!(state.paste_tail.is_none());
    }

    #[test]
    fn paste_tail_expires_after_quiet_fallback_and_only_drop_refreshes_key_time() {
        let now = Instant::now();
        let mut state = AppState::new();
        state.set_paste_tail(PasteTailMatcher::new("abc".to_string()), now);

        state.record_paste_tail_action(TailAction::Forward, now + Duration::from_secs(1));
        assert!(state.expire_paste_tail(now + PASTE_TAIL_QUIET_FALLBACK));
        assert!(!state.paste_tail_active());

        state.set_paste_tail(PasteTailMatcher::new("abc".to_string()), now);
        state.record_paste_tail_action(TailAction::Drop, now + Duration::from_secs(1));

        assert!(!state.expire_paste_tail(now + PASTE_TAIL_QUIET_FALLBACK));
        assert!(state.paste_tail_active());
        assert!(state.expire_paste_tail(now + Duration::from_secs(1) + PASTE_TAIL_QUIET_FALLBACK));
        assert!(!state.paste_tail_active());
    }

    fn sample_plan() -> Plan {
        Plan {
            title: "Add plan mode".to_string(),
            steps: vec![
                PlanStep {
                    description: "Wire permission gate".to_string(),
                    validation: "cargo test permission passes".to_string(),
                },
                PlanStep {
                    description: "Add update_plan tool".to_string(),
                    validation: "update_plan tests pass".to_string(),
                },
            ],
        }
    }

    fn activate_plan(state: &mut AppState) {
        let (responder, _rx) = oneshot::channel();
        state.apply(AgentEvent::PlanApprovalRequired(PlanApprovalRequest {
            plan: sample_plan(),
            responder,
        }));
        state.answer_pending_plan_approval(PlanDecision::Approve);
    }

    #[test]
    fn approve_activates_current_plan_with_all_pending_steps() {
        let (responder, _rx) = oneshot::channel();
        let mut state = AppState::new();
        state.apply(AgentEvent::PlanApprovalRequired(PlanApprovalRequest {
            plan: sample_plan(),
            responder,
        }));
        state.answer_pending_plan_approval(PlanDecision::Approve);

        let plan = state.current_plan.expect("plan should be active");
        assert_eq!(plan.title, "Add plan mode");
        assert_eq!(plan.steps.len(), 2);
        assert!(plan
            .steps
            .iter()
            .all(|step| step.status == StepStatus::Pending));
    }

    #[test]
    fn reject_does_not_activate_current_plan() {
        let (responder, _rx) = oneshot::channel();
        let mut state = AppState::new();
        state.apply(AgentEvent::PlanApprovalRequired(PlanApprovalRequest {
            plan: sample_plan(),
            responder,
        }));
        state.answer_pending_plan_approval(PlanDecision::Reject("revise".to_string()));

        assert!(state.current_plan.is_none());
    }

    #[test]
    fn plan_progress_done_updates_step_and_validation_result() {
        let mut state = AppState::new();
        activate_plan(&mut state);

        state.apply(AgentEvent::PlanProgress(PlanProgressUpdate {
            step: 1,
            status: StepStatus::Done,
            validation_result: Some("cargo test → 12 passed".to_string()),
        }));

        let plan = state.current_plan.as_ref().unwrap();
        assert_eq!(plan.steps[0].status, StepStatus::Done);
        assert_eq!(
            plan.steps[0].validation_result.as_deref(),
            Some("cargo test → 12 passed")
        );
        assert_eq!(plan.steps[1].status, StepStatus::Pending);
    }

    #[test]
    fn plan_progress_step_zero_is_ignored_without_panic() {
        let mut state = AppState::new();
        activate_plan(&mut state);
        let before = state.current_plan.clone();

        state.apply(AgentEvent::PlanProgress(PlanProgressUpdate {
            step: 0,
            status: StepStatus::Done,
            validation_result: None,
        }));

        assert_eq!(state.current_plan, before);
    }

    #[test]
    fn plan_progress_out_of_bounds_step_is_ignored() {
        let mut state = AppState::new();
        activate_plan(&mut state);
        let before = state.current_plan.clone();

        state.apply(AgentEvent::PlanProgress(PlanProgressUpdate {
            step: 99,
            status: StepStatus::Done,
            validation_result: None,
        }));

        assert_eq!(state.current_plan, before);
    }

    #[test]
    fn plan_progress_without_active_plan_is_ignored() {
        let mut state = AppState::new();
        state.apply(AgentEvent::PlanProgress(PlanProgressUpdate {
            step: 1,
            status: StepStatus::InProgress,
            validation_result: None,
        }));
        assert!(state.current_plan.is_none());
    }

    #[test]
    fn ready_direct_prompt_clears_current_plan() {
        let (input_tx, _input_rx) = mpsc::unbounded_channel();
        let mut state = AppState::new();
        activate_plan(&mut state);
        state.apply(AgentEvent::PlanProgress(PlanProgressUpdate {
            step: 1,
            status: StepStatus::Done,
            validation_result: Some("done".to_string()),
        }));
        state.phase = Phase::Ready;
        state.apply_input_action(InputBufferAction::InsertStr("next task".to_string()));
        state.on_key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()),
            &input_tx,
        );

        assert!(state.current_plan.is_none());
    }

    #[test]
    fn dequeue_next_clears_current_plan() {
        let mut state = AppState::new();
        activate_plan(&mut state);
        state.enqueue_prompt("queued".to_string());

        let _ = state.dequeue_next();

        assert!(state.current_plan.is_none());
    }

    #[test]
    fn enqueue_does_not_clear_current_plan() {
        let mut state = AppState::new();
        activate_plan(&mut state);
        state.phase = Phase::Busy;

        state.enqueue_prompt("queued".to_string());

        assert!(state.current_plan.is_some());
    }

    #[test]
    fn turn_complete_does_not_clear_current_plan() {
        let mut state = AppState::new();
        activate_plan(&mut state);
        state.phase = Phase::Busy;

        state.apply(AgentEvent::TurnComplete);

        assert!(state.current_plan.is_some());
    }

    #[test]
    fn interrupted_does_not_clear_current_plan() {
        let mut state = AppState::new();
        activate_plan(&mut state);
        state.phase = Phase::Busy;

        state.apply(AgentEvent::Interrupted);

        assert!(state.current_plan.is_some());
    }

    #[test]
    fn error_does_not_clear_current_plan() {
        let mut state = AppState::new();
        activate_plan(&mut state);
        state.phase = Phase::Busy;

        state.apply(AgentEvent::Error("boom".to_string()));

        assert!(state.current_plan.is_some());
    }
}
