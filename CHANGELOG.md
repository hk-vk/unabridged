# Changelog

What changed in each release of Unabridged. The release workflow copies a version's section into its GitHub release notes, so add one before tagging.

## 0.2.0

- **Stream any book.** Play starts a book straight away without downloading it first. Downloading is now optional: a small download icon next to Play saves the book for offline listening. Recently streamed parts are cached, so going back over a stretch doesn't fetch it again.
- **The player opens at once.** Pressing Play takes you straight to Now Playing, which loads the book there. Skips pressed while it loads count from where you left off.
- **Read along on streamed books.** Read along fetches the chapter you're in, then transcribes it as before. The first lines appear within seconds, and it stops working on a chapter you've moved away from.
- **Search the transcript.** The search button over read along (or ⌘F) finds a word in the chapter; pick a match to jump there.
- **Recovers from a dropped connection.** A stream that stalls reconnects by itself, and Play on a book that failed to load tries again from where it got to.

## 0.1.0

The first public release: a native Mac app for your Audible library.

- **Your whole library, natively.** Library, In progress, On this Mac, Bookmarks, Authors, Series and Finished, with search across titles, authors and narrators.
- **Progress you can see at a glance.** Progress bars on covers, a chapter bar for the book you're playing, and "Almost done" when a book is nearly finished.
- **A player built around chapters.** Each book's player takes its colour from the cover. Skip 30 seconds, change speed from 0.75× to 2.5×, set a sleep timer for minutes or the end of the chapter, and add bookmarks.
- **Read along.** An on-device transcript follows the narration; click a line to jump there, and change the text size with ⌘+ and ⌘−. Needs macOS 26.
- **Mini player.** A small floating window that stays above your other apps.
- **Pick up where you left off.** Your position syncs with Audible, so you can move between your Mac and your phone.
- **Offline listening.** Download books to your Mac and play them without a connection.
- **Explore.** Browse Audible's catalog and new releases.
- **Looks right on your Mac.** Light, dark or automatic, five accent colours, and a light or dark app icon.
- **Automatic updates.** Checks once a day; Settings → About has Check for Updates….
