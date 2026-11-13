//! The navigator's plan block, one account after the other.
//!
//! The block of `navigator/plan.rs` shows the windows the last Claude Code
//! session reported, whatever account it ran on: with one subscription that is
//! the whole story, with several it is one account's figures under no name,
//! and they change each time another account's pane reports. The figures of
//! every account are already kept (`usage.rs`), so with two accounts or more
//! the block names each one, with its own windows and its next
//! reset. With a single account it stays as it was.
//!
//! A window whose reset has passed since it was reported counts as empty: the
//! plan started that window over, and the figure on file is from before.

use chrono::DateTime;
use iced::widget::{Space, column, container, row, text, tooltip};
use iced::{Alignment, Element, Length, Padding};

use hangar_i18n::{Key, Translator};
use hangar_theme::tokens::{font_size, space};
use hangar_theme::{fonts, style};

use crate::ctx::Ctx;
use crate::message::Message;
use crate::navigator::plan::{meter, meter_pct, reset_text, rule, window_label};
use crate::panes_logic::wall_clock_ms;

use super::live;
use super::model::ClaudeAccount;
use super::switch::{AccountUsage, WindowUsage};

/// A reset as epoch milliseconds, when the figure says one.
fn reset_ms(window: &WindowUsage) -> Option<i64> {
    let at = window.resets_at.as_deref()?;
    DateTime::parse_from_rfc3339(at)
        .ok()
        .map(|parsed| parsed.timestamp_millis())
}

/// The percent a window's meter shows at `now`: none once its reset has
/// passed.
pub fn shown_pct(window: &WindowUsage, now: i64) -> u8 {
    match reset_ms(window) {
        Some(reset) if reset <= now => 0,
        _ => meter_pct(window.percent_used),
    }
}

/// The account's soonest reset still to come.
pub fn next_reset(usage: &AccountUsage, now: i64) -> Option<i64> {
    [usage.five_hour.as_ref(), usage.weekly.as_ref()]
        .into_iter()
        .flatten()
        .filter_map(reset_ms)
        .filter(|at| *at > now)
        .min()
}

/// The accounts the block lists: every one with figures, and the logged-in
/// ones without, which say so. `None` when the per-account block has nothing
/// to add to the plain one: a single account, or no figures at all yet.
pub fn shown<'a>(
    accounts: &'a [ClaudeAccount],
    usage: &'a super::switch::Usage,
) -> Option<Vec<(&'a ClaudeAccount, Option<&'a AccountUsage>)>> {
    if accounts.len() < 2 || usage.is_empty() {
        return None;
    }
    let listed: Vec<_> = accounts
        .iter()
        .map(|account| (account, usage.get(&account.id)))
        .filter(|(account, figures)| figures.is_some() || account.logged_in)
        .collect();
    (!listed.is_empty()).then_some(listed)
}

/// The block, or `None` when the plain one should show instead.
pub fn view<'a>(ctx: Ctx<'a>) -> Option<Element<'a, Message>> {
    let accounts = live(ctx).accounts.as_deref()?;
    let listed = shown(accounts, &live(ctx).usage)?;
    let t = ctx.theme;
    let tr: Translator = ctx.i18n;
    let locale = tr.locale();
    let now = i64::try_from(wall_clock_ms()).unwrap_or_default();

    let mut lines = column![
        text(tr.t(Key::SidebarPlan).into_owned())
            .size(font_size::CAPTION)
            .font(fonts::ui_semibold())
            .color(t.colors.fg_muted)
    ]
    .spacing(space::XS);

    for (account, figures) in listed {
        let mut head = row![
            container(
                text(account.label.clone())
                    .size(font_size::MICRO)
                    .wrapping(text::Wrapping::None)
                    .color(t.colors.fg_secondary),
            )
            .max_width(140.0)
            .clip(true),
            Space::new().width(Length::Fill),
        ]
        .spacing(space::MD)
        .align_y(Alignment::Center);
        if let Some(next) = figures.and_then(|figures| next_reset(figures, now)) {
            head = head.push(
                text(reset_text(tr, next, now))
                    .size(font_size::CAPTION)
                    .color(t.colors.fg_subtle)
                    .wrapping(text::Wrapping::None),
            );
        }
        let mut entry = column![head].spacing(0);
        let windows = [
            (
                "five_hour",
                figures.and_then(|figures| figures.five_hour.as_ref()),
            ),
            (
                "seven_day",
                figures.and_then(|figures| figures.weekly.as_ref()),
            ),
        ];
        let mut any = false;
        for (kind, window) in windows {
            let Some(window) = window else { continue };
            any = true;
            entry = entry.push(meter(
                t,
                locale,
                window_label(tr, kind),
                shown_pct(window, now),
            ));
        }
        if !any {
            entry = entry.push(
                text(tr.t(Key::SidebarPlanNoFigures).into_owned())
                    .size(font_size::CAPTION)
                    .color(t.colors.fg_subtle),
            );
        }
        lines = lines.push(entry);
    }

    let block = container(lines).width(Length::Fill).padding(Padding {
        top: 9.0,
        right: 16.0,
        bottom: 10.0,
        left: 16.0,
    });
    Some(
        column![
            rule(t),
            tooltip(
                block,
                container(
                    text(tr.t(Key::SidebarPlanAccountsHint).into_owned())
                        .size(font_size::CAPTION_LG)
                )
                .padding([space::XS - 1.0, space::MD - 1.0])
                .style(style::container::tooltip(t)),
                tooltip::Position::Top,
            )
        ]
        .into(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account(id: &str, logged_in: bool) -> ClaudeAccount {
        ClaudeAccount {
            id: id.into(),
            label: id.into(),
            is_main: id == "main",
            logged_in,
            email: None,
            plan: None,
        }
    }

    fn window(percent_used: f64, resets_at: Option<&str>) -> Option<WindowUsage> {
        Some(WindowUsage {
            percent_used,
            resets_at: resets_at.map(str::to_string),
            at: 1,
        })
    }

    const RESET: &str = "2026-11-06T18:00:00Z";

    fn reset() -> i64 {
        DateTime::parse_from_rfc3339(RESET)
            .unwrap()
            .timestamp_millis()
    }

    #[test]
    fn a_single_account_or_no_figures_leaves_the_plain_block() {
        let usage = super::super::switch::Usage::from([(
            "main".to_string(),
            AccountUsage {
                five_hour: window(10.0, None),
                weekly: None,
            },
        )]);
        assert!(shown(&[account("main", true)], &usage).is_none());
        let both = [account("main", true), account("perso", true)];
        assert!(shown(&both, &super::super::switch::Usage::new()).is_none());
        assert!(shown(&both, &usage).is_some());
    }

    #[test]
    fn an_account_with_figures_or_logged_in_is_listed_and_the_others_are_not() {
        let usage = super::super::switch::Usage::from([
            ("gone".to_string(), AccountUsage::default()),
            (
                "pro".to_string(),
                AccountUsage {
                    five_hour: window(5.0, None),
                    weekly: None,
                },
            ),
        ]);
        let all = [
            account("main", true),
            account("pro", false),
            account("idle", false),
        ];
        let ids: Vec<_> = shown(&all, &usage)
            .expect("listed")
            .into_iter()
            .map(|(account, figures)| (account.id.as_str(), figures.is_some()))
            .collect();
        assert_eq!(ids, [("main", false), ("pro", true)]);
    }

    #[test]
    fn a_window_past_its_reset_is_shown_empty() {
        let window = window(87.0, Some(RESET)).unwrap();
        assert_eq!(shown_pct(&window, reset() - 1), 87);
        assert_eq!(shown_pct(&window, reset()), 0);
        let undated = WindowUsage {
            resets_at: None,
            ..window
        };
        assert_eq!(shown_pct(&undated, i64::MAX), 87);
    }

    #[test]
    fn the_next_reset_is_the_soonest_still_to_come() {
        let usage = AccountUsage {
            five_hour: window(1.0, Some("2026-11-06T12:00:00Z")),
            weekly: window(1.0, Some(RESET)),
        };
        let noon = DateTime::parse_from_rfc3339("2026-11-06T12:00:00Z")
            .unwrap()
            .timestamp_millis();
        assert_eq!(next_reset(&usage, noon - 1), Some(noon));
        assert_eq!(next_reset(&usage, noon), Some(reset()));
        assert_eq!(next_reset(&usage, reset()), None);
    }
}
