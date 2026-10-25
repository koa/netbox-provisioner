use super::*;

/// A directory of its own under the system's temporary one, empty.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("saved-job-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

fn saved(device: u32) -> SavedJob {
    SavedJob {
        device: DeviceId(device),
        device_name: "lab-ap1".into(),
        site: Some(SiteId(3)),
        saved_at: SystemTime::UNIX_EPOCH + Duration::from_secs(1_790_000_000),
        journal_entry: None,
        work: SavedWork::Update {
            state: UpdateState::Rebooting,
            update: Box::new(SavedUpdate {
                credentials: Some("lab".into()),
                watchers: vec![("lab-np1".into(), Watching::default())],
                run: UpdateRun {
                    from: "7.23.8".into(),
                    target: "7.24.5".into(),
                    newest_v6: "7.24.5".into(),
                    wanted: vec!["wifi-qcom".into()],
                    partition: "part0".into(),
                    wait: Duration::from_secs(1500),
                    ospf: ["default-v2 192.168.100.1".into()].into(),
                    expected: Some("7.24.5".into()),
                    failed: None,
                },
            }),
        },
    }
}

/// What is written is read back as it was, a write replacing the one before; a write that
/// didn't end (its temporary file) is removed, other files are left alone.
#[test]
fn a_saved_job_is_read_back() {
    let jobs = scratch("read");
    write(&jobs, &saved(7)).expect("write");
    let mut again = saved(7);
    again.work = SavedWork::Apply {
        state: ApplyState::StoppingFallback,
        apply: Box::new(SavedApply {
            credentials: Some("lab".into()),
            script: ScriptRun {
                run: "a1b2c3d4".into(),
                armed: true,
                armed_at: SystemTime::UNIX_EPOCH + Duration::from_secs(1_790_000_000),
                fires: SystemTime::UNIX_EPOCH + Duration::from_secs(1_790_000_210),
                unmarked: SystemTime::UNIX_EPOCH + Duration::from_secs(1_790_000_090),
                reprovision: vec!["/interface/wifi/radio".into()],
            },
            hold: Some(SavedHold {
                own: Some(Watching {
                    watch_address: Some("192.168.100.1".into()),
                    poe_pings: Vec::new(),
                }),
                others: vec![("lab-np1".into(), Watching::default())],
            }),
            batch: Some(SavedBatch {
                devices: vec![DeviceId(7), DeviceId(8)],
                delay: Duration::from_secs(265),
            }),
        }),
    };
    write(&jobs, &again).expect("write again");
    fs::write(jobs.join("8.json.tmp"), "{").expect("temporary");
    fs::write(jobs.join("notes.txt"), "kept").expect("other");
    let read = read_all(&jobs).expect("read");
    assert_eq!(read.len(), 1);
    assert_eq!(read[0].device, DeviceId(7));
    assert_eq!(read[0].saved, Ok(again));
    assert!(!jobs.join("8.json.tmp").exists());
    assert!(jobs.join("notes.txt").exists());
    let _ = fs::remove_dir_all(&jobs);
}

/// A file that isn't a saved job, or one saved for another device, waits as unreadable: it
/// is shown and discarded by hand, never taken up.
#[test]
fn an_unreadable_file_waits_as_such() {
    let jobs = scratch("unreadable");
    fs::write(jobs.join("5.json"), "{\"device\": 5}").expect("broken");
    write(&jobs, &saved(6)).expect("write");
    fs::rename(jobs.join("6.json"), jobs.join("9.json")).expect("rename");
    let read = read_all(&jobs).expect("read");
    let devices = read.iter().map(|waiting| (waiting.device.0, waiting.saved.is_err())).collect::<Vec<_>>();
    assert_eq!(devices, [(5, true), (9, true)]);
    let _ = fs::remove_dir_all(&jobs);
}

/// A second provisioner with the same `state_dir` doesn't get its lock.
#[test]
fn the_lock_is_held_once() {
    let dir = scratch("lock");
    let first = lock(&dir).expect("first");
    assert!(matches!(lock(&dir), Err(StoreError::Locked(_))));
    drop(first);
    assert!(lock(&dir).is_ok());
    let _ = fs::remove_dir_all(&dir);
}
