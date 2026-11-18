//! How an account is marked: what the person chose for it in Settings ›
//! Accounts, a tone of the theme and an icon, drawn the same wherever the
//! account is named (a navigator row, a pane header, the toolbar's button
//! and its menu). With several subscriptions side by side, a colour tells
//! them apart at a glance where a grey word has to be read.
//!
//! The tones are the theme's own (`Tone`), never a colour of the person's:
//! each one is held to its contrast on every theme, light or dark, as the
//! other badges are. Without a choice, with coloured badges off, or with a
//! single account (nothing to tell apart), an account keeps the plain label
//! it always had.

use hangar_i18n::Key;
use hangar_theme::{Theme, style, tokens};
use hangar_ui::badge::{self, Tone};
use hangar_ui::{Icon, icon};
use iced::widget::text::Wrapping;
use iced::widget::{row, text};
use iced::{Alignment, Color, Element};

use crate::ctx::Ctx;
use crate::message::Message;
use crate::state::{AccountStyle, AccountTone};

use super::live;

/// The icons an account can wear, in the order the settings offer them, each
/// with the name its button's tooltip gives: a few that say what a
/// subscription is for, not the whole set.
pub const ICONS: [(Icon, Key); 10] = [
    (Icon::Bolt, Key::AccountsSettingsIconBolt),
    (Icon::Workspace, Key::AccountsSettingsIconWorkspace),
    (Icon::Folder, Key::AccountsSettingsIconFolder),
    (Icon::Gamepad, Key::AccountsSettingsIconGamepad),
    (Icon::Cube, Key::AccountsSettingsIconCube),
    (Icon::Flag, Key::AccountsSettingsIconFlag),
    (Icon::Bookmark, Key::AccountsSettingsIconBookmark),
    (Icon::Terminal, Key::AccountsSettingsIconTerminal),
    (Icon::Moon, Key::AccountsSettingsIconMoon),
    (Icon::Lock, Key::AccountsSettingsIconLock),
];

/// What an account's label is drawn with: the tone and the icon this version
/// can draw of the style chosen for it. Plain is neither.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Mark {
    pub tone: Option<AccountTone>,
    pub icon: Option<Icon>,
}

impl Mark {
    /// The mark of `chosen`; an icon this version does not know is none.
    pub fn of_style(chosen: &AccountStyle) -> Mark {
        Mark {
            tone: chosen.tone,
            icon: chosen.icon.as_deref().and_then(Icon::from_name),
        }
    }

    /// Nothing to draw but the name.
    pub fn is_plain(self) -> bool {
        self.tone.is_none() && self.icon.is_none()
    }
}

/// Whether accounts wear their marks: coloured badges are on, and there are
/// at least two accounts to tell apart. With a single one, Settings ›
/// Accounts does not offer the switch, so nothing of it may show either.
pub fn shown(ctx: Ctx<'_>) -> bool {
    ctx.model.settings().account_badges
        && live(ctx)
            .accounts
            .as_ref()
            .is_some_and(|all| all.len() >= 2)
}

/// The mark of account `id`: what was chosen for it while marks are
/// `shown`, plain otherwise. A choice made stays in the settings while they
/// are not shown, for when they are again.
pub fn of(ctx: Ctx<'_>, id: &str) -> Mark {
    if !shown(ctx) {
        return Mark::default();
    }
    Mark::of_style(&ctx.model.settings().account_style(id))
}

/// The theme's tone a choice names.
pub fn tone(chosen: AccountTone) -> Tone {
    match chosen {
        AccountTone::Accent => Tone::Accent,
        AccountTone::Success => Tone::Success,
        AccountTone::Warning => Tone::Warning,
        AccountTone::Danger => Tone::Danger,
    }
}

/// The colour a tone's words and icon take, on the theme's tint of it.
pub fn ink(t: &Theme, chosen: AccountTone) -> Color {
    style::container::tone_text(t, tone(chosen))
}

/// An account's name as a label of `size`, as `mark` says: in a badge of its
/// tone, with its icon, when it has a tone; else the plain word, after its
/// icon if it has one, in `plain` (or in the colour around it, `None`). It
/// never wraps: what holds it decides how wide it may grow.
pub fn label<'a>(
    t: &'a Theme,
    mark: Mark,
    name: String,
    size: f32,
    plain: Option<Color>,
) -> Element<'a, Message> {
    if let Some(chosen) = mark.tone {
        return badge::named(t, name, tone(chosen))
            .icon(mark.icon)
            .size(size)
            .into();
    }
    let words = text(name)
        .size(size)
        .wrapping(Wrapping::None)
        .color_maybe(plain);
    let Some(glyph) = mark.icon else {
        return words.into();
    };
    let glyph = icon(glyph, badge::icon_side(size));
    let glyph = match plain {
        Some(color) => glyph.color(color),
        None => glyph,
    };
    row![glyph, words]
        .spacing(tokens::space::XS)
        .align_y(Alignment::Center)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_tone_is_one_of_the_themes() {
        let tones: Vec<_> = AccountTone::ALL.into_iter().map(tone).collect();
        assert_eq!(
            tones,
            [Tone::Accent, Tone::Success, Tone::Warning, Tone::Danger]
        );
    }

    #[test]
    fn an_icon_is_read_by_its_name_and_an_unknown_one_is_none() {
        let named = |name: &str| AccountStyle {
            icon: Some(name.into()),
            ..AccountStyle::default()
        };
        assert_eq!(Mark::of_style(&named("gamepad")).icon, Some(Icon::Gamepad));
        assert_eq!(Mark::of_style(&named("unicorn")).icon, None);
        assert!(Mark::of_style(&named("unicorn")).is_plain());
        assert!(Mark::of_style(&AccountStyle::default()).is_plain());
        // Each icon offered is stored under a name that reads back.
        for (glyph, _) in ICONS {
            assert_eq!(Icon::from_name(glyph.name()), Some(glyph));
        }
    }
}
