//! The entries of a menu that choose a Claude account
//! (`components/accounts/accountMenuItems.ts` and `PaneAccountItems.tsx`):
//! « Default (Perso) » first, then one entry per account, the current choice
//! checked. They sit at the end of the navigator's workspace menu and of a
//! pane's « … » menu, which draw them as they draw their own lines. A
//! workspace filed in a folder that names an account gets « Folder default
//! (Pro) » first: what it runs on without a choice of its own. A navigator
//! folder's menu has the same lines, for the account of its workspaces.

use hangar_engine::accounts::{AccountLevels, DEFAULT_CHOICE};
use hangar_i18n::{Key, Translator, args};

use crate::ctx::Ctx;
use crate::message::Message;
use crate::shell::MenuEntry;
use crate::state::folders;
use crate::state::types::AgentId;
use crate::state::{PaneId, WsId};

use super::model::{ClaudeAccount, read_account_choice};
use super::switch::{account_name, default_label};
use super::{Msg, live};

/// One line, as plain data: what it says, whether it is the current choice,
/// and what picking it chooses (`None`: follow the default).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountMenuItem {
    pub key: String,
    pub label: String,
    pub checked: bool,
    pub select: Option<String>,
}

/// `default` names the account « Default » stands for, `value` is the own
/// account of what the menu is for (a workspace, a pane); `None` when it
/// follows the default. Labels only, never emails.
pub fn account_menu_items(
    t: Translator,
    accounts: &[ClaudeAccount],
    default: Option<&str>,
    value: Option<&str>,
) -> Vec<AccountMenuItem> {
    let own = read_account_choice(value);
    let known_own = own
        .as_deref()
        .filter(|own| accounts.iter().any(|account| account.id == *own));
    let template = t.t(Key::AccountsPickerDefault);
    let unknown = t.t(Key::AccountsPickerUnknown);
    let mut items = vec![AccountMenuItem {
        key: "default".into(),
        label: default_label(accounts, default, &template, &unknown),
        checked: known_own.is_none(),
        select: None,
    }];
    items.extend(accounts.iter().map(|account| AccountMenuItem {
        key: account.id.clone(),
        label: account.label.clone(),
        checked: known_own == Some(account.id.as_str()),
        select: Some(account.id.clone()),
    }));
    items
}

/// The lines of a workspace's menu. `folder` is the account the workspace
/// reads from its folder, when that folder names one that exists: « Folder
/// default (Pro) » comes first and stands for no choice of its own, and
/// « Default (Perso) » becomes a choice, the person's default over the
/// folder's (`DEFAULT_CHOICE`). Without it, [`account_menu_items`].
pub fn workspace_menu_items(
    t: Translator,
    accounts: &[ClaudeAccount],
    default: Option<&str>,
    folder: Option<&str>,
    value: Option<&str>,
) -> Vec<AccountMenuItem> {
    let mut items = account_menu_items(t, accounts, default, value);
    let Some(folder) = folder else {
        return items;
    };
    let own = read_account_choice(value);
    let chose_default = own.as_deref() == Some(DEFAULT_CHOICE);
    let known_own = own.filter(|own| accounts.iter().any(|account| account.id == *own));
    if let Some(chosen) = items.first_mut() {
        chosen.checked = chose_default;
        chosen.select = Some(DEFAULT_CHOICE.to_string());
    }
    let unknown = t.t(Key::AccountsPickerUnknown);
    let name = account_name(accounts, Some(folder), &unknown);
    items.insert(
        0,
        AccountMenuItem {
            key: "folder".into(),
            label: t
                .t_with(
                    Key::AccountsPickerFolderDefault,
                    &args!("name" => name.as_str()),
                )
                .into_owned(),
            checked: known_own.is_none() && !chose_default,
            select: None,
        },
    );
    items
}

fn radio(item: AccountMenuItem, pick: impl Fn(Option<String>) -> Message) -> MenuEntry {
    MenuEntry::Item {
        label: item.label,
        icon: None,
        danger: false,
        checked: Some(item.checked),
        disabled: None,
        on_select: pick(item.select),
    }
}

/// The account lines of a navigator folder's menu: « Default (Perso) » first
/// (its workspaces follow the default), then one per account, the folder's
/// choice checked. The workspaces filed in it that chose no account of their
/// own run on it, those moved in later too. Nothing before the accounts are
/// known, nor with a single account, where there is nothing to choose.
pub fn folder_entries(ctx: Ctx<'_>, folder: &str) -> Vec<MenuEntry> {
    let Some(accounts) = live(ctx).accounts.as_deref().filter(|all| all.len() > 1) else {
        return Vec::new();
    };
    let Some(entry) = ctx
        .model
        .state()
        .folders
        .iter()
        .find(|entry| entry.id == folder)
    else {
        return Vec::new();
    };
    let items = account_menu_items(
        ctx.i18n,
        accounts,
        ctx.model.settings().claude_account.as_deref(),
        folders::folder_account(entry),
    );
    let mut entries = vec![
        MenuEntry::Separator,
        MenuEntry::Heading(ctx.t(Key::AccountsMenuFolderTitle).into_owned()),
    ];
    entries.extend(items.into_iter().map(|item| {
        radio(item, |id| {
            Message::Accounts(Msg::SetFolder {
                folder: folder.to_string(),
                id,
            })
        })
    }));
    entries
}

/// The account lines of the navigator's menu (`pane` `None`: the workspace's
/// own choice) or of a pane's menu (that pane's own choice, over its
/// workspace's). Nothing before the accounts are known, and nothing for a
/// pane that does not run Claude Code, or that was opened to log in.
pub fn menu_entries(ctx: Ctx<'_>, ws: &WsId, pane: Option<&PaneId>) -> Vec<MenuEntry> {
    let Some(accounts) = live(ctx).accounts.as_deref() else {
        return Vec::new();
    };
    let Some(workspace) = ctx.model.workspace(ws) else {
        return Vec::new();
    };
    let settings = ctx.model.settings().claude_account.as_deref();
    match pane {
        None => {
            // The same as the toolbar's click: this workspace's own account,
            // or its folder's.
            let folder = folders::account_of(&ctx.model.state().folders, workspace)
                .filter(|id| accounts.iter().any(|account| account.id == *id));
            let items = workspace_menu_items(
                ctx.i18n,
                accounts,
                settings,
                folder,
                workspace.claude_account.as_deref(),
            );
            let mut entries = vec![
                MenuEntry::Separator,
                MenuEntry::Heading(ctx.t(Key::AccountsMenuTitle).into_owned()),
            ];
            entries.extend(items.into_iter().map(|item| {
                radio(item, |id| {
                    Message::Accounts(Msg::SetWorkspace {
                        ws: ws.clone(),
                        id,
                        restart: false,
                    })
                })
            }));
            entries
        }
        Some(pane_id) => {
            let Some(pane) = workspace.panes.iter().find(|pane| pane.id == *pane_id) else {
                return Vec::new();
            };
            if pane.agent != AgentId::Claude || pane.login == Some(true) {
                return Vec::new();
            }
            // « Default » here means the pane follows its workspace, so the
            // label names the account the workspace is on: its own, its
            // folder's or the default.
            let follows = AccountLevels::of(ctx.model.state(), workspace, None)
                .resolve(accounts)
                .id;
            let items = account_menu_items(
                ctx.i18n,
                accounts,
                Some(&follows),
                pane.claude_account.as_deref(),
            );
            let mut entries = vec![
                MenuEntry::Separator,
                MenuEntry::Heading(ctx.t(Key::AccountsPaneMenuHeading).into_owned()),
            ];
            entries.extend(items.into_iter().map(|item| {
                radio(item, |id| {
                    Message::Accounts(Msg::SetPane {
                        ws: ws.clone(),
                        pane: pane_id.clone(),
                        id,
                    })
                })
            }));
            entries
        }
    }
}

#[cfg(test)]
mod tests {
    use hangar_i18n::Locale;

    use super::*;

    fn accounts() -> Vec<ClaudeAccount> {
        vec![
            ClaudeAccount {
                id: "main".into(),
                label: "Principal".into(),
                is_main: true,
                logged_in: true,
                email: None,
                plan: None,
            },
            ClaudeAccount {
                id: "perso".into(),
                label: "Perso".into(),
                is_main: false,
                logged_in: true,
                email: Some("me@example.com".into()),
                plan: None,
            },
        ]
    }

    const T: Translator = Translator::new(Locale::En);

    #[test]
    fn lists_the_default_first_and_checks_it_when_the_workspace_has_no_account() {
        let items = account_menu_items(T, &accounts(), Some("perso"), None);
        let shown: Vec<_> = items
            .iter()
            .map(|item| (item.label.as_str(), item.checked))
            .collect();
        assert_eq!(
            shown,
            [
                ("Default (Perso)", true),
                ("Principal", false),
                ("Perso", false)
            ]
        );
    }

    #[test]
    fn checks_the_workspaces_own_account_and_reports_the_choice() {
        let items = account_menu_items(T, &accounts(), None, Some("perso"));
        let checked: Vec<_> = items.iter().map(|item| item.checked).collect();
        assert_eq!(checked, [false, false, true]);
        let picked: Vec<_> = items
            .iter()
            .take(2)
            .map(|item| item.select.clone())
            .collect();
        assert_eq!(picked, [None, Some("main".to_string())]);
    }

    #[test]
    fn treats_a_dead_id_as_the_default_and_never_prints_an_email() {
        let items = account_menu_items(T, &accounts(), None, Some("gone"));
        assert!(items[0].checked);
        assert!(items.iter().all(|item| !item.label.contains('@')));
    }

    fn shown(items: &[AccountMenuItem]) -> Vec<(&str, bool, Option<&str>)> {
        items
            .iter()
            .map(|item| (item.label.as_str(), item.checked, item.select.as_deref()))
            .collect()
    }

    #[test]
    fn in_a_folder_with_an_account_the_folders_line_is_no_choice_and_default_one() {
        let items = workspace_menu_items(T, &accounts(), Some("main"), Some("perso"), None);
        assert_eq!(
            shown(&items),
            [
                ("Folder default (Perso)", true, None),
                ("Default (Principal)", false, Some(DEFAULT_CHOICE)),
                ("Principal", false, Some("main")),
                ("Perso", false, Some("perso")),
            ]
        );
        let checked = |value| -> Vec<bool> {
            workspace_menu_items(T, &accounts(), Some("main"), Some("perso"), value)
                .iter()
                .map(|item| item.checked)
                .collect()
        };
        assert_eq!(checked(Some(DEFAULT_CHOICE)), [false, true, false, false]);
        assert_eq!(checked(Some("perso")), [false, false, false, true]);
        // An account that is gone: the folder's applies.
        assert_eq!(checked(Some("gone")), [true, false, false, false]);
    }

    #[test]
    fn outside_such_a_folder_the_workspaces_lines_are_the_usual_ones() {
        assert_eq!(
            workspace_menu_items(T, &accounts(), Some("perso"), None, None),
            account_menu_items(T, &accounts(), Some("perso"), None)
        );
    }

    #[test]
    fn says_the_same_in_french() {
        let items = account_menu_items(
            Translator::new(Locale::Fr),
            &accounts(),
            Some("perso"),
            None,
        );
        assert_eq!(items[0].label, "Par défaut (Perso)");
    }
}
