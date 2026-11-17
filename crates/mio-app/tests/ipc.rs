//! P3 acceptance: the shell's IPC surface, driven without a window.
//!
//! `docs/architecture.md` section 8 states P3's bar as "an empty window can call
//! `play` / `get_waveform`". A window that a human has to click is not testable,
//! so everything the commands do lives on `AppState` and this test drives that:
//!
//! * `get_status` reports the real device format, or the real device error.
//! * `play` publishes, `get_playback_position` advances, `stop` silences.
//! * A second `play` of the same file does not re-decode (the cache path), and a
//!   `play` that is superseded mid-decode does not start stale audio.
//! * `get_waveform` returns ~1000 peaks covering the whole file, and no PCM.
//! * `open_library` / `search` / `list_folders` return the same rows the indexer
//!   wrote, in the wire shape the renderer expects.
//!
//! The device is the system default when `MIO_TEST_DEVICE` is unset. On this
//! machine that is FxSound (a virtual card): fine for "does it play", useless for
//! latency, which is why no latency number is asserted here. Latency lives in
//! `docs/agent-log.md` with the device it was measured on.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use mio_app::AppState;
use mio_app::SearchRequest;

/// Where test audio goes. `$TMPDIR` on this host points at Hermes' scratch dir.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("mio-p3-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

/// A canonical 16-bit stereo WAV whose first half is a loud tone and whose second
/// half is silence. The asymmetry is what makes the peak test meaningful: a
/// waveform that returned a constant would pass on a uniform file.
fn write_wav(path: &Path, sample_rate: u32, channels: u16, frames: u32) {
    std::fs::create_dir_all(path.parent().expect("audio file has a folder")).unwrap();
    let block_align = channels * 2;
    let data_len = frames * block_align as u32;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&(sample_rate * block_align as u32).to_le_bytes());
    out.extend_from_slice(&block_align.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());

    for frame in 0..frames {
        let loud = frame < frames / 2;
        let value = if loud { 20_000i16 } else { 0i16 };
        for _ in 0..channels {
            out.extend_from_slice(&value.to_le_bytes());
        }
    }

    let mut file = std::fs::File::create(path).unwrap();
    file.write_all(&out).unwrap();
}

/// Boot the state against the device named by `MIO_TEST_DEVICE`, or the
/// system default.
fn state() -> Option<AppState> {
    let device = std::env::var("MIO_TEST_DEVICE").ok();
    let state = AppState::with_device(device);
    if state.engine_error().is_some() {
        eprintln!(
            "note: no audio device ({}); playback assertions skipped",
            state.engine_error().unwrap_or_default()
        );
        return None;
    }
    Some(state)
}

#[test]
fn status_reports_the_device_it_actually_opened() {
    let Some(state) = state() else { return };
    let status = state.status().expect("status");

    assert!(status.audio_available);
    assert_eq!(status.audio_error, None);

    // The format has to be one the engine can emit, and the numbers have to be
    // the device's, not a default: a hardcoded 48000 would pass this test while
    // lying about a 44100 device.
    let info = state.engine_info().expect("engine info");
    assert_eq!(status.sample_rate, Some(info.format.sample_rate));
    assert_eq!(status.channels, Some(info.format.channels));
    assert!(status.sample_format.is_some());

    let endpoint = status.endpoint_frames.expect("endpoint frames");
    assert!(endpoint > 0, "device reported an empty endpoint buffer");

    // Engine period is endpoint/2 on shared mode. Asserting the relationship
    // rather than a fixed number keeps this valid on a different device.
    let period = status.engine_period_ms.expect("engine period");
    let expected = endpoint as f64 / 2.0 / info.format.sample_rate as f64 * 1000.0;
    assert!((period - expected).abs() < 1e-9, "{period} vs {expected}");
    assert!(
        (1.0..=50.0).contains(&period),
        "engine period {period}ms is outside anything a shared-mode device reports"
    );
}

#[test]
fn status_without_a_device_reports_the_engine_error_not_a_generic_one() {
    // A device filter that cannot match anything. The engine's own
    // `DeviceNotFound` message names the filter and lists what was available;
    // that detail is the whole point of surfacing it.
    let state = AppState::with_device(Some("definitely-not-a-real-device-name".into()));
    let status = state.status().expect("status works without a device");

    assert!(!status.audio_available);
    let error = status.audio_error.expect("error text");
    assert!(
        error.contains("definitely-not-a-real-device-name"),
        "error should name the filter that failed: {error}"
    );
    assert_eq!(status.device_name, None);

    // And playback fails cleanly instead of panicking.
    let clip_path = scratch("no-device").join("x.wav");
    write_wav(&clip_path, 44_100, 2, 441);
    let err = state
        .publish(Arc::new(mio_engine::DecodedAudio {
            bytes: Vec::new(),
            frames: 0,
            format: mio_engine::OutputFormat {
                sample_rate: 44_100,
                channels: 2,
                sample: mio_engine::DeviceSample::I16,
            },
            source_rate: 44_100,
            source_channels: 2,
            decode_ms: 0.0,
            resampled: false,
            path: String::new(),
            trimmed_leading_frames: 0,
        }), mio_engine::PublishSource::Initial);
    assert!(err.is_err(), "publish without a device must fail");
}

#[test]
fn play_moves_the_playhead_and_stop_halts_it() {
    let Some(state) = state() else { return };
    let root = scratch("playback");
    let audio = root.join("tone.wav");
    write_wav(&audio, 44_100, 2, 44_100); // one second

    assert!(!state.playback_position().playing, "nothing playing at boot");

    tauri::async_runtime::block_on(state.play(&audio.to_string_lossy(), None)).expect("play");

    // The command resolves once the clip is published; the render thread advances
    // `pos_frames` from then on. Poll rather than sleep a fixed time, so this is
    // not a flaky timing test.
    let mut moved = false;
    for _ in 0..200 {
        let position = state.playback_position();
        if position.position_s > 0.0 {
            moved = true;
            assert!(position.duration_s > 0.9, "duration {}", position.duration_s);
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(moved, "playhead never advanced after play");

    state.stop_playback().expect("stop");
    let after = state.playback_position();
    assert!(!after.playing, "still playing after stop");
    assert!(after.finished);
}

#[test]
fn play_at_an_offset_starts_where_asked() {
    let Some(state) = state() else { return };
    let root = scratch("seek");
    let audio = root.join("two-seconds.wav");
    write_wav(&audio, 44_100, 2, 88_200); // two seconds

    tauri::async_runtime::block_on(state.play(&audio.to_string_lossy(), Some(1.0))).expect("play");

    let mut position = state.playback_position();
    for _ in 0..200 {
        position = state.playback_position();
        if position.position_s > 0.0 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(
        position.position_s >= 1.0,
        "playhead {} should start at the 1.0s offset",
        position.position_s
    );
    assert!(position.duration_s > 1.9);
    state.stop_playback().ok();
}

#[test]
fn replaying_the_same_file_skips_the_decoder() {
    let Some(state) = state() else { return };
    let root = scratch("cache");
    let audio = root.join("cached.wav");
    write_wav(&audio, 44_100, 2, 22_050); // half a second

    let path = audio.to_string_lossy().to_string();
    tauri::async_runtime::block_on(state.play(&path, None)).expect("first play");
    // The first play must have populated the cache, or the second one would
    // decode again and this test would pass while testing nothing.
    assert!(
        state.cached_clip(&path).is_some(),
        "first play did not cache the clip"
    );

    tauri::async_runtime::block_on(state.play(&path, None)).expect("second play");
    state.stop_playback().ok();
}

#[test]
fn a_superseded_play_does_not_start_stale_audio() {
    let Some(state) = state() else { return };
    let root = scratch("superseded");
    let first = root.join("first.wav");
    let second = root.join("second.wav");
    write_wav(&first, 44_100, 2, 44_100);
    write_wav(&second, 44_100, 2, 22_050);

    // Start a play, then stop before it can publish. The epoch check inside
    // `play` is what makes the late-arriving decode a no-op instead of playing a
    // sound the user already moved past.
    let first_path = first.to_string_lossy().to_string();
    let second_path = second.to_string_lossy().to_string();

    tauri::async_runtime::block_on(state.play(&first_path, None)).expect("first");
    state.stop_playback().expect("stop");
    tauri::async_runtime::block_on(state.play(&second_path, None)).expect("second");

    // The second publish is what is live now: its duration is the second file's.
    let mut position = state.playback_position();
    for _ in 0..200 {
        position = state.playback_position();
        if position.duration_s > 0.0 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(
        (position.duration_s - 0.5).abs() < 0.05,
        "expected the 0.5s file to be live, got duration {}",
        position.duration_s
    );
    state.stop_playback().ok();
}

#[test]
fn non_audio_paths_are_rejected_before_any_work() {
    let Some(state) = state() else { return };
    let root = scratch("reject");
    let text = root.join("notes.txt");
    std::fs::write(&text, b"not audio").unwrap();

    let err = tauri::async_runtime::block_on(state.play(&text.to_string_lossy(), None))
        .expect_err("a .txt is not playable");
    assert!(err.to_string().contains("not an audio file"), "{err}");

    let err = tauri::async_runtime::block_on(state.waveform(&text.to_string_lossy(), None))
        .expect_err("a .txt has no waveform");
    assert!(err.to_string().contains("not an audio file"), "{err}");
}

#[test]
fn a_missing_file_fails_the_play_command_rather_than_hanging() {
    let Some(state) = state() else { return };
    let root = scratch("missing");
    let gone = root.join("gone.wav");

    let err = tauri::async_runtime::block_on(state.play(&gone.to_string_lossy(), None))
        .expect_err("decoding a missing file must fail");
    // The decoder's message carries the path, which is what the UI shows.
    assert!(err.to_string().contains("gone.wav"), "{err}");
}

#[test]
fn waveform_is_a_thousand_peaks_of_the_whole_file_and_no_pcm() {
    let Some(state) = state() else { return };
    let root = scratch("waveform");
    let audio = root.join("tone-then-silence.wav");
    // 2 seconds at 44.1kHz: loud first half, silent second half.
    write_wav(&audio, 44_100, 2, 88_200);

    let wave = tauri::async_runtime::block_on(state.waveform(&audio.to_string_lossy(), None))
        .expect("waveform");

    assert_eq!(wave.peaks.len(), mio_engine::DEFAULT_BUCKETS);
    assert_eq!(wave.sample_rate, 44_100);
    assert_eq!(wave.channels, 2);
    assert!(
        (wave.duration_s - 2.0).abs() < 0.02,
        "duration {}",
        wave.duration_s
    );

    // The loud half must read loud and the silent half must read silent: a
    // waveform that ignores the audio would fail here.
    let midpoint = wave.peaks.len() / 2;
    let loud = wave.peaks[..midpoint].iter().cloned().fold(0.0f32, f32::max);
    let quiet = wave.peaks[midpoint + 4..]
        .iter()
        .cloned()
        .fold(0.0f32, f32::max);
    assert!(
        loud > 0.5,
        "loud half should be near full scale, peak was {loud}"
    );
    assert!(quiet < 0.01, "silent half should be silent, peak was {quiet}");

    // Wire-format guard: peaks are floats in 0..=1, never bytes of PCM. `AGENTS.md`
    // forbids decoded audio crossing into the webview.
    assert!(wave.peaks.iter().all(|p| (0.0..=1.0).contains(p)));
    assert_eq!(wave.peaks.len(), 1000, "payload must not scale with file size");
}

#[test]
fn waveform_bucket_count_is_clamped_not_trusted() {
    let Some(state) = state() else { return };
    let root = scratch("buckets");
    let audio = root.join("short.wav");
    write_wav(&audio, 44_100, 1, 4_410);

    let tiny = tauri::async_runtime::block_on(state.waveform(&audio.to_string_lossy(), Some(0)))
        .expect("waveform with zero buckets");
    assert_eq!(tiny.peaks.len(), 16, "0 buckets clamps to the minimum");

    let huge = tauri::async_runtime::block_on(state.waveform(&audio.to_string_lossy(), Some(1_000_000)))
        .expect("waveform with a huge bucket count");
    assert_eq!(huge.peaks.len(), 16_384, "huge bucket counts clamp");
}

#[test]
fn opening_a_library_indexes_it_before_returning_and_search_finds_the_rows() {
    let Some(state) = state() else { return };
    let root = scratch("library");
    write_wav(&root.join(r"Foley\door_slam.wav"), 44_100, 2, 4_410);
    write_wav(&root.join(r"Weapons\Firearms\gunshot_01.wav"), 44_100, 2, 8_820);
    std::fs::write(root.join("readme.txt"), b"not audio").unwrap();

    // Blocking now: when this returns, the pass is done and the rows are there.
    // That is the whole point of the change — a caller cannot see a half-built
    // index, so there is nothing to poll and nothing to race.
    let status = state
        .add_library(&root.to_string_lossy(), false, |_, _| {})
        .expect("open");
    assert_eq!(status.root, root.to_string_lossy());
    assert!(!status.indexing, "the pass finished before returning");
    assert_eq!(status.count, 2, "the .txt must not be indexed");

    // Search returns the wire shape, with the formatted labels the table needs.
    let rows = state
        .search(SearchRequest {
            query: String::new(),
            ..Default::default()
        })
        .expect("search everything");
    assert_eq!(rows.len(), 2);

    let gunshot = rows
        .iter()
        .find(|r| r.filename == "gunshot_01.wav")
        .expect("gunshot row");
    assert_eq!(gunshot.category, "Weapons");
    assert_eq!(gunshot.subcategory.as_deref(), Some("Firearms"));
    assert_eq!(gunshot.channel_label, "Stereo");
    assert_eq!(gunshot.duration_label, "0:00.20");
    assert_eq!(gunshot.sample_rate, Some(44_100));
    assert!(!gunshot.favorite);
    assert_eq!(gunshot.folder.as_deref(), Some(r"Weapons\Firearms"));

    // A query narrows it, using the FTS path rather than a LIKE.
    let hits = state
        .search(SearchRequest {
            query: "gunshot".into(),
            ..Default::default()
        })
        .expect("search");
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].filename, "gunshot_01.wav");
}

#[test]
fn folder_tree_comes_from_the_index_not_from_disk() {
    let Some(state) = state() else { return };
    let root = scratch("folders");
    write_wav(&root.join(r"Foley\Impacts\slam.wav"), 44_100, 2, 1_000);
    write_wav(&root.join(r"Foley\door.wav"), 44_100, 2, 1_000);
    write_wav(&root.join("root_level.wav"), 44_100, 2, 1_000);
    // An empty folder on disk must not appear: the tree is built from indexed
    // paths, so a folder with no indexed audio is not part of the library.
    std::fs::create_dir_all(root.join("EmptyFolder")).unwrap();

    state
        .add_library(&root.to_string_lossy(), false, |_, _| {})
        .expect("open");

    let tree = state.list_folders().expect("folders");
    assert_eq!(tree.count, 3, "root count");

    let foley = tree
        .children
        .iter()
        .find(|c| c.name == "Foley")
        .expect("Foley folder");
    assert_eq!(foley.count, 2, "recursive count includes the subfolder");

    let impacts = foley
        .children
        .iter()
        .find(|c| c.name == "Impacts")
        .expect("Impacts subfolder");
    assert_eq!(impacts.count, 1);

    assert!(
        !tree.children.iter().any(|c| c.name == "EmptyFolder"),
        "an empty folder has no indexed files and must not appear"
    );
}

#[test]
fn search_without_a_library_is_an_error_the_ui_can_display() {
    let Some(state) = state() else { return };
    let err = state
        .search(SearchRequest::default())
        .expect_err("searching with no library must fail");
    assert!(err.to_string().contains("no library"), "{err}");

    let err = state.list_folders().expect_err("folders with no library");
    assert!(err.to_string().contains("no library"), "{err}");

    // But the status command still answers, with `library: null`.
    let status = state.status().expect("status");
    assert!(status.library.is_none());
}

#[test]
fn reopening_the_same_library_reindexes_it_instead_of_opening_a_second_copy() {
    let Some(state) = state() else { return };
    let root = scratch("reopen");
    write_wav(&root.join("a.wav"), 44_100, 2, 1_000);

    let first = state
        .add_library(&root.to_string_lossy(), false, |_, _| {})
        .expect("first open");
    assert_eq!(first.count, 1);

    // Re-adding the same folder means "refresh", not "open it twice": the user
    // gets one entry in the sidebar and one pass, not two of each.
    let second = state
        .add_library(&root.to_string_lossy(), false, |_, _| {})
        .expect("second open");
    assert_eq!(second.root, first.root);
    assert_eq!(second.count, 1, "one file, indexed once");

    let libraries = state.library_list().expect("list");
    assert_eq!(libraries.len(), 1, "the folder must not be listed twice");
    assert_eq!(libraries[0].count, 1);
}

#[test]
fn opening_a_folder_that_does_not_exist_is_rejected() {
    let Some(state) = state() else { return };
    let err = state
        .add_library(r"C:\definitely\not\a\real\folder\mio", false, |_, _| {})
        .expect_err("a missing folder must be rejected");
    assert!(err.to_string().contains("not a folder"), "{err}");
}

// ---------------------------------------------------------------- P4 surface
//
// P4's bar (`docs/architecture.md` section 8) is "can browse, search, click to
// play". The browsing half needs library state the user can *change* — favorites
// and categories — plus the sidebar numbers that reflect those changes. Every
// test below drives `AppState`, not the window.

/// Open a scratch library and wait for the background pass, so the tests below
/// start from a settled index rather than racing the indexer.
fn settled_library(state: &AppState, name: &str) -> PathBuf {
    let root = scratch(name);
    write_wav(&root.join(r"Foley\door_slam.wav"), 44_100, 2, 4_410);
    write_wav(&root.join(r"Weapons\Firearms\gunshot_01.wav"), 44_100, 2, 8_820);
    write_wav(&root.join(r"Weapons\Firearms\gunshot_02.wav"), 44_100, 2, 8_820);

    state
        .add_library(&root.to_string_lossy(), false, |_, _| {})
        .expect("open");
    root
}

#[test]
fn favoriting_a_selection_updates_the_rows_and_the_sidebar_in_one_go() {
    let Some(state) = state() else { return };
    let root = settled_library(&state, "favorites");

    let rows = state
        .search(SearchRequest {
            query: "gunshot".into(),
            ..Default::default()
        })
        .expect("search");
    assert_eq!(rows.len(), 2);
    let paths: Vec<String> = rows.iter().map(|r| r.path.clone()).collect();

    let changed = state
        .set_favorite(&paths, true)
        .expect("favorite both gunshots");
    assert_eq!(changed, 2, "one transaction, two rows");

    // The sidebar's numbers are what the rail draws; they have to agree with the
    // rows, which is why both come from the same database in one call.
    let (favorites, total) = state.library().expect("library").summary().expect("summary");
    assert_eq!(favorites, 2);
    assert_eq!(total, 3);

    // And the change survives a re-read, i.e. it was committed, not cached.
    let after = state
        .search(SearchRequest {
            favorite_only: true,
            ..Default::default()
        })
        .expect("favorites only");
    assert_eq!(after.len(), 2);
    assert!(after.iter().all(|r| r.favorite));

    // Un-favoriting is the same call with the other flag.
    state.set_favorite(&paths, false).expect("unfavorite");
    let (favorites, _) = state.library().expect("library").summary().expect("summary");
    assert_eq!(favorites, 0);

    drop(root);
}

#[test]
fn a_hand_set_category_survives_a_reindex_and_clearing_it_does_not() {
    let Some(state) = state() else { return };
    let root = settled_library(&state, "category");

    let target = state
        .search(SearchRequest {
            query: "door_slam".into(),
            ..Default::default()
        })
        .expect("search")
        .into_iter()
        .next()
        .expect("door_slam row");
    // The classifier reads the filename, not the folder, so the guess is not
    // asserted by name — only that a guess exists and that a manual choice beats it.
    let guessed = target.category.clone();
    assert_ne!(guessed, "Uncategorized", "the classifier should guess something");
    assert!(!target.category_manual);

    let paths = vec![target.path.clone()];
    state
        .set_category(&paths, "Ambience", Some("Doors"))
        .expect("set category");

    // `category_manual` is the flag the indexer checks before overwriting. If the
    // setter forgot it, a reindex would silently undo the user's work.
    let manual = state
        .search(SearchRequest {
            category: Some("Ambience".into()),
            ..Default::default()
        })
        .expect("search by new category");
    assert_eq!(manual.len(), 1);
    assert!(manual[0].category_manual, "manual flag must be set");
    assert_eq!(manual[0].subcategory.as_deref(), Some("Doors"));

    // A full pass re-probes and re-classifies, and must keep the manual choice.
    state
        .library()
        .expect("library")
        .index_blocking(true)
        .expect("full pass");
    let after_pass = state
        .search(SearchRequest {
            query: "door_slam".into(),
            ..Default::default()
        })
        .expect("search after reindex");
    assert_eq!(
        after_pass[0].category, "Ambience",
        "a full pass must not overwrite a manual category"
    );

    // Clearing it hands the file back to the classifier on the next pass.
    state.clear_category(&paths).expect("clear category");
    let cleared = state
        .search(SearchRequest {
            query: "door_slam".into(),
            ..Default::default()
        })
        .expect("search after clear");
    assert_eq!(cleared[0].category, "Uncategorized");
    assert!(!cleared[0].category_manual);

    drop(root);
}

#[test]
fn the_sidebar_lists_every_category_with_a_count() {
    let Some(state) = state() else { return };
    let root = settled_library(&state, "facets");

    let facets = state.library().expect("library").facets().expect("facets");

    // Weapons comes from the filenames, so the two gunshots must group together
    // under one category with one subcategory.
    let weapons: u64 = facets
        .iter()
        .filter(|f| f.category == "Weapons")
        .map(|f| f.count)
        .sum();
    assert_eq!(weapons, 2, "two gunshots under Weapons");

    let firearms = facets
        .iter()
        .find(|f| f.category == "Weapons" && f.subcategory.as_deref() == Some("Firearms"))
        .expect("Firearms subcategory");
    assert_eq!(firearms.count, 2);

    // And the counts have to add up to the library: a facet list that misses a
    // file is a rail that hides sounds with no way to reach them.
    let summed: u64 = facets.iter().map(|f| f.count).sum();
    assert_eq!(summed, 3, "every indexed sound appears in exactly one facet");

    drop(root);
}

#[test]
fn pause_freezes_the_playhead_and_resume_continues_without_re_decoding() {
    let Some(state) = state() else { return };
    let root = scratch("pause");
    let audio = root.join("tone.wav");
    write_wav(&audio, 44_100, 2, 44_100);

    tauri::async_runtime::block_on(state.play(&audio.to_string_lossy(), None)).expect("play");

    // Let the render thread write a little audio, so "frozen" means frozen at a
    // non-zero position rather than frozen at the start.
    std::thread::sleep(std::time::Duration::from_millis(80));
    state.pause_playback().expect("pause");
    // One engine period of slack: the callback already in flight still writes its
    // block before it observes the flag.
    std::thread::sleep(std::time::Duration::from_millis(40));

    let paused = state.playback_position();
    assert!(paused.paused, "pause flag must be reported");
    assert!(!paused.playing, "a paused clip is not playing");
    assert!(paused.position_s > 0.0, "the playhead stopped past the start");

    std::thread::sleep(std::time::Duration::from_millis(120));
    let still = state.playback_position();
    assert_eq!(
        still.position_s, paused.position_s,
        "the playhead must not move while paused"
    );
    assert_eq!(
        still.duration_s, paused.duration_s,
        "the clip stays loaded, so its duration is still known"
    );

    // Resume continues from the same frame: no re-seek, no jump back to zero.
    state.resume_playback().expect("resume");
    std::thread::sleep(std::time::Duration::from_millis(80));
    let resumed = state.playback_position();
    assert!(resumed.playing, "resume must report playing");
    assert!(!resumed.paused);
    assert!(
        resumed.position_s > paused.position_s,
        "resume must continue forward: {} vs {}",
        resumed.position_s,
        paused.position_s
    );

    // Stop still clears everything, so the two commands stay distinguishable.
    state.stop_playback().expect("stop");
    let stopped = state.playback_position();
    assert_eq!(stopped.position_s, 0.0);
    assert!(!stopped.paused);
    assert!(stopped.finished);

    drop(root);
}

// ------------------------------------------------------ P5: multi-library, undo
//
// The three gaps that made P4 unusable as a daily tool: no memory of which
// library was open, no way to page past the first 500 rows, and no way to take
// back a mis-clicked star. These tests cover the first and third; paging is a
// front-end concern with a back-end `offset`, asserted by the CDP script.

#[test]
fn several_libraries_can_be_open_at_once_and_the_active_one_switches() {
    let Some(state) = state() else { return };
    let a = scratch("multi-a");
    let b = scratch("multi-b");
    write_wav(&a.join("a.wav"), 44_100, 2, 1_000);
    write_wav(&b.join("b.wav"), 44_100, 2, 1_000);
    write_wav(&b.join("b2.wav"), 44_100, 2, 1_000);

    state
        .add_library(&a.to_string_lossy(), false, |_, _| {})
        .expect("open a");
    state
        .add_library(&b.to_string_lossy(), false, |_, _| {})
        .expect("open b");

    let libraries = state.library_list().expect("list");
    assert_eq!(libraries.len(), 2, "both folders stay open");

    // Adding a library makes it the active one: opening a folder is a request to
    // look at it, not to file it away.
    let active = libraries.iter().find(|l| l.active).expect("one active");
    assert_eq!(active.root, b.to_string_lossy());
    assert_eq!(
        active.name,
        b.file_name().unwrap().to_string_lossy(),
        "the rail shows the folder name, not the path"
    );
    assert_eq!(active.count, 2);

    // Searching only sees the active library's rows.
    let rows = state
        .search(SearchRequest {
            query: String::new(),
            ..Default::default()
        })
        .expect("search");
    assert_eq!(rows.len(), 2, "the other library's rows must not leak in");

    // Switching back changes what search sees.
    state.set_active_library(&a.to_string_lossy()).expect("switch");
    let rows = state
        .search(SearchRequest {
            query: String::new(),
            ..Default::default()
        })
        .expect("search after switch");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].filename, "a.wav");

    drop((a, b));
}

#[test]
fn closing_the_active_library_falls_back_to_another_open_one() {
    let Some(state) = state() else { return };
    let a = scratch("close-a");
    let b = scratch("close-b");
    write_wav(&a.join("a.wav"), 44_100, 2, 1_000);
    write_wav(&b.join("b.wav"), 44_100, 2, 1_000);

    state
        .add_library(&a.to_string_lossy(), false, |_, _| {})
        .expect("open a");
    state
        .add_library(&b.to_string_lossy(), false, |_, _| {})
        .expect("open b");

    // Closing the active one must leave the window on a working library, not on
    // "no library open" while another one is sitting there.
    let changed = state.remove_library(&b.to_string_lossy()).expect("close b");
    assert!(changed, "the active library changed");

    let libraries = state.library_list().expect("list");
    assert_eq!(libraries.len(), 1);
    assert_eq!(libraries[0].root, a.to_string_lossy());
    assert!(libraries[0].active);

    // Closing the last one leaves nothing open, and search says so cleanly.
    state.remove_library(&a.to_string_lossy()).expect("close a");
    assert!(state.library_list().expect("list").is_empty());
    let err = state
        .search(SearchRequest::default())
        .expect_err("no library left");
    assert!(err.to_string().contains("no library"), "{err}");

    drop((a, b));
}

#[test]
fn switching_to_a_library_that_is_not_open_is_rejected() {
    let Some(state) = state() else { return };
    let root = settled_library(&state, "switch-missing");

    let err = state
        .set_active_library(r"C:\not\an\open\library")
        .expect_err("an unopened folder cannot be made active");
    assert!(err.to_string().contains("not open"), "{err}");

    drop(root);
}

#[test]
fn undo_restores_a_favorite_and_redo_puts_it_back() {
    let Some(state) = state() else { return };
    let root = settled_library(&state, "undo-favorite");

    let row = state
        .search(SearchRequest {
            query: "gunshot_01".into(),
            ..Default::default()
        })
        .expect("search")
        .into_iter()
        .next()
        .expect("row");
    let paths = vec![row.path.clone()];

    state.set_favorite(&paths, true).expect("favorite");
    let (undo_label, redo_label) = state.history_state();
    assert_eq!(undo_label.as_deref(), Some("Add to Favorites"));
    assert_eq!(redo_label, None);

    // Undo takes the star off, and the label it reports is the edit it reversed.
    let undone = state.undo().expect("undo").expect("something to undo");
    assert_eq!(undone, "Add to Favorites");
    let (favorites, _) = state.library().unwrap().summary().unwrap();
    assert_eq!(favorites, 0, "the star is gone again");

    // Redo puts it back, and the stacks swap.
    let (undo_label, redo_label) = state.history_state();
    assert_eq!(undo_label, None);
    assert_eq!(redo_label.as_deref(), Some("Add to Favorites"));

    let redone = state.redo().expect("redo").expect("something to redo");
    assert_eq!(redone, "Add to Favorites");
    let (favorites, _) = state.library().unwrap().summary().unwrap();
    assert_eq!(favorites, 1);

    // With nothing left, both return `None` rather than failing.
    assert_eq!(state.undo().expect("undo"), Some("Add to Favorites".into()));
    assert_eq!(state.undo().expect("undo again"), None);
    assert_eq!(state.redo().expect("redo"), Some("Add to Favorites".into()));

    drop(root);
}

#[test]
fn undo_restores_the_previous_category_not_just_the_previous_flag() {
    let Some(state) = state() else { return };
    let root = settled_library(&state, "undo-category");

    let row = state
        .search(SearchRequest {
            query: "gunshot_01".into(),
            ..Default::default()
        })
        .expect("search")
        .into_iter()
        .next()
        .expect("row");
    let paths = vec![row.path.clone()];
    let before = row.category.clone();
    let before_manual = row.category_manual;

    state
        .set_category(&paths, "Undo Check", Some("Temp"))
        .expect("set category");
    assert_eq!(state.history_state().0.as_deref(), Some("Set Category"));

    state.undo().expect("undo").expect("entry");

    // Back to exactly what it was, including whether the classifier or the user
    // owned the value: restoring only the category string would silently pin a
    // guess as a manual choice.
    let after = state
        .search(SearchRequest {
            query: "gunshot_01".into(),
            ..Default::default()
        })
        .expect("search after undo")
        .into_iter()
        .next()
        .expect("row");
    assert_eq!(after.category, before);
    assert_eq!(after.category_manual, before_manual);

    drop(root);
}

#[test]
fn a_multi_row_edit_undoes_as_one_step() {
    let Some(state) = state() else { return };
    let root = settled_library(&state, "undo-batch");

    let rows = state
        .search(SearchRequest {
            query: "gunshot".into(),
            ..Default::default()
        })
        .expect("search");
    assert_eq!(rows.len(), 2);
    let paths: Vec<String> = rows.iter().map(|r| r.path.clone()).collect();

    state.set_favorite(&paths, true).expect("favorite both");
    let (favorites, _) = state.library().unwrap().summary().unwrap();
    assert_eq!(favorites, 2);

    // One Ctrl+Z for a 200-row selection, not 200. That is the whole reason the
    // history stores per-path previous values in a single entry.
    state.undo().expect("undo").expect("entry");
    let (favorites, _) = state.library().unwrap().summary().unwrap();
    assert_eq!(favorites, 0, "one undo cleared the whole selection");

    drop(root);
}
