# Library and Workshop interface (0.2.4)

The library follows the familiar wallpaper browser arrangement: a compact navigation bar, collection filters on the left, a virtualized thumbnail grid in the centre and a persistent preview/display/settings inspector on the right. Artwork remains the primary visual content. A single click selects a tile for customization. Double-click applies it directly to the selected display through the same guarded action as Apply, including the current property overrides. Window preview is a separate secondary action. Favorite-star clicks, including double-clicks, only toggle the favorite.

The library banner names the selected display, its current wallpaper and its state, with a Show wallpaper action. The display picker lists the wallpaper title and state next to each connector. Switching displays selects that display's wallpaper in the inspector and scrolls its tile into view when it matches the current library filters. Tiles list all assigned displays; the selected display also has an explicit Playing, Paused or Saved badge. Live supervisor state takes precedence over a saved assignment; saved assignments without a running renderer say Saved · not running. Window previews do not appear as desktop assignments. Steam items retain their identity across download, installed and preserved paths. Assignments update only after the supervisor responds; rejected replacements retain the previous wallpaper.

Each installed tile has an always-visible star button at the top right, with an accessible action label and selected state. It adds/removes a local favorite without selecting or applying the wallpaper. The inspector has a text-labelled equivalent. The left-hand Favorites filter combines with the other filters and displays the available favorite count. Favorites, the filter and the selected sort are saved atomically in the state directory, independently of pending playback settings.

Sort includes Title, Type, Subscribed: newest first and Subscribed: oldest first. Subscription sorting uses Steam's list-added timestamp from an unfiltered, paginated subscription query, including the user's installed mature content regardless of Workshop browsing preferences. Date sorts show progress, a Sync Steam dates action and the number of dated Workshop items. All undated items appear last in either direction; title and path break ties consistently. A completed sync replaces the offline cache atomically; a failed connection, page or save preserves the previous cache. The helper is released after synchronization unless Workshop or active downloads still need it.

On Hyprland the initially selected display is the focused monitor. The Apply button names its target, for example “Set on DP-2”. An explicit display selection remains selected during background library refreshes.

At startup a modal display picker slides down and fades in over 220 ms. Monitor cards reflect the compositor's actual logical positions, sizes, rotation and scale, including negative coordinates. Clicking a card chooses its connector and closes the picker; applying a wallpaper remains a separate action. The current target is also named in the top navigation, where the picker can be reopened from any tab. Escape and Close retain the existing target. A small or overlapping arrangement gains separate labelled selectors; missing position data is labelled and uses a row rather than an invented physical arrangement. With egui animations disabled the picker appears immediately.

Workshop uses the same navigation, tokens, three-column structure and thumbnail cache. Its left panel contains Popular this week, Newest, My subscriptions, and type/resolution/category filters; its centre contains a labelled search field, result count, pagination and virtualized artwork cards. Its right panel shows the selected item's details, an explicit Download & subscribe action, a link to the Workshop page and the current session's download queue. Download states have text as well as progress; an unknown total uses a spinner rather than a fabricated percentage. Completed downloads link back to Installed, where display selection and application retain the existing flow. Connection failures remain in the Workshop view with a Connect to Steam action.

The sort menu includes Popular Today / Week / Month / Year / All Time, Newest and My subscriptions. Show NSFW is a labelled checkbox, off by default, remembered alongside the selected sort. Steam account preferences remain applicable; local Installed content is independent. Missing-file recovery has an explicit Restore missing downloads action and a persistent queue.

## Wallpaper customization

The installed inspector opens on **Customize**, with a compact artwork preview and the wallpaper's own options immediately visible. **Details** contains compatibility information, favorites, the project location and manual connector entry. The inspector defaults to 364 px and can resize from 320 to 480 px. The display selector, Apply and window preview stay in a fixed footer while the content scrolls; playback controls remain available there for the selected output.

Properties use their native types: labelled checkboxes, bounded sliders with an editable numeric value, color swatches with an sRGB hex readout, labelled dropdown choices, text inputs and asynchronous native file/folder dialogs. Empty text is a valid override. Controls use author `order` (including fractions), falling back to `index` and key; group properties become collapsible sections. Author HTML is rendered as plain text, with no remote artwork or embedded code. English author translations and common built-in labels replace raw localization keys.

Lists with more than eight editable options include a search field. Changed-only filtering and a change count appear for larger lists or after an edit. Search also matches group titles and opens matching groups. Reset is available per setting and for the full wallpaper. Restoring a default removes its renderer override. Display conditions support property references, boolean operators, parentheses and common comparisons; unsupported expressions leave settings visible. No JavaScript is evaluated.

Draft settings survive selecting another wallpaper or reselecting the same tile during the session, including Steam moving a project into its installed folder. Selecting a wallpaper initially restores its saved property overrides for the selected display when available. Changes become active only through Apply or window preview, which start the renderer with the overrides; Apply saves the display assignment through the existing supervisor. Drafts are not a live renderer update. The hidden `ui-snapshot --wallpaper <ID-or-path>` development command captures the native Customize view without applying it.

## Startup playback

On a normal GUI launch, saved wallpaper assignments are restored once in a background worker after monitor, renderer and asset discovery. Discovery continues while an assets drive is unavailable. The selected target display does not limit restoration: every connected display with a saved assignment is considered. Existing running or paused renderers stay intact, and repeated/concurrent app openings do not restart them. Window previews and disconnected outputs are excluded. A failed wallpaper leaves its assignment intact and does not prevent other saved wallpapers from starting. Progress and any failures appear in the existing feedback area. Development UI snapshots never restore playback.

## Tokens

| Role | Value |
| --- | --- |
| Canvas | `#202329` |
| Panel | `#252930` |
| Surface | `#2D323B` |
| Text | `#E9EDF3` |
| Secondary text | `#A7B1C0` |
| Accent / selection | `#73B9FF` |
| Error text / border | `#FF8A91` on `#42272E` |

The existing egui typeface and bundled CJK fallback are retained: 14 px body, 12 px secondary labels, 20 px headings. Corners are 4 px; native button semantics preserve keyboard navigation and activation. Thumbnail titles wrap to two lines with ellipsis; their full text is available in the inspector and tooltip. Warnings use explicit text as well as colour.

## Compatibility policy

Unsupported project formats, including Windows application wallpapers, are marked red. CORSAIR-O-Tron's reproduced legacy numeric-property failure is marked only with the pinned v0.9.4 renderer and the affected metadata. A different renderer or corrected numeric values remove that specific classification. Other web/SceneScript projects stay neutral: script usage alone is not proof of incompatibility. Missing assets or an unavailable renderer disable playback without labelling every wallpaper incompatible.

## Resource limits

Only visible grid rows request thumbnails. At most four image decoders run concurrently, each subject to input, dimension and allocation limits; decoded previews are capped at 640 × 400. A bounded least-recently-used texture cache retains at most 96 entries, plus the small set of in-flight requests. Scans and decoding stay off the UI thread. Animated preview files show a still frame rather than continuously playing the whole library.
