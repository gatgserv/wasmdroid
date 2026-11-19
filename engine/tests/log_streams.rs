use base64::{Engine as _, engine::general_purpose::STANDARD};
use std::io::Write;
use wasmdroid_engine::{
    EngineError,
    logs::live::{self, LogQuery, LogStream, LogWriter},
    session::EngineSession,
};

#[test]
fn live_records_are_binary_exact_ordered_paginated_and_survive_reopen() {
    let root = tempfile::tempdir().unwrap();
    let session = EngineSession::open(root.path()).unwrap();
    live::begin_run(&session, "app", "first").unwrap();
    let mut writer = LogWriter::open(&session, "app", "first").unwrap();
    let payloads = [
        b"\0\xff\x1b[31m".as_slice(),
        "notă\n".as_bytes(),
        b"stderr".as_slice(),
    ];
    for (index, payload) in payloads.iter().enumerate() {
        writer
            .append(
                if index == 2 {
                    LogStream::Stderr
                } else {
                    LogStream::Stdout
                },
                payload,
            )
            .unwrap();
    }
    let first = live::read(
        &session,
        "app",
        &LogQuery {
            limit: 2,
            ..Default::default()
        },
    )
    .unwrap()
    .unwrap();
    assert_eq!(first.records.len(), 2);
    assert_eq!(first.next_sequence, 2);
    assert!(!first.incomplete_tail);
    assert_eq!(
        STANDARD.decode(&first.records[0].data_base64).unwrap(),
        payloads[0]
    );
    let rest = live::read(
        &session,
        "app",
        &LogQuery {
            after: 2,
            ..Default::default()
        },
    )
    .unwrap()
    .unwrap();
    assert_eq!(rest.records.len(), 1);
    assert_eq!(rest.records[0].stream, LogStream::Stderr);
    assert_eq!(
        STANDARD.decode(&rest.records[0].data_base64).unwrap(),
        payloads[2]
    );
    assert!(matches!(
        live::begin_run(&session, "app", "second"),
        Err(EngineError::Conflict(_))
    ));
    drop(session);
    assert!(matches!(
        EngineSession::open(root.path()),
        Err(EngineError::Conflict(_))
    ));
    drop(writer);
    let session = EngineSession::open(root.path()).unwrap();
    assert_eq!(
        live::read(&session, "app", &LogQuery::default())
            .unwrap()
            .unwrap()
            .records
            .len(),
        3
    );
    assert!(LogWriter::open(&session, "app", "first").is_err());
    live::begin_run(&session, "app", "second").unwrap();
    let mut new_writer = LogWriter::open(&session, "app", "second").unwrap();
    new_writer
        .append(LogStream::Stdout, b"new generation")
        .unwrap();
    let latest = live::read(&session, "app", &LogQuery::default())
        .unwrap()
        .unwrap();
    assert_eq!(latest.generation, "second");
    assert_eq!(latest.records.len(), 1);
    assert_eq!(
        live::read(
            &session,
            "app",
            &LogQuery {
                generation: Some("first".into()),
                ..Default::default()
            }
        )
        .unwrap()
        .unwrap()
        .records
        .len(),
        3
    );
}
#[test]
fn rotation_is_bounded_and_lost_history_is_explicit() {
    let root = tempfile::tempdir().unwrap();
    let session = EngineSession::open(root.path()).unwrap();
    live::begin_run(&session, "app", "one").unwrap();
    let mut writer = LogWriter::open(&session, "app", "one").unwrap();
    for number in 1..=300u64 {
        let mut payload = vec![0xA5; 4096];
        payload[..8].copy_from_slice(&number.to_le_bytes());
        writer.append(LogStream::Stdout, &payload).unwrap();
    }
    let directory = root.path().join("logs/app/live/one");
    let segments = std::fs::read_dir(&directory)
        .unwrap()
        .map(|entry| entry.unwrap())
        .filter(|entry| entry.path().extension().is_some_and(|e| e == "segment"))
        .collect::<Vec<_>>();
    assert!(segments.len() <= 4);
    assert!(
        segments
            .iter()
            .map(|e| e.metadata().unwrap().len())
            .sum::<u64>()
            <= 1024 * 1024
    );
    let mut cursor = 0;
    let mut sequences = Vec::new();
    loop {
        let page = live::read(
            &session,
            "app",
            &LogQuery {
                after: cursor,
                ..Default::default()
            },
        )
        .unwrap()
        .unwrap();
        if cursor == 0 {
            assert!(page.gap);
            assert!(page.first_sequence.unwrap() > 1);
        }
        for record in &page.records {
            let data = STANDARD.decode(&record.data_base64).unwrap();
            assert_eq!(
                u64::from_le_bytes(data[..8].try_into().unwrap()),
                record.sequence
            );
            sequences.push(record.sequence);
        }
        if page.next_sequence == cursor {
            break;
        }
        cursor = page.next_sequence;
    }
    assert_eq!(sequences.last(), Some(&300));
    assert!(sequences.windows(2).all(|p| p[1] == p[0] + 1));
    let tail = live::read(
        &session,
        "app",
        &LogQuery {
            tail: Some(3),
            ..Default::default()
        },
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        tail.records.iter().map(|r| r.sequence).collect::<Vec<_>>(),
        [298, 299, 300]
    );
    let since = tail.records[0].timestamp_ms.saturating_add(1);
    let filtered = live::read(
        &session,
        "app",
        &LogQuery {
            since_ms: Some(since),
            tail: Some(3),
            ..Default::default()
        },
    )
    .unwrap()
    .unwrap();
    assert!(filtered.records.iter().all(|r| r.timestamp_ms >= since));
    assert_eq!(filtered.next_sequence, 300);
}
#[test]
fn crash_partial_frames_are_visible_but_never_returned_as_output() {
    let root = tempfile::tempdir().unwrap();
    let session = EngineSession::open(root.path()).unwrap();
    live::begin_run(&session, "app", "one").unwrap();
    let mut writer = LogWriter::open(&session, "app", "one").unwrap();
    writer.append(LogStream::Stdout, b"complete").unwrap();
    drop(writer);
    let path = root
        .path()
        .join("logs/app/live/one/00000000000000000001.segment");
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap();
    let mut header = [0u8; 24];
    header[..8].copy_from_slice(&2u64.to_le_bytes());
    header[16..20].copy_from_slice(&10u32.to_le_bytes());
    header[20] = 1;
    file.write_all(&header).unwrap();
    file.write_all(b"partial").unwrap();
    drop(file);
    let page = live::read(&session, "app", &LogQuery::default())
        .unwrap()
        .unwrap();
    assert!(page.incomplete_tail);
    assert_eq!(page.records.len(), 1);
    assert_eq!(page.next_sequence, 1);
    assert_eq!(
        STANDARD.decode(&page.records[0].data_base64).unwrap(),
        b"complete"
    );
}
#[test]
fn only_three_identified_runs_are_retained_and_unknown_files_are_preserved() {
    let root = tempfile::tempdir().unwrap();
    let session = EngineSession::open(root.path()).unwrap();
    for number in 1..=5 {
        let generation = format!("run-{number}");
        live::begin_run(&session, "app", &generation).unwrap();
        let mut writer = LogWriter::open(&session, "app", &generation).unwrap();
        writer.append(LogStream::Stdout, b"output").unwrap();
    }
    assert!(!root.path().join("logs/app/live/run-1").exists());
    assert!(!root.path().join("logs/app/live/run-2").exists());
    assert!(root.path().join("logs/app/live/run-3").is_dir());
    let unknown = root.path().join("logs/app/live/not-a-run");
    std::fs::create_dir(&unknown).unwrap();
    std::fs::write(unknown.join("saved"), b"preserve").unwrap();
    live::begin_run(&session, "app", "run-6").unwrap();
    assert_eq!(std::fs::read(unknown.join("saved")).unwrap(), b"preserve");
    assert!(
        live::read(
            &session,
            "app",
            &LogQuery {
                generation: Some("../../outside".into()),
                ..Default::default()
            }
        )
        .is_err()
    );
}
