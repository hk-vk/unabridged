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
  Geist Pixel Square reports "Geist Pixel". Literata Regular reports "Literata". Verified at runtime with
  `cx.text_system().all_font_names()` and `resolve_font`: Geist 400/500/600 resolve to three distinct faces.

## Text input

- gpui-component's `Input::new(&Entity<InputState>)` renders a full text field; `.appearance(false)` drops its
  own border and background so it can sit inside our search bar. `InputState::new(window, cx)
  .placeholder(..)` (gpui-base `input/base/state.rs`); it emits `InputEvent::{Change, PressEnter, Focus, Blur}`.
  Its text and caret colours come from the gpui-component `Theme` global, so we sync that from our theme.

## Elevation (shadows and button polish)

What GPUI does (`window.rs` `paint_drop_shadows` / `paint_inset_shadows`, `shaders.metal` `shadow_fragment`):

- Drop shadows: bounds are `(bounds + offset).dilate(spread)`, snapped outward with `cover_bounds`
  (floor/ceil to device pixels), but the corner radii stay the element's radii; they are not grown
  by the spread.
- Inset shadows: the hole is `(bounds + offset).dilate(-spread)` with radii shrunk by the spread, and the
  result is clipped to the element's rounded SDF with a one-pixel anti-aliased edge.
- `blur_radius == 0` is special-cased to an anti-aliased SDF edge, so zero-blur insets are crisp.
- Borders are snapped with `round_stroke_to_device_pixel` (never thinner than one device pixel) and share
  the element's SDF, so they are concentric with the fill by construction.

Why the mock-up looks rough: it stacks `inset 0 1px 0` highlights, a `0 0 0 0.5px` spread ring and
blurred drops. In GPUI that ring keeps the element radius while growing outward by 0.5 pt, and its bounds
snap outward, so the halo is 0 to 1 device px wide depending on position, with corners that do not
follow the fill. Highlights at a fixed 1 pt offset also collide with whatever edge is drawn.

The pattern (`crates/ui/src/elevation.rs`), similar to how AppKit draws bezel buttons:

1. **Ring = border inside the bounds**, exactly one device pixel (`1 / scale_factor`): 0.5 pt at 2x,
   1 pt at 1x. Never a spread shadow.
2. **Highlight / shade = zero-blur inset shadow** offset by `ring + 1 device px`, so exactly one device
   pixel of light (top) or shade (bottom) shows just inside the ring at any scale. The crescent at the
   corners comes from the clipped SDF, so it tapers cleanly.
3. **Inner stroke** (floating player, translucent windows) = inset shadow with `spread = ring + 1 device px`;
   GPUI shrinks the hole radius by the spread, so the stroke stays concentric.
4. **Drops have zero spread** and only offset and blur (tight contact shadow plus soft ambient or coloured glow).
5. Every level is named (`Elevation::{CoverShelf, Cover, CoverHero, CoverStage, ButtonPrimary,
   ButtonSecondary, Chip, ButtonInk, ButtonLight, ButtonGlass, Field, SwitchOn/Off, Knob, FloatingPlayer,
   Banner, WindowMaterial}`) and resolves against the theme, so no component writes a shadow literal. Unit
   tests assert that no level uses spread drops and that every highlight sits inside a ring.

Verified on a 2x display with `Window::render_to_image` (see "Snapshots"): rings are a single crisp device
pixel, corners concentric, no halo. 1x follows from the same arithmetic (ring and band are whole device
pixels by construction) but was not rendered here, as no 1x display was attached.

Clipping caveat: GPUI content masks are rectangles, so `overflow_hidden` does not clip to rounded corners.
Children that touch a rounded parent's corners round their own corners: cover images take the frame's
radius, the current-chapter progress fill rounds its leading corners, and grouped-list edge rows round to
`concentric(card_radius, border)`.

## Icons

Hugeicons free (`@hugeicons/core-free-icons` 4.4.0, MIT; licence in `assets/icons/LICENSE.md`), stroke-rounded at
the set's 1.5 stroke. `tools/hugeicons-to-svg.py` converts the package's ESM arrays to SVG files; play and pause
are also filled (`fill="currentColor"`) to match the design's solid transport glyphs. `ui::Icon` is the only way to
reference them: a macro generates the enum, its `icons/<name>.svg` path and the `include_bytes!` table that
`ui::Assets` serves, so a missing file fails the build.

## Snapshots

`Window::render_to_image()` (`window.rs:3095`) renders the current scene offscreen through Metal and returns an
`RgbaImage`. It needs the `test-support` feature on both `gpui-pre` and `gpui-pre-macos`; enabling it through
`gpui-kit/test-support` fails to resolve (it drags in gpui-fast with a conflicting taffy pin), so the app's
`snapshot` feature enables those two crates directly. This avoids needing screen-recording permission.
