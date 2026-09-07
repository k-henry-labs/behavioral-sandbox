//! The screens: the menu at the door, the notebook's list, one run, and the form for a new one.
//!
//! - **The posture is the layout.** A row shows what a run could touch before its name is read
//!   twice; a run's pane spells it out; the form's sentence is `Posture::sentence`, generated
//!   from the fields, so starting is confirming what the record will say (rule 3 as a screen).
//! - **Nothing here is a verb.** Every button becomes a `bsx` call or a file read; the CLI does
//!   the same thing with the same words.

use iced::widget::{
    button, checkbox, column, container, mouse_area, row, rule, scrollable, shader, slider, space,
    text, text_input, toggler,
};
use iced::{Element, Fill, Font, Length};

use bsx_record::{Record, Verb};

use crate::{App, Field, Form, Message, Stream, Switch, cli, icons};

/// Identifiers, and only identifiers: a name, a command, a path, an id. Prose is the system's
/// own sans, so a row reads as a sentence rather than a terminal dump.
const MONO: Font = Font::MONOSPACE;

/// The name of a run: the one thing a reader is scanning for down a column.
const NAME: Font = Font {
    weight: iced::font::Weight::Semibold,
    ..Font::MONOSPACE
};

/// A pane's own name, at the head of the screen the sidebar opened.
const HEAD: f32 = 17.0;

/// The system's sans in the weight a name is set in.
const HEADING: Font = Font {
    weight: iced::font::Weight::Semibold,
    ..Font::DEFAULT
};

/// The type scale. Three sizes, so a card has a first, second and third thing to read.
const TITLE: f32 = 15.0;
pub(crate) const BODY: f32 = 13.0;
const SMALL: f32 = 12.0;

/// How a run ended, as a colour: running, ended cleanly, or ended badly. The dot and the state
/// share it, so the two cannot disagree.
fn status_colour(theme: &iced::Theme, record: &Record, live: bool) -> iced::Color {
    let palette = theme.extended_palette();
    if live {
        return palette.success.base.color;
    }
    match record.end {
        Some(bsx_record::End::Exit(0)) => palette.background.strong.text,
        Some(bsx_record::End::Exit(_) | bsx_record::End::Signal(_) | bsx_record::End::Failed) => {
            palette.danger.base.color
        }
        _ => palette.background.strong.text,
    }
}

/// Muted text: everything that is not the name or the command.
fn muted(theme: &iced::Theme) -> iced::Color {
    theme.extended_palette().background.strong.text
}

/// The scroll rail as macOS draws it: no rail, a translucent pill in a lane of its own.
fn scroll(theme: &iced::Theme, status: scrollable::Status) -> scrollable::Style {
    let mut style = scrollable::default(theme, status);
    let pill = theme
        .extended_palette()
        .background
        .base
        .text
        .scale_alpha(0.25);
    for rail in [&mut style.vertical_rail, &mut style.horizontal_rail] {
        rail.background = None;
        rail.border = iced::Border::default();
        rail.scroller.background = iced::Background::Color(pill);
        rail.scroller.border = iced::Border {
            radius: 3.0.into(),
            ..iced::Border::default()
        };
    }
    style
}

/// A scrollbar embedded beside the content, so it can never sit over what it scrolls.
fn lane() -> scrollable::Direction {
    scrollable::Direction::Vertical(
        scrollable::Scrollbar::new()
            .width(6)
            .scroller_width(6)
            .spacing(12),
    )
}

/// The window's own furniture: the sidebar on the left, the open screen beside it.
pub(crate) fn chrome<'a>(app: &'a App, content: Element<'a, Message>) -> Element<'a, Message> {
    let out = app.sidebar_out();
    let mut panes = row![];
    if out > 0.0 {
        panes = panes
            .push(sidebar(app, SIDEBAR * out))
            .push(rule::vertical(1).style(divider));
    }
    // The toggle rides over both panes rather than inside either, so that one glyph crosses the
    // window as the sidebar folds instead of two swapping places.
    iced::widget::stack![
        panes.push(content),
        container(sidebar_toggle()).padding(iced::Padding {
            top: TOGGLE_TOP - HALO_OVERHANG,
            right: 0.0,
            bottom: 0.0,
            left: toggle_at(out) - HALO_OVERHANG,
        }),
    ]
    // `push_under`, not a first layer: a stack takes its size from the layer it was built on, and
    // this one is 52 tall. Under everything, so a control on that line answers a click first and
    // only what nothing else wanted reaches the window's own line, which a double click zooms.
    .push_under(
        mouse_area(space().width(Fill).height(HEAD_BAR)).on_double_click(Message::ZoomWindow),
    )
    .into()
}

/// The sidebar at `width`: where this machine's sandboxes are reached, clipped to what the fold
/// has left it rather than laid out again at every width.
fn sidebar(app: &App, width: f32) -> Element<'_, Message> {
    let running = app.runs.iter().filter(|r| app.is_live(r)).count();
    let on_list = matches!(app.screen, crate::Screen::List | crate::Screen::Run(_));
    let nav = column![
        tab(
            icons::GRID,
            "Sandboxes",
            (running > 0).then(|| running.to_string()),
            on_list,
            Message::List,
        ),
        tab(
            icons::SQUARE_PLUS,
            "New run",
            None,
            app.screen == crate::Screen::New,
            Message::NewRun
        ),
        tab(
            icons::SETTINGS,
            "Settings",
            None,
            app.screen == crate::Screen::Settings,
            Message::Settings,
        ),
    ]
    .spacing(3);
    container(nav.height(Fill))
        .style(rail)
        .width(Length::Fixed(width))
        .height(Fill)
        .clip(true)
        .padding(iced::Padding {
            top: NAV_TOP,
            right: RAIL_PAD,
            bottom: 12.0,
            left: RAIL_PAD,
        })
        .into()
}

/// The button that folds the sidebar and brings it back, wearing the glyph macOS gives it.
fn sidebar_toggle<'a>() -> Element<'a, Message> {
    button(
        icons::glyph(icons::PANEL_LEFT, TOGGLE_ICON)
            .center()
            .width(HALO),
    )
    .style(round)
    .padding(0)
    .height(HALO)
    .on_press(Message::ToggleSidebar)
    .into()
}

/// The room the toggle takes in the head's line: what the lights' room and the head's inset
/// are measured against.
const TOGGLE: f32 = 28.0;

/// The circle the toggle wears under the pointer, drawn on the same centre as its room and
/// past it on every side by [`HALO_OVERHANG`], as a toolbar icon's halo is wider than the icon.
const HALO: f32 = 36.0;
const HALO_OVERHANG: f32 = (HALO - TOGGLE) / 2.0;

/// The toggle's glyph, drawn larger than a row's: the platform sets this one bigger than the
/// icons beside a label, and it is the only icon standing on its own.
const TOGGLE_ICON: f32 = 20.0;

/// An icon on its own: nothing until the pointer finds it, then the circle a toolbar icon wears,
/// a step past a row's hover so it reads on the rail as well as on the page.
fn round(theme: &iced::Theme, status: button::Status) -> button::Style {
    let palette = theme.extended_palette();
    let surface = match status {
        button::Status::Hovered | button::Status::Pressed => crate::theme::selected(theme),
        button::Status::Active | button::Status::Disabled => iced::Color::TRANSPARENT,
    };
    let mut style = role(surface, palette.background.base.text, None, status);
    style.border.radius = (HALO / 2.0).into();
    style
}

/// The room the traffic lights take at the window's top-left corner, before the first control.
const LIGHTS: f32 = 91.0;

/// A head's height, as a toolbar window has one: 19.5 of room over its content, which is what a
/// window whose titlebar carries a toolbar leaves over the traffic lights in it.
const HEAD_BAR: f32 = 52.0;

/// Where the toggle's circle starts, so its glyph is centred on that same line.
const TOGGLE_TOP: f32 = (HEAD_BAR - TOGGLE) / 2.0;

/// The room the sidebar keeps at its own edges, and so where the toggle rests inside it.
const RAIL_PAD: f32 = 10.0;

/// Where the sidebar's first tab starts: under the toggle and the gap after it.
const NAV_TOP: f32 = TOGGLE_TOP + TOGGLE + 18.0;

/// Where the toggle stands: over the room the lights leave when the sidebar is folded away, at
/// the sidebar's own inner edge when it is out, and along that line while it moves.
fn toggle_at(out: f32) -> f32 {
    LIGHTS + (SIDEBAR - RAIL_PAD - TOGGLE - LIGHTS) * out
}

/// Where a pane's head starts, at this much of the sidebar: at the gutter, out past the lights
/// and the toggle by as much as the sidebar is folded away.
fn head_inset_at(out: f32) -> f32 {
    GUTTER + (LIGHTS + TOGGLE) * (1.0 - out)
}

/// Where the head of the pane this window is showing starts.
fn head_inset(app: &App) -> f32 {
    head_inset_at(app.sidebar_out())
}

/// One sidebar tab: its name, an optional count, and the pill it wears while its screen is open.
fn tab<'a>(
    icon: icons::Icon,
    label: &'a str,
    count: Option<String>,
    open: bool,
    message: Message,
) -> Element<'a, Message> {
    let mut line = row![
        icons::glyph(icon, ICON),
        // One line whatever room is left: a label that rewrapped would step down the rail on
        // every frame of a fold.
        text(label).size(TAB).wrapping(text::Wrapping::None),
    ]
    .spacing(12);
    line = line.push(space().width(Fill));
    if let Some(count) = count {
        line = line.push(text(count).size(SMALL).style(|t| text::Style {
            color: Some(muted(t)),
        }));
    }
    button(line.align_y(iced::alignment::Vertical::Center))
        .style(move |t, s| if open { selected_tab(t) } else { ghost(t, s) })
        .width(Fill)
        .padding([9, 12])
        .on_press(message)
        .into()
}

/// The pill under the open screen's tab: the rail a step darker, flat, as a source list marks
/// its row.
fn selected_tab(theme: &iced::Theme) -> button::Style {
    let palette = theme.extended_palette();
    role(
        crate::theme::selected(theme),
        palette.background.base.text,
        None,
        button::Status::Active,
    )
}

/// The sidebar's surface: the page tinted one step, which the rule beside it parts from the page.
fn rail(theme: &iced::Theme) -> container::Style {
    container::Style {
        background: Some(iced::Background::Color(crate::theme::recessed(theme))),
        ..container::Style::default()
    }
}

/// One muted line of prose, the quiet register. Takes what `text` takes, so a fixed line
/// borrows rather than allocating on every redraw.
fn muted_line<'a>(line: impl text::IntoFragment<'a>, size: f32) -> Element<'a, Message> {
    text(line)
        .size(size)
        .style(|t| text::Style {
            color: Some(muted(t)),
        })
        .into()
}

/// Where the `bsx` this window would spawn is, or what to set when it is nowhere.
fn bsx_line(app: &App) -> String {
    let home = std::env::var("HOME").ok();
    match &app.platform.bsx {
        Some(path) => tilde(home.as_deref(), path),
        None => "Not found: set $BSX_CLI, or put bsx beside bsx-app or on PATH.".to_string(),
    }
}

/// Where the default guest root is, and whether anything is there yet.
fn root_line(app: &App) -> String {
    let home = std::env::var("HOME").ok();
    match &app.platform.root {
        cli::GuestRoot::Present(path) => tilde(home.as_deref(), path),
        cli::GuestRoot::Absent(path) => {
            format!("{} (nothing there yet)", tilde(home.as_deref(), path))
        }
        cli::GuestRoot::Unset => "None: set $BSX_GUEST_ROOT.".to_string(),
    }
}

/// A path spelled the way a person says it: `home` contracted to `~`.
fn tilde(home: Option<&str>, path: &std::path::Path) -> String {
    let spelled = path.display().to_string();
    match home {
        Some(home) if !home.is_empty() && spelled.starts_with(home) => {
            format!("~{}", &spelled[home.len()..])
        }
        _ => spelled,
    }
}

/// The notebook's own knobs, one heading block per area; a later knob joins its block.
pub(crate) fn settings(app: &App) -> Element<'_, Message> {
    let modes = row(crate::theme::MODES.iter().map(|mode| {
        let on = *mode == app.mode;
        button(text(mode.to_string()).size(BODY))
            .style(move |t, s| if on { segment(t) } else { push(t, s) })
            .padding(PAGE_PAD)
            .on_press(Message::SetTheme(*mode))
            .into()
    }))
    .spacing(6);
    let theme_note = if app.theme_overridden {
        "Started with --theme or $BSX_THEME, which outranks this pick at the next launch."
    } else {
        "Light, dark, or whichever the desktop is showing."
    };
    let at = crate::SCALES
        .iter()
        .position(|s| s.0 == app.scale)
        .and_then(|i| u8::try_from(i).ok())
        .unwrap_or(1);
    let steps = u8::try_from(crate::SCALES.len() - 1).unwrap_or(3);
    let scale = column![
        slider(0..=steps, at, |i| Message::SetScale(
            crate::SCALES[usize::from(i)]
        ))
        .style(rail_of)
        .width(Fill),
        ticks(crate::SCALES.iter().map(ToString::to_string).collect()),
    ]
    .spacing(6);
    let mut body = column![
        setting(
            None,
            "Behavioral Sandbox",
            format!("version {}", env!("CARGO_PKG_VERSION")),
            space().width(0)
        ),
        setting(Some(icons::SUN_MOON), "Theme", theme_note, modes),
        stacked(
            Some(icons::SCALING),
            "Scale",
            "How large the notebook draws everything.",
            scale
        ),
        setting(
            Some(icons::ROCKET),
            "Open on a new run",
            "A plain launch shows the form instead of the notebook; --open and a named run \
             outrank it.",
            toggler(app.opens_on == crate::OpenScreen::New)
                .size(22)
                .style(switch)
                .on_toggle(|on| Message::SetOpensOn(if on {
                    crate::OpenScreen::New
                } else {
                    crate::OpenScreen::List
                })),
        ),
        setting(
            Some(icons::TERMINAL),
            "Command line",
            bsx_line(app),
            space().width(0)
        ),
        setting(
            Some(icons::FOLDER),
            "Guest root",
            root_line(app),
            space().width(0)
        ),
        row![
            space().width(Fill),
            page_button("Reset to defaults", push).on_press(Message::ResetSettings),
        ],
    ]
    .spacing(28)
    .width(Fill);
    if let Some(status) = &app.status {
        body = body.push(text(status).size(BODY));
    }
    framed(app, row![head_title("Settings")].into(), body)
}

/// One setting as a source-list app lays one out: its name over a grey line of what it does,
/// and the control at the row's right edge.
fn setting<'a>(
    icon: Option<icons::Icon>,
    title: &'a str,
    what: impl text::IntoFragment<'a>,
    control: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    row![labelled(icon, title, what).width(Fill), control.into()]
        .spacing(16)
        .align_y(iced::alignment::Vertical::Center)
        .into()
}

/// A setting's name over its line, with its icon at the left where there is one.
fn labelled<'a>(
    icon: Option<icons::Icon>,
    title: &'a str,
    what: impl text::IntoFragment<'a>,
) -> iced::widget::Row<'a, Message> {
    let mut line = row![].spacing(12);
    if let Some(icon) = icon {
        line = line.push(icons::glyph(icon, ICON));
    }
    line.push(
        column![text(title).size(TAB), muted_line(what, BODY)]
            .spacing(4)
            .width(Fill),
    )
}

/// A setting whose control wants the row's whole width, so it sits under the line instead.
fn stacked<'a>(
    icon: Option<icons::Icon>,
    title: &'a str,
    what: impl text::IntoFragment<'a>,
    control: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    column![labelled(icon, title, what), control.into()]
        .spacing(6)
        .into()
}

/// The labels under a stepped slider, one per step, the first flush left and the last flush
/// right so each sits under its notch.
fn ticks<'a>(labels: Vec<String>) -> Element<'a, Message> {
    let mut line = row![].width(Fill);
    let last = labels.len().saturating_sub(1);
    for (i, label) in labels.into_iter().enumerate() {
        line = line.push(muted_line(label, SMALL));
        if i < last {
            line = line.push(space().width(Fill));
        }
    }
    line.into()
}

/// The segment that is picked: the same bordered shape as [`push`], a step of grey darker.
fn segment(theme: &iced::Theme) -> button::Style {
    let palette = theme.extended_palette();
    let mut style = role(
        crate::theme::selected(theme),
        palette.background.base.text,
        Some(hairline(theme)),
        button::Status::Active,
    );
    style.shadow = RAISE;
    style
}

/// A slider as macOS draws one on a settings page: a grey rail, a white knob held by a hairline.
fn rail_of(theme: &iced::Theme, status: slider::Status) -> slider::Style {
    let palette = theme.extended_palette();
    let mut style = slider::default(theme, status);
    let rail = iced::Background::Color(crate::theme::selected(theme));
    style.rail.backgrounds = (rail, rail);
    style.rail.width = 4.0;
    style.handle = slider::Handle {
        shape: slider::HandleShape::Circle { radius: 9.0 },
        background: iced::Background::Color(palette.background.base.color),
        border_width: 1.0,
        border_color: hairline(theme),
    };
    style
}

/// A switch as macOS draws one: grey when off, the accent when on, a white knob either way.
fn switch(theme: &iced::Theme, status: toggler::Status) -> toggler::Style {
    let palette = theme.extended_palette();
    let mut style = toggler::default(theme, status);
    let on = matches!(
        status,
        toggler::Status::Active { is_toggled: true }
            | toggler::Status::Hovered { is_toggled: true }
    );
    style.background = iced::Background::Color(if on {
        palette.primary.base.color
    } else {
        crate::theme::selected(theme)
    });
    style.foreground = iced::Background::Color(iced::Color::WHITE);
    style
}

/// The corner an action takes; a surface takes [`CARD_RADIUS`], a field [`FIELD_RADIUS`].
const RADIUS: f32 = 8.0;
const CARD_RADIUS: f32 = 12.0;
const FIELD_RADIUS: f32 = 8.0;

/// The hairline every surface is held by: the palette's own text at a tenth, so it reads as an
/// edge catching light on any theme.
fn hairline(theme: &iced::Theme) -> iced::Color {
    theme
        .extended_palette()
        .background
        .base
        .text
        .scale_alpha(0.1)
}

/// A rule between two panes, or across a form: the same hairline a surface is held by, full
/// length, so nothing on a page is parted by a heavier line than its edges are drawn in.
fn divider(theme: &iced::Theme) -> rule::Style {
    rule::Style {
        color: hairline(theme),
        radius: 0.0.into(),
        fill_mode: rule::FillMode::Full,
        snap: true,
    }
}

/// The faint shadow under a push button's edge, as macOS sets one on the page.
const RAISE: iced::Shadow = iced::Shadow {
    color: iced::Color {
        a: 0.08,
        ..iced::Color::BLACK
    },
    offset: iced::Vector::new(0.0, 1.0),
    blur_radius: 2.0,
};

/// A push button as macOS draws one: the page's own surface inside a hairline, no colour of its
/// own, a step of grey under the pointer. Every action here is one of these.
fn push(theme: &iced::Theme, status: button::Status) -> button::Style {
    let palette = theme.extended_palette();
    bordered(theme, palette.background.base.text, status)
}

/// The default action, as macOS fills one: the accent under the label it carries, a step darker
/// under the pointer. One to a screen, or none of them reads as the way out.
fn primary(theme: &iced::Theme, status: button::Status) -> button::Style {
    let palette = theme.extended_palette();
    let surface = match status {
        button::Status::Hovered | button::Status::Pressed => palette.primary.strong.color,
        button::Status::Active | button::Status::Disabled => palette.primary.base.color,
    };
    let mut style = role(surface, palette.primary.base.text, None, status);
    style.shadow = RAISE;
    style
}

/// The destructive act: the same push button, its label in the palette's danger.
fn destructive(theme: &iced::Theme, status: button::Status) -> button::Style {
    let palette = theme.extended_palette();
    bordered(theme, palette.danger.base.color, status)
}

/// Navigation that stays quiet until the pointer finds it: no edge, the same grey under it.
fn ghost(theme: &iced::Theme, status: button::Status) -> button::Style {
    let palette = theme.extended_palette();
    let surface = match status {
        button::Status::Hovered | button::Status::Pressed => crate::theme::hovered(theme),
        button::Status::Active | button::Status::Disabled => iced::Color::TRANSPARENT,
    };
    role(surface, palette.background.base.text, None, status)
}

/// The shape [`push`] and [`destructive`] share, differing only in what colour the label is.
fn bordered(theme: &iced::Theme, text: iced::Color, status: button::Status) -> button::Style {
    let surface = match status {
        button::Status::Hovered | button::Status::Pressed => crate::theme::hovered(theme),
        button::Status::Active | button::Status::Disabled => crate::theme::raised(theme),
    };
    let mut style = role(surface, text, Some(hairline(theme)), status);
    style.shadow = RAISE;
    style
}

/// One shape for every role: a fill on the corner, an optional hairline, and the whole control
/// fading when disabled, the way a macOS dialog draws its buttons.
fn role(
    surface: iced::Color,
    text: iced::Color,
    edge: Option<iced::Color>,
    status: button::Status,
) -> button::Style {
    let faded = matches!(status, button::Status::Disabled);
    let dim = |color: iced::Color| if faded { color.scale_alpha(0.5) } else { color };
    button::Style {
        background: Some(iced::Background::Color(dim(surface))),
        text_color: dim(text),
        border: iced::Border {
            color: edge.map_or(iced::Color::TRANSPARENT, dim),
            width: f32::from(u8::from(edge.is_some())),
            radius: RADIUS.into(),
        },
        ..button::Style::default()
    }
}

/// A card: the page's own surface a shade lifted, held by a hairline border.
fn card(theme: &iced::Theme) -> container::Style {
    container::Style {
        background: Some(iced::Background::Color(crate::theme::raised(theme))),
        border: iced::Border {
            radius: CARD_RADIUS.into(),
            ..iced::Border::default()
        },
        ..container::Style::default()
    }
}

/// A text entry as macOS draws one: the default look on a rounded corner.
fn entry(theme: &iced::Theme, status: text_input::Status) -> text_input::Style {
    let mut style = text_input::default(theme, status);
    style.background = iced::Background::Color(crate::theme::raised(theme));
    style.border.radius = FIELD_RADIUS.into();
    style
}

/// The notebook: what is running, then what has run.
pub(crate) fn list(app: &App) -> Element<'_, Message> {
    let live: Vec<&Record> = app.runs.iter().filter(|r| app.is_live(r)).collect();
    let past: Vec<&Record> = app.runs.iter().filter(|r| !app.is_live(r)).collect();
    let start = row![head_title("Sandboxes"), space().width(Fill)];
    let header = if app.confirm_clear {
        start.push(
            row![
                text(format!("remove {}?", crate::ended_runs(past.len()))).size(BODY),
                small_button("Remove", destructive).on_press(Message::ClearConfirmed),
                small_button("Keep", push).on_press(Message::ClearCancelled),
            ]
            .spacing(12)
            .align_y(iced::alignment::Vertical::Center),
        )
    } else {
        let mut ordinary = start;
        if !past.is_empty() {
            ordinary =
                ordinary.push(small_button("Clear history", push).on_press(Message::ClearHistory));
        }
        ordinary.push(small_button("New run", push).on_press(Message::NewRun))
    }
    .spacing(12)
    .align_y(iced::alignment::Vertical::Center);

    let mut rows = column![].spacing(8);
    if !live.is_empty() {
        rows = rows.push(section("RUNNING", live.len()));
        for record in &live {
            rows = rows.push(run_row(app, record));
        }
    }
    if !past.is_empty() {
        rows = rows.push(section("HISTORY", past.len()));
        for record in &past {
            rows = rows.push(run_row(app, record));
        }
    }
    if app.runs.is_empty() {
        rows = rows.push(
            text("No runs yet. Start one here, or with `bsx run`, `bsx shell` or `bsx up`.")
                .size(BODY)
                .style(|t| text::Style {
                    color: Some(muted(t)),
                }),
        );
    }
    // A card is read left to right, so it stops where reading does: a row stretched across a wide
    // window puts its two halves too far apart to take in at once.
    let mut body = column![
        scrollable(rows)
            .direction(lane())
            .style(scroll)
            .height(Fill)
    ]
    .spacing(14)
    .width(Fill);
    if let Some(status) = &app.status {
        body = body.push(text(status).size(BODY));
    }
    framed(app, header.into(), body)
}

/// The width a page of cards stops at, in logical pixels.
const PAGE: f32 = 1000.0;

/// The inset a pane's own name and its actions sit at, off the sidebar and the window's edge.
const GUTTER: f32 = 24.0;

/// Where the page begins, from the window's own top edge rather than from the head above it, so
/// that the room a reader sees does not follow the head's height.
const PAGE_TOP: f32 = 82.0;

/// A screen the sidebar opened: its name at the pane's own edge, its content in a column centred
/// under it at the width a row is still taken in at one glance.
fn framed<'a>(
    app: &App,
    head: Element<'a, Message>,
    body: iced::widget::Column<'a, Message>,
) -> Element<'a, Message> {
    column![
        head_bar(app, head),
        container(body.max_width(PAGE))
            .width(Fill)
            .height(Fill)
            .padding(iced::Padding {
                top: PAGE_TOP - HEAD_BAR,
                right: GUTTER,
                bottom: GUTTER,
                left: GUTTER,
            }),
    ]
    .into()
}

/// What a head's name drops by, so that its letters rather than the font's room above them
/// centre on the head's line: twice the ascent's overshoot of the cap, at [`HEAD`].
const CAP_DROP: f32 = 4.0;

/// A pane's own name on a head's line, one line of type on its own body and dropped onto the
/// line by [`CAP_DROP`].
fn head_title(name: &str) -> Element<'_, Message> {
    container(text(name).size(HEAD).font(HEADING).line_height(1.0))
        .padding(iced::Padding::ZERO.top(CAP_DROP))
        .into()
}

/// A control that has to fit a line rather than a page: a head's, whose room the traffic lights
/// set, or the end of a row in the notebook.
fn small_button<'a>(
    label: &'a str,
    style: fn(&iced::Theme, button::Status) -> button::Style,
) -> iced::widget::Button<'a, Message> {
    button(text(label).size(BODY))
        .style(style)
        .padding(SMALL_PAD)
}

/// The room around a control that fits a line, and around a page's action.
const SMALL_PAD: [f32; 2] = [4.0, 10.0];
const PAGE_PAD: [f32; 2] = [6.0, 14.0];

/// A page's committing action, at the end of a form: the notebook's own type on the room macOS
/// gives a dialog's push button, which is more than a control on a line gets.
fn page_button<'a>(
    label: &'a str,
    style: fn(&iced::Theme, button::Status) -> button::Style,
) -> iced::widget::Button<'a, Message> {
    button(text(label).size(BODY))
        .style(style)
        .padding(PAGE_PAD)
}

/// A window's head: one row centred on the line the traffic lights sit on, and clear of them.
fn head_bar<'a>(app: &App, head: Element<'a, Message>) -> Element<'a, Message> {
    container(head)
        .height(HEAD_BAR)
        .align_y(iced::alignment::Vertical::Center)
        .padding(iced::Padding {
            top: 0.0,
            right: GUTTER,
            bottom: 0.0,
            left: head_inset(app),
        })
        .width(Fill)
        .into()
}

/// The sidebar's width: the nav labels plus their counts, and no more.
const SIDEBAR: f32 = 200.0;

/// A sidebar row's label, a step up from body text, as a source list sets one.
const TAB: f32 = 14.0;

/// An icon beside a label, drawn a little larger than the label's text, as macOS sets them.
const ICON: f32 = 17.0;

/// The one heading style: small, muted and set apart, on a pane and on a section alike.
fn heading(title: &str) -> Element<'_, Message> {
    text(title)
        .size(SMALL)
        .font(HEADING)
        .style(|t| text::Style {
            color: Some(muted(t)),
        })
        .into()
}

/// A section heading with how many are under it.
fn section(title: &'static str, count: usize) -> Element<'static, Message> {
    row![
        heading(title),
        text(format!("{count}")).size(SMALL).style(|t| text::Style {
            color: Some(muted(t))
        }),
        space().width(Fill),
    ]
    .spacing(8)
    .padding([10, 2])
    .into()
}

/// One card: the name and how it went on the first line, the command on the second, what it
/// could touch on the third, and a live frame beside them when there is one.
fn run_row<'a>(app: &'a App, record: &'a Record) -> Element<'a, Message> {
    let live = app.is_live(record);
    let command = if record.command.is_empty() {
        match record.verb {
            Verb::Up => "a sandbox to exec into".to_string(),
            _ => String::new(),
        }
    } else {
        record.command.join(" ")
    };
    // How it went and how long. No wall clock: that is on the run's own screen, and a row has
    // room for one of the two.
    let state = if live {
        format!(
            "running {}",
            bsx_record::format_duration(bsx_record::now_ms().saturating_sub(record.started_ms))
        )
    } else {
        let end = record.end.map(|e| e.to_string()).unwrap_or_default();
        match record.ended_ms {
            Some(ended) => format!(
                "{end} · {}",
                bsx_record::format_duration(ended.saturating_sub(record.started_ms))
            ),
            None => end,
        }
    };
    let dot = record.clone();
    let title = row![
        text("●").size(SMALL).style(move |t| text::Style {
            color: Some(status_colour(t, &dot, live))
        }),
        text(&record.name).font(NAME).size(TITLE),
        space().width(Fill),
    ]
    .spacing(8)
    .align_y(iced::alignment::Vertical::Center);
    // The command is one line and clipped, never wrapped: a long one reflowing a card pushes
    // every card below it out of place. The whole of it is on the run's own screen.
    let command = text(command)
        .font(MONO)
        .size(BODY)
        .wrapping(text::Wrapping::None)
        .style(|t| text::Style {
            color: Some(muted(t)),
        });
    // What it could touch, spelled rather than abbreviated: this is the line that says whether a
    // sandbox could reach the network or a directory, and it is worth the words.
    let posture = text(posture_tags(record))
        .size(SMALL)
        .wrapping(text::Wrapping::None)
        .style(|t| text::Style {
            color: Some(muted(t)),
        });
    let text_side = column![title, command, posture].spacing(4);
    // A running sandbox with a display shows it, so the list says what each one is doing rather
    // than only what it was asked to do.
    let told: Element<'_, Message> = match crate::frame_program(app, &crate::RunName::of(record)) {
        Some(program) if live => row![
            container(shader(program).width(Fill).height(Fill))
                .width(Length::Fixed(THUMBNAIL.0))
                .height(Length::Fixed(THUMBNAIL.1))
                .style(container::dark),
            text_side.width(Fill),
        ]
        .spacing(14)
        .align_y(iced::alignment::Vertical::Center)
        .into(),
        _ => text_side.width(Fill).into(),
    };
    // What this row can be told to do, at its own end: a sandbox is stopped where it is listed
    // rather than only on its own screen. There is no pause, because libkrun has no suspend.
    let body = row![
        told,
        text(state)
            .size(SMALL)
            .wrapping(text::Wrapping::None)
            .style(move |t| text::Style {
                color: Some(muted(t))
            }),
        row_actions(record, live),
    ]
    .spacing(12)
    .align_y(iced::alignment::Vertical::Center);
    // A button rather than a container under a `mouse_area`: the row is a thing you click, so it
    // answers the pointer with the step of grey a source list gives a row under one.
    button(body)
        .width(Fill)
        .padding(12)
        .style(row_card)
        .on_press(Message::Open(crate::RunId::of(record)))
        .into()
}

/// A card that is a row you click: the raised surface, a step under the pointer, no edge.
fn row_card(theme: &iced::Theme, status: button::Status) -> button::Style {
    let surface = match status {
        button::Status::Hovered | button::Status::Pressed => crate::theme::raised_hovered(theme),
        button::Status::Active | button::Status::Disabled => crate::theme::raised(theme),
    };
    let mut style = role(
        surface,
        theme.extended_palette().background.base.text,
        None,
        status,
    );
    style.border.radius = CARD_RADIUS.into();
    style
}

/// What a row can be told to do without opening the run: stop a live one, run an ended one
/// again, or take its record away. Everything else stays on the run's own screen.
fn row_actions<'a>(record: &Record, live: bool) -> iced::widget::Row<'a, Message> {
    if live {
        return row![
            small_button("Stop", destructive).on_press(Message::Stop(crate::RunName::of(record)))
        ];
    }
    row![
        small_button("Re-run", push).on_press(Message::Rerun(crate::RunId::of(record))),
        small_button("Delete", destructive).on_press(Message::Delete(crate::RunId::of(record))),
    ]
    .spacing(6)
}

/// How big a live sandbox's frame is in the list, in logical pixels. Wide enough to tell two
/// desktops apart at a glance, small enough that a screen of them is still a list.
const THUMBNAIL: (f32, f32) = (160.0, 120.0);

/// The posture in a glance, in words: the display, the network, and what of the host it can
/// reach. Abbreviations save a few characters and cost the reader the sentence, and this is the
/// line that says whether a sandbox could touch the network or a directory.
fn posture_tags(record: &Record) -> String {
    let p = &record.posture;
    let mut parts = Vec::new();
    if let Some(display) = p.display {
        parts.push(display.as_spec().replace('x', "\u{d7}"));
    }
    parts.push(
        if p.network == bsx_record::Network::Tsi {
            "network via host"
        } else {
            "no network"
        }
        .to_string(),
    );
    if p.rootfs == bsx_record::Rootfs::Writable {
        parts.push("writable root".to_string());
    }
    // One share is worth naming; several are worth counting, or the line outgrows the card.
    match (p.mounts.len(), p.shares.len()) {
        (0, 0) => parts.push("no host directories".to_string()),
        (1, 0) => {
            let (guest, host) = &p.mounts[0];
            parts.push(format!("{} \u{2190} {}", guest.display(), host.display()));
        }
        (0, 1) => {
            let (tag, host) = &p.shares[0];
            parts.push(format!("{tag} \u{2190} {}", host.display()));
        }
        (m, sh) => parts.push(format!("{} host directories", m + sh)),
    }
    if p.results {
        parts.push(bsx_record::RESULTS_GUEST_PATH.to_string());
    }
    if p.sound {
        parts.push("sound".to_string());
    }
    if p.gpu {
        parts.push("gpu".to_string());
    }
    parts.join(" \u{b7} ")
}

/// One run: its record on the left, its display and output on the right.
pub(crate) fn run<'a>(app: &'a App, id: &crate::RunId) -> Element<'a, Message> {
    let Some(record) = app.record(id) else {
        return framed(
            app,
            small_button("← runs", ghost).on_press(Message::Back).into(),
            column![text(format!("the run {id} is no longer in the notebook")).size(BODY)],
        );
    };
    let live = app.is_live(record);
    let mut bar = row![
        small_button("← runs", ghost).on_press(Message::Back),
        text(&record.name).font(MONO).size(TITLE).line_height(1.0),
        space().width(Fill),
    ]
    .spacing(12)
    .align_y(iced::alignment::Vertical::Center);
    bar =
        bar.push(small_button("Export", push).on_press(Message::Export(crate::RunId::of(record))));
    if live {
        if record.verb == Verb::Up {
            bar = bar.push(
                small_button("Shell", push).on_press(Message::Shell(crate::RunName::of(record))),
            );
        }
        bar = bar.push(
            small_button("Stop", destructive).on_press(Message::Stop(crate::RunName::of(record))),
        );
    } else {
        bar = bar
            .push(small_button("Re-run", push).on_press(Message::Rerun(crate::RunId::of(record))));
        bar = bar.push(
            small_button("Delete", destructive).on_press(Message::Delete(crate::RunId::of(record))),
        );
    }

    let left = scrollable(
        column![
            pane("POSTURE", posture_lines(record)),
            pane("RUN", run_lines(record, live)),
            pane("RESULTS", results_lines(app, record)),
        ]
        .spacing(12),
    )
    .direction(lane())
    .style(scroll)
    // A share of the window rather than a fixed width: the panes hold paths, and 340 logical
    // pixels of monospace broke a guest root across two lines on a narrow window.
    .width(Length::FillPortion(2));

    let mut right = column![].spacing(10);
    if live && record.posture.display.is_some() {
        let display: Element<'_, Message> =
            match crate::frame_program(app, &crate::RunName::of(record)) {
                Some(program) => shader(program).width(Fill).height(Fill).into(),
                None => container(text("leasing the display…").size(BODY))
                    .center(Fill)
                    .into(),
            };
        right = right.push(
            container(display)
                .width(Fill)
                .height(Length::FillPortion(3))
                .style(container::dark),
        );
    }
    right = right.push(output_pane(app, record).height(Length::FillPortion(2)));

    let body = row![
        left,
        rule::vertical(1).style(divider),
        right.width(Length::FillPortion(3))
    ]
    .spacing(12)
    .height(Fill);
    let mut page = column![
        head_bar(app, bar.into()),
        container(body).height(Fill).padding(iced::Padding {
            top: 0.0,
            right: GUTTER,
            bottom: GUTTER,
            left: GUTTER,
        }),
    ];
    if let Some(status) = &app.status {
        page = page.push(container(text(status).size(BODY)).padding([0.0, GUTTER]));
    }
    page.into()
}

/// The width a form's label column takes: the longest of them, so every field starts on one line.
const FORM_LABEL: f32 = 100.0;

/// The width a field holding a number takes, sized to the number rather than to the row.
const FORM_NUMBER: f32 = 96.0;

/// The width the label column of a pane takes, in logical pixels: the longest label plus a gap.
const LABEL: f32 = 74.0;

/// A checkbox's box, at the side macOS draws one beside body text.
const CHECK: f32 = 14.0;

/// A titled box of `label`, `value` rows, as two widgets so a long value wraps in its column.
fn pane<'a>(title: &'a str, rows: Vec<(String, String)>) -> Element<'a, Message> {
    let mut body = column![heading(title)].spacing(3);
    for (label, value) in rows {
        body = body.push(
            row![
                text(label)
                    .font(MONO)
                    .size(BODY)
                    .width(Length::Fixed(LABEL)),
                text(value).font(MONO).size(BODY).width(Fill),
            ]
            .spacing(4),
        );
    }
    container(body.padding(10)).width(Fill).style(card).into()
}

fn posture_lines(record: &Record) -> Vec<(String, String)> {
    let p = &record.posture;
    let mut lines = vec![(
        "root".to_string(),
        format!("{}, {}", p.root.display(), p.rootfs.as_word()),
    )];
    for (guest, host) in &p.mounts {
        lines.push((
            "mount".to_string(),
            format!("{} = {}", guest.display(), host.display()),
        ));
    }
    for (tag, host) in &p.shares {
        lines.push(("share".to_string(), format!("{tag} = {}", host.display())));
    }
    if p.mounts.is_empty() && p.shares.is_empty() {
        lines.push(("share".to_string(), "none".to_string()));
    }
    lines.push(("network".to_string(), p.network.as_word().to_string()));
    lines.push((
        "display".to_string(),
        p.display
            .map_or_else(|| "none".to_string(), |d| d.as_spec()),
    ));
    lines.push((
        "sound".to_string(),
        if p.sound { "on" } else { "off" }.to_string(),
    ));
    lines.push((
        "gpu".to_string(),
        if p.gpu { "on" } else { "off" }.to_string(),
    ));
    lines.push((
        "results".to_string(),
        if p.results {
            bsx_record::RESULTS_GUEST_PATH
        } else {
            "off"
        }
        .to_string(),
    ));
    lines.push((
        "limits".to_string(),
        format!("{} vcpu, {} MiB", p.vcpus, p.mem_mib),
    ));
    lines.push((
        "agent".to_string(),
        if record.verb == Verb::Up {
            "present"
        } else {
            "none"
        }
        .to_string(),
    ));
    lines
}

fn run_lines(record: &Record, live: bool) -> Vec<(String, String)> {
    let mut lines = vec![
        ("verb".to_string(), record.verb.as_word().to_string()),
        (
            "started".to_string(),
            bsx_record::format_time(record.started_ms),
        ),
    ];
    if !record.command.is_empty() {
        lines.push(("command".to_string(), record.command.join(" ")));
    }
    if let Some(pid) = record.pid {
        lines.push(("pid".to_string(), pid.to_string()));
    }
    match (live, record.end, record.ended_ms) {
        (true, _, _) => lines.push(("state".to_string(), "running".to_string())),
        (false, Some(end), Some(ended)) => {
            lines.push(("ended".to_string(), bsx_record::format_time(ended)));
            lines.push(("end".to_string(), end.to_string()));
        }
        (false, Some(end), None) => lines.push(("end".to_string(), end.to_string())),
        (false, None, _) => lines.push(("state".to_string(), "not answering".to_string())),
    }
    lines.push(("id".to_string(), record.id.clone()));
    lines
}

fn results_lines(app: &App, record: &Record) -> Vec<(String, String)> {
    if app.results.is_empty() {
        let dir = app.store.dir_of(&record.id);
        let home = std::env::var("HOME").ok();
        return vec![(
            "(none)".to_string(),
            format!("in {}", tilde(home.as_deref(), &dir.results())),
        )];
    }
    // The file is the value here, not the label: a result's path is the long half, so it gets the
    // column that wraps.
    app.results
        .iter()
        .map(|(file, size)| (bytes(*size), file.display().to_string()))
        .collect()
}

/// The captured output: one button per stream, and the tail of the chosen one.
fn output_pane<'a>(app: &'a App, record: &'a Record) -> iced::widget::Container<'a, Message> {
    let mut head = row![heading("OUTPUT"), space().width(Fill)].spacing(8);
    for stream in Stream::of(record.verb) {
        let on = app.output.stream == Some(*stream);
        head = head.push(
            button(text(stream.label()).size(BODY))
                .style(move |t, s| if on { segment(t) } else { push(t, s) })
                .padding(SMALL_PAD)
                .on_press(Message::Show(*stream)),
        );
    }
    let note = match (app.output.size, app.output.capped) {
        (0, _) => "(nothing yet)".to_string(),
        (size, true) => format!(
            "{} shown of {} (the record capped it)",
            bytes(size.min(crate::OUTPUT_TAIL)),
            bytes(size)
        ),
        (size, false) if size > crate::OUTPUT_TAIL => {
            format!("the last {} of {}", bytes(crate::OUTPUT_TAIL), bytes(size))
        }
        (size, false) => bytes(size),
    };
    let body = column![
        head,
        text(note).size(SMALL),
        scrollable(text(&app.output.text).font(MONO).size(BODY))
            .direction(lane())
            .style(scroll)
            .height(Fill),
    ]
    .spacing(6)
    .padding(10);
    container(body).width(Fill).style(card)
}

/// A field's aside, indented into the value column so the left edge stays the labels'.
fn caption(line: &'static str) -> Element<'static, Message> {
    row![
        space().width(Length::Fixed(FORM_LABEL + 8.0)),
        text(line).size(SMALL).style(|t| text::Style {
            color: Some(muted(t)),
        }),
    ]
    .into()
}

/// The form for a new run, with the posture sentence above the buttons.
pub(crate) fn new_run<'a>(app: &'a App, form: &'a Form) -> Element<'a, Message> {
    let field = |label: &'static str, value: &'a str, which: Field, width: Length| {
        row![
            text(label).width(Length::Fixed(FORM_LABEL)),
            text_input("", value)
                .style(entry)
                .on_input(move |v| Message::Field(which, v))
                .font(MONO)
                .width(width),
        ]
        .spacing(8)
        .align_y(iced::alignment::Vertical::Center)
    };
    let switch = |label: &'static str, on: bool, which: Switch| {
        checkbox(on)
            .label(label)
            .size(CHECK)
            .on_toggle(move |v| Message::Switch(which, v))
    };
    let mut posture = bsx_record::Posture::new(
        std::path::PathBuf::from(form.root.trim()),
        form.vcpus.trim().parse().unwrap_or(1),
        form.mem_mib.trim().parse().unwrap_or(512),
    );
    posture.rootfs = if form.writable_root {
        bsx_record::Rootfs::Writable
    } else {
        bsx_record::Rootfs::ReadOnly
    };
    posture.network = if form.network {
        bsx_record::Network::Tsi
    } else {
        bsx_record::Network::None
    };
    posture.mounts = form
        .mounts
        .split_whitespace()
        .filter_map(|m| m.split_once('='))
        .map(|(g, h)| (g.into(), h.into()))
        .collect();
    posture.shares = form
        .shares
        .split_whitespace()
        .filter_map(|m| m.split_once('='))
        .map(|(t, h)| (t.to_string(), h.into()))
        .collect();
    posture.display = form
        .display
        .then(|| bsx_record::DisplayMode::parse(form.display_size.trim()))
        .flatten();
    posture.sound = form.sound;
    posture.gpu = form.gpu;
    posture.results = form.results;

    let mut page = column![
        field("Name", &form.name, Field::Name, Fill),
        field("Root", &form.root, Field::Root, Fill),
        switch(
            "The guest may write its root",
            form.writable_root,
            Switch::WritableRoot
        ),
        field("Command", &form.command, Field::Command, Fill),
        caption("words split on spaces; empty starts a sandbox to exec into"),
        field("Mounts", &form.mounts, Field::Mounts, Fill),
        caption("GUESTDIR=HOSTDIR, space-separated, read-write"),
        field("Shares", &form.shares, Field::Shares, Fill),
        switch(
            "Network through the host (TSI)",
            form.network,
            Switch::Network
        ),
        row![
            switch("Display", form.display, Switch::Display),
            text_input("640x480", &form.display_size)
                .style(entry)
                .on_input(|v| Message::Field(Field::DisplaySize, v))
                .font(MONO)
                .width(Length::Fixed(140.0)),
            switch("Sound", form.sound, Switch::Sound),
            switch("GPU", form.gpu, Switch::Gpu),
        ]
        .spacing(16)
        .align_y(iced::alignment::Vertical::Center),
        switch(
            "Keep what the guest writes to /results in the record",
            form.results,
            Switch::Results
        ),
        row![
            field(
                "vCPUs",
                &form.vcpus,
                Field::Vcpus,
                Length::Fixed(FORM_NUMBER)
            ),
            field(
                "Memory MiB",
                &form.mem_mib,
                Field::Mem,
                Length::Fixed(FORM_NUMBER)
            ),
        ]
        .spacing(16),
        rule::horizontal(1).style(divider),
        text(posture.sentence()).size(BODY),
        row![
            space().width(Fill),
            page_button("Cancel", push).on_press(Message::Back),
            page_button("Start sandbox", primary).on_press(Message::Start),
        ]
        .spacing(12),
    ]
    .spacing(10)
    .width(Fill);
    if let Some(status) = &app.status {
        page = page.push(text(status).size(BODY));
    }
    framed(
        app,
        head_title("New run"),
        column![scrollable(page).direction(lane()).style(scroll)].width(Fill),
    )
}

/// `n` bytes as a reader wants them.
fn bytes(n: u64) -> String {
    if n >= 1024 * 1024 {
        format!("{:.1} MiB", n as f64 / (1024.0 * 1024.0))
    } else if n >= 1024 {
        format!("{:.1} KiB", n as f64 / 1024.0)
    } else {
        format!("{n} B")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The toggle and a head cross the window on their own tracks as the sidebar folds. This
    /// walks the fold and holds the gap between them to the gutter at every step.
    #[test]
    fn the_toggle_keeps_its_room_from_a_head_across_the_whole_fold() {
        for step in 0u8..=100 {
            let out = f32::from(step) / 100.0;
            let toggle_ends = toggle_at(out) + TOGGLE;
            let head_starts = SIDEBAR * out + head_inset_at(out);
            assert!(
                head_starts - toggle_ends >= GUTTER,
                "at {out} out, the head starts at {head_starts} and the toggle ends at {toggle_ends}"
            );
        }
    }

    /// A path under home is spelled with `~`; anything else is left whole.
    #[test]
    fn a_path_under_home_is_spelled_with_a_tilde() {
        let path = std::path::Path::new("/Users/x/Desktop/tree");
        assert_eq!(tilde(Some("/Users/x"), path), "~/Desktop/tree");
        assert_eq!(tilde(Some("/Users/y"), path), "/Users/x/Desktop/tree");
        assert_eq!(tilde(None, path), "/Users/x/Desktop/tree");
        assert_eq!(tilde(Some(""), path), "/Users/x/Desktop/tree");
    }
}
