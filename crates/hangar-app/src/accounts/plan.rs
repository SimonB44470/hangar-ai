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
//! plan started that window over, and the figure on file is from before
//! (`navigator::plan::shown_pct`, which the plain block follows too).

use iced::widget::{Space, column, container, row, text, tooltip};
use iced::{Alignment, Element, Length, Padding};

use hangar_i18n::{Key, Translator};
use hangar_theme::tokens::{font_size, space};
use hangar_theme::{fonts, style};

use crate::ctx::Ctx;
use crate::message::Message;
use crate::navigator::plan::{meter, next_reset, reset_text, rule, shown_pct, window_label};
use crate::panes_logic::wall_clock_ms;
use crate::state::live::RateLimit;

use super::live;
use super::model::ClaudeAccount;
use super::switch::{AccountUsage, FIVE_HOUR, WEEKLY};

/// An account's windows as the plain block reads a session's: the plan's
/// two, then a gateway's, by kind. The same functions then draw them and
/// find their resets.
pub fn limits(usage: &AccountUsage) -> Vec<RateLimit> {
    let plan = [
        (FIVE_HOUR, usage.five_hour.as_ref()),
        (WEEKLY, usage.weekly.as_ref()),
    ];
    plan.into_iter()
        .filter_map(|(kind, window)| Some((kind, window?)))
        .chain(
            usage
                .others
                .iter()
                .map(|(kind, window)| (kind.as_str(), window)),
        )
        .map(|(kind, window)| RateLimit {
            kind: kind.to_owned(),
            percent_used: window.percent_used,
            resets_at: window.resets_at.clone(),
        })
        .collect()
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
        let windows = figures.map(limits).unwrap_or_default();
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
        if let Some(next) = next_reset(&windows, now) {
            head = head.push(
                text(reset_text(tr, next, now))
                    .size(font_size::CAPTION)
                    .color(t.colors.fg_subtle)
                    .wrapping(text::Wrapping::None),
            );
        }
        let mut entry = column![head].spacing(0);
        for limit in &windows {
            entry = entry.push(meter(
                t,
                locale,
                window_label(tr, &limit.kind),
                shown_pct(limit, now),
            ));
        }
        if windows.is_empty() {
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
    use super::super::switch::{Usage, WindowUsage};
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

    fn five_hours(percent_used: f64) -> AccountUsage {
        AccountUsage {
            five_hour: window(percent_used, None),
            ..AccountUsage::default()
        }
    }

    #[test]
    fn a_single_account_or_no_figures_leaves_the_plain_block() {
        let usage = Usage::from([("main".to_string(), five_hours(10.0))]);
        assert!(shown(&[account("main", true)], &usage).is_none());
        let both = [account("main", true), account("perso", true)];
        assert!(shown(&both, &Usage::new()).is_none());
        assert!(shown(&both, &usage).is_some());
    }

    #[test]
    fn an_account_with_figures_or_logged_in_is_listed_and_the_others_are_not() {
        let usage = Usage::from([
            ("gone".to_string(), AccountUsage::default()),
            ("pro".to_string(), five_hours(5.0)),
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

    /// A window whose reset passed since it was reported shows empty, in the
    /// per-account block as in the plain one (`shown_pct`).
    #[test]
    fn a_window_past_its_reset_is_shown_empty() {
        let reset = chrono::DateTime::parse_from_rfc3339("2026-11-06T18:00:00Z")
            .expect("a date")
            .timestamp_millis();
        let usage = AccountUsage {
            weekly: window(87.0, Some("2026-11-06T18:00:00Z")),
            others: [(
                "monthly".to_string(),
                WindowUsage {
                    percent_used: 87.0,
                    resets_at: None,
                    at: 1,
                },
            )]
            .into(),
            ..AccountUsage::default()
        };
        let windows = limits(&usage);
        assert_eq!(shown_pct(&windows[0], reset - 1), 87);
        assert_eq!(shown_pct(&windows[0], reset), 0);
        // Without a reset, the figure stands.
        assert_eq!(shown_pct(&windows[1], i64::MAX), 87);
    }

    /// An account's windows read as a session's report: the plan's two in
    /// their order, then a gateway's, which the plain block showed too; the
    /// soonest reset is found among all of them.
    #[test]
    fn an_accounts_windows_are_the_plans_two_then_a_gateways() {
        let usage = AccountUsage {
            five_hour: window(10.0, Some("2026-11-06T12:00:00Z")),
            weekly: window(20.0, None),
            others: [(
                "monthly".to_string(),
                window(30.0, Some("2026-11-06T09:00:00Z")).expect("a window"),
            )]
            .into(),
        };
        let windows = limits(&usage);
        let kinds: Vec<_> = windows
            .iter()
            .map(|limit| (limit.kind.as_str(), limit.percent_used))
            .collect();
        assert_eq!(
            kinds,
            [("five_hour", 10.0), ("seven_day", 20.0), ("monthly", 30.0)]
        );
        let at = |iso: &str| {
            chrono::DateTime::parse_from_rfc3339(iso)
                .expect("a date")
                .timestamp_millis()
        };
        let nine = at("2026-11-06T09:00:00Z");
        assert_eq!(next_reset(&windows, nine - 1), Some(nine));
        assert_eq!(next_reset(&windows, nine), Some(at("2026-11-06T12:00:00Z")));
        // Nothing reported: no window, no reset.
        assert!(limits(&AccountUsage::default()).is_empty());
        assert_eq!(limits(&five_hours(5.0)).len(), 1);
    }
}
