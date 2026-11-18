//! Badges and counters: small marks that say how many, or what kind.
//!
//! ```ignore
//! use hangar_ui::badge::{self, Tone};
//!
//! badge::badge(t, "new", Tone::Accent)
//! badge::badge(t, "2 conflicts", Tone::Danger)
//! badge::named(t, "Perso", Tone::Success).icon(Icon::Gamepad) // a name told apart at a glance
//! badge::count(t, 12)               // the quiet mono number of a folder's size
//! badge::tag(t, "custom")           // a word set on a surface
//! ```

use hangar_theme::tokens::{font_size, space};
use hangar_theme::{Theme, fonts, style};
use iced::widget::text::Wrapping;
use iced::widget::{container, row, text};
use iced::{Alignment, Element, Padding};

use crate::icon::{Icon, icon};

pub use hangar_theme::style::Tone;

/// A word or a number in a capsule tinted for what it says (`.chip`).
pub fn badge<'a, M: 'a>(t: &'a Theme, label: impl Into<String>, tone: Tone) -> Element<'a, M> {
    let ink = style::container::tone_text(t, tone);
    capsule(
        t,
        text(label.into())
            .size(font_size::MICRO)
            .font(fonts::mono())
            .style(move |_: &iced::Theme| text::Style { color: Some(ink) }),
        tone,
    )
}

/// A name in the capsule of [`badge`], with an icon before it when it has
/// one: what something is called rather than what kind or how many (a Claude
/// account), so in the interface's face rather than the mono one, and at the
/// size of the words it stands among. The icon and the name are both in the
/// tone's ink, and the name never wraps: what holds the badge decides how
/// wide it may grow.
pub fn named<'a>(t: &'a Theme, label: impl Into<String>, tone: Tone) -> Named<'a> {
    Named {
        t,
        label: label.into(),
        tone,
        icon: None,
        size: font_size::MICRO,
    }
}

/// The badge [`named`] makes.
#[derive(Debug, Clone)]
#[must_use]
pub struct Named<'a> {
    t: &'a Theme,
    label: String,
    tone: Tone,
    icon: Option<Icon>,
    size: f32,
}

impl Named<'_> {
    /// An icon before the name, or none.
    pub fn icon(mut self, lead: impl Into<Option<Icon>>) -> Self {
        self.icon = lead.into();
        self
    }

    /// The size of the name, [`font_size::MICRO`] unless told; the icon
    /// follows it.
    pub fn size(mut self, size: f32) -> Self {
        self.size = size;
        self
    }
}

impl<'a, M: 'a> From<Named<'a>> for Element<'a, M> {
    fn from(named: Named<'a>) -> Self {
        let ink = style::container::tone_text(named.t, named.tone);
        let mut content = row![].spacing(space::XS).align_y(Alignment::Center);
        if let Some(lead) = named.icon {
            content = content.push(icon(lead, icon_side(named.size)).color(ink));
        }
        content = content.push(
            text(named.label)
                .size(named.size)
                .font(fonts::ui_medium())
                .wrapping(Wrapping::None)
                .color(ink),
        );
        capsule(named.t, content, named.tone)
    }
}

/// The side of an icon set before words of `size`, on whole pixels: a little
/// more than the words, as tall as their capitals with what hangs below.
pub fn icon_side(size: f32) -> f32 {
    (size + 1.0).round()
}

/// The capsule every badge is drawn in: its tone's tint, rounded.
fn capsule<'a, M: 'a>(
    t: &'a Theme,
    content: impl Into<Element<'a, M>>,
    tone: Tone,
) -> Element<'a, M> {
    container(content)
        .padding(Padding::from([1.0, space::SM]))
        .align_y(Alignment::Center)
        .style(style::container::badge(t, tone))
        .into()
}

/// A number in the quietest text, in the mono face: how many a folder holds
/// (`.nav-count`).
pub fn count<'a, M: 'a>(t: &'a Theme, n: usize) -> Element<'a, M> {
    text(n.to_string())
        .size(font_size::LABEL)
        .font(fonts::mono())
        .style(style::text::subtle(t))
        .into()
}

/// A small word on a surface of its own (`.key-row__tag`): "custom".
pub fn tag<'a, M: 'a>(t: &'a Theme, label: impl Into<String>) -> Element<'a, M> {
    container(
        text(label.into())
            .size(font_size::LABEL)
            .font(fonts::ui_medium()),
    )
    .padding(Padding::from([0.0, space::SM]))
    .style(style::container::tag(t))
    .into()
}

#[cfg(test)]
mod tests {
    use hangar_theme::default_theme;

    use super::*;
    use crate::testing::simulate;

    /// Where the name of `badge` starts.
    fn name_x(badge: Named<'_>) -> f32 {
        let element: Element<'_, ()> = badge.into();
        let mut ui = simulate(element);
        ui.find("Perso").expect("the name").bounds().x
    }

    /// The icon goes before the name, and takes its side and the gap after
    /// it: the name of a badge with one starts that much further right.
    #[test]
    fn a_named_badge_puts_its_icon_before_the_name() {
        let t = default_theme();
        let plain = name_x(named(t, "Perso", Tone::Success));
        let marked = name_x(named(t, "Perso", Tone::Success).icon(Icon::Gamepad));
        let shift = icon_side(font_size::MICRO) + space::XS;
        assert!(
            (marked - plain - shift).abs() < 0.5,
            "the name moved {} for an icon of {shift}",
            marked - plain
        );
    }
}
