//! Host-owned status line: persistent items, one inline prompt, badge placement.
//!
//! Hunk's status bar became a host-owned subsystem: consumers push declarative
//! text items and await one line of text, and the host decides what fits and
//! paints it. The store below holds the state machine (set-order items, a FIFO
//! prompt queue, reload and shutdown lifetimes), the layout fits everything
//! into one terminal row deterministically without a theme, and the symbolic
//! tone mapping resolves item colors against the active theme at paint time.

use ratatui::style::{Color, Modifier};
use workdeck_diff::sanitize_terminal_line;
use workdeck_extension_api::{
    ExtensionStatusAlignment, ExtensionStatusAttribute, ExtensionStatusSpan, ExtensionStatusTone,
};

use crate::theme::{AppTheme, ratatui_theme_color};
use crate::{measure_text_width, slice_text_by_width};

/// One symbolic run of status text; the same span vocabulary file views use.
pub type StatusSpan = ExtensionStatusSpan;

/// Key of the residual host item showing the committed file filter.
pub const HOST_FILTER_ITEM_ID: &str = "host:filter";
/// Key of the residual host item showing the one notice channel.
pub const HOST_NOTICE_ITEM_ID: &str = "host:notice";
/// Key of the persistent daemon link condition; it outranks timed notices when the row
/// overflows and stays until the link reconnects.
pub const HOST_DAEMON_ITEM_ID: &str = "host:daemon";

/// One persistent status contribution, keyed by a globally unique id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusItem {
    pub id: String,
    pub spans: Vec<StatusSpan>,
    pub alignment: ExtensionStatusAlignment,
    /// Higher survives longer when the row overflows.
    pub priority: i32,
}

impl StatusItem {
    #[must_use]
    pub fn new(id: impl Into<String>, text: impl Into<String>) -> Self {
        Self::with_tone(id, text, None)
    }

    #[must_use]
    pub fn with_tone(
        id: impl Into<String>,
        text: impl Into<String>,
        tone: Option<ExtensionStatusTone>,
    ) -> Self {
        Self {
            id: id.into(),
            spans: vec![StatusSpan {
                text: text.into(),
                tone,
                attributes: Vec::new(),
            }],
            alignment: ExtensionStatusAlignment::Left,
            priority: 0,
        }
    }
}

/// One prompt the host should draw, normalized from what a consumer asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusPromptRequest {
    /// Monotonic per-store id, so answer state never carries between prompts.
    pub id: u64,
    pub prefix: String,
    pub placeholder: String,
    /// Live text of the field; the store updates it as the user types.
    pub value: String,
    /// Marker naming a third-party owner, painted before the prefix.
    pub attribution: Option<String>,
    /// Keep this host input across content reloads; extension requests never do.
    pub survive_reload: bool,
    /// The native owner opted into per-edit delivery of the live value.
    pub wants_change: bool,
    /// Review generation the request must still match when it settles.
    pub review_generation: Option<u64>,
}

/// The status line as one surface reads it.
#[derive(Debug, Clone, Copy)]
pub struct StatusLineSnapshot<'a> {
    pub items: &'a [StatusItem],
    pub prompt: Option<&'a StatusPromptRequest>,
}

/// Callback run on every prompt edit; `Err` carries the failure detail.
pub type StatusPromptChange = Box<dyn Fn(&str) -> Result<(), String>>;
/// Callback receiving a prompt-edit failure detail, once per prompt.
pub type StatusPromptChangeWarning = Box<dyn Fn(&str)>;
/// Callback deciding whether a prompt's owner still holds authority.
pub type StatusPromptLiveness = Box<dyn Fn() -> bool>;

/// What a consumer asks of the inline prompt; host and extensions share the shape.
#[derive(Default)]
pub struct StatusPromptOptions {
    pub prefix: String,
    pub placeholder: String,
    pub initial: String,
    /// Called on every edit; a failure is reported once and the prompt continues.
    pub on_change: Option<StatusPromptChange>,
}

/// How the host scopes and attributes one prompt request.
#[derive(Default)]
pub struct StatusPromptRequestOptions {
    pub survive_reload: bool,
    pub attribution: Option<String>,
    pub wants_change: bool,
    pub review_generation: Option<u64>,
    /// Whether the requester still holds authority; a dead owner cancels.
    pub is_live: Option<StatusPromptLiveness>,
    /// Receives the failure detail of a failing `on_change`, once per prompt.
    pub on_change_failed: Option<StatusPromptChangeWarning>,
}

/// Who asked for one prompt, so settlement reaches the right consumer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StatusPromptOwner {
    /// The host's own surface, e.g. the file filter.
    Host,
    /// A native extension awaiting `workdeck/prompt/line-complete`.
    Extension {
        extension_index: usize,
        extension_id: String,
        request_id: String,
    },
    /// Workdeck's own bundled tier, e.g. the `/` content search. Shares the
    /// extension prompt's editing behavior without extension attribution.
    Vendor,
}

/// One resolved prompt and its answer, returned to the surface that owns it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusPromptSettlement {
    pub request: StatusPromptRequest,
    pub owner: StatusPromptOwner,
    /// The submitted text, or `None` on cancel, reload, or teardown.
    pub answer: Option<String>,
}

struct PendingPrompt {
    request: StatusPromptRequest,
    owner: StatusPromptOwner,
    is_live: Option<StatusPromptLiveness>,
    on_change: Option<StatusPromptChange>,
    on_change_failed: Option<StatusPromptChangeWarning>,
    /// Whether a failing `on_change` has already been reported for this prompt.
    warned: bool,
}

/// Holds the status line's state — persistent items and the FIFO prompt queue.
///
/// Prompt lifetimes mirror the extension dialogs: one on screen at a time,
/// later requests queue in call order, a reload cancels pending prompts except
/// host requests explicitly retained across content changes, and shutdown
/// settles everything and refuses later requests so no consumer is left
/// awaiting. Settled prompts are returned to the caller rather than delivered
/// through callbacks, so the surface processes them outside the store.
#[derive(Default)]
pub struct StatusLineStore {
    items: Vec<StatusItem>,
    pending: Vec<PendingPrompt>,
    closed: bool,
    next_id: u64,
}

impl std::fmt::Debug for StatusLineStore {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("StatusLineStore")
            .field("items", &self.items)
            .field("pending", &self.pending.len())
            .field("closed", &self.closed)
            .field("next_id", &self.next_id)
            .finish()
    }
}

impl StatusLineStore {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            items: Vec::new(),
            pending: Vec::new(),
            closed: false,
            next_id: 1,
        }
    }

    /// Read the current row state; items keep set order, prompt is the queue head.
    #[must_use]
    pub fn snapshot(&self) -> StatusLineSnapshot<'_> {
        StatusLineSnapshot {
            items: &self.items,
            prompt: self.pending.first().map(|entry| &entry.request),
        }
    }

    /// Set or replace one item, keeping its slot when it already exists.
    pub fn set_item(&mut self, item: StatusItem) {
        match self
            .items
            .iter()
            .position(|existing| existing.id == item.id)
        {
            Some(index) => self.items[index] = item,
            None => self.items.push(item),
        }
    }

    /// Clear one item and forget its slot.
    pub fn clear_item(&mut self, id: &str) {
        self.items.retain(|item| item.id != id);
    }

    /// Remove every item whose id the predicate accepts.
    pub fn clear_items(&mut self, predicate: impl Fn(&str) -> bool) {
        self.items.retain(|item| !predicate(&item.id));
    }

    /// Queue one prompt; `None` means the request was refused outright (store
    /// shut down or owner dead), which settles its cancel value immediately.
    pub fn open_prompt(
        &mut self,
        options: StatusPromptOptions,
        request_options: StatusPromptRequestOptions,
        owner: StatusPromptOwner,
    ) -> Option<u64> {
        let live = request_options
            .is_live
            .as_ref()
            .is_none_or(|is_live| is_live());
        if self.closed || !live {
            return None;
        }
        let request = StatusPromptRequest {
            id: self.next_id,
            prefix: sanitize_terminal_line(&options.prefix),
            placeholder: sanitize_terminal_line(&options.placeholder),
            value: sanitize_terminal_line(&options.initial),
            attribution: request_options
                .attribution
                .as_deref()
                .filter(|attribution| !attribution.is_empty())
                .map(sanitize_terminal_line),
            survive_reload: request_options.survive_reload,
            wants_change: request_options.wants_change,
            review_generation: request_options.review_generation,
        };
        self.next_id = self.next_id.saturating_add(1);
        let id = request.id;
        self.pending.push(PendingPrompt {
            request,
            owner,
            is_live: request_options.is_live,
            on_change: options.on_change,
            on_change_failed: request_options.on_change_failed,
            warned: false,
        });
        Some(id)
    }

    #[must_use]
    pub fn current_prompt_id(&self) -> Option<u64> {
        self.pending.first().map(|entry| entry.request.id)
    }

    /// Who owns the current prompt, so settlement and edit delivery reach it.
    #[must_use]
    pub fn current_prompt_owner(&self) -> Option<StatusPromptOwner> {
        self.pending.first().map(|entry| entry.owner.clone())
    }

    /// Whether the current prompt's owner asked for per-edit delivery.
    #[must_use]
    pub fn current_prompt_wants_change(&self) -> bool {
        self.pending
            .first()
            .is_some_and(|entry| entry.request.wants_change)
    }

    /// Replace the current prompt's text as the user types; ignored for a
    /// non-current id. Runs the prompt's `on_change`, reporting a failure once.
    pub fn update_prompt_value(&mut self, id: u64, value: impl Into<String>) -> bool {
        let Some(active) = self.pending.first_mut() else {
            return false;
        };
        if active.request.id != id {
            return false;
        }
        active.request.value = value.into();
        let value = active.request.value.clone();
        let Some(on_change) = active.on_change.as_ref() else {
            return true;
        };
        if let Err(detail) = on_change(value.as_str()) {
            Self::warn_once(active, &detail);
        }
        true
    }

    /// Report one failed per-edit delivery from a native owner, once per prompt.
    pub fn report_prompt_change_failure(&mut self, id: u64, detail: &str) {
        if let Some(active) = self
            .pending
            .first_mut()
            .filter(|entry| entry.request.id == id)
        {
            Self::warn_once(active, detail);
        }
    }

    /// Resolve the current prompt with its live value if its owner still holds
    /// authority; a dead owner settles with `None`. Ignored for a non-current id.
    pub fn submit_prompt(&mut self, id: u64) -> Option<StatusPromptSettlement> {
        let live = self
            .pending
            .first()
            .is_some_and(|entry| entry.is_live.as_ref().is_none_or(|is_live| is_live()));
        let answer = live.then(|| {
            self.pending
                .first()
                .map(|entry| entry.request.value.clone())
        });
        self.settle_current(id, answer.flatten())
    }

    /// Resolve the current prompt with `None`; ignored for a non-current id.
    pub fn cancel_prompt(&mut self, id: u64) -> Option<StatusPromptSettlement> {
        self.settle_current(id, None)
    }

    /// Cancel reload-scoped prompts while retaining opted-in host inputs and
    /// their queue order.
    pub fn cancel_reload_prompts(&mut self) -> Vec<StatusPromptSettlement> {
        let mut cancelled = Vec::new();
        let mut retained = Vec::new();
        for entry in self.pending.drain(..) {
            if entry.request.survive_reload {
                retained.push(entry);
            } else {
                cancelled.push(settle_none(entry));
            }
        }
        self.pending = retained;
        cancelled
    }

    /// Cancel the visible prompt and everything queued, keeping the store open.
    pub fn cancel_all_prompts(&mut self) -> Vec<StatusPromptSettlement> {
        self.pending.drain(..).map(settle_none).collect()
    }

    /// Cancel everything and refuse further prompts.
    pub fn shutdown(&mut self) -> Vec<StatusPromptSettlement> {
        self.closed = true;
        self.cancel_all_prompts()
    }

    fn settle_current(
        &mut self,
        id: u64,
        answer: Option<String>,
    ) -> Option<StatusPromptSettlement> {
        if self.pending.first()?.request.id != id {
            return None;
        }
        let entry = self.pending.remove(0);
        Some(StatusPromptSettlement {
            request: entry.request,
            owner: entry.owner,
            answer,
        })
    }

    fn warn_once(entry: &mut PendingPrompt, detail: &str) {
        if entry.warned {
            return;
        }
        entry.warned = true;
        if let Some(on_change_failed) = entry.on_change_failed.as_ref() {
            on_change_failed(detail);
        }
    }
}

fn settle_none(entry: PendingPrompt) -> StatusPromptSettlement {
    StatusPromptSettlement {
        request: entry.request,
        owner: entry.owner,
        answer: None,
    }
}

// ---------------------------------------------------------------------------
// Layout
// ---------------------------------------------------------------------------

/// Cells of horizontal padding on each side of the row.
pub const STATUS_LINE_PADDING: usize = 1;
/// Cells separating two adjacent items, or an item from the badge.
const ITEM_GAP: usize = 2;
const BADGE_GAP: usize = 1;
/// Reserve this many input cells whenever the row leaves enough room.
const MIN_PROMPT_INPUT_WIDTH: usize = 4;
const ELLIPSIS: &str = "…";

/// What a prompt needs from the layout: its painted lead-in and input width.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatusPromptLayoutInput<'a> {
    pub prefix: &'a str,
    pub attribution: Option<&'a str>,
}

/// One item after fitting: sanitized spans, possibly truncated, plus its width.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlacedStatusItem {
    pub id: String,
    pub spans: Vec<StatusSpan>,
    pub width: usize,
}

/// The keyboard-mode badge after fitting; never dropped, capped at half the row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlacedStatusBadge {
    pub text: String,
    pub width: usize,
}

/// The prompt's painted lead-in and how wide the input may be.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlacedStatusPrompt {
    pub prefix: String,
    pub attribution: Option<String>,
    pub input_width: usize,
}

/// Everything the row shows after fitting into one terminal row.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StatusLineLayout {
    pub left: Vec<PlacedStatusItem>,
    pub right: Vec<PlacedStatusItem>,
    pub prompt: Option<PlacedStatusPrompt>,
    pub badge: Option<PlacedStatusBadge>,
}

/// Report whether the row has anything to show, so the host can drop it when idle.
#[must_use]
pub fn status_line_has_content(snapshot: StatusLineSnapshot<'_>, badge: Option<&str>) -> bool {
    snapshot.prompt.is_some()
        || badge.is_some_and(|badge| !badge.is_empty())
        || snapshot
            .items
            .iter()
            .any(|item| item.spans.iter().any(|span| !span.text.is_empty()))
}

struct ItemCandidate {
    item: PlacedStatusItem,
    right: bool,
    priority: i32,
    order: usize,
}

/// Sanitize one item's spans and measure them, dropping spans that end up empty.
fn place_item(item: &StatusItem) -> Option<PlacedStatusItem> {
    let mut spans = Vec::new();
    let mut width = 0;
    for span in &item.spans {
        let text = sanitize_terminal_line(&span.text);
        if text.is_empty() {
            continue;
        }
        width += measure_text_width(&text);
        spans.push(StatusSpan {
            text,
            tone: span.tone,
            attributes: span.attributes.clone(),
        });
    }
    if spans.is_empty() {
        return None;
    }
    Some(PlacedStatusItem {
        id: item.id.clone(),
        spans,
        width,
    })
}

/// Clip one placed item to `width` cells, ending it with an ellipsis.
fn truncate_item(item: PlacedStatusItem, width: usize) -> Option<PlacedStatusItem> {
    if width == 0 {
        return None;
    }
    if item.width <= width {
        return Some(item);
    }
    let ellipsis_width = measure_text_width(ELLIPSIS);
    let mut remaining = width.saturating_sub(ellipsis_width);
    let mut spans = Vec::new();
    for span in &item.spans {
        let span_width = measure_text_width(&span.text);
        if span_width <= remaining {
            spans.push(span.clone());
            remaining -= span_width;
            continue;
        }
        let sliced = slice_text_by_width(&span.text, 0, remaining);
        let consumed = sliced.width;
        spans.push(StatusSpan {
            text: format!("{}{ELLIPSIS}", sliced.text),
            tone: span.tone,
            attributes: span.attributes.clone(),
        });
        return Some(PlacedStatusItem {
            id: item.id,
            spans,
            width: width - (remaining - consumed),
        });
    }
    // Every span fit under the reduced budget, so the ellipsis lands on its own.
    spans.push(StatusSpan {
        text: ELLIPSIS.into(),
        tone: None,
        attributes: Vec::new(),
    });
    Some(PlacedStatusItem {
        id: item.id,
        spans,
        width: width - remaining,
    })
}

/// Width of a run of items with the standard gap between neighbors.
fn items_width<'a>(items: impl IntoIterator<Item = &'a PlacedStatusItem>) -> usize {
    let mut total = 0;
    let mut count = 0;
    for item in items {
        total += item.width;
        count += 1;
    }
    if count == 0 {
        return 0;
    }
    total + ITEM_GAP * (count - 1)
}

/// Width the left and right runs occupy together, including the separating gap.
fn candidates_width(candidates: &[ItemCandidate]) -> usize {
    let left: Vec<&PlacedStatusItem> = candidates
        .iter()
        .filter(|entry| !entry.right)
        .map(|entry| &entry.item)
        .collect();
    let right: Vec<&PlacedStatusItem> = candidates
        .iter()
        .filter(|entry| entry.right)
        .map(|entry| &entry.item)
        .collect();
    let gap = usize::from(!left.is_empty() && !right.is_empty()) * ITEM_GAP;
    items_width(left.iter().copied()) + gap + items_width(right.iter().copied())
}

/// Drop and truncate items until they fit `available` cells.
///
/// Candidates keep their display order; the drop order is ascending priority
/// across both alignments, newest first among equal priorities, so the item a
/// consumer set most recently is the first casualty of its tier. The last
/// survivor is truncated unless `truncate` is off.
fn fit_items(
    candidates: Vec<ItemCandidate>,
    available: usize,
    truncate: bool,
) -> (Vec<PlacedStatusItem>, Vec<PlacedStatusItem>) {
    let mut surviving = candidates;
    while surviving.len() > 1 && candidates_width(&surviving) > available {
        let mut victim = 0;
        for index in 1..surviving.len() {
            let candidate = &surviving[index];
            let current = &surviving[victim];
            if candidate.priority < current.priority
                || (candidate.priority == current.priority && candidate.order > current.order)
            {
                victim = index;
            }
        }
        surviving.remove(victim);
    }
    if let [only] = &surviving[..]
        && only.item.width > available
    {
        let truncated = truncate
            .then(|| truncate_item(only.item.clone(), available))
            .flatten();
        return match truncated {
            Some(item) => split_by_alignment(vec![ItemCandidate {
                item,
                right: only.right,
                priority: only.priority,
                order: only.order,
            }]),
            None => (Vec::new(), Vec::new()),
        };
    }
    split_by_alignment(surviving)
}

fn split_by_alignment(
    candidates: Vec<ItemCandidate>,
) -> (Vec<PlacedStatusItem>, Vec<PlacedStatusItem>) {
    let mut left = Vec::new();
    let mut right = Vec::new();
    for entry in candidates {
        if entry.right {
            right.push(entry.item);
        } else {
            left.push(entry.item);
        }
    }
    (left, right)
}

/// Fit every status contribution into one row of `width` cells.
///
/// Deterministic and theme-free: the badge is never dropped, a prompt takes the
/// whole left region while it is open, and overflow drops the lowest-priority
/// item whole before the last survivor is truncated. Prompt lead-ins truncate
/// before consuming the input's minimum visible space.
#[must_use]
pub fn layout_status_line(input: StatusLineLayoutInput<'_>) -> StatusLineLayout {
    let row_width = input.width.saturating_sub(STATUS_LINE_PADDING * 2);

    let badge = input.badge.map(sanitize_terminal_line).and_then(|text| {
        (!text.is_empty()).then(|| PlacedStatusBadge {
            width: (measure_text_width(&text) + 2).min((input.width / 2).max(6)),
            text,
        })
    });
    let available =
        row_width.saturating_sub(badge.as_ref().map_or(0, |badge| badge.width + BADGE_GAP));

    let mut candidates = Vec::new();
    for (order, item) in input.items.iter().enumerate() {
        if let Some(placed) = place_item(item) {
            candidates.push(ItemCandidate {
                item: placed,
                right: item.alignment == ExtensionStatusAlignment::Right,
                priority: item.priority,
                order,
            });
        }
    }

    if let Some(prompt) = input.prompt {
        let mut attribution = prompt
            .attribution
            .map(sanitize_terminal_line)
            .filter(|attribution| !attribution.is_empty());
        let mut prefix = sanitize_terminal_line(prompt.prefix);
        let mut lead_width = attribution
            .as_deref()
            .map_or(0, |attribution| measure_text_width(attribution) + 1)
            + usize::from(!prefix.is_empty()) * (measure_text_width(&prefix) + 1);
        let lead_budget = available.saturating_sub(MIN_PROMPT_INPUT_WIDTH);
        if lead_width > lead_budget {
            // Treat attribution and prefix as one lead-in, retaining the
            // third-party marker first. Its trailing space and ellipsis also
            // consume cells; omit it if neither can fit.
            let lead = [attribution.as_deref(), Some(prefix.as_str())]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" ");
            let truncated = if lead_budget >= 2 {
                format!(
                    "{}{ELLIPSIS}",
                    slice_text_by_width(&lead, 0, lead_budget - 2).text
                )
            } else {
                String::new()
            };
            lead_width = usize::from(!truncated.is_empty()) * (measure_text_width(&truncated) + 1);
            if attribution.is_some() {
                attribution = (!truncated.is_empty()).then_some(truncated);
                prefix = String::new();
            } else {
                prefix = truncated;
            }
        }
        // Right items keep their place only whole, and only while the input
        // keeps its minimum: a clipped status fragment beside a prompt reads as
        // noise rather than information.
        let right_budget = available
            .saturating_sub(lead_width)
            .saturating_sub(MIN_PROMPT_INPUT_WIDTH)
            .saturating_sub(ITEM_GAP);
        let right = if right_budget > 0 {
            fit_items(
                candidates.into_iter().filter(|entry| entry.right).collect(),
                right_budget,
                false,
            )
            .1
        } else {
            Vec::new()
        };
        let right_width = usize::from(!right.is_empty()) * (items_width(right.iter()) + ITEM_GAP);
        let input_width = available.saturating_sub(lead_width + right_width);
        return StatusLineLayout {
            left: Vec::new(),
            prompt: Some(PlacedStatusPrompt {
                prefix,
                attribution,
                input_width,
            }),
            right,
            badge,
        };
    }

    let (left, right) = fit_items(candidates, available, true);
    StatusLineLayout {
        left,
        right,
        prompt: None,
        badge,
    }
}

/// Inputs to [`layout_status_line`].
#[derive(Debug, Clone, Copy)]
pub struct StatusLineLayoutInput<'a> {
    pub items: &'a [StatusItem],
    pub prompt: Option<StatusPromptLayoutInput<'a>>,
    /// The keyboard-mode badge text, or `None` when no mode is active.
    pub badge: Option<&'a str>,
    /// Full terminal width the row spans.
    pub width: usize,
}

// ---------------------------------------------------------------------------
// Symbolic paint mapping
// ---------------------------------------------------------------------------

/// Resolve a generic tone against the active theme; unknown or absent tones
/// paint as body text. Layout and measurement never touch a theme — this is
/// the one mapping from a semantic color to the active palette.
#[must_use]
pub fn symbolic_tone_color(tone: Option<ExtensionStatusTone>, theme: &AppTheme) -> Color {
    let color = match tone {
        Some(ExtensionStatusTone::Muted) | None => &theme.muted,
        Some(ExtensionStatusTone::Accent) => &theme.accent,
        Some(ExtensionStatusTone::AccentMuted) => &theme.accent_muted,
        Some(ExtensionStatusTone::Syntax) => &theme.syntax_colors.default,
        Some(ExtensionStatusTone::Added) => &theme.file_new,
        Some(ExtensionStatusTone::Removed) => &theme.file_deleted,
    };
    ratatui_theme_color(color)
}

/// Combine generic emphasis attributes into the terminal modifier bitmask.
#[must_use]
pub fn symbolic_text_attributes(attributes: &[ExtensionStatusAttribute]) -> Modifier {
    let mut modifier = Modifier::empty();
    for attribute in attributes {
        match attribute {
            ExtensionStatusAttribute::Bold => modifier |= Modifier::BOLD,
            ExtensionStatusAttribute::Italic => modifier |= Modifier::ITALIC,
            ExtensionStatusAttribute::Underline => modifier |= Modifier::UNDERLINED,
            ExtensionStatusAttribute::Strikethrough => modifier |= Modifier::CROSSED_OUT,
        }
    }
    modifier
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    fn item(id: &str, text: &str) -> StatusItem {
        StatusItem::new(id, text)
    }

    fn item_with(
        id: &str,
        text: &str,
        alignment: ExtensionStatusAlignment,
        priority: i32,
    ) -> StatusItem {
        StatusItem {
            alignment,
            priority,
            ..item(id, text)
        }
    }

    fn placed_text(placed: &[PlacedStatusItem]) -> Vec<String> {
        placed
            .iter()
            .map(|entry| {
                entry
                    .spans
                    .iter()
                    .map(|span| span.text.as_str())
                    .collect::<String>()
            })
            .collect()
    }

    fn open(store: &mut StatusLineStore, prefix: &str) -> u64 {
        store
            .open_prompt(
                StatusPromptOptions {
                    prefix: prefix.into(),
                    ..Default::default()
                },
                StatusPromptRequestOptions::default(),
                StatusPromptOwner::Host,
            )
            .expect("store is open")
    }

    // Translated from Hunk statusLine/store.test.ts (515188ea, MIT, Modem
    // Labs Inc.; see THIRD_PARTY_NOTICES).
    #[test]
    fn set_adds_an_item_in_set_order_and_replaces_it_in_place() {
        let mut store = StatusLineStore::new();
        store.set_item(item("a", "one"));
        store.set_item(item("b", "two"));
        store.set_item(item("a", "uno"));
        assert_eq!(
            store
                .snapshot()
                .items
                .iter()
                .map(|entry| entry.spans[0].text.clone())
                .collect::<Vec<_>>(),
            ["uno", "two"]
        );
    }

    #[test]
    fn clear_removes_an_item_and_forgets_its_slot() {
        let mut store = StatusLineStore::new();
        store.set_item(item("a", "one"));
        store.set_item(item("b", "two"));
        store.clear_item("a");
        store.set_item(item("a", "again"));
        assert_eq!(
            store
                .snapshot()
                .items
                .iter()
                .map(|entry| entry.id.clone())
                .collect::<Vec<_>>(),
            ["b", "a"]
        );
    }

    #[test]
    fn clear_items_removes_every_item_the_predicate_matches() {
        let mut store = StatusLineStore::new();
        store.set_item(item("ext:a", "one"));
        store.set_item(item("host:b", "two"));
        store.set_item(item("ext:c", "three"));
        store.clear_items(|id| id.starts_with("ext:"));
        assert_eq!(
            store
                .snapshot()
                .items
                .iter()
                .map(|entry| entry.id.clone())
                .collect::<Vec<_>>(),
            ["host:b"]
        );
    }

    #[test]
    fn requesting_a_prompt_makes_it_current_with_its_normalized_options() {
        let mut store = StatusLineStore::new();
        open(&mut store, "/");
        store.update_prompt_value(1, "foo");
        let prompt = store.snapshot().prompt.unwrap();
        // Prefix, placeholder, initial, and attribution all sanitize to one line.
        assert_eq!(prompt.id, 1);
        assert_eq!(prompt.prefix, "/");
        let mut attributed = StatusLineStore::new();
        attributed.open_prompt(
            StatusPromptOptions {
                prefix: "/\u{1b}[31m".into(),
                placeholder: "p\u{7}".into(),
                initial: "x\u{1b}[0m".into(),
                on_change: None,
            },
            StatusPromptRequestOptions {
                attribution: Some("ext search".into()),
                ..Default::default()
            },
            StatusPromptOwner::Host,
        );
        let prompt = attributed.snapshot().prompt.unwrap();
        assert_eq!(prompt.prefix, "/");
        assert_eq!(prompt.placeholder, "p");
        assert_eq!(prompt.value, "x");
        assert_eq!(prompt.attribution.as_deref(), Some("ext search"));
    }

    #[test]
    fn submit_resolves_the_live_value_and_promotes_the_next_queued_prompt() {
        let mut store = StatusLineStore::new();
        let first = open(&mut store, "/");
        let second = open(&mut store, ":");
        assert_eq!(store.snapshot().prompt.unwrap().prefix, "/");

        store.update_prompt_value(first, "needle");
        let settled = store.submit_prompt(first).unwrap();
        assert_eq!(settled.answer.as_deref(), Some("needle"));
        assert_eq!(store.snapshot().prompt.unwrap().prefix, ":");

        let cancelled = store.cancel_prompt(second).unwrap();
        assert_eq!(cancelled.answer, None);
        assert!(store.snapshot().prompt.is_none());
    }

    #[test]
    fn update_prompt_value_reports_every_edit_through_on_change() {
        let mut store = StatusLineStore::new();
        let seen: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        store.open_prompt(
            StatusPromptOptions {
                on_change: Some(Box::new({
                    let seen = Arc::clone(&seen);
                    move |value: &str| {
                        seen.lock().unwrap().push(value.to_owned());
                        Ok(())
                    }
                })),
                ..Default::default()
            },
            StatusPromptRequestOptions::default(),
            StatusPromptOwner::Host,
        );
        store.update_prompt_value(1, "a");
        store.update_prompt_value(1, "ab");
        assert_eq!(*seen.lock().unwrap(), ["a", "ab"]);
        assert_eq!(store.snapshot().prompt.unwrap().value, "ab");
    }

    #[test]
    fn a_failing_on_change_warns_once_and_the_prompt_continues() {
        let mut store = StatusLineStore::new();
        let warnings: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        store.open_prompt(
            StatusPromptOptions {
                on_change: Some(Box::new(|_: &str| Err("boom".to_owned()))),
                ..Default::default()
            },
            StatusPromptRequestOptions {
                on_change_failed: Some(Box::new({
                    let warnings = Arc::clone(&warnings);
                    move |detail: &str| warnings.lock().unwrap().push(detail.to_owned())
                })),
                ..Default::default()
            },
            StatusPromptOwner::Host,
        );
        store.update_prompt_value(1, "a");
        store.update_prompt_value(1, "ab");
        assert_eq!(*warnings.lock().unwrap(), ["boom"]);
        assert_eq!(store.snapshot().prompt.unwrap().value, "ab");
    }

    #[test]
    fn answers_for_a_prompt_that_is_not_current_are_ignored() {
        let mut store = StatusLineStore::new();
        let first = open(&mut store, "/");
        let _second = open(&mut store, ":");
        assert!(store.submit_prompt(2).is_none());
        assert!(!store.update_prompt_value(2, "late"));
        assert_eq!(store.snapshot().prompt.unwrap().id, first);
        assert_eq!(
            store.submit_prompt(first).unwrap().answer,
            Some(String::new())
        );
    }

    #[test]
    fn cancel_all_prompts_settles_open_and_queued_prompts_and_keeps_the_store_open() {
        let mut store = StatusLineStore::new();
        let first = open(&mut store, "/");
        let _second = open(&mut store, ":");
        let settled = store.cancel_all_prompts();
        assert_eq!(settled.len(), 2);
        assert!(settled.iter().all(|entry| entry.answer.is_none()));
        assert!(store.snapshot().prompt.is_none());
        assert_eq!(open(&mut store, "again"), first + 2);
        assert_eq!(store.snapshot().prompt.unwrap().prefix, "again");
    }

    #[test]
    fn reload_preserves_opted_in_host_prompts_and_cancels_other_open_and_queued_prompts() {
        let mut store = StatusLineStore::new();
        let extension = open(&mut store, "");
        let host = store
            .open_prompt(
                StatusPromptOptions {
                    prefix: "filter:".into(),
                    ..Default::default()
                },
                StatusPromptRequestOptions {
                    survive_reload: true,
                    ..Default::default()
                },
                StatusPromptOwner::Host,
            )
            .unwrap();
        let queued = open(&mut store, "");
        let cancelled = store.cancel_reload_prompts();
        assert_eq!(cancelled.len(), 2);
        assert!(cancelled.iter().all(|entry| entry.answer.is_none()));
        assert_eq!(
            cancelled
                .iter()
                .map(|entry| entry.request.id)
                .collect::<Vec<_>>(),
            [extension, queued]
        );
        assert_eq!(store.snapshot().prompt.unwrap().id, host);
        store.update_prompt_value(host, "after");
        let before = store.snapshot().prompt.unwrap().clone();
        assert!(store.cancel_reload_prompts().is_empty());
        assert_eq!(store.snapshot().prompt.unwrap().clone(), before);
        let settled = store.shutdown();
        assert_eq!(settled.len(), 1);
        assert_eq!(settled[0].answer, None);
    }

    #[test]
    fn shutdown_settles_pending_prompts_and_refuses_later_ones_immediately() {
        let mut store = StatusLineStore::new();
        open(&mut store, "/");
        assert_eq!(store.shutdown().len(), 1);
        assert!(
            store
                .open_prompt(
                    StatusPromptOptions::default(),
                    StatusPromptRequestOptions::default(),
                    StatusPromptOwner::Host,
                )
                .is_none()
        );
        assert!(store.snapshot().prompt.is_none());
    }

    #[test]
    fn a_prompt_whose_owner_is_no_longer_live_resolves_null_without_appearing() {
        let mut store = StatusLineStore::new();
        assert!(
            store
                .open_prompt(
                    StatusPromptOptions::default(),
                    StatusPromptRequestOptions {
                        is_live: Some(Box::new(|| false)),
                        ..Default::default()
                    },
                    StatusPromptOwner::Host,
                )
                .is_none()
        );
        assert!(store.snapshot().prompt.is_none());
    }

    #[test]
    fn submitting_a_prompt_whose_owner_expired_while_open_resolves_null() {
        let mut store = StatusLineStore::new();
        let live = Arc::new(Mutex::new(true));
        store.open_prompt(
            StatusPromptOptions::default(),
            StatusPromptRequestOptions {
                is_live: Some(Box::new({
                    let live = Arc::clone(&live);
                    move || *live.lock().unwrap()
                })),
                ..Default::default()
            },
            StatusPromptOwner::Host,
        );
        store.update_prompt_value(1, "typed");
        *live.lock().unwrap() = false;
        assert_eq!(store.submit_prompt(1).unwrap().answer, None);
    }

    // Translated from Hunk statusLine/layout.test.ts (515188ea, MIT, Modem
    // Labs Inc.; see THIRD_PARTY_NOTICES).
    fn layout(
        items: &[StatusItem],
        prompt: Option<StatusPromptLayoutInput<'_>>,
        badge: Option<&str>,
        width: usize,
    ) -> StatusLineLayout {
        layout_status_line(StatusLineLayoutInput {
            items,
            prompt,
            badge,
            width,
        })
    }

    #[test]
    fn places_left_items_in_set_order_and_right_items_beside_the_badge() {
        let items = [
            item("a", "filter=foo"),
            item_with("b", "3 viewed", ExtensionStatusAlignment::Right, 0),
            item("c", "note"),
        ];
        let result = layout(&items, None, Some("Search — Esc exits"), 80);
        assert_eq!(placed_text(&result.left), ["filter=foo", "note"]);
        assert_eq!(placed_text(&result.right), ["3 viewed"]);
        assert_eq!(
            result.badge,
            Some(PlacedStatusBadge {
                text: "Search — Esc exits".into(),
                width: 20,
            })
        );
        assert!(result.prompt.is_none());
    }

    #[test]
    fn an_item_with_no_spans_contributes_nothing() {
        let empty = StatusItem {
            id: "a".into(),
            spans: Vec::new(),
            alignment: ExtensionStatusAlignment::Left,
            priority: 0,
        };
        let result = layout(&[empty, item("b", "shown")], None, None, 80);
        assert_eq!(placed_text(&result.left), ["shown"]);
    }

    #[test]
    fn a_prompt_takes_the_left_region_and_keeps_right_items_and_the_badge() {
        let items = [
            item("a", "filter=foo"),
            item_with("b", "3 viewed", ExtensionStatusAlignment::Right, 0),
        ];
        let result = layout(
            &items,
            Some(StatusPromptLayoutInput {
                prefix: "/",
                attribution: None,
            }),
            Some("Mode"),
            40,
        );
        assert!(result.left.is_empty());
        assert_eq!(placed_text(&result.right), ["3 viewed"]);
        // 40 − 2 padding − badge (6) − 1 gap − right (8) − 2 gap − prefix (1) − 1 space = 19.
        assert_eq!(
            result.prompt,
            Some(PlacedStatusPrompt {
                prefix: "/".into(),
                attribution: None,
                input_width: 19,
            })
        );
    }

    #[test]
    fn a_prompt_paints_its_attribution_before_the_prefix() {
        let result = layout(
            &[],
            Some(StatusPromptLayoutInput {
                prefix: "/",
                attribution: Some("ext search"),
            }),
            None,
            30,
        );
        assert_eq!(
            result.prompt,
            Some(PlacedStatusPrompt {
                prefix: "/".into(),
                attribution: Some("ext search".into()),
                input_width: 15,
            })
        );
    }

    #[test]
    fn a_narrow_prompt_drops_right_items_before_starving_the_input() {
        let items = [StatusItem {
            id: "b".into(),
            spans: vec![StatusSpan {
                text: "some right-aligned status".into(),
                tone: None,
                attributes: Vec::new(),
            }],
            alignment: ExtensionStatusAlignment::Right,
            priority: 0,
        }];
        let result = layout(
            &items,
            Some(StatusPromptLayoutInput {
                prefix: "filter:",
                attribution: None,
            }),
            None,
            20,
        );
        assert!(result.right.is_empty());
        // 20 − 2 padding − prefix (7) − 1 space = 10.
        assert_eq!(result.prompt.as_ref().unwrap().input_width, 10);
    }

    #[test]
    fn truncates_a_long_prompt_lead_in_before_starving_the_input_beside_a_badge() {
        for prompt in [
            StatusPromptLayoutInput {
                prefix: "a very long prompt prefix:",
                attribution: None,
            },
            StatusPromptLayoutInput {
                prefix: "検索:",
                attribution: Some("ext 非常に長い拡張機能"),
            },
        ] {
            let result = layout(&[], Some(prompt), Some("Mode"), 20);
            let placed = result.prompt.as_ref().unwrap();
            let lead = [placed.attribution.as_deref(), Some(placed.prefix.as_str())]
                .into_iter()
                .flatten()
                .filter(|part| !part.is_empty())
                .collect::<Vec<_>>()
                .join(" ");
            assert!(lead.ends_with('…'), "lead {lead}");
            assert!(placed.input_width >= MIN_PROMPT_INPUT_WIDTH);
            assert_eq!(
                result.badge,
                Some(PlacedStatusBadge {
                    text: "Mode".into(),
                    width: 6,
                })
            );
            let badge_width = result.badge.as_ref().unwrap().width;
            assert!(
                STATUS_LINE_PADDING * 2
                    + measure_text_width(&lead)
                    + 1
                    + placed.input_width
                    + 1
                    + badge_width
                    <= 20
            );
            // Deterministic: the same input yields the same placement.
            assert_eq!(
                layout(&[], Some(prompt), Some("Mode"), 20),
                layout(&[], Some(prompt), Some("Mode"), 20)
            );
        }
    }

    #[test]
    fn an_impossibly_narrow_row_uses_only_the_input_cells_actually_available() {
        let result = layout(
            &[],
            Some(StatusPromptLayoutInput {
                prefix: "filter:",
                attribution: None,
            }),
            Some("Mode"),
            10,
        );
        assert_eq!(
            result.prompt,
            Some(PlacedStatusPrompt {
                prefix: String::new(),
                attribution: None,
                input_width: 1,
            })
        );
        assert_eq!(
            result.badge,
            Some(PlacedStatusBadge {
                text: "Mode".into(),
                width: 6,
            })
        );
    }

    #[test]
    fn a_short_row_truncates_the_prefix_to_reserve_four_input_cells() {
        let result = layout(
            &[],
            Some(StatusPromptLayoutInput {
                prefix: "filter:",
                attribution: None,
            }),
            None,
            8,
        );
        assert_eq!(
            result.prompt,
            Some(PlacedStatusPrompt {
                prefix: "…".into(),
                attribution: None,
                input_width: 4,
            })
        );
    }

    #[test]
    fn overflow_drops_the_lowest_priority_item_whole_newest_first_among_equals() {
        let items = [
            item_with("keep", "important", ExtensionStatusAlignment::Left, 2),
            item_with("first", "aaaaaaaaaa", ExtensionStatusAlignment::Left, 0),
            item_with("second", "bbbbbbbbbb", ExtensionStatusAlignment::Left, 0),
        ];
        let result = layout(&items, None, None, 2 + 9 + 2 + 10 + 1);
        assert_eq!(placed_text(&result.left), ["important", "aaaaaaaaaa"]);
    }

    #[test]
    fn the_last_surviving_item_is_truncated_with_an_ellipsis() {
        let items = [
            item_with("a", "abcdefghij", ExtensionStatusAlignment::Left, 0),
            item_with("b", "klmnopqrst", ExtensionStatusAlignment::Left, 1),
        ];
        let result = layout(&items, None, None, 2 + 6);
        assert_eq!(placed_text(&result.left), ["klmno…"]);
        assert_eq!(result.left[0].width, 6);
    }

    #[test]
    fn truncation_cuts_across_spans_and_keeps_their_tones() {
        let items = [StatusItem {
            id: "a".into(),
            spans: vec![
                StatusSpan {
                    text: "[2/7] ".into(),
                    tone: Some(ExtensionStatusTone::Accent),
                    attributes: Vec::new(),
                },
                StatusSpan {
                    text: "src/file.ts".into(),
                    tone: Some(ExtensionStatusTone::Muted),
                    attributes: Vec::new(),
                },
            ],
            alignment: ExtensionStatusAlignment::Left,
            priority: 0,
        }];
        let result = layout(&items, None, None, 2 + 10);
        assert_eq!(
            result.left[0].spans,
            vec![
                StatusSpan {
                    text: "[2/7] ".into(),
                    tone: Some(ExtensionStatusTone::Accent),
                    attributes: Vec::new(),
                },
                StatusSpan {
                    text: "src…".into(),
                    tone: Some(ExtensionStatusTone::Muted),
                    attributes: Vec::new(),
                },
            ]
        );
    }

    #[test]
    fn overflow_drops_by_priority_across_both_alignments() {
        let items = [
            item_with("left", "left status", ExtensionStatusAlignment::Left, 1),
            StatusItem {
                id: "hint".into(),
                spans: vec![StatusSpan {
                    text: "a long right-aligned hint".into(),
                    tone: None,
                    attributes: Vec::new(),
                }],
                alignment: ExtensionStatusAlignment::Right,
                priority: 0,
            },
        ];
        let result = layout(&items, None, None, 2 + 11 + 2 + 5);
        assert_eq!(placed_text(&result.left), ["left status"]);
        assert!(result.right.is_empty());
    }

    #[test]
    fn the_badge_is_never_dropped_and_is_capped_at_half_the_row() {
        let result = layout(&[item("a", "status text")], None, Some(&"x".repeat(60)), 20);
        let badge = result.badge.unwrap();
        assert_eq!(badge.width, 10);
        assert_eq!(placed_text(&result.left), ["status…"]);
    }

    #[test]
    fn a_wide_row_keeps_every_item_without_truncation() {
        let items = [
            item("a", "left one"),
            item("b", "left two"),
            item_with("c", "right", ExtensionStatusAlignment::Right, 0),
        ];
        let result = layout(&items, None, Some("Mode"), 200);
        assert_eq!(placed_text(&result.left), ["left one", "left two"]);
        assert_eq!(
            result
                .left
                .iter()
                .map(|entry| entry.width)
                .collect::<Vec<_>>(),
            [8, 8]
        );
        assert_eq!(placed_text(&result.right), ["right"]);
    }

    #[test]
    fn control_characters_in_item_text_are_sanitized_before_measurement() {
        let mut hostile = item("a", "bad");
        hostile.spans[0].text = "bad\u{1b}[31mtext".into();
        let result = layout(&[hostile], None, None, 80);
        assert!(!result.left[0].spans[0].text.contains('\u{1b}'));
        assert_eq!(result.left[0].width, result.left[0].spans[0].text.len());
    }

    // Translated from Hunk StatusLine.test.tsx's statusLineHasContent cases
    // (515188ea, MIT, Modem Labs Inc.; see THIRD_PARTY_NOTICES).
    #[test]
    fn status_line_has_content_ignores_items_with_empty_spans() {
        let mut store = StatusLineStore::new();
        store.set_item(StatusItem {
            id: "a".into(),
            spans: Vec::new(),
            alignment: ExtensionStatusAlignment::Left,
            priority: 0,
        });
        assert!(!status_line_has_content(store.snapshot(), None));
        store.set_item(item("a", ""));
        assert!(!status_line_has_content(store.snapshot(), None));
        store.set_item(item("a", "x"));
        assert!(status_line_has_content(store.snapshot(), None));
        store.clear_item("a");
        assert!(!status_line_has_content(store.snapshot(), None));
        assert!(status_line_has_content(store.snapshot(), Some("Mode")));
        open(&mut store, "/");
        assert!(status_line_has_content(store.snapshot(), None));
    }

    #[test]
    fn symbolic_tones_and_attributes_map_onto_the_active_theme() {
        let theme = crate::theme::resolve_theme(None, None, &Vec::new());
        let all = [
            None,
            Some(ExtensionStatusTone::Muted),
            Some(ExtensionStatusTone::Accent),
            Some(ExtensionStatusTone::AccentMuted),
            Some(ExtensionStatusTone::Syntax),
            Some(ExtensionStatusTone::Added),
            Some(ExtensionStatusTone::Removed),
        ];
        let mut colors: Vec<_> = all
            .iter()
            .map(|tone| symbolic_tone_color(*tone, &theme))
            .collect();
        colors.dedup();
        assert!(colors.len() > 1, "tones must resolve to distinct colors");
        assert_eq!(
            symbolic_text_attributes(&[
                ExtensionStatusAttribute::Bold,
                ExtensionStatusAttribute::Underline
            ]),
            Modifier::BOLD | Modifier::UNDERLINED
        );
        assert_eq!(symbolic_text_attributes(&[]), Modifier::empty());
    }
}
