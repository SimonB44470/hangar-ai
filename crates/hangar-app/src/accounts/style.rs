//! How an account is marked: what the person chose for it in Settings ›
//! Accounts, a tone of the theme and an icon, drawn the same wherever the
//! account is named (a navigator row, a pane header, the toolbar's button
//! and menu, the plan block). With several subscriptions side by side, a
//! colour tells them apart at a glance where a grey word has to be read.
//!
//! The tones are the theme's own (`Tone`), never a colour of the person's:
//! each one is held to its contrast on every theme, light or dark, as the
//! other badges are. Without a choice an account keeps the plain label it
//! always had.

use hangar_theme::{Theme, fonts, style, tokens};
use hangar_ui::{Icon, icon};
use iced::widget::text::Wrapping;
use iced::widget::{container, row, text};
use iced::{Alignment, Color, Element, Padding};

use crate::ctx::Ctx;
use crate::message::Message;
use crate::state::{AccountStyle, AccountTone};

/// The icons an account can wear, in the order the settings offer them: a
/// few that say what a subscription is for, not the whole set.
pub const ICONS: [Icon; 10] = [
    Icon::Bolt,
    Icon::Workspace,
    Icon::Folder,
    Icon::Gamepad,
    Icon::Cube,
    Icon::Flag,
    Icon::Bookmark,
    Icon::Terminal,
    Icon::Moon,
    Icon::Lock,
];

/// The style chosen for account `id`, if any, and only while coloured
/// badges are on: off, every account keeps its plain label.
pub fn of<'a>(ctx: Ctx<'a>, id: &str) -> Option<&'a AccountStyle> {
    let settings = ctx.model.settings();
    if !settings.account_badges {
        return None;
    }
    settings.account_styles.get(id)
}

/// The theme's tone a choice names.
pub fn tone(tone: AccountTone) -> style::Tone {
    match tone {
        AccountTone::Accent => style::Tone::Accent,
        AccountTone::Success => style::Tone::Success,
        AccountTone::Warning => style::Tone::Warning,
        AccountTone::Danger => style::Tone::Danger,
    }
}

/// The icon a style names; a name this version does not know is no icon.
pub fn icon_of(style: Option<&AccountStyle>) -> Option<Icon> {
    style?.icon.as_deref().and_then(Icon::from_name)
}

/// The colour an account's words and icon take: its tone's, or `plain`.
pub fn ink(t: &Theme, style: Option<&AccountStyle>, plain: Color) -> Color {
    match style.and_then(|style| style.tone) {
        Some(chosen) => style::container::tone_text(t, tone(chosen)),
        None => plain,
    }
}

/// An account's name as a label of `size`: in a capsule of its tone, with its
/// icon, when it has a style; the plain word in `plain` otherwise. It never
/// wraps: a container around it decides how wide it may grow.
pub fn label<'a>(
    t: &'a Theme,
    style: Option<&AccountStyle>,
    name: String,
    size: f32,
    plain: Color,
) -> Element<'a, Message> {
    let color = ink(t, style, plain);
    let words = text(name).size(size).wrapping(Wrapping::None).color(color);
    let tone = style.and_then(|style| style.tone);
    let words = if tone.is_some() {
        words.font(fonts::ui_medium())
    } else {
        words
    };
    let mut content = row![].spacing(tokens::space::XS).align_y(Alignment::Center);
    if let Some(glyph) = icon_of(style) {
        content = content.push(icon(glyph, (size + 1.0).round()).color(color));
    }
    content = content.push(words);
    match tone {
        Some(chosen) => container(content)
            .padding(Padding::from([1.0, tokens::space::SM]))
            .align_y(Alignment::Center)
            .style(style::container::badge(t, self::tone(chosen)))
            .into(),
        None => content.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_tone_is_one_of_the_themes() {
        let tones: Vec<_> = AccountTone::ALL.into_iter().map(tone).collect();
        assert_eq!(
            tones,
            [
                style::Tone::Accent,
                style::Tone::Success,
                style::Tone::Warning,
                style::Tone::Danger
            ]
        );
    }

    #[test]
    fn an_icon_is_read_by_its_name_and_an_unknown_one_is_none() {
        let named = |name: &str| AccountStyle {
            icon: Some(name.into()),
            ..AccountStyle::default()
        };
        assert_eq!(icon_of(Some(&named("gamepad"))), Some(Icon::Gamepad));
        assert_eq!(icon_of(Some(&named("unicorn"))), None);
        assert_eq!(icon_of(None), None);
        // Each icon offered is stored under a name that reads back.
        for glyph in ICONS {
            assert_eq!(Icon::from_name(glyph.name()), Some(glyph));
        }
    }
}
