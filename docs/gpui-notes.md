# GPUI notes

Verified against the sources in `~/.cargo/registry/src/*/`: `gpui-kit 0.9.1`, which re-exports
`gpui-pre 0.3.12` (a snapshot of Zed's `gpui`, zed@d2a4fb6), plus `gpui-base` and `gpui-component 0.9.1`.
Paths below are relative to `gpui-pre-0.3.12/src` unless stated. Nothing here is guessed: each line names
the file it came from.

## Crate facade

- Depend on `gpui-kit` only. `use gpui_kit::*;` is GPUI itself (`gpui-kit/src/lib.rs` glob re-export).
  `gpui_kit::component` is gpui-component, `gpui_kit::base` is gpui-base.
- `gpui_kit::application()` opens the platform; `Application::with_assets(impl AssetSource)` (`app.rs:266`)
  registers the asset source; `gpui_kit::init(cx)` initialises gpui-base and gpui-component (needed for
  their `Input`). `gpui_kit::open_window` wraps the view in a `base::Root`.
- `gpui_kit::actions!(namespace, [A, B])` defines unit actions without a `gpui` crate alias.

## Windows

- `WindowOptions` (`platform.rs:2602`): `titlebar: Some(TitlebarOptions { title, appears_transparent: true,
  traffic_light_position: Some(point(px(x), px(y))) })` draws content under the titlebar with the native
  traffic lights repositioned. `window_min_size`, `window_bounds: Some(WindowBounds::Windowed(bounds))`,
  `kind: WindowKind::{Normal, PopUp, Floating, Dialog, AnchoredPopup}`, `is_resizable`, `is_minimizable`.
- `window_background: WindowBackgroundAppearance::{Opaque, Transparent, Blurred}` (`platform.rs:2899`).
  `Blurred` is a whole-window behind-window blur (NSVisualEffect-like). There is no per-element backdrop
  blur: an element cannot blur what the window itself draws beneath it.
- `app_owns_titlebar_drag` is only for windows that move themselves with `Window::start_window_move`.
- Appearance: `cx.window_appearance()` (`app.rs:1810`), `window.appearance()` and
  `window.observe_window_appearance(cb)` (`window.rs:2537`) give `WindowAppearance::{Light, VibrantLight,
  Dark, VibrantDark}` for the Automatic mode.
- No status-bar (NSStatusItem) API exists in gpui, gpui-base or gpui-component; a menu bar extra needs a
  direct AppKit bridge.

## Styling

- Backgrounds: `.bg(impl Into<Fill>)` accepts an `Hsla` or a `Background`. `linear_gradient(angle_deg,
  linear_color_stop(color, 0.0), linear_color_stop(color, 1.0))` (`color.rs:865`) supports two stops only;
  angle 180 runs top to bottom.
- Shadows: `.shadow(Vec<BoxShadow>)`. `BoxShadow::new(x, y, color).blur_radius(..).spread_radius(..).inset()`
  (`style.rs:349`). Inset shadows are supported in this snapshot.
- Paint order inside one element (`style.rs:Style::paint`): drop shadows, background, inset shadows,
  children, border. The border is drawn on top of children and inside the bounds.
- Text: `.font_family()`, `.font_weight(FontWeight::MEDIUM)`, `.text_size(px())`, `.line_height(px()|relative())`,
  `.text_color()`, `.truncate()`, `.text_ellipsis()`, `.whitespace_nowrap()`, `.font_features(FontFeatures(..))`
  (`styled.rs`). `FontFeatures(Arc<Vec<(String, u32)>>)` takes OpenType tags such as `("tnum", 1)`.
- There is no letter-spacing / tracking property in `TextStyle` (`style.rs:438`).

## Elements

- `svg().path("icons/x.svg")` loads through the asset source and paints a monochrome mask tinted by
  `text_color` (`elements/svg.rs`); without a text colour nothing is painted.
- `img(src)`: a `&str` / `String` that is not a URL is `Resource::Embedded` and loads via the asset source
  (`elements/img.rs:59`). `PathBuf` loads from disk. `.object_fit(ObjectFit::Cover)` crops, and corner radii
  from `.rounded_*` clip the image.
- Interactivity (`elements/div.rs`): `.id(..)` makes a `Stateful<Div>` that can take `.on_click`, `.hover`,
  `.active`, `.overflow_y_scroll()`, `.track_scroll(&ScrollHandle)`. `.track_focus(&FocusHandle)` and
  `.key_context("Name")` scope actions; `.on_action(cx.listener(|this, action: &A, window, cx| ..))`.
- Key bindings: `cx.bind_keys([KeyBinding::new("cmd-f", FocusSearch, None)])` (`app.rs:2732`).

## Fonts

- `cx.text_system().add_fonts(vec![Cow::Borrowed(bytes)])` (`text_system.rs:301`) registers TTFs.
- Family names come from the font's typographic family (name ID 16, then 1). Geist Medium and SemiBold
  report typographic family "Geist", so `.font_family("Geist")` plus a weight selects them.
  Geist Pixel Square reports "Geist Pixel". Literata Regular reports "Literata".

## Text input

- gpui-component's `Input::new(&Entity<InputState>)` renders a full text field; `.appearance(false)` drops its
  own border and background so it can sit inside our search bar. `InputState::new(window, cx)
  .placeholder(..)` (gpui-base `input/base/state.rs`); it emits `InputEvent::{Change, PressEnter, Focus, Blur}`.
  Its text and caret colours come from the gpui-component `Theme` global, so we sync that from our theme.
