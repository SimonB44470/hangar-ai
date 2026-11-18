//! The pure logic behind « Comptes Claude »'s interface (`lib/accountSwitch.ts`):
//! which panes restart after a switch, how much of each account's limits is
//! used, and how an account is named. No store, no commands: the views and the
//! update hand it plain data.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::state::live::RateLimit;

use super::model::{AccountId, ClaudeAccount, read_account_choice, resolve_account};

/// What this logic needs to know of a pane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SwitchPane {
    pub id: String,
    /// `claude` is the only agent an account applies to.
    pub agent: String,
    /// The pane's own account (override); absent or blank follows its
    /// workspace.
    pub claude_account: Option<String>,
    /// Only a running agent has anything to restart.
    pub running: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SwitchWorkspace {
    pub id: String,
    /// The workspace's own account; absent or blank follows the default.
    pub claude_account: Option<String>,
    pub panes: Vec<SwitchPane>,
}

fn is_claude(pane: &SwitchPane) -> bool {
    pane.agent == "claude"
}

fn has_own(choice: Option<&str>) -> bool {
    read_account_choice(choice).is_some()
}

/// The Claude Code panes of `workspace` that follow it, so that change when
/// its account does. A pane with an account of its own is untouched.
pub fn panes_to_restart_on_workspace_change(workspace: &SwitchWorkspace) -> Vec<&SwitchPane> {
    workspace
        .panes
        .iter()
        .filter(|pane| is_claude(pane) && !has_own(pane.claude_account.as_deref()))
        .collect()
}

/// The Claude Code panes that follow the default when it changes: those of
/// the workspaces without an account of their own, minus the panes with
/// theirs.
pub fn panes_to_restart_on_default_change(workspaces: &[SwitchWorkspace]) -> Vec<&SwitchPane> {
    workspaces
        .iter()
        .filter(|workspace| !has_own(workspace.claude_account.as_deref()))
        .flat_map(panes_to_restart_on_workspace_change)
        .collect()
}

/// How many of `panes` are running an agent right now.
pub fn running_count(panes: &[&SwitchPane]) -> usize {
    panes.iter().filter(|pane| pane.running).count()
}

/// One report of the hangar-bridge: a pane's rate limits at a moment.
#[derive(Debug, Clone, PartialEq)]
pub struct UsageMeasure {
    pub pane_id: String,
    /// Epoch milliseconds.
    pub at: u64,
    pub rate_limits: Vec<RateLimit>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowUsage {
    pub percent_used: f64,
    pub resets_at: Option<String>,
    pub at: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountUsage {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub five_hour: Option<WindowUsage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weekly: Option<WindowUsage>,
    /// The windows of a gateway Claude Code runs through, by their kind
    /// (any but the plan's two), as the plan block shows them. Written only
    /// when there are some, so a file from before stays as it was.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub others: BTreeMap<String, WindowUsage>,
}

/// The usage of each account, by id. The file `account-usage.json` is this,
/// as the web app's `hangar.accountUsage` was.
pub type Usage = BTreeMap<AccountId, AccountUsage>;

pub(super) const FIVE_HOUR: &str = "five_hour";
pub(super) const WEEKLY: &str = "seven_day";

/// The usage of each account, from the measures of the panes launched on it:
/// the latest one per account and per window, the plan's two and a
/// gateway's (`AccountUsage::others`). A window an account never reported
/// is absent, which the interface leaves out. A pane whose account is not
/// known (`account_of` gives nothing) counts for nobody.
pub fn usage_by_account(
    measures: &[UsageMeasure],
    account_of: impl Fn(&str) -> Option<AccountId>,
) -> Usage {
    let mut result = Usage::new();
    for measure in measures {
        let Some(account) = account_of(&measure.pane_id).filter(|account| !account.is_empty())
        else {
            continue;
        };
        for limit in &measure.rate_limits {
            // Only a named window and a real number: an account that
            // reported nothing usable gets no entry at all.
            if limit.kind.is_empty() || !limit.percent_used.is_finite() {
                continue;
            }
            let fresh = WindowUsage {
                percent_used: limit.percent_used,
                resets_at: limit.resets_at.clone(),
                at: measure.at,
            };
            let entry = result.entry(account.clone()).or_default();
            let slot = match limit.kind.as_str() {
                FIVE_HOUR => &mut entry.five_hour,
                WEEKLY => &mut entry.weekly,
                other => {
                    if entry
                        .others
                        .get(other)
                        .is_none_or(|known| known.at <= measure.at)
                    {
                        entry.others.insert(other.to_string(), fresh);
                    }
                    continue;
                }
            };
            if slot.as_ref().is_some_and(|known| known.at > measure.at) {
                continue;
            }
            *slot = Some(fresh);
        }
    }
    result
}

/// The account's label, or `unknown` when it does not exist (any more).
pub fn account_name(accounts: &[ClaudeAccount], id: Option<&str>, unknown: &str) -> String {
    accounts
        .iter()
        .find(|account| Some(account.id.as_str()) == id)
        .map_or_else(|| unknown.to_string(), |account| account.label.clone())
}

/// « Default (Perso) »: `template` holds `{name}`. The default is what the
/// settings name, main when they name nothing or an account that is gone.
pub fn default_label(
    accounts: &[ClaudeAccount],
    default: Option<&str>,
    template: &str,
    unknown: &str,
) -> String {
    let resolved = resolve_account(accounts, default, None, None);
    template.replacen(
        "{name}",
        &account_name(accounts, Some(&resolved.id), unknown),
        1,
    )
}

/// The workspaces whose own account is `id`. Kept from `lib/accountSwitch.ts`
/// for the callers that count them (the page lists who uses an account
/// through the resolution instead, which also counts the ones that follow the
/// default).
#[allow(dead_code)]
pub fn workspaces_using<'a>(
    workspaces: &'a [SwitchWorkspace],
    id: &str,
) -> Vec<&'a SwitchWorkspace> {
    workspaces
        .iter()
        .filter(|workspace| {
            read_account_choice(workspace.claude_account.as_deref()).as_deref() == Some(id)
        })
        .collect()
}

/// `known` with `fresh` laid over it: per account and window, the more recent
/// measure wins. Neither argument is modified.
pub fn merge_usage(known: &Usage, fresh: &Usage) -> Usage {
    let mut result = known.clone();
    for (account, usage) in fresh {
        let merged = result.entry(account.clone()).or_default();
        let newer = |next: &Option<WindowUsage>, old: &Option<WindowUsage>| match (next, old) {
            (Some(next), Some(old)) => next.at >= old.at,
            (Some(_), None) => true,
            _ => false,
        };
        if newer(&usage.five_hour, &merged.five_hour) {
            merged.five_hour = usage.five_hour.clone();
        }
        if newer(&usage.weekly, &merged.weekly) {
            merged.weekly = usage.weekly.clone();
        }
        for (kind, window) in &usage.others {
            if merged
                .others
                .get(kind)
                .is_none_or(|old| window.at >= old.at)
            {
                merged.others.insert(kind.clone(), window.clone());
            }
        }
    }
    result
}

/// What a running Claude Code pane should do about its account, when the
/// person does not want to be asked (`Settings::account_switch_ask` off).
///
/// Claude Code reads its login once, as it starts (`CLAUDE_CONFIG_DIR`), so a
/// pane moves to another account by restarting on its conversation, which
/// every account finds, `projects/` being shared (core's accounts.rs). A
/// restart ends whatever the agent runs, so a pane moves only once Hangar
/// knows it has nothing to lose: its turn over as the hangar-bridge mod
/// reports it (never a guess read off the screen), nothing typed in its
/// prompt, and no command of its own still running below it (a build, a
/// watch on its CI: `hangar_core::pty::pane_job`). What is not known yet
/// waits, and is looked at again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Follow {
    /// Nothing to do: on its account already, or not a pane an account moves.
    Stay,
    /// Everything else says go: ask whether a command it started still runs
    /// below it, and move once nothing does.
    Check,
    /// Restart now on `FollowInput::wanted`.
    Now,
    /// Later: its agent is inside a turn (at work, or waiting on the person),
    /// or its turns are not reported (no mod loaded, or nothing said yet).
    AfterTurn,
    /// Later: the person typed in it since its last turn and has not sent it.
    AfterDraft,
    /// Later: a command it started still runs, or nobody can tell: asked
    /// again a little later.
    AfterJob,
    /// Not at all: the account it should move to is not logged in, so a
    /// restart would only land it back where it is.
    NotLoggedIn,
}

impl Follow {
    /// The pane is to move, as soon as it can: its header says so.
    pub fn pending(self) -> bool {
        matches!(
            self,
            Follow::Check | Follow::AfterTurn | Follow::AfterDraft | Follow::AfterJob
        )
    }
}

/// Where a pane's agent stands in its turns, as the hangar-bridge mod
/// reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Turn {
    /// At work, or waiting on the person: the turn needs it.
    Inside,
    /// Between two turns.
    Between,
    /// Not reported: the mod is not loaded, or has said nothing yet. What
    /// the screen suggests is no ground for a restart.
    Unknown,
}

/// What is known of the commands a pane's agent started itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Job {
    /// Not asked yet.
    Unasked,
    /// Nothing it started runs.
    Nothing,
    /// One runs, or its process table could not be read.
    Busy,
}

/// What `follow` reads of a pane, all of it known without a service.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FollowInput<'a> {
    /// A Claude Code pane run by this computer (not on a node, whose
    /// Claude Code starts on the node's own login), not opened to log in.
    pub movable: bool,
    /// The account its Claude Code was started on; `None` when none runs.
    pub launched: Option<&'a str>,
    /// The account it should run on now (pane > workspace > default).
    pub wanted: &'a str,
    /// Whether `wanted` is logged in; `None` while the list is not known.
    pub wanted_logged_in: Option<bool>,
    /// The account a move was already started for: core can fall back to
    /// another account at launch, and a pane must not restart over and over
    /// toward one it cannot run on.
    pub tried: Option<&'a str>,
    pub turn: Turn,
    /// Typed in since its last turn, recently.
    pub drafting: bool,
    pub job: Job,
}

pub fn follow(input: &FollowInput<'_>) -> Follow {
    let Some(launched) = input.launched else {
        return Follow::Stay;
    };
    if !input.movable || launched == input.wanted || input.tried == Some(input.wanted) {
        return Follow::Stay;
    }
    match input.wanted_logged_in {
        None => return Follow::Stay,
        Some(false) => return Follow::NotLoggedIn,
        Some(true) => {}
    }
    if input.turn != Turn::Between {
        return Follow::AfterTurn;
    }
    if input.drafting {
        return Follow::AfterDraft;
    }
    match input.job {
        Job::Unasked => Follow::Check,
        Job::Busy => Follow::AfterJob,
        Job::Nothing => Follow::Now,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account(id: &str, label: &str, is_main: bool) -> ClaudeAccount {
        ClaudeAccount {
            id: id.into(),
            label: label.into(),
            is_main,
            logged_in: true,
            email: None,
            plan: None,
        }
    }

    fn accounts() -> Vec<ClaudeAccount> {
        vec![
            account("main", "Principal", true),
            account("perso", "Perso", false),
        ]
    }

    fn pane(id: &str, agent: &str, claude_account: Option<&str>, running: bool) -> SwitchPane {
        SwitchPane {
            id: id.into(),
            agent: agent.into(),
            claude_account: claude_account.map(str::to_string),
            running,
        }
    }

    fn workspaces() -> Vec<SwitchWorkspace> {
        vec![
            SwitchWorkspace {
                id: "w1".into(),
                claude_account: None,
                panes: vec![
                    pane("a", "claude", None, true),
                    pane("b", "shell", None, true),
                    pane("c", "claude", Some("perso"), true),
                    pane("d", "claude", None, false),
                ],
            },
            SwitchWorkspace {
                id: "w2".into(),
                claude_account: Some("perso".into()),
                panes: vec![pane("e", "claude", None, true)],
            },
            SwitchWorkspace {
                id: "w3".into(),
                claude_account: Some("  ".into()),
                panes: vec![
                    pane("f", "shell", None, false),
                    pane("g", "claude", None, false),
                ],
            },
        ]
    }

    fn ids(panes: &[&SwitchPane]) -> Vec<String> {
        panes.iter().map(|pane| pane.id.clone()).collect()
    }

    #[test]
    fn restarts_only_the_claude_panes_that_follow_the_workspace() {
        let all = workspaces();
        assert_eq!(
            ids(&panes_to_restart_on_workspace_change(&all[0])),
            ["a", "d"]
        );
        assert_eq!(ids(&panes_to_restart_on_workspace_change(&all[1])), ["e"]);
    }

    #[test]
    fn leaves_workspaces_and_panes_with_their_own_account_out_of_a_default_change() {
        assert_eq!(
            ids(&panes_to_restart_on_default_change(&workspaces())),
            ["a", "d", "g"]
        );
    }

    #[test]
    fn counts_the_running_ones() {
        let all = workspaces();
        assert_eq!(
            running_count(&panes_to_restart_on_workspace_change(&all[0])),
            1
        );
    }

    fn limit(kind: &str, percent_used: f64, resets_at: Option<&str>) -> RateLimit {
        RateLimit {
            kind: kind.into(),
            percent_used,
            resets_at: resets_at.map(str::to_string),
        }
    }

    fn owner(pane: &str) -> Option<AccountId> {
        match pane {
            "p1" | "p2" => Some("main".into()),
            "p3" => Some("perso".into()),
            _ => None,
        }
    }

    #[test]
    fn keeps_the_latest_measure_per_account_and_window() {
        let usage = usage_by_account(
            &[
                UsageMeasure {
                    pane_id: "p1".into(),
                    at: 200,
                    rate_limits: vec![limit("five_hour", 40.0, None)],
                },
                UsageMeasure {
                    pane_id: "p2".into(),
                    at: 100,
                    rate_limits: vec![
                        limit("five_hour", 10.0, None),
                        limit("seven_day", 55.4, Some("x")),
                    ],
                },
                UsageMeasure {
                    pane_id: "p3".into(),
                    at: 50,
                    rate_limits: vec![limit("seven_day", 5.0, None)],
                },
            ],
            owner,
        );
        let main = &usage["main"];
        assert_eq!(main.five_hour.as_ref().map(|w| w.percent_used), Some(40.0));
        assert_eq!(main.weekly.as_ref().map(|w| w.percent_used), Some(55.4));
        assert!(usage["perso"].five_hour.is_none());
    }

    #[test]
    fn ignores_unknown_panes_and_non_numbers() {
        let usage = usage_by_account(
            &[
                UsageMeasure {
                    pane_id: "zz".into(),
                    at: 1,
                    rate_limits: vec![limit("five_hour", 9.0, None)],
                },
                UsageMeasure {
                    pane_id: "p1".into(),
                    at: 2,
                    rate_limits: vec![
                        limit("five_hour", f64::NAN, None),
                        limit("monthly", f64::INFINITY, None),
                        limit("", 9.0, None),
                    ],
                },
            ],
            owner,
        );
        assert!(usage.is_empty(), "{usage:?}");
    }

    /// A gateway's own windows are kept apart from the plan's two, the
    /// latest of each, and survive a merge: the plan block shows them.
    #[test]
    fn keeps_a_gateways_windows_apart() {
        let usage = usage_by_account(
            &[
                UsageMeasure {
                    pane_id: "p1".into(),
                    at: 2,
                    rate_limits: vec![
                        limit("monthly", 12.0, Some("2026-12-01T00:00:00Z")),
                        limit("five_hour", 30.0, None),
                    ],
                },
                UsageMeasure {
                    pane_id: "p2".into(),
                    at: 1,
                    rate_limits: vec![limit("monthly", 99.0, None)],
                },
            ],
            owner,
        );
        let main = &usage["main"];
        assert_eq!(main.five_hour.as_ref().map(|w| w.percent_used), Some(30.0));
        assert!(main.weekly.is_none());
        let monthly = &main.others["monthly"];
        assert_eq!((monthly.percent_used, monthly.at), (12.0, 2));

        let later = usage_by_account(
            &[UsageMeasure {
                pane_id: "p1".into(),
                at: 3,
                rate_limits: vec![limit("monthly", 20.0, None)],
            }],
            owner,
        );
        let merged = merge_usage(&usage, &later);
        assert_eq!(merged["main"].others["monthly"].percent_used, 20.0);
        assert_eq!(merge_usage(&later, &usage), merged, "the newer one wins");
        assert_eq!(
            merged["main"].five_hour.as_ref().map(|w| w.percent_used),
            Some(30.0)
        );
    }

    #[test]
    fn names_an_account_or_says_unknown() {
        assert_eq!(account_name(&accounts(), Some("perso"), "unknown"), "Perso");
        assert_eq!(
            account_name(&accounts(), Some("gone"), "unknown"),
            "unknown"
        );
        assert_eq!(account_name(&accounts(), None, "unknown"), "unknown");
    }

    #[test]
    fn labels_the_default() {
        let all = accounts();
        let template = "Default ({name})";
        assert_eq!(
            default_label(&all, Some("perso"), template, "unknown"),
            "Default (Perso)"
        );
        assert_eq!(
            default_label(&all, None, template, "unknown"),
            "Default (Principal)"
        );
        assert_eq!(
            default_label(&all, Some("gone"), template, "unknown"),
            "Default (Principal)"
        );
    }

    #[test]
    fn lists_the_workspaces_on_an_account() {
        let all = workspaces();
        let using: Vec<_> = workspaces_using(&all, "perso")
            .into_iter()
            .map(|workspace| workspace.id.clone())
            .collect();
        assert_eq!(using, ["w2"]);
    }

    fn window(percent_used: f64, at: u64) -> Option<WindowUsage> {
        Some(WindowUsage {
            percent_used,
            resets_at: None,
            at,
        })
    }

    fn usage_of(five_hour: Option<WindowUsage>, weekly: Option<WindowUsage>) -> AccountUsage {
        AccountUsage {
            five_hour,
            weekly,
            ..AccountUsage::default()
        }
    }

    #[test]
    fn keeps_the_newer_measure_per_account_and_window_and_leaves_the_inputs_alone() {
        let known = Usage::from([(
            "perso".to_string(),
            usage_of(window(10.0, 100), window(40.0, 100)),
        )]);
        let fresh = Usage::from([
            ("perso".to_string(), usage_of(window(20.0, 200), None)),
            ("main".to_string(), usage_of(None, window(5.0, 50))),
        ]);
        let merged = merge_usage(&known, &fresh);
        assert_eq!(
            merged["perso"].five_hour.as_ref().map(|w| w.percent_used),
            Some(20.0)
        );
        assert_eq!(
            merged["perso"].weekly.as_ref().map(|w| w.percent_used),
            Some(40.0)
        );
        assert_eq!(
            merged["main"].weekly.as_ref().map(|w| w.percent_used),
            Some(5.0)
        );
        assert_eq!(
            known["perso"].five_hour.as_ref().map(|w| w.percent_used),
            Some(10.0)
        );
    }

    #[test]
    fn ignores_an_older_measure() {
        let known = Usage::from([("perso".to_string(), usage_of(window(30.0, 300), None))]);
        let fresh = Usage::from([("perso".to_string(), usage_of(window(1.0, 100), None))]);
        let merged = merge_usage(&known, &fresh);
        assert_eq!(
            merged["perso"].five_hour.as_ref().map(|w| w.percent_used),
            Some(30.0)
        );
    }

    #[test]
    fn the_file_keeps_the_shape_the_web_app_stored() {
        let usage = Usage::from([(
            "perso".to_string(),
            usage_of(
                Some(WindowUsage {
                    percent_used: 12.5,
                    resets_at: Some("2026-11-03T18:00:00Z".into()),
                    at: 7,
                }),
                None,
            ),
        )]);
        let text = serde_json::to_string(&usage).expect("serialises");
        assert_eq!(
            text,
            r#"{"perso":{"fiveHour":{"percentUsed":12.5,"resetsAt":"2026-11-03T18:00:00Z","at":7}}}"#
        );
        let back: Usage = serde_json::from_str(&text).expect("reads back");
        assert_eq!(back, usage);
    }

    /// A pane between two turns as the mod reports them, nothing typed, and
    /// nothing of its own found running below it.
    fn input<'a>(launched: Option<&'a str>, wanted: &'a str) -> FollowInput<'a> {
        FollowInput {
            movable: true,
            launched,
            wanted,
            wanted_logged_in: Some(true),
            tried: None,
            turn: Turn::Between,
            drafting: false,
            job: Job::Nothing,
        }
    }

    #[test]
    fn a_pane_known_to_have_nothing_to_lose_moves_at_once() {
        assert_eq!(follow(&input(Some("main"), "perso")), Follow::Now);
        assert!(!Follow::Now.pending());
    }

    #[test]
    fn a_pane_on_its_account_or_not_running_stays() {
        assert_eq!(follow(&input(Some("perso"), "perso")), Follow::Stay);
        assert_eq!(follow(&input(None, "perso")), Follow::Stay);
        // A shell, a login pane, a pane on a node.
        let elsewhere = FollowInput {
            movable: false,
            ..input(Some("main"), "perso")
        };
        assert_eq!(follow(&elsewhere), Follow::Stay);
    }

    #[test]
    fn a_pane_inside_a_turn_or_whose_turns_are_not_reported_waits() {
        for turn in [Turn::Inside, Turn::Unknown] {
            let busy = FollowInput {
                turn,
                drafting: true,
                job: Job::Unasked,
                ..input(Some("main"), "perso")
            };
            assert_eq!(follow(&busy), Follow::AfterTurn, "{turn:?}");
            assert!(follow(&busy).pending());
        }
    }

    #[test]
    fn a_draft_is_never_lost() {
        let typing = FollowInput {
            drafting: true,
            job: Job::Unasked,
            ..input(Some("main"), "perso")
        };
        assert_eq!(follow(&typing), Follow::AfterDraft);
        assert!(follow(&typing).pending());
    }

    #[test]
    fn a_command_of_its_own_is_asked_about_and_waited_for() {
        let unasked = FollowInput {
            job: Job::Unasked,
            ..input(Some("main"), "perso")
        };
        assert_eq!(follow(&unasked), Follow::Check);
        assert!(follow(&unasked).pending());
        // A build, a watch on its CI, or a process table that could not be
        // read: the pane waits and is asked again.
        let busy = FollowInput {
            job: Job::Busy,
            ..input(Some("main"), "perso")
        };
        assert_eq!(follow(&busy), Follow::AfterJob);
        assert!(follow(&busy).pending());
    }

    #[test]
    fn an_account_not_logged_in_or_unknown_moves_nothing() {
        let out = FollowInput {
            wanted_logged_in: Some(false),
            ..input(Some("main"), "perso")
        };
        assert_eq!(follow(&out), Follow::NotLoggedIn);
        assert!(!follow(&out).pending());
        let unknown = FollowInput {
            wanted_logged_in: None,
            ..input(Some("main"), "perso")
        };
        assert_eq!(follow(&unknown), Follow::Stay);
    }

    #[test]
    fn a_move_already_tried_is_not_tried_again() {
        // Core started the pane on main although perso was asked: no loop.
        let tried = FollowInput {
            tried: Some("perso"),
            ..input(Some("main"), "perso")
        };
        assert_eq!(follow(&tried), Follow::Stay);
        // Two switches in a row: only the last account counts.
        let again = FollowInput {
            tried: Some("perso"),
            ..input(Some("main"), "pro")
        };
        assert_eq!(follow(&again), Follow::Now);
    }
}
