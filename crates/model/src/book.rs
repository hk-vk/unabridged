//! Books, chapters, listening progress and catalogue entries.

use std::time::Duration;

/// Stable identifier of a title (an Audible ASIN for the real backend).
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BookId(pub String);

impl BookId {
    /// An id from any string.
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }
}

/// One chapter of a book.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chapter {
    /// Chapter title without the "Chapter N" prefix; empty if the book has none.
    pub title: String,
    /// Offset of the chapter from the start of the book.
    pub start: Duration,
    /// Length of the chapter.
    pub duration: Duration,
}

impl Chapter {
    /// Offset of the end of the chapter.
    pub fn end(&self) -> Duration {
        self.start + self.duration
    }
}

/// A title: metadata shared by library items and catalogue entries.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Book {
    /// Identifier.
    pub id: BookId,
    /// Title.
    pub title: String,
    /// Authors, in credit order.
    pub authors: Vec<String>,
    /// Narrators, in credit order.
    pub narrators: Vec<String>,
    /// Cover image: an embedded asset path, a file path or a URL.
    pub cover: String,
    /// Chapters in order; their durations sum to the runtime.
    pub chapters: Vec<Chapter>,
    /// Primary genre, if known.
    pub genre: Option<String>,
}

impl Book {
    /// Total length.
    pub fn runtime(&self) -> Duration {
        self.chapters.last().map(Chapter::end).unwrap_or_default()
    }

    /// "Ines Calloway, read by Tomas Reyes".
    pub fn credit(&self) -> String {
        let authors = join_names(&self.authors);
        if self.narrators.is_empty() {
            authors
        } else {
            format!("{authors}, read by {}", join_names(&self.narrators))
        }
    }

    /// The first author, for compact credits.
    pub fn primary_author(&self) -> &str {
        self.authors.first().map(String::as_str).unwrap_or_default()
    }
}

/// "A", "A and B", "A, B and C".
pub fn join_names(names: &[String]) -> String {
    match names {
        [] => String::new(),
        [one] => one.clone(),
        [init @ .., last] => format!("{} and {last}", init.join(", ")),
    }
}

/// Where a listening position was last synced from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Device {
    /// This computer.
    ThisMac,
    /// An iPhone.
    IPhone,
    /// An iPad.
    IPad,
    /// The Audible website.
    Web,
    /// Any other named device.
    Other(String),
}

impl Device {
    /// "This Mac", "iPhone", "iPad".
    pub fn label(&self) -> &str {
        match self {
            Device::ThisMac => "This Mac",
            Device::IPhone => "iPhone",
            Device::IPad => "iPad",
            Device::Web => "the web",
            Device::Other(name) => name,
        }
    }

    /// The phrase after "Picked up from": "your iPhone", "this Mac".
    pub fn possessive(&self) -> String {
        match self {
            Device::ThisMac => "this Mac".into(),
            Device::Web => "the web".into(),
            other => format!("your {}", other.label()),
        }
    }
}

/// Where the listener is in a book.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Progress {
    /// Offset from the start of the book.
    pub position: Duration,
    /// Device the position came from.
    pub device: Device,
    /// How long ago it was last played.
    pub last_played_ago: Duration,
}

/// A title in the user's library.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LibraryItem {
    /// The title.
    pub book: Book,
    /// Listening progress; `None` if never started.
    pub progress: Option<Progress>,
    /// Whether the title is marked finished.
    pub finished: bool,
    /// Whether the audio is downloaded to this Mac.
    pub downloaded: bool,
    /// How long ago it was added to the library.
    pub added_ago: Duration,
}

impl LibraryItem {
    /// Started but not finished.
    pub fn is_in_progress(&self) -> bool {
        self.progress.is_some() && !self.finished
    }
}

/// The user's library: the items fetched so far and the full count.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Library {
    /// Items, in no particular order.
    pub items: Vec<LibraryItem>,
    /// Number of titles in the account (may exceed `items` while paging).
    pub total: usize,
    /// User collections.
    pub collections: Vec<Collection>,
}

impl Library {
    /// In-progress items, most recently played first.
    pub fn in_progress(&self) -> Vec<&LibraryItem> {
        let mut items: Vec<_> = self.items.iter().filter(|i| i.is_in_progress()).collect();
        items.sort_by_key(|i| i.progress.as_ref().map(|p| p.last_played_ago));
        items
    }

    /// Items added within `window`, newest first.
    pub fn added_within(&self, window: Duration) -> Vec<&LibraryItem> {
        let mut items: Vec<_> = self
            .items
            .iter()
            .filter(|i| i.added_ago <= window)
            .collect();
        items.sort_by_key(|i| i.added_ago);
        items
    }

    /// Looks an item up by id.
    pub fn get(&self, id: &BookId) -> Option<&LibraryItem> {
        self.items.iter().find(|i| &i.book.id == id)
    }
}

/// A user-made list of titles.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Collection {
    /// Display name.
    pub name: String,
    /// Members.
    pub books: Vec<BookId>,
}

/// The price of a catalogue title.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Price {
    /// Bought with membership credits.
    Credits(u32),
    /// Cash price in minor units of `currency`.
    Cash {
        /// Amount in cents.
        cents: u64,
        /// ISO currency code.
        currency: String,
    },
}

impl Price {
    /// "1 credit", "$14.95".
    pub fn label(&self) -> String {
        match self {
            Price::Credits(1) => "1 credit".into(),
            Price::Credits(n) => format!("{n} credits"),
            Price::Cash { cents, currency } => {
                let symbol = match currency.as_str() {
                    "USD" => "$",
                    "GBP" => "£",
                    "EUR" => "€",
                    _ => "",
                };
                format!("{symbol}{}.{:02}", cents / 100, cents % 100)
            }
        }
    }
}

/// A title in the store.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogItem {
    /// The title.
    pub book: Book,
    /// Price.
    pub price: Price,
}

/// Colour family of a genre tile; the app maps it onto accent themes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GenreTone {
    /// Coral.
    Ember,
    /// Teal.
    Fjord,
    /// Violet.
    Plum,
    /// Yellow.
    Saffron,
    /// Green.
    Moss,
    /// Neutral fill.
    Neutral,
}

/// A browsable genre.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Genre {
    /// Display name.
    pub name: String,
    /// Tile colour.
    pub tone: GenreTone,
}

/// The featured title on Explore.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Featured {
    /// The title.
    pub item: CatalogItem,
    /// "New this week in Fantasy".
    pub context: String,
    /// One-sentence pitch.
    pub blurb: String,
}

/// Everything the Explore screen shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExploreFeed {
    /// The banner title.
    pub featured: Featured,
    /// Genre tiles.
    pub genres: Vec<Genre>,
    /// "New and noteworthy".
    pub new_releases: Vec<CatalogItem>,
}

/// A wall-clock time of day, for "synced at 9:32 PM".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimeOfDay {
    /// 0..24.
    pub hour: u8,
    /// 0..60.
    pub minute: u8,
}

/// The signed-in account.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Account {
    /// Marketplace name, such as "Audible US".
    pub marketplace: String,
    /// Membership credits available.
    pub credits: u32,
    /// When the library last synced, if it has.
    pub last_sync: Option<TimeOfDay>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credits_join_names_naturally() {
        let names = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(join_names(&names(&["A"])), "A");
        assert_eq!(join_names(&names(&["A", "B"])), "A and B");
        assert_eq!(join_names(&names(&["A", "B", "C"])), "A, B and C");
    }

    #[test]
    fn prices_format_as_in_the_store() {
        assert_eq!(Price::Credits(1).label(), "1 credit");
        assert_eq!(Price::Credits(2).label(), "2 credits");
        let cash = Price::Cash {
            cents: 1495,
            currency: "USD".into(),
        };
        assert_eq!(cash.label(), "$14.95");
    }
}
