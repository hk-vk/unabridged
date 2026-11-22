//! Reusable components. Each is a [`gpui_kit::RenderOnce`] builder that reads
//! the active theme and applies [`crate::elevation`] levels; none takes raw colours
//! or sizes except where a caller legitimately chooses between theme roles.

mod button;
mod chip;
mod controls;
mod cover;
mod cover_cache;
mod menu;
mod navigation;
mod progress;

pub use button::{BackLink, Button, ButtonShape, ButtonVariant, ClickHandler, IconButton};
pub use chip::{Chip, ChipStyle, KeyHint};
pub use controls::{GroupedList, ListRow, SearchField, Switch, TextField, ToggleHandler};
pub use cover::{Cover, CoverSize};
pub use menu::MenuTrigger;
pub use navigation::{
    SIDEBAR_ROW_PADDING, SectionHeader, SidebarHeader, SidebarItem, TOOLBAR_PADDING, Toolbar,
};
pub use progress::{Scrubber, ScrubberColors, SeekHandler, SegmentedProgress};
