# Validation — 24 October 2026

## Passed

- `cargo test --locked --all-targets`: 31 tests passed (including Workshop validation, helper lifecycle, search input, bounded progress, thumbnail host restrictions, compatibility classification, library filters, CJK glyph coverage and headless GUI views).
- `cargo check --locked --no-default-features`: CLI-only build passed.
- `cargo clippy --locked --all-targets -- -D warnings`: passed without warnings.
- `cargo fmt --check`: passed.
- Optimized Linux x86_64 build 0.2.1 completed; GUI builds embed the licensed CJK fallback font. The installed executable loads its colocated Steamworks runtime through `$ORIGIN`, verified without `LD_LIBRARY_PATH`.
- Native GUI launched on the user's Hyprland session and its own rendered buffer was inspected.
- Read-only monitor discovery found `HDMI-A-1`, `DP-1` and `DP-2`.
- `run.sh doctor --json` selected the local portable v0.9.4 renderer and detected its extended flags.
- The renderer archive was SHA-256 verified; its `--help` completed successfully.
- The original local web fixture's metadata loads without a Steam installation.

## Workshop integration (0.2.0)

- Connected to the user's already-running native Steam session without password entry, cookie access, API keys or SteamCMD.
- Queried the current account's Workshop subscriptions; live item-link lookup returned the expected Wallpaper Engine item and its installed/subscribed state.
- Live search for `forest` with Scene, Nature and 1920 × 1080 filters returned 50 correctly tagged results on page 1 (497 total). Page 2 returned a different set of results.
- Downloaded the already-subscribed item `3556597122`, **KDH - Zoey (Fighting Style)**, about 1.9 MB, through the new backend with normal priority. Observed checking, subscribing, queued, installing and completed events. Steam reported the installed content directory; Wallweave's `inspect` subsequently loaded its packed scene with `staged=false`. No wallpaper was applied during this test.
- An unavailable item ID was rejected before subscription. Deterministic tests also reject foreign-app, banned and collection metadata.
- Captured and inspected the native Workshop UI with real forest search results and remote previews in the user's Hyprland session. Headless view checks cover 960 × 640, 1360 × 840 and 1920 × 1080, including long CJK titles and failed-download states.
- Helper tests verify SDK stdout filtering, command/result transport, startup errors, and termination/reaping on session drop. Normal browsing and the renderer supervisor remain separate processes.

Flatpak Steam connectivity, offline-to-online transitions during a transfer, and other desktop/platform combinations have not been live-tested. Closing the GUI retains subscriptions in Steam; a reopened Workshop can find them under My subscriptions, while the per-session queue is not persisted by Wallweave.

The supervisor tests use executable fixtures, not a real wallpaper engine. They verify process lifecycle, per-monitor pause/stop, startup-failure preservation, missing mounts, retained assignments and that an unrelated process is not signalled.

## Automatic performance pause (2026-10-27)

**Superseded safety finding:** the subsequent Hyprland process-suspension implementation caused system-wide audio dropouts when Discord gained focus. Passing process-stop/resume tests did not establish audio safety. That implementation has been removed; see the cooperative-pause validation below.

- All 61 tests passed, plus `cargo fmt --check`, `cargo clippy --locked --all-targets -- -D warnings` and `cargo check --locked --no-default-features`.
- Fake-renderer coverage checks supported flags for every mode, unsupported-renderer fallback, preview isolation, preference persistence without changing legacy configuration, unchanged-rule no-ops, preservation of properties/manual pause and keeping old processes after a failed replacement.
- Captured and visually inspected the native Settings view with the new Performance control. The release binary and Steam runtime were installed using `scripts/build-app.py`.
- Restored the existing three wallpapers under the updated supervisor in Hyprland 0.56.2 with bundled renderer v0.9.4. All three processes received the native focus/maximized flags and remained running without supervisor failures. Native logs recorded `Paused: windows` and `Resumed` during actual focus changes; read-only checks confirmed the focused output was paused.
- A temporary GTK window was opened normally and fullscreen, then closed. Its initial scripted monitor-placement check failed because the installed Hyprland no longer accepts the legacy dispatcher, so this is not a deterministic fullscreen/monitor test pass. The subsequent focus checks used live window/output data without moving user windows. Actual game performance, GPU savings and equivalent behavior on KDE/GNOME remain unverified.

## Hyprland special-workspace resume fix (2026-10-27)

- Super+M/S in the user's configuration toggle special workspaces rather than closing their applications. Hidden clients can retain mapped, visible and focus/fullscreen state. The initial native renderer-only integration did not validate workspace membership.
- Hyprland now uses a supervisor worker with bounded IPC reads and a two-second freshness limit. Automatic process-group pause is separate from manual pause. Competing renderer window rules are disabled; malformed or stale compositor replies release automatic pauses only.
- All 66 tests pass, including an IPC fixture that repeatedly shows/hides a special workspace while leaving stale focus/fullscreen client flags intact. It verifies the same owned PID resumes, actual process stop/continue state, preview/other-output isolation, retained manual pauses, detection failure/recovery and disabling automatic pause. Formatting, Clippy with warnings denied and the CLI-only check also pass.
- Built and installed the release app and restored the three existing wallpapers with workspace-aware detection. Live testing on DP-1 verified show → pause, six seconds stopped, hide → resume and reopen → pause without changing the renderer PID. Subsequent CPU activity confirmed execution resumed. Closing the test window was not a clean standalone live check: Hyprland had automatically grouped it with existing windows, which remained on the special workspace. The affected ChatGPT/Zen group was returned to workspace 2; closed-window behavior is covered by deterministic tests. No further live window manipulation was used.
- Captured and inspected the updated Settings view. Real-game benchmarks and other compositor sessions remain outside this check.

## Cooperative pause and audio regression fix (2026-10-27)

- Removed process suspension from manual and automatic pause. The old SIGSTOP path froze the renderer's audio threads and was associated with system-wide audio loss when Discord gained focus. Auto-pause now defaults to off and existing saved preferences are preserved.
- Added a small GPL-3.0 adapter patch against upstream `a239861367cb8d338ef8ba346f937067c3a266d8`. `--pause-control-file` feeds the renderer's existing internal pause loop. Audio/event threads stay running; private atomic control files combine manual and automatic reasons. Renewal occurs every 500 ms; a missing/invalid/stale file releases external pause within about three seconds. A legacy renderer is never suspended as a fallback. Its manual pause is unavailable and Hyprland automatic pause stays off.
- All 73 Rust unit/workflow tests passed. The strengthened special-workspace test also verifies that manual Resume does not override an ongoing automatic pause. Renderer heartbeat checks replace the old tests that expected stopped processes. Additional tests cover unsupported renderers, separate private controls, renewal and cleanup. Formatting, Clippy with warnings denied and the CLI-only check passed.
- A standalone native C++ test checks valid pause/resume, invalid/oversized/missing data, stale renewal and a FIFO without blocking. The modified renderer was built and installed locally using the recorded patch. The installed runtime has no unresolved libraries or dependencies on temporary build paths.
- `scripts/check-pause-renderer.py` ran against both the build and installed runtime in separate Xvfb, D-Bus, PipeWire, Pulse and WirePlumber sessions. Three real video pause/resume cycles kept the same live process; captured frames stopped changing during pause and changed after resume. An independent 440 Hz stream remained present in every sampled 200 ms audio window through the cycles. Lease expiry resumed playback. The test uses a null sink and does not route test sound to the user's devices. Its initial run passed the media checks but exposed a private document-portal cleanup race; the final harness disables D-Bus service activation and exits cleanly.
- Installed the app and cooperative renderer, restarted the supervisor and restored exactly the three previously running wallpapers with unchanged assignments/properties. All three report `pause_supported: true`, `auto_pause: off`, no manual/automatic pause and no supervisor failures. The user's disabled auto-pause preference was retained. No user windows or workspaces were moved for this fix.
- Evidence is stored locally in `dist/cooperative-pause-installed-check/`: native logs, audio measurements and before/after desktop state. This verifies video pause and independent audio in isolation, not every wallpaper/audio backend or game-performance scenario. Automatic pause was deliberately left off on the live desktop; Discord focus was not retested there with the feature enabled.

### Re-enabled at the user's request (2026-10-27)

`config auto-pause focused` enabled the new control for all three desktop wallpapers. Read-only checks confirmed cooperative support on every output, an external pause on the focused display, continued playback on the other displays and no supervisor failures. All renderer processes remained live rather than signal-stopped. No user windows or workspaces were moved. The 73 Rust tests, formatting, Clippy and CLI-only check passed again before uploading the changes. The isolated audio regression results above remain the audio evidence; a live Discord call was not exercised by this check.

The updated portable archive and AppImage built locally and passed checksum, launch, diagnostics, isolated user install/uninstall, desktop-entry and AppImage supervisor-lifetime smoke tests. They contain the cooperative renderer build script/patch and the font license notices. These local packages use the host's library baseline and were not substituted for the existing public v0.2.4 release downloads.

## Release 0.2.5 preparation (2026-10-27)

- The versioned app passed all 73 Rust tests, formatting, Clippy and the CLI-only build. Four additional installer regressions cover repeatable upgrades, preserving the old runtime, checksum mismatch, unsupported pause capability and escaping archive links.
- The new portable app was installed over the actual published v0.2.4 archive in an isolated home. Configuration, wallpaper assignments/properties, the auto-pause preference and saved state remained byte-for-byte intact after upgrade and after removal. AppImage startup, cache reuse and background supervisor lifetime also passed.
- The cooperative renderer was built in Debian 12 from the pinned upstream revision plus the pause patch. Its standalone C++ pause-control test passed. Upstream portable packaging collected 183 libraries and verified its glibc requirement is no newer than 2.35. The release includes the separate renderer runtime and matching engine/dependency sources.
- The final renderer archive passed installation and library resolution on Ubuntu 22.04. Its real video completed three pause/resume cycles and lease expiry in isolated Xvfb/PipeWire/Pulse/WirePlumber sessions; every measured audio interval retained the independent 440 Hz signal. The test harness now handles WirePlumber 0.4's Lua hardware-monitor configuration as well as 0.5 profiles. Test infrastructure startup failures on the older session were corrected before the successful run; no user desktop/audio session was used.
- Installer regressions passed under Python 3.10 and 3.11 as well as the development host. The final archive also passed a real `--configure` upgrade and repeat installation with existing configuration/assignments preserved. Evidence is in the ignored local `dist/release-renderer-ubuntu-check/` directory.
- A pre-release GitHub run exposed a race in the fake renderer heartbeat: truncating writes could be observed as empty strings by both test reads. The fixture now replaces the heartbeat atomically, and the assertion waits up to two seconds for callback progress instead of assuming a fixed scheduling interval. All 73 tests passed after this test-only correction; the runtime pause implementation is unchanged.

## Not yet verified

- Full visual equivalence to the original Windows engine and every interactive feature. Short loading/rendering checks cover 44 supported projects; known renderer limitations are listed below.
- Any claim that previously broken JavaScript/SceneScript wallpapers are repaired.
- Audio behavior, fullscreen pause, suspend, monitor hotplug and GPU compatibility beyond the tested Hyprland/NVIDIA session.
- Actual KDE, GNOME or other compositor sessions.
- A full independent Rust scene renderer or independent SceneScript implementation; neither is included in 0.1.

The 0.1.4 tests temporarily applied renderers on DP-2 using isolated configuration/state. Test processes were then stopped, leaving the original shell backgrounds in place. No autostart, shell extension or system renderer package was changed. The portable renderer is stored inside the project and excluded from the source archive and Git.

## Wayland responsiveness fix (0.1.1)

The user's Hyprland "Application Not Responding" report happened directly after launcher startup. Ordinary visible-window launches initially did not reproduce it. Moving the 0.1.0 window to a hidden special workspace did reproduce a blocked event loop: Wayland pong responses stopped for 17 seconds and resumed in a burst when the window became visible. Another hidden interval produced a 21-second gap. The main thread changed from the normal event-loop epoll wait to a polling wait, while the protocol trace stopped after EGL frame traffic. A debugger attachment to the original user-started process was unavailable, so its exact stack was not captured.

0.1.1 disables blocking EGL VSync when `WAYLAND_DISPLAY` is set, following [glutin's documented warning about hidden Wayland windows](https://docs.rs/glutin/0.32.3/glutin/surface/enum.SwapInterval.html). Winit's compositor frame callbacks continue to pace redraws.

The installed `.desktop` entry was launched with protocol tracing for verification. Its window appeared after 0.23 seconds. In two hide/show cycles of 18 hidden seconds each, it sent 12 pong responses per hidden interval. The largest gap across all 32 responses was 1.50 seconds, matching the compositor's normal ping interval. Its own rendered UI buffer was inspected after the change. This reproduces and fixes the hidden-window failure mechanism; it does not establish that every possible cause of a startup stall has been eliminated.

Launcher starts now retain private startup output and panic backtraces in `gui.log`, with one previous launch retained. The final GUI was reopened without protocol tracing.

## Steam discovery and download status (0.1.2)

A read-only inspection of the mounted Steam library found 1,107 Workshop item records, 27 completed download counters, no installed Workshop records and an empty `workshop/content/431960`. Download files were still in `workshop/downloads/431960`; their presence alone is not evidence of completed installation. No staging files were copied, moved or modified.

The corrected scan discovers 19 installed built-in projects: 16 scenes, two web wallpapers and one unsupported Windows application. It reports the pending Workshop downloads in both JSON diagnostics and the GUI. The Library view refreshes in a background worker every ten seconds; pending downloads continue to be checked on other tabs. Native snapshots wait for the initial scan to complete.

Two new workflow tests cover Steam's transition from staged downloads to installed content, partially downloaded and zero-byte counters, built-in/local projects in custom installation folders, legacy metadata without a type, packed scene launch planning, and rejection of path traversal or escaping symlinks even when a package is present. Packed JSON entries retain their logical filename for the renderer. Real package rendering and script compatibility remain unverified.

## Direct playback of completed downloads (0.1.3)

The scan now makes 26 completed staging projects available alongside the 19 built-in projects: 14 staged scenes, 11 staged videos and one staged web project. The 27th completed item is a preset referring to base Workshop item 3528851083. Its base download still has zero completed bytes, and preset composition is not implemented by this host.

Steam's positive, completed byte counters and requested manifest revision gate staging eligibility. These checks run again at launch time. Partial downloads, zero-byte counters and completed data for superseded requested revisions do not qualify. Installed copies win when both locations are present, and a missing saved staging path resolves to its installed counterpart.

Workflow tests simulate Steam moving a playing staging item into its installed directory. Both a monitor renderer and a window preview restart from the new folder; property overrides and the monitor's paused state survive. The saved monitor assignment follows the new path, while previews remain unsaved. Existing process isolation and failed-startup replacement tests remain passing.

An isolated native preview of completed staging video 3692552963 used the pinned portable renderer and the user's available shared assets. The renderer remained alive for five seconds, mapped a 1280×720 preview window, and logged H.264 2560×1440 at 12 fps with NVDEC hardware decoding. The isolated supervisor and preview were then shut down. No desktop background was applied and no Steam content was changed. This was a process/window/decoder check, not a visual review of every frame or a scene/web compatibility test.

## CJK and native stability checks (0.1.4)

The GUI now embeds the unmodified Noto Sans CJK Regular collection, using its SC face as a fallback after the existing Latin fonts. A test checks real simplified/traditional Chinese, Japanese and Korean glyph availability in both proportional and monospace UI families. A native UI snapshot confirms that the previously missing Chinese wallpaper titles are legible. No system font installation is required for these UI glyphs.

All 44 locally available supported projects were checked at least once: 39 in preview windows, with the remaining five checked directly as backgrounds after the user asked to prioritize desktop placement. Thirty-eight of the 39 previews started and remained alive for five seconds; all eight background layer tests passed. The background surfaces were registered as `desktop`, layer 1 (bottom), at 1920×1080 on DP-2. Representative video, scene and web backgrounds passed pause/resume and stop checks.

Video and scene renderer captures contain nonuniform image data. The renderer's own web screenshot path produced a black image, so web output was additionally verified from the compositor on a temporary empty workspace. Both the original local web fixture and the completed staged web project produced nonblack rendered content. The prior workspace and focused monitor were restored after each capture.

The updated GUI passed two more 18-second hidden-window cycles while renderer testing was underway: 12 pong responses in each hidden interval, with a maximum response interval of 1.51 seconds. No Wallweave GUI crash or hang was observed. These are bounded smoke tests, not a claim of indefinite stability or complete script fidelity.

Known findings in the external v0.9.4 renderer/project data:

- `corsair_o_tron` does not load: legacy property values stored as strings trigger a numeric type error. Wallweave reports the failed startup and remains operational.
- `audiophile` starts but logs skipped legacy objects without IDs; its complete visual behavior is unverified.
- The staged web project logs a missing optional `resource/image/1.webp`; it nevertheless produces visible desktop output. The whole project was not compared against the Windows engine.
- The Windows application project and the preset requiring an unavailable base are outside the supported project set.

Temporary renderers, browser helpers and the isolated supervisor were checked after shutdown; no test processes or test desktop layers remained. The three original shell background surfaces were still present.

## Wallpaper library redesign (0.1.5)

The native library now uses a virtualized thumbnail grid with type, source and compatibility filters, search, title/type sorting and adjustable tile size. A persistent inspector separates display application from window preview. Active wallpaper pause/resume/stop controls are available beside the display. The interface and launcher icon use an anthracite/blue palette.

Native rendered buffers were inspected with the real library and with the two incompatible built-in projects. Thumbnails, Chinese titles, red badges, detailed reasons and disabled launch buttons render correctly. Unsupported formats are marked red; CORSAIR-O-Tron's reproduced metadata failure is classified only for the pinned v0.9.4 renderer and affected property values. Unverified scripts are not automatically classified as incompatible. Unit tests cover this distinction and combined filtering, plus populated views at 960×640, 1360×840 and 1920×1080.

Image decoding remains off the UI thread, limited to four concurrent workers, with bounded image dimensions, input/allocation limits and a 96-entry texture cache. GUI rendering was tested without applying or replacing the user's active wallpaper assignments.


## Staging cleanup regression and repair (0.2.1)

The 0.2.0 single-item SDK test exposed a Steam interaction missed by the initial validation: the single-item commit removed the old bulk staging tree. Steam retained counters for 557 completed items while only two Workshop items were installed, leaving 21 projects visible with the built-ins. This is content loss in the temporary download tree, not a grid rendering limit.

Two running wallpapers were preserved from their open file descriptors into the library's Wallweave-owned recovery directory; their project metadata was regenerated from the previous scan. No Btrfs subvolume snapshots were available. Remaining absent content requires downloading again. The app now distinguishes physical availability from Steam's historical counters and provides a saved recovery queue.

New regression tests simulate removal of the entire Steam staging tree and verify that preserved projects remain playable, old saved paths resolve, installed copies take precedence, unpublished copies are not exposed, stale counters report missing files, and incomplete/outdated staging is not preserved as completed. 31 tests, Clippy and the CLI-only check pass.

Live queries validated all five popularity windows, subscriptions with NSFW hidden, and NSFW inclusion. The forest query returned 11,269 results with the local exclusions and 11,695 with inclusion, subject to the current Steam account settings. Recovery is a real background download and is not represented as already completed.

## Display selection and recovery check (0.2.2)

The recovery queue continued advancing during the check on 24 October. The local scan rose from 71 to 79 available projects, while pending recovery jobs fell from 520 to 516. These are observations of ongoing recovery, not a claim that restoration is finished. A native Installed snapshot showed the available library count and project tiles.

The playback supervisor was still the running 0.1.4 executable. Applying the already selected Chainsaw Man Power video to DP-1 from its preserved cache succeeded, replaced that monitor's previous renderer, and left the other monitors running. Hyprland reported one Wallweave desktop surface per output. Subsequent user choices changed HDMI-A-1 successfully; the user confirmed the missing monitor selection caused the apparent failure. No supervisor restart was necessary.

The Apply button now names its output (for example, “Set on DP-2”), and Hyprland initially selects the focused monitor instead of always choosing the first connector. Background scans retain an explicit selection. All 31 existing unit/workflow tests passed.

## Animated monitor picker (0.2.3)

The startup picker was inspected in a native Wayland snapshot using the real three-screen Hyprland arrangement: DP-1 above HDMI-A-1 and DP-2. Cards retain the logical layout, with connector names and resolutions. The picker uses a 220 ms eased downward slide and fade, supports direct card selection, focused-card Enter activation, Escape/Close, and reopening from the navigation bar. Choosing a display does not send a playback command.

Five added tests cover Hyprland negative origins/fractional scaling/rotation, wlr current-mode parsing and unknown positions, arrangement fitting at two sizes, real card pointer activation and startup-only opening, and keyboard activation with motion disabled. All 36 tests passed; Clippy with warnings denied, formatting and the CLI-only build check passed. The release binary and runtime were installed atomically.

The updated GUI was reopened on its previous workspace 2. Its process reports 0.2.3. The previous GUI and Workshop helper exited; the renderer supervisor and all three playing wallpapers remained running. A saved queue backup was kept before reopening. The new helper resumed the same first queued item (2845897495), confirmed by Steam starting its download at 01:50:32. At 01:50:33 the library scan found 101 available projects and the saved recovery queue contained 505 pending items. Recovery remains ongoing.


## Per-wallpaper customization (24 October 2026)

The native Customize inspector was inspected on Wayland with Iuno -5- Wuthering Waves (color and boolean controls), Connecting Dots (color and combo controls), and Hate-chan (sliders and collapsible groups). The footer remains visible while the property list scrolls. The snapshots did not apply a wallpaper or change saved playback assignments.

All 48 unit/workflow tests passed. Seven property-editor tests cover author ordering and English localization, HTML stripping, dependent option visibility and comparison types, empty text and default normalization, restoration of saved assignments and per-wallpaper drafts across Steam moves, pointer activation of a checkbox and its Reset button, control widths, and the Apply button staying within 960 × 640 and 1360 × 840 viewports with over 100 options. Clippy with warnings denied, formatting and the CLI-only build check passed.

Display-condition support is limited to references, boolean operations, parentheses and comparisons; unknown expressions leave controls visible. Applying property changes restarts the renderer through the existing supervisor. Continuous live property updates and executing custom user shortcuts are outside this UI change.


## Quick apply and display assignments (24 October 2026)

Installed tiles now support double-click to apply to the selected display through the existing supervisor path, including property overrides and readiness/busy checks. A single click still only selects. The favorite star consumes both single and double clicks independently. The current-wallpaper banner, per-display picker list and tile badges distinguish Playing, Paused and Saved · not running. Live state takes precedence over saved assignments; previews are excluded. Display selection opens the associated wallpaper and reveals its tile if it matches the current filters, with Steam moves recognized without reading missing paths.

All 54 unit/workflow tests passed, along with Clippy with warnings denied, formatting and the CLI-only check. A headless pointer test sends a real tile double-click to a private mock supervisor socket and verifies exactly one Apply request for DP-2 with the expected properties. Other tests cover guard conditions, saved/runtime precedence, preview exclusion, display selection and draft retention, Steam path identity, picker titles/states, painted tile assignment labels and repeated star clicks.

Native Wayland snapshots with the real library verified the overview for three displays, both running wallpapers and a saved inactive assignment, the current-wallpaper banner, automatic tile reveal and the saved-display badge. These inspections did not apply or replace the user's desktop wallpapers.


## Automatic restore when opening Wallweave (24 October 2026)

Normal GUI startup now restores saved wallpaper assignments and their property overrides on connected outputs after discovery is ready. Startup work runs off the UI thread. The supervisor restores only idle outputs, preserves live/paused renderers, continues after an individual wallpaper fails and leaves unavailable assignments intact. A restore lock coordinates concurrent GUI starts. Older running supervisors remain usable through their existing Status/Apply protocol; GUI snapshots suppress restore entirely. No desktop login-autostart entry is installed.

All 58 unit/workflow tests passed, along with Clippy with warnings denied, formatting and the CLI-only check. The restart workflow shuts down a real test supervisor and fake renderer processes, starts a fresh supervisor, restores two saved outputs and verifies the saved property values in the renderer logs. Concurrent subsequent restores and direct supervisor restores retain the original PIDs and paused state. Additional tests cover offline content on one output while another restores, disconnected displays and previews being skipped, retained assignments, compatibility with an older supervisor and startup discovery/snapshot guards. The actual desktop session was not restarted for this validation.

## Game memory release — v0.2.6 (27 October 2026)

- All 80 unit/workflow tests passed, including Steam process markers, exiting/zombie processes, minimized or hidden Hyprland game windows, actual renderer termination/reaping, preview isolation, property and manual-pause restoration, delayed restoration, stop cancellation, unknown detection and retry after missing content. Formatting, Clippy with warnings denied and the CLI-only build check passed. The 30 GUI tests were repeated after the final status-summary change.
- Native settings snapshots were inspected on Hyprland with the new option enabled, both during desktop playback and while all three wallpapers were unloaded. The settings summary and footer report unloaded outputs consistently.
- Live lifecycle validation used the existing three desktop wallpapers and two temporary `sleep` processes carrying Steam game environment markers. It verified that all three renderer PIDs disappeared, that one remaining simulated game kept them unloaded, and that all three assignments restarted after both test processes exited. The saved configuration was byte-for-byte unchanged. GPU memory usage changed from 5,913 MiB to 3,347 MiB during this check. This result is specific to that desktop session; no game FPS benchmark was performed.
- The v0.2.5 renderer runtime and matching source archives are reused unchanged. Both were checked against the published v0.2.5 SHA-256 manifest. The v0.2.6 installer points to the new release's copies with the same pinned runtime digest.
- The release workflow builds the app on Ubuntu 22.04, checks upgrades from the published v0.2.5 portable archive and preserves both pause and gaming preferences. It also tests AppImage background supervision and builds/installs/removes the native Arch package before publication. These CI checks are publication gates; their actual results are recorded by GitHub Actions.
