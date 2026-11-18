//! The account choice of the workspace dialog (`WorkspaceAccountPicker.tsx`):
//! « Default (Perso) » or one of the accounts. It shows the account's label
//! only, never its email. For a workspace filed in a folder that names an
//! account, « Folder default (Pro) » comes first and stands for no choice of
//! its own; « Default (Perso) » is then a choice, the person's default over
//! the folder's (`DEFAULT_CHOICE`).
//!
//! When the value names an account that no longer exists, a plain note says
//! the default applies; picking anything replaces the dead id.

use std::fmt;

use hangar_i18n::{Key, args};
use hangar_ui::{input, text};
use iced::Element;
use iced::widget::{Space, column};

use crate::ctx::Ctx;
use crate::message::Message;
use crate::workspace_dialog;

use super::live;
use super::model::{ClaudeAccount, DEFAULT_CHOICE, read_account_choice};
use super::switch::{account_name, default_label};

/// One line of the select: the account it picks (`None`: no choice of its
/// own) and what it says.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Choice {
    id: Option<String>,
    label: String,
}

impl fmt::Display for Choice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.label)
    }
}

/// `value` is the workspace's own choice (`None`: none), `folder` the
/// account it would read from its folder, if that names one.
pub fn view<'a>(
    ctx: Ctx<'a>,
    value: Option<&'a str>,
    folder: Option<&'a str>,
) -> Element<'a, Message> {
    let Some(accounts) = live(ctx).accounts.as_deref() else {
        return Space::new().width(0).height(0).into();
    };
    let options = choices(ctx, accounts, folder);
    let chosen = read_account_choice(value);
    let missing = chosen
        .clone()
        .filter(|id| id != DEFAULT_CHOICE && !accounts.iter().any(|account| account.id == *id));
    let selected = selected(&options, chosen.as_deref().filter(|_| missing.is_none()));

    let select = input::select(ctx.theme, options, selected, |choice: Choice| {
        Message::WorkspaceDialog(workspace_dialog::Msg::Account(choice.id))
    });
    let mut field = column![select].spacing(4);
    if let Some(id) = missing {
        field = field.push(text::hint(
            ctx.theme,
            ctx.i18n
                .t_with(Key::AccountsPickerMissing, &args!("id" => id.as_str()))
                .into_owned(),
        ));
    }
    field.into()
}

/// The lines of the select: « Folder default (Pro) » when `folder` names an
/// account that exists, then « Default (Perso) », then each account.
fn choices(ctx: Ctx<'_>, accounts: &[ClaudeAccount], folder: Option<&str>) -> Vec<Choice> {
    let unknown = ctx.t(Key::AccountsPickerUnknown);
    let folder = folder.filter(|id| accounts.iter().any(|account| account.id == *id));
    let default = default_label(
        accounts,
        ctx.model.settings().claude_account.as_deref(),
        &ctx.t(Key::AccountsPickerDefault),
        &unknown,
    );
    let mut options = Vec::new();
    match folder {
        Some(folder) => {
            let name = account_name(accounts, Some(folder), &unknown);
            options.push(Choice {
                id: None,
                label: ctx
                    .i18n
                    .t_with(
                        Key::AccountsPickerFolderDefault,
                        &args!("name" => name.as_str()),
                    )
                    .into_owned(),
            });
            options.push(Choice {
                id: Some(DEFAULT_CHOICE.to_string()),
                label: default,
            });
        }
        None => options.push(Choice {
            id: None,
            label: default,
        }),
    }
    options.extend(accounts.iter().map(|account| Choice {
        id: Some(account.id.clone()),
        label: account.label.clone(),
    }));
    options
}

/// The line `chosen` picks. A dead id selects nothing of its own: the first
/// line, what the workspace runs on without a choice, stands for it, as it
/// does for « Default » chosen outside a folder that names an account.
fn selected(options: &[Choice], chosen: Option<&str>) -> Option<Choice> {
    chosen
        .and_then(|id| {
            options
                .iter()
                .find(|option| option.id.as_deref() == Some(id))
        })
        .or_else(|| options.first())
        .cloned()
}

#[cfg(test)]
mod tests {
    use hangar_i18n::Locale;

    use super::*;
    use crate::workspace_dialog::testing::Harness;

    fn shown(options: &[Choice]) -> Vec<(&str, Option<&str>)> {
        options
            .iter()
            .map(|choice| (choice.label.as_str(), choice.id.as_deref()))
            .collect()
    }

    #[test]
    fn in_a_folder_that_names_an_account_its_line_comes_first_and_default_is_a_choice() {
        let h = Harness::new(Locale::En, "graphite");
        let accounts = live(h.ctx()).accounts.clone().unwrap_or_default();
        let options = choices(h.ctx(), &accounts, Some("perso"));
        assert_eq!(
            shown(&options),
            [
                ("Folder default (Perso)", None),
                ("Default (Principal)", Some(DEFAULT_CHOICE)),
                ("Principal", Some("main")),
                ("Perso", Some("perso")),
                ("Travail", Some("pro")),
            ]
        );
        let label = |chosen| selected(&options, chosen).map(|choice| choice.label);
        assert_eq!(label(None).as_deref(), Some("Folder default (Perso)"));
        assert_eq!(
            label(Some(DEFAULT_CHOICE)).as_deref(),
            Some("Default (Principal)")
        );
        assert_eq!(label(Some("pro")).as_deref(), Some("Travail"));
    }

    #[test]
    fn elsewhere_default_stands_for_no_choice_and_for_the_default_chosen() {
        let h = Harness::new(Locale::En, "graphite");
        let accounts = live(h.ctx()).accounts.clone().unwrap_or_default();
        // No folder, or one naming an account that is gone.
        for folder in [None, Some("gone")] {
            let options = choices(h.ctx(), &accounts, folder);
            assert_eq!(shown(&options)[0], ("Default (Principal)", None));
            assert_eq!(options.len(), accounts.len() + 1);
            let label = |chosen| selected(&options, chosen).map(|choice| choice.label);
            assert_eq!(
                label(Some(DEFAULT_CHOICE)).as_deref(),
                Some("Default (Principal)")
            );
        }
    }
}
