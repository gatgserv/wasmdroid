use wasmdroid_engine::{
    logs::live::{self, LogWriter},
    session::EngineSession,
    terminal::{FRAME_JSON_LIMIT, FrameQuery, MAX_COLS, MAX_ROWS, Terminal, TerminalPage},
};

fn text(page: &TerminalPage) -> String {
    page.cells
        .iter()
        .map(|row| {
            row.iter()
                .filter(|c| c.4 != 0)
                .map(|c| c.0.as_str())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}
#[test]
fn terminal_applies_cursor_color_alternate_screen_and_split_unicode() {
    let mut terminal = Terminal::new(4, 20).unwrap();
    terminal.process(b"one\r\n\x1b[31mRED\x1b[m", false);
    let frame = terminal.page(&FrameQuery::default()).unwrap();
    assert!(text(&frame).contains("one\nRED"));
    assert_eq!(frame.cells[1][0].1, 1);
    terminal.process(b"\x1b[1;1H\x1b[2K", false);
    terminal.process("notă界".as_bytes().split_last().unwrap().1, false);
    terminal.process(&["notă界".as_bytes()["notă界".len() - 1]], false);
    let frame = terminal.page(&FrameQuery::default()).unwrap();
    assert!(text(&frame).starts_with("notă界"));
    assert_eq!(frame.cells[0][4].4, 2);
    assert_eq!(frame.cells[0][5].4, 0);
    terminal.process(b"\x1b[?1049hALT", false);
    let alternate = terminal.page(&FrameQuery::default()).unwrap();
    assert!(alternate.alternate_screen);
    assert!(text(&alternate).contains("ALT"));
    terminal.process(b"\x1b[?1049l", false);
    let restored = terminal.page(&FrameQuery::default()).unwrap();
    assert!(!restored.alternate_screen);
    assert!(text(&restored).starts_with("notă界"));
}
#[test]
fn immutable_frame_pages_survive_live_output_and_idle_frames_are_small() {
    let mut terminal = Terminal::new(40, 80).unwrap();
    terminal.process(b"original", true);
    let first = terminal.page(&FrameQuery::default()).unwrap();
    assert_eq!(first.cells.len(), 16);
    assert!(first.has_more);
    terminal.process(b"\x1b[20;1Hlater", false);
    let second = terminal
        .page(&FrameQuery {
            token: Some(first.token),
            start_row: 16,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(first.version, second.version);
    assert!(!text(&second).contains("later"));
    let fresh = terminal.page(&FrameQuery::default()).unwrap();
    assert!(fresh.version > first.version);
    assert!(
        terminal
            .page(&FrameQuery {
                token: Some(first.token),
                start_row: 16,
                ..Default::default()
            })
            .is_err()
    );
    let idle = terminal
        .page_json(&FrameQuery {
            known_version: Some(fresh.version),
            ..Default::default()
        })
        .unwrap();
    assert!(
        serde_json::from_str::<serde_json::Value>(&idle).unwrap()["unchanged"]
            .as_bool()
            .unwrap()
    );
    assert!(idle.len() < 128);
    terminal.resize(40, 80).unwrap();
    assert_eq!(
        terminal.page(&FrameQuery::default()).unwrap().version,
        fresh.version
    );
}
#[test]
fn replies_bounds_hostile_strings_and_control_modes_stay_virtual() {
    let mut terminal = Terminal::new(5, 40).unwrap();
    let replies = terminal.process(b"\x1b[3;7H\x1b[6n\x1b[5n\x1b[c\x1b[18t", false);
    assert_eq!(replies, b"\x1b[3;7R\x1b[0n\x1b[?1;2c\x1b[8;5;40t");
    assert!(terminal.process(b"\x1b]52;c;?\x07", false).is_empty());
    let mut hostile = b"\x1b]2;".to_vec();
    hostile.extend(vec![b'x'; 100_000]);
    hostile.extend(b"\x07\x1b[?2004h\x1b[?1hVISIBLE");
    assert!(terminal.process(&hostile, false).is_empty());
    let frame = terminal.page(&FrameQuery::default()).unwrap();
    assert!(frame.bracketed_paste && frame.application_cursor);
    assert!(text(&frame).contains("VISIBLE"));
    assert!(Terminal::new(0, 10).is_err());
    assert!(terminal.resize(MAX_ROWS + 1, MAX_COLS).is_err());
    terminal.resize(MAX_ROWS, MAX_COLS).unwrap();
    terminal.process("界\u{0301}".repeat(40000).as_bytes(), false);
    let first = terminal.page_json(&FrameQuery::default()).unwrap();
    assert!(first.len() <= FRAME_JSON_LIMIT);
    let page: TerminalPage = serde_json::from_str(&first).unwrap();
    page.validate().unwrap();
    assert!(page.scrollback_len <= 256);
}
#[test]
fn final_display_is_published_in_complete_bounded_pages_and_pruned_with_its_run() {
    let root = tempfile::tempdir().unwrap();
    let session = EngineSession::open(root.path()).unwrap();
    live::begin_run(&session, "app", "one").unwrap();
    assert!(
        live::read_terminal(&session, "app", "one", 0)
            .unwrap()
            .is_none()
    );
    let mut writer = LogWriter::open(&session, "app", "one").unwrap();
    let mut terminal = Terminal::new(33, 91).unwrap();
    terminal.process(b"\x1b[33;1HFINAL", false);
    let mut pages = Vec::new();
    let mut query = FrameQuery::default();
    loop {
        let page = terminal.page(&query).unwrap();
        query.token = Some(page.token);
        query.start_row = page.start_row + page.cells.len() as u16;
        let more = page.has_more;
        pages.push(page);
        if !more {
            break;
        }
    }
    writer.save_terminal(&pages).unwrap();
    let last: TerminalPage = serde_json::from_str(
        &live::read_terminal(&session, "app", "one", 32)
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(last.rows, 33);
    assert!(text(&last).starts_with("FINAL"));
    drop(writer);
    for generation in ["two", "three", "four"] {
        live::begin_run(&session, "app", generation).unwrap();
    }
    assert!(
        live::read_terminal(&session, "app", "one", 0)
            .unwrap()
            .is_none()
    );
}
