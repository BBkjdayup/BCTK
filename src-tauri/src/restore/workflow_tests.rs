//! Restart tests use separate real SQLite databases and .tqb archives. Directory
//! moves model a process stopping at each boundary, without touching user data.
use super::*;

struct Fixture {
    directory: PathBuf,
    data_root: PathBuf,
    restore_root: PathBuf,
    operation: PathBuf,
    journal: ActiveRestoreJournal,
    original_uuid: String,
    original_hashes: Vec<String>,
    incoming_hashes: Vec<String>,
}

async fn seed(database: &Database, label: &str) {
    sqlx::query("INSERT INTO subjects(id,name,name_key,sort_order,created_at_ms,updated_at_ms) VALUES(?,?,?,0,1,1)")
        .bind(Uuid::now_v7().to_string()).bind(label).bind(label)
        .execute(database.pool()).await.unwrap();
    for name in ["resources", "templates"] {
        fs::write(
            database.paths().data_root().join(name).join("fixture.txt"),
            label,
        )
        .unwrap();
    }
}

fn component_hashes(root: &Path) -> Vec<String> {
    COMPONENTS
        .iter()
        .map(|name| hash_candidate_tree_blocking(&root.join(name)).unwrap())
        .collect()
}

impl Fixture {
    async fn new() -> Self {
        let directory =
            std::env::temp_dir().join(format!("tk-restore-restart-{}", Uuid::now_v7().simple()));
        fs::create_dir(&directory).unwrap();
        let source = Database::open(directory.join("source"), "0.1.101-test")
            .await
            .unwrap();
        seed(&source, "backup-content").await;
        let archive = directory.join("source.tqb");
        backups::create_manual_backup(&source, archive.clone(), "0.1.101-test")
            .await
            .unwrap();
        source.close().await;
        drop(source);

        let data_root = directory.join("live");
        let state = AppState::initialize(data_root.clone(), "0.1.101-test".into()).await;
        assert!(state.initialization_error().await.is_none());
        let database = state.database().await.unwrap();
        seed(&database, "original-content").await;
        let original_uuid = database.database_uuid().await.unwrap();
        let owned: Database = (*database).clone();
        drop(database);
        let plan = prepare_restore(&state, archive).await.unwrap();
        assert_ne!(plan.incoming_summary.database_uuid, original_uuid);
        schedule_restore(
            &state,
            &ScheduleRestoreRequestApi {
                restore_token: plan.restore_token.clone(),
                expected_archive_sha256_hex: plan.archive_inspection.archive_sha256_hex,
                confirmed_current_data_replacement: true,
            },
        )
        .await
        .unwrap();
        owned.close().await;
        drop(owned);
        drop(state);
        let restore_root = existing_restore_root(&data_root).unwrap().unwrap();
        let operation = checked_operation_dir(&restore_root, &plan.restore_token).unwrap();
        let journal = read_json(&restore_root.join("active.json")).unwrap();
        validate_active_restore(&data_root, &operation, &journal)
            .await
            .unwrap();
        let original_hashes = component_hashes(&data_root);
        let incoming_hashes = component_hashes(&operation.join("incoming"));
        Self {
            directory,
            data_root,
            restore_root,
            operation,
            journal,
            original_uuid,
            original_hashes,
            incoming_hashes,
        }
    }

    async fn switch(&self) {
        switch_operation(&self.data_root, &self.operation, &self.journal)
            .await
            .unwrap();
    }

    fn assert_original_unchanged(&self) {
        assert_eq!(component_hashes(&self.data_root), self.original_hashes);
    }

    fn assert_finished(&self, outcome: &str) {
        assert!(!self.restore_root.join("active.json").exists());
        let terminal: RestoreResultApi = read_json(&self.operation.join("terminal.json")).unwrap();
        let last = get_last_restore_result(&self.data_root).unwrap().unwrap();
        assert_eq!(terminal.operation_id, self.journal.operation_id);
        assert_eq!(terminal.outcome, outcome);
        assert_eq!(
            serde_json::to_value(terminal).unwrap(),
            serde_json::to_value(last).unwrap()
        );
    }

    async fn assert_database(&self, expected_uuid: &str, label: &str) {
        let state = crate::startup::initialize_data_root(
            self.data_root.clone(),
            "0.1.101-test".into(),
            &crate::startup::StartupProgress::default(),
        )
        .await;
        assert!(state.initialization_error().await.is_none());
        let database = state.database().await.unwrap();
        assert_eq!(database.database_uuid().await.unwrap(), expected_uuid);
        let names: Vec<String> = sqlx::query_scalar("SELECT name FROM subjects ORDER BY name")
            .fetch_all(database.pool())
            .await
            .unwrap();
        assert_eq!(names, vec![label]);
        database.verify_integrity().await.unwrap();
        database.close().await;
        drop(database);
        drop(state);
        for name in ["resources", "templates"] {
            assert_eq!(
                fs::read_to_string(self.data_root.join(name).join("fixture.txt")).unwrap(),
                label
            );
        }
    }

    async fn cleanup(self) {
        let path = fs::canonicalize(&self.directory).unwrap();
        assert_eq!(
            path.parent(),
            Some(fs::canonicalize(std::env::temp_dir()).unwrap().as_path())
        );
        assert!(
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("tk-restore-restart-")
        );
        for attempt in 0..30 {
            match fs::remove_dir_all(&path) {
                Ok(()) => return,
                Err(_) if attempt < 29 => tokio::time::sleep(Duration::from_millis(50)).await,
                Err(error) => panic!("cannot remove isolated restore fixture: {error}"),
            }
        }
    }
}

#[tokio::test]
async fn completed_rollback_without_terminal_recovers_original_data_on_restart() {
    let fixture = Fixture::new().await;
    fixture.switch().await;
    rollback_operation(&fixture.data_root, &fixture.operation).unwrap();
    fixture.assert_original_unchanged();
    assert_eq!(
        component_hashes(&fixture.operation.join("failed-new")),
        fixture.incoming_hashes
    );
    assert!(!fixture.operation.join("terminal.json").exists());
    assert!(
        COMPONENTS
            .iter()
            .all(|name| !fixture.operation.join("old").join(name).exists())
    );

    let result = apply_pending_restore(&fixture.data_root)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(result.outcome, "rolled_back");
    fixture.assert_original_unchanged();
    fixture.assert_finished("rolled_back");
    assert!(
        apply_pending_restore(&fixture.data_root)
            .await
            .unwrap()
            .is_none()
    );
    fixture
        .assert_database(&fixture.original_uuid, "original-content")
        .await;
    fixture.cleanup().await;
}

#[tokio::test]
async fn restart_after_each_switch_move_rolls_back_without_losing_either_tree() {
    for completed_moves in 1..=6 {
        let fixture = Fixture::new().await;
        let old = fixture.operation.join("old");
        fs::create_dir(&old).unwrap();
        let moves = COMPONENTS
            .iter()
            .map(|name| (fixture.data_root.join(name), old.join(name)))
            .chain(COMPONENTS.iter().map(|name| {
                (
                    fixture.operation.join("incoming").join(name),
                    fixture.data_root.join(name),
                )
            }));
        // Reach every legal cut point, including a gap with no live database.
        for (source, destination) in moves.take(completed_moves) {
            fs::rename(source, destination).unwrap();
        }
        let result = apply_pending_restore(&fixture.data_root)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            result.outcome, "rolled_back",
            "switch cut {completed_moves}"
        );
        fixture.assert_original_unchanged();
        fixture.assert_finished("rolled_back");
        for (index, name) in COMPONENTS.iter().enumerate() {
            let incoming = fixture.operation.join("incoming").join(name);
            let rejected = fixture.operation.join("failed-new").join(name);
            assert_ne!(incoming.exists(), rejected.exists());
            assert_eq!(
                hash_candidate_tree_blocking(if incoming.exists() {
                    &incoming
                } else {
                    &rejected
                })
                .unwrap(),
                fixture.incoming_hashes[index]
            );
        }
        fixture
            .assert_database(&fixture.original_uuid, "original-content")
            .await;
        fixture.cleanup().await;
    }
}

#[tokio::test]
async fn restart_after_each_rollback_move_finishes_the_same_rollback() {
    for completed_moves in 0..=6 {
        let fixture = Fixture::new().await;
        fixture.switch().await;
        let rejected = fixture.operation.join("failed-new");
        fs::create_dir(&rejected).unwrap();
        let moves = COMPONENTS.iter().rev().flat_map(|name| {
            [
                (fixture.data_root.join(name), rejected.join(name)),
                (
                    fixture.operation.join("old").join(name),
                    fixture.data_root.join(name),
                ),
            ]
        });
        for (source, destination) in moves.take(completed_moves) {
            fs::rename(source, destination).unwrap();
        }
        let result = apply_pending_restore(&fixture.data_root)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            result.outcome, "rolled_back",
            "rollback cut {completed_moves}"
        );
        fixture.assert_original_unchanged();
        assert_eq!(component_hashes(&rejected), fixture.incoming_hashes);
        fixture.assert_finished("rolled_back");
        assert!(
            apply_pending_restore(&fixture.data_root)
                .await
                .unwrap()
                .is_none()
        );
        fixture.cleanup().await;
    }
}

#[tokio::test]
async fn empty_rollback_staging_prevents_restarting_an_aborted_switch() {
    let fixture = Fixture::new().await;
    fs::create_dir(fixture.operation.join("old")).unwrap();
    // A switch can fail before its first rename. Rollback still records its
    // phase, although both old/ and failed-new/ then remain empty.
    rollback_operation(&fixture.data_root, &fixture.operation).unwrap();
    assert_eq!(
        fs::read_dir(fixture.operation.join("failed-new"))
            .unwrap()
            .count(),
        0
    );
    let result = apply_pending_restore(&fixture.data_root)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(result.outcome, "rolled_back");
    fixture.assert_original_unchanged();
    assert_eq!(
        component_hashes(&fixture.operation.join("incoming")),
        fixture.incoming_hashes
    );
    fixture.assert_finished("rolled_back");
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_scheduled_restore_with_no_moves_still_applies_the_backup() {
    for old_directory_created in [false, true] {
        let fixture = Fixture::new().await;
        if old_directory_created {
            fs::create_dir(fixture.operation.join("old")).unwrap();
        }
        let result = apply_pending_restore(&fixture.data_root)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(result.outcome, "success");
        assert_eq!(
            result.restored_database_uuid.as_deref(),
            Some(fixture.journal.source_database_uuid.as_str())
        );
        assert_eq!(
            component_hashes(&fixture.operation.join("old")),
            fixture.original_hashes
        );
        fixture.assert_finished("success");
        fixture
            .assert_database(&fixture.journal.source_database_uuid, "backup-content")
            .await;
        fixture.cleanup().await;
    }
}

#[tokio::test]
async fn terminal_result_survives_result_write_failure_and_repeated_startup() {
    for rolled_back in [false, true] {
        let fixture = Fixture::new().await;
        if rolled_back {
            fixture.switch().await;
            rollback_operation(&fixture.data_root, &fixture.operation).unwrap();
        }
        // Force result persistence to fail after terminal.json was committed.
        // Removing this empty fixture directory models the I/O fault clearing.
        let last_path = fixture.restore_root.join("last-result.json");
        fs::create_dir(&last_path).unwrap();
        let error = apply_pending_restore(&fixture.data_root).await.unwrap_err();
        assert_eq!(error.code, "RESTORE_STATE_INVALID");
        assert!(fixture.operation.join("terminal.json").is_file());
        assert!(fixture.restore_root.join("active.json").is_file());
        let before_retry = component_hashes(&fixture.data_root);
        fs::remove_dir(&last_path).unwrap();
        let result = apply_pending_restore(&fixture.data_root)
            .await
            .unwrap()
            .unwrap();
        let expected = if rolled_back {
            "rolled_back"
        } else {
            "success"
        };
        assert_eq!(result.outcome, expected);
        assert_eq!(component_hashes(&fixture.data_root), before_retry);
        fixture.assert_finished(expected);

        // A restart after terminal/last-result commit but before active.json
        // deletion must only repeat bookkeeping, never switch directories again.
        write_json_new(&fixture.restore_root.join("active.json"), &fixture.journal).unwrap();
        let repeated = apply_pending_restore(&fixture.data_root)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            serde_json::to_value(repeated).unwrap(),
            serde_json::to_value(result).unwrap()
        );
        assert_eq!(component_hashes(&fixture.data_root), before_retry);
        fixture.assert_finished(expected);
        if rolled_back {
            fixture.assert_original_unchanged();
        }
        fixture.cleanup().await;
    }
}

#[tokio::test]
async fn missing_original_component_keeps_database_closed_and_retry_can_finish() {
    let fixture = Fixture::new().await;
    fixture.switch().await;
    rollback_operation(&fixture.data_root, &fixture.operation).unwrap();
    let parked = fixture.directory.join("parked-templates");
    fs::rename(fixture.data_root.join("templates"), &parked).unwrap();
    let database_file = fixture.data_root.join("database/zhitiku.sqlite3");
    let before = fs::read(&database_file).unwrap();
    let state = crate::startup::initialize_data_root(
        fixture.data_root.clone(),
        "0.1.101-test".into(),
        &crate::startup::StartupProgress::default(),
    )
    .await;
    assert!(state.initialization_error().await.is_some());
    assert!(state.database().await.is_err());
    assert_eq!(fs::read(&database_file).unwrap(), before);
    assert!(!fixture.data_root.join("templates").exists());
    assert!(!fixture.operation.join("terminal.json").exists());
    assert!(fixture.restore_root.join("active.json").exists());

    fs::rename(&parked, fixture.data_root.join("templates")).unwrap();
    state.retry_initialize().await.unwrap();
    assert!(state.initialization_error().await.is_none());
    let database = state.database().await.unwrap();
    assert_eq!(
        database.database_uuid().await.unwrap(),
        fixture.original_uuid
    );
    database.close().await;
    drop(database);
    drop(state);
    fixture.assert_finished("rolled_back");
    fixture
        .assert_database(&fixture.original_uuid, "original-content")
        .await;
    fixture.cleanup().await;
}

#[tokio::test]
async fn invalid_rollback_marker_and_missing_retained_root_fail_closed() {
    for marker_is_file in [true, false] {
        let fixture = Fixture::new().await;
        fixture.switch().await;
        rollback_operation(&fixture.data_root, &fixture.operation).unwrap();
        if marker_is_file {
            fs::rename(
                fixture.operation.join("failed-new"),
                fixture.operation.join("parked-candidate"),
            )
            .unwrap();
            fs::write(fixture.operation.join("failed-new"), "not a directory").unwrap();
        } else {
            fs::remove_dir(fixture.operation.join("old")).unwrap();
        }
        let error = apply_pending_restore(&fixture.data_root).await.unwrap_err();
        assert_eq!(
            error.code,
            if marker_is_file {
                "RESTORE_DIRECTORY_INVALID"
            } else {
                "RESTORE_ROLLBACK_SOURCE_MISSING"
            }
        );
        fixture.assert_original_unchanged();
        assert!(!fixture.operation.join("terminal.json").exists());
        assert!(fixture.restore_root.join("active.json").exists());
        fixture.cleanup().await;
    }
}
