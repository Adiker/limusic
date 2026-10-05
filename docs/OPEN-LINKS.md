# Open YouTube links

Home and Search each have a URL field next to the song search field. Paste a link and press
**Enter**, or click the arrow inside the field. The field uses the same appearance and size as
the adjacent search field; on narrow windows the controls wrap so both remain available.

- **Song:** starts playback with its radio queue. A watch URL containing both `v` and `list`
  plays the song identified by `v`.
- **Playlist:** opens the playlist, including playlists shared only by link.
- **Album or artist:** opens the corresponding page.

Supported forms include `music.youtube.com/watch?v=…`, `youtube.com/watch?v=…`, `youtu.be/…`,
`music.youtube.com/playlist?list=…`, album links under `music.youtube.com/browse/MPRE…`, and artist
links under `music.youtube.com/channel/UC…` or `/browse/UC…`. You can paste a hostname without
`https://`; surrounding whitespace is ignored. An album's `OLAK5uy_…` playlist link opens its
track list as a playlist.

An unsupported link shows an error and stays in the field so you can correct it. Opening a link
does not submit a text search or add an entry to recent searches. Playback and online pages still
require the usual YouTube connection.

## Keyboard shortcut

Press **Ctrl+L** (**⌘L** on macOS) to focus the URL field and select its contents for replacement.
On pages without a URL field, the shortcut opens the existing **Open link** dialog. It also selects
the URL in that dialog when it is already open. Other open dialogs keep their focus; the mini
player does not handle this shortcut.

The shortcut appears in the empty URL field and in the keyboard shortcut list, opened with
**Ctrl+H** (**⌘/** on macOS). The title bar's **Open link** button remains available from any page.
