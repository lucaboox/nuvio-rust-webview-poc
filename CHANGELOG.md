# Changelog

Release notes for Nuvio Desktop are maintained here and published to the matching GitHub Release by the release workflow.

## [Unreleased]

## [0.1.0-alpha.6] - 2026-09-06

- The interface is now the same one the web app uses, so a fix made in either place appears in both. Everything below comes with it.
- Added languages: the whole interface is translated into German, Spanish, French, Italian and Japanese, and follows your device by default.
- Added a person page: opening an actor shows what else they are in, sorted by popularity, with their upcoming work.
- Added RTX Video Super Resolution on NVIDIA hardware, under Playback. It uses the same setting as the official desktop client, and applies to the next video you start.
- Added a refresh button to the source list, so a debrid cache that fills or a release that appears can be picked up without leaving the sheet.
- Added subtitle and audio fallback languages, matching the official client's behaviour when nothing matches your first choice.
- Added picture modes to the player — Fit, Zoom, Fill and Stretch — and a next-episode button beside play.
- The home screen no longer waits for the slowest addon: rows appear as they answer, and one that stalls is left behind rather than holding up the rest.
- Episode ratings and MDBList no longer block the details page, and are cached for a day.
- Application updates work again, with the check moved into Settings under App version.
- Fixed a long list of interface faults: the loading screen no longer flickers through three separate screens at startup, the source sheet closes by swiping like every other panel, integrations open where you tapped them, and the settings controls are the same switches everywhere.

## [0.1.0-alpha.5] - 2026-08-15

- Added Dismiss to the Continue Watching menu, which clears a title the same way the mobile app does.
- Added the show password toggle back to sign-in.
- Improved settings, which now apply immediately, save in the background, and pick up changes made on another device when the window regains focus.
- Matched Nuvio's audio and subtitle language lists exactly.
- Sped up startup by syncing only what changed in watch history, and stopped Continue Watching running short of titles.
- Fixed addons that answer with a redirect being dropped, which could leave a title with no sources at all.
- Fixed the source picker showing two scrollbars.

## [0.1.0-alpha.4] - 2026-08-14

- Added optional self-hosted Nuvio backend configuration at sign-in.
- Added download storage cleanup for empty show folders and orphaned artwork.
- Grouped trailers and extras using the video categories supplied by metadata addons.
- Added an in-app release history sourced from the bundled changelog with GitHub Release notes as a fallback.

## [0.1.0-alpha.2] - 2026-08-14

- Added managed downloads with configurable storage and offline artwork.
- Added person and creator browsing from title details.
- Added embedded trailer playback and expanded native player controls.
- Added RTX Video Super Resolution support on compatible NVIDIA systems.
- Improved catalog ordering, collections, source selection, and playback responsiveness.

## [0.1.0-alpha.1] - 2026-08-13

- Published the first installable Windows alpha with signed in-app updates.
- Added the initial React, Tauri, Rust, and direct-libmpv desktop architecture.
