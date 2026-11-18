//! The accounts' views drawn and driven headless, and the update logic that
//! needs no service (the store's own actions are P0b's, so what is checked of
//! a switch is the restart it asks for).

// A state is built by assigning the one field a case is about: a struct
// literal with `..Default::default()` would bury it.
#![allow(clippy::field_reassign_with_default)]

use hangar_i18n::Locale;
use hangar_theme::tokens::{font_size, space};
use hangar_ui::badge::icon_side;
use iced::widget::container;
use iced::{Alignment, Element, Length};
use serde_json::json;

use crate::message::Message;
use crate::shell::MenuEntry;
use crate::state::{AccountStyle, AccountTone};
use crate::workspace_dialog::testing::{Harness, messages};

use super::account_style::{self, Mark};
use super::settings_page::Page;
use super::switch::{AccountUsage, WindowUsage};
use super::switcher::{Menu, SwitchScope};
use super::*;

fn ws(id: &str) -> WsId {
    id.to_string()
}

fn accounts_msg(messages: &[Message], wanted: impl Fn(&Msg) -> bool) -> bool {
    messages.iter().any(|message| match message {
        Message::Accounts(msg) => wanted(msg),
        _ => false,
    })
}

/// The toolbar's trail: the switcher at the right end of a bar.
fn toolbar<'a>(element: Element<'a, Message>) -> Element<'a, Message> {
    container(element)
        .width(Length::Fill)
        .align_x(Alignment::End)
        .into()
}

fn window(percent_used: f64) -> Option<WindowUsage> {
    Some(WindowUsage {
        percent_used,
        resets_at: None,
        at: 1,
    })
}

// ── the switcher, drawn ───────────────────────────────────────────────

#[test]
fn the_button_names_the_account_of_the_workspace_on_screen() {
    let h = Harness::new(Locale::En, "graphite");
    let st = State::default();
    let mut ui = h.show(toolbar(switcher(&st, h.ctx())));
    assert!(ui.find("Principal").is_ok());
    h.shot(&mut ui, "accounts-switcher-closed-en-graphite");
    ui.click("Principal").expect("the button is drawn");
    assert!(accounts_msg(&messages(ui), |msg| matches!(
        msg,
        Msg::ToggleMenu
    )));
}

#[test]
fn a_workspace_on_its_own_account_shows_that_account() {
    let h = Harness::new(Locale::En, "graphite").edit(|state| {
        state.active_workspace_id = Some("w2".into());
    });
    let st = State::default();
    let mut ui = h.show(toolbar(switcher(&st, h.ctx())));
    assert!(ui.find("Perso").is_ok());
}

#[test]
fn nothing_is_drawn_before_the_accounts_are_known() {
    let mut h = Harness::new(Locale::En, "graphite");
    h.model.live.accounts.accounts = None;
    let st = State::default();
    let mut ui = h.show(toolbar(switcher(&st, h.ctx())));
    assert!(ui.find("Principal").is_err());
}

#[test]
fn the_list_has_a_row_per_account_with_its_figures_and_both_scopes() {
    let mut h = Harness::new(Locale::En, "graphite");
    h.model.live.accounts.usage = std::collections::BTreeMap::from([(
        "main".to_string(),
        AccountUsage {
            five_hour: window(40.0),
            weekly: window(55.4),
            ..AccountUsage::default()
        },
    )]);
    let mut st = State::default();
    st.switcher = Menu::Open;
    let mut ui = h.show(toolbar(switcher(&st, h.ctx())));
    // « 5 h » is bound by the typesetter, so the lines are built from the
    // dictionary's own words.
    let t = hangar_i18n::Translator::new(Locale::En);
    let (five, week) = (
        t.t(hangar_i18n::Key::AccountsSwitcherFive),
        t.t(hangar_i18n::Key::AccountsSwitcherWeek),
    );
    let reported = format!("{five} 40% · {week} 55%");
    for text in [
        "This workspace uses",
        "Perso",
        "Travail",
        "not logged in",
        "For every workspace",
        // Figures of the account that reported, one word for the one that
        // has none yet.
        reported.as_str(),
        "no figures yet",
    ] {
        assert!(ui.find(text).is_ok(), "{text} is in the menu");
    }
    h.shot(&mut ui, "accounts-switcher-open-en-graphite");
    ui.click("Perso").expect("the row is drawn");
    assert!(accounts_msg(&messages(ui), |msg| matches!(
        msg,
        Msg::Choose { ws, id, scope: SwitchScope::Workspace } if ws == "w1" && id == "perso"
    )));
}

#[test]
fn the_second_part_of_a_row_makes_the_account_the_default_of_every_workspace() {
    let h = Harness::new(Locale::En, "graphite");
    let mut st = State::default();
    st.switcher = Menu::Open;
    let mut ui = h.show(toolbar(switcher(&st, h.ctx())));
    ui.click("For every workspace")
        .expect("the button is drawn");
    // The first row is the main account.
    assert!(accounts_msg(&messages(ui), |msg| matches!(
        msg,
        Msg::Choose { id, scope: SwitchScope::Default, .. } if id == "main"
    )));
}

#[test]
fn with_agents_running_the_click_asks_before_it_restarts_them() {
    let mut h = Harness::new(Locale::En, "graphite");
    h.model.live.accounts.launched.note("p1", Some("main"));
    let mut st = State::default();
    st.switcher = Menu::Ask {
        ws: ws("w1"),
        id: "perso".into(),
        scope: SwitchScope::Workspace,
    };
    let mut ui = h.show(toolbar(switcher(&st, h.ctx())));
    for text in [
        "Switch this workspace to Perso",
        "1 agent running.",
        "Restart this workspace\u{2019}s agents now (conversations kept)",
        "On next launch",
        "Back",
    ] {
        assert!(ui.find(text).is_ok(), "{text} is in the question");
    }
    h.shot(&mut ui, "accounts-switcher-ask-en-graphite");
    ui.click("On next launch").expect("the button is drawn");
    assert!(accounts_msg(&messages(ui), |msg| matches!(
        msg,
        Msg::Decide(false)
    )));

    let mut ui = h.show(toolbar(switcher(&st, h.ctx())));
    ui.click("Restart this workspace\u{2019}s agents now (conversations kept)")
        .expect("the button is drawn");
    assert!(accounts_msg(&messages(ui), |msg| matches!(
        msg,
        Msg::Decide(true)
    )));

    let mut ui = h.show(toolbar(switcher(&st, h.ctx())));
    ui.click("Back").expect("the button is drawn");
    assert!(accounts_msg(&messages(ui), |msg| matches!(msg, Msg::Back)));
}

#[test]
fn the_question_for_the_default_counts_every_agent_that_follows_it() {
    let mut h = Harness::new(Locale::Fr, "github-light");
    h.model.live.accounts.launched.note("p1", Some("main"));
    // p3 is in a workspace with an account of its own: it does not follow.
    h.model.live.accounts.launched.note("p3", Some("perso"));
    let mut st = State::default();
    st.switcher = Menu::Ask {
        ws: ws("w1"),
        id: "perso".into(),
        scope: SwitchScope::Default,
    };
    let mut ui = h.show(toolbar(switcher(&st, h.ctx())));
    assert!(ui.find("1 agent en cours.").is_ok());
    h.shot(&mut ui, "accounts-switcher-ask-fr-github-light");
}

// ── the switcher, driven ──────────────────────────────────────────────

fn choose(scope: SwitchScope, id: &str) -> Msg {
    Msg::Choose {
        ws: ws("w1"),
        id: id.into(),
        scope,
    }
}

#[test]
fn the_button_opens_and_closes_the_popover() {
    let mut h = Harness::new(Locale::En, "graphite");
    let mut st = State::default();
    let _ = switcher_step(&mut st, &mut h.model, Msg::ToggleMenu);
    assert_eq!(st.switcher, Menu::Open);
    let _ = switcher_step(&mut st, &mut h.model, Msg::ToggleMenu);
    assert_eq!(st.switcher, Menu::Closed);
    let _ = switcher_step(&mut st, &mut h.model, Msg::ToggleMenu);
    let _ = switcher_step(&mut st, &mut h.model, Msg::CloseMenu);
    assert_eq!(st.switcher, Menu::Closed);
}

#[test]
fn with_nothing_running_a_click_switches_at_once_and_closes() {
    let mut h = Harness::new(Locale::En, "graphite");
    let mut st = State::default();
    st.switcher = Menu::Open;
    let task = switcher_step(
        &mut st,
        &mut h.model,
        choose(SwitchScope::Workspace, "perso"),
    );
    assert_eq!(st.switcher, Menu::Closed);
    // Nothing to restart.
    assert_eq!(task.units(), 0);
}

/// The person who asked to be asked (`account_switch_ask`).
fn asking() -> Harness {
    Harness::new(Locale::En, "graphite").edit(|state| state.settings.account_switch_ask = true)
}

#[test]
fn with_agents_running_a_click_asks_and_the_answer_decides_the_restart() {
    let mut h = asking();
    h.model.live.accounts.launched.note("p1", Some("main"));
    let mut st = State::default();
    st.switcher = Menu::Open;
    let task = switcher_step(
        &mut st,
        &mut h.model,
        choose(SwitchScope::Workspace, "perso"),
    );
    assert_eq!(task.units(), 0);
    assert_eq!(
        st.switcher,
        Menu::Ask {
            ws: ws("w1"),
            id: "perso".into(),
            scope: SwitchScope::Workspace
        }
    );
    // Back to the list, then ask again.
    let _ = switcher_step(&mut st, &mut h.model, Msg::Back);
    assert_eq!(st.switcher, Menu::Open);
    let _ = switcher_step(
        &mut st,
        &mut h.model,
        choose(SwitchScope::Workspace, "perso"),
    );
    // On the next launch: switched, nothing restarted.
    let later = switcher_step(&mut st, &mut h.model, Msg::Decide(false));
    assert_eq!(later.units(), 0);
    assert_eq!(st.switcher, Menu::Closed);
    // Now: the running agent restarts.
    let _ = switcher_step(
        &mut st,
        &mut h.model,
        choose(SwitchScope::Workspace, "perso"),
    );
    let now = switcher_step(&mut st, &mut h.model, Msg::Decide(true));
    assert_eq!(now.units(), 1);
    // An answer with no question asked is nothing.
    assert_eq!(
        switcher_step(&mut st, &mut h.model, Msg::Decide(true)).units(),
        0
    );
}

#[test]
fn a_switch_restarts_only_the_running_claude_panes_that_follow() {
    let mut h = Harness::new(Locale::En, "graphite");
    // p1 (Claude, follows) runs, p2 is a shell: only p1.
    h.model.live.accounts.launched.note("p1", Some("main"));
    assert_eq!(
        set_workspace_account(&mut h.model, &ws("w1"), Some("perso".into()), true).units(),
        1
    );
    assert_eq!(
        set_workspace_account(&mut h.model, &ws("w1"), Some("perso".into()), false).units(),
        0
    );
    // Nothing runs in w2 (p3 was never launched).
    assert_eq!(
        set_workspace_account(&mut h.model, &ws("w2"), None, true).units(),
        0
    );
    // A workspace that is gone.
    assert_eq!(
        set_workspace_account(&mut h.model, &ws("gone"), None, true).units(),
        0
    );
}

#[test]
fn a_pane_with_an_account_of_its_own_is_not_restarted_by_its_workspace() {
    let mut h = Harness::new(Locale::En, "graphite").edit(|state| {
        state.workspaces[0].panes[0].claude_account = Some("pro".into());
    });
    h.model.live.accounts.launched.note("p1", Some("pro"));
    assert_eq!(
        set_workspace_account(&mut h.model, &ws("w1"), Some("perso".into()), true).units(),
        0
    );
}

#[test]
fn a_new_default_restarts_the_agents_of_the_workspaces_that_follow_it() {
    let mut h = Harness::new(Locale::En, "graphite");
    h.model.live.accounts.launched.note("p1", Some("main"));
    // w2 has an account of its own: its agent keeps running.
    h.model.live.accounts.launched.note("p3", Some("perso"));
    assert_eq!(
        set_default_account(&mut h.model, "pro".into(), true).units(),
        1
    );
    assert_eq!(
        set_default_account(&mut h.model, "pro".into(), false).units(),
        0
    );
}

#[test]
fn a_pane_pick_restarts_the_agent_only_when_it_runs() {
    let mut h = asking();
    assert_eq!(
        set_pane_account(
            &mut h.model,
            &ws("w1"),
            &"p1".to_string(),
            Some("pro".into())
        )
        .units(),
        0
    );
    h.model.live.accounts.launched.note("p1", Some("main"));
    assert_eq!(
        set_pane_account(
            &mut h.model,
            &ws("w1"),
            &"p1".to_string(),
            Some("pro".into())
        )
        .units(),
        1
    );
}

#[test]
fn a_pane_opened_to_log_in_runs_no_agent_so_nothing_restarts() {
    let mut h = Harness::new(Locale::En, "graphite").edit(|state| {
        state.workspaces[0].panes[0].login = Some(true);
    });
    h.model.live.accounts.launched.note("p1", Some("pro"));
    assert_eq!(
        set_workspace_account(&mut h.model, &ws("w1"), Some("perso".into()), true).units(),
        0
    );
}

// ── the usage from the bridge ─────────────────────────────────────────

fn usage_event(percent: f64) -> serde_json::Value {
    json!({
        "kind": "usage",
        "context": { "tokens": 10, "window": 200, "percent": 5.0 },
        "rateLimits": [
            { "kind": "five_hour", "percentUsed": percent, "resetsAt": "soon" },
            { "kind": "seven_day", "percentUsed": 12.0, "resetsAt": null }
        ],
        "costUsd": 0.1
    })
}

#[test]
fn a_usage_report_counts_for_the_account_the_pane_was_launched_on() {
    let mut h = Harness::new(Locale::En, "graphite");
    h.model.live.accounts.launched.note("p1", Some("perso"));
    assert!(record_usage(
        &mut h.model,
        &"p1".to_string(),
        &usage_event(40.0),
        100
    ));
    let usage = &h.model.live.accounts.usage;
    assert_eq!(
        usage["perso"].five_hour.as_ref().map(|w| w.percent_used),
        Some(40.0)
    );
    assert_eq!(
        usage["perso"].weekly.as_ref().map(|w| w.percent_used),
        Some(12.0)
    );
    assert_eq!(usage["perso"].five_hour.as_ref().map(|w| w.at), Some(100));
    // A later report replaces it; an earlier one does not.
    assert!(record_usage(
        &mut h.model,
        &"p1".to_string(),
        &usage_event(60.0),
        200
    ));
    assert!(record_usage(
        &mut h.model,
        &"p1".to_string(),
        &usage_event(10.0),
        50
    ));
    let usage = &h.model.live.accounts.usage;
    assert_eq!(
        usage["perso"].five_hour.as_ref().map(|w| w.percent_used),
        Some(60.0)
    );
}

#[test]
fn a_usage_report_of_a_pane_with_no_account_or_without_limits_is_nothing() {
    let mut h = Harness::new(Locale::En, "graphite");
    // Not launched on any account.
    assert!(!record_usage(
        &mut h.model,
        &"p1".to_string(),
        &usage_event(40.0),
        1
    ));
    h.model.live.accounts.launched.note("p1", Some("main"));
    let no_limits = json!({
        "kind": "usage",
        "context": { "tokens": 1, "window": 2, "percent": null },
        "rateLimits": [],
        "costUsd": null
    });
    assert!(!record_usage(
        &mut h.model,
        &"p1".to_string(),
        &no_limits,
        1
    ));
    // Another kind of report.
    let turn = json!({ "kind": "turn", "phase": "start" });
    assert!(!record_usage(&mut h.model, &"p1".to_string(), &turn, 1));
    assert!(h.model.live.accounts.usage.is_empty());
}

// ── the badges and the menu entries ───────────────────────────────────

#[test]
fn a_workspace_on_an_account_other_than_the_default_wears_its_label() {
    let h = Harness::new(Locale::En, "graphite");
    let own = ws("w2");
    let mut ui = h.show(badge(h.ctx(), &own));
    assert!(ui.find("Perso").is_ok());
    // On the default: nothing.
    let default = ws("w1");
    let mut ui = h.show(badge(h.ctx(), &default));
    assert!(ui.find("Perso").is_err() && ui.find("Principal").is_err());
}

#[test]
fn a_workspace_that_names_the_default_wears_nothing() {
    let h = Harness::new(Locale::En, "graphite").edit(|state| {
        state.workspaces[1].claude_account = Some("main".into());
    });
    let w = ws("w2");
    let mut ui = h.show(badge(h.ctx(), &w));
    assert!(ui.find("Principal").is_err());
}

#[test]
fn no_badge_with_a_single_account() {
    let mut h = Harness::new(Locale::En, "graphite");
    h.model.live.accounts.accounts = Some(vec![crate::workspace_dialog::testing::account(
        "main",
        "Principal",
        true,
    )]);
    let w = ws("w2");
    let mut ui = h.show(badge(h.ctx(), &w));
    assert!(ui.find("Principal").is_err());
}

#[test]
fn a_pane_on_another_account_than_its_workspace_wears_its_label() {
    let h = Harness::new(Locale::En, "graphite").edit(|state| {
        state.workspaces[1].panes[0].claude_account = Some("pro".into());
    });
    let (w, p) = (ws("w2"), "p3".to_string());
    let mut ui = h.show(pane_badge(h.ctx(), &w, &p));
    assert!(ui.find("Travail").is_ok());
    // Its workspace's own account is no news.
    let h = Harness::new(Locale::En, "graphite").edit(|state| {
        state.workspaces[1].panes[0].claude_account = Some("perso".into());
    });
    let mut ui = h.show(pane_badge(h.ctx(), &w, &p));
    assert!(ui.find("Perso").is_err());
    // A shell has no account.
    let (w, p) = (ws("w1"), "p2".to_string());
    let mut ui = h.show(pane_badge(h.ctx(), &w, &p));
    assert!(ui.find("Principal").is_err());
}

#[test]
fn a_pane_on_its_way_to_another_account_says_so_in_its_header() {
    let mut h = Harness::new(Locale::En, "graphite");
    let (w, p) = (ws("w1"), "p1".to_string());
    h.model.live.accounts.moving.insert(
        p.clone(),
        Moving {
            to: "perso".into(),
            hold: Hold::Turn,
        },
    );
    {
        let mut ui = h.show(pane_badge(h.ctx(), &w, &p));
        assert!(ui.find("→ Perso").is_ok());
        h.shot(&mut ui, "accounts-pane-moving-en-graphite");
    }
    // Toward an account not logged in: it stays, and says why.
    h.model.live.accounts.moving.insert(
        p.clone(),
        Moving {
            to: "pro".into(),
            hold: Hold::NotLoggedIn,
        },
    );
    let mut ui = h.show(pane_badge(h.ctx(), &w, &p));
    assert!(ui.find("Travail: not logged in").is_ok());
    h.shot(&mut ui, "accounts-pane-moving-blocked-en-graphite");
}

fn labels(entries: &[MenuEntry]) -> Vec<String> {
    entries
        .iter()
        .map(|entry| match entry {
            MenuEntry::Item { label, checked, .. } => {
                format!("{}{label}", if *checked == Some(true) { "✓ " } else { "" })
            }
            MenuEntry::Heading(text) => format!("# {text}"),
            MenuEntry::Separator => "---".to_string(),
        })
        .collect()
}

#[test]
fn the_workspaces_menu_lists_the_default_and_every_account_with_the_choice_checked() {
    let h = Harness::new(Locale::En, "graphite");
    let w = ws("w2");
    assert_eq!(
        labels(&menu_items(h.ctx(), &w, None)),
        [
            "---",
            "# Claude account",
            "Default (Principal)",
            "Principal",
            "✓ Perso",
            "Travail"
        ]
    );
    let w = ws("w1");
    let entries = menu_items(h.ctx(), &w, None);
    assert_eq!(labels(&entries)[2], "✓ Default (Principal)");
    // A pick is « this workspace, not restarting » (the toolbar's own click
    // asks about that).
    let MenuEntry::Item { on_select, .. } = &entries[4] else {
        panic!("an item");
    };
    assert!(matches!(
        on_select,
        Message::Accounts(Msg::SetWorkspace { ws, id: Some(id), restart: false })
            if ws == "w1" && id == "perso"
    ));
}

#[test]
fn a_claude_panes_menu_names_what_its_workspace_follows() {
    let h = Harness::new(Locale::En, "graphite");
    let (w, p) = (ws("w2"), "p3".to_string());
    // « Default » is what the workspace is on: Perso.
    assert_eq!(
        labels(&menu_items(h.ctx(), &w, Some(&p))),
        [
            "---",
            "# Claude account for this pane",
            "✓ Default (Perso)",
            "Principal",
            "Perso",
            "Travail"
        ]
    );
    let entries = menu_items(h.ctx(), &w, Some(&p));
    let MenuEntry::Item { on_select, .. } = &entries[5] else {
        panic!("an item");
    };
    assert!(matches!(
        on_select,
        Message::Accounts(Msg::SetPane { ws, pane, id: Some(id) })
            if ws == "w2" && pane == "p3" && id == "pro"
    ));
}

#[test]
fn a_shell_or_a_login_pane_has_no_account_entries() {
    let h = Harness::new(Locale::En, "graphite").edit(|state| {
        state.workspaces[0].panes[0].login = Some(true);
    });
    let w = ws("w1");
    // p2 is a shell, p1 was opened to log in.
    assert!(menu_items(h.ctx(), &w, Some(&"p2".to_string())).is_empty());
    assert!(menu_items(h.ctx(), &w, Some(&"p1".to_string())).is_empty());
    // Not a pane of the workspace.
    assert!(menu_items(h.ctx(), &w, Some(&"nope".to_string())).is_empty());
}

#[test]
fn no_entries_before_the_accounts_are_known() {
    let mut h = Harness::new(Locale::En, "graphite");
    h.model.live.accounts.accounts = None;
    assert!(menu_items(h.ctx(), &ws("w1"), None).is_empty());
}

// ── the picker ────────────────────────────────────────────────────────

#[test]
fn a_picker_whose_account_is_gone_says_the_default_applies() {
    let h = Harness::new(Locale::En, "graphite");
    let mut ui = h.show(workspace_picker(h.ctx(), Some("gone")));
    assert!(
        ui.find("The account \u{201c}gone\u{201d} no longer exists: the default applies.")
            .is_ok()
    );
    let mut ui = h.show(workspace_picker(h.ctx(), Some("perso")));
    assert!(ui.find("no longer exists").is_err());
    let mut ui = h.show(workspace_picker(h.ctx(), None));
    assert!(ui.find("no longer exists").is_err());
}

// ── Settings › Accounts ───────────────────────────────────────────────

#[test]
fn the_page_lists_every_account_with_who_uses_it_and_what_can_be_done() {
    let h = Harness::new(Locale::En, "graphite");
    let st = State::default();
    let mut ui = h.show(settings_page(&st, h.ctx()));
    for text in [
        "Claude accounts",
        "Principal",
        "Perso",
        "Travail",
        "Default",
        "Logged in",
        "Not logged in",
        "Max",
        "perso@example.com",
        "Used by hangar",
        "Used by shop",
        "No workspace uses it",
        "Log in",
        "Make default",
        "Rename",
        "Remove",
        "The main account is Claude Code\u{2019}s own configuration: it cannot be removed.",
        "Add an account",
        "Add and log in",
        "A pane opens so you can log in through your browser.",
        "Ask before moving running agents",
    ] {
        assert!(ui.find(text).is_ok(), "{text} is on the page");
    }
    h.shot(&mut ui, "accounts-page-en-graphite");
}

#[test]
fn the_page_speaks_french_in_a_light_theme() {
    let h = Harness::new(Locale::Fr, "github-light");
    let st = State::default();
    let mut ui = h.show(settings_page(&st, h.ctx()));
    for text in [
        "Comptes Claude",
        "Non connecté",
        "Se connecter",
        "Ajouter un compte",
    ] {
        assert!(ui.find(text).is_ok(), "{text} is on the page");
    }
    h.shot(&mut ui, "accounts-page-fr-github-light");
}

#[test]
fn a_machine_that_cannot_separate_logins_is_warned() {
    let mut h = Harness::new(Locale::En, "graphite");
    h.model.live.accounts.support = Some(Support {
        isolated_logins: false,
        note: Some("Update Claude Code first.".into()),
    });
    let mut st = State::default();
    st.page.error = Some("Could not do that: disk full".into());
    let mut ui = h.show(settings_page(&st, h.ctx()));
    for text in [
        "Logins are not kept apart on this computer",
        "Update Claude Code first.",
        "Could not do that: disk full",
    ] {
        assert!(ui.find(text).is_ok(), "{text}");
    }
    h.shot(&mut ui, "accounts-page-warning-en-graphite");
    let mut isolated = Harness::new(Locale::En, "graphite");
    isolated.model.live.accounts.support = Some(Support {
        isolated_logins: true,
        note: None,
    });
    let quiet = State::default();
    let mut ui = isolated.show(settings_page(&quiet, isolated.ctx()));
    assert!(
        ui.find("Logins are not kept apart on this computer")
            .is_err()
    );
}

#[test]
fn the_default_account_has_no_make_default_and_a_logged_in_one_no_log_in() {
    let h = Harness::new(Locale::En, "graphite");
    let st = State::default();
    let mut ui = h.show(settings_page(&st, h.ctx()));
    // Make default is offered for the two accounts that are not the default.
    ui.click("Make default").expect("a button is drawn");
    assert!(accounts_msg(&messages(ui), |msg| matches!(
        msg,
        Msg::MakeDefault(id) if id == "perso"
    )));
    let mut ui = h.show(settings_page(&st, h.ctx()));
    ui.click("Log in").expect("a button is drawn");
    assert!(accounts_msg(&messages(ui), |msg| matches!(
        msg,
        Msg::LogIn(id) if id == "pro"
    )));
}

#[test]
fn renaming_opens_a_form_that_refuses_an_empty_or_taken_name() {
    let h = Harness::new(Locale::En, "graphite");
    let renaming = |typed: &str| {
        let mut st = State::default();
        st.page.renaming = Some(("perso".into(), typed.into()));
        st
    };
    let empty = renaming("");
    let mut ui = h.show(settings_page(&empty, h.ctx()));
    assert!(ui.find("Give the account a name.").is_ok());
    h.shot(&mut ui, "accounts-page-rename-en-graphite");
    ui.click("Save").expect("the button is drawn");
    assert!(!accounts_msg(&messages(ui), |msg| matches!(
        msg,
        Msg::RenameSubmit
    )));

    let taken = renaming("travail");
    let mut ui = h.show(settings_page(&taken, h.ctx()));
    assert!(ui.find("Another account already has this name.").is_ok());

    let good = renaming("Personal");
    let mut ui = h.show(settings_page(&good, h.ctx()));
    ui.click("Save").expect("the button is drawn");
    assert!(accounts_msg(&messages(ui), |msg| matches!(
        msg,
        Msg::RenameSubmit
    )));
    let mut ui = h.show(settings_page(&good, h.ctx()));
    ui.click("Cancel").expect("the button is drawn");
    assert!(accounts_msg(&messages(ui), |msg| matches!(
        msg,
        Msg::RenameCancel
    )));
}

#[test]
fn removing_asks_first_and_the_main_account_cannot_be_removed() {
    let h = Harness::new(Locale::En, "graphite");
    let mut st = State::default();
    st.page.removing = Some("perso".into());
    let mut ui = h.show(settings_page(&st, h.ctx()));
    for text in [
        "Remove \u{201c}Perso\u{201d}?",
        "Its login is deleted from this computer. Workspaces on it go back to the default account.",
        "Remove the account",
    ] {
        assert!(ui.find(text).is_ok(), "{text}");
    }
    h.shot(&mut ui, "accounts-page-remove-en-graphite");
    ui.click("Remove the account").expect("the button is drawn");
    assert!(accounts_msg(&messages(ui), |msg| matches!(
        msg,
        Msg::RemoveConfirm(id) if id == "perso"
    )));
}

#[test]
fn everything_is_disabled_while_a_command_runs() {
    let h = Harness::new(Locale::En, "graphite");
    let mut st = State::default();
    st.page.busy = true;
    let mut ui = h.show(settings_page(&st, h.ctx()));
    ui.click("Rename").expect("a button is drawn");
    ui.click("Add and log in").expect("the button is drawn");
    ui.click("Make default").expect("a button is drawn");
    assert!(messages(ui).is_empty());
}

#[test]
fn the_add_form_shows_its_problem_only_after_a_first_try() {
    let h = Harness::new(Locale::En, "graphite");
    let fresh = State::default();
    let mut ui = h.show(settings_page(&fresh, h.ctx()));
    assert!(ui.find("Give the account a name.").is_err());

    let mut tried = State::default();
    tried.page.touched = true;
    let mut ui = h.show(settings_page(&tried, h.ctx()));
    assert!(ui.find("Give the account a name.").is_ok());

    let mut taken = State::default();
    taken.page.touched = true;
    taken.page.name = "perso".into();
    let mut ui = h.show(settings_page(&taken, h.ctx()));
    assert!(ui.find("Another account already has this name.").is_ok());
}

// ── the page's commands ───────────────────────────────────────────────

#[test]
fn a_rename_starts_only_with_a_good_name_and_the_page_then_waits() {
    let h = Harness::new(Locale::En, "graphite");
    let mut page = Page::default();
    assert_eq!(begin_rename_for_test(&mut page, &h), None);
    page.renaming = Some(("perso".into(), "  ".into()));
    assert_eq!(begin_rename_for_test(&mut page, &h), None);
    assert!(!page.busy);
    page.renaming = Some(("perso".into(), " Personal ".into()));
    assert_eq!(
        begin_rename_for_test(&mut page, &h),
        Some(("perso".to_string(), "Personal".to_string()))
    );
    assert!(page.busy);
}

fn begin_rename_for_test(page: &mut Page, h: &Harness) -> Option<(String, String)> {
    settings_page::begin_rename(page, &h.model)
}

#[test]
fn a_removal_never_starts_for_main() {
    let mut page = Page::default();
    assert!(!settings_page::begin_remove(&mut page, "main"));
    assert!(!page.busy);
    assert!(settings_page::begin_remove(&mut page, "perso"));
    assert!(page.busy);
}

#[test]
fn an_add_starts_with_a_good_name_and_remembers_it_was_tried() {
    let h = Harness::new(Locale::En, "graphite");
    let mut page = Page::default();
    assert_eq!(settings_page::begin_add(&mut page, &h.model), None);
    assert!(page.touched);
    page.name = " Work ".into();
    assert_eq!(
        settings_page::begin_add(&mut page, &h.model),
        Some("Work".to_string())
    );
    assert!(page.busy);
}

#[test]
fn what_the_commands_answer_changes_the_page() {
    let mut h = Harness::new(Locale::En, "graphite");
    let mut page = Page::default();

    // A rename that worked closes its form; one that failed keeps it open.
    page.renaming = Some(("perso".into(), "Personal".into()));
    page.busy = true;
    let _ = settings_page::step(
        &mut page,
        &mut h.model,
        Msg::Renamed(Err("No such account.".into())),
    );
    assert!(!page.busy);
    assert!(page.renaming.is_some());
    assert_eq!(
        page.error.as_deref(),
        Some("Could not do that: No such account.")
    );
    page.busy = true;
    let _ = settings_page::step(&mut page, &mut h.model, Msg::Renamed(Ok(())));
    assert!(page.renaming.is_none());

    // A removal that failed keeps its question.
    page.removing = Some("perso".into());
    page.busy = true;
    let _ = settings_page::step(
        &mut page,
        &mut h.model,
        Msg::Removed(
            "perso".into(),
            Err("1 pane still runs on this account".into()),
        ),
    );
    assert_eq!(page.removing.as_deref(), Some("perso"));
    assert!(!page.busy);
    let _ = settings_page::step(
        &mut page,
        &mut h.model,
        Msg::Removed("perso".into(), Ok(())),
    );
    assert!(page.removing.is_none());

    // An add that failed keeps the name to retry; one that worked clears it.
    page.name = "Work".into();
    page.touched = true;
    let _ = settings_page::step(&mut page, &mut h.model, Msg::Added(Err("no".into())));
    assert_eq!(page.name, "Work");
    let _ = settings_page::step(&mut page, &mut h.model, Msg::Added(Ok("work".into())));
    assert!(page.name.is_empty() && !page.touched);
}

#[test]
fn the_rename_form_follows_what_is_typed_and_goes_on_cancel() {
    let mut h = Harness::new(Locale::En, "graphite");
    let mut page = Page::default();
    let _ = settings_page::step(&mut page, &mut h.model, Msg::RenameStart("perso".into()));
    assert_eq!(page.renaming, Some(("perso".into(), "Perso".into())));
    let _ = settings_page::step(&mut page, &mut h.model, Msg::RenameInput("Personal".into()));
    assert_eq!(page.renaming, Some(("perso".into(), "Personal".into())));
    let _ = settings_page::step(&mut page, &mut h.model, Msg::RenameCancel);
    assert!(page.renaming.is_none());
    let _ = settings_page::step(&mut page, &mut h.model, Msg::RemoveAsk("pro".into()));
    assert_eq!(page.removing.as_deref(), Some("pro"));
    let _ = settings_page::step(&mut page, &mut h.model, Msg::RemoveCancel);
    assert!(page.removing.is_none());
}

// ── the strings (`strings.test.ts`, the cases that are not typography) ──
//
// The dictionary's own tests (`crates/hangar-i18n/tests/dictionary.rs`) hold
// every French string, the accounts' included, to the glossary's typography
// and both languages to the same placeholders. What is checked here is how
// the accounts' strings come out.

const NNBSP: char = '\u{202f}';

#[test]
fn the_running_agents_take_the_plural_of_the_language() {
    use hangar_i18n::{Key, tn};
    assert_eq!(
        tn(Locale::En, Key::AccountsSwitcherRunning, 1, &[]),
        "1 agent running."
    );
    assert_eq!(
        tn(Locale::Fr, Key::AccountsSwitcherRunning, 3, &[]),
        "3 agents en cours."
    );
}

#[test]
fn a_percentage_is_set_the_french_way() {
    use hangar_i18n::{Key, args, t_with};
    assert_eq!(
        t_with(Locale::Fr, Key::AccountsSwitcherPercent, &args!("n" => 40)),
        format!("40{NNBSP}%")
    );
    assert_eq!(
        t_with(Locale::En, Key::AccountsSwitcherPercent, &args!("n" => 40)),
        "40%"
    );
}

#[test]
fn what_fills_a_placeholder_is_never_touched() {
    use hangar_i18n::{Key, args, t_with};
    assert_eq!(
        t_with(
            Locale::Fr,
            Key::AccountsSettingsRemoveAsk,
            &args!("name" => "Perso : 2 ?")
        ),
        format!("Retirer «{NNBSP}Perso : 2 ?{NNBSP}»{NNBSP}?")
    );
}

// ── ACC-01: an agent that ended is not running ────────────────────────

#[test]
fn an_agent_that_ended_is_not_asked_about_or_restarted() {
    let mut h = Harness::new(Locale::En, "graphite");
    h.model.live.accounts.launched.note("p1", Some("main"));
    assert_eq!(running_in(&h.model, &ws("w1"), SwitchScope::Workspace), 1);
    // What P5 does when the pane's shell exits or the pane is closed.
    assert!(h.model.live.accounts.launched.note("p1", None));
    assert_eq!(running_in(&h.model, &ws("w1"), SwitchScope::Workspace), 0);
    assert_eq!(running_in(&h.model, &ws("w1"), SwitchScope::Default), 0);
    let workspace = h.model.workspace(&ws("w1")).expect("w1 exists");
    assert!(
        switch_workspace(workspace, &h.model.live.accounts)
            .panes
            .iter()
            .all(|pane| !pane.running)
    );
}

// ── ACC-02: Enter while a request is in flight sends nothing ──────────

#[test]
fn a_second_enter_while_busy_sends_nothing() {
    let h = Harness::new(Locale::En, "graphite");
    let mut page = Page::default();
    page.name = "Work".into();
    assert_eq!(
        settings_page::begin_add(&mut page, &h.model),
        Some("Work".to_string())
    );
    // The same Enter again, before `account_add` answered.
    assert_eq!(settings_page::begin_add(&mut page, &h.model), None);

    page.renaming = Some(("perso".into(), "Personal".into()));
    assert_eq!(begin_rename_for_test(&mut page, &h), None);
    assert!(!settings_page::begin_remove(&mut page, "perso"));
}

// ── ACC-03: an older list, an early save ──────────────────────────────

fn named(id: &str) -> ClaudeAccount {
    ClaudeAccount {
        id: id.into(),
        label: id.into(),
        is_main: id == MAIN_ACCOUNT,
        logged_in: true,
        email: None,
        plan: None,
    }
}

#[test]
fn an_older_list_that_answers_last_does_not_win() {
    let mut h = Harness::new(Locale::En, "graphite");
    let mut st = State::default();
    listed(&mut st, &mut h.model, 2, vec![named("main"), named("new")]);
    listed(&mut st, &mut h.model, 1, vec![named("main")]);
    let ids: Vec<_> = h
        .model
        .live
        .accounts
        .accounts
        .as_ref()
        .unwrap()
        .iter()
        .map(|account| account.id.as_str())
        .collect();
    assert_eq!(ids, ["main", "new"]);
}

#[test]
fn usage_is_not_saved_before_the_file_was_read() {
    let mut h = Harness::new(Locale::En, "graphite");
    let mut st = State::default();
    // A bridge report lands before the file is read.
    assert!(!may_save(&mut st));
    // Reading it releases the save that was held back, once.
    assert!(usage_arrived(&mut st, &mut h.model, Usage::default()));
    assert!(may_save(&mut st));
    assert!(!usage_arrived(&mut st, &mut h.model, Usage::default()));
}

#[test]
fn unasked_a_click_switches_at_once_and_leaves_the_restart_to_the_panes() {
    let mut h = Harness::new(Locale::En, "graphite");
    h.model.live.accounts.launched.note("p1", Some("main"));
    let mut st = State::default();
    st.switcher = Menu::Open;
    let task = switcher_step(
        &mut st,
        &mut h.model,
        choose(SwitchScope::Workspace, "perso"),
    );
    // No question, no restart from here: the pane moves between two turns.
    assert_eq!(st.switcher, Menu::Closed);
    assert_eq!(task.units(), 0);
    assert_eq!(
        h.model
            .workspace(&ws("w1"))
            .unwrap()
            .claude_account
            .as_deref(),
        Some("perso")
    );
}

#[test]
fn unasked_a_pane_pick_restarts_nothing_from_the_menu() {
    let mut h = Harness::new(Locale::En, "graphite");
    h.model.live.accounts.launched.note("p1", Some("main"));
    assert_eq!(
        set_pane_account(
            &mut h.model,
            &ws("w1"),
            &"p1".to_string(),
            Some("pro".into())
        )
        .units(),
        0
    );
}

#[test]
fn the_setting_switches_the_question_on_and_off() {
    let mut h = Harness::new(Locale::En, "graphite");
    assert!(!h.model.settings().account_switch_ask);
    let _ = set_ask_switch(&mut h.model, true);
    assert!(h.model.settings().account_switch_ask);
    let _ = set_ask_switch(&mut h.model, false);
    assert!(!h.model.settings().account_switch_ask);
}

/// The figures of two accounts: Principal on both windows, Perso on its five
/// hours only.
fn reported(h: &mut Harness) {
    h.model.live.accounts.usage = std::collections::BTreeMap::from([
        (
            "main".to_string(),
            AccountUsage {
                five_hour: window(37.0),
                weekly: window(22.0),
                ..AccountUsage::default()
            },
        ),
        (
            "perso".to_string(),
            AccountUsage {
                five_hour: window(93.0),
                weekly: None,
                ..AccountUsage::default()
            },
        ),
    ]);
}

#[test]
fn the_plan_block_has_a_line_per_account_once_several_have_reported() {
    let mut h = Harness::new(Locale::En, "graphite");
    assert!(plan_block(h.ctx()).is_none(), "nothing reported yet");
    reported(&mut h);
    let block = plan_block(h.ctx()).expect("a block per account");
    let mut ui = h.show(container(block).width(260));
    for text in ["Plan", "Principal", "Perso", "37%", "22%", "93%"] {
        assert!(ui.find(text).is_ok(), "{text} is in the block");
    }
    // Travail is not logged in and never reported: it is left out.
    assert!(ui.find("Travail").is_err());
    h.shot(&mut ui, "accounts-plan-en-graphite");

    let mut h = Harness::new(Locale::Fr, "github-light");
    reported(&mut h);
    let block = plan_block(h.ctx()).expect("a block per account");
    let mut ui = h.show(container(block).width(260));
    assert!(ui.find("Forfait").is_ok());
    h.shot(&mut ui, "accounts-plan-fr-github-light");
}

#[test]
fn a_single_account_keeps_the_plain_plan_block() {
    let mut h = Harness::new(Locale::En, "graphite");
    h.model.live.accounts.accounts = Some(vec![crate::workspace_dialog::testing::account(
        "main",
        "Principal",
        true,
    )]);
    reported(&mut h);
    assert!(plan_block(h.ctx()).is_none());
}

/// The "→ Perso" chip's tooltip says what the pane waits for: its turn,
/// a mod that does not report it, a draft, the command it runs by name, or
/// processes that could not be read.
#[test]
fn a_moving_panes_tooltip_says_what_it_waits_for() {
    let t = hangar_i18n::Translator::new(Locale::En);
    let title = |hold: Hold| badge::moving_title(t, "Perso", &hold);
    assert!(title(Hold::Turn).contains("its turn is over"));
    assert!(title(Hold::Unreported).contains("hangar-bridge"));
    assert!(title(Hold::Draft).contains("typed"));
    assert!(title(Hold::Command("cargo build --release".into())).contains("cargo build --release"));
    assert!(title(Hold::Unreadable).contains("could not be read"));
    assert!(title(Hold::NotLoggedIn).contains("not logged in"));
    let fr = hangar_i18n::Translator::new(Locale::Fr);
    assert!(badge::moving_title(fr, "Perso", &Hold::Command("make".into())).contains("make"));
}

/// Asked first, a switch restarts the agents of this computer only: a pane
/// on a node runs on the node's login, and is neither counted nor
/// restarted.
#[test]
fn a_pane_on_a_node_is_not_restarted_by_a_switch() {
    let mut h = asking().edit(|state| {
        state.workspaces[0].panes.push(crate::state::Pane {
            id: "far".into(),
            name: "Far".into(),
            agent: crate::state::AgentId::Claude,
            node: Some("n1".into()),
            ..crate::state::Pane::default()
        });
    });
    h.model.live.accounts.launched.note("far", Some("main"));
    assert_eq!(running_in(&h.model, &ws("w1"), SwitchScope::Workspace), 0);
    assert_eq!(
        set_workspace_account(&mut h.model, &ws("w1"), Some("perso".into()), true).units(),
        0
    );
    h.model.live.accounts.launched.note("p1", Some("main"));
    assert_eq!(running_in(&h.model, &ws("w1"), SwitchScope::Workspace), 1);
}

// ── the badges' styles ─────────────────────────────────────────────────

/// Coloured badges on: the main account amber with the bolt, « Perso » green
/// with the gamepad, « Travail » plain.
fn styled(h: Harness) -> Harness {
    h.edit(|state| {
        let settings = &mut state.settings;
        settings.account_badges = true;
        settings.set_account_tone("main", Some(AccountTone::Warning));
        settings.set_account_icon("main", Some("bolt"));
        settings.set_account_tone("perso", Some(AccountTone::Success));
        settings.set_account_icon("perso", Some("gamepad"));
    })
}

/// Where the name `text` starts in `ui`.
fn x_of(ui: &mut iced_test::Simulator<'_, Message>, text: &str) -> f32 {
    ui.find(text).expect("the name is drawn").bounds().x
}

/// How much further right a name starts in a badge of a tone with an icon
/// than as a plain word: the capsule's side padding, the icon and the gap
/// after it. A badge with no tone has no capsule, one with no icon no icon.
fn badge_shift(size: f32) -> f32 {
    space::SM + icon_side(size) + space::XS
}

#[test]
fn a_styled_account_wears_its_badge_in_the_navigator() {
    let own = ws("w2");
    let plain = Harness::new(Locale::En, "graphite");
    let plain = x_of(&mut plain.show(badge(plain.ctx(), &own)), "Perso");

    let h = styled(Harness::new(Locale::En, "graphite"));
    assert_eq!(
        account_style::of(h.ctx(), "perso"),
        Mark {
            tone: Some(AccountTone::Success),
            icon: Some(hangar_ui::Icon::Gamepad),
        }
    );
    let mut ui = h.show(badge(h.ctx(), &own));
    let shift = x_of(&mut ui, "Perso") - plain;
    assert!(
        (shift - badge_shift(font_size::MICRO)).abs() < 0.5,
        "the name moved {shift}: it is not in its badge"
    );
    h.shot(&mut ui, "accounts-badge-styled-en-graphite");
}

#[test]
fn a_styled_account_wears_its_badge_in_the_menu() {
    let mut st = State::default();
    st.switcher = Menu::Open;
    let plain = Harness::new(Locale::En, "graphite");
    let mut ui = plain.show(toolbar(switcher(&st, plain.ctx())));
    let (perso, travail) = (x_of(&mut ui, "Perso"), x_of(&mut ui, "Travail"));
    drop(ui);

    let h = styled(Harness::new(Locale::En, "graphite"));
    let mut ui = h.show(toolbar(switcher(&st, h.ctx())));
    let shift = x_of(&mut ui, "Perso") - perso;
    assert!(
        (shift - badge_shift(font_size::BODY)).abs() < 0.5,
        "the name moved {shift}: it is not in its badge"
    );
    // An account with no style keeps its plain name, where it was.
    assert!((x_of(&mut ui, "Travail") - travail).abs() < 0.5);
    // Not logged in says so on the line under the name.
    assert!(ui.find("not logged in").is_ok());
    h.shot(&mut ui, "accounts-switcher-styled-en-graphite");
}

#[test]
fn the_page_offers_a_tone_and_an_icon_and_a_choice_is_kept() {
    let mut h = Harness::new(Locale::En, "graphite");
    let st = State::default();
    let mut page = Page::default();
    let t = hangar_i18n::Translator::new(Locale::En);
    // The switch's own label is drawn by iced's toggler, which no search
    // finds: its hint, right under it, stands for it.
    let switch = t.t(hangar_i18n::Key::AccountsSettingsBadgesHint);
    let editor = t.t(hangar_i18n::Key::AccountsSettingsBadge);
    {
        // Off by default: the page offers the switch, not the choices.
        let mut ui = h.show(settings_page(&st, h.ctx()));
        assert!(ui.find(switch.as_ref()).is_ok());
        assert!(ui.find(editor.as_ref()).is_err());
    }
    let _ = settings_page::step(&mut page, &mut h.model, Msg::SetBadges(true));
    assert!(h.model.settings().account_badges);
    {
        let mut ui = h.show(settings_page(&st, h.ctx()));
        assert!(ui.find(editor.as_ref()).is_ok());
        assert!(ui.find("No icon").is_ok());
        h.shot(&mut ui, "accounts-page-badges-en-graphite");
    }
    let _ = settings_page::step(
        &mut page,
        &mut h.model,
        Msg::SetTone("perso".into(), Some(AccountTone::Success)),
    );
    let _ = settings_page::step(
        &mut page,
        &mut h.model,
        Msg::SetIcon("perso".into(), Some(hangar_ui::Icon::Gamepad)),
    );
    assert_eq!(
        h.model.settings().account_style("perso"),
        AccountStyle {
            tone: Some(AccountTone::Success),
            icon: Some("gamepad".into()),
        }
    );
    {
        let mut ui = h.show(settings_page(&st, h.ctx()));
        h.shot(&mut ui, "accounts-page-badges-chosen-en-graphite");
    }

    // Taking both back leaves nothing in the file.
    let _ = settings_page::step(&mut page, &mut h.model, Msg::SetTone("perso".into(), None));
    let _ = settings_page::step(&mut page, &mut h.model, Msg::SetIcon("perso".into(), None));
    assert!(h.model.settings().account_styles.is_empty());
}

#[test]
fn with_badges_off_every_account_keeps_its_plain_label() {
    let h = styled(Harness::new(Locale::En, "graphite")).edit(|state| {
        state.settings.account_badges = false;
    });
    // The choices stay kept for when the badges are turned on again...
    assert!(h.model.settings().account_styles.contains_key("perso"));
    // ...but nothing draws them while they are off.
    assert!(!account_style::shown(h.ctx()));
    assert!(account_style::of(h.ctx(), "perso").is_plain());
    assert!(account_style::of(h.ctx(), "main").is_plain());
}

/// With a single account no badge is drawn, so the switch is not offered,
/// and a style chosen before the others went shows nowhere, the toolbar
/// included, rather than stay on with no switch to turn it off.
#[test]
fn a_single_account_is_offered_no_badge() {
    let mut h = styled(Harness::new(Locale::En, "graphite"));
    h.model.live.accounts.accounts = h
        .model
        .live
        .accounts
        .accounts
        .take()
        .map(|all| all.into_iter().filter(|account| account.is_main).collect());
    assert!(!account_style::shown(h.ctx()));
    assert!(account_style::of(h.ctx(), "main").is_plain());
    let t = hangar_i18n::Translator::new(Locale::En);
    let switch = t.t(hangar_i18n::Key::AccountsSettingsBadgesHint);
    let st = State::default();
    let mut ui = h.show(settings_page(&st, h.ctx()));
    assert!(ui.find("Principal").is_ok());
    assert!(ui.find(switch.as_ref()).is_err());
}

#[test]
fn a_renamed_account_keeps_its_badge_and_a_removed_one_takes_it_along() {
    let mut h = styled(Harness::new(Locale::En, "graphite"));
    let mut page = Page::default();
    page.renaming = Some(("perso".into(), "Personal".into()));
    page.busy = true;
    let _ = settings_page::step(&mut page, &mut h.model, Msg::Renamed(Ok(())));
    assert!(h.model.settings().account_styles.contains_key("perso"));

    page.busy = true;
    let _ = settings_page::step(
        &mut page,
        &mut h.model,
        Msg::Removed("perso".into(), Ok(())),
    );
    assert!(!h.model.settings().account_styles.contains_key("perso"));
    assert!(h.model.settings().account_styles.contains_key("main"));
}
