//! The response's Console tab (#194, design 1a of "Console & Workspace History"): a filter bar
//! (All, Network, Logs, Errors, a text filter and Clear) over the timeline of the last send. The
//! network entry opens into a tree or the raw HTTP text.
//!
//! Everything shown comes from [`ConsoleTimeline`], built once per response on the background
//! thread, and from [`crate::state::console::ConsoleView`], which caches the visible tree rows.
//! Nothing here parses or formats a body.

use std::sync::Arc;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::input::Input;
use gpui_kit::component::scroll::Scrollbar;
use gpui_kit::component::*;
use gpui_kit::prelude::*;
use gpui_kit::*;
use rust_i18n::t;

use postino_core::ConsoleLevel;

use crate::state::console::{
    ConsoleEntry, ConsoleFilter, ConsoleTimeline, EntrySource, NetworkDetail, TreeRow, ValueTone,
    format_clock, tree_as_json,
};
use crate::state::response_render::{format_duration, format_size};
use crate::theme::metrics::{INPUT_PADDING_X, RADIUS_MD, RADIUS_SM};
use crate::theme::{Palette, PaletteExt, PostinoColors};
use crate::views::components::{GhostButton, SegmentedControl, SegmentedItem, edit_menu};

use super::root::AppView;

/// Height of the filter bar.
const FILTER_BAR_HEIGHT: f32 = 40.0;
/// Height of a chip, the filter field and the buttons of the bars.
const BAR_CONTROL_HEIGHT: f32 = 24.0;
/// Width of the text filter.
const FILTER_FIELD_WIDTH: f32 = 220.0;
/// Height of a timeline row.
const ENTRY_HEIGHT: f32 = 30.0;
/// Horizontal padding of the bars and the rows.
const PADDING_X: f32 = 14.0;
/// Gap between the columns of a row.
const COLUMN_GAP: f32 = 12.0;
/// Width of the chevron or icon column.
const ICON_COLUMN_WIDTH: f32 = 12.0;
/// Width of the time column (`10:42:03.118`).
const TIME_COLUMN_WIDTH: f32 = 88.0;
/// Width of the source column (`post-response`, a method).
const SOURCE_COLUMN_WIDTH: f32 = 96.0;
/// Left padding of the network detail: lines it up with the time column.
const DETAIL_PADDING_LEFT: f32 = PADDING_X + ICON_COLUMN_WIDTH + COLUMN_GAP;
/// Height of a tree row.
const TREE_ROW_HEIGHT: f32 = 22.0;
/// Indentation of one tree level.
const TREE_INDENT: f32 = 16.0;
/// Text size of the timeline and the tree.
const MONO_TEXT_SIZE: f32 = 12.0;

impl AppView {
    /// Renders the Console tab of the response of `tab_id`.
    pub(crate) fn render_console_tab(
        &mut self,
        tab_id: &str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let palette = cx.palette();
        let generation = self.response_generations.get(tab_id);
        self.console_view.sync(tab_id, generation);
        let Some(timeline) = self.response_consoles.get(tab_id).cloned() else {
            return div().into_any_element();
        };
        let mono_font = cx.theme().mono_font_family.clone();

        let visible = self.console_view.visible(&timeline);
        let list = if timeline.entries.is_empty() || self.console_view.cleared {
            empty_message(t!("response.console.empty"), &palette)
        } else if visible.is_empty() {
            empty_message(t!("response.console.no_match"), &palette)
        } else {
            let mut list = v_flex()
                .font_family(mono_font)
                .text_size(px(MONO_TEXT_SIZE));
            for (index, entry) in visible.into_iter().enumerate() {
                list = list.child(match entry {
                    ConsoleEntry::Network { .. } => self.render_network_entry(entry, &timeline, cx),
                    _ => render_plain_entry(index, entry, &palette),
                });
            }
            list.into_any_element()
        };

        v_flex()
            .size_full()
            .child(self.render_console_filter_bar(&timeline, cx))
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .child(
                        div()
                            .id("console-list")
                            .size_full()
                            .overflow_y_scroll()
                            .track_scroll(&self.response_scroll)
                            .child(list),
                    )
                    .child(
                        div()
                            .absolute()
                            .inset_0()
                            .child(Scrollbar::new(&self.response_scroll)),
                    ),
            )
            .into_any_element()
    }

    /// The chips, the text filter and Clear.
    fn render_console_filter_bar(
        &self,
        timeline: &ConsoleTimeline,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let palette = cx.palette();
        let weak = cx.weak_entity();
        let current = self.console_view.filter;
        let chips: Vec<_> = ConsoleFilter::ALL
            .into_iter()
            .map(|filter| {
                let weak = weak.clone();
                let selected = filter == current;
                let count = (filter == ConsoleFilter::Errors && timeline.error_count > 0)
                    .then(|| timeline.error_count.to_string());
                h_flex()
                    .id(match filter {
                        ConsoleFilter::All => "console-filter-all",
                        ConsoleFilter::Network => "console-filter-network",
                        ConsoleFilter::Logs => "console-filter-logs",
                        ConsoleFilter::Errors => "console-filter-errors",
                    })
                    .h(px(BAR_CONTROL_HEIGHT))
                    .items_center()
                    .gap(px(6.0))
                    .px(px(10.0))
                    .rounded(px(RADIUS_SM))
                    .text_size(px(12.0))
                    .cursor_pointer()
                    .map(|chip| {
                        if selected {
                            chip.bg(palette.border).text_color(palette.fg)
                        } else {
                            chip.text_color(palette.fg_muted)
                                .hover(|style| style.bg(palette.hover))
                                .active(|style| style.bg(palette.pressed))
                        }
                    })
                    .child(filter_label(filter))
                    .when_some(count, |chip, count| {
                        chip.child(div().text_color(palette.danger).child(count))
                    })
                    .on_click(move |_, _, cx| {
                        let _ = weak.update(cx, |view, cx| {
                            view.console_view.filter = filter;
                            cx.notify();
                        });
                    })
            })
            .collect();

        let clear_weak = weak.clone();
        h_flex()
            .flex_none()
            .h(px(FILTER_BAR_HEIGHT))
            .items_center()
            .gap(px(6.0))
            .px(px(PADDING_X))
            .border_b_1()
            .border_color(palette.border)
            .children(chips)
            .child(div().flex_1())
            .child(
                h_flex()
                    .id("console-filter-field")
                    .w(px(FILTER_FIELD_WIDTH))
                    .h(px(BAR_CONTROL_HEIGHT + 2.0))
                    .items_center()
                    .gap_2()
                    .px(px(9.0))
                    .rounded(px(RADIUS_MD - 1.0))
                    .bg(palette.raised)
                    .border_1()
                    .border_color(palette.border)
                    .hover({
                        let border_strong = palette.border_strong;
                        move |style| style.border_color(border_strong)
                    })
                    .text_color(palette.fg_subtle)
                    .child(Icon::new(Lucide::Search).small())
                    .child(
                        Input::new(&self.console_filter_input)
                            .context_menu(edit_menu(&self.console_filter_input, cx))
                            .pl_0()
                            .pr(px(INPUT_PADDING_X))
                            .h(px(BAR_CONTROL_HEIGHT))
                            .py_0()
                            .text_size(px(12.0))
                            .bordered(false)
                            .text_color(palette.fg)
                            .bg(palette.bg.opacity(0.0)),
                    ),
            )
            .child(
                GhostButton::new("console-clear", t!("response.console.clear"))
                    .icon(Icon::new(Lucide::Trash))
                    .height(BAR_CONTROL_HEIGHT + 2.0)
                    .on_click(move |_, _, cx| {
                        let _ = clear_weak.update(cx, |view, cx| {
                            view.console_view.cleared = true;
                            cx.notify();
                        });
                    }),
            )
    }

    /// The network entry: a clickable summary row and, when open, its detail.
    fn render_network_entry(
        &mut self,
        entry: &ConsoleEntry,
        timeline: &Arc<ConsoleTimeline>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let ConsoleEntry::Network {
            time,
            method,
            url,
            status,
            duration,
            size,
        } = entry
        else {
            return div().into_any_element();
        };
        let palette = cx.palette();
        let weak = cx.weak_entity();
        let open = self.console_view.network_open;
        let status_color = palette.status_colors(*status).0;

        let summary = h_flex()
            .id("console-network")
            .h(px(ENTRY_HEIGHT))
            .items_center()
            .gap(px(COLUMN_GAP))
            .px(px(PADDING_X))
            .cursor_pointer()
            .hover(|style| style.bg(palette.hover))
            .child(
                div()
                    .w(px(ICON_COLUMN_WIDTH))
                    .text_color(palette.fg_muted)
                    .child(
                        Icon::new(if open {
                            Lucide::ChevronDown
                        } else {
                            Lucide::ChevronRight
                        })
                        .with_size(px(12.0)),
                    ),
            )
            .child(time_cell(*time, &palette))
            .child(
                div()
                    .w(px(SOURCE_COLUMN_WIDTH))
                    .flex_none()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(palette.method_color(method))
                    .child(method.to_string()),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_color(palette.fg)
                    .child(url.clone()),
            )
            .when_some(*status, |row, status| {
                row.child(div().text_color(status_color).child(status.to_string()))
            })
            .when_some(*duration, |row, duration| {
                row.child(
                    div()
                        .text_color(palette.fg_muted)
                        .child(format_duration(duration)),
                )
            })
            .when_some(*size, |row, size| {
                row.child(div().text_color(palette.fg_muted).child(format_size(size)))
            })
            .on_click(move |_, _, cx| {
                let _ = weak.update(cx, |view, cx| {
                    view.console_view.network_open = !view.console_view.network_open;
                    cx.notify();
                });
            });

        let detail = match (&timeline.network, open) {
            (Some(detail), true) => Some(self.render_network_detail(detail, cx)),
            _ => None,
        };
        v_flex()
            .border_b_1()
            .border_color(palette.border.opacity(0.5))
            .when(open, |entry| entry.bg(palette.surface))
            .child(summary)
            .children(detail)
            .into_any_element()
    }

    /// The Tree/Raw switch, Expand all, Collapse and Copy, over the tree or the raw text.
    fn render_network_detail(
        &mut self,
        detail: &NetworkDetail,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let palette = cx.palette();
        let weak = cx.weak_entity();
        let raw = self.console_view.raw;

        let switch = SegmentedControl::new("console-view-switch")
            .item(
                SegmentedItem::new(t!("response.console.tree"))
                    .selected(!raw)
                    .on_click({
                        let weak = weak.clone();
                        move |_, cx| {
                            let _ = weak.update(cx, |view, cx| {
                                view.console_view.raw = false;
                                cx.notify();
                            });
                        }
                    }),
            )
            .item(
                SegmentedItem::new(t!("response.body.raw"))
                    .selected(raw)
                    .on_click({
                        let weak = weak.clone();
                        move |_, cx| {
                            let _ = weak.update(cx, |view, cx| {
                                view.console_view.raw = true;
                                cx.notify();
                            });
                        }
                    }),
            );

        let expand_detail = detail.clone();
        let expand_weak = weak.clone();
        let collapse_weak = weak.clone();
        let copy_detail = detail.clone();
        let toolbar = h_flex()
            .items_center()
            .gap(px(6.0))
            .font_family(cx.theme().font_family.clone())
            .child(switch)
            .child(div().flex_1())
            .child(
                GhostButton::new("console-expand-all", t!("response.console.expand_all"))
                    .icon(Icon::new(Lucide::ChevronsUpDown))
                    .height(BAR_CONTROL_HEIGHT)
                    .on_click(move |_, _, cx| {
                        let _ = expand_weak.update(cx, |view, cx| {
                            view.console_view.expand_all(&expand_detail);
                            cx.notify();
                        });
                    }),
            )
            .child(
                GhostButton::new("console-collapse", t!("response.console.collapse"))
                    .icon(Icon::new(Lucide::ChevronsDownUp))
                    .height(BAR_CONTROL_HEIGHT)
                    .on_click(move |_, _, cx| {
                        let _ = collapse_weak.update(cx, |view, cx| {
                            view.console_view.collapse_all();
                            cx.notify();
                        });
                    }),
            )
            .child(
                GhostButton::new("console-copy", t!("common.copy"))
                    .icon(Icon::new(Lucide::Copy))
                    .height(BAR_CONTROL_HEIGHT)
                    .on_click(move |_, _, cx| {
                        let text = if raw {
                            raw_text(&copy_detail)
                        } else {
                            tree_as_json(&copy_detail.tree)
                        };
                        cx.write_to_clipboard(ClipboardItem::new_string(text));
                    }),
            );

        let body = if raw {
            render_raw(detail, &palette, cx)
        } else {
            let rows = self.console_view.rows(detail);
            let mut tree = v_flex();
            for (index, row) in rows.iter().enumerate() {
                tree = tree.child(render_tree_row(index, row, weak.clone(), &palette));
            }
            tree.into_any_element()
        };

        v_flex()
            .gap_2()
            .pt(px(4.0))
            .pb(px(PADDING_X))
            .pl(px(DETAIL_PADDING_LEFT))
            .pr(px(PADDING_X))
            .child(toolbar)
            .child(
                div()
                    .py_2()
                    .border_1()
                    .border_color(palette.border)
                    .rounded(px(RADIUS_MD))
                    .bg(palette.bg)
                    .child(body),
            )
            .into_any_element()
    }
}

/// The label of a filter chip.
fn filter_label(filter: ConsoleFilter) -> SharedString {
    match filter {
        ConsoleFilter::All => t!("response.console.filter_all"),
        ConsoleFilter::Network => t!("response.console.filter_network"),
        ConsoleFilter::Logs => t!("response.console.filter_logs"),
        ConsoleFilter::Errors => t!("response.console.filter_errors"),
    }
    .into_owned()
    .into()
}

/// The muted message shown instead of the list.
fn empty_message(text: impl Into<SharedString>, palette: &Palette) -> AnyElement {
    div()
        .px(px(PADDING_X))
        .py_2()
        .text_color(palette.fg_muted)
        .child(text.into())
        .into_any_element()
}

/// The time column.
fn time_cell(time: Option<std::time::SystemTime>, palette: &Palette) -> impl IntoElement {
    div()
        .w(px(TIME_COLUMN_WIDTH))
        .flex_none()
        .text_color(palette.fg_subtle)
        .children(time.map(format_clock))
}

/// The name a script goes by in the source and location columns. Kept in English, like the
/// `::: pre` and `::: post` markers they stand for.
fn source_name(source: EntrySource) -> &'static str {
    match source {
        EntrySource::Pre => "pre-request",
        EntrySource::Post => "post-response",
        EntrySource::Network => "network",
    }
}

/// A log line or an error row.
fn render_plain_entry(index: usize, entry: &ConsoleEntry, palette: &Palette) -> AnyElement {
    let (time, source, text, line, color, error) = match entry {
        ConsoleEntry::Log {
            time,
            source,
            level,
            text,
            line,
        } => {
            let color = match level {
                ConsoleLevel::Log | ConsoleLevel::Info => palette.fg,
                ConsoleLevel::Warn => palette.warning,
                ConsoleLevel::Error => palette.danger,
            };
            (
                *time,
                *source,
                text,
                *line,
                color,
                *level == ConsoleLevel::Error,
            )
        }
        ConsoleEntry::Error {
            time,
            source,
            message,
            line,
        } => (*time, *source, message, *line, palette.danger, true),
        ConsoleEntry::Network { .. } => return div().into_any_element(),
    };
    let is_failure = matches!(entry, ConsoleEntry::Error { .. });
    let location = line
        .filter(|_| source != EntrySource::Network)
        .map(|line| format!("{}.js:{line}", source_name(source)));

    h_flex()
        .id(("console-entry", index))
        .min_h(px(ENTRY_HEIGHT))
        .items_center()
        .gap(px(COLUMN_GAP))
        .px(px(PADDING_X))
        .py(px(4.0))
        .border_b_1()
        .border_color(palette.border.opacity(0.5))
        .when(is_failure, |row| row.bg(palette.danger_subtle))
        .child(
            div()
                .w(px(ICON_COLUMN_WIDTH))
                .flex_none()
                .text_color(palette.danger)
                .when(error, |cell| {
                    cell.child(Icon::new(Lucide::CircleX).with_size(px(12.0)))
                }),
        )
        .child(time_cell(time, palette))
        .child(
            div()
                .w(px(SOURCE_COLUMN_WIDTH))
                .flex_none()
                .text_color(palette.fg_muted)
                .child(source_name(source)),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_color(color)
                .child(text.clone()),
        )
        .children(location.map(|location| {
            div()
                .flex_none()
                .text_color(palette.fg_subtle)
                .child(location)
        }))
        .into_any_element()
}

/// One row of the network tree. A branch opens or closes on click.
fn render_tree_row(
    index: usize,
    row: &TreeRow,
    weak: WeakEntity<AppView>,
    palette: &Palette,
) -> AnyElement {
    let value_color = match row.tone {
        ValueTone::String => palette.syntax_color("string"),
        ValueTone::Number => palette.syntax_color("number"),
        ValueTone::Constant => palette.syntax_color("boolean"),
        ValueTone::Summary => palette.fg_muted,
    };
    let chevron = row.open.map(|open| {
        Icon::new(if open {
            Lucide::ChevronDown
        } else {
            Lucide::ChevronRight
        })
        .with_size(px(12.0))
    });
    let path = row.path.clone();
    let branch = row.open.is_some();
    h_flex()
        .id(("console-tree-row", index))
        .h(px(TREE_ROW_HEIGHT))
        .items_center()
        .gap(px(6.0))
        .pr(px(12.0))
        .pl(px(12.0 + row.depth as f32 * TREE_INDENT))
        .hover(|style| style.bg(palette.hover))
        .when(branch, |line| line.cursor_pointer())
        .child(
            div()
                .w(px(12.0))
                .flex_none()
                .text_color(palette.fg_subtle)
                .children(chevron),
        )
        .child(
            h_flex()
                .flex_none()
                .child(
                    div()
                        .text_color(palette.syntax_color("property"))
                        .child(row.key.clone()),
                )
                .child(div().text_color(palette.fg_subtle).child(":")),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_color(value_color)
                .child(row.text.clone()),
        )
        .when(branch, |line| {
            line.on_click(move |_, _, cx| {
                let _ = weak.update(cx, |view, cx| {
                    view.console_view.toggle(&path);
                    cx.notify();
                });
            })
        })
        .into_any_element()
}

/// The raw request and response, each under its label.
fn render_raw(detail: &NetworkDetail, palette: &Palette, cx: &App) -> AnyElement {
    let label = |text: SharedString| {
        div()
            .font_family(cx.theme().font_family.clone())
            .text_size(px(11.0))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(palette.fg_subtle)
            .child(text)
    };
    let block = |text: &Arc<str>| {
        div()
            .text_color(palette.fg)
            .line_height(relative(1.6))
            .child(SharedString::from(text.clone()))
    };
    v_flex()
        .gap_3()
        .px(px(PADDING_X))
        .py_1()
        .child(label(
            t!("response.console.raw_request").into_owned().into(),
        ))
        .child(block(&detail.raw_request))
        .when_some(detail.raw_response.as_ref(), |raw, response| {
            raw.child(label(
                t!("response.console.raw_response").into_owned().into(),
            ))
            .child(block(response))
        })
        .into_any_element()
}

/// The raw text of both sides, for Copy.
fn raw_text(detail: &NetworkDetail) -> String {
    match &detail.raw_response {
        Some(response) => format!("{}\n\n{response}", detail.raw_request),
        None => detail.raw_request.to_string(),
    }
}
