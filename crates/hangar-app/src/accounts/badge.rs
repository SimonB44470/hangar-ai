//! A discreet label naming the Claude account a workspace or a pane runs on
//! (`AccountBadge.tsx`, `PaneAccountBadge.tsx`, and the navigator's
//! `accountBadge`). It shows the account's label, never its email, and it is
//! a label, not a control: in the tone and with the icon chosen for the
//! account (`account_style`), or as a plain grey word.

use hangar_i18n::{Key, Translator, args};
use hangar_theme::{style, tokens};
use hangar_ui::tip;
use iced::Element;
use iced::widget::text::Wrapping;
use iced::widget::{Space, container, text};

use crate::ctx::Ctx;
use crate::message::Message;
use crate::state::types::AgentId;
use crate::state::{PaneId, WsId};

use super::model::{AccountLevels, AccountSource, ClaudeAccount};
use super::switch::account_name;
use super::{Hold, account_style, live};

/// How wide a label grows before it is cut (`max-width: 10em` at 10 px).
const MAX_WIDTH: f32 = 100.0;

fn nothing<'a>() -> Element<'a, Message> {
    Space::new().width(0).height(0).into()
}

/// With a single account there is nothing to tell apart, so nothing is drawn.
/// The label takes the style the person gave the account, if any. `folder`
/// names the navigator folder the account comes from, when it is that
/// folder's rather than a choice of its own: the tooltip says so.
fn chip<'a>(
    ctx: Ctx<'a>,
    accounts: &[ClaudeAccount],
    id: &str,
    folder: Option<&str>,
) -> Element<'a, Message> {
    if accounts.len() < 2 {
        return nothing();
    }
    let unknown = ctx.t(Key::AccountsPickerUnknown);
    let name = account_name(accounts, Some(id), &unknown);
    let title = match folder {
        Some(folder) => ctx.i18n.t_with(
            Key::AccountsBadgeFolderTitle,
            &args!("name" => name.as_str(), "folder" => folder),
        ),
        None => ctx
            .i18n
            .t_with(Key::AccountsBadgeTitle, &args!("name" => name.as_str())),
    }
    .into_owned();
    let label = container(account_style::label(
        ctx.theme,
        account_style::of(ctx, id),
        name,
        tokens::font_size::MICRO,
        Some(ctx.theme.colors.fg_secondary),
    ))
    .max_width(MAX_WIDTH);
    tip::over(ctx.theme, label, title)
}

/// « → Perso »: the pane moves to that account once Hangar knows it has
/// nothing to lose (`switch::follow`), and its tooltip says what it waits
/// for; in the danger colour when it cannot, that account not being logged
/// in.
fn moving_chip<'a>(
    ctx: Ctx<'a>,
    accounts: &[ClaudeAccount],
    moving: &super::Moving,
) -> Element<'a, Message> {
    let unknown = ctx.t(Key::AccountsPickerUnknown);
    let name = account_name(accounts, Some(&moving.to), &unknown);
    let words = if moving.blocked() {
        Key::AccountsMovingBlocked
    } else {
        Key::AccountsMoving
    };
    let label = ctx
        .i18n
        .t_with(words, &args!("name" => name.as_str()))
        .into_owned();
    let title = moving_title(ctx.i18n, &name, &moving.hold);
    let words = text(label)
        .size(tokens::font_size::MICRO)
        .wrapping(Wrapping::None);
    let words = if moving.blocked() {
        words.style(style::text::danger(ctx.theme))
    } else {
        words.style(style::text::secondary(ctx.theme))
    };
    let chip = container(words).max_width(MAX_WIDTH);
    tip::over(ctx.theme, chip, title)
}

/// Why a pane waits to move to `name`, as its header's tooltip says it.
pub(super) fn moving_title(t: Translator, name: &str, hold: &Hold) -> String {
    let key = match hold {
        Hold::NotLoggedIn => Key::AccountsMovingBlockedTitle,
        Hold::Turn => Key::AccountsMovingTurnTitle,
        Hold::Unreported => Key::AccountsMovingUnreportedTitle,
        Hold::Draft => Key::AccountsMovingDraftTitle,
        Hold::Checking => Key::AccountsMovingTitle,
        Hold::Command(_) => Key::AccountsMovingCommandTitle,
        Hold::Unreadable => Key::AccountsMovingUnreadableTitle,
    };
    let command = match hold {
        Hold::Command(what) => what.as_str(),
        _ => "",
    };
    t.t_with(key, &args!("name" => name, "command" => command))
        .into_owned()
}

/// A navigator row's badge: only a workspace on an account other than the
/// default is worth a label, its own or the one it reads from its folder.
pub fn workspace<'a>(ctx: Ctx<'a>, ws: &'a WsId) -> Element<'a, Message> {
    let Some(accounts) = live(ctx).accounts.as_deref() else {
        return nothing();
    };
    let Some(workspace) = ctx.model.workspace(ws) else {
        return nothing();
    };
    let state = ctx.model.state();
    let own = AccountLevels::of(state, workspace, None).resolve(accounts);
    let default = AccountLevels {
        default: state.settings.claude_account.clone(),
        ..AccountLevels::default()
    }
    .resolve(accounts);
    let folder = match own.source {
        AccountSource::Workspace => None,
        AccountSource::Folder => state
            .folders
            .iter()
            .find(|folder| workspace.folder_id.as_deref() == Some(folder.id.as_str()))
            .map(|folder| folder.name.as_str()),
        _ => return nothing(),
    };
    if own.id == default.id {
        return nothing();
    }
    chip(ctx, accounts, &own.id, folder)
}

/// A pane header's badge: it names the account of a pane whose agent runs on
/// another one than its workspace's (a pane-level override). Nothing
/// otherwise.
pub fn pane<'a>(ctx: Ctx<'a>, ws: &'a WsId, pane: &'a PaneId) -> Element<'a, Message> {
    let Some(accounts) = live(ctx).accounts.as_deref() else {
        return nothing();
    };
    let Some(workspace) = ctx.model.workspace(ws) else {
        return nothing();
    };
    let Some(pane) = workspace
        .panes
        .iter()
        .find(|candidate| candidate.id == *pane)
    else {
        return nothing();
    };
    if pane.agent != AgentId::Claude {
        return nothing();
    }
    // On its way to another account, or held back from it.
    if let Some(moving) = live(ctx).moving.get(&pane.id) {
        return moving_chip(ctx, accounts, moving);
    }
    let state = ctx.model.state();
    let own = AccountLevels::of(state, workspace, Some(pane)).resolve(accounts);
    if own.source != AccountSource::Pane {
        return nothing();
    }
    // What the workspace runs on: its own, its folder's or the default.
    let base = AccountLevels::of(state, workspace, None).resolve(accounts);
    if own.id == base.id {
        return nothing();
    }
    chip(ctx, accounts, &own.id, None)
}
